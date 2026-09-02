//! Disk caches — fresh format (invariant #5: the C# `cache.idx`
//! BinaryFormatter files have NO compat requirement).
//!
//! Layout: one directory holding one file per entry, named by the
//! FNV-1a hash of the key plus a `.cache` suffix. Each file starts
//! with a fixed header: magic, key-hash, key-text length, key-text
//! (UTF-8, used for verification on open), data length — then the raw
//! data. The index is rebuilt by scanning the directory, so a crash
//! between writes cannot corrupt siblings, and stale entries are
//! found by comparing the stored key text.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 4] = b"CRC1";
const HEADER_FIXED: usize = 4 + 8 + 4;

pub struct DiskCache {
    dir: PathBuf,
}

impl DiskCache {
    /// Opens (creating if needed) a cache directory.
    pub fn open(dir: &Path) -> io::Result<DiskCache> {
        std::fs::create_dir_all(dir)?;
        Ok(DiskCache {
            dir: dir.to_path_buf(),
        })
    }

    pub fn path_of(&self, hash: u64) -> PathBuf {
        self.dir.join(format!("{hash:016x}.cache"))
    }

    /// `DiskCache.IsAvailable`.
    pub fn is_available(&self, hash: u64, key_text: &str) -> bool {
        self.read(hash, key_text).is_some()
    }

    /// Reads one entry; verifies the stored key text matches, so a
    /// hash collision (or stale file) is rejected.
    pub fn read(&self, hash: u64, key_text: &str) -> Option<Vec<u8>> {
        let mut file = std::fs::File::open(self.path_of(hash)).ok()?;
        let mut header = [0u8; HEADER_FIXED];
        file.read_exact(&mut header).ok()?;
        if &header[..4] != MAGIC {
            return None;
        }
        let stored_hash = u64::from_le_bytes(header[4..12].try_into().ok()?);
        let key_len = u32::from_le_bytes(header[12..16].try_into().ok()?) as usize;
        if stored_hash != hash || key_len > 64 * 1024 {
            return None;
        }
        let mut key_buf = vec![0u8; key_len];
        file.read_exact(&mut key_buf).ok()?;
        if String::from_utf8_lossy(&key_buf) != key_text {
            return None;
        }
        let mut len_buf = [0u8; 4];
        file.read_exact(&mut len_buf).ok()?;
        let data_len = u32::from_le_bytes(len_buf) as usize;
        let mut data = vec![0u8; data_len];
        file.read_exact(&mut data).ok()?;
        Some(data)
    }

    /// Writes one entry (atomic: temp file + rename, the C#
    /// `FileUtility.SafeDelete` discipline).
    pub fn write(&self, hash: u64, key_text: &str, data: &[u8]) -> io::Result<()> {
        let key_bytes = key_text.as_bytes();
        let mut out = Vec::with_capacity(HEADER_FIXED + key_bytes.len() + data.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&hash.to_le_bytes());
        out.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(key_bytes);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);

        let tmp = self.dir.join(format!(".tmp-{}", std::process::id()));
        std::fs::write(&tmp, &out)?;
        std::fs::rename(&tmp, self.path_of(hash))
    }

    /// `DiskCache.Clear` — drop the whole cache directory content.
    pub fn clear(&self) -> io::Result<()> {
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                std::fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }
}

/// FNV-1a 64-bit — the cache-file naming hash (not a compat surface;
/// keys compare by their full text inside the file).
pub fn fnv1a(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in text.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "comicrust-disk-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_read_roundtrip() {
        let cache = DiskCache::open(&cache_dir("rw")).unwrap();
        let hash = fnv1a("/comics/a.cbz|page 3");
        cache
            .write(hash, "/comics/a.cbz|page 3", b"jpeg-bytes")
            .unwrap();
        assert_eq!(
            cache.read(hash, "/comics/a.cbz|page 3").unwrap(),
            b"jpeg-bytes"
        );
        assert!(cache.is_available(hash, "/comics/a.cbz|page 3"));
        assert!(!cache.is_available(hash, "/comics/other.cbz|page 3"));
        cache.clear().unwrap();
        assert!(!cache.is_available(hash, "/comics/a.cbz|page 3"));
    }

    #[test]
    fn truncated_file_rejected() {
        let cache = DiskCache::open(&cache_dir("trunc")).unwrap();
        let hash = fnv1a("key2");
        cache.write(hash, "key2", b"0123456789").unwrap();
        let path = cache.path_of(hash);
        let data = std::fs::read(&path).unwrap();
        std::fs::write(&path, &data[..data.len() - 4]).unwrap();
        assert!(cache.read(hash, "key2").is_none());
        cache.clear().unwrap();
    }
}
