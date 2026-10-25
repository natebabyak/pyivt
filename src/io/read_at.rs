//! Positional readers with optional I/O metrics.

use std::fs::File;
use std::path::Path;
use std::sync::Mutex;

#[cfg(not(windows))]
use std::io::{Read, Seek, SeekFrom};

use crate::error::IvtError;

/// Counts bytes and operations for benchmarks; not part of the stable public API.
#[derive(Debug, Clone, Default)]
pub struct IoMetrics {
    pub bytes_read: u64,
    pub read_ops: u64,
    pub largest_range: usize,
    pub fallback_used: bool,
}

impl IoMetrics {
    pub fn record(&mut self, nbytes: usize) {
        self.bytes_read += nbytes as u64;
        self.read_ops += 1;
        self.largest_range = self.largest_range.max(nbytes);
    }
}

pub trait ReadAt {
    fn len(&self) -> Result<u64, IvtError>;
    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> Result<(), IvtError>;

    #[cfg(test)]
    fn read_range(&self, offset: u64, length: usize) -> Result<Vec<u8>, IvtError> {
        let mut buf = vec![0u8; length];
        self.read_exact_at(offset, &mut buf)?;
        Ok(buf)
    }
}

fn check_bounds(offset: u64, length: usize, file_len: u64) -> Result<(), IvtError> {
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
    Ok(())
}

/// In-memory byte source (tests and bytes-based open).
#[cfg(test)]
pub struct MemoryReadAt {
    data: Vec<u8>,
}

#[cfg(test)]
impl MemoryReadAt {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

#[cfg(test)]
impl ReadAt for MemoryReadAt {
    fn len(&self) -> Result<u64, IvtError> {
        Ok(self.data.len() as u64)
    }

    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> Result<(), IvtError> {
        let file_len = self.data.len() as u64;
        check_bounds(offset, buffer.len(), file_len)?;
        let start = offset as usize;
        let end = start + buffer.len();
        buffer.copy_from_slice(&self.data[start..end]);
        Ok(())
    }
}

/// File-backed positional reader.
pub struct FileReadAt {
    file: Mutex<File>,
    len: u64,
}

impl FileReadAt {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, IvtError> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        let len = file.metadata()?.len();
        Ok(Self {
            file: Mutex::new(file),
            len,
        })
    }
}

impl ReadAt for FileReadAt {
    fn len(&self) -> Result<u64, IvtError> {
        Ok(self.len)
    }

    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> Result<(), IvtError> {
        check_bounds(offset, buffer.len(), self.len)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            let file = self.file.lock().unwrap();
            let mut read = 0usize;
            while read < buffer.len() {
                let n = file
                    .seek_read(&mut buffer[read..], offset + read as u64)
                    .map_err(IvtError::Io)?;
                if n == 0 {
                    return Err(IvtError::UnexpectedEof {
                        offset: offset + read as u64,
                        expected: buffer.len() - read,
                        actual: 0,
                    });
                }
                read += n;
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let mut file = self.file.lock().unwrap();
            file.seek(SeekFrom::Start(offset))?;
            let mut read = 0usize;
            while read < buffer.len() {
                let n = file.read(&mut buffer[read..])?;
                if n == 0 {
                    return Err(IvtError::UnexpectedEof {
                        offset: offset + read as u64,
                        expected: buffer.len() - read,
                        actual: 0,
                    });
                }
                read += n;
            }
            Ok(())
        }
    }
}

/// Wraps a [`ReadAt`] and accumulates [`IoMetrics`].
#[cfg(test)]
pub struct InstrumentedReadAt<R: ReadAt> {
    inner: R,
    metrics: Mutex<IoMetrics>,
}

#[cfg(test)]
impl<R: ReadAt> InstrumentedReadAt<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            metrics: Mutex::new(IoMetrics::default()),
        }
    }

    pub fn metrics(&self) -> IoMetrics {
        self.metrics.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl<R: ReadAt> ReadAt for InstrumentedReadAt<R> {
    fn len(&self) -> Result<u64, IvtError> {
        self.inner.len()
    }

    fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> Result<(), IvtError> {
        self.inner.read_exact_at(offset, buffer)?;
        self.metrics.lock().unwrap().record(buffer.len());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_read_exact() {
        let m = MemoryReadAt::new(vec![10, 20, 30, 40, 50]);
        let mut buf = [0u8; 3];
        m.read_exact_at(1, &mut buf).unwrap();
        assert_eq!(buf, [20, 30, 40]);
    }

    #[test]
    fn memory_oob_errors() {
        let m = MemoryReadAt::new(vec![1, 2, 3]);
        let mut buf = [0u8; 2];
        assert!(matches!(
            m.read_exact_at(2, &mut buf),
            Err(IvtError::InvalidPointer { .. })
        ));
    }

    #[test]
    fn instrumented_counts() {
        let m = InstrumentedReadAt::new(MemoryReadAt::new(vec![0u8; 100]));
        let mut a = [0u8; 10];
        let mut b = [0u8; 20];
        m.read_exact_at(0, &mut a).unwrap();
        m.read_exact_at(10, &mut b).unwrap();
        let met = m.metrics();
        assert_eq!(met.bytes_read, 30);
        assert_eq!(met.read_ops, 2);
        assert_eq!(met.largest_range, 20);
    }
}
