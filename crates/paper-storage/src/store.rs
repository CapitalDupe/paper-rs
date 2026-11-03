//! A directory of chunk files with crash-safe writes.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use paper_world::chunk::Chunk;
use paper_world::pos::ChunkPos;

use crate::codec::{decode_chunk, encode_chunk};
use crate::error::StorageError;

/// Largest chunk file that will be read.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Stores chunks as individual files under one directory.
///
/// Saves are atomic: the data is written to a temporary file, flushed to disk,
/// and renamed over the destination, then the directory is flushed. A crash at
/// any point leaves the previous chunk intact. Leftover temporary files are
/// ignored by loads and overwritten by the next save. Callers must not save the
/// same chunk from two threads at once.
#[derive(Debug, Clone)]
pub struct ChunkStore {
    root: PathBuf,
}

impl ChunkStore {
    /// Opens (creating if needed) a store rooted at `root`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// The store's directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, pos: ChunkPos) -> PathBuf {
        self.root.join(format!("c.{}.{}.mgchunk", pos.x, pos.z))
    }

    /// Whether a chunk file exists for `pos`.
    pub fn contains(&self, pos: ChunkPos) -> bool {
        self.path(pos).is_file()
    }

    /// Loads the chunk at `pos`, or `None` if it has never been saved.
    pub fn load(&self, pos: ChunkPos) -> Result<Option<Chunk>, StorageError> {
        let path = self.path(pos);
        let meta = match fs::metadata(&path) {
            Ok(meta) => meta,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        if meta.len() > MAX_FILE_BYTES {
            return Err(StorageError::Corrupt("chunk file too large"));
        }
        let bytes = fs::read(&path)?;
        decode_chunk(&bytes, pos).map(Some)
    }

    /// Saves a chunk atomically.
    pub fn save(&self, chunk: &Chunk) -> Result<(), StorageError> {
        let bytes = encode_chunk(chunk)?;
        let path = self.path(chunk.pos());
        let tmp = path.with_extension("mgchunk.tmp");
        if let Err(err) = write_durably(&tmp, &bytes) {
            let _ = fs::remove_file(&tmp);
            return Err(err.into());
        }
        if let Err(err) = fs::rename(&tmp, &path) {
            let _ = fs::remove_file(&tmp);
            return Err(err.into());
        }
        if let Ok(dir) = File::open(&self.root) {
            let _ = dir.sync_all();
        }
        Ok(())
    }
}

fn write_durably(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use paper_world::block::BlockStateId;
    use paper_world::chunk::WorldHeight;

    use super::*;

    fn chunk_with(pos: ChunkPos, id: u32) -> Chunk {
        let mut chunk = Chunk::new(pos, WorldHeight::default());
        chunk.set_block(3, 70, 4, BlockStateId(id));
        chunk.set_block(0, -64, 0, BlockStateId(id + 1));
        chunk
    }

    #[test]
    fn saves_and_loads_a_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChunkStore::open(dir.path()).unwrap();
        let pos = ChunkPos::new(-5, 9);
        let chunk = chunk_with(pos, 10);
        assert!(!store.contains(pos));
        store.save(&chunk).unwrap();
        assert!(store.contains(pos));
        assert_eq!(store.load(pos).unwrap(), Some(chunk));
    }

    #[test]
    fn missing_chunks_load_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChunkStore::open(dir.path()).unwrap();
        assert_eq!(store.load(ChunkPos::new(0, 0)).unwrap(), None);
    }

    #[test]
    fn the_latest_save_wins() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChunkStore::open(dir.path()).unwrap();
        let pos = ChunkPos::new(1, 1);
        store.save(&chunk_with(pos, 10)).unwrap();
        let newer = chunk_with(pos, 50);
        store.save(&newer).unwrap();
        assert_eq!(store.load(pos).unwrap(), Some(newer));
    }

    #[test]
    fn a_leftover_temp_file_from_a_crash_is_harmless() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChunkStore::open(dir.path()).unwrap();
        let pos = ChunkPos::new(2, 2);
        let good = chunk_with(pos, 10);
        store.save(&good).unwrap();

        let tmp = store.path(pos).with_extension("mgchunk.tmp");
        fs::write(&tmp, b"half-written garbage").unwrap();
        assert_eq!(store.load(pos).unwrap(), Some(good));

        let newer = chunk_with(pos, 20);
        store.save(&newer).unwrap();
        assert!(!tmp.exists());
        assert_eq!(store.load(pos).unwrap(), Some(newer));
    }

    #[test]
    fn corruption_is_reported_not_papered_over() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChunkStore::open(dir.path()).unwrap();
        let pos = ChunkPos::new(0, 0);
        store.save(&chunk_with(pos, 10)).unwrap();
        let path = store.path(pos);
        let mut bytes = fs::read(&path).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0xFF;
        fs::write(&path, bytes).unwrap();
        assert!(matches!(store.load(pos), Err(StorageError::Corrupt(_))));
    }

    #[test]
    fn a_file_copied_to_the_wrong_name_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChunkStore::open(dir.path()).unwrap();
        let from = ChunkPos::new(1, 2);
        let to = ChunkPos::new(3, 4);
        store.save(&chunk_with(from, 10)).unwrap();
        fs::copy(store.path(from), store.path(to)).unwrap();
        assert!(matches!(store.load(to), Err(StorageError::PosMismatch)));
    }
}
