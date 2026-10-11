//! The OLE compound file (CFB) container of a `.pub` file.
//!
//! Streams are read whole into memory, each capped at [`MAX_STREAM`] bytes, and
//! the compound file itself at [`MAX_INPUT`] bytes, so a hostile size field cannot
//! make the importer allocate without bound.

use std::io::{Cursor, Read};

use crate::IoError;

/// Largest input accepted (a Publisher file with many photos is tens of MB).
pub const MAX_INPUT: usize = 256 * 1024 * 1024;
/// Largest single stream read into memory.
pub const MAX_STREAM: u64 = 64 * 1024 * 1024;

/// An opened compound file.
pub struct Container {
    cf: cfb::CompoundFile<Cursor<Vec<u8>>>,
}

impl Container {
    /// Open a compound file from bytes. The bytes are copied once.
    pub fn open(bytes: &[u8]) -> Result<Self, IoError> {
        if bytes.len() > MAX_INPUT {
            return Err(IoError::Format(format!(
                "pub: file of {} bytes exceeds the {MAX_INPUT} byte limit",
                bytes.len()
            )));
        }
        let cf = cfb::CompoundFile::open(Cursor::new(bytes.to_vec()))
            .map_err(|e| IoError::Format(format!("pub: not an OLE compound file: {e}")))?;
        Ok(Self { cf })
    }

    /// Whether a stream exists at `path` (for example `Quill/QuillSub/CONTENTS`).
    pub fn has_stream(&self, path: &str) -> bool {
        self.cf.is_stream(path)
    }

    /// Read a whole stream. `None` when it does not exist or cannot be read.
    pub fn stream(&mut self, path: &str) -> Option<Vec<u8>> {
        if !self.cf.is_stream(path) {
            return None;
        }
        let s = self.cf.open_stream(path).ok()?;
        let mut out = Vec::new();
        s.take(MAX_STREAM).read_to_end(&mut out).ok()?;
        Some(out)
    }

    /// Paths of all streams, for the report.
    pub fn stream_paths(&self) -> Vec<String> {
        self.cf
            .walk()
            .filter(|e| e.is_stream())
            .map(|e| e.path().display().to_string())
            .collect()
    }
}
