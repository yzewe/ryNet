#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let Ok(profiles) = rv_config::parse_subscription(text) else { return };
    for p in &profiles {
        let _ = p.main_outbound();
        for m in p.balancer_members() {
            let _ = p.member(&m.tag);
        }
        for o in &p.outbounds {
            let _ = o.kind.server();
            let _ = rv_config::find_profile(&profiles, &format!("{}{}{}", p.remarks, rv_config::MEMBER_SEP, o.tag));
        }
    }
});
