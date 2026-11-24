#![no_main]

use libfuzzer_sys::fuzz_target;
use std::collections::HashMap;

fuzz_target!(|data: &[u8]| {
    let Ok(line) = std::str::from_utf8(data) else { return };
    let _ = rv_config::links::parse(line);
    if line.contains("://") {
        let _ = rv_config::parse_subscription(line);
    }
    let headers: HashMap<String, String> = ["profile-title", "subscription-userinfo", "profile-update-interval"]
        .into_iter()
        .map(|k| (k.to_string(), line.to_string()))
        .collect();
    let _ = rv_config::SubscriptionInfo::from_headers(&headers);
});
