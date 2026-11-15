//! Typed errors for IVT I/O and parsing.
//!
//! Failures that reject a file are classified as corrupt or unsupported.
//! A file is corrupt when the evidence is at the file envelope: a bad
//! signature or fixed header word, or a pointer outside the file. A file is
//! unsupported when its envelope is valid but a record inside it uses a
//! layout this reader does not recognise. Messages lead with that verdict and
//! a plain explanation; the low-level detail follows after `Details:`.

use std::fmt;

#[derive(Debug)]
pub enum IvtError {
    Io(std::io::Error),
    UnexpectedEof {
        offset: u64,
        expected: usize,
        actual: usize,
    },
    InvalidPointer {
        offset: u64,
        length: usize,
        file_len: u64,
    },
    InvalidDirectory(&'static str),
    Corrupt {
        summary: String,
        detail: String,
    },
    Unsupported {
        summary: String,
        detail: String,
    },
    UnsupportedLayout(String),
    Parse(String),
}

/// Why a file was rejected, independent of the message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Io,
    Corrupt,
    Unsupported,
    Other,
}

pub(crate) const CORRUPT_PREFIX: &str = "Corrupt IVT file: ";
pub(crate) const UNSUPPORTED_PREFIX: &str = "Unsupported IVT file: ";

impl IvtError {
    pub fn corrupt(summary: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Corrupt {
            summary: summary.into(),
            detail: detail.into(),
        }
    }

    pub fn unsupported(summary: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Unsupported {
            summary: summary.into(),
            detail: detail.into(),
        }
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Io(_) => ErrorKind::Io,
            Self::UnexpectedEof { .. }
            | Self::InvalidPointer { .. }
            | Self::InvalidDirectory(_)
            | Self::Corrupt { .. } => ErrorKind::Corrupt,
            Self::Unsupported { .. } | Self::UnsupportedLayout(_) => ErrorKind::Unsupported,
            Self::Parse(_) => ErrorKind::Other,
        }
    }
}

fn verdict(f: &mut fmt::Formatter<'_>, prefix: &str, summary: &str, detail: &str) -> fmt::Result {
    write!(f, "{prefix}{summary}")?;
    if !detail.is_empty() {
        write!(f, ". Details: {detail}")?;
    }
    Ok(())
}

impl fmt::Display for IvtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::UnexpectedEof {
                offset,
                expected,
                actual,
            } => verdict(
                f,
                CORRUPT_PREFIX,
                &format!(
                    "the file appears truncated: a read at byte {offset} needs {expected} bytes but only {actual} remain"
                ),
                &format!("unexpected EOF at offset {offset}: expected {expected} bytes, got {actual}"),
            ),
            Self::InvalidPointer {
                offset,
                length,
                file_len,
            } => verdict(
                f,
                CORRUPT_PREFIX,
                &format!(
                    "a record points outside the file: it needs bytes {offset}..{} but the file is {file_len} bytes (likely an incomplete copy or a damaged directory)",
                    offset.saturating_add(*length as u64)
                ),
                &format!("invalid pointer offset={offset} length={length} file_len={file_len}"),
            ),
            Self::InvalidDirectory(what) => verdict(
                f,
                CORRUPT_PREFIX,
                &format!("a native directory is damaged or points outside the file ({what})"),
                &format!("invalid directory: {what}"),
            ),
            Self::Corrupt { summary, detail } => verdict(f, CORRUPT_PREFIX, summary, detail),
            Self::Unsupported { summary, detail } => {
                verdict(f, UNSUPPORTED_PREFIX, summary, detail)
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_leads_and_detail_follows() {
        let error = IvtError::corrupt("the file appears truncated", "slot=544");
        assert_eq!(error.kind(), ErrorKind::Corrupt);
        assert_eq!(
            error.to_string(),
            "Corrupt IVT file: the file appears truncated. Details: slot=544"
        );
        let error = IvtError::unsupported("descriptor layout not recognised", "");
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        assert_eq!(
            error.to_string(),
            "Unsupported IVT file: descriptor layout not recognised"
        );
        assert!(IvtError::InvalidDirectory("metadata directory range")
            .to_string()
            .starts_with(CORRUPT_PREFIX));
    }
}
