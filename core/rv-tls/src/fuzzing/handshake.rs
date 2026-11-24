use super::*;
use std::collections::HashSet;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

fn offset_in(outer: &[u8], inner: &[u8]) -> usize {
    let (o, i) = (outer.as_ptr() as usize, inner.as_ptr() as usize);
    assert!(i >= o && i - o + inner.len() <= outer.len());
    i - o
}

pub fn reader(data: &[u8]) {
    let Some((&k, rest)) = data.split_first() else { return };
    let (ops, input) = rest.split_at(usize::from(k % 16).min(rest.len()));
    let mut r = Reader(input);
    for &op in ops {
        let b = r.0;
        let (hdr, got) = match op % 8 {
            0 => {
                let n = usize::from(op >> 3);
                (0, r.take(n).inspect(|t| assert_eq!(t.len(), n)))
            }
            1 => (
                0,
                r.u8().map(|v| {
                    assert_eq!(v, b[0]);
                    &b[..1]
                }),
            ),
            2 => (
                0,
                r.u16().map(|v| {
                    assert_eq!(v.to_be_bytes(), b[..2]);
                    &b[..2]
                }),
            ),
            3 => (
                0,
                r.u24().map(|v| {
                    assert_eq!(v, u32::from_be_bytes([0, b[0], b[1], b[2]]) as usize);
                    &b[..3]
                }),
            ),
            4 => (1, r.vec8().inspect(|t| assert_eq!(t.len(), usize::from(b[0])))),
            5 => (2, r.vec16().inspect(|t| assert_eq!(t.len(), usize::from(u16::from_be_bytes([b[0], b[1]]))))),
            6 => (3, r.vec24().inspect(|t| assert_eq!(t.len(), u32::from_be_bytes([0, b[0], b[1], b[2]]) as usize))),
            _ => (0, r.take(usize::MAX)),
        };
        match got {
            Ok(t) => {
                assert_eq!(offset_in(b, t), hdr);
                assert_eq!(offset_in(b, r.0), hdr + t.len());
                assert_eq!(r.0.len(), b.len() - hdr - t.len());
            }
            Err(_) if matches!(op % 8, 4..=6) => return,
            Err(_) => assert_eq!(r.0.len(), b.len()),
        }
    }
    let (a, b) = input.split_at(input.len() / 2);
    assert_eq!(ct_eq(a, b), a == b);
}

pub fn extensions(data: &[u8]) {
    for tls13 in [true, false] {
        if let Ok(limit) = record_size_limit(data, tls13) {
            assert!((64 - usize::from(tls13)..=MAX_PT).contains(&limit));
        }
    }
    let mut r = Reader(data);
    let Ok(exts) = r.extensions() else { return };
    let declared = usize::from(u16::from_be_bytes([data[0], data[1]]));
    let mut seen = HashSet::new();
    let mut at = 2;
    for (t, b) in &exts {
        assert!(seen.insert(*t), "повтор расширения {t}");
        assert_eq!(u16::from_be_bytes([data[at], data[at + 1]]), *t);
        assert_eq!(usize::from(u16::from_be_bytes([data[at + 2], data[at + 3]])), b.len());
        assert_eq!(offset_in(data, b), at + 4);
        at += 4 + b.len();
        if *t == 28 {
            for tls13 in [true, false] {
                if let Ok(limit) = record_size_limit(b, tls13) {
                    assert!((64 - usize::from(tls13)..=MAX_PT).contains(&limit));
                }
            }
        }
    }
    assert_eq!(at, 2 + declared);
    assert_eq!(r.0.len(), data.len() - 2 - declared);
}

fn certificate_body(body: &[u8]) -> Option<Vec<Vec<u8>>> {
    let certs = parse_certificate_body(body).ok()?;
    assert!(!certs.is_empty() && certs.len() <= MAX_CERTS);
    assert!(certs.iter().all(|c| !c.is_empty()));
    let total: usize = certs.iter().map(|c| 5 + c.len()).sum();
    assert!(1 + 3 + total <= body.len());
    Some(certs)
}

pub fn certificate(data: &[u8]) {
    let Some((&kind, body)) = data.split_first() else { return };
    let Some(certs) = certificate_body(body) else { return };
    let hash = |c: &[u8]| -> [u8; 32] { digest::digest(&digest::SHA256, c).as_ref().try_into().unwrap() };
    let _ = match kind % 4 {
        0 => verify_chain(&certs, "fuzz.test", &[]),
        1 => verify_pinned(&certs, &Pins { sha256: vec![hash(certs.last().unwrap())], names: Vec::new() }, "fuzz.test"),
        2 => verify_pinned(&certs, &Pins { sha256: Vec::new(), names: vec!["fuzz.test".into()] }, ""),
        _ => verify_pinned(&certs, &Pins { sha256: vec![[0; 32]], names: Vec::new() }, "fuzz.test"),
    };
}

pub fn compressed_certificate(data: &[u8]) {
    let mut r = Reader(data);
    if let (Ok(alg), Ok(len), Ok(packed)) = (r.u16(), r.u24(), r.vec24())
        && let Ok(out) = decompress_cert(alg, packed, len)
    {
        assert!(out.len() == len && len <= MAX_CERT_LIST);
        let _ = certificate_body(&out);
    }
    if data.len() > MAX_CERT_LIST {
        return;
    }
    let packed = miniz_oxide::deflate::compress_to_vec_zlib(data, (data.len() % 11) as u8);
    let zlib = hello::CERT_COMPRESSION_ZLIB;
    assert!(decompress_cert(zlib, &packed, data.len()).ok().as_deref() == Some(data), "zlib: сжатое не распаковалось обратно");
    assert!(decompress_cert(zlib, &packed, data.len() + 1).is_err());
    if let Some(shorter) = data.len().checked_sub(1) {
        assert!(decompress_cert(zlib, &packed, shorter).is_err());
    }
}

pub fn certificate_verify(data: &[u8]) {
    let Some((&kind, rest)) = data.split_first() else { return };
    let mut r = Reader(rest);
    let (Ok(cert), Ok(scheme), Ok(sig)) = (r.vec16(), r.u16(), r.vec16()) else { return };
    let certs = vec![cert.to_vec()];
    let peer = match kind % 4 {
        0 => Peer::Insecure(certs),
        1 => Peer::Pinned(certs),
        2 => Peer::WebPki(certs),
        _ => Peer::Reality(cert.get(..32).map_or([0; 32], |k| k.try_into().unwrap())),
    };
    let _ = verify_cv(&peer, scheme, sig, r.0);
}

fn mldsa65_key() -> &'static [u8] {
    static KEY: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    KEY.get_or_init(|| {
        use ml_dsa::{KeyInit, Keypair, MlDsa65, SigningKey};
        SigningKey::<MlDsa65>::new_from_slice(&[42; 32]).unwrap().verifying_key().encode().to_vec()
    })
}

pub fn reality(data: &[u8]) {
    const AUTH: [u8; 32] = [7; 32];
    let Some((&kind, leaf)) = data.split_first() else { return };
    let mut leaf = leaf.to_vec();
    let key =
        der::parse(&leaf).filter(|c| c.key_alg == [0x2b, 0x65, 0x70] && c.sig.len() == 64).and_then(|c| <[u8; 32]>::try_from(c.key).ok());
    if let Some(k) = key {
        let mac = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA512, &AUTH), &k);
        let at = leaf.len() - 64;
        leaf[at..].copy_from_slice(mac.as_ref());
    }
    let verify = if kind & 1 != 0 { mldsa65_key() } else { &[][..] };
    let r = reality_verify_cert(&leaf, &AUTH, verify, b"\x01\0\0\x05hello", b"\x02\0\0\x05world");
    if let Some(k) = key
        && verify.is_empty()
    {
        assert_eq!(r.ok(), Some(k), "верная HMAC-подпись — сертификат принят");
    }
}

struct Server {
    stream: Vec<u8>,
    at: usize,
    step: usize,
    sent: Vec<u8>,
    patched: bool,
}

impl Server {
    fn patch_session_id(&mut self) {
        let sid: [u8; 32] = self.sent[44..76].try_into().unwrap();
        let mut at = 0;
        while let Some(h) = self.stream.get(at..at + 5) {
            let (typ, next) = (h[0], at + 5 + usize::from(u16::from_be_bytes([h[3], h[4]])));
            if typ == CT_HANDSHAKE
                && self.stream.get(at + 5) == Some(&HT_SERVER_HELLO)
                && self.stream.get(at + 43) == Some(&32)
                && let Some(dst) = self.stream.get_mut(at + 44..at + 76)
            {
                dst.copy_from_slice(&sid);
            }
            at = next;
        }
    }
}

impl AsyncRead for Server {
    fn poll_read(mut self: Pin<&mut Self>, _: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let s = &mut *self;
        if !s.patched && s.sent.len() >= 76 {
            s.patched = true;
            s.patch_session_id();
        }
        let n = (s.stream.len() - s.at).min(buf.remaining()).min(s.step);
        buf.put_slice(&s.stream[s.at..s.at + n]);
        s.at += n;
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Server {
    fn poll_write(mut self: Pin<&mut Self>, _: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.sent.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

const FINGERPRINTS: [crate::Fingerprint; 8] = {
    use crate::Fingerprint::*;
    [Chrome, Firefox, Safari, Ios13, Golang, Chrome120Pq, Android, Firefox63]
};

pub fn server_flight(data: &[u8]) {
    let Some((&ctl, stream)) = data.split_first() else { return };
    let cfg = ClientConfig {
        server_name: "fuzz.test".into(),
        alpn: Vec::new(),
        alpn_override: false,
        fingerprint: FINGERPRINTS[usize::from(ctl & 7)],
        verify: if ctl & 8 != 0 { Verify::WebPki } else { Verify::InsecureSkipVerify },
    };
    let step = [usize::MAX, 1, 2, 3, 5, 8, 13, 50, 100, 300, 1000, 2000, 4000, 9000, 17000, 40000][usize::from(ctl >> 4)];
    let mut conn = Conn::new(Server { stream: stream.to_vec(), at: 0, step, sent: Vec::new(), patched: false });
    let mut fut = std::pin::pin!(run(&mut conn, &cfg, &[]));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(fut.as_mut().poll(&mut cx).is_ready(), "рукопожатие ждёт не сокет");
}
