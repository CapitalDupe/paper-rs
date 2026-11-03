//! Crash-safe chunk persistence for the Magna MC server prototype.
//!
//! Each chunk is one file in a versioned, checksummed format. Writes go to a
//! temporary file that is flushed to disk and then atomically renamed over the
//! destination, so a crash leaves either the old chunk or the new one, never a
//! torn mix. Loading validates everything and never trusts lengths from disk.
//!
//! This is the server's own format, not Minecraft's region format. Importing and
//! exporting vanilla worlds is a separate, later piece of work.

pub mod codec;
pub mod crc;
pub mod error;
pub mod store;

pub use error::StorageError;
pub use store::ChunkStore;
