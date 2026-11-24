use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Default)]
pub(crate) struct Health {
    pub alive: bool,
    pub delay: Duration,
    samples: Vec<Option<Duration>>,
    source: Option<bool>,
    updated: Option<std::time::Instant>,
}
impl Health {
    pub fn observe(&mut self, rtt: Option<Duration>) {
        self.record(rtt, false);
    }
    pub fn transport(&mut self, rtt: Option<Duration>) {
        self.record(rtt, true);
    }
    pub fn fresh(&self) -> bool {
        self.updated.is_some_and(|t| t.elapsed() < Duration::from_secs(300))
    }
    fn record(&mut self, rtt: Option<Duration>, transport: bool) {
        if self.source != Some(transport) {
            self.samples.clear();
            self.source = Some(transport);
        }
        self.updated = Some(std::time::Instant::now());
        self.alive = rtt.is_some();
        if let Some(rtt) = rtt {
            self.delay = rtt;
        }
        if self.samples.len() == 32 {
            self.samples.remove(0);
        }
        self.samples.push(rtt);
    }
    fn stats(&self) -> (f64, f64, f64) {
        let v: Vec<_> = self.samples.iter().flatten().map(Duration::as_secs_f64).collect();
        if v.len() < 2 {
            return (
                self.delay.as_secs_f64(),
                self.delay.as_secs_f64(),
                if self.samples.is_empty() { 1.0 } else { 1.0 - v.len() as f64 / self.samples.len() as f64 },
            );
        }
        let avg = v.iter().sum::<f64>() / v.len() as f64;
        let dev = (v.iter().map(|x| (x - avg).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
        (avg, dev, 1.0 - v.len() as f64 / self.samples.len() as f64)
    }
}

pub(crate) enum Strategy {
    Random,
    RoundRobin,
    LeastPing,
    LeastLoad(Value),
}
impl Strategy {
    pub fn parse(v: &Value) -> Result<Self, String> {
        match v.get("type").and_then(Value::as_str).unwrap_or("random").to_ascii_lowercase().as_str() {
            "" | "random" => Ok(Self::Random),
            "roundrobin" => Ok(Self::RoundRobin),
            "leastping" => Ok(Self::LeastPing),
            "leastload" => Ok(Self::LeastLoad(v.get("settings").cloned().unwrap_or(Value::Null))),
            t => Err(format!("balancer: unknown strategy {t}")),
        }
    }
    pub fn measured(&self) -> bool {
        matches!(self, Self::LeastPing | Self::LeastLoad(_))
    }
    pub fn select(&self, nodes: &[(&str, Health)]) -> Vec<usize> {
        match self {
            Self::Random | Self::RoundRobin => (0..nodes.len()).collect(),
            Self::LeastPing => nodes
                .iter()
                .enumerate()
                .filter(|(_, (_, h))| h.alive && h.fresh())
                .min_by_key(|(_, (_, h))| h.delay)
                .map(|(i, _)| vec![i])
                .unwrap_or_default(),
            Self::LeastLoad(s) => {
                let max = s.get("maxRTT").map(duration).unwrap_or(0.0);
                let tolerance = s.get("tolerance").and_then(Value::as_f64).unwrap_or(0.0).clamp(0.0, 1.0);
                let mut v: Vec<_> = nodes
                    .iter()
                    .enumerate()
                    .filter_map(|(i, (tag, h))| {
                        let (avg, dev, fail) = h.stats();
                        (h.alive && h.fresh() && (max <= 0.0 || h.delay.as_secs_f64() < max) && (tolerance <= 0.0 || fail <= tolerance))
                            .then(|| (i, dev * weight(s, tag).sqrt(), avg, *tag))
                    })
                    .collect();
                v.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)).then(a.3.cmp(b.3)));
                let expected = usize::try_from(s.get("expected").and_then(Value::as_i64).unwrap_or(0).max(0)).unwrap_or(usize::MAX);
                if expected > v.len() {
                    return v.into_iter().map(|n| n.0).collect();
                }
                let baselines: Vec<_> =
                    s.get("baselines").and_then(Value::as_array).into_iter().flatten().map(duration).filter(|b| *b > 0.0).collect();
                let mut count = if baselines.is_empty() { expected.max(1).min(v.len()) } else { 0 };
                for b in baselines {
                    while count < v.len() && v[count].1 < b {
                        count += 1;
                    }
                    if count >= expected.max(1) {
                        break;
                    }
                }
                if expected > 0 {
                    count = count.max(expected).min(v.len());
                }
                v.into_iter().take(count).map(|n| n.0).collect()
            }
        }
    }
}

pub(crate) fn duration(v: &Value) -> f64 {
    if let Some(n) = v.as_f64() {
        return n / 1e9;
    }
    let mut s = v.as_str().unwrap_or("");
    let mut total = 0.0;
    while !s.is_empty() {
        let n = s.find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-').unwrap_or(s.len());
        let Ok(value) = s[..n].parse::<f64>() else { return 0.0 };
        s = &s[n..];
        let Some((unit, scale)) = [("ns", 1e-9), ("us", 1e-6), ("µs", 1e-6), ("ms", 1e-3), ("s", 1.0), ("m", 60.0), ("h", 3600.0)]
            .into_iter()
            .find(|(u, _)| s.starts_with(u))
        else {
            return 0.0;
        };
        total += value * scale;
        s = &s[unit.len()..];
    }
    total
}
fn weight(s: &Value, tag: &str) -> f64 {
    for w in s.get("costs").and_then(Value::as_array).into_iter().flatten() {
        let find = w.get("match").and_then(Value::as_str).unwrap_or("");
        let matched = if w.get("regexp").and_then(Value::as_bool).unwrap_or(false) {
            regex_lite::Regex::new(find).ok().and_then(|r| r.find(tag).map(|m| m.as_str().to_string()))
        } else {
            tag.contains(find).then(|| find.to_string())
        };
        let Some(m) = matched.filter(|s| !s.is_empty()) else { continue };
        if let Some(v) = w.get("value").and_then(Value::as_f64).filter(|v| *v > 0.0) {
            return v;
        }
        return regex_lite::Regex::new(r"\d+(\.\d+)?").unwrap().find(&m).and_then(|m| m.as_str().parse().ok()).unwrap_or(1.0);
    }
    1.0
}
pub(crate) fn random(n: usize) -> usize {
    use ring::rand::SecureRandom;
    let mut b = [0; 8];
    let ceiling = u64::MAX - u64::MAX % n as u64;
    loop {
        ring::rand::SystemRandom::new().fill(&mut b).expect("balancer RNG");
        let x = u64::from_ne_bytes(b);
        if x < ceiling {
            return (x % n as u64) as usize;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn h(ms: u64) -> Health {
        let mut h = Health::default();
        h.observe(Some(Duration::from_millis(ms)));
        h
    }
    #[test]
    fn expected_past_usize_keeps_all_nodes() {
        let nodes = [("slow", h(90)), ("fast", h(20)), ("medium", h(40))];
        let s = Strategy::parse(&json!({"type": "leastLoad", "settings": {"expected": 4294967296i64}})).unwrap();
        assert_eq!(s.select(&nodes), vec![1, 2, 0]);
    }
    #[test]
    fn least_load_keeps_xray_expected_baselines_costs_and_max_rtt() {
        let nodes = [("slow", h(90)), ("fast", h(20)), ("medium", h(40))];
        let s = |settings| Strategy::parse(&json!({"type": "leastLoad", "settings": settings})).unwrap().select(&nodes);
        assert_eq!(s(json!({"expected": 2})), vec![1, 2]);
        assert_eq!(s(json!({"baselines": ["10ms"], "expected": 0})), Vec::<usize>::new());
        assert_eq!(s(json!({"baselines": ["50ms"], "expected": 0})), vec![1, 2]);
        assert_eq!(s(json!({"maxRTT": "40ms", "expected": 3})), vec![1]);
        assert_eq!(s(json!({"costs": [{"match": "fast", "value": 25}], "expected": 1})), vec![2]);
        let mut failed = h(10);
        failed.observe(None);
        failed.observe(Some(Duration::from_millis(10)));
        let s = Strategy::parse(&json!({"type": "leastLoad", "settings": {"tolerance": 0.2, "expected": 2}})).unwrap();
        assert_eq!(s.select(&[("flaky", failed), ("alive", h(20))]), vec![1]);
    }
}
