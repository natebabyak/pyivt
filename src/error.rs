//! Typed errors for IVT I/O and parsing.

use std::fmt;

#[derive(Debug)]
pub enum IvtError {
    Io(std::io::Error),
    UnexpectedEof {
        offset: u64,
        expected: usize,
        actual: usize,
    },
    InvalidSignature([u8; 4]),
    InvalidPointer {
        offset: u64,
        length: usize,
        file_len: u64,
    },
    InvalidDirectory(&'static str),
    DescriptorNotFound,
    UnsupportedLayout(String),
    Parse(String),
}

impl fmt::Display for IvtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::UnexpectedEof {
                offset,
                expected,
                actual,
            } => write!(
                f,
                "unexpected EOF at offset {offset}: expected {expected} bytes, got {actual}"
            ),
            Self::InvalidSignature(sig) => write!(
                f,
                "unrecognised IVT signature: {:02x}{:02x}{:02x}{:02x}",
                sig[0], sig[1], sig[2], sig[3]
            ),
            Self::InvalidPointer {
                offset,
                length,
                file_len,
            } => write!(
                f,
                "invalid pointer offset={offset} length={length} file_len={file_len}"
            ),
            Self::InvalidDirectory(what) => write!(f, "invalid directory: {what}"),
            Self::DescriptorNotFound => write!(f, "unsupported IVT descriptor: native axis matrix is missing or invalid; unverified descriptor fallback is disabled"),
            Self::UnsupportedLayout(msg) => write!(f, "unsupported layout: {msg}"),
            Self::Parse(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for IvtError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for IvtError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<IvtError> for String {
    fn from(e: IvtError) -> Self {
        e.to_string()
    }
}
