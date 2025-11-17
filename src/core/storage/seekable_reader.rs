use bytes::Bytes;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
use tokio::runtime::Handle;

use super::traits::StorageBackend;
use crate::error::Result;

/// A seekable reader that uses range requests to read from remote storage
///
/// This allows FileReader (which requires Read + Seek) to efficiently read
/// only the necessary parts of a remote file (e.g., just the footer for schema).
pub struct SeekableReader {
    backend: Arc<dyn StorageBackend>,
    path: String,
    file_size: u64,
    position: u64,
    buffer: Option<Bytes>,
    buffer_start: u64,
    handle: Handle,
}

impl SeekableReader {
    /// Create a new seekable reader
    pub async fn new(backend: Arc<dyn StorageBackend>, path: String) -> Result<Self> {
        // Get file size
        let metadata = backend.head(&path).await?;
        let file_size = metadata.size;

        // Get handle to current runtime (we're already in an async context)
        let handle = Handle::current();

        Ok(Self {
            backend,
            path,
            file_size,
            position: 0,
            buffer: None,
            buffer_start: 0,
            handle,
        })
    }

    /// Read data from current position, fetching via range request if needed
    fn read_range(&mut self, len: usize) -> Result<Bytes> {
        let start = self.position;
        let end = (start + len as u64).min(self.file_size);

        // Check if we have this data in buffer
        if let Some(ref buf) = self.buffer {
            let buf_end = self.buffer_start + buf.len() as u64;
            if start >= self.buffer_start && end <= buf_end {
                // Data is in buffer
                let offset = (start - self.buffer_start) as usize;
                let len = (end - start) as usize;
                return Ok(buf.slice(offset..offset + len));
            }
        }

        // Need to fetch from storage
        // Fetch a larger chunk (64KB) to reduce number of requests
        const CHUNK_SIZE: u64 = 64 * 1024;
        let fetch_end = (start + CHUNK_SIZE).min(self.file_size);

        // Use block_in_place to run async code from sync context
        // This is safe because we're in a multi-threaded runtime
        let backend = self.backend.clone();
        let path = self.path.clone();
        let bytes = tokio::task::block_in_place(|| {
            self.handle
                .block_on(backend.get_range(&path, start, fetch_end))
        })?;

        // Cache the fetched chunk
        self.buffer = Some(bytes.clone());
        self.buffer_start = start;

        // Return the requested portion
        let len = (end - start) as usize;
        Ok(bytes.slice(0..len))
    }
}

impl Read for SeekableReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.position >= self.file_size {
            return Ok(0); // EOF
        }

        let bytes = self
            .read_range(buf.len())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e)))?;

        let len = bytes.len();
        buf[..len].copy_from_slice(&bytes);
        self.position += len as u64;
        Ok(len)
    }
}

impl Seek for SeekableReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(offset) => offset as i64,
            SeekFrom::End(offset) => self.file_size as i64 + offset,
            SeekFrom::Current(offset) => self.position as i64 + offset,
        };

        if new_pos < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Cannot seek to negative position",
            ));
        }

        self.position = new_pos as u64;
        Ok(self.position)
    }
}
