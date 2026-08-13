//! Buffered regular-file operations for opaque handles.

use super::*;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom};
use std::path::Path;

impl HostFile {
    fn can_read(&self) -> bool {
        matches!(self.mode, FileMode::Read | FileMode::ReadWrite)
    }

    fn can_write(&self) -> bool {
        matches!(
            self.mode,
            FileMode::Write | FileMode::Append | FileMode::ReadWrite
        )
    }

    fn prepare_read(&mut self) -> io::Result<()> {
        if !self.can_read() {
            return Err(not_readable_error());
        }
        self.flush_write_buffer()
    }

    fn prepare_write(&mut self) -> io::Result<()> {
        if !self.can_write() {
            return Err(not_writable_error());
        }
        let unread = self.read_buffer.len().saturating_sub(self.read_cursor);
        if unread > 0 {
            let unread = i64::try_from(unread)
                .map_err(|_too_large| io::Error::other("read buffer exceeded seek range"))?;
            self.file.seek(SeekFrom::Current(-unread))?;
        }
        self.read_buffer.clear();
        self.read_cursor = 0;
        Ok(())
    }

    fn fill_read_buffer(&mut self) -> io::Result<usize> {
        self.prepare_read()?;
        let capacity = self.buffering.capacity()?;
        self.read_buffer.resize(capacity, 0);
        let read = self.file.read(&mut self.read_buffer)?;
        self.read_buffer.truncate(read);
        self.read_cursor = 0;
        Ok(read)
    }

    pub(super) fn read_line(&mut self) -> io::Result<String> {
        let mut bytes = Vec::new();
        loop {
            if self.read_cursor == self.read_buffer.len() && self.fill_read_buffer()? == 0 {
                break;
            }
            let Some(remaining) = self.read_buffer.get(self.read_cursor..) else {
                solveig_base::ice!("file read cursor exceeded its buffer")
            };
            if let Some(newline) = remaining.iter().position(|byte| *byte == b'\n') {
                let Some(line) = remaining.get(..newline) else {
                    solveig_base::ice!("newline position exceeded the file read buffer")
                };
                bytes.extend_from_slice(line);
                self.read_cursor += newline + 1;
                if bytes.last() == Some(&b'\r') {
                    bytes.pop();
                }
                break;
            }
            bytes.extend_from_slice(remaining);
            self.read_cursor = self.read_buffer.len();
        }
        String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub(super) fn read_contents(&mut self) -> io::Result<String> {
        self.prepare_read()?;
        let Some(remaining) = self.read_buffer.get(self.read_cursor..) else {
            solveig_base::ice!("file read cursor exceeded its buffer")
        };
        let mut bytes = remaining.to_vec();
        self.read_buffer.clear();
        self.read_cursor = 0;
        self.file.read_to_end(&mut bytes)?;
        String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub(super) fn read_bytes(&mut self, maximum: i64) -> io::Result<Vec<u8>> {
        let maximum = byte_count(maximum)?;
        self.prepare_read()?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(maximum).map_err(|error| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                format!("byte read is too large: {error}"),
            )
        })?;
        while bytes.len() < maximum {
            if self.read_cursor == self.read_buffer.len() && self.fill_read_buffer()? == 0 {
                break;
            }
            let Some(remaining) = self.read_buffer.get(self.read_cursor..) else {
                solveig_base::ice!("file read cursor exceeded its buffer")
            };
            let take = remaining.len().min(maximum - bytes.len());
            let Some(chunk) = remaining.get(..take) else {
                solveig_base::ice!("bounded byte read exceeded the file read buffer")
            };
            bytes.extend_from_slice(chunk);
            self.read_cursor += take;
        }
        Ok(bytes)
    }

    pub(super) fn is_eof(&mut self) -> io::Result<bool> {
        self.prepare_read()?;
        if self.read_cursor < self.read_buffer.len() {
            return Ok(false);
        }
        Ok(self.fill_read_buffer()? == 0)
    }

    pub(super) fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.prepare_write()?;
        self.write_buffer.extend_from_slice(bytes);
        if self.buffering.flushes(&self.write_buffer)? {
            self.flush_write_buffer()?;
        }
        Ok(())
    }

    pub(super) fn flush(&mut self) -> io::Result<()> {
        self.prepare_write_if_needed()?;
        self.file.flush()
    }

    pub(super) fn set_buffering(&mut self, buffering: Buffering) -> io::Result<()> {
        self.flush_write_buffer()?;
        let unread = self.read_buffer.len().saturating_sub(self.read_cursor);
        if unread > 0 {
            let unread = i64::try_from(unread)
                .map_err(|_too_large| io::Error::other("read buffer exceeded seek range"))?;
            self.file.seek(SeekFrom::Current(-unread))?;
        }
        self.read_buffer.clear();
        self.read_cursor = 0;
        self.buffering = buffering;
        Ok(())
    }

    fn prepare_write_if_needed(&mut self) -> io::Result<()> {
        if self.can_write() {
            self.flush_write_buffer()
        } else {
            Ok(())
        }
    }

    fn flush_write_buffer(&mut self) -> io::Result<()> {
        if !self.write_buffer.is_empty() {
            self.file.write_all(&self.write_buffer)?;
            self.write_buffer.clear();
        }
        Ok(())
    }
}

pub(super) fn open_host_file(path: &Path, mode: FileMode) -> io::Result<File> {
    let mut options = OpenOptions::new();
    match mode {
        FileMode::Read => options.read(true),
        FileMode::Write => options.write(true).create(true).truncate(true),
        FileMode::Append => options.append(true).create(true),
        FileMode::ReadWrite => options.read(true).write(true),
    };
    options.open(path)
}

fn byte_count(maximum: i64) -> io::Result<usize> {
    usize::try_from(maximum).map_err(|_negative| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "hGetBytes maximum must be non-negative and fit the host address space",
        )
    })
}
