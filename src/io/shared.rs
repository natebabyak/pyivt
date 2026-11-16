//! One reference-counted copy of in-memory IVT contents.

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

/// Immutable file contents shared by the metadata source and the reader.
///
/// The bytes are either owned (`Vec<u8>`) or borrowed from an owner that keeps
/// an immutable buffer alive, such as a Python `bytes` object.
#[derive(Clone)]
pub struct SharedBytes(Arc<dyn AsRef<[u8]> + Send + Sync>);

impl SharedBytes {
    /// Share the immutable buffer of `owner` without copying it.
    pub fn from_owner(owner: impl AsRef<[u8]> + Send + Sync + 'static) -> Self {
        Self(Arc::new(owner))
    }
}

impl Deref for SharedBytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        (*self.0).as_ref()
    }
}

impl AsRef<[u8]> for SharedBytes {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl From<Vec<u8>> for SharedBytes {
    fn from(data: Vec<u8>) -> Self {
        Self::from_owner(data)
    }
}

impl fmt::Debug for SharedBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedBytes")
            .field("len", &self.len())
            .finish()
    }
}
