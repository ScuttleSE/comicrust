//! The `ImagePool` queue layer: the five background page/thumbnail
//! queues with the exact C# construction (thread counts, priorities,
//! sizes, add modes), and the worker render chains wired to the Phase 1
//! cr-image pipeline (decode → adjust → rotate → cache).
//!
//! C# construction (`ImagePool()`):
//!
//! ```text
//! threadCount = ProcessorCount.Clamp(1, MaximumQueueThreads /* 4 */)
//! fastPageQueue        1 thread   BelowNormal  pageCount*2   AddToTop
//! slowPageQueue        threadCount BelowNormal pageCount*2   AddToTop
//! fastThumbnailQueue   1 thread   Lowest       256           AddToTop
//! slowThumbnailQueue   threadCount Lowest      256           AddToTop
//! slowThumbnailQueueUnlimited threadCount Lowest int.MaxValue AddToTop
//! ```
//!
//! Deviations (headless port): the provider is opened per work item
//! (the C# reuses `ComicBookImageProvider` instances managed by the
//! caller); thread priorities are stored but not applied; the memory
//! pools use the Phase 1 LRU shape.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cr_core::model::bitmap_adjustment::BitmapAdjustment;
use cr_core::model::comic_book::ComicBook;
use cr_core::model::enums::ImageRotation;
use cr_image::disk::{fnv1a, DiskCache};
use cr_image::keys::{ImageKey, PageKey, ThumbnailKey};
use cr_image::thumbnail::thumbnail_from_image;
use cr_image::{decode, rotate, Image};
use cr_io::ComicProvider;

use crate::queue::{AddMode, ProcessingQueue, ThreadPriority};

/// `ImagePool` defaults (`DefaultThumbCount`, `DefaultThumbSize`,
/// `DefaultPageCount`).
pub const DEFAULT_THUMB_COUNT: usize = 20;
pub const DEFAULT_THUMB_SIZE: usize = 5_242_880;
pub const DEFAULT_PAGE_COUNT: usize = 5;

/// `CacheManager.MemoryThumbnailCacheSize` — the ITEM capacity of
/// the thumbnail memory cache (the C# CacheManager constructs
/// `new ImagePool(8192, settings.MemoryThumbCacheSizeMB,
/// settings.MemoryPageCacheCount)`).
pub const MEMORY_THUMBNAIL_CACHE_SIZE: usize = 8192;

/// One cache event (`ImagePool.PageCached`/`ThumbnailCached` — the
/// C# fires them when an item enters the MEMORY cache; the
/// `CacheManager` handler writes the decoded pixel size into the
/// book's `ComicPageInfo`).
#[derive(Clone, Debug, PartialEq)]
pub enum CacheEvent {
    PageCached {
        location: String,
        index: usize,
        width: u32,
        height: u32,
    },
    ThumbnailCached {
        location: String,
        index: usize,
        width: u32,
        height: u32,
    },
}

/// `std::mpsc::Sender` is not `Sync`; the queue workers need
/// `Send + Sync` callbacks (the ADR-019 `PageTx` shape).
#[derive(Clone)]
pub struct CacheEventTx(Arc<Mutex<std::sync::mpsc::Sender<CacheEvent>>>);

impl CacheEventTx {
    /// Wraps a channel sender (the worker sink).
    pub fn new(tx: std::sync::mpsc::Sender<CacheEvent>) -> CacheEventTx {
        CacheEventTx(Arc::new(Mutex::new(tx)))
    }

    pub fn send(&self, event: CacheEvent) {
        if let Ok(tx) = self.0.lock() {
            let _ = tx.send(event);
        }
    }
}

/// The cache construction (`CacheManager` ctor parity): the two
/// disk caches with budgets + enable flags, and the memory pool
/// capacities from the settings.
pub struct ImagePoolConfig {
    pub page_cache_dir: Option<PathBuf>,
    pub thumb_cache_dir: Option<PathBuf>,
    /// `Settings.PageCacheSizeMB`.
    pub page_cache_size_mb: u64,
    /// `Settings.ThumbCacheSizeMB`.
    pub thumb_cache_size_mb: u64,
    /// `Settings.PageCacheEnabled`.
    pub page_cache_enabled: bool,
    /// `Settings.ThumbCacheEnabled`.
    pub thumb_cache_enabled: bool,
    /// `Settings.MemoryPageCacheCount` (C# default 25, 20..100).
    pub page_memory_count: usize,
    /// `Settings.MemoryThumbCacheSizeMB` bytes; items capped at
    /// `MEMORY_THUMBNAIL_CACHE_SIZE` (8192).
    pub thumb_memory_bytes: usize,
    /// The `CustomThumbnails` folder (`Paths::custom_thumbnail_path`).
    pub custom_thumb_dir: Option<PathBuf>,
}

impl Default for ImagePoolConfig {
    fn default() -> Self {
        ImagePoolConfig {
            page_cache_dir: None,
            thumb_cache_dir: None,
            page_cache_size_mb: 0,
            thumb_cache_size_mb: 0,
            page_cache_enabled: true,
            thumb_cache_enabled: true,
            page_memory_count: DEFAULT_PAGE_COUNT,
            thumb_memory_bytes: DEFAULT_THUMB_SIZE,
            custom_thumb_dir: None,
        }
    }
}

/// The five queues of the C# `ImagePool`.
pub struct ImagePool {
    pub fast_page_queue: ProcessingQueue<PageKey>,
    pub slow_page_queue: ProcessingQueue<PageKey>,
    pub fast_thumbnail_queue: ProcessingQueue<ThumbnailKey>,
    pub slow_thumbnail_queue: ProcessingQueue<ThumbnailKey>,
    pub slow_thumbnail_queue_unlimited: ProcessingQueue<ThumbnailKey>,
    /// Page memory pool (5 items).
    pub pages: Arc<Mutex<cr_image::memory::MemoryPool<Image>>>,
    /// Thumbnail memory pool (20 items / 5 MB).
    pub thumbs: Arc<Mutex<cr_image::memory::MemoryPool<Vec<u8>>>>,
    /// Page disk cache (`pages.DiskCache`).
    pub page_disk: Option<Arc<DiskCache>>,
    /// Thumbnail disk cache (`thumbs.DiskCache`).
    pub thumb_disk: Option<Arc<DiskCache>>,
    /// The `CustomThumbnails` folder (one file per custom thumb; the
    /// C# `CustomThumbnailFolder`).
    custom_thumb_dir: Option<PathBuf>,
    /// `PageCached`/`ThumbnailCached` sink (`set_event_tx`).
    event_tx: Mutex<Option<CacheEventTx>>,
}

impl ImagePool {
    /// The historical construction (tests/probes): disk caches under
    /// `pages`/`thumbs` subfolders of `cache_dir`, defaults for the
    /// memory pools.
    pub fn new(cache_dir: Option<&Path>) -> Self {
        let config = match cache_dir {
            Some(dir) => ImagePoolConfig {
                page_cache_dir: Some(dir.join("pages")),
                thumb_cache_dir: Some(dir.join("thumbs")),
                ..ImagePoolConfig::default()
            },
            None => ImagePoolConfig::default(),
        };
        Self::with_config(&config)
    }

    /// The `CacheManager` construction: budgets + capacities from
    /// the config.
    pub fn with_config(config: &ImagePoolConfig) -> Self {
        let thread_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .clamp(1, 4);
        let page_cap = DEFAULT_PAGE_COUNT * 2;
        let (page_disk, thumb_disk) = {
            let open = |dir: &Option<PathBuf>, mb: u64, enabled: bool| {
                dir.as_ref()
                    .and_then(|d| DiskCache::open_with(d, mb, enabled).ok())
                    .map(Arc::new)
            };
            (
                open(
                    &config.page_cache_dir,
                    config.page_cache_size_mb,
                    config.page_cache_enabled,
                ),
                open(
                    &config.thumb_cache_dir,
                    config.thumb_cache_size_mb,
                    config.thumb_cache_enabled,
                ),
            )
        };
        let mut fast_page_queue = ProcessingQueue::new_single(
            "Background Fast Page Queue",
            ThreadPriority::BelowNormal,
            page_cap,
        );
        let mut slow_page_queue = ProcessingQueue::new(
            thread_count,
            "Background Slow Page Queue",
            ThreadPriority::BelowNormal,
            page_cap,
        );
        let mut fast_thumbnail_queue = ProcessingQueue::new_single(
            "Background Fast Thumbnails Queue",
            ThreadPriority::Lowest,
            256,
        );
        let mut slow_thumbnail_queue = ProcessingQueue::new(
            thread_count,
            "Background Slow Thumbnails Queue",
            ThreadPriority::Lowest,
            256,
        );
        let mut slow_thumbnail_queue_unlimited = ProcessingQueue::new(
            thread_count,
            "Background Slow Thumbnails Unlimited Queue",
            ThreadPriority::Lowest,
            usize::MAX,
        );
        // All five queues default to AddToTop.
        fast_page_queue.set_default_mode(AddMode::AddToTop);
        slow_page_queue.set_default_mode(AddMode::AddToTop);
        fast_thumbnail_queue.set_default_mode(AddMode::AddToTop);
        slow_thumbnail_queue.set_default_mode(AddMode::AddToTop);
        slow_thumbnail_queue_unlimited.set_default_mode(AddMode::AddToTop);
        ImagePool {
            fast_page_queue,
            slow_page_queue,
            fast_thumbnail_queue,
            slow_thumbnail_queue,
            slow_thumbnail_queue_unlimited,
            pages: Arc::new(Mutex::new(cr_image::memory::MemoryPool::new(
                config.page_memory_count,
                0,
            ))),
            thumbs: Arc::new(Mutex::new(cr_image::memory::MemoryPool::new(
                MEMORY_THUMBNAIL_CACHE_SIZE,
                config.thumb_memory_bytes,
            ))),
            page_disk,
            thumb_disk,
            custom_thumb_dir: config.custom_thumb_dir.clone(),
            event_tx: Mutex::new(None),
        }
    }

    /// `ImagePool.IsWorking` — true while ANY of the five queues has
    /// work. The C# `MainForm.UpdateActivityTimerTick` reads this once
    /// a second to show the page/thumbnail activity lamp
    /// (`MainForm.cs:3975`).
    pub fn is_working(&self) -> bool {
        self.fast_page_queue.is_active()
            || self.slow_page_queue.is_active()
            || self.fast_thumbnail_queue.is_active()
            || self.slow_thumbnail_queue.is_active()
            || self.slow_thumbnail_queue_unlimited.is_active()
    }

    /// `ImagePool.PageCached`/`ThumbnailCached` wiring: the sink the
    /// UI thread drains (the C# `CacheManager` handlers ride the
    /// cache events directly; the port bridges over mpsc — the
    /// worker closures are `Send + Sync`). Takes `&self`: the pool
    /// is shared through an `Arc`.
    pub fn set_event_tx(&self, tx: CacheEventTx) {
        if let Ok(mut slot) = self.event_tx.lock() {
            *slot = Some(tx);
        }
    }

    /// `ImagePool.AddPageToQueue(key, ..., bottom)` — fast queue when
    /// the page renders from disk-cache bytes alone, slow otherwise.
    /// The port routes everything through the fast queue when the key
    /// needs no rendering work and the disk cache has the raw page.
    pub fn add_page_to_queue(
        &self,
        key: PageKey,
        callback_key: Option<&str>,
        callback: impl Fn(&PageKey) + Send + Sync + 'static,
        bottom: bool,
    ) {
        let mode = if bottom {
            AddMode::AddToBottom
        } else {
            AddMode::AddToTop
        };
        let queue = if self.needs_slow_path(&key.key) {
            &self.slow_page_queue
        } else {
            &self.fast_page_queue
        };
        queue.add_item_with_key(key, callback_key, callback, mode);
    }

    fn needs_slow_path(&self, key: &ImageKey) -> bool {
        // The C# splits by whether the bytes can come from the disk
        // cache (fast) or must be rendered through the provider (slow).
        // Port: a page with no disk cache hit takes the slow path.
        match &self.page_disk {
            Some(disk) => !disk.is_available(fnv1a(&base_key_text(key)), &base_key_text(key)),
            None => true,
        }
    }

    /// `AddThumbToQueue` — fast when the thumbnail disk cache has the
    /// entry, slow when it must be created.
    pub fn add_thumb_to_queue(
        &self,
        key: ThumbnailKey,
        callback_key: Option<&str>,
        callback: impl Fn(&ThumbnailKey) + Send + Sync + 'static,
    ) {
        let text = base_key_text(&key.key);
        let fast = self
            .thumb_disk
            .as_ref()
            .is_some_and(|d| d.is_available(fnv1a(&text), &text));
        if fast {
            self.fast_thumbnail_queue.add_item_with_key(
                key,
                callback_key,
                callback,
                AddMode::AddToTop,
            );
        } else {
            self.slow_thumbnail_queue.add_item_with_key(
                key,
                callback_key,
                callback,
                AddMode::AddToTop,
            );
        }
    }

    /// `GenerateFrontCoverThumbnail` — the unlimited queue; the
    /// worker skips entries already on disk and renders through the
    /// standard chain otherwise (the C# `IsAvailable`-then-`GetThumbnail`
    /// shape).
    pub fn generate_front_cover_thumbnail(self: &Arc<Self>, key: ThumbnailKey) {
        let pool = Arc::clone(self);
        let _ = self.slow_thumbnail_queue_unlimited.add_item_with_key(
            key,
            None,
            move |k| {
                let _ = pool.render_thumbnail(k);
            },
            AddMode::AddToTop,
        );
    }

    /// Whether the thumbnail is ALREADY rendered (memory pool or
    /// disk cache) — the on-demand gate: with
    /// `GenerateThumbnailsOnDemand` off, the view only loads cached
    /// covers and never starts render work (the backfill command
    /// fills the cache instead).
    pub fn thumbnail_cached(&self, key: &ThumbnailKey) -> bool {
        let text = base_key_text(&key.key);
        let hash = fnv1a(&text);
        if let Ok(mut pool) = self.thumbs.lock() {
            if pool.get(hash).is_some() {
                return true;
            }
        }
        self.thumb_disk
            .as_ref()
            .is_some_and(|d| d.is_available(hash, &text))
    }

    /// `AreImagesPending(filePath)`.
    pub fn are_images_pending(&self, file_path: &str) -> bool {
        let pending = |items: &[ImageKey]| items.iter().any(|k| k.location == file_path);
        if pending(&page_keys(&self.slow_page_queue))
            || pending(&page_keys(&self.fast_page_queue))
            || pending(&thumb_keys(&self.fast_thumbnail_queue))
            || pending(&thumb_keys(&self.slow_thumbnail_queue_unlimited))
        {
            return true;
        }
        pending(&thumb_keys(&self.slow_thumbnail_queue))
    }

    /// The worker render chain for a page (`ImagePool.GetPage` →
    /// `AddImage` closure): memory pool first, then decode, partial
    /// disk-cache tiers, adjust, rotate, and re-cache.
    pub fn render_page(&self, key: &PageKey) -> Option<Image> {
        // Memory pool short-circuit (`pagePool.GetPage(onlyMemory)`
        // runs before any provider work in the C#).
        let hash = page_hash(key);
        if let Ok(mut pool) = self.pages.lock() {
            if let Some(cached) = pool.get(hash) {
                return Some(cached.clone());
            }
        }
        let provider = ComicProvider::open(Path::new(&key.key.location)).ok()?;
        let bytes = provider.read_page(key.key.index)?;
        let key_text = base_key_text(&key.key);
        // The RAW page slot hash (the disk tier stores the
        // unprocessed page); the memory slot uses the tiered
        // `page_hash` above.
        let raw_hash = fnv1a(&key_text);

        // Partial disk-cache tiers: (rotation, adjustment) from the
        // most-processed to the least (`GetPartialDiskPage` chain).
        let mut adjustment = key.adjustment;
        let mut rotation = key.key.rotation;
        let mut image: Option<Image> = None;
        if !(adjustment.is_empty() && rotation == ImageRotation::None) {
            for (rot, adj) in [
                (ImageRotation::None, adjustment),
                (rotation, BitmapAdjustment::default()),
                (ImageRotation::None, BitmapAdjustment::default()),
            ] {
                let tier_text = tier_key_text(&key.key, rot, &adj);
                if let Some(disk) = &self.page_disk {
                    if let Some(raw) = disk.read(fnv1a(&tier_text), &tier_text) {
                        image = decode::decode(&raw).ok();
                        adjustment = adj;
                        rotation = rot;
                        break;
                    }
                }
            }
        }

        let mut img = match image {
            Some(img) => img,
            None => {
                let img = decode::decode(&bytes).ok()?;
                // Slow providers cache the unprocessed page.
                if is_slow(&provider) {
                    if let Some(disk) = &self.page_disk {
                        if let Some(jpeg) = decode::normalize_to_jpeg(&bytes) {
                            let _ = disk.write(raw_hash, &key_text, &jpeg);
                        }
                    }
                }
                img
            }
        };
        if !adjustment.is_empty() {
            let _ = cr_image::adjust::apply_adjustment(&mut img, &adjustment);
        }
        if rotation != ImageRotation::None {
            img = rotate(&img, rotation).ok()?;
        }

        // Cache the final result in memory under the tiered hash
        // (the get above and every `get_page_memory` poll use the
        // same slot — the base-text hash never matched them).
        let size = img.rgba.len();
        if let Ok(mut pool) = self.pages.lock() {
            let _ = pool.lock_item(hash, || Ok((img.clone(), size)));
        }
        // `PageCached` (the memory-cache ItemAdded event): the
        // write-back consumes the decoded pixel size.
        let tx = self.event_tx.lock().ok().and_then(|s| s.clone());
        if let Some(tx) = tx {
            tx.send(CacheEvent::PageCached {
                location: key.key.location.clone(),
                index: key.key.index,
                width: img.width,
                height: img.height,
            });
        }
        Some(img)
    }

    /// `pagePool.GetPage(key, onlyMemory: true)` — the cache-hit
    /// check the reader runs on the UI thread before any queue work.
    pub fn get_page_memory(&self, key: &PageKey) -> Option<Image> {
        let hash = page_hash(key);
        let mut pool = self.pages.lock().ok()?;
        pool.get(hash).cloned()
    }

    /// Whether a page render is still queued or in flight — the
    /// reader's poll distinguishes "wait" from "decode failed" (the
    /// C# pool caches the error page, the reader renders it).
    pub fn is_page_pending(&self, key: &PageKey) -> bool {
        self.fast_page_queue
            .pending_items()
            .iter()
            .chain(self.slow_page_queue.pending_items().iter())
            .any(|k| k == key)
    }

    /// `thumbs.MemoryCache.Get` — the memory hit for an already
    /// rendered thumbnail (the UI's cache-first check; the C#
    /// `GetThumbnail` reads the memory cache before any queue work).
    pub fn get_thumb_memory(&self, key: &ThumbnailKey) -> Option<Vec<u8>> {
        let hash = fnv1a(&base_key_text(&key.key));
        let mut pool = self.thumbs.lock().ok()?;
        pool.get(hash).cloned()
    }

    /// `ImagePool.AddCustomThumbnail`: stores the image as a 512px
    /// thumbnail under a GUID name in the custom folder and returns
    /// the key text (the C# stores the guid; `GetThumbnailKey` wraps
    /// it as `custom:\\<guid>`).
    pub fn add_custom_thumbnail(&self, image: &cr_image::Image) -> Option<String> {
        let dir = self.custom_thumb_dir.as_ref()?;
        let text = cr_core::xml::scalar::CrGuid::new_random().to_d_string();
        let thumb = thumbnail_from_image(image, (image.width, image.height)).ok()?;
        std::fs::create_dir_all(dir).ok()?;
        std::fs::write(dir.join(&text), thumb.to_bytes()).ok()?;
        Some(text)
    }

    /// `ImagePool.RemoveCustomThumbnail`.
    pub fn remove_custom_thumbnail(&self, key: &str) {
        if let Some(dir) = &self.custom_thumb_dir {
            let _ = std::fs::remove_file(dir.join(key));
        }
    }

    /// The worker render chain for a thumbnail: render the page, build
    /// the 512px JPEG q60 thumbnail, cache to disk and memory.
    pub fn render_thumbnail(&self, key: &ThumbnailKey) -> Option<Vec<u8>> {
        let text = base_key_text(&key.key);
        let hash = fnv1a(&text);
        // Memory first (the C# cache-first `GetThumbnail` ordering —
        // without this the pool re-decodes the same cover for every
        // view that asks).
        if let Ok(mut pool) = self.thumbs.lock() {
            if let Some(cached) = pool.get(hash) {
                return Some(cached.clone());
            }
        }
        if let Some(disk) = &self.thumb_disk {
            if let Some(bytes) = disk.read(hash, &text) {
                return Some(bytes);
            }
        }
        let bytes = self.produce_thumbnail(key)?;
        let original = (0u32, 0u32);
        let _ = original;
        if let Some(disk) = &self.thumb_disk {
            let _ = disk.write(hash, &text, &bytes);
        }
        if let Ok(mut pool) = self.thumbs.lock() {
            let _ = pool.lock_item(hash, || Ok((bytes.clone(), bytes.len())));
        }
        Some(bytes)
    }

    /// The produce half of `render_thumbnail`: the custom-thumbnail
    /// resource loads its file; everything else renders the page.
    fn produce_thumbnail(&self, key: &ThumbnailKey) -> Option<Vec<u8>> {
        if let cr_image::keys::ThumbnailSource::Resource {
            resource_type,
            resource_location,
        } = &key.source_kind
        {
            if resource_type == "custom" {
                let dir = self.custom_thumb_dir.as_ref()?;
                return std::fs::read(dir.join(resource_location)).ok();
            }
            // the unknown-resource locator renders nothing (the C#
            // `resource:\\Unknown` for fileless books without a thumb)
            return None;
        }
        let page_key = PageKey::new(key.key.clone(), BitmapAdjustment::default());
        let img = self.render_page(&page_key)?;
        let original = (img.width, img.height);
        let thumb = thumbnail_from_image(&img, original).ok()?;
        // `ThumbnailCached` (the memory-cache ItemAdded event).
        let tx = self.event_tx.lock().ok().and_then(|s| s.clone());
        if let Some(tx) = tx {
            tx.send(CacheEvent::ThumbnailCached {
                location: key.key.location.clone(),
                index: key.key.index,
                width: original.0,
                height: original.1,
            });
        }
        Some(thumb.to_bytes())
    }
}

impl ImagePool {
    /// Adds a page with the standard render chain as its callback
    /// (`CachePage` shape). Takes `Arc<Self>` so the worker closure can
    /// own the pool.
    pub fn add_page_with_render(self: &Arc<Self>, key: PageKey, bottom: bool) {
        let mode = if bottom {
            AddMode::AddToBottom
        } else {
            AddMode::AddToTop
        };
        let queue = if self.needs_slow_path(&key.key) {
            &self.slow_page_queue
        } else {
            &self.fast_page_queue
        };
        let pool = Arc::clone(self);
        let _ = queue.add_item_with_key(
            key,
            None,
            move |k| {
                let _ = pool.render_page(k);
            },
            mode,
        );
    }

    /// Adds a thumbnail with the standard render chain as its callback.
    pub fn add_thumb_with_render(self: &Arc<Self>, key: ThumbnailKey) {
        let text = base_key_text(&key.key);
        let fast = self
            .thumb_disk
            .as_ref()
            .is_some_and(|d| d.is_available(fnv1a(&text), &text));
        let pool = Arc::clone(self);
        let _ = if fast {
            self.fast_thumbnail_queue.add_item_with_key(
                key,
                None,
                move |k| {
                    let _ = pool.render_thumbnail(k);
                },
                AddMode::AddToTop,
            )
        } else {
            self.slow_thumbnail_queue.add_item_with_key(
                key,
                None,
                move |k| {
                    let _ = pool.render_thumbnail(k);
                },
                AddMode::AddToTop,
            )
        };
    }
}

fn page_keys(queue: &ProcessingQueue<PageKey>) -> Vec<ImageKey> {
    queue.pending_items().into_iter().map(|k| k.key).collect()
}

fn thumb_keys(queue: &ProcessingQueue<ThumbnailKey>) -> Vec<ImageKey> {
    queue.pending_items().into_iter().map(|k| k.key).collect()
}

/// The unprocessed page key text (location + index + file identity) —
/// the disk-cache slot of the raw page.
fn base_key_text(key: &ImageKey) -> String {
    format!(
        "{}|{}|{}|{}",
        key.location, key.size, key.modified, key.index
    )
}

fn tier_key_text(key: &ImageKey, rotation: ImageRotation, adjustment: &BitmapAdjustment) -> String {
    format!(
        "{}|r{}|a{}|{}|{}|{}|{}|{}|{}",
        base_key_text(key),
        rotation as u8,
        adjustment.saturation.to_bits(),
        adjustment.contrast.to_bits(),
        adjustment.brightness.to_bits(),
        adjustment.gamma.to_bits(),
        adjustment.white_point_argb,
        adjustment.options.to_xml(),
        adjustment.sharpen
    )
}

fn page_hash(key: &PageKey) -> u64 {
    fnv1a(&tier_key_text(&key.key, key.key.rotation, &key.adjustment))
}

/// The C# `provider.IsSlow`: archive/pdf/djvu providers re-open their
/// source per read; folder providers are fast.
fn is_slow(provider: &ComicProvider) -> bool {
    provider.format().id != cr_io::formats::ids::FOLDER
}

/// Extra disk path helper for tests.
pub fn disk_cache_dir(dir: &Path) -> PathBuf {
    dir.to_path_buf()
}

/// `ComicBook.GetThumbnailKey` — the thumbnail key for the
/// book's front cover. A fileless book with a custom thumbnail
/// shows that (`resource` locator `custom:\\<key>`); file-backed
/// books key on the cover PAGE index, which translates back to
/// its PROVIDER image index (`TranslatePageToImageIndex`), and
/// the stored page rotation rides along (`GetThumbnailKey`
/// parity). Books without page metadata keep the historical key
/// (index 0, no rotation) — `FrontCoverPageIndex` defaults to 0.
pub fn front_cover_thumbnail_key(book: &ComicBook) -> ThumbnailKey {
    // The C# `GetThumbnailKey`: fileless books show the custom
    // thumbnail when one is set (`custom:\\<key>`); a linked book
    // never uses it.
    if book.file_path.is_empty() {
        if let Some(custom) = &book.custom_thumbnail_key {
            let key = ImageKey::new(
                "custom",
                format!("custom:\\\\{custom}"),
                0,
                0,
                0,
                ImageRotation::None,
            );
            return ThumbnailKey::with_locator(key);
        }
    }
    let page = book.info.front_cover_page_index().max(0) as usize;
    let image_index = book
        .info
        .pages
        .get(page)
        .map(|p| p.image_index().max(0) as usize)
        .unwrap_or(page);
    let rotation = book
        .info
        .pages
        .get(page)
        .map(|p| p.rotation)
        .unwrap_or(ImageRotation::None);
    let path = std::path::Path::new(&book.file_path);
    ThumbnailKey::new(ImageKey::from_file(
        book.file_path.clone(),
        path,
        image_index,
        rotation,
    ))
}
