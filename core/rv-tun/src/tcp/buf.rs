#[cfg(not(kani))]
use std::cell::RefCell;
use std::collections::VecDeque;

#[cfg(not(kani))]
pub(crate) const CHUNK: usize = 16 * 1024;
#[cfg(kani)]
pub(crate) const CHUNK: usize = 16;
#[cfg(not(kani))]
const POOL_MAX: usize = 64;
#[cfg(not(kani))]
const POOL_KEEP: usize = 8;
#[cfg(not(kani))]
const POOL_IDLE: std::time::Duration = std::time::Duration::from_secs(30);

#[cfg(not(kani))]
thread_local! {
    static POOL: RefCell<Vec<Box<[u8]>>> = const { RefCell::new(Vec::new()) };
    static MISS_AT: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

#[cfg(not(kani))]
fn alloc() -> Box<[u8]> {
    POOL.with(|p| p.borrow_mut().pop()).unwrap_or_else(|| {
        MISS_AT.with(|m| m.set(Some(std::time::Instant::now())));
        vec![0u8; CHUNK].into_boxed_slice()
    })
}

#[cfg(not(kani))]
pub(crate) fn trim(now: std::time::Instant) {
    POOL.with(|p| {
        let mut p = p.borrow_mut();
        if p.len() > POOL_KEEP && MISS_AT.with(|m| m.get()).is_none_or(|t| now.saturating_duration_since(t) >= POOL_IDLE) {
            p.truncate(POOL_KEEP);
            p.shrink_to_fit();
        }
    });
}

#[cfg(kani)]
pub(crate) fn trim(_: std::time::Instant) {}

#[cfg(not(kani))]
fn release(c: Box<[u8]>) {
    POOL.with(|p| {
        let mut p = p.borrow_mut();
        if p.len() < POOL_MAX {
            p.push(c);
        }
    });
}

#[cfg(kani)]
fn alloc() -> Box<[u8]> {
    vec![0u8; CHUNK].into_boxed_slice()
}

#[cfg(kani)]
fn release(_: Box<[u8]>) {}

#[derive(Default)]
pub(crate) struct Chain {
    chunks: VecDeque<Box<[u8]>>,
    head: usize,
    len: usize,
}

impl Chain {
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn push(&mut self, mut d: &[u8]) {
        while !d.is_empty() {
            let end = self.head + self.len;
            let (i, o) = (end / CHUNK, end % CHUNK);
            if i == self.chunks.len() {
                self.chunks.push_back(alloc());
            }
            let n = (CHUNK - o).min(d.len());
            self.chunks[i][o..o + n].copy_from_slice(&d[..n]);
            self.len += n;
            d = &d[n..];
        }
    }

    pub(crate) fn tail_space(&mut self, limit: usize) -> &mut [u8] {
        let end = self.head + self.len;
        let (i, o) = (end / CHUNK, end % CHUNK);
        if i == self.chunks.len() {
            self.chunks.push_back(alloc());
        }
        let n = (CHUNK - o).min(limit);
        &mut self.chunks[i][o..o + n]
    }

    pub(crate) fn commit(&mut self, n: usize) {
        self.len += n;
        if self.len == 0 && self.head == 0 {
            for c in self.chunks.drain(..) {
                release(c);
            }
        }
    }

    pub(crate) fn front(&self) -> &[u8] {
        if self.len == 0 {
            return &[];
        }
        let n = (CHUNK - self.head).min(self.len);
        &self.chunks[0][self.head..self.head + n]
    }

    pub(crate) fn consume(&mut self, n: usize) {
        let n = n.min(self.len);
        self.len -= n;
        self.head += n;
        while self.head >= CHUNK {
            if let Some(c) = self.chunks.pop_front() {
                release(c);
            }
            self.head -= CHUNK;
        }
        if self.len == 0 {
            for c in self.chunks.drain(..) {
                release(c);
            }
            self.head = 0;
        }
    }

    pub(crate) fn slices<'a>(&'a self, off: usize, len: usize, mut f: impl FnMut(&'a [u8])) {
        debug_assert!(off + len <= self.len);
        let mut pos = self.head + off;
        let end = pos + len;
        while pos < end {
            let (i, o) = (pos / CHUNK, pos % CHUNK);
            let n = (CHUNK - o).min(end - pos);
            f(&self.chunks[i][o..o + n]);
            pos += n;
        }
    }

    pub(crate) fn read(&mut self, out: &mut [u8]) -> usize {
        let n = out.len().min(self.len);
        let mut at = 0;
        self.slices(0, n, |s| {
            out[at..at + s.len()].copy_from_slice(s);
            at += s.len();
        });
        self.consume(n);
        n
    }

    pub(crate) fn clear(&mut self) {
        self.consume(self.len);
    }
}

impl Drop for Chain {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(kani)]
#[path = "proofs/buf.rs"]
mod proofs;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_roundtrip_across_chunks() {
        let mut c = Chain::default();
        let data: Vec<u8> = (0..100_000u32).map(|i| (i * 7) as u8).collect();
        let mut fed = 0;
        let mut got = Vec::new();
        let mut step = 1;
        while got.len() < data.len() {
            let n = (step * 997 % 40_000).min(data.len() - fed);
            c.push(&data[fed..fed + n]);
            fed += n;
            let mut part = Vec::new();
            c.slices(0, c.len().min(30_000), |s| part.extend_from_slice(s));
            let mut out = vec![0u8; step * 1301 % 25_000 + 1];
            let r = c.read(&mut out);
            assert_eq!(&out[..r], &part[..r]);
            got.extend_from_slice(&out[..r]);
            step += 1;
        }
        assert_eq!(got, data);
        assert!(c.is_empty() && c.chunks.is_empty());
    }

    #[test]
    fn tail_space_and_front_roundtrip() {
        let mut c = Chain::default();
        let data: Vec<u8> = (0..50_000u32).map(|i| (i % 251) as u8).collect();
        let mut fed = 0;
        while fed < data.len() {
            let sp = c.tail_space(7_000);
            let n = sp.len().min(data.len() - fed);
            sp[..n].copy_from_slice(&data[fed..fed + n]);
            c.commit(n);
            fed += n;
        }
        assert_eq!(c.len(), data.len());
        let mut got = Vec::new();
        while !c.is_empty() {
            let f = c.front();
            let n = f.len().min(5_000);
            got.extend_from_slice(&f[..n]);
            c.consume(n);
        }
        assert_eq!(got, data);
        let _ = c.tail_space(100);
        c.commit(0);
        assert!(c.chunks.is_empty(), "пустой хвост вернулся в пул");
    }
}
