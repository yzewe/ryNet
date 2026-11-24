use rv_net::{DialOptions, FragmentConfig, FragmentStream, dial_tcp};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn hello() -> Vec<u8> {
    let payload = [vec![1], vec![0x55; 599]].concat();
    let mut b = vec![22, 3, 1];
    b.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    b.extend(payload);
    b
}

#[tokio::test]
async fn fragment_tls_and_passthrough_after_first_write() {
    let (a, mut b) = tokio::io::duplex(4096);
    let mut w = FragmentStream::new(a, FragmentConfig::tls_hello((1, 80), (0, 0)));
    let src = hello();
    w.write_all(&src).await.unwrap();
    let tail = b"later bytes";
    w.write_all(tail).await.unwrap();
    w.shutdown().await.unwrap();
    let mut got = Vec::new();
    b.read_to_end(&mut got).await.unwrap();
    let mut at = 0;
    let mut payload = Vec::new();
    let mut records = 0;
    while at < got.len() && records < 100 && payload.len() < src.len() - 5 {
        assert_eq!(got[at], 22);
        assert_eq!(&got[at + 1..at + 3], &[3, 1]);
        let n = u16::from_be_bytes([got[at + 3], got[at + 4]]) as usize;
        if at + 5 + n > got.len() {
            break;
        }
        assert!((1..=80).contains(&n));
        payload.extend_from_slice(&got[at + 5..at + 5 + n]);
        at += 5 + n;
        records += 1;
    }
    assert!(records > 1);
    assert_eq!(payload, src[5..]);
    assert_eq!(&got[at..], tail);
}

#[tokio::test]
async fn first_non_hello_is_unchanged() {
    let (a, mut b) = tokio::io::duplex(128);
    let mut w = FragmentStream::new(a, FragmentConfig::tls_hello((5, 10), (0, 0)));
    w.write_all(b"plain").await.unwrap();
    w.shutdown().await.unwrap();
    let mut got = Vec::new();
    b.read_to_end(&mut got).await.unwrap();
    assert_eq!(got, b"plain");
}

#[tokio::test]
async fn dial_and_protect_hook() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let called = Arc::new(AtomicUsize::new(0));
    let seen = called.clone();
    let opts = DialOptions {
        protect: Some(Arc::new(move |fd| {
            assert!(fd >= 0);
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })),
        ..Default::default()
    };
    let client = dial_tcp("127.0.0.1", port, &opts).await.unwrap();
    let (_server, _) = listener.accept().await.unwrap();
    drop(client);
    assert_eq!(called.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn protect_error_fails_dial() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let opts = DialOptions {
        protect: Some(Arc::new(|_| Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied")))),
        ..Default::default()
    };
    assert_eq!(dial_tcp("127.0.0.1", port, &opts).await.unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn explicit_tcp_keepalive_reaches_the_socket() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let opts = rv_net::DialOptions {
        tcp_keepalive: Some((std::time::Duration::from_secs(45), std::time::Duration::from_secs(30))),
        ..Default::default()
    };
    let stream = rv_net::dial_tcp("127.0.0.1", listener.local_addr().unwrap().port(), &opts).await.unwrap();
    let sock = socket2::SockRef::from(&stream);
    assert!(sock.keepalive().unwrap());
    assert_eq!(sock.tcp_keepalive_time().unwrap(), std::time::Duration::from_secs(45));
    assert_eq!(sock.tcp_keepalive_interval().unwrap(), std::time::Duration::from_secs(30));
}
