//! Memory pool — the `ImageManagerBase`/`Cache` eviction shape:
//! bounded by item capacity *and* byte budget, least-recently-used
//! eviction, with `lock`-style access bumping recency. Queue
//! machinery (`ProcessingQueue`) is Phase 2 work.

use std::collections::HashMap;

use crate::error::Result;
use crate::Image;

/// LRU memory cache with capacity and byte limits
/// (`ImageManager.MemoryCache` shape: `pageCount` items for pages,
/// `thumbCount` items / `thumbSize` bytes for thumbnails).
pub struct MemoryPool<V> {
    max_items: usize,
    max_bytes: usize,
    bytes: usize,
    entries: HashMap<u64, Entry<V>>,
    /// Monotonic clock for recency (bumped on access).
    clock: u64,
}

struct Entry<V> {
    value: V,
    size: usize,
    last_use: u64,
}

impl<V> MemoryPool<V> {
    /// `ImagePool(thumbCount, thumbSize, pageCount)` budgets: pages
    /// default 5 items, thumbnails 20 items / 5 MB
    /// (`DefaultThumbCount`/`DefaultThumbSize`/`DefaultPageCount`).
    pub fn new(max_items: usize, max_bytes: usize) -> MemoryPool<V> {
        MemoryPool {
            max_items,
            max_bytes,
            bytes: 0,
            entries: HashMap::new(),
            clock: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn byte_size(&self) -> usize {
        self.bytes
    }

    /// Cache hit without producing — bumps recency on hit.
    pub fn get(&mut self, hash: u64) -> Option<&mut V> {
        self.clock += 1;
        let clock = self.clock;
        let entry = self.entries.get_mut(&hash)?;
        entry.last_use = clock;
        Some(&mut entry.value)
    }

    /// `Cache.LockItem` — get-or-insert, bumping recency on hit and
    /// evicting on insert.
    pub fn lock_item<F>(&mut self, hash: u64, make: F) -> Result<&mut V>
    where
        F: FnOnce() -> Result<(V, usize)>,
    {
        self.clock += 1;
        let clock = self.clock;
        if !self.entries.contains_key(&hash) {
            let (value, size) = make()?;
            self.insert(hash, value, size, clock)?;
        }
        let entry = self.entries.get_mut(&hash).expect("just inserted");
        entry.last_use = clock;
        Ok(&mut entry.value)
    }

    fn insert(&mut self, hash: u64, value: V, size: usize, clock: u64) -> Result<()> {
        // Evict LRU entries until the new entry fits the budgets
        // (the C# Cache trims ItemCapacity and DataCapacity on add).
        while (self.entries.len() >= self.max_items)
            || (self.max_bytes > 0
                && self.bytes + size > self.max_bytes
                && !self.entries.is_empty())
        {
            let Some(victim) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_use)
                .map(|(h, _)| *h)
            else {
                break;
            };
            self.remove(victim);
        }
        // A single oversized entry still gets cached (the C# keeps
        // the newest item even when it exceeds the byte budget).
        self.bytes += size;
        self.entries.insert(
            hash,
            Entry {
                value,
                size,
                last_use: clock,
            },
        );
        Ok(())
    }

    pub fn remove(&mut self, hash: u64) -> Option<V> {
        let entry = self.entries.remove(&hash)?;
        self.bytes -= entry.size;
        Some(entry.value)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

/// Page memory pool: `ImageManager(pageCount)`, 5 items, no byte
/// budget.
pub fn page_pool() -> MemoryPool<Image> {
    MemoryPool::new(5, 0)
}

/// Thumbnail memory pool: `ThumbnailManager(20, 5 MB)`.
pub fn thumbnail_pool() -> MemoryPool<crate::thumbnail::Thumbnail> {
    MemoryPool::new(20, 5_242_880)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool() -> MemoryPool<u64> {
        MemoryPool::new(3, 100)
    }

    #[test]
    fn evicts_least_recently_used() {
        let mut p = pool();
        *p.lock_item(1, || Ok((100, 10))).unwrap() += 0;
        *p.lock_item(2, || Ok((200, 10))).unwrap() += 0;
        // Touch 1 to make 2 the LRU.
        *p.lock_item(1, || Ok((999, 10))).unwrap() += 0;
        *p.lock_item(3, || Ok((300, 10))).unwrap() += 0;
        // Cache full (3 items). Touching 1 again bumps it, then
        // inserting evicts 2.
        *p.lock_item(1, || unreachable!()).unwrap() += 0;
        *p.lock_item(4, || Ok((400, 10))).unwrap() += 0;
        assert_eq!(p.len(), 3);
        let keys = [1usize, 3, 4];
        assert!(keys.iter().all(|k| p.entries.contains_key(&(*k as u64))));
        assert!(!p.entries.contains_key(&2));
    }

    #[test]
    fn byte_budget_evicts() {
        let mut p = MemoryPool::new(10, 25);
        *p.lock_item(1, || Ok((1, 10))).unwrap() += 0;
        *p.lock_item(2, || Ok((2, 10))).unwrap() += 0;
        // 10 more bytes would exceed 25 → evict entry 1 (LRU).
        *p.lock_item(3, || Ok((3, 10))).unwrap() += 0;
        assert_eq!(p.len(), 2);
        assert!(!p.entries.contains_key(&1));
    }

    #[test]
    fn get_hits_without_producing() {
        let mut pool: MemoryPool<String> = MemoryPool::new(2, 1024);
        pool.lock_item(1, || Ok(("first".into(), 5))).unwrap();
        // Produce is not called on a hit; recency bumps.
        let hit = pool.get(1).map(|v| v.clone());
        assert_eq!(hit.as_deref(), Some("first"));
        let again = pool.lock_item(1, || Err(crate::Error::UnsupportedFormat));
        assert_eq!(again.unwrap().as_str(), "first");
        assert!(pool.get(2).is_none());
    }

    #[test]
    fn lock_item_produces_on_miss() {
        let mut pool: MemoryPool<u32> = MemoryPool::new(2, 1024);
        assert_eq!(*pool.lock_item(7, || Ok((42, 4))).unwrap(), 42);
    }
}
