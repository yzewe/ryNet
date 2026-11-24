use super::*;

pub fn udp(data: &[u8]) {
    if let Ok(Some((from, payload, used))) = parse_udp(data) {
        assert!(used <= data.len());
        let start = used - payload.len();
        assert_eq!(payload.as_ptr(), data[start..].as_ptr());
        assert_eq!(&data[start - 2..start], b"\r\n");
        assert_eq!(start, if from.is_ipv4() { 1 + 4 + 6 } else { 1 + 16 + 6 });
        assert_eq!(u16::from_be_bytes([data[start - 4], data[start - 3]]) as usize, payload.len());
        let k = data.len() % used;
        assert!(matches!(parse_udp(&data[..k]), Ok(None)));
    }
}
