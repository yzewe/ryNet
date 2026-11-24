#![no_std]

pub const API_VERSION: u32 = 1;
pub const IMPORT_MODULE: &str = "rynet";
pub const MANIFEST_SECTION: &str = "rynet.manifest";

pub const FILE_MAGIC: &[u8; 7] = b"\x89RNP\r\n\x1a";
pub const FILE_VERSION: u8 = 1;
pub const FILE_EXTENSION: &str = "rnp";
pub const FILE_MIME: &str = "application/vnd.rynet.plugin";

#[derive(Debug, PartialEq, Eq)]
pub enum File<'a> {
    Module(&'a [u8]),
    Newer(u8),
    Unknown,
}

pub fn open_file(bytes: &[u8]) -> File<'_> {
    const WASM: &[u8] = b"\0asm";
    match bytes.strip_prefix(FILE_MAGIC.as_slice()).map(|r| r.split_first()) {
        Some(Some((&FILE_VERSION, module))) if module.starts_with(WASM) => File::Module(module),
        Some(Some((&v, _))) if v > FILE_VERSION => File::Newer(v),
        Some(_) => File::Unknown,
        None if bytes.starts_with(WASM) => File::Module(bytes),
        None => File::Unknown,
    }
}

pub mod err {
    pub const AGAIN: i32 = -1;
    pub const CLOSED: i32 = -2;
    pub const INVALID: i32 = -3;
    pub const DENIED: i32 = -4;
    pub const LIMIT: i32 = -5;
    pub const FAILED: i32 = -6;
    pub const UNSUPPORTED: i32 = -7;
    pub const PENDING: i32 = -8;
    pub const TOO_SMALL: i32 = -9;
    pub const TIMEOUT: i32 = -10;
    pub const AUTH: i32 = -11;
}

pub mod ev {
    pub const OPEN: u32 = 1;
    pub const READABLE: u32 = 2;
    pub const WRITABLE: u32 = 3;
    pub const DONE: u32 = 4;
    pub const TIMER: u32 = 5;
    pub const CLOSED: u32 = 6;
    pub const STOP: u32 = 7;
}

pub mod conn {
    pub const TCP: u32 = 1;
    pub const UDP: u32 = 2;
    pub const STREAM_TRANSPORT: u32 = 3;
    pub const DATAGRAM_TRANSPORT: u32 = 4;
}

pub mod open {
    pub const TCP: u32 = 1;
    pub const UDP: u32 = 2;
    pub const TLS: u32 = 3;
    pub const TRANSPORT: u32 = 4;
    pub const QUIC: u32 = 5;
    pub const QUIC_BI: u32 = 6;
    pub const QUIC_UNI: u32 = 7;
    pub const H2: u32 = 8;
    pub const H2_REQUEST: u32 = 9;
    pub const HTTP: u32 = 10;
    pub const RESOLVE: u32 = 11;
    pub const TUN: u32 = 12;
    pub const QUIC_ACCEPT_BI: u32 = 13;
    pub const QUIC_ACCEPT_UNI: u32 = 14;
}

pub mod hash {
    pub const SHA1: u32 = 1;
    pub const SHA256: u32 = 2;
    pub const SHA384: u32 = 3;
    pub const SHA512: u32 = 4;
    pub const BLAKE2S256: u32 = 5;
    pub const BLAKE2B512: u32 = 6;
    pub const BLAKE3: u32 = 7;
    pub const MD5: u32 = 8;
}

pub mod mac {
    pub const HMAC_SHA1: u32 = 1;
    pub const HMAC_SHA256: u32 = 2;
    pub const HMAC_SHA384: u32 = 3;
    pub const HMAC_SHA512: u32 = 4;
    pub const HMAC_MD5: u32 = 5;
    pub const HMAC_BLAKE2S: u32 = 6;
    pub const BLAKE2S_KEYED_128: u32 = 7;
    pub const BLAKE2S_KEYED_256: u32 = 8;
    pub const BLAKE3_KEYED: u32 = 9;
}

pub mod sig {
    pub const ECDSA_P256_SHA256: u32 = 1;
    pub const ECDSA_P384_SHA384: u32 = 2;
    pub const ED25519: u32 = 3;
    pub const RSA_PKCS1_SHA256: u32 = 4;
    pub const RSA_PSS_SHA256: u32 = 5;
}

pub mod aead {
    pub const AES128_GCM: u32 = 1;
    pub const AES256_GCM: u32 = 2;
    pub const CHACHA20_POLY1305: u32 = 3;
    pub const XCHACHA20_POLY1305: u32 = 4;
}

pub mod cipher {
    pub const AES128_CTR: u32 = 1;
    pub const AES256_CTR: u32 = 2;
    pub const CHACHA20: u32 = 3;
    pub const AES128_ECB: u32 = 4;
    pub const AES256_ECB: u32 = 5;
    pub const AES128_ECB_DECRYPT: u32 = 6;
    pub const AES256_ECB_DECRYPT: u32 = 7;
    pub const XCHACHA20: u32 = 8;
}

pub mod log {
    pub const ERROR: u32 = 1;
    pub const WARN: u32 = 2;
    pub const INFO: u32 = 3;
    pub const DEBUG: u32 = 4;
}

pub fn err_name(code: i32) -> &'static str {
    match code {
        err::AGAIN => "again",
        err::CLOSED => "closed",
        err::INVALID => "invalid",
        err::DENIED => "denied",
        err::LIMIT => "limit",
        err::FAILED => "failed",
        err::UNSUPPORTED => "unsupported",
        err::PENDING => "pending",
        err::TOO_SMALL => "too small",
        err::TIMEOUT => "timeout",
        err::AUTH => "auth",
        c if c >= 0 => "ok",
        _ => "unknown",
    }
}
