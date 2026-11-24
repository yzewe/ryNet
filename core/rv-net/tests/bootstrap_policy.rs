#[tokio::test]
async fn auto_falls_back_to_system_dns_but_explicit_provider_does_not() {
    let protect: rv_net::Protect =
        std::sync::Arc::new(|_| Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "blocked test DoH socket")));
    rv_net::bootstrap::configure("auto").unwrap();
    let auto = rv_net::resolve_protected("localhost", Some(&protect)).await;
    rv_net::bootstrap::configure("cloudflare").unwrap();
    let explicit = rv_net::resolve_protected("localhost", Some(&protect)).await;
    rv_net::bootstrap::configure("system").unwrap();
    assert!(auto.unwrap().iter().any(|ip| ip.is_loopback()), "Auto must fall back to the network DNS");
    assert!(explicit.is_err(), "explicit DoH leaked through the system resolver: {explicit:?}");
}
