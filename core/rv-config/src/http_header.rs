use serde_json::Value;

pub type HeaderChoices = Vec<(String, Vec<String>)>;

#[derive(Debug, Clone)]
pub struct HttpHeader {
    pub request_version: String,
    pub method: String,
    pub paths: Vec<String>,
    pub request_headers: HeaderChoices,
    pub response_version: String,
    pub status: String,
    pub reason: String,
    pub response_headers: HeaderChoices,
}

fn text(v: &Value, key: &str, default: &str) -> Result<String, String> {
    match v.get(key).filter(|v| !v.is_null()) {
        None => Ok(default.into()),
        Some(Value::String(s)) if s.is_empty() => Ok(default.into()),
        Some(Value::String(s)) if !s.contains(['\r', '\n', '\0']) => Ok(s.clone()),
        _ => Err(format!("tcp http header: invalid {key}")),
    }
}

fn list(v: &Value) -> Result<Vec<String>, String> {
    match v {
        Value::String(s) if !s.contains(['\r', '\n', '\0']) => Ok(vec![s.clone()]),
        Value::Array(a) => a
            .iter()
            .map(|v| match v {
                Value::String(s) if !s.contains(['\r', '\n', '\0']) => Ok(s.clone()),
                _ => Err("tcp http header: invalid string list".into()),
            })
            .collect(),
        _ => Err("tcp http header: invalid string list".into()),
    }
}

fn headers(v: &Value) -> Result<HeaderChoices, String> {
    let Some(v) = v.get("headers").filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let map = v.as_object().ok_or("tcp http header: headers must be an object")?;
    let mut out = Vec::new();
    for (k, v) in map {
        if k.is_empty() || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)) {
            return Err("tcp http header: invalid header name".into());
        }
        out.push((k.clone(), list(v)?));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

impl HttpHeader {
    pub fn from_json(v: &Value) -> Result<Self, String> {
        let request = &v["request"];
        let response = &v["response"];
        if [request, response].iter().any(|v| !v.is_null() && !v.is_object()) {
            return Err("tcp http header: request/response must be objects".into());
        }
        let mut paths = request.get("path").filter(|v| !v.is_null()).map(list).transpose()?.unwrap_or_default();
        if paths.is_empty() {
            paths.push("/".into());
        }
        if paths.iter().any(|p| p.is_empty() || p.bytes().any(|b| b <= b' ' || b == 127)) {
            return Err("tcp http header: invalid request path".into());
        }
        let method = text(request, "method", "GET")?;
        if !method.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)) {
            return Err("tcp http header: invalid request method".into());
        }
        Ok(Self {
            request_version: text(request, "version", "1.1")?,
            method,
            paths,
            request_headers: headers(request)?,
            response_version: text(response, "version", "1.1")?,
            status: text(response, "status", "200")?,
            reason: text(response, "reason", "OK")?,
            response_headers: headers(response)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn xray_defaults_and_request_response_choices_are_preserved() {
        let h = HttpHeader::from_json(&json!({})).unwrap();
        assert_eq!((&h.method[..], &h.request_version[..], &h.status[..], &h.reason[..]), ("GET", "1.1", "200", "OK"));
        assert_eq!(h.paths, ["/"]);
        assert!(h.request_headers.is_empty());
        assert!(h.response_headers.is_empty());
        let h = HttpHeader::from_json(&json!({"request": {"version": "1.0", "method": "POST", "path": ["/one", "/two"], "headers": {"z-last": [], "Host": "test.local"}},
            "response": {"version": "1.0", "status": "201", "reason": "Created", "headers": {"X-Test": ["one", "two"]}}})).unwrap();
        assert_eq!(h.paths, ["/one", "/two"]);
        assert_eq!(h.request_headers[0], ("Host".into(), vec!["test.local".into()]));
        assert_eq!(h.request_headers[1], ("z-last".into(), Vec::new()));
        assert_eq!((&h.response_version[..], &h.status[..], &h.reason[..]), ("1.0", "201", "Created"));
        assert_eq!(h.response_headers[0].1, ["one", "two"]);
        for v in [
            json!({"request": {"headers": {"Host": null}}}),
            json!({"request": {"path": "/bad\r\n"}}),
            json!({"request": {"method": "GET bad"}}),
        ] {
            assert!(HttpHeader::from_json(&v).is_err());
        }
    }
}
