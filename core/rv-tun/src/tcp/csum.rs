#[derive(Clone, Copy, Default)]
pub(crate) struct Csum {
    acc: u64,
    ne: u64,
    odd: bool,
}

impl Csum {
    pub(crate) fn add(&mut self, mut d: &[u8]) {
        if d.is_empty() {
            return;
        }
        if self.odd {
            self.acc += d[0] as u64;
            d = &d[1..];
            self.odd = false;
        }
        let (wide, rest) = d.as_chunks::<16>();
        let mut ne = 0u64;
        for c in wide {
            let w = |i: usize| u32::from_ne_bytes([c[i], c[i + 1], c[i + 2], c[i + 3]]) as u64;
            ne += w(0) + w(4) + w(8) + w(12);
        }
        self.ne += ne;
        let mut acc = 0u64;
        let (words, rest) = rest.as_chunks::<2>();
        for w in words {
            acc += u16::from_be_bytes(*w) as u64;
        }
        if let [b] = rest {
            acc += (*b as u64) << 8;
            self.odd = true;
        }
        self.acc += acc;
    }

    pub(crate) fn add_u16(&mut self, v: u16) {
        debug_assert!(!self.odd);
        self.acc += v as u64;
    }

    pub(crate) fn finish(self) -> u16 {
        let fold = |mut s: u64| {
            while s >> 16 != 0 {
                s = (s & 0xffff) + (s >> 16);
            }
            s as u16
        };
        let ne = fold(self.ne);
        let ne = if cfg!(target_endian = "little") { ne.swap_bytes() } else { ne };
        !fold(self.acc + ne as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(d: &[u8]) -> u16 {
        let mut s = 0u32;
        for w in d.chunks(2) {
            s += u16::from_be_bytes([w[0], *w.get(1).unwrap_or(&0)]) as u32;
            s = (s & 0xffff) + (s >> 16);
        }
        !(s as u16)
    }

    #[test]
    fn split_anywhere_matches_reference() {
        let mut x = 0x1234_5678u32;
        let data: Vec<u8> = (0..3001)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect();
        for len in [0, 1, 2, 3, 15, 16, 17, 100, 1499, 3001] {
            let d = &data[..len];
            let want = reference(d);
            for cut1 in [0, 1, 2, 3, 7, len / 2, len] {
                for cut2 in [cut1, cut1 + 1, cut1 + 5, len] {
                    let (c1, c2) = (cut1.min(len), cut2.min(len).max(cut1.min(len)));
                    let mut c = Csum::default();
                    c.add(&d[..c1]);
                    c.add(&d[c1..c2]);
                    c.add(&d[c2..]);
                    assert_eq!(c.finish(), want, "len {len} cuts {c1} {c2}");
                }
            }
        }
    }
}

#[cfg(test)]
mod bench {
    use super::*;

    #[test]
    #[ignore]
    fn csum_speed() {
        let data: Vec<u8> = (0..65536u32).map(|i| ((i * 2654435761u32) >> 24) as u8).collect();
        let t0 = std::time::Instant::now();
        let mut x = 0u16;
        for i in 0..20_000 {
            let mut c = Csum::default();
            c.add(&data[i % 3..]);
            x ^= c.finish();
        }
        let gb = 20_000.0 * 65536.0 / 1e9;
        eprintln!("csum {:.1} GB/s ({x})", gb / t0.elapsed().as_secs_f64());
    }
}
