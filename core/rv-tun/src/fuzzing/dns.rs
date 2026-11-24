use super::*;

fn key(m: &[u8]) {
    let Some(k) = cache_key(m) else { return };
    let e = skip_name(m, 12).expect("ключ есть — имя разобрано");
    assert!(k.len() >= 5 && m.len() >= 12 + k.len() && u16::from_be_bytes([m[4], m[5]]) == 1);
    assert_eq!(k.len(), e + 4 - 12);
    assert!(k[..e - 12].eq_ignore_ascii_case(&m[12..e]) && k[e - 12..] == m[e..e + 4]);
    let mut up = m.to_vec();
    let mut i = 12;
    while i + 1 < e && up[i] < 64 {
        let n = usize::from(up[i]);
        up[i + 1..i + 1 + n].make_ascii_uppercase();
        i += 1 + n;
    }
    assert_eq!(cache_key(&up), Some(k));
}

fn fail(q: &[u8]) {
    let Some(r) = servfail(q) else { return };
    let e = skip_name(q, 12).expect("SERVFAIL — по разобранному вопросу");
    assert!(r.len() >= 12 && r.len() <= q.len());
    assert_eq!(r[..2], q[..2]);
    assert!(r[2] & 0x80 != 0 && r[3] & 0x0f == 2);
    assert_eq!(r[4..12], [0, 1, 0, 0, 0, 0, 0, 0]);
    assert_eq!(r[12..], q[12..e + 4]);
    if u16::from_be_bytes([q[4], q[5]]) == 1 {
        assert!(answers(q, &r));
    }
    assert!(!usable(q, &r));
    assert_eq!(reply_ttl(&r), None);
}

fn ttl(r: &[u8]) {
    let Some(t) = reply_ttl(r) else { return };
    let rcode = r[3] & 0x0f;
    assert!(t <= CACHE_TTL_MAX && r[2] & 0x02 == 0 && (rcode == 0 || rcode == 3));
    if u16::from_be_bytes([r[6], r[7]]) == 0 || rcode == 3 {
        assert!(t <= CACHE_NEG_MAX);
    }
}

fn age(m: &[u8], by: u32) {
    let mut c = m.to_vec();
    age_ttls(&mut c, by);
    assert_eq!(c.len(), m.len());
    let q_end = skip_name(m, 12).map_or(m.len(), |e| (e + 4).min(m.len()));
    assert_eq!(c[..q_end], m[..q_end]);
    if by == 0 {
        assert_eq!(c, m);
    }
    let (before, after) = (reply_ttl(m), reply_ttl(&c));
    assert_eq!(before.is_some(), after.is_some());
    if before.is_some_and(|t| t >= 1) {
        assert!(after <= before);
    }
}

pub fn message(data: &[u8]) {
    if let Some(e) = skip_name(data, 12) {
        assert!(e > 12 && e <= data.len());
    }
    if let Some(name) = qname(data) {
        assert!(!name.is_empty() && !name.bytes().any(|c| c.is_ascii_uppercase()));
    }
    key(data);
    fail(data);
    ttl(data);
    for by in [0, 1, 59, 600, u32::MAX] {
        age(data, by);
    }
}

pub fn pair(data: &[u8]) {
    let [a, b, rest @ ..] = data else { return };
    let (q, r) = rest.split_at(usize::from(u16::from_be_bytes([*a, *b])).min(rest.len()));
    if answers(q, r) {
        assert!(r.len() >= 12 && r[..2] == q[..2] && r[2] & 0x80 != 0);
        assert!(cache_key(q).is_some() && cache_key(r) == cache_key(q));
    }
    if usable(q, r) {
        let rcode = r[3] & 0x0f;
        assert!(rcode != 2 && rcode != 5);
        let qtype = skip_name(q, 12).and_then(|e| q.get(e..e + 2)).map(|t| u16::from_be_bytes([t[0], t[1]]));
        if let Some(t @ (1 | 28)) = qtype {
            assert!(rcode == 0 && has_record(r, t));
        }
    }
    for t in [1, 5, 28, u16::from_be_bytes([*a, *b])] {
        if has_record(r, t) {
            assert!(r.len() >= 12 && u16::from_be_bytes([r[6], r[7]]) >= 1);
        }
    }
}

fn piece(rest: &[u8]) -> Option<(&[u8], &[u8])> {
    let (&n, tail) = rest.split_first()?;
    Some(tail.split_at(usize::from(n).min(tail.len())))
}

fn reply_to(q: &[u8], rcode: u8, an: u8, records: &[u8]) -> Vec<u8> {
    let mut r = q.to_vec();
    if r.len() >= 12 {
        if let Some(e) = skip_name(&r, 12) {
            r.truncate(e + 4);
        }
        r[2] |= 0x80;
        r[3] = (r[3] & 0xf0) | (rcode & 0x0f);
        r[6..8].copy_from_slice(&[0, an]);
    }
    r.extend_from_slice(records);
    r
}

fn hit(c: &Cache, q: &[u8], stored: &HashMap<Vec<u8>, Vec<u8>>) {
    let Some(h) = c.get(q) else { return };
    let k = cache_key(q).expect("попадание только по ключу");
    let s = stored.get(&k).expect("в кэше только сохранённое");
    let e = skip_name(q, 12).unwrap();
    assert_eq!(h.len(), s.len());
    assert_eq!(h[..2], q[..2]);
    assert_eq!(h[2..12], s[2..12]);
    assert_eq!(h[12..e], q[12..e]);
    assert_eq!(h[e..e + 4], s[e..e + 4]);
    assert!(answers(q, &h));
    assert!(reply_ttl(&h) <= reply_ttl(s));
}

pub fn cache(data: &[u8]) {
    let mut c = Cache::default();
    let mut stored = HashMap::new();
    let mut rest = data;
    while let Some((&op, tail)) = rest.split_first() {
        let Some((q, tail)) = piece(tail) else { break };
        rest = tail;
        if op & 1 != 0 {
            hit(&c, q, &stored);
            continue;
        }
        let r = if op & 2 == 0 {
            let Some((r, tail)) = piece(rest) else { break };
            rest = tail;
            r.to_vec()
        } else {
            let Some((&an, tail)) = rest.split_first() else { break };
            let Some((records, tail)) = piece(tail) else { break };
            rest = tail;
            reply_to(q, op >> 4, an, records)
        };
        let before = Instant::now();
        assert_eq!(c.put(q, &r), answers(q, &r));
        assert!(c.map.len() <= CACHE_MAX);
        if let (true, Some(k), Some(t)) = (answers(q, &r), cache_key(q), reply_ttl(&r))
            && t > 0
            && r.len() <= CACHE_REPLY_MAX
        {
            stored.insert(k, r);
            let found = c.get(q).is_some();
            if Instant::now() < before + Duration::from_secs(u64::from(t)) {
                assert!(found, "только что сохранённый ответ не найден");
            }
        }
        hit(&c, q, &stored);
    }
}
