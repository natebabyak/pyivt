//! File- or memory-backed IVT byte source.
//!
//! Reads are not cached here. Callers that follow metadata pointers plan their
//! reads with [`crate::io::PlannedReader`], which batches nearby ranges for one
//! parse and then drops them; records are rarely read twice.

use std::path::Path;
use std::sync::Mutex;

use crate::error::IvtError;
use crate::io::read_at::{FileReadAt, IoMetrics, ReadAt};
use crate::io::SharedBytes;

/// Byte source for metadata and (optionally) full-file decode.
pub enum IvtSource {
    Memory {
        data: SharedBytes,
        metrics: Mutex<IoMetrics>,
    },
    File {
        reader: FileReadAt,
        metrics: Mutex<IoMetrics>,
    },
}

impl IvtSource {
    pub fn from_bytes(data: impl Into<SharedBytes>) -> Self {
        Self::from_bytes_with_metrics(data, IoMetrics::default())
    }

    pub fn from_bytes_with_metrics(data: impl Into<SharedBytes>, metrics: IoMetrics) -> Self {
        Self::Memory {
            data: data.into(),
            metrics: Mutex::new(metrics),
        }
    }

    pub fn open_file(path: impl AsRef<Path>) -> Result<Self, IvtError> {
        let path = path.as_ref().to_path_buf();
        let reader = FileReadAt::open(&path)?;
        Ok(Self::File {
            reader,
            metrics: Mutex::new(IoMetrics::default()),
        })
    }

    pub fn len(&self) -> Result<u64, IvtError> {
        match self {
            Self::Memory { data, .. } => Ok(data.len() as u64),
            Self::File { reader, .. } => reader.len(),
        }
    }

    pub fn metrics(&self) -> IoMetrics {
        match self {
            Self::Memory { metrics, .. } | Self::File { metrics, .. } => {
                metrics.lock().unwrap().clone()
            }
        }
    }

    pub fn as_memory_slice(&self) -> Option<&[u8]> {
        match self {
            Self::Memory { data, .. } => Some(data.as_ref()),
            Self::File { .. } => None,
        }
    }

    /// Read exactly `buffer.len()` bytes at `offset`.
    pub fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> Result<(), IvtError> {
        let length = buffer.len();
        match self {
            Self::Memory { data, metrics } => {
                let file_len = data.len() as u64;
                let end = offset
                    .checked_add(length as u64)
                    .ok_or(IvtError::InvalidPointer {
                        offset,
                        length,
                        file_len,
                    })?;
                if end > file_len {
                    return Err(IvtError::InvalidPointer {
                        offset,
                        length,
                        file_len,
                    });
                }
                buffer.copy_from_slice(&data[offset as usize..offset as usize + length]);
                metrics.lock().unwrap().record(length);
                Ok(())
            }
            Self::File { reader, metrics } => {
                reader.read_exact_at(offset, buffer)?;
                metrics.lock().unwrap().record(length);
                Ok(())
            }
        }
    }
}

impl ReadAt for IvtSource {
    fn len(&self) -> Result<u64, IvtError> {
        IvtSource::len(self)
    }

    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> Result<(), IvtError> {
        IvtSource::read_exact_at(self, offset, buffer)
    }
}
