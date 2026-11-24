use std::io;

pub fn ipv6(ipv4_only: bool, block: impl FnOnce() -> io::Result<()>, setup: impl FnOnce() -> io::Result<()>) -> io::Result<bool> {
    if ipv4_only {
        block()?;
        Ok(false)
    } else {
        setup()?;
        Ok(true)
    }
}

pub const ADAPTER_TRIES: u32 = 10;

pub fn retry_pause(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_millis((250u64 << attempt.min(3)).min(2000))
}

pub fn ready(mut check: impl FnMut() -> io::Result<()>, mut pause: impl FnMut()) -> io::Result<()> {
    for attempt in 0..80 {
        match check() {
            Ok(()) => return Ok(()),
            Err(e) if matches!(e.raw_os_error(), Some(2 | 1168)) && attempt < 79 => pause(),
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interface_registration_is_bounded() {
        let mut checks = 0;
        let mut sleeps = 0;
        ready(
            || {
                checks += 1;
                if checks < 4 { Err(io::Error::from_raw_os_error(1168)) } else { Ok(()) }
            },
            || sleeps += 1,
        )
        .unwrap();
        assert_eq!((checks, sleeps), (4, 3));
        checks = 0;
        assert!(
            ready(
                || {
                    checks += 1;
                    Err(io::Error::from_raw_os_error(1168))
                },
                || {}
            )
            .is_err()
        );
        assert_eq!(checks, 80);
        checks = 0;
        assert!(
            ready(
                || {
                    checks += 1;
                    Err(io::Error::from_raw_os_error(5))
                },
                || {}
            )
            .is_err()
        );
        assert_eq!(checks, 1);
    }
    #[test]
    fn adapter_tries_outlast_a_device_being_removed() {
        let total: std::time::Duration = (0..ADAPTER_TRIES - 1).map(retry_pause).sum();
        assert!(total >= std::time::Duration::from_secs(12), "{total:?}");
        assert!(total <= std::time::Duration::from_secs(20), "{total:?}");
        assert_eq!(retry_pause(0), std::time::Duration::from_millis(250));
        assert_eq!(retry_pause(9), std::time::Duration::from_secs(2));
    }

    #[test]
    fn ipv4_only_requires_working_ipv6_block() {
        assert!(!ipv6(true, || Ok(()), || panic!("IPv6 must not be configured")).unwrap());
        assert!(ipv6(true, || Err(io::ErrorKind::PermissionDenied.into()), || panic!("must fail closed")).is_err());
        assert!(ipv6(false, || panic!("not IPv4-only"), || Ok(())).unwrap());
        assert!(ipv6(false, || Ok(()), || Err(io::ErrorKind::NotFound.into())).is_err());
    }
}
