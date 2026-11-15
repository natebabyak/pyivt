//! Typed errors for IVT I/O and parsing.
//!
//! Failures that reject a file are classified as corrupt or unsupported.
//! A file is corrupt when the evidence is at the file envelope: a bad
//! signature or fixed header word, or a pointer outside the file. A file is
//! unsupported when its envelope is valid but a record inside it uses a
//! layout this reader does not recognise. Messages lead with that verdict and
//! a plain explanation; the low-level detail follows after `Details:`.
//!
//! Failures found while decoding a record keep their original wording but
//! carry the same classification, so callers branch on `kind()` and read
//! structured `Context` fields, never on message text.

use std::fmt;

/// Structured location of a failure. Fields are `None` when not known.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Context {
    pub offset: Option<u64>,
    pub length: Option<u64>,
    pub file_len: Option<u64>,
    pub slot: Option<usize>,
    pub entry: Option<usize>,
    pub dimension: Option<usize>,
}

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
        context: Box<Context>,
    },
    Unsupported {
        summary: String,
        detail: String,
        context: Box<Context>,
    },
    UnsupportedLayout(String),
    /// A damaged record found while decoding: truncated, overflowing, or
    /// pointing outside its container.
    CorruptRecord {
        message: String,
        context: Box<Context>,
    },
    /// A record whose encoding this reader does not recognise.
    UnsupportedRecord {
        message: String,
        context: Box<Context>,
    },
    /// A memory estimate or allocation exceeded a safety limit.
    ResourceLimit {
        message: String,
        estimate: Option<u64>,
        limit: Option<u64>,
    },
    /// Invalid input that is not a property of the file, or an internal error.
    Parse(String),
}

/// Why a file was rejected, independent of the message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Io,
    Corrupt,
    Unsupported,
    ResourceLimit,
    Other,
}

pub(crate) const CORRUPT_PREFIX: &str = "Corrupt IVT file: ";
pub(crate) const UNSUPPORTED_PREFIX: &str = "Unsupported IVT file: ";

impl IvtError {
    pub fn corrupt(summary: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Corrupt {
            summary: summary.into(),
            detail: detail.into(),
            context: Box::default(),
        }
    }

    pub fn unsupported(summary: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Unsupported {
            summary: summary.into(),
            detail: detail.into(),
            context: Box::default(),
        }
    }

    pub fn corrupt_record(message: impl Into<String>) -> Self {
        Self::CorruptRecord {
            message: message.into(),
            context: Box::default(),
        }
    }

    pub fn unsupported_record(message: impl Into<String>) -> Self {
        Self::UnsupportedRecord {
            message: message.into(),
            context: Box::default(),
        }
    }

    pub fn resource_limit(
        message: impl Into<String>,
        estimate: Option<u64>,
        limit: Option<u64>,
    ) -> Self {
        Self::ResourceLimit {
            message: message.into(),
            estimate,
            limit,
        }
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Io(_) => ErrorKind::Io,
            Self::UnexpectedEof { .. }
            | Self::InvalidPointer { .. }
            | Self::InvalidDirectory(_)
            | Self::Corrupt { .. }
            | Self::CorruptRecord { .. } => ErrorKind::Corrupt,
            Self::Unsupported { .. }
            | Self::UnsupportedLayout(_)
            | Self::UnsupportedRecord { .. } => ErrorKind::Unsupported,
            Self::ResourceLimit { .. } => ErrorKind::ResourceLimit,
            Self::Parse(_) => ErrorKind::Other,
        }
    }

    /// Structured location fields, including those built into a variant.
    pub fn context(&self) -> Context {
        match self {
            Self::UnexpectedEof { offset, .. } => Context {
                offset: Some(*offset),
                ..Context::default()
            },
            Self::InvalidPointer {
                offset,
                length,
                file_len,
            } => Context {
                offset: Some(*offset),
                length: Some(*length as u64),
                file_len: Some(*file_len),
                ..Context::default()
            },
            Self::Corrupt { context, .. }
            | Self::Unsupported { context, .. }
            | Self::CorruptRecord { context, .. }
            | Self::UnsupportedRecord { context, .. } => **context,
            _ => Context::default(),
        }
    }

    /// Add location fields that are not already set. The innermost error knows
    /// the most precise location, so outer callers only fill in gaps.
    pub fn with(self, extra: Context) -> Self {
        let mut this = self.contextual();
        if let Self::Corrupt { context, .. }
        | Self::Unsupported { context, .. }
        | Self::CorruptRecord { context, .. }
        | Self::UnsupportedRecord { context, .. } = &mut this
        {
            context.offset = context.offset.or(extra.offset);
            context.length = context.length.or(extra.length);
            context.file_len = context.file_len.or(extra.file_len);
            context.slot = context.slot.or(extra.slot);
            context.entry = context.entry.or(extra.entry);
            context.dimension = context.dimension.or(extra.dimension);
        }
        this
    }

    /// Re-express a fixed-field corrupt variant as `Corrupt` so it can hold
    /// more context. The message is unchanged.
    fn contextual(self) -> Self {
        match self.verdict_parts() {
            Some((summary, detail)) => Self::Corrupt {
                summary,
                detail,
                context: Box::new(self.context()),
            },
            None => self,
        }
    }

    /// Summary and detail of the fixed-field corrupt variants.
    fn verdict_parts(&self) -> Option<(String, String)> {
        Some(match self {
            Self::UnexpectedEof {
                offset,
                expected,
                actual,
            } => (
                format!(
                    "the file appears truncated: a read at byte {offset} needs {expected} bytes but only {actual} remain"
                ),
                format!("unexpected EOF at offset {offset}: expected {expected} bytes, got {actual}"),
            ),
            Self::InvalidPointer {
                offset,
                length,
                file_len,
            } => (
                format!(
                    "a record points outside the file: it needs bytes {offset}..{} but the file is {file_len} bytes (likely an incomplete copy or a damaged directory)",
                    offset.saturating_add(*length as u64)
                ),
                format!("invalid pointer offset={offset} length={length} file_len={file_len}"),
            ),
            Self::InvalidDirectory(what) => (
                format!("a native directory is damaged or points outside the file ({what})"),
                format!("invalid directory: {what}"),
            ),
            _ => return None,
        })
    }

    /// Rewrite the message, for example to say where it happened, while keeping
    /// the classification and every structured field.
    pub fn reword(self, f: impl FnOnce(String) -> String) -> Self {
        let context = self.context();
        let kind = self.kind();
        match self {
            Self::Io(_) => self,
            Self::ResourceLimit {
                message,
                estimate,
                limit,
            } => Self::ResourceLimit {
                message: f(message),
                estimate,
                limit,
            },
            Self::Parse(message) => Self::Parse(f(message)),
            other => {
                let message = f(other.to_string());
                if kind == ErrorKind::Unsupported {
                    Self::UnsupportedRecord {
                        message,
                        context: Box::new(context),
                    }
                } else {
                    Self::CorruptRecord {
                        message,
                        context: Box::new(context),
                    }
                }
            }
        }
    }
}

/// Shorthand for a damaged record found while decoding.
pub(crate) fn corrupt_record(message: impl Into<String>) -> IvtError {
    IvtError::corrupt_record(message)
}

/// Shorthand for a record encoding this reader does not recognise.
pub(crate) fn unsupported_record(message: impl Into<String>) -> IvtError {
    IvtError::unsupported_record(message)
}

/// Shorthand for a safety limit without a known estimate.
pub(crate) fn limit_exceeded(message: impl Into<String>) -> IvtError {
    IvtError::resource_limit(message, None, None)
}

impl Context {
    pub fn offset(offset: impl TryInto<u64>) -> Self {
        Self {
            offset: offset.try_into().ok(),
            ..Self::default()
        }
    }

    pub fn entry(entry: usize) -> Self {
        Self {
            entry: Some(entry),
            ..Self::default()
        }
    }

    pub fn dimension(dimension: usize) -> Self {
        Self {
            dimension: Some(dimension),
            ..Self::default()
        }
    }

    pub fn slot(slot: usize) -> Self {
        Self {
            slot: Some(slot),
            ..Self::default()
        }
    }

    pub fn and_file_len(mut self, file_len: impl TryInto<u64>) -> Self {
        self.file_len = file_len.try_into().ok();
        self
    }

    pub fn and_offset(mut self, offset: impl TryInto<u64>) -> Self {
        self.offset = offset.try_into().ok();
        self
    }

    pub fn and_length(mut self, length: impl TryInto<u64>) -> Self {
        self.length = length.try_into().ok();
        self
    }

    pub fn and_slot(mut self, slot: usize) -> Self {
        self.slot = Some(slot);
        self
    }

    pub fn and_entry(mut self, entry: usize) -> Self {
        self.entry = Some(entry);
        self
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
            Self::UnexpectedEof { .. }
            | Self::InvalidPointer { .. }
            | Self::InvalidDirectory(_) => {
                let (summary, detail) = self.verdict_parts().unwrap();
                verdict(f, CORRUPT_PREFIX, &summary, &detail)
            }
            Self::Corrupt {
                summary, detail, ..
            } => verdict(f, CORRUPT_PREFIX, summary, detail),
            Self::Unsupported {
                summary, detail, ..
            } => verdict(f, UNSUPPORTED_PREFIX, summary, detail),
            Self::UnsupportedLayout(msg) => write!(f, "unsupported layout: {msg}"),
            Self::CorruptRecord { message, .. }
            | Self::UnsupportedRecord { message, .. }
            | Self::ResourceLimit { message, .. }
            | Self::Parse(message) => write!(f, "{message}"),
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

/// Arrow failures while assembling output are internal, not file faults.
impl From<arrow::error::ArrowError> for IvtError {
    fn from(e: arrow::error::ArrowError) -> Self {
        Self::Parse(e.to_string())
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

    #[test]
    fn reword_keeps_kind_and_context() {
        let error = IvtError::unsupported_record("unknown record 7")
            .with(Context::entry(3).and_offset(40u64))
            .reword(|m| format!("page: {m}"))
            .with(Context::offset(99u64).and_file_len(100u64));
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        assert_eq!(error.to_string(), "page: unknown record 7");
        let context = error.context();
        assert_eq!(context.entry, Some(3));
        assert_eq!(context.offset, Some(40));
        assert_eq!(context.file_len, Some(100));
        let error = IvtError::InvalidDirectory("metadata slot");
        let text = error.to_string();
        let error = error.with(Context::slot(824));
        assert_eq!(error.to_string(), text);
        assert_eq!(error.context().slot, Some(824));
        let error = error.reword(|m| format!("x: {m}"));
        assert_eq!(error.kind(), ErrorKind::Corrupt);
        let limit = IvtError::resource_limit("too big", Some(10), Some(5)).reword(|m| m + "!");
        assert_eq!(limit.kind(), ErrorKind::ResourceLimit);
        assert_eq!(limit.to_string(), "too big!");
    }
}
