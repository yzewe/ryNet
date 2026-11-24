use bytes::Bytes;
use rand::RngExt;
use tracing::{debug, trace, trace_span};

use super::{Connection, SentFrames, spaces::SentPacket};
use crate::{
    ConnectionId, Instant, TransportError, TransportErrorCode,
    connection::ConnectionSide,
    frame::{self, Close},
    packet::{FIXED_BIT, Header, InitialHeader, LongType, PacketNumber, PartialEncode, SpaceId},
};

pub(super) struct PacketBuilder {
    pub(super) datagram_start: usize,
    pub(super) space: SpaceId,
    pub(super) partial_encode: PartialEncode,
    pub(super) ack_eliciting: bool,
    pub(super) exact_number: u64,
    pub(super) short_header: bool,
    /// Smallest absolute position in the associated buffer that must be occupied by this packet's
    /// frames
    pub(super) min_size: usize,
    /// Largest absolute position in the associated buffer that may be occupied by this packet's
    /// frames
    pub(super) max_size: usize,
    pub(super) tag_len: usize,
    pub(super) _span: tracing::span::EnteredSpan,
}

impl PacketBuilder {
    /// Write a new packet header to `buffer` and determine the packet's properties
    ///
    /// Marks the connection drained and returns `None` if the confidentiality limit would be
    /// violated.
    pub(super) fn new(
        now: Instant,
        space_id: SpaceId,
        dst_cid: ConnectionId,
        buffer: &mut Vec<u8>,
        buffer_capacity: usize,
        datagram_start: usize,
        ack_eliciting: bool,
        conn: &mut Connection,
    ) -> Option<Self> {
        let version = conn.version;
        // Initiate key update if we're approaching the confidentiality limit
        let sent_with_keys = conn.spaces[space_id].sent_with_keys;
        if space_id == SpaceId::Data {
            if sent_with_keys >= conn.key_phase_size {
                debug!("routine key update due to phase exhaustion");
                conn.force_key_update();
            }
        } else {
            let confidentiality_limit = conn.spaces[space_id]
                .crypto
                .as_ref()
                .map_or_else(
                    || &conn.zero_rtt_crypto.as_ref().unwrap().packet,
                    |keys| &keys.packet.local,
                )
                .confidentiality_limit();
            if sent_with_keys.saturating_add(1) == confidentiality_limit {
                // We still have time to attempt a graceful close
                conn.close_inner(
                    now,
                    Close::Connection(frame::ConnectionClose {
                        error_code: TransportErrorCode::AEAD_LIMIT_REACHED,
                        frame_type: None,
                        reason: Bytes::from_static(b"confidentiality limit reached"),
                    }),
                )
            } else if sent_with_keys > confidentiality_limit {
                // Confidentiality limited violated and there's nothing we can do
                conn.kill(
                    TransportError::AEAD_LIMIT_REACHED("confidentiality limit reached").into(),
                );
                return None;
            }
        }

        let space = &mut conn.spaces[space_id];
        let exact_number = match space_id {
            SpaceId::Data => conn.packet_number_filter.allocate(&mut conn.rng, space),
            _ => space.get_tx_number(),
        };

        let span = trace_span!("send", space = ?space_id, pn = exact_number).entered();

        let number = PacketNumber::for_quic_go(
            exact_number,
            space.largest_acked_packet,
            conn.endpoint_config.quic_go_compat,
        );
        let header = match space_id {
            SpaceId::Data if space.crypto.is_some() => Header::Short {
                dst_cid,
                number,
                spin: if conn.spin_enabled {
                    conn.spin
                } else {
                    conn.rng.random()
                },
                key_phase: conn.key_phase,
            },
            SpaceId::Data => Header::Long {
                ty: LongType::ZeroRtt,
                src_cid: conn.handshake_cid,
                dst_cid,
                number,
                version,
            },
            SpaceId::Handshake => Header::Long {
                ty: LongType::Handshake,
                src_cid: conn.handshake_cid,
                dst_cid,
                number,
                version,
            },
            SpaceId::Initial => Header::Initial(InitialHeader {
                src_cid: conn.handshake_cid,
                dst_cid,
                token: match &conn.side {
                    ConnectionSide::Client { token, .. } => token.clone(),
                    ConnectionSide::Server { .. } => Bytes::new(),
                },
                number,
                version,
            }),
        };
        let partial_encode = header.encode(buffer);
        if conn.peer_params.grease_quic_bit && conn.rng.random() {
            buffer[partial_encode.start] ^= FIXED_BIT;
        }

        let (sample_size, tag_len) = if let Some(ref crypto) = space.crypto {
            (
                crypto.header.local.sample_size(),
                crypto.packet.local.tag_len(),
            )
        } else if space_id == SpaceId::Data {
            let zero_rtt = conn.zero_rtt_crypto.as_ref().unwrap();
            (zero_rtt.header.sample_size(), zero_rtt.packet.tag_len())
        } else {
            unreachable!();
        };

        // Each packet must be large enough for header protection sampling, i.e. the combined
        // lengths of the encoded packet number and protected payload must be at least 4 bytes
        // longer than the sample required for header protection. Further, each packet should be at
        // least tag_len + 6 bytes larger than the destination CID on incoming packets so that the
        // peer may send stateless resets that are indistinguishable from regular traffic.

        // pn_len + payload_len + tag_len >= sample_size + 4
        // payload_len >= sample_size + 4 - pn_len - tag_len
        let min_size = Ord::max(
            buffer.len() + (sample_size + 4).saturating_sub(number.len() + tag_len),
            partial_encode.start + dst_cid.len() + 6,
        );
        let max_size = buffer_capacity - tag_len;
        debug_assert!(max_size >= min_size);

        Some(Self {
            datagram_start,
            space: space_id,
            partial_encode,
            exact_number,
            short_header: header.is_short(),
            min_size,
            max_size,
            tag_len,
            ack_eliciting,
            _span: span,
        })
    }

    /// Append the minimum amount of padding to the packet such that, after encryption, the
    /// enclosing datagram will occupy at least `min_size` bytes
    pub(super) fn pad_to(&mut self, min_size: u16) {
        // The datagram might already have a larger minimum size than the caller is requesting, if
        // e.g. we're coalescing packets and have populated more than `min_size` bytes with packets
        // already.
        self.min_size = Ord::max(
            self.min_size,
            self.datagram_start + (min_size as usize) - self.tag_len,
        );
    }

    pub(super) fn finish_and_track(
        self,
        now: Instant,
        conn: &mut Connection,
        sent: Option<SentFrames>,
        buffer: &mut Vec<u8>,
    ) {
        let ack_eliciting = self.ack_eliciting;
        let exact_number = self.exact_number;
        let space_id = self.space;
        let (size, padded) = self.finish(conn, now, buffer);
        let sent = match sent {
            Some(sent) => sent,
            None => return,
        };

        let size = match padded || ack_eliciting {
            true => size as u16,
            false => 0,
        };

        let packet = SentPacket {
            path_generation: conn.path.generation(),
            largest_acked: sent.largest_acked,
            time_sent: now,
            size,
            ack_eliciting,
            retransmits: sent.retransmits,
            stream_frames: sent.stream_frames,
        };

        conn.path
            .sent(exact_number, packet, &mut conn.spaces[space_id]);
        conn.stats.path.sent_packets += 1;
        conn.reset_keep_alive(now);
        if size != 0 {
            if ack_eliciting {
                conn.spaces[space_id].time_of_last_ack_eliciting_packet = Some(now);
                if conn.permit_idle_reset {
                    conn.reset_idle_timeout(now, space_id);
                }
                conn.permit_idle_reset = false;
            }
            conn.set_loss_detection_timer(now);
            conn.path.pacing.on_transmit(size);
        }
    }

    /// Encrypt packet, returning the length of the packet and whether padding was added
    pub(super) fn finish(
        self,
        conn: &mut Connection,
        now: Instant,
        buffer: &mut Vec<u8>,
    ) -> (usize, bool) {
        let pad = buffer.len() < self.min_size;
        let go_initial = self.space == SpaceId::Initial
            && conn.endpoint_config.quic_go_compat
            && conn.side.is_client();
        if go_initial {
            let start = self.partial_encode.start + self.partial_encode.header_len;
            let padding = self.min_size.saturating_sub(buffer.len());
            go_layout(buffer, start, padding, &mut conn.rng);
        } else if pad {
            trace!("PADDING * {}", self.min_size - buffer.len());
            buffer.resize(self.min_size, 0);
        }

        let space = &conn.spaces[self.space];
        let (header_crypto, packet_crypto) = if let Some(ref crypto) = space.crypto {
            (&*crypto.header.local, &*crypto.packet.local)
        } else if self.space == SpaceId::Data {
            let zero_rtt = conn.zero_rtt_crypto.as_ref().unwrap();
            (&*zero_rtt.header, &*zero_rtt.packet)
        } else {
            unreachable!("tried to send {:?} packet without keys", self.space);
        };

        debug_assert_eq!(
            packet_crypto.tag_len(),
            self.tag_len,
            "Mismatching crypto tag len"
        );

        buffer.resize(buffer.len() + packet_crypto.tag_len(), 0);
        let encode_start = self.partial_encode.start;
        let packet_buf = &mut buffer[encode_start..];
        self.partial_encode.finish(
            packet_buf,
            header_crypto,
            Some((self.exact_number, packet_crypto)),
        );

        let len = buffer.len() - encode_start;
        conn.config.qlog_sink.emit_packet_sent(
            self.exact_number,
            len,
            self.space,
            self.space == SpaceId::Data && conn.spaces[SpaceId::Data].crypto.is_none(),
            now,
            conn.orig_rem_cid,
        );

        (len, pad)
    }
}

/// Кадры клиентского Initial в порядке quic-go (`packetPacker.appendPacketPayload`): ACK, затем
/// весь PADDING, затем остальные кадры в случайном порядке. Полезная нагрузка — от `start` до
/// конца `buffer`; к ней добавляется `padding` нулей. Непонятный кадр — нули просто в конец.
fn go_layout(buffer: &mut Vec<u8>, start: usize, padding: usize, rng: &mut impl rand::Rng) {
    let Some((ack, zeros, mut frames)) = initial_frames(&buffer[start..]) else {
        buffer.resize(buffer.len() + padding, 0);
        return;
    };
    for i in (1..frames.len()).rev() {
        frames.swap(i, rng.random_range(0..=i));
    }
    let payload = &buffer[start..];
    let mut out = Vec::with_capacity(payload.len() + padding);
    if let Some(r) = ack {
        out.extend_from_slice(&payload[r]);
    }
    out.resize(out.len() + zeros + padding, 0);
    for r in frames {
        out.extend_from_slice(&payload[r]);
    }
    buffer.truncate(start);
    buffer.extend_from_slice(&out);
}

/// Границы кадров Initial: ACK, число байт PADDING и прочие кадры (PING, CRYPTO,
/// CONNECTION_CLOSE). `None` — встретился другой кадр или запись обрезана.
fn initial_frames(
    payload: &[u8],
) -> Option<(
    Option<std::ops::Range<usize>>,
    usize,
    Vec<std::ops::Range<usize>>,
)> {
    use crate::coding::BufExt;
    let mut r = std::io::Cursor::new(payload);
    let (mut ack, mut zeros, mut frames) = (None, 0, Vec::new());
    while (r.position() as usize) < payload.len() {
        let begin = r.position() as usize;
        let ty = r.get_var().ok()?;
        match ty {
            0x00 => {
                zeros += 1;
                continue;
            }
            0x01 => {}
            0x02 | 0x03 => {
                r.get_var().ok()?; // largest
                r.get_var().ok()?; // delay
                let ranges = r.get_var().ok()?;
                r.get_var().ok()?; // first range
                for _ in 0..ranges {
                    r.get_var().ok()?;
                    r.get_var().ok()?;
                }
                if ty == 0x03 {
                    for _ in 0..3 {
                        r.get_var().ok()?;
                    }
                }
            }
            0x06 => {
                r.get_var().ok()?; // offset
                let len = r.get_var().ok()? as usize;
                r.set_position(r.position().checked_add(len as u64)?);
            }
            0x1c | 0x1d => {
                r.get_var().ok()?; // error code
                if ty == 0x1c {
                    r.get_var().ok()?; // frame type
                }
                let len = r.get_var().ok()? as usize;
                r.set_position(r.position().checked_add(len as u64)?);
            }
            _ => return None,
        }
        let end = r.position() as usize;
        if end > payload.len() {
            return None;
        }
        if matches!(ty, 0x02 | 0x03) && ack.is_none() {
            ack = Some(begin..end);
        } else {
            frames.push(begin..end);
        }
    }
    Some((ack, zeros, frames))
}
