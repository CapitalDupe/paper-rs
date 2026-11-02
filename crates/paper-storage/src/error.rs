//! Storage errors.

use std::error::Error;
use std::fmt;
use std::io;

/// Errors produced while saving or loading chunks.
#[derive(Debug)]
pub enum StorageError {
    /// The file system failed.
    Io(io::Error),
    /// The data on disk is damaged or malformed.
    Corrupt(&'static str),
    /// The file holds a different chunk than the one requested.
    PosMismatch,
    /// The file uses a format version this build does not understand.
    Unsupported(u8),
    /// The chunk cannot be represented in the file format.
    Unencodable(&'static str),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "i/o error: {err}"),
            Self::Corrupt(why) => write!(f, "corrupt chunk data: {why}"),
            Self::PosMismatch => f.write_str("chunk file holds a different chunk"),
            Self::Unsupported(version) => write!(f, "unsupported chunk format version {version}"),
            Self::Unencodable(why) => write!(f, "cannot encode chunk: {why}"),
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for StorageError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}
