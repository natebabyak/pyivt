//! File- or memory-backed IVT byte source with a small range cache.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::error::IvtError;
use crate::io::planner::LoadedRange;
use crate::io::read_at::{FileReadAt, IoMetrics, ReadAt};

const CACHE_CAP: usize = 32;

#[derive(Debug)]
pub(crate) struct RangeCache {
    entries: Vec<LoadedRange>,
}

impl RangeCache {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn find(&self, offset: u64, length: usize) -> Option<&[u8]> {
        for e in &self.entries {
            if let Some(s) = e.slice_at(offset, length) {
                return Some(s);
            }
        }
        None
    }

    fn insert(&mut self, lr: LoadedRange) {
        // Prefer covering existing entries that are subsets.
        self.entries
            .retain(|e| !(lr.offset <= e.offset && lr.end() >= e.end()));
        if self.entries.len() >= CACHE_CAP {
            self.entries.remove(0);
        }
        self.entries.push(lr);
    }
}

/// Byte source for metadata and (optionally) full-file decode.
pub enum IvtSource {
    Memory {
        data: Arc<[u8]>,
        metrics: Mutex<IoMetrics>,
    },
    File {
        reader: FileReadAt,
        cache: Mutex<RangeCache>,
        metrics: Mutex<IoMetrics>,
    },
}

impl IvtSource {
    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self::from_bytes_with_metrics(data, IoMetrics::default())
    }

    pub fn from_bytes_with_metrics(data: Vec<u8>, metrics: IoMetrics) -> Self {
        Self::Memory {
            data: Arc::from(data.into_boxed_slice()),
            metrics: Mutex::new(metrics),
        }
    }

    pub fn open_file(path: impl AsRef<Path>) -> Result<Self, IvtError> {
        let path = path.as_ref().to_path_buf();
        let reader = FileReadAt::open(&path)?;
        Ok(Self::File {
            reader,
            cache: Mutex::new(RangeCache::new()),
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

    pub fn memory_arc(&self) -> Option<Arc<[u8]>> {
        match self {
            Self::Memory { data, .. } => Some(Arc::clone(data)),
            Self::File { .. } => None,
        }
    }

    /// Read exactly `length` bytes at `offset`, using the cache when possible.
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
            Self::File {
                reader,
                cache,
                metrics,
                ..
            } => {
                {
                    let cache = cache.lock().unwrap();
                    if let Some(s) = cache.find(offset, length) {
                        buffer.copy_from_slice(s);
                        return Ok(());
                    }
                }
                reader.read_exact_at(offset, buffer)?;
                metrics.lock().unwrap().record(length);
                let mut cache = cache.lock().unwrap();
                cache.insert(LoadedRange {
                    offset,
                    data: buffer.to_vec(),
                });
                Ok(())
            }
        }
    }

    pub fn read_range(&self, offset: u64, length: usize) -> Result<Vec<u8>, IvtError> {
        let mut buf = vec![0u8; length];
        self.read_exact_at(offset, &mut buf)?;
        Ok(buf)
    }

    /// Ensure a contiguous prefix of the file is available as a Vec (header region).
    pub fn read_prefix(&self, nbytes: usize) -> Result<Vec<u8>, IvtError> {
        let len = self.len()? as usize;
        let n = nbytes.min(len);
        self.read_range(0, n)
    }
}
