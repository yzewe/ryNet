use super::*;

fn offset_in(outer: &[u8], inner: &[u8]) -> usize {
    let (o, i) = (outer.as_ptr() as usize, inner.as_ptr() as usize);
    assert!(i >= o && i - o + inner.len() <= outer.len());
    i - o
}

fn encode_len(len: usize, out: &mut [u8; 5]) -> usize {
    if len < 0x80 {
        out[0] = len as u8;
        return 1;
    }
    let bytes = (len as u32).to_be_bytes();
    let skip = bytes.iter().take_while(|&&b| b == 0).count();
    let n = 4 - skip;
    out[0] = 0x80 | n as u8;
    out[1..1 + n].copy_from_slice(&bytes[skip..]);
    1 + n
}

pub fn tlv(data: &[u8]) {
    let mut rest = data;
    match next(&mut rest) {
        Some((tag, body)) => {
            assert_eq!(tag, data[0]);
            let start = offset_in(data, body);
            let mut enc = [0u8; 5];
            let n = encode_len(body.len(), &mut enc);
            assert_eq!(start, 1 + n);
            assert_eq!(&data[1..start], &enc[..n]);
            assert_eq!(offset_in(data, rest), start + body.len());
            assert_eq!(rest.len(), data.len() - start - body.len());
        }
        None => assert_eq!(rest.len(), data.len()),
    }
    let Some((&tag, body)) = data.split_first() else { return };
    let mut enc = [0u8; 5];
    let n = encode_len(body.len(), &mut enc);
    let wire = [&[tag][..], &enc[..n], body, &[tag]].concat();
    let mut rest = &wire[..];
    assert_eq!(next(&mut rest), Some((tag, body)));
    assert_eq!(rest, [tag]);
}

pub fn cert(data: &[u8]) {
    if let Some(c) = parse(data) {
        let ka = offset_in(data, c.key_alg);
        let k = offset_in(data, c.key);
        let s = offset_in(data, c.sig);
        assert!(ka + c.key_alg.len() <= k && k + c.key.len() <= s);
        if !c.extensions.is_empty() {
            let e = offset_in(data, c.extensions);
            assert!(k + c.key.len() <= e && e + c.extensions.len() <= s);
        }
        assert_eq!(s + c.sig.len(), data.len());
        for x in [0x00, 0xff] {
            assert!(parse(&[data, &[x]].concat()).is_none());
        }
        let _ = c.is_ca();
        if let Some(v) = c.first_extension() {
            offset_in(c.extensions, v);
        }
    }
    let raw = Cert { key_alg: &[], key: &[], sig: &[], extensions: data };
    let _ = raw.is_ca();
    if let Some(v) = raw.first_extension() {
        offset_in(data, v);
    }
}
