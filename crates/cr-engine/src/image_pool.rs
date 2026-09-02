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
}

impl ImagePool {
    /// The default construction; disk caches are created under `cache_dir`
    /// when given (`pages`/`thumbs` subfolders, fresh-format Phase 1
    /// caches).
    pub fn new(cache_dir: Option<&Path>) -> Self {
        let thread_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .clamp(1, 4);
        let page_cap = DEFAULT_PAGE_COUNT * 2;
        let (page_disk, thumb_disk) = match cache_dir {
            Some(dir) => (
                DiskCache::open(&dir.join("pages")).ok().map(Arc::new),
                DiskCache::open(&dir.join("thumbs")).ok().map(Arc::new),
            ),
            None => (None, None),
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
            pages: Arc::new(Mutex::new(cr_image::memory::page_pool())),
            thumbs: Arc::new(Mutex::new(cr_image::memory::MemoryPool::new(
                DEFAULT_THUMB_COUNT,
                DEFAULT_THUMB_SIZE,
            ))),
            page_disk,
            thumb_disk,
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

    /// `GenerateFrontCoverThumbnail` — the unlimited queue.
    pub fn generate_front_cover_thumbnail(&self, key: ThumbnailKey) {
        self.slow_thumbnail_queue_unlimited.add_item(key, |k| {
            // The render chain runs in the worker; nothing else to do —
            // the C# callback checks the disk cache and renders.
            let _ = k;
        });
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
    /// `AddImage` closure): decode, apply the partial disk-cache tiers,
    /// adjust, rotate, then cache.
    pub fn render_page(&self, key: &PageKey) -> Option<Image> {
        let provider = ComicProvider::open(Path::new(&key.key.location)).ok()?;
        let bytes = provider.read_page(key.key.index)?;
        let key_text = base_key_text(&key.key);
        let hash = fnv1a(&key_text);

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
                            let _ = disk.write(hash, &key_text, &jpeg);
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

        // Cache the final result in memory.
        let size = img.rgba.len();
        let hash = page_hash(key);
        if let Ok(mut pool) = self.pages.lock() {
            let _ = pool.lock_item(hash, || Ok((img.clone(), size)));
        }
        Some(img)
    }

    /// The worker render chain for a thumbnail: render the page, build
    /// the 512px JPEG q60 thumbnail, cache to disk and memory.
    pub fn render_thumbnail(&self, key: &ThumbnailKey) -> Option<Vec<u8>> {
        let text = base_key_text(&key.key);
        let hash = fnv1a(&text);
        if let Some(disk) = &self.thumb_disk {
            if let Some(bytes) = disk.read(hash, &text) {
                return Some(bytes);
            }
        }
        let page_key = PageKey::new(key.key.clone(), BitmapAdjustment::default());
        let img = self.render_page(&page_key)?;
        let original = (img.width, img.height);
        let thumb = thumbnail_from_image(&img, original).ok()?;
        let bytes = thumb.to_bytes();
        if let Some(disk) = &self.thumb_disk {
            let _ = disk.write(hash, &text, &bytes);
        }
        if let Ok(mut pool) = self.thumbs.lock() {
            let _ = pool.lock_item(hash, || Ok((bytes.clone(), bytes.len())));
        }
        Some(bytes)
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
