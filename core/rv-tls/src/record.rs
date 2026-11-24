use std::cell::RefCell;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::Error;
use crate::keysched::{Direction, TAG_LEN};

pub(crate) const CT_CCS: u8 = 20;
pub(crate) const CT_ALERT: u8 = 21;
pub(crate) const CT_HANDSHAKE: u8 = 22;
pub(crate) const CT_APPDATA: u8 = 23;

pub(crate) const MAX_PT: usize = 16384;
const MAX_CT: usize = MAX_PT + 2048;
const RBUF_CAP: usize = 2 * (5 + MAX_CT);
const WBUF_CAP: usize = 5 + MAX_PT + 1 + TAG_LEN;
const MAX_CCS: u8 = 16;
const POOL_MAX: usize = 4;

thread_local! {
    static RPOOL: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    static WPOOL: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
}

fn take_rbuf() -> Vec<u8> {
    RPOOL.with(|p| p.borrow_mut().pop()).unwrap_or_else(|| vec![0; RBUF_CAP])
}

fn give_rbuf(b: Vec<u8>) {
    if b.len() == RBUF_CAP {
        RPOOL.with(|p| {
            let mut p = p.borrow_mut();
            if p.len() < POOL_MAX {
                p.push(b)
            }
        });
    }
}

fn take_wbuf() -> Vec<u8> {
    WPOOL.with(|p| p.borrow_mut().pop()).unwrap_or_else(|| Vec::with_capacity(WBUF_CAP))
}

fn give_wbuf(mut b: Vec<u8>) {
    if (WBUF_CAP..=2 * WBUF_CAP).contains(&b.capacity()) {
        b.clear();
        WPOOL.with(|p| {
            let mut p = p.borrow_mut();
            if p.len() < POOL_MAX {
                p.push(b)
            }
        });
    }
}

pub(crate) struct Conn<S> {
    pub io: S,

    rbuf: Vec<u8>,
    rpos: usize,
    rend: usize,
    pt_start: usize,
    pt_end: usize,
    pub rx: Option<Direction>,
    pub want_ccs: bool,
    pub ccs_left: u8,
    pub hello_sent: bool,
    read_direct: bool,
    read_eof: bool,
    records: u64,
    post_hs: Vec<u8>,
    key_update_requested: bool,

    wbuf: Vec<u8>,
    wpos: usize,
    sealed_end: usize,
    open: bool,
    pub tx: Option<Direction>,
    write_direct: bool,
    write_closed: bool,
    app_records: usize,
    app_bytes: usize,
    open_limit: usize,
    pub record_limit: usize,
}

pub(crate) struct Record {
    pub ctype: u8,
    pub start: usize,
    pub end: usize,
}

impl<S> Conn<S> {
    pub fn new(io: S) -> Self {
        Self {
            io,
            rbuf: Vec::new(),
            rpos: 0,
            rend: 0,
            pt_start: 0,
            pt_end: 0,
            rx: None,
            want_ccs: false,
            ccs_left: MAX_CCS,
            hello_sent: false,
            read_direct: false,
            read_eof: false,
            records: 0,
            post_hs: Vec::new(),
            key_update_requested: false,
            wbuf: Vec::new(),
            wpos: 0,
            sealed_end: 0,
            open: false,
            tx: None,
            write_direct: false,
            write_closed: false,
            app_records: 0,
            app_bytes: 0,
            open_limit: MAX_PT,
            record_limit: MAX_PT,
        }
    }

    pub fn slice(&self, start: usize, end: usize) -> &[u8] {
        &self.rbuf[start..end]
    }

    pub fn is_read_direct(&self) -> bool {
        self.read_direct
    }
    pub fn is_write_direct(&self) -> bool {
        self.write_direct
    }

    pub fn switch_read_to_direct(&mut self) -> Vec<u8> {
        let pending = self.rbuf[self.pt_start..self.pt_end].to_vec();
        self.pt_start = 0;
        self.pt_end = 0;
        self.read_direct = true;
        self.rx = None;
        if self.rpos == self.rend {
            give_rbuf(std::mem::take(&mut self.rbuf));
            self.rpos = 0;
            self.rend = 0;
        }
        pending
    }

    pub fn switch_write_to_direct(&mut self) {
        self.seal_open();
        self.write_direct = true;
    }

    fn seal_open(&mut self) {
        if !self.open {
            return;
        }
        self.open = false;
        let start = self.sealed_end;
        let pt_len = self.wbuf.len() - start - 5;
        if pt_len == 0 {
            self.wbuf.truncate(start);
            return;
        }
        let Some(tx) = self.tx.as_mut() else {
            self.wbuf.truncate(start);
            return;
        };
        self.app_records = self.app_records.saturating_add(1);
        self.app_bytes = self.app_bytes.saturating_add(pt_len + 5 + tx.overhead());
        if tx.is_tls12() {
            let explicit = tx.explicit_nonce_len();
            self.wbuf.splice(start + 5..start + 5, std::iter::repeat_n(0, explicit));
            if tx.is_cbc() {
                tx.seal_cbc(&mut self.wbuf, start, CT_APPDATA);
                self.sealed_end = self.wbuf.len();
                return;
            }
        } else {
            self.wbuf.push(CT_APPDATA);
        }
        let len = (pt_len + tx.overhead()) as u16;
        let header = [CT_APPDATA, 3, 3, (len >> 8) as u8, len as u8];
        self.wbuf[start..start + 5].copy_from_slice(&header);
        let tag = tx.seal(&header, &mut self.wbuf[start + 5..]);
        self.wbuf.extend_from_slice(tag.as_ref());
        self.sealed_end = self.wbuf.len();
    }

    fn wbuf_ready(&mut self) {
        if self.wbuf.capacity() == 0 {
            self.wbuf = take_wbuf();
        }
    }

    pub fn queue(&mut self, ctype: u8, legacy_version: u16, data: &[u8]) {
        self.seal_open();
        self.wbuf_ready();
        let v = legacy_version.to_be_bytes();
        for chunk in data.chunks(self.record_limit) {
            let start = self.wbuf.len();
            match (&mut self.tx, ctype) {
                (Some(tx), t) if t != CT_CCS => {
                    let len = (chunk.len() + tx.overhead()) as u16;
                    let header = [if tx.is_tls12() { ctype } else { CT_APPDATA }, 3, 3, (len >> 8) as u8, len as u8];
                    self.wbuf.extend_from_slice(&header);
                    self.wbuf.resize(self.wbuf.len() + tx.explicit_nonce_len(), 0);
                    self.wbuf.extend_from_slice(chunk);
                    if tx.is_cbc() {
                        tx.seal_cbc(&mut self.wbuf, start, ctype);
                    } else {
                        if !tx.is_tls12() {
                            self.wbuf.push(ctype);
                        }
                        let tag = tx.seal(&header, &mut self.wbuf[start + 5..]);
                        self.wbuf.extend_from_slice(tag.as_ref());
                    }
                }
                _ => {
                    let len = chunk.len() as u16;
                    self.wbuf.extend_from_slice(&[ctype, v[0], v[1], (len >> 8) as u8, len as u8]);
                    self.wbuf.extend_from_slice(chunk);
                }
            }
            self.app_bytes = self.app_bytes.saturating_add(self.wbuf.len() - start);
        }
        self.sealed_end = self.wbuf.len();
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Conn<S> {
    pub fn poll_write_out(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.wpos < self.sealed_end {
            let Poll::Ready(n) = Pin::new(&mut self.io).poll_write(cx, &self.wbuf[self.wpos..self.sealed_end]) else {
                if self.wpos >= MAX_PT {
                    self.reclaim();
                }
                return Poll::Pending;
            };
            let n = n?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.wpos += n;
        }
        self.reclaim();
        Poll::Ready(Ok(()))
    }

    fn reclaim(&mut self) {
        self.wbuf.drain(..self.wpos);
        self.sealed_end -= self.wpos;
        self.wpos = 0;
        if self.wbuf.is_empty() && self.wbuf.capacity() != 0 {
            give_wbuf(std::mem::take(&mut self.wbuf));
        }
    }

    pub async fn flush_out(&mut self) -> io::Result<()> {
        std::future::poll_fn(|cx| {
            ready!(self.poll_write_out(cx))?;
            Pin::new(&mut self.io).poll_flush(cx)
        })
        .await
    }

    fn handle_key_update_reply(&mut self) {
        if self.key_update_requested && !self.write_direct && self.tx.is_some() {
            self.key_update_requested = false;
            self.queue(CT_HANDSHAKE, 0x0303, &[24, 0, 0, 1, 0]);
            if let Some(tx) = self.tx.as_mut() {
                tx.update();
            }
        }
    }

    pub fn poll_write_app(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        if self.write_closed && !buf.is_empty() {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if self.write_direct {
            ready!(self.poll_write_out(cx))?;
            if self.wbuf.capacity() != 0 && self.wbuf.is_empty() {
                give_wbuf(std::mem::take(&mut self.wbuf));
            }
            return Pin::new(&mut self.io).poll_write(cx, buf);
        }
        self.handle_key_update_reply();
        if self.wpos < self.sealed_end {
            match self.poll_write_out(cx) {
                Poll::Ready(r) => r?,
                Poll::Pending if self.sealed_end - self.wpos >= MAX_PT => return Poll::Pending,
                Poll::Pending => {}
            }
        }
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if self.tx.is_none() {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if !self.open {
            self.open_limit = if self.app_bytes >= 128 * 1024 {
                self.record_limit
            } else {
                (self.tx.as_ref().unwrap().go_payload(1208) * (self.app_records + 1).min(1001)).min(self.record_limit)
            };
            self.wbuf_ready();
            self.wbuf.extend_from_slice(&[0; 5]);
            self.open = true;
        }
        let used = self.wbuf.len() - self.sealed_end - 5;
        let n = buf.len().min(self.open_limit - used);
        self.wbuf.extend_from_slice(&buf[..n]);
        if used + n == self.open_limit {
            self.seal_open();
        }
        Poll::Ready(Ok(n))
    }

    pub fn poll_flush_app(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.handle_key_update_reply();
        self.seal_open();
        ready!(self.poll_write_out(cx))?;
        Pin::new(&mut self.io).poll_flush(cx)
    }

    pub fn poll_shutdown_app(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.write_closed && !self.write_direct {
            self.seal_open();
            ready!(self.poll_write_out(cx))?;
            ready!(Pin::new(&mut self.io).poll_flush(cx))?;
            self.write_closed = true;
            return Poll::Ready(Ok(()));
        }
        self.write_closed = true;
        if !self.write_direct && self.tx.is_some() {
            self.queue(CT_ALERT, 0x0303, &[1, 0]);
            self.tx = None;
        }
        ready!(self.poll_write_out(cx))?;
        Pin::new(&mut self.io).poll_shutdown(cx)
    }

    fn poll_fill(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<bool>> {
        if self.rbuf.is_empty() {
            self.rbuf = take_rbuf();
        }
        if self.rpos > 0 && self.pt_start == self.pt_end {
            self.rbuf.copy_within(self.rpos..self.rend, 0);
            self.rend -= self.rpos;
            self.rpos = 0;
            self.pt_start = 0;
            self.pt_end = 0;
        }
        if self.rend == self.rbuf.len() {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::InvalidData, "tls: record buffer overflow")));
        }
        let mut rb = ReadBuf::new(&mut self.rbuf[self.rend..]);
        let Poll::Ready(r) = Pin::new(&mut self.io).poll_read(cx, &mut rb) else {
            if self.rpos == self.rend && self.pt_start == self.pt_end {
                give_rbuf(std::mem::take(&mut self.rbuf));
                (self.rpos, self.rend, self.pt_start, self.pt_end) = (0, 0, 0, 0);
            }
            return Poll::Pending;
        };
        r?;
        let n = rb.filled().len();
        self.rend += n;
        Poll::Ready(Ok(n > 0))
    }

    pub fn poll_record(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<Record>, Error>> {
        loop {
            let avail = self.rend - self.rpos;
            if avail >= 5 {
                let h = &self.rbuf[self.rpos..self.rpos + 5];
                let (outer, len) = (h[0], u16::from_be_bytes([h[3], h[4]]) as usize);
                if self.rx.as_ref().is_some_and(Direction::is_tls12) && h[1..3] != [3, 3] {
                    return Poll::Ready(Err(Error::Protocol("TLS 1.2 record version")));
                }
                if len > MAX_CT {
                    return Poll::Ready(Err(Error::Record(format!("record too long: header {:02x?} after {} records", h, self.records))));
                }
                if avail >= 5 + len {
                    let header: [u8; 5] = self.rbuf[self.rpos..self.rpos + 5].try_into().unwrap();
                    let start = self.rpos + 5;
                    let end = start + len;
                    self.rpos = end;
                    if outer != CT_CCS && (outer == CT_APPDATA || self.rx.as_ref().is_some_and(Direction::is_tls12)) {
                        let rx = self.rx.as_mut().ok_or(Error::Protocol("encrypted record before keys"))?;
                        let records = self.records;
                        let n = rx
                            .open(&header, &mut self.rbuf[start..end])
                            .map_err(|_| Error::Record(format!("bad record mac: header {header:02x?} after {records} records")))?;
                        self.records += 1;
                        if rx.is_tls12() {
                            if n > MAX_PT {
                                return Poll::Ready(Err(Error::Protocol("TLS 1.2 plaintext record too long")));
                            }
                            let start = start + rx.explicit_nonce_len();
                            return Poll::Ready(Ok(Some(Record { ctype: outer, start, end: start + n })));
                        }
                        let body = &self.rbuf[start..start + n];
                        let Some(tpos) = body.iter().rposition(|&b| b != 0) else {
                            return Poll::Ready(Err(Error::Protocol("record without content type")));
                        };
                        return Poll::Ready(Ok(Some(Record { ctype: body[tpos], start, end: start + tpos })));
                    }
                    if outer == CT_CCS && !self.want_ccs {
                        if self.rx.as_ref().is_some_and(Direction::is_tls12) {
                            return Poll::Ready(Err(Error::Protocol("TLS 1.2 unexpected CCS")));
                        }
                        if self.ccs_left == 0 || self.rbuf[start..end] != [1] {
                            return Poll::Ready(Err(Error::Protocol("unexpected CCS")));
                        }
                        self.ccs_left -= 1;
                        continue;
                    }
                    if self.rx.is_some() {
                        return Poll::Ready(Err(Error::Protocol("unencrypted record after keys")));
                    }
                    return Poll::Ready(Ok(Some(Record { ctype: outer, start, end })));
                }
            }
            if !ready!(self.poll_fill(cx)).map_err(Error::Io)? {
                return Poll::Ready(if self.rend == self.rpos { Ok(None) } else { Err(Error::Protocol("truncated record")) });
            }
        }
    }

    pub fn poll_read_app(&mut self, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if self.read_direct {
            if self.rpos < self.rend {
                let n = (self.rend - self.rpos).min(buf.remaining());
                buf.put_slice(&self.rbuf[self.rpos..self.rpos + n]);
                self.rpos += n;
                if self.rpos == self.rend {
                    give_rbuf(std::mem::take(&mut self.rbuf));
                    self.rpos = 0;
                    self.rend = 0;
                }
                return Poll::Ready(Ok(()));
            }
            return Pin::new(&mut self.io).poll_read(cx, buf);
        }
        loop {
            if self.pt_start < self.pt_end {
                let n = (self.pt_end - self.pt_start).min(buf.remaining());
                buf.put_slice(&self.rbuf[self.pt_start..self.pt_start + n]);
                self.pt_start += n;
                return Poll::Ready(Ok(()));
            }
            if self.read_eof {
                return Poll::Ready(Ok(()));
            }
            let rec = match ready!(self.poll_record(cx)) {
                Ok(Some(r)) => r,
                Ok(None) => {
                    self.read_eof = true;
                    return Poll::Ready(Ok(()));
                }
                Err(e) => return Poll::Ready(Err(e.into())),
            };
            match rec.ctype {
                CT_APPDATA => {
                    self.pt_start = rec.start;
                    self.pt_end = rec.end;
                }
                CT_ALERT => {
                    let a = &self.rbuf[rec.start..rec.end];
                    if a.len() == 2 && a[1] == 0 {
                        self.read_eof = true;
                        return Poll::Ready(Ok(()));
                    }
                    return Poll::Ready(Err(Error::Alert(a.get(1).copied().unwrap_or(0)).into()));
                }
                CT_HANDSHAKE => {
                    let (s, e) = (rec.start, rec.end);
                    self.post_hs.extend_from_slice(&self.rbuf[s..e]);
                    self.process_post_handshake().map_err(io::Error::from)?;
                }
                _ => return Poll::Ready(Err(Error::Protocol("unexpected record type").into())),
            }
        }
    }

    fn process_post_handshake(&mut self) -> Result<(), Error> {
        let mut at = 0;
        while let Some(&[typ, a, b, c]) = self.post_hs.get(at..at + 4) {
            let len = u32::from_be_bytes([0, a, b, c]) as usize;
            let Some(body) = self.post_hs.get(at + 4..at + 4 + len) else {
                break;
            };
            match typ {
                4 => {}
                24 => {
                    if self.rx.as_ref().is_some_and(Direction::is_tls12) {
                        return Err(Error::Protocol("KeyUpdate in TLS 1.2"));
                    }
                    if len != 1 {
                        return Err(Error::Protocol("bad KeyUpdate"));
                    }
                    self.rx.as_mut().ok_or(Error::Protocol("KeyUpdate without keys"))?.update();
                    if body[0] == 1 {
                        self.key_update_requested = true;
                    }
                }
                _ => return Err(Error::Protocol("unexpected post-handshake message")),
            }
            at += 4 + len;
        }
        self.post_hs.drain(..at);
        if self.post_hs.len() > 1 << 16 {
            return Err(Error::Protocol("post-handshake message too large"));
        }
        Ok(())
    }
}

impl From<Error> for io::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::Io(e) => e,
            other => io::Error::new(io::ErrorKind::InvalidData, other),
        }
    }
}

#[cfg(kani)]
#[path = "proofs/record.rs"]
mod proofs;

#[cfg(fuzzing)]
#[path = "fuzzing/record.rs"]
pub mod fuzzing;

#[cfg(test)]
mod wire_tests {
    use super::*;
    use crate::keysched::{Secret, suite_by_id};

    #[tokio::test]
    async fn first_application_records_grow_like_go_tls() {
        let (io, mut peer) = tokio::io::duplex(128 * 1024);
        let mut c = Conn::new(io);
        c.tx = Some(Direction::new(suite_by_id(0x1301).unwrap(), Secret::zeros(32)));
        let data = vec![42; 32 * 1024];
        let mut offset = 0;
        while offset < data.len() {
            offset += std::future::poll_fn(|cx| c.poll_write_app(cx, &data[offset..])).await.unwrap();
        }
        std::future::poll_fn(|cx| c.poll_flush_app(cx)).await.unwrap();
        let mut got = vec![0; offset + 4096];
        use tokio::io::AsyncReadExt;
        let n = peer.read(&mut got).await.unwrap();
        let mut at = 0;
        let mut sizes = Vec::new();
        while at < n {
            let size = u16::from_be_bytes([got[at + 3], got[at + 4]]) as usize;
            sizes.push(size - 17);
            at += 5 + size;
        }
        assert_eq!(&sizes[..3], &[1186, 2372, 3558]);
        assert_eq!(sizes.iter().sum::<usize>(), data.len());
    }

    struct Trickle(bool);
    impl AsyncRead for Trickle {
        fn poll_read(self: Pin<&mut Self>, _: &mut Context<'_>, _: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
            Poll::Pending
        }
    }
    impl AsyncWrite for Trickle {
        fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
            self.0 = !self.0;
            if self.0 {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Poll::Ready(Ok(buf.len().min(4096)))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn partial_writes_do_not_pile_up_sent_bytes() {
        let mut c = Conn::new(Trickle(false));
        c.tx = Some(Direction::new(suite_by_id(0x1301).unwrap(), Secret::zeros(32)));
        let data = vec![42; 64 * 1024];
        let (mut sent, mut peak) = (0, 0);
        while sent < 8 << 20 {
            sent += std::future::poll_fn(|cx| c.poll_write_app(cx, &data)).await.unwrap();
            peak = peak.max(c.wbuf.capacity());
        }
        assert!(peak <= 4 * WBUF_CAP, "send buffer grew to {peak}");
        std::future::poll_fn(|cx| c.poll_flush_app(cx)).await.unwrap();
        assert!(c.wbuf.is_empty() && c.wbuf.capacity() <= 2 * WBUF_CAP);
    }

    #[tokio::test]
    async fn idle_connection_gives_its_buffers_back() {
        use tokio::io::AsyncWriteExt;
        let suite = suite_by_id(0x1301).unwrap();
        let (io, mut peer) = tokio::io::duplex(64 * 1024);
        let mut c = Conn::new(io);
        c.rx = Some(Direction::new(suite, Secret::zeros(32)));
        c.tx = Some(Direction::new(suite, Secret::zeros(32)));
        std::future::poll_fn(|cx| c.poll_write_app(cx, b"ping")).await.unwrap();
        std::future::poll_fn(|cx| c.poll_flush_app(cx)).await.unwrap();
        assert_eq!(c.wbuf.capacity(), 0, "send buffer kept after flush");
        let mut server = Direction::new(suite, Secret::zeros(32));
        let header = [CT_APPDATA, 3, 3, 0, 19];
        let mut body = vec![b'h', b'i', CT_APPDATA];
        let tag = server.seal(&header, &mut body);
        peer.write_all(&[header.to_vec(), body, tag.as_ref().to_vec()].concat()).await.unwrap();
        let mut buf = [0; 8];
        let mut rb = ReadBuf::new(&mut buf);
        std::future::poll_fn(|cx| c.poll_read_app(cx, &mut rb)).await.unwrap();
        assert_eq!(rb.filled(), b"hi");
        let mut rb = ReadBuf::new(&mut buf);
        assert!(std::future::poll_fn(|cx| Poll::Ready(c.poll_read_app(cx, &mut rb))).await.is_pending());
        assert_eq!(c.rbuf.capacity(), 0, "receive buffer kept while idle");
        let header = [CT_APPDATA, 3, 3, 0, 20];
        let mut body = vec![b'b', b'y', b'e', CT_APPDATA];
        let tag = server.seal(&header, &mut body);
        peer.write_all(&[header.to_vec(), body, tag.as_ref().to_vec()].concat()).await.unwrap();
        let mut rb = ReadBuf::new(&mut buf);
        std::future::poll_fn(|cx| c.poll_read_app(cx, &mut rb)).await.unwrap();
        assert_eq!(rb.filled(), b"bye");
    }

    #[test]
    fn post_handshake_messages_are_consumed_in_one_pass() {
        let suite = suite_by_id(0x1301).unwrap();
        let mut c = Conn::new(Trickle(false));
        c.rx = Some(Direction::new(suite, Secret::zeros(32)));
        c.post_hs = [4, 0, 0, 0].repeat(4000);
        c.post_hs.extend_from_slice(&[24, 0, 0, 1, 1, 4, 0]);
        c.process_post_handshake().unwrap();
        assert_eq!(c.post_hs, [4, 0]);
        assert!(c.key_update_requested);
        c.post_hs.extend_from_slice(&[0, 2, 7, 7]);
        c.process_post_handshake().unwrap();
        assert!(c.post_hs.is_empty());
        let mut server = Direction::new(suite, Secret::zeros(32));
        server.update();
        let header = [CT_APPDATA, 3, 3, 0, 18];
        let mut record = vec![1, CT_APPDATA];
        let tag = server.seal(&header, &mut record);
        record.extend_from_slice(tag.as_ref());
        assert_eq!(c.rx.as_mut().unwrap().open(&header, &mut record), Ok(2));
        c.post_hs.extend_from_slice(&[13, 0, 0, 0]);
        assert!(c.process_post_handshake().is_err());
    }

    #[tokio::test]
    async fn tls13_ccs_is_ignored_only_as_a_bounded_handshake_courtesy() {
        use tokio::io::AsyncWriteExt;
        let suite = suite_by_id(0x1301).unwrap();
        let ccs = vec![CT_CCS, 3, 3, 0, 1, 1];
        for (records, ccs_left, ok) in [
            (vec![ccs.clone(); MAX_CCS as usize], MAX_CCS, true),
            (vec![ccs.clone(); MAX_CCS as usize + 1], MAX_CCS, false),
            (vec![ccs.clone()], 0, false),
            (vec![vec![CT_CCS, 3, 3, 0, 1, 2]], MAX_CCS, false),
            (vec![vec![CT_CCS, 3, 3, 0, 2, 1, 1]], MAX_CCS, false),
        ] {
            let (io, mut peer) = tokio::io::duplex(4096);
            let mut c = Conn::new(io);
            c.rx = Some(Direction::new(suite, Secret::zeros(32)));
            c.ccs_left = ccs_left;
            let mut server = Direction::new(suite, Secret::zeros(32));
            let header = [CT_APPDATA, 3, 3, 0, 19];
            let mut body = vec![b'h', b'i', CT_APPDATA];
            let tag = server.seal(&header, &mut body);
            peer.write_all(&[records.concat(), header.to_vec(), body, tag.as_ref().to_vec()].concat()).await.unwrap();
            let mut buf = [0; 8];
            let mut rb = ReadBuf::new(&mut buf);
            let r = std::future::poll_fn(|cx| c.poll_read_app(cx, &mut rb)).await;
            assert_eq!(r.is_ok() && rb.filled() == b"hi", ok, "{} records, {ccs_left} allowed", records.len());
        }
    }

    #[tokio::test]
    async fn outgoing_records_respect_the_peer_record_size_limit() {
        use tokio::io::AsyncReadExt;
        let (io, mut peer) = tokio::io::duplex(1 << 20);
        let mut c = Conn::new(io);
        c.tx = Some(Direction::new(suite_by_id(0x1301).unwrap(), Secret::zeros(32)));
        c.record_limit = 63;
        c.queue(CT_HANDSHAKE, 0x0303, &[20; 100]);
        let data = vec![42; 300 * 1024];
        let mut offset = 0;
        while offset < data.len() {
            offset += std::future::poll_fn(|cx| c.poll_write_app(cx, &data[offset..])).await.unwrap();
        }
        std::future::poll_fn(|cx| c.poll_flush_app(cx)).await.unwrap();
        drop(c);
        let mut wire = Vec::new();
        peer.read_to_end(&mut wire).await.unwrap();
        let (mut at, mut total) = (0, 0);
        while at < wire.len() {
            let size = u16::from_be_bytes([wire[at + 3], wire[at + 4]]) as usize;
            assert!(size - 1 - TAG_LEN <= 63, "record of {size} bytes");
            total += size - 1 - TAG_LEN;
            at += 5 + size;
        }
        assert_eq!(total, 100 + data.len());
    }
}
