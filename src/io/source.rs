//! File- or memory-backed IVT byte source with a small range cache.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::error::IvtError;
use crate::io::planner::{coalesce_ranges, ByteRange, LoadedRange, DEFAULT_MERGE_GAP};
use crate::io::read_at::{FileReadAt, IoMetrics, MemoryReadAt, ReadAt};

const CACHE_CAP: usize = 32;

#[derive(Debug)]
struct RangeCache {
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
        path: PathBuf,
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

    pub fn from_arc(data: Arc<[u8]>) -> Self {
        Self::Memory {
            data,
            metrics: Mutex::new(IoMetrics::default()),
        }
    }

    pub fn open_file(path: impl AsRef<Path>) -> Result<Self, IvtError> {
        let path = path.as_ref().to_path_buf();
        let reader = FileReadAt::open(&path)?;
        Ok(Self::File {
            path,
            reader,
            cache: Mutex::new(RangeCache::new()),
            metrics: Mutex::new(IoMetrics::default()),
        })
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Memory { .. } => None,
            Self::File { path, .. } => Some(path.as_path()),
        }
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

    pub fn set_fallback_used(&self) {
        match self {
            Self::Memory { metrics, .. } | Self::File { metrics, .. } => {
                metrics.lock().unwrap().fallback_used = true;
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

    /// Plan, coalesce, and load ranges into the cache; return copies.
    pub fn load_ranges(
        &self,
        ranges: &[ByteRange],
        merge_gap: u64,
    ) -> Result<Vec<LoadedRange>, IvtError> {
        let file_len = self.len()?;
        let merged = coalesce_ranges(ranges, merge_gap, file_len)?;
        let mut out = Vec::with_capacity(merged.len());
        for r in merged {
            // Serve from cache when fully covered.
            if let Self::File { cache, .. } = self {
                let cache = cache.lock().unwrap();
                if let Some(s) = cache.find(r.offset, r.length) {
                    out.push(LoadedRange {
                        offset: r.offset,
                        data: s.to_vec(),
                    });
                    continue;
                }
            }
            if let Self::Memory { data, metrics } = self {
                let end = r.offset as usize + r.length;
                let slice = &data[r.offset as usize..end];
                metrics.lock().unwrap().record(r.length);
                out.push(LoadedRange {
                    offset: r.offset,
                    data: slice.to_vec(),
                });
                continue;
            }
            let data = match self {
                Self::File {
                    reader,
                    cache,
                    metrics,
                    ..
                } => {
                    let data = reader.read_range(r.offset, r.length)?;
                    metrics.lock().unwrap().record(r.length);
                    cache.lock().unwrap().insert(LoadedRange {
                        offset: r.offset,
                        data: data.clone(),
                    });
                    data
                }
                Self::Memory { .. } => unreachable!(),
            };
            out.push(LoadedRange {
                offset: r.offset,
                data,
            });
        }
        Ok(out)
    }

    pub fn load_ranges_default(&self, ranges: &[ByteRange]) -> Result<Vec<LoadedRange>, IvtError> {
        self.load_ranges(ranges, DEFAULT_MERGE_GAP)
    }

    /// Read the entire file (Arrow decode / compatibility fallback).
    pub fn read_all(&self) -> Result<Vec<u8>, IvtError> {
        let len = self.len()?;
        if len > usize::MAX as u64 {
            return Err(IvtError::Parse(
                "IVT file too large to load into memory".into(),
            ));
        }
        self.read_range(0, len as usize)
    }

    /// Ensure a contiguous prefix of the file is available as a Vec (header region).
    pub fn read_prefix(&self, nbytes: usize) -> Result<Vec<u8>, IvtError> {
        let len = self.len()? as usize;
        let n = nbytes.min(len);
        self.read_range(0, n)
    }
}

/// Convenience: open a path for positional reads without wrapping in IvtSource.
#[allow(dead_code)]
pub fn open_file_reader(path: impl AsRef<Path>) -> Result<FileReadAt, IvtError> {
    FileReadAt::open(path)
}

/// Convenience for tests.
#[allow(dead_code)]
pub fn memory_reader(data: Vec<u8>) -> MemoryReadAt {
    MemoryReadAt::new(data)
}

// Keep File in scope for Windows FileExt path documentation / future use.
#[allow(dead_code)]
fn _file_type_anchor(_: &File) {}
