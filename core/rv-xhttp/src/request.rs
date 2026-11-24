use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use bytes::Bytes;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    StreamOne,
    StreamUp,
    PacketUp,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Range {
    pub from: i64,
    pub to: i64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Xmux {
    pub max_concurrency: Range,
    pub max_connections: Range,
    pub c_max_reuse_times: Range,
    pub h_max_request_times: Range,
    pub h_max_reusable_secs: Range,
    pub h_keep_alive_period: i64,
}
#[derive(Clone, Debug)]
pub struct Config {
    mode_name: String,
    host_name: String,
    req_host: String,
    path: String,
    headers: Vec<(String, String)>,
    method: String,
    no_grpc: bool,
    data_place: String,
    data_key: String,
    chunk: Range,
    session_place: String,
    session_key: String,
    session_table: String,
    session_length: Range,
    seq_place: String,
    seq_key: String,
    pad_obfs: bool,
    pad_key: String,
    pad_header: String,
    pad_place: String,
    pad_method: String,
    pad_bytes: Range,
    max_post: Range,
    min_interval: Range,
    xmux_config: Xmux,
    download: Option<Value>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Bytes>,
}

fn get<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
    v.get(k).or_else(|| v.as_object()?.iter().find(|(name, _)| name.eq_ignore_ascii_case(k)).map(|(_, x)| x))
}
fn string(v: &Value, k: &str, d: &str) -> String {
    get(v, k).and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or(d).to_owned()
}
fn boolv(v: &Value, k: &str) -> bool {
    get(v, k).and_then(Value::as_bool).unwrap_or(false)
}
fn int(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}
impl Range {
    pub fn parse(v: &Value) -> Result<Self, String> {
        if let Some(n) = int(v) {
            return Ok(Self { from: n, to: n });
        }
        let s = v.as_str().ok_or("range must be a number or a-b")?;
        if let Some((a, b)) = s.split_once('-') {
            let from = a.parse().map_err(|_| "invalid range start")?;
            let to = b.parse().map_err(|_| "invalid range end")?;
            if from > to {
                return Err("range start exceeds end".into());
            }
            Ok(Self { from, to })
        } else {
            let n = s.parse().map_err(|_| "invalid range")?;
            Ok(Self { from: n, to: n })
        }
    }
}
fn range(v: &Value, k: &str, d: Range) -> Range {
    get(v, k).and_then(|x| Range::parse(x).ok()).unwrap_or(d)
}
const SECS_MAX: i64 = 24 * 60 * 60;
const PAD_MAX: i64 = 64 * 1024;
const SESSION_ID_MAX: i64 = 4096;
const POST_MAX: i64 = 16 << 20;
const CHUNK_MAX: i64 = 1 << 20;
fn capped(r: Range, max: i64) -> Range {
    Range { from: r.from.min(max), to: r.to.min(max) }
}
fn predefined_table(name: &str) -> &str {
    match name {
        "ALPHABET" => "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
        "Alphabet" => "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
        "BASE36" => "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ",
        "Base62" => "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
        "HEX" => "0123456789ABCDEF",
        "alphabet" => "abcdefghijklmnopqrstuvwxyz",
        "base36" => "0123456789abcdefghijklmnopqrstuvwxyz",
        "hex" => "0123456789abcdef",
        "number" => "0123456789",
        other => other,
    }
}
fn session_room(n: usize, len: Range) -> u64 {
    let mut sum = 0u64;
    for k in len.from.max(0)..=len.to.max(0) {
        sum = sum.saturating_add(u32::try_from(k).ok().and_then(|k| (n as u64).checked_pow(k)).unwrap_or(u64::MAX));
        if sum >= 1 << 31 {
            break;
        }
    }
    sum
}
fn or_default(r: Range, d: Range) -> Range {
    if r.to == 0 { d } else { r }
}
const PAD_DEFAULT: Range = Range { from: 100, to: 1000 };
const POST_DEFAULT: Range = Range { from: 1_000_000, to: 1_000_000 };
const INTERVAL_DEFAULT: Range = Range { from: 30, to: 30 };
fn canonical(s: &str) -> String {
    let mut upper = true;
    s.chars()
        .map(|c| {
            if c == '-' {
                upper = true;
                c
            } else if upper {
                upper = false;
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}
fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
            out.push(b as char)
        } else if b == b' ' {
            out.push('+')
        } else {
            out.push_str(&format!("%{b:02X}"))
        }
    }
    out
}
fn dec(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => match (hex(b[i + 1]), hex(b[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h << 4 | l);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
fn query_append(path: &mut String, key: &str, val: &str) {
    let (base, q) = path.split_once('?').map(|(a, b)| (a.to_owned(), b.to_owned())).unwrap_or((path.clone(), String::new()));
    let mut pairs: Vec<(String, String)> = q
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (a, b) = p.split_once('=').unwrap_or((p, ""));
            (dec(a), dec(b))
        })
        .collect();
    pairs.retain(|(k, _)| k != key);
    pairs.push((key.into(), val.into()));
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    *path = format!("{}?{}", base, pairs.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&"));
}
pub fn random_text(n: usize, alphabet: &[u8]) -> String {
    let rng = SystemRandom::new();
    let mut out = String::with_capacity(n);
    if alphabet.is_empty() || alphabet.len() > 256 {
        return out;
    }
    let limit = 256 - 256 % alphabet.len();
    let mut b = vec![0u8; n + n / 4 + 16];
    while out.len() < n {
        if rng.fill(&mut b).is_err() {
            break;
        }
        for &x in b.iter().filter(|&&x| (x as usize) < limit).take(n - out.len()) {
            out.push(alphabet[x as usize % alphabet.len()] as char);
        }
    }
    out
}
pub(crate) const HPACK_BITS: [u8; 257] = [
    13, 23, 28, 28, 28, 28, 28, 28, 28, 24, 30, 28, 28, 30, 28, 28, 28, 28, 28, 28, 28, 28, 30, 28, 28, 28, 28, 28, 28, 28, 28, 28, 6, 10,
    10, 12, 13, 6, 8, 11, 10, 10, 8, 11, 8, 6, 6, 6, 5, 5, 5, 6, 6, 6, 6, 6, 6, 6, 7, 8, 15, 6, 12, 10, 13, 6, 7, 7, 7, 7, 7, 7, 7, 7, 7,
    7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 8, 7, 8, 13, 19, 13, 14, 6, 15, 5, 6, 5, 6, 5, 6, 6, 6, 5, 7, 7, 6, 6, 6, 5, 6, 7, 6, 5, 5, 6,
    7, 7, 7, 7, 7, 15, 11, 14, 13, 28, 20, 22, 20, 20, 22, 22, 22, 23, 22, 23, 23, 23, 23, 23, 24, 23, 24, 24, 22, 23, 24, 23, 23, 23, 23,
    21, 22, 23, 22, 23, 23, 24, 22, 21, 20, 22, 22, 23, 23, 21, 23, 22, 22, 24, 21, 22, 23, 23, 21, 21, 22, 21, 23, 22, 23, 23, 20, 22, 22,
    22, 23, 22, 22, 23, 26, 26, 20, 19, 22, 23, 22, 25, 26, 26, 26, 27, 27, 26, 24, 25, 19, 21, 26, 27, 27, 26, 27, 24, 21, 21, 26, 26, 28,
    27, 27, 27, 20, 24, 20, 21, 22, 21, 21, 23, 22, 22, 25, 25, 24, 24, 26, 23, 26, 27, 26, 26, 27, 27, 27, 27, 27, 28, 27, 27, 27, 27, 27,
    26, 30,
];

#[doc(hidden)]
pub fn huffman_len(s: &str) -> usize {
    s.bytes().map(|b| HPACK_BITS[b as usize] as usize).sum::<usize>().div_ceil(8)
}

fn padding(n: usize, method: &str) -> String {
    if method != "tokenish" {
        return "X".repeat(n);
    }
    let chars = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut s = random_text(((n as f64) / 0.8).ceil() as usize, chars);
    for i in 0..150 {
        let h = huffman_len(&s);
        if (h as i64 - n as i64).abs() <= 2 {
            return s;
        }
        if h < n {
            s.push(if i % 2 == 0 { 'X' } else { 'Z' });
        } else if s.len() > 1 {
            s.pop();
        } else {
            break;
        }
    }
    s
}

#[cfg(test)]
fn padding_valid(s: &str, r: Range, method: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if method == "tokenish" {
        let n = huffman_len(s) as i64;
        n >= 0.max(r.from - 2) && n <= r.to + 2
    } else {
        (s.len() as i64) >= r.from && (s.len() as i64) <= r.to
    }
}
impl Config {
    pub fn from_json(v: &Value) -> Result<Config, String> {
        let mut c = v.clone();
        if let Some(mut e) = get(v, "extra").filter(|e| e.is_object()).cloned()
            && let Some(obj) = e.as_object_mut()
        {
            obj.retain(|k, _| !["host", "path", "mode"].iter().any(|n| k.eq_ignore_ascii_case(n)));
            for k in ["host", "path", "mode"] {
                if let Some(x) = get(v, k) {
                    obj.insert(k.into(), x.clone());
                }
            }
            c = e;
        }
        let mode_name = Some(string(&c, "mode", "auto")).filter(|m| !m.is_empty()).unwrap_or_else(|| "auto".into());
        if !["auto", "packet-up", "stream-up", "stream-one"].contains(&mode_name.as_str()) {
            return Err(format!("unsupported mode: {mode_name}"));
        }
        if mode_name == "stream-one" && get(&c, "downloadSettings").is_some_and(|d| !d.is_null()) {
            return Err("xhttp: downloadSettings cannot be used with stream-one".into());
        }
        let mut headers = Vec::new();
        if let Some(m) = get(&c, "headers").and_then(Value::as_object) {
            for (k, v) in m {
                if k.eq_ignore_ascii_case("host") {
                    return Err(r#""headers" can't contain "host""#.into());
                }
                headers.push((canonical(k), v.as_str().unwrap_or("").to_owned()))
            }
        }
        let dp = string(&c, "uplinkDataPlacement", "body");
        let packet_up = mode_name == "packet-up";
        match dp.as_str() {
            "auto" | "body" => {}
            "cookie" | "header" if packet_up => {}
            "cookie" | "header" => return Err(format!("uplinkDataPlacement can be {dp} only in packet-up mode")),
            _ => return Err(format!("unsupported uplink data placement: {dp}")),
        }
        let method = string(&c, "uplinkHTTPMethod", "POST").to_ascii_uppercase();
        if method == "GET" && !packet_up {
            return Err("uplinkHTTPMethod can be GET only in packet-up mode".into());
        }
        let (session_place, seq_place) = (string(&c, "sessionIdPlacement", "path"), string(&c, "seqPlacement", "path"));
        for place in [&session_place, &seq_place] {
            if !["path", "cookie", "header", "query"].contains(&place.as_str()) {
                return Err(format!("unsupported session or seq placement: {place}"));
            }
        }
        let (pad_place, pad_method) = (string(&c, "xPaddingPlacement", "queryInHeader"), string(&c, "xPaddingMethod", "repeat-x"));
        if !["cookie", "header", "query", "queryInHeader"].contains(&pad_place.as_str()) {
            return Err(format!("unsupported padding placement: {pad_place}"));
        }
        if !["repeat-x", "tokenish"].contains(&pad_method.as_str()) {
            return Err(format!("unsupported padding method: {pad_method}"));
        }
        let session_table = predefined_table(&string(&c, "sessionIdTable", "")).to_owned();
        let session_length = capped(range(&c, "sessionIdLength", Range::default()), SESSION_ID_MAX);
        if !session_table.is_empty() {
            if session_room(session_table.len(), session_length) < 1 << 31 {
                return Err("sessionIdTable or sessionIdLength is too small".into());
            }
            if session_length.from <= 0 {
                return Err("sessionIdLength.from must be greater than 0".into());
            }
            if !session_table.is_ascii() {
                return Err("sessionIdTable must contain only ASCII characters".into());
            }
        }
        let pad = range(&c, "xPaddingBytes", PAD_DEFAULT);
        if pad != Range::default() && (pad.from <= 0 || pad.to <= 0) {
            return Err("xPaddingBytes cannot be disabled".into());
        }
        let pad = or_default(pad, PAD_DEFAULT);
        let mut xm = Xmux::default();
        if let Some(x) = get(&c, "xmux") {
            xm.max_concurrency = range(x, "maxConcurrency", Range::default());
            xm.max_connections = range(x, "maxConnections", Range::default());
            xm.c_max_reuse_times = range(x, "cMaxReuseTimes", Range::default());
            xm.h_max_request_times = range(x, "hMaxRequestTimes", Range::default());
            xm.h_max_reusable_secs = capped(range(x, "hMaxReusableSecs", Range::default()), SECS_MAX);
            xm.h_keep_alive_period = get(x, "hKeepAlivePeriod").and_then(Value::as_i64).unwrap_or(0).min(SECS_MAX)
        }
        if xm.max_connections.to > 0 && xm.max_concurrency.to > 0 {
            return Err("maxConnections cannot be specified together with maxConcurrency".into());
        }
        if xm == Xmux::default() {
            xm.max_connections = Range { from: 3, to: 3 };
            xm.h_max_request_times = Range { from: 600, to: 900 };
            xm.h_max_reusable_secs = Range { from: 1800, to: 3000 };
        }
        Ok(Config {
            mode_name,
            host_name: string(&c, "host", ""),
            req_host: String::new(),
            path: string(&c, "path", "/"),
            headers,
            method,
            no_grpc: boolv(&c, "noGRPCHeader"),
            data_place: dp.clone(),
            data_key: string(&c, "uplinkDataKey", if dp == "cookie" { "x_data" } else { "X-Data" }),
            chunk: capped(range(&c, "uplinkChunkSize", Range::default()), CHUNK_MAX),
            session_place,
            session_key: string(&c, "sessionIdKey", ""),
            session_table,
            session_length,
            seq_place,
            seq_key: string(&c, "seqKey", ""),
            pad_obfs: boolv(&c, "xPaddingObfsMode"),
            pad_key: string(&c, "xPaddingKey", "x_padding"),
            pad_header: string(&c, "xPaddingHeader", "X-Padding"),
            pad_place,
            pad_method,
            pad_bytes: capped(pad, PAD_MAX),
            max_post: capped(or_default(range(&c, "scMaxEachPostBytes", POST_DEFAULT), POST_DEFAULT), POST_MAX),
            min_interval: or_default(range(&c, "scMinPostsIntervalMs", INTERVAL_DEFAULT), INTERVAL_DEFAULT),
            xmux_config: xm,
            download: get(&c, "downloadSettings").filter(|d| !d.is_null()).cloned(),
        })
    }
    pub fn mode(&self, reality: bool) -> Mode {
        match self.mode_name.as_str() {
            "stream-one" => Mode::StreamOne,
            "stream-up" => Mode::StreamUp,
            "packet-up" => Mode::PacketUp,
            _ if reality && self.download.is_some() => Mode::StreamUp,
            _ if reality => Mode::StreamOne,
            _ => Mode::PacketUp,
        }
    }
    pub fn set_request_host(&mut self, host: &str) {
        self.req_host = host.to_string();
    }

    pub fn host(&self, s: &str, a: &str) -> String {
        if !self.host_name.is_empty() {
            self.host_name.clone()
        } else if !s.is_empty() {
            s.into()
        } else {
            a.into()
        }
    }
    pub fn xmux(&self) -> Xmux {
        self.xmux_config
    }
    pub(crate) fn golang_user_agent(&self) -> bool {
        crate::browser::get(&self.headers, "User-Agent") == Some("golang")
    }
    pub fn download_settings(&self) -> Option<&Value> {
        self.download.as_ref()
    }
    pub fn max_each_post_bytes(&self) -> Range {
        self.max_post
    }
    pub fn min_posts_interval_ms(&self) -> Range {
        self.min_interval
    }
    fn normalized_chunk(&self) -> Range {
        if self.chunk.to != 0 {
            if self.chunk.from < 64 { Range { from: 64, to: 64.max(self.chunk.to) } } else { self.chunk }
        } else {
            match self.data_place.as_str() {
                "header" => Range { from: 3000, to: 4000 },
                "cookie" => Range { from: 2048, to: 3072 },
                _ => self.max_post,
            }
        }
    }
    pub fn generate_session_id(&self) -> String {
        let n = if self.session_length.to > 0 { rand_range(self.session_length).max(0) as usize } else { 36 };
        let table = &self.session_table;
        if table.is_empty() {
            let mut b = [0; 16];
            let _ = SystemRandom::new().fill(&mut b);
            let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
            return format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32]);
        }
        random_text(n, table.as_bytes())
    }
    fn make(&self, session: Option<&str>, seq: Option<&str>, upload: bool, payload: Option<Vec<u8>>) -> Request {
        let packet = payload.is_some();
        let mut path = self.path.split('?').next().unwrap_or("").to_owned();
        if path.is_empty() || !path.starts_with('/') {
            path.insert(0, '/')
        };
        let mut query = self.path.split_once('?').map(|x| x.1.to_owned()).unwrap_or_default();
        if !path.ends_with('/') && (self.session_place == "path" || self.seq_place == "path") {
            path.push('/')
        }
        let mut h = self.headers.clone();
        default_fetch_headers(&mut h);
        let mut cookies: Vec<(String, String)> = Vec::new();
        let padlen = rand_range(self.pad_bytes).max(0) as usize;
        let pad = padding(padlen, &self.pad_method);
        let pp = if self.pad_obfs { self.pad_place.as_str() } else { "queryInHeader" };
        let pk = if self.pad_obfs { self.pad_key.as_str() } else { "x_padding" };
        let ph = if self.pad_obfs { self.pad_header.as_str() } else { "Referer" };
        match pp {
            "header" => set(&mut h, ph, &pad),
            "cookie" => cookies.push((pk.into(), pad)),
            "query" => {
                let mut tmp = format!("{path}?{query}");
                query_append(&mut tmp, pk, &pad);
                query = tmp.split_once('?').map(|x| x.1.to_owned()).unwrap_or_default()
            }
            "queryInHeader" => {
                let value = format!("https://{}{}?{}={}", self.req_host, path, pk, pad);
                set(&mut h, ph, &value)
            }
            _ => {}
        }
        let mut put_meta = |place: &str, key: &str, val: &str| match place {
            "path" => {
                if !path.ends_with('/') {
                    path.push('/');
                }
                path.push_str(val)
            }
            "query" => {
                let mut tmp = format!("{path}?{query}");
                query_append(&mut tmp, key, val);
                query = tmp.split_once('?').map(|x| x.1.to_owned()).unwrap_or_default()
            }
            "header" => set(&mut h, key, val),
            "cookie" => cookies.push((key.into(), val.into())),
            _ => {}
        };
        let sk = if self.session_key.is_empty() {
            match self.session_place.as_str() {
                "header" => "X-Session",
                "query" | "cookie" => "x_session",
                _ => "",
            }
        } else {
            &self.session_key
        };
        let qk = if self.seq_key.is_empty() {
            match self.seq_place.as_str() {
                "header" => "X-Seq",
                "query" | "cookie" => "x_seq",
                _ => "",
            }
        } else {
            &self.seq_key
        };
        if let Some(s) = session {
            put_meta(&self.session_place, sk, s)
        }
        if let Some(s) = seq {
            put_meta(&self.seq_place, qk, s)
        }
        let method = if packet || upload { self.method.clone() } else { "GET".into() };
        let mut body = None;
        if let Some(data) = payload {
            match self.data_place.as_str() {
                "header" => {
                    let encoded = URL_SAFE_NO_PAD.encode(&data);
                    let mut left = encoded.as_str();
                    let mut i = 0;
                    while !left.is_empty() {
                        let n = rand_range(self.normalized_chunk()).max(1) as usize;
                        let take = n.min(left.len());
                        set(&mut h, &format!("{}-{i}", self.data_key), &left[..take]);
                        left = &left[take..];
                        i += 1
                    }
                }
                "cookie" => {
                    let encoded = URL_SAFE_NO_PAD.encode(&data);
                    let mut left = encoded.as_str();
                    let mut i = 0;
                    while !left.is_empty() {
                        let n = rand_range(self.normalized_chunk()).max(1) as usize;
                        let take = n.min(left.len());
                        cookies.push((format!("{}_{}", self.data_key, i), left[..take].into()));
                        left = &left[take..];
                        i += 1
                    }
                }
                _ => body = Some(Bytes::from(data)),
            }
        }
        if !self.no_grpc && upload && !packet {
            set(&mut h, "Content-Type", "application/grpc")
        }
        if !cookies.is_empty() {
            set(&mut h, "Cookie", &cookies.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; "))
        }
        let mut full = path;
        if !query.is_empty() {
            full.push('?');
            full.push_str(&query)
        }
        h.sort_by(|a, b| a.0.cmp(&b.0));
        Request { method, path: full, headers: h, body }
    }
    pub fn stream_request(&self, s: Option<&str>, upload: bool) -> Request {
        self.make(s, None, upload, None)
    }
    pub fn packet_request(&self, s: &str, seq: u64, payload: Vec<u8>) -> Request {
        self.make(Some(s), Some(&seq.to_string()), true, Some(payload))
    }
}
fn set(h: &mut Vec<(String, String)>, k: &str, v: &str) {
    let key = canonical(k);
    if let Some(x) = h.iter_mut().find(|x| x.0.eq_ignore_ascii_case(&key)) {
        x.0 = key;
        x.1 = v.into()
    } else {
        h.push((key, v.into()))
    }
}
fn default_fetch_headers(h: &mut Vec<(String, String)>) {
    crate::browser::apply_defaults(h, crate::browser::Variant::Fetch);
}
pub fn rand_range(r: Range) -> i64 {
    if r.from >= r.to {
        return r.from;
    }
    let width = r.to.abs_diff(r.from).saturating_add(1);
    let mut b = [0; 8];
    if SystemRandom::new().fill(&mut b).is_err() {
        return r.from;
    }
    r.from.wrapping_add((u64::from_ne_bytes(b) % width) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn field_names_like_go_json() {
        let a = Config::from_json(&serde_json::json!({"mode": "packet-up", "sessionIDPlacement": "query", "sessionIDKey": "sid"})).unwrap();
        let b = Config::from_json(&serde_json::json!({"mode": "packet-up", "sessionIdPlacement": "query", "sessionIdKey": "sid"})).unwrap();
        assert_eq!(a.session_place, "query");
        assert_eq!(a.session_key, "sid");
        assert_eq!((a.session_place, a.session_key), (b.session_place, b.session_key));
    }
    #[test]
    fn huge_numbers_from_a_profile_are_capped() {
        let c = Config::from_json(&serde_json::json!({"xPaddingBytes": "1-9223372036854775807", "sessionIdLength": i64::MAX,
            "sessionIdTable": "Base62", "xmux": {"hMaxReusableSecs": i64::MAX, "hKeepAlivePeriod": i64::MAX}}))
        .unwrap();
        assert_eq!(c.xmux().h_max_reusable_secs, Range { from: SECS_MAX, to: SECS_MAX });
        assert_eq!(c.xmux().h_keep_alive_period, SECS_MAX);
        assert_eq!(c.pad_bytes, Range { from: 1, to: PAD_MAX });
        assert_eq!(c.generate_session_id().len(), SESSION_ID_MAX as usize);
        for r in [Range { from: 0, to: i64::MAX }, Range { from: i64::MIN, to: i64::MAX }, Range { from: -5, to: 5 }] {
            for _ in 0..100 {
                let v = rand_range(r);
                assert!(v >= r.from && v <= r.to, "{v} {r:?}");
            }
        }
    }
    #[test]
    fn post_and_chunk_sizes_from_a_profile_are_capped() {
        let c = Config::from_json(&serde_json::json!({"mode": "packet-up", "scMaxEachPostBytes": 4294967296i64,
            "uplinkChunkSize": 4294967296i64, "uplinkDataPlacement": "header"}))
        .unwrap();
        assert_eq!(c.max_each_post_bytes(), Range { from: POST_MAX, to: POST_MAX });
        assert_eq!(c.chunk, Range { from: CHUNK_MAX, to: CHUNK_MAX });
        let req = c.packet_request("sid", 0, vec![7; 300]);
        assert!(req.headers.len() < 64, "{} headers", req.headers.len());
    }
    #[test]
    fn zero_sizes_mean_defaults_like_xray() {
        for zero in [serde_json::json!(0), serde_json::json!("0"), serde_json::json!("0-0")] {
            let c =
                Config::from_json(&serde_json::json!({"xPaddingBytes": zero, "scMaxEachPostBytes": zero, "scMinPostsIntervalMs": zero}))
                    .unwrap();
            assert_eq!(c.pad_bytes, Range { from: 100, to: 1000 }, "{zero}");
            assert_eq!(c.max_each_post_bytes(), Range { from: 1_000_000, to: 1_000_000 }, "{zero}");
            assert_eq!(c.min_posts_interval_ms(), Range { from: 30, to: 30 }, "{zero}");
        }
        for off in [serde_json::json!("0-100"), serde_json::json!(-1)] {
            assert!(Config::from_json(&serde_json::json!({"xPaddingBytes": off})).is_err(), "{off}");
        }
    }
    #[test]
    fn empty_strings_mean_defaults_like_xray() {
        let mut v = serde_json::json!({"mode": "packet-up", "path": "/p"});
        for k in [
            "uplinkHTTPMethod",
            "uplinkDataPlacement",
            "uplinkDataKey",
            "sessionIdPlacement",
            "sessionIdKey",
            "seqPlacement",
            "seqKey",
            "xPaddingKey",
            "xPaddingHeader",
            "xPaddingPlacement",
            "xPaddingMethod",
        ] {
            v[k] = "".into();
        }
        let mut c = Config::from_json(&v).unwrap();
        c.set_request_host("h.example");
        let req = c.packet_request("sid", 7, vec![1, 2, 3]);
        assert_eq!(req.method, "POST");
        assert!(req.path.starts_with("/p/sid/7"), "{}", req.path);
        assert!(req.headers.iter().any(|(k, v)| k == "Referer" && v.contains("x_padding=X")), "{:?}", req.headers);
        assert_eq!(req.body.as_deref(), Some(&[1u8, 2, 3][..]));
    }
    #[test]
    fn auto_data_placement_sends_the_body_like_xray() {
        let mut c = Config::from_json(&serde_json::json!({"mode": "packet-up", "path": "/p", "uplinkDataPlacement": "auto"})).unwrap();
        c.set_request_host("h.example");
        let req = c.packet_request("sid", 0, vec![7; 10_000]);
        assert_eq!(req.body.as_deref(), Some(&[7u8; 10_000][..]));
        let header_bytes: usize = req.headers.iter().map(|(k, v)| k.len() + v.len()).sum();
        assert!(header_bytes < 2000, "{header_bytes} bytes of headers");
        assert_eq!(c.normalized_chunk(), c.max_each_post_bytes());
    }
    #[test]
    fn session_id_tables_like_xray() {
        for (name, table) in [
            ("hex", "0123456789abcdef"),
            ("HEX", "0123456789ABCDEF"),
            ("base36", "0123456789abcdefghijklmnopqrstuvwxyz"),
            ("Alphabet", "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"),
            ("Base62", "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"),
        ] {
            let c = Config::from_json(&serde_json::json!({"sessionIdTable": name, "sessionIdLength": 16})).unwrap();
            let id = c.generate_session_id();
            assert_eq!(id.len(), 16, "{name}");
            assert!(id.chars().all(|ch| table.contains(ch)), "{name}: {id}");
        }
        for bad in [
            serde_json::json!({"sessionIdTable": "hex", "sessionIdLength": 7}),
            serde_json::json!({"sessionIdTable": "Base62"}),
            serde_json::json!({"sessionIdTable": "ab", "sessionIdLength": "20-30"}),
            serde_json::json!({"sessionIdTable": "абвгдежзийклмнопрстуф", "sessionIdLength": 16}),
            serde_json::json!({"sessionIdTable": "Base62", "sessionIdLength": "0-16"}),
        ] {
            assert!(Config::from_json(&bad).is_err(), "{bad}");
        }
        assert!(Config::from_json(&serde_json::json!({"sessionIdTable": "hex", "sessionIdLength": 8})).is_ok());
    }
    #[test]
    fn settings_xray_refuses_are_refused() {
        use serde_json::json;
        for bad in [
            json!({"headers": {"Host": "h"}}),
            json!({"headers": {"host": "h"}}),
            json!({"uplinkHTTPMethod": "GET"}),
            json!({"mode": "stream-up", "uplinkHTTPMethod": "get"}),
            json!({"xPaddingPlacement": "bad"}),
            json!({"xPaddingMethod": "bad"}),
            json!({"uplinkDataPlacement": "bad"}),
            json!({"mode": "stream-up", "uplinkDataPlacement": "header"}),
            json!({"uplinkDataPlacement": "cookie"}),
            json!({"sessionIdPlacement": "bad"}),
            json!({"seqPlacement": "bad"}),
            json!({"xmux": {"maxConcurrency": 4, "maxConnections": 4}}),
            json!({"xmux": {"maxConcurrency": "16-32", "maxConnections": 4}}),
        ] {
            assert!(Config::from_json(&bad).is_err(), "{bad}");
        }
        for ok in [
            json!({"mode": "packet-up", "uplinkHTTPMethod": "GET", "uplinkDataPlacement": "header"}),
            json!({"mode": "packet-up", "uplinkDataPlacement": "cookie"}),
            json!({"xmux": {"maxConcurrency": 4, "maxConnections": 0}}),
            json!({"xPaddingPlacement": "header", "xPaddingMethod": "tokenish", "sessionIdPlacement": "cookie", "seqPlacement": "query",
                "uplinkDataPlacement": "auto"}),
            json!({"headers": {"DNT": "1"}, "uplinkHTTPMethod": "PUT"}),
        ] {
            assert!(Config::from_json(&ok).is_ok(), "{ok}");
        }
    }
    #[test]
    fn extra_overlay_like_xray() {
        let c = Config::from_json(&serde_json::json!({
            "path": "/out", "Extra": {"path": "/in", "host": "in.example", "mode": "stream-up", "xPaddingBytes": "10-20"}
        }))
        .unwrap();
        assert_eq!(c.path, "/out");
        assert_eq!(c.host_name, "");
        assert_eq!(c.mode_name, "auto");
        assert_eq!(c.pad_bytes, Range { from: 10, to: 20 });
        let d = Config::from_json(&serde_json::json!({"downloadSettings": null})).unwrap();
        assert!(d.download.is_none());
        let d = Config::from_json(&serde_json::json!({"downloadSettings": {"address": "x"}})).unwrap();
        assert_eq!(d.mode(false), Mode::PacketUp);
        assert_eq!(d.mode(true), Mode::StreamUp);
    }
    #[test]
    fn stream_one_rejects_download_settings_like_xray() {
        assert!(
            Config::from_json(&serde_json::json!({"mode": "stream-one", "downloadSettings": {"address": "localhost", "port": 443}}))
                .is_err()
        );
        assert!(Config::from_json(&serde_json::json!({"mode": "stream-one", "downloadSettings": null})).is_ok());
    }
    #[test]
    fn query_is_not_double_encoded() {
        let mut p = "/x?a=%2Fb&c=d+e".to_string();
        query_append(&mut p, "x_padding", "XX");
        assert_eq!(p, "/x?a=%2Fb&c=d+e&x_padding=XX");
    }
    #[test]
    fn random_text_terminates_for_power_of_two_alphabets() {
        for len in [1usize, 2, 16, 64, 62, 256] {
            let alphabet: Vec<u8> = (0..len).map(|i| b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ-_"[i % 64]).collect();
            assert_eq!(random_text(40, &alphabet).len(), 40, "alphabet of {len}");
        }
        assert_eq!(random_text(8, b""), "");
    }
    #[test]
    fn parses_ranges_and_numeric_values() {
        assert_eq!(Range::parse(&Value::String("16-32".into())).unwrap(), Range { from: 16, to: 32 });
        assert_eq!(Range::parse(&Value::from(7)).unwrap(), Range { from: 7, to: 7 });
        assert!(Range::parse(&Value::String("32-16".into())).is_err());
    }
    #[test]
    fn normalizes_post_defaults_and_extra_override() {
        let cfg = Config::from_json(&serde_json::json!({"mode":"auto","path":"/x", "extra":{"scMinPostsIntervalMs":"3-6"}})).unwrap();
        assert_eq!(cfg.min_posts_interval_ms(), Range { from: 3, to: 6 });
        assert_eq!(cfg.max_each_post_bytes(), Range { from: 1_000_000, to: 1_000_000 });
        assert_eq!(cfg.mode(true), Mode::StreamOne);
        assert_eq!(cfg.mode(false), Mode::PacketUp);
    }
    #[test]
    fn hpack_huffman_lengths_and_tokenish_padding_match_wire_length() {
        assert_eq!(huffman_len("X"), 1);
        assert_eq!(huffman_len("XX"), 2);
        for target in [128usize, 256, 768, 1024] {
            let value = padding(target, "tokenish");
            assert!(value.bytes().all(|b| b.is_ascii_alphanumeric()));
            assert!(padding_valid(&value, Range { from: target as i64, to: target as i64 }, "tokenish"));
        }
    }
    #[test]
    fn browser_fetch_defaults_match_xray_chrome_defaults() {
        let mut headers = Vec::new();
        default_fetch_headers(&mut headers);
        let get = |name: &str| headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str()).unwrap();
        let v = crate::browser::chrome_version();
        assert_eq!(get("User-Agent"), crate::browser::chrome_ua());
        assert!(get("User-Agent").contains(&format!("Chrome/{v}.0.0.0")));
        assert_eq!(get("Sec-CH-UA"), crate::browser::chrome_ch_ua(v, "chrome"));
        assert_eq!(get("Sec-Fetch-Mode"), "cors");
        assert_eq!(get("Sec-Fetch-Dest"), "empty");
        assert_eq!(get("Sec-Fetch-Site"), "same-origin");
        assert_eq!(get("Priority"), "u=1, i");
    }
}
