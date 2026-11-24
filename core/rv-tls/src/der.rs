pub(crate) struct Cert<'a> {
    pub key_alg: &'a [u8],
    pub key: &'a [u8],
    pub sig: &'a [u8],
    extensions: &'a [u8],
}

const SEQ: u8 = 0x30;
const OID: u8 = 0x06;
const BITS: u8 = 0x03;
const BOOL: u8 = 0x01;
const OCTETS: u8 = 0x04;
const INTEGER: u8 = 0x02;
const UTC_TIME: u8 = 0x17;
const GENERALIZED_TIME: u8 = 0x18;
const BASIC_CONSTRAINTS: &[u8] = &[0x55, 0x1d, 0x13];

fn next<'a>(buf: &mut &'a [u8]) -> Option<(u8, &'a [u8])> {
    let (&tag, rest) = buf.split_first()?;
    let (&l0, mut rest) = rest.split_first()?;
    let len = if l0 < 0x80 {
        l0 as usize
    } else {
        let n = (l0 & 0x7f) as usize;
        if n == 0 || n > 4 || rest.len() < n {
            return None;
        }
        let (l, r) = rest.split_at(n);
        rest = r;
        let len = l.iter().fold(0usize, |a, &b| a << 8 | b as usize);
        if l[0] == 0 || len < 0x80 {
            return None;
        }
        len
    };
    if rest.len() < len {
        return None;
    }
    let (body, tail) = rest.split_at(len);
    *buf = tail;
    Some((tag, body))
}

fn expect<'a>(buf: &mut &'a [u8], tag: u8) -> Option<&'a [u8]> {
    next(buf).filter(|(t, _)| *t == tag).map(|(_, b)| b)
}

fn bits(b: &[u8]) -> Option<&[u8]> {
    b.split_first().filter(|(u, _)| **u == 0).map(|(_, rest)| rest)
}

fn algorithm<'a>(buf: &mut &'a [u8]) -> Option<&'a [u8]> {
    let mut alg = expect(buf, SEQ)?;
    let oid = expect(&mut alg, OID)?;
    if !alg.is_empty() {
        next(&mut alg)?;
    }
    alg.is_empty().then_some(oid)
}

pub(crate) fn parse(der: &[u8]) -> Option<Cert<'_>> {
    let mut d = der;
    let mut cert = expect(&mut d, SEQ)?;
    let mut tbs = expect(&mut cert, SEQ)?;
    algorithm(&mut cert)?;
    let sig = bits(expect(&mut cert, BITS)?)?;
    if !cert.is_empty() || !d.is_empty() {
        return None;
    }
    if tbs.first() == Some(&0xa0) {
        let mut v = expect(&mut tbs, 0xa0)?;
        let ver = expect(&mut v, INTEGER)?;
        if !v.is_empty() || ver.len() != 1 || ver[0] > 2 {
            return None;
        }
    }
    if expect(&mut tbs, INTEGER)?.is_empty() {
        return None;
    }
    algorithm(&mut tbs)?;
    expect(&mut tbs, SEQ)?;
    let mut validity = expect(&mut tbs, SEQ)?;
    for _ in 0..2 {
        let (t, _) = next(&mut validity)?;
        if t != UTC_TIME && t != GENERALIZED_TIME {
            return None;
        }
    }
    if !validity.is_empty() {
        return None;
    }
    expect(&mut tbs, SEQ)?;
    let mut spki = expect(&mut tbs, SEQ)?;
    let key_alg = algorithm(&mut spki)?;
    let key = bits(expect(&mut spki, BITS)?)?;
    if !spki.is_empty() {
        return None;
    }
    let mut extensions: &[u8] = &[];
    let mut last = 0u8;
    while !tbs.is_empty() {
        let (tag, body) = next(&mut tbs)?;
        let order = match tag {
            0x81 => 1,
            0x82 => 2,
            0xa3 => 3,
            _ => return None,
        };
        if order <= last {
            return None;
        }
        last = order;
        if tag == 0xa3 {
            let mut b = body;
            extensions = expect(&mut b, SEQ)?;
            if !b.is_empty() || extensions.is_empty() {
                return None;
            }
        }
    }
    Some(Cert { key_alg, key, sig, extensions })
}

fn boolean(b: &[u8]) -> Option<bool> {
    match b {
        [0x00] => Some(false),
        [0xff] => Some(true),
        _ => None,
    }
}

impl Cert<'_> {
    pub fn first_extension(&self) -> Option<&[u8]> {
        let mut exts = self.extensions;
        let mut ext = expect(&mut exts, SEQ)?;
        expect(&mut ext, OID)?;
        if ext.first() == Some(&BOOL) {
            expect(&mut ext, BOOL).and_then(boolean)?;
        }
        let value = expect(&mut ext, OCTETS)?;
        ext.is_empty().then_some(value)
    }

    pub fn is_ca(&self) -> bool {
        let mut exts = self.extensions;
        let mut found = None;
        while !exts.is_empty() {
            let Some(mut ext) = expect(&mut exts, SEQ) else { return false };
            let Some(oid) = expect(&mut ext, OID) else { return false };
            if oid != BASIC_CONSTRAINTS {
                continue;
            }
            if found.is_some() {
                return false;
            }
            if ext.first() == Some(&BOOL) {
                match expect(&mut ext, BOOL).and_then(boolean) {
                    Some(true) => {}
                    _ => return false,
                }
            }
            let Some(mut value) = expect(&mut ext, OCTETS) else { return false };
            let Some(mut bc) = expect(&mut value, SEQ) else { return false };
            if !ext.is_empty() || !value.is_empty() {
                return false;
            }
            let mut ca = false;
            if bc.first() == Some(&BOOL) {
                match expect(&mut bc, BOOL).and_then(boolean) {
                    Some(true) => ca = true,
                    _ => return false,
                }
            }
            if !bc.is_empty() {
                match expect(&mut bc, INTEGER) {
                    Some(n) if !n.is_empty() && n[0] & 0x80 == 0 && bc.is_empty() => {}
                    _ => return false,
                }
            }
            found = Some(ca);
        }
        found.unwrap_or(false)
    }
}

#[cfg(kani)]
#[path = "proofs/der.rs"]
mod proofs;

#[cfg(fuzzing)]
#[path = "fuzzing/der.rs"]
pub mod fuzzing;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_key_signature_and_ca() {
        for ca in [false, true] {
            let mut params = rcgen::CertificateParams::new(vec!["a.example".into()]).unwrap();
            if ca {
                params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
            }
            let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519).unwrap();
            let cert = params.self_signed(&key).unwrap();
            let c = parse(cert.der()).unwrap();
            assert_eq!(c.key_alg, [0x2b, 0x65, 0x70]);
            assert_eq!(c.key, key.public_key_raw());
            assert_eq!(c.sig.len(), 64);
            assert_eq!(c.is_ca(), ca);
        }
        assert!(parse(&[0x30, 0x81]).is_none());
    }

    fn tlv(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![tag, body.len() as u8];
        v.extend_from_slice(body);
        v
    }

    fn bc(value: &[u8]) -> Vec<u8> {
        let ext = [tlv(OID, BASIC_CONSTRAINTS), tlv(OCTETS, &tlv(SEQ, value))].concat();
        tlv(SEQ, &ext)
    }

    #[test]
    fn malformed_basic_constraints_is_not_ca() {
        let ca = |exts: Vec<u8>| Cert { key_alg: &[], key: &[], sig: &[], extensions: &exts }.is_ca();
        assert!(ca(bc(&[0x01, 0x01, 0xff])));
        assert!(ca(bc(&[0x01, 0x01, 0xff, 0x02, 0x01, 0x00])));
        assert!(!ca(bc(&[0x01, 0x02, 0xff, 0x00])));
        assert!(!ca(bc(&[0x01, 0x01, 0x01])));
        assert!(!ca(bc(&[0x01, 0x01, 0xff, 0x05, 0x00])));
        assert!(!ca([bc(&[0x01, 0x01, 0xff]), bc(&[0x01, 0x01, 0xff])].concat()));
        assert!(!ca(bc(&[])));
    }

    #[test]
    fn malformed_structure_is_rejected() {
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519).unwrap();
        let der = rcgen::CertificateParams::new(vec!["a.example".into()]).unwrap().self_signed(&key).unwrap().der().to_vec();
        assert!(parse(&der).is_some());
        let mut tail = der.clone();
        tail.push(0);
        assert!(parse(&tail).is_none());
        assert!(parse(&[0x30, 0x81, 0x01, 0x00]).is_none());
    }
}
