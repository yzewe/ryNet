use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rv_xhttp::request::{Config, Range};
use serde_json::Value;
use std::{collections::BTreeMap, fs};

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn pair(s: &str) -> (i64, i64) {
    let (a, b) = s.split_once('-').unwrap();
    (a.parse().unwrap(), b.parse().unwrap())
}
fn hex(s: &str) -> Vec<u8> {
    s.as_bytes().as_chunks::<2>().0.iter().map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap()).collect()
}
fn class(s: &str) -> &'static str {
    if s.bytes().all(|b| b.is_ascii_digit()) {
        "digits"
    } else if s.bytes().all(|b| b.is_ascii_lowercase()) {
        "lower"
    } else if s.bytes().all(|b| b.is_ascii_uppercase()) {
        "upper"
    } else if s.bytes().all(|b| b.is_ascii_alphanumeric()) {
        "alnum"
    } else {
        "mixed"
    }
}
fn normalized_path(p: &str, sid_len: usize) -> String {
    p.split('/')
        .map(|part| {
            let uuid = part.len() == 36 && part.as_bytes().get(8) == Some(&b'-');
            let table = part.len() == sid_len && !part.bytes().all(|b| b.is_ascii_digit());
            if uuid || table { "<sid>" } else { part }
        })
        .collect::<Vec<_>>()
        .join("/")
}
fn main_headers(v: &Value) -> BTreeMap<String, String> {
    v.as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect::<Vec<_>>().join(", ")))
        .collect()
}

#[test]
fn generated_requests_match_xray_reference() {
    let data: Value = serde_json::from_slice(&fs::read("tests/fixtures/xray_requests.json").unwrap()).unwrap();
    assert!(!data.as_array().unwrap().is_empty());
    for f in data.as_array().unwrap() {
        let settings = &f["xhttpSettings"];
        let mut c = Config::from_json(settings).unwrap();
        c.set_request_host(s(f, "host"));
        let mode = |reality| format!("{:?}", c.mode(reality));
        assert_eq!(
            mode(true),
            match s(f, "modeReality") {
                "stream-one" => "StreamOne",
                "stream-up" => "StreamUp",
                _ => "PacketUp",
            }
        );
        assert_eq!(
            mode(false),
            match s(f, "modeTLS") {
                "stream-one" => "StreamOne",
                "stream-up" => "StreamUp",
                _ => "PacketUp",
            }
        );
        assert_eq!(c.host(s(f, "serverName"), ""), s(f, "host"));
        for (key, got) in [("scMaxEachPostBytes", c.max_each_post_bytes()), ("scMinPostsIntervalMs", c.min_posts_interval_ms())] {
            let (a, b) = pair(s(f, key));
            assert_eq!(got, Range { from: a, to: b }, "{key}");
        }
        let xm = c.xmux();
        for (key, got) in [
            ("maxConcurrency", xm.max_concurrency),
            ("maxConnections", xm.max_connections),
            ("cMaxReuseTimes", xm.c_max_reuse_times),
            ("hMaxRequestTimes", xm.h_max_request_times),
            ("hMaxReusableSecs", xm.h_max_reusable_secs),
        ] {
            let x = &f["xmux"][key];
            assert_eq!((got.from, got.to), (x["from"].as_i64().unwrap_or(0), x["to"].as_i64().unwrap_or(0)), "xmux {key}");
        }
        assert_eq!(xm.h_keep_alive_period, f["xmux"]["hKeepAlivePeriod"].as_i64().unwrap_or(0));
        let sid = c.generate_session_id();
        for sample in f["samples"].as_array().unwrap() {
            let kind = s(sample, "kind");
            let n = sample["payloadLen"].as_u64().unwrap_or(0) as usize;
            let payload: Vec<u8> = (0..n).map(|j| (j % 251) as u8).collect();
            let req = match kind {
                "stream-one" => c.stream_request(None, true),
                "stream-down" => c.stream_request(Some(&sid), false),
                "packet" => c.packet_request(&sid, s(sample, "seq").parse().unwrap(), payload.clone()),
                _ => panic!("kind {kind}"),
            };
            assert_eq!(req.method, s(sample, "method"), "method {kind}");
            let seq_place = s(&settings["extra"], "seqPlacement");
            if kind == "packet" && (seq_place.is_empty() || seq_place == "path") {
                assert!(req.path.split('?').next().unwrap().ends_with(&format!("/{}", s(sample, "seq"))), "seq in path");
            }
            assert_eq!(
                normalized_path(req.path.split('?').next().unwrap_or(&req.path), sid.len()),
                normalized_path(sample["path"].as_str().unwrap().split('?').next().unwrap(), sid.len()),
                "path structure {kind}"
            );
            let expected = main_headers(&sample["headers"]);
            let actual: BTreeMap<String, String> = req.headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.clone())).collect();
            let dk = format!("{}-", s(&settings["extra"], "uplinkDataKey").to_ascii_lowercase());
            let names = |m: &BTreeMap<String, String>| m.keys().filter(|k| dk == "-" || !k.starts_with(&dk)).cloned().collect::<Vec<_>>();
            assert_eq!(names(&actual), names(&expected), "header names {kind}");
            let data_key = s(&settings["extra"], "uplinkDataKey").to_ascii_lowercase();
            for (key, xv) in &expected {
                let ov = &actual[key];
                let pad_header = {
                    let h = s(&settings["extra"], "xPaddingHeader");
                    if h.is_empty() { "referer".to_string() } else { h.to_ascii_lowercase() }
                };
                if *key == pad_header || key == "referer" {
                    let key_name = {
                        let k = s(&settings["extra"], "xPaddingKey");
                        if k.is_empty() { "x_padding".to_string() } else { k.to_string() }
                    };
                    let marker = format!("{key_name}=");
                    let marker = marker.as_str();
                    let (xp, xpad) = xv.split_once(marker).unwrap();
                    let (op, opad) = ov.split_once(marker).unwrap();
                    assert_eq!(op, xp, "padding URL prefix");
                    let xp = xpad.split('&').next().unwrap();
                    let op = opad.split('&').next().unwrap();
                    let bounds = pair(s(f, "xPaddingBytes"));
                    let token = s(&settings["extra"], "xPaddingMethod") == "tokenish";
                    let size = if token { rv_xhttp::request::huffman_len(op) as i64 } else { op.len() as i64 };
                    let xsize = if token { rv_xhttp::request::huffman_len(xp) as i64 } else { xp.len() as i64 };
                    let ok = |n: i64| {
                        if token { (bounds.0 - 2..=bounds.1 + 2).contains(&n) } else { (bounds.0..=bounds.1).contains(&n) }
                    };
                    assert!(ok(xsize), "xray padding length {xsize}");
                    assert!(ok(size), "our padding length {size}");
                    assert_eq!(class(op), class(xp), "padding character class");
                } else if !data_key.is_empty() && key.starts_with(&format!("{data_key}-")) {
                } else if key == "sec-ch-ua" || key == "user-agent" {
                    let ver = |ua: &str| -> i64 {
                        ua.split("Chrome/").nth(1).and_then(|r| r.split('.').next()).and_then(|v| v.parse().ok()).unwrap_or(0)
                    };
                    let xv_ver = ver(&expected["user-agent"]);
                    let ov_ver = ver(&actual["user-agent"]);
                    if key == "sec-ch-ua" {
                        assert_eq!(*xv, rv_xhttp::browser::chrome_ch_ua(xv_ver, "chrome"), "xray sec-ch-ua shape");
                        assert_eq!(*ov, rv_xhttp::browser::chrome_ch_ua(ov_ver, "chrome"), "our sec-ch-ua shape");
                    } else {
                        assert_eq!(ov.replace(&ov_ver.to_string(), "N"), xv.replace(&xv_ver.to_string(), "N"), "user-agent shape");
                    }
                } else {
                    assert_eq!(ov, xv, "header {key} {kind}");
                }
            }
            let want = sample["bodyHex"].as_str().map(hex).unwrap_or_default();
            let got = req.body.unwrap_or_default();
            if !want.is_empty() && kind != "stream-one" {
                assert_eq!(got, want, "body {kind}");
            }
            let place = s(&settings["extra"], "uplinkDataPlacement");
            if kind == "packet" && matches!(place, "header" | "auto" | "cookie") {
                let data_key = s(&settings["extra"], "uplinkDataKey");
                let mut chunks: Vec<(usize, String)> = actual
                    .iter()
                    .filter_map(|(k, v)| {
                        k.strip_prefix(&data_key.to_ascii_lowercase())
                            .and_then(|x| x.strip_prefix('-'))
                            .and_then(|i| i.parse().ok())
                            .map(|i| (i, v.clone()))
                    })
                    .collect();
                chunks.sort_by_key(|x| x.0);
                let encoded = chunks.iter().map(|x| x.1.as_str()).collect::<String>();
                assert_eq!(URL_SAFE_NO_PAD.decode(encoded).unwrap(), payload, "encoded payload");
                let chunk = pair(s(f, "uplinkChunkSize"));
                for (_, v) in chunks.iter().take(chunks.len().saturating_sub(1)) {
                    assert!((chunk.0 as usize..=chunk.1 as usize).contains(&v.len()), "chunk size");
                }
            }
        }
    }
}
