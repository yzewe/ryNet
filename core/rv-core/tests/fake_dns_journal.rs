#[test]
fn app_dns_query_cannot_forge_fake_dns_journal() {
    let dir = std::env::temp_dir().join(format!("rv-fake-forge-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fakedns");
    std::fs::write(&path, "5 bank.test\n").unwrap();
    rv_core::dns::fake::persist(&path);

    let mut q = vec![0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    q.push(10);
    q.extend_from_slice(b"x\n5 victim");
    q.push(4);
    q.extend_from_slice(b"test");
    q.extend_from_slice(&[0, 0, 1, 0, 1]);
    if let Some(question) = rv_core::dns::question(&q) {
        rv_core::dns::fake::ip_for(&question.name);
    }

    let journal = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert!(!journal.lines().any(|l| l == "5 victim.test"), "{journal:?}");
}
