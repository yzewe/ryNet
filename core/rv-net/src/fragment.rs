use ring::rand::{SecureRandom, SystemRandom};
use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time::Sleep;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FragmentConfig {
    pub packets: (u64, u64),
    pub lengths: Vec<(u64, u64)>,
    pub delays: Vec<(u64, u64)>,
    pub max_split: (u64, u64),
}

impl FragmentConfig {
    pub fn tls_hello(length: (u64, u64), interval_ms: (u64, u64)) -> Self {
        Self { packets: (0, 1), lengths: vec![length], delays: vec![interval_ms], max_split: (0, 0) }
    }

    fn length(&self, i: usize) -> (u64, u64) {
        self.lengths.get(i).or(self.lengths.last()).copied().unwrap_or((1, 1))
    }

    fn delay(&self, i: usize) -> (u64, u64) {
        self.delays.get(i).or(self.delays.last()).copied().unwrap_or((0, 0))
    }

    fn merge_hello(&self) -> bool {
        self.delays.len() == 1 && self.delays[0].1 == 0
    }
}

pub struct FragmentStream<S> {
    inner: S,
    cfg: FragmentConfig,
    count: u64,
    queue: VecDeque<(Vec<u8>, u64)>,
    offset: usize,
    sleep: Option<Pin<Box<Sleep>>>,
}

impl<S> FragmentStream<S> {
    pub fn new(inner: S, cfg: FragmentConfig) -> Self {
        Self { inner, cfg, count: 0, queue: VecDeque::new(), offset: 0, sleep: None }
    }
    pub fn get_ref(&self) -> &S {
        &self.inner
    }
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.inner
    }
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for FragmentStream<S> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for FragmentStream<S> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if !this.queue.is_empty() {
            match drain(this, cx) {
                Poll::Ready(Ok(())) => {}
                other => return other.map(|r| r.map(|_| 0)),
            }
        }
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let count = this.count + 1;
        let (from, to) = this.cfg.packets;
        let split = if (from, to) == (0, 1) {
            if count == 1 && is_client_hello(buf) { Some(hello_chunks(buf, &this.cfg)?) } else { None }
        } else if from == 0 || (from..=to).contains(&count) {
            Some(raw_chunks(buf, &this.cfg)?)
        } else {
            None
        };
        let Some(chunks) = split else {
            let r = Pin::new(&mut this.inner).poll_write(cx, buf);
            if r.is_ready() {
                this.count = count;
            }
            return r;
        };
        this.count = count;
        this.queue = chunks;
        this.offset = 0;
        match drain(this, cx) {
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            _ => Poll::Ready(Ok(buf.len())),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match drain(this, cx) {
            Poll::Ready(Ok(())) => Pin::new(&mut this.inner).poll_flush(cx),
            other => other,
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match drain(this, cx) {
            Poll::Ready(Ok(())) => Pin::new(&mut this.inner).poll_shutdown(cx),
            other => other,
        }
    }
}

fn random((min, max): (u64, u64)) -> io::Result<u64> {
    if max < min {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid fragment range"));
    }
    if min == max {
        return Ok(min);
    }
    let mut bytes = [0; 8];
    SystemRandom::new().fill(&mut bytes).map_err(|_| io::Error::other("random source failed"))?;
    let r = u64::from_be_bytes(bytes);
    Ok(match (max - min).checked_add(1) {
        Some(width) => min + r % width,
        None => r,
    })
}

fn is_client_hello(b: &[u8]) -> bool {
    b.len() > 5 && b[0] == 22 && b.len() >= 5 + u16::from_be_bytes([b[3], b[4]]) as usize && b.get(5) == Some(&1)
}

fn piece(cfg: &FragmentConfig, i: usize) -> io::Result<usize> {
    Ok(usize::try_from(random(cfg.length(i))?).unwrap_or(usize::MAX).max(1))
}

fn hello_chunks(b: &[u8], cfg: &FragmentConfig) -> io::Result<VecDeque<(Vec<u8>, u64)>> {
    let rec_len = 5 + u16::from_be_bytes([b[3], b[4]]) as usize;
    let data = &b[5..rec_len];
    let max_split = random(cfg.max_split)?;
    let merge = cfg.merge_hello();
    let mut out = VecDeque::new();
    let mut merged = Vec::new();
    let mut from = 0;
    let mut i = 0usize;
    while from < data.len() {
        let mut to = from.saturating_add(piece(cfg, i)?);
        if to > data.len() || (max_split > 0 && i as u64 + 1 >= max_split) {
            to = data.len();
        }
        let mut rec = Vec::with_capacity(5 + to - from);
        rec.extend_from_slice(&b[..3]);
        rec.extend_from_slice(&((to - from) as u16).to_be_bytes());
        rec.extend_from_slice(&data[from..to]);
        if merge {
            merged.extend_from_slice(&rec);
        } else {
            out.push_back((rec, random(cfg.delay(i))?));
        }
        from = to;
        i += 1;
    }
    if merge {
        out.push_back((merged, 0));
    }
    if b.len() > rec_len {
        out.push_back((b[rec_len..].to_vec(), 0));
    }
    cap_pauses(&mut out);
    Ok(out)
}

fn raw_chunks(b: &[u8], cfg: &FragmentConfig) -> io::Result<VecDeque<(Vec<u8>, u64)>> {
    let max_split = random(cfg.max_split)?;
    let mut out = VecDeque::new();
    let mut from = 0;
    let mut i = 0usize;
    while from < b.len() {
        let mut to = from.saturating_add(piece(cfg, i)?);
        if to > b.len() || (max_split > 0 && i as u64 + 1 >= max_split) {
            to = b.len();
        }
        out.push_back((b[from..to].to_vec(), random(cfg.delay(i))?));
        from = to;
        i += 1;
    }
    cap_pauses(&mut out);
    Ok(out)
}

const MAX_PAUSES_MS: u64 = 1000;

fn cap_pauses(q: &mut VecDeque<(Vec<u8>, u64)>) {
    let waited = q.len().saturating_sub(1);
    let total = q.iter().take(waited).fold(0u64, |sum, (_, d)| sum.saturating_add(*d));
    if total <= MAX_PAUSES_MS {
        return;
    }
    for (_, d) in q.iter_mut().take(waited) {
        *d = (u128::from(*d) * u128::from(MAX_PAUSES_MS) / u128::from(total)) as u64;
    }
}

fn drain<S: AsyncWrite + Unpin>(this: &mut FragmentStream<S>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    loop {
        if let Some(sleep) = &mut this.sleep {
            if sleep.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            this.sleep = None;
        }
        let Some((chunk, delay)) = this.queue.front() else { return Poll::Ready(Ok(())) };
        while this.offset < chunk.len() {
            match Pin::new(&mut this.inner).poll_write(cx, &chunk[this.offset..]) {
                Poll::Ready(Ok(0)) => return Poll::Ready(Err(io::Error::new(io::ErrorKind::WriteZero, "failed to write fragment"))),
                Poll::Ready(Ok(n)) => this.offset += n,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
        let delay = *delay;
        this.queue.pop_front();
        this.offset = 0;
        if delay > 0 && !this.queue.is_empty() {
            if let Poll::Ready(Err(e)) = Pin::new(&mut this.inner).poll_flush(cx) {
                return Poll::Ready(Err(e));
            }
            this.sleep = Some(Box::pin(tokio::time::sleep(Duration::from_millis(delay))));
        }
    }
}

#[cfg(kani)]
#[path = "proofs/fragment.rs"]
mod proofs;

#[cfg(test)]
mod tests {
    use super::*;

    fn hello(n: usize) -> Vec<u8> {
        let mut b = vec![22, 3, 1, (n >> 8) as u8, n as u8, 1];
        b.extend(std::iter::repeat_n(7u8, n - 1));
        b
    }

    #[test]
    fn hello_split_keeps_payload_and_respects_max_split() {
        let cfg = FragmentConfig { packets: (0, 1), lengths: vec![(10, 20)], delays: vec![(5, 5)], max_split: (3, 3) };
        let b = hello(200);
        let q = hello_chunks(&b, &cfg).unwrap();
        assert_eq!(q.len(), 3);
        let mut payload = Vec::new();
        for (rec, _) in &q {
            assert_eq!(&rec[..3], &b[..3]);
            let l = u16::from_be_bytes([rec[3], rec[4]]) as usize;
            assert_eq!(l, rec.len() - 5);
            payload.extend_from_slice(&rec[5..]);
        }
        assert_eq!(payload, b[5..]);
    }

    #[test]
    fn merged_hello_is_one_write() {
        let cfg = FragmentConfig::tls_hello((10, 20), (0, 0));
        let q = hello_chunks(&hello(100), &cfg).unwrap();
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn raw_split_covers_the_record() {
        let cfg = FragmentConfig { packets: (1, 3), lengths: vec![(50, 100)], delays: vec![(10, 20)], max_split: (0, 0) };
        let b: Vec<u8> = (0..1000u32).map(|i| i as u8).collect();
        let q = raw_chunks(&b, &cfg).unwrap();
        assert!(q.len() >= 10);
        assert_eq!(q.iter().flat_map(|(c, _)| c.iter().copied()).collect::<Vec<_>>(), b);
    }

    #[test]
    fn huge_ranges_do_not_abort() {
        let cfg = FragmentConfig { packets: (0, 0), lengths: vec![(100, u64::MAX)], delays: vec![(0, 0)], max_split: (0, u64::MAX) };
        let b: Vec<u8> = (0..1000u32).map(|i| i as u8).collect();
        for _ in 0..50 {
            let q = raw_chunks(&b, &cfg).unwrap();
            assert_eq!(q.iter().flat_map(|(c, _)| c.iter().copied()).collect::<Vec<_>>(), b);
        }
        assert!(random((0, u64::MAX)).is_ok());
    }

    #[test]
    fn length_past_usize_is_one_whole_piece() {
        let cfg = FragmentConfig { packets: (0, 1), lengths: vec![(1 << 32, 1 << 32)], delays: vec![(0, 0)], max_split: (0, 0) };
        assert!(piece(&cfg, 0).unwrap() >= 1);
        let b = hello(200);
        let q = hello_chunks(&b, &cfg).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(&q[0].0[5..], &b[5..]);
    }

    #[test]
    fn pauses_of_one_write_add_up_to_a_second_at_most() {
        let strong = FragmentConfig::tls_hello((5, 20), (20, 50));
        let b = hello(1800);
        for _ in 0..20 {
            let q = hello_chunks(&b, &strong).unwrap();
            assert!(q.len() >= 90, "{}", q.len());
            let waited: Vec<u64> = q.iter().take(q.len() - 1).map(|(_, d)| *d).collect();
            assert!(waited.iter().sum::<u64>() <= MAX_PAUSES_MS && waited.iter().all(|d| *d > 0), "{waited:?}");
            assert_eq!(q.iter().flat_map(|(r, _)| r[5..].iter().copied()).collect::<Vec<_>>(), b[5..]);
        }
        let raw = FragmentConfig { packets: (1, 1), lengths: vec![(1, 2)], delays: vec![(30, 30)], max_split: (0, 0) };
        let data = vec![9u8; 500];
        let q = raw_chunks(&data, &raw).unwrap();
        assert!(q.iter().map(|(_, d)| d).sum::<u64>() <= MAX_PAUSES_MS + 30);
        let light = FragmentConfig::tls_hello((100, 200), (10, 20));
        assert!(hello_chunks(&hello(600), &light).unwrap().iter().all(|(_, d)| (10..=20).contains(d)), "a light split keeps its pauses");
    }
}
