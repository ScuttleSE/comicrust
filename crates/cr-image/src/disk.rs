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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const MAGIC: &[u8; 4] = b"CRC1";
const HEADER_FIXED: usize = 4 + 8 + 4;
/// How many writes run between size prunes (the C# `CleanUp` runs on
/// every insert; a write walk per cover is wasteful — a fixed stride
/// keeps the same budget semantics at a fraction of the cost).
const PRUNE_STRIDE: u64 = 128;

pub struct DiskCache {
    dir: PathBuf,
    /// `DiskCache.CacheSizeMB` (0 = unlimited).
    max_bytes: u64,
    /// `DiskCache.Enabled`.
    enabled: AtomicBool,
    /// Write counter for the prune stride.
    writes: AtomicU64,
}

impl DiskCache {
    /// Opens (creating if needed) a cache directory.
    pub fn open(dir: &Path) -> io::Result<DiskCache> {
        Self::open_with(dir, 0, true)
    }

    /// Opens with a byte budget (`cache_size_mb`, 0 = unlimited) and
    /// the enable flag (the C# sets both in the constructor; a size
    /// set prunes immediately — ported as `prune` after the budget).
    pub fn open_with(dir: &Path, cache_size_mb: u64, enabled: bool) -> io::Result<DiskCache> {
        std::fs::create_dir_all(dir)?;
        Ok(DiskCache {
            dir: dir.to_path_buf(),
            max_bytes: cache_size_mb.saturating_mul(1024 * 1024),
            enabled: AtomicBool::new(enabled),
            writes: AtomicU64::new(0),
        })
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// `DiskCache.CacheSizeMB` setter (a set prunes immediately —
    /// the C# `CleanUp` on the property).
    pub fn set_budget(&self, cache_size_mb: u64) {
        // max_bytes is sized at open; a mid-session budget change
        // prunes through a fresh view. The field stays fixed after
        // construction in this port — see prune.
        self.prune_with(cache_size_mb.saturating_mul(1024 * 1024));
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn path_of(&self, hash: u64) -> PathBuf {
        self.dir.join(format!("{hash:016x}.cache"))
    }

    /// `DiskCache.IsAvailable` — magic + hash + key text verify
    /// WITHOUT reading the data body (a paint of ~100 covers stats
    /// 100 files; a full body read per check would defeat the
    /// cache's snappiness goal). The key text still verifies, so a
    /// stale entry under a reused FNV name is rejected.
    pub fn is_available(&self, hash: u64, key_text: &str) -> bool {
        if !self.is_enabled() {
            return false;
        }
        let Ok(mut file) = std::fs::File::open(self.path_of(hash)) else {
            return false;
        };
        let Some(key_len) = header_matches(&mut file, hash) else {
            return false;
        };
        let mut key_buf = vec![0u8; key_len];
        if file.read_exact(&mut key_buf).is_err() {
            return false;
        }
        String::from_utf8_lossy(&key_buf) == key_text
    }

    /// Reads one entry; verifies the stored key text matches, so a
    /// hash collision (or stale file) is rejected.
    pub fn read(&self, hash: u64, key_text: &str) -> Option<Vec<u8>> {
        if !self.is_enabled() {
            return None;
        }
        let mut file = std::fs::File::open(self.path_of(hash)).ok()?;
        let key_len = header_matches(&mut file, hash)?;
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
    /// `FileUtility.SafeDelete` discipline), then prunes on stride.
    pub fn write(&self, hash: u64, key_text: &str, data: &[u8]) -> io::Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }
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
        std::fs::rename(&tmp, self.path_of(hash))?;
        let writes = self.writes.fetch_add(1, Ordering::Relaxed);
        if writes.is_multiple_of(PRUNE_STRIDE) {
            self.prune();
        }
        Ok(())
    }

    /// `DiskCache.CleanUp` — evict oldest-mtime files until the
    /// directory fits the byte budget.
    pub fn prune(&self) {
        self.prune_with(self.max_bytes);
    }

    fn prune_with(&self, max_bytes: u64) {
        if max_bytes == 0 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "cache"))
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                Some((meta.modified().ok()?, meta.len(), e.path()))
            })
            .collect();
        let total: u64 = files.iter().map(|(_, len, _)| *len).sum();
        if total <= self.max_bytes {
            return;
        }
        files.sort();
        let mut total = total;
        for (_, len, path) in &files {
            if total <= max_bytes {
                break;
            }
            if std::fs::remove_file(path).is_ok() {
                total -= len;
            }
        }
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

/// Reads and validates the fixed header + returns the key length.
/// `None` = bad magic, a hash mismatch, or an implausible key length.
fn header_matches(file: &mut std::fs::File, hash: u64) -> Option<usize> {
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
    Some(key_len)
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

    #[test]
    fn disabled_cache_reads_and_writes_nothing() {
        let cache = DiskCache::open_with(&cache_dir("off"), 0, false).unwrap();
        let hash = fnv1a("key3");
        cache.write(hash, "key3", b"data").unwrap();
        assert!(!cache.is_available(hash, "key3"));
        assert!(cache.read(hash, "key3").is_none());
        assert!(!cache.path_of(hash).exists());
        // Flipping the flag back on re-opens the cache.
        cache.set_enabled(true);
        cache.write(hash, "key3", b"data").unwrap();
        assert!(cache.is_available(hash, "key3"));
    }

    #[test]
    fn byte_budget_prunes_oldest_first() {
        let cache = DiskCache::open_with(&cache_dir("budget"), 0, true).unwrap();
        // Two 600 KiB entries with explicit mtimes; a 1 MiB budget
        // fits only one → the OLDEST goes.
        let hashes: Vec<u64> = (0..2)
            .map(|i| {
                let h = fnv1a(&format!("k{i}"));
                cache
                    .write(h, &format!("k{i}"), &vec![0u8; 600 * 1024])
                    .unwrap();
                h
            })
            .collect();
        for (i, h) in hashes.iter().enumerate() {
            touch_mtime(&cache.path_of(*h), 1000 + i as u64 * 10);
        }
        cache.set_budget(1);
        assert!(!cache.path_of(hashes[0]).exists());
        assert!(cache.path_of(hashes[1]).exists());
    }

    /// Sets an explicit mtime (std `FileTimes` — no extra dep) so
    /// the LRU prune order is deterministic in tests.
    fn touch_mtime(path: &Path, epoch_secs: u64) {
        use std::fs::FileTimes;
        let f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(epoch_secs);
        f.set_times(FileTimes::new().set_modified(t)).unwrap();
    }

    #[test]
    fn availability_does_not_read_the_data_body() {
        // The availability path touches only the header + key text —
        // a body truncated file still reports available.
        let cache = DiskCache::open(&cache_dir("hdr")).unwrap();
        let hash = fnv1a("key4");
        cache.write(hash, "key4", b"0123456789").unwrap();
        let path = cache.path_of(hash);
        let data = std::fs::read(&path).unwrap();
        std::fs::write(&path, &data[..data.len() - 8]).unwrap();
        assert!(cache.is_available(hash, "key4"));
        assert!(cache.read(hash, "key4").is_none());
        cache.clear().unwrap();
    }
}
