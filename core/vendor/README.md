# Wire compatibility patches

Sources are unmodified registry snapshots except the changes described here. Licenses are
retained. Keep these versions pinned and port the hooks explicitly when upgrading.

quinn-proto 0.11.18: optional controller pacing rate independent of congestion window;
exact lost-packet callback; controller admission policy (Go Brutal permits one packet beyond cwnd);
opt-in quic-go transport parameter order/ACK capabilities; separate initial/max receive windows;
Go BBR profile gains and an explicit BBR pacing rate; quic-go CID limit even with
zero source CID; negotiated idle/PTO keepalive bounds for Go QUIC; with quic-go compatibility
nothing is sent once the idle deadline has passed unless a keep-alive is due (quic-go checks the
idle timeout before sending, while the Quinn driver transmits before handling timers).
Transport options to omit `max_datagram_frame_size` while still receiving datagrams and to assume a peer limit when the peer omits it (quic-go `OmitMaxDatagramFrameSize`, `AssumePeerMaxDatagramFrameSize`).
Default behavior of other Quinn users is preserved.

h2 0.4.20: Go net/http and grpc-go request pseudo-header order selected by an explicit
request extension, preserved before h2 clears arbitrary extensions;
caller-provided PING payload with exact acknowledgement matching; receive window refresh
policies of x/net/http2 and grpc-go (`WindowRefresh`), where the grpc-go one returns the
connection window on receipt of DATA rather than on application reads, one WINDOW_UPDATE per
DATA frame that crosses a quarter of the window; connection WINDOW_UPDATEs queued ahead of the
next PING and SETTINGS, as grpc-go sends the unacknowledged credit before a BDP ping and only
the growth when the BDP estimator widens the window; that growth also resizes the automatic
small DATA frame budget, as `set_target_window_size` does upstream.

rustls 0.23.45: opt-in Go QUIC ClientHello extension contents/order (signature_algorithms_cert as in Go 1.26.5, without ML-DSA), including separate
TLS 1.3 handshake signature and certificate signature lists. Enabled by Hysteria,
XHTTP over HTTP/3 and explicit local DoQ. Groups and signatures are backed by real implementations in
the caller's provider. Default Rustls behavior is unchanged.
