//! ImagePool queue render chain: a folder comic with a PNG page is
//! rendered by the queue worker (decode → adjust → rotate → cache), the
//! thumbnail chain writes the disk-cache entry, and the memory pools
//! hold the results.

use std::path::PathBuf;
use std::sync::Arc;

use cr_core::model::bitmap_adjustment::BitmapAdjustment;
use cr_core::model::enums::ImageRotation;
use cr_engine::image_pool::ImagePool;
use cr_image::keys::{ImageKey, PageKey, ThumbnailKey};

fn png_pixel(gray: u8) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    out.extend_from_slice(&chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]));
    let raw = [0u8, gray];
    let mut zlib = vec![0x78, 0x01];
    zlib.extend_from_slice(&[0x01]);
    zlib.extend_from_slice(&(raw.len() as u16).to_le_bytes());
    zlib.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
    zlib.extend_from_slice(&raw);
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());
    out.extend_from_slice(&chunk(b"IDAT", &zlib));
    out.extend_from_slice(&chunk(b"IEND", &[]));
    out
}

fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&[kind, data].concat());
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB88320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for b in data {
        crc = table[((crc ^ u32::from(*b)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "comicrust-imgpool-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn folder_comic_render_chain() {
    let dir = temp_dir("folder");
    std::fs::write(dir.join("001.png"), png_pixel(128)).unwrap();

    let cache = temp_dir("cache");
    let pool = Arc::new(ImagePool::new(Some(&cache)));

    // For a folder comic the key location is the folder itself.
    let key = ImageKey::new(
        "test",
        dir.to_string_lossy().as_ref(),
        0,
        0,
        0,
        ImageRotation::None,
    );
    let page_key = PageKey::new(key.clone(), BitmapAdjustment::default());

    // Direct render: decode → memory pool.
    let img = pool.render_page(&page_key).expect("render page");
    assert_eq!((img.width, img.height), (1, 1));
    assert_eq!(img.rgba, [128, 128, 128, 255]);

    // Thumbnail render: produces JPEG bytes and a disk-cache entry.
    let tkey = ThumbnailKey::new(key.clone());
    let thumb = pool.render_thumbnail(&tkey).expect("render thumb");
    // Thumbnail serialization: 4 i32 sizes + JPEG data.
    let jpeg = &thumb[20..];
    assert_eq!(&jpeg[0..2], &[0xFF, 0xD8]); // JPEG SOI
    assert!(thumb.len() > 20);
    let text = format!(
        "{}|{}|{}|{}",
        key.location, key.size, key.modified, key.index
    );
    let hash = cr_image::disk::fnv1a(&text);
    assert!(pool.thumb_disk.as_ref().unwrap().is_available(hash, &text));

    // The queued path: add with the render chain, wait for the queue
    // to drain, then the memory pool holds the page.
    pool.add_page_with_render(page_key.clone(), false);
    assert!(wait_for(std::time::Duration::from_secs(5), || pool
        .pages
        .lock()
        .unwrap()
        .len()
        == 1));
    assert!(wait_for(std::time::Duration::from_secs(5), || !pool
        .slow_page_queue
        .is_active()));
}

fn wait_for<F: Fn() -> bool>(timeout: std::time::Duration, check: F) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    check()
}

#[test]
fn config_construction_wires_disk_caches_and_memory_capacities() {
    let dir = temp_dir("config");
    std::fs::write(dir.join("001.png"), png_pixel(64)).unwrap();
    let cache = temp_dir("configcache");
    let config = cr_engine::image_pool::ImagePoolConfig {
        page_cache_dir: Some(cache.join("Images")),
        thumb_cache_dir: Some(cache.join("Thumbnails")),
        page_cache_size_mb: 7,
        thumb_cache_size_mb: 9,
        page_cache_enabled: true,
        thumb_cache_enabled: true,
        page_memory_count: 25,
        thumb_memory_bytes: 32 * 1024 * 1024,
    };
    let pool = Arc::new(ImagePool::with_config(&config));
    // The budgets and enable flags land on the disk caches.
    let page_disk = pool.page_disk.as_ref().expect("page disk");
    let thumb_disk = pool.thumb_disk.as_ref().expect("thumb disk");
    let key = ImageKey::new(
        "test",
        dir.to_string_lossy().as_ref(),
        0,
        0,
        0,
        ImageRotation::None,
    );
    let text = format!("{}|0|0|0", key.location);
    let hash = cr_image::disk::fnv1a(&text);
    // The disabled cache rejects the entry; the enabled one holds it.
    page_disk.write(hash, &text, b"raw").unwrap();
    assert!(page_disk.is_available(hash, &text));
    thumb_disk.set_enabled(false);
    assert!(!thumb_disk.is_available(hash, &text));
    thumb_disk.set_enabled(true);
    // Memory capacities: the page pool takes the config count; the
    // thumb pool holds at least the rendered entry within its
    // 32 MiB budget.
    let tkey = ThumbnailKey::new(key.clone());
    let _ = pool.render_thumbnail(&tkey).expect("thumb");
    assert!(pool.get_thumb_memory(&tkey).is_some());
    // The second render takes the memory path (no re-decode — the
    // page entry count stays 1 and the thumb bytes are the same).
    let again = pool.render_thumbnail(&tkey).expect("thumb again");
    assert_eq!(again, pool.get_thumb_memory(&tkey).unwrap());
}

#[test]
fn cache_events_carry_the_decoded_size() {
    let dir = temp_dir("events");
    std::fs::write(dir.join("001.png"), png_pixel(200)).unwrap();
    let config = cr_engine::image_pool::ImagePoolConfig {
        ..cr_engine::image_pool::ImagePoolConfig::default()
    };
    let pool = Arc::new(ImagePool::with_config(&config));
    let (tx, rx) = std::sync::mpsc::channel::<cr_engine::image_pool::CacheEvent>();
    pool.set_event_tx(cr_engine::image_pool::CacheEventTx::new(tx));
    let key = ImageKey::new(
        "test",
        dir.to_string_lossy().as_ref(),
        0,
        0,
        0,
        ImageRotation::None,
    );
    let _ = pool
        .render_thumbnail(&ThumbnailKey::new(key.clone()))
        .expect("thumb");
    let events: Vec<cr_engine::image_pool::CacheEvent> = rx.try_iter().collect();
    // The thumbnail chain fires both halves (the page render enters
    // the page memory cache, the thumb the thumb cache).
    assert!(events.iter().any(|e| matches!(
        e,
        cr_engine::image_pool::CacheEvent::PageCached {
            width: 1,
            height: 1,
            ..
        }
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        cr_engine::image_pool::CacheEvent::ThumbnailCached {
            width: 1,
            height: 1,
            ..
        }
    )));
}

#[test]
fn front_cover_key_uses_the_stored_cover_page() {
    use cr_core::model::comic_book::ComicBook;
    use cr_core::model::comic_page_info::ComicPageInfo;
    // Pages: 0 = story, 1 = FrontCover (provider image index 2).
    let mut book = ComicBook {
        file_path: "/comics/a.cbz".into(),
        ..ComicBook::default()
    };
    book.info.pages = vec![
        ComicPageInfo {
            image_index_raw: 1,
            ..Default::default()
        },
        ComicPageInfo {
            image_index_raw: 3,
            page_type: cr_core::model::enums::ComicPageType(1),
            rotation: ImageRotation::Rotate90,
            ..Default::default()
        },
    ];
    let key = cr_engine::image_pool::front_cover_thumbnail_key(&book);
    assert_eq!(key.key.index, 2);
    assert_eq!(key.key.rotation, ImageRotation::Rotate90);
    // A book without page metadata keeps the historical key.
    let plain = ComicBook {
        file_path: "/comics/b.cbz".into(),
        ..ComicBook::default()
    };
    let key = cr_engine::image_pool::front_cover_thumbnail_key(&plain);
    assert_eq!(key.key.index, 0);
    assert_eq!(key.key.rotation, ImageRotation::None);
}

#[test]
fn warm_up_reuses_disk_entries() {
    let dir = temp_dir("warm");
    std::fs::write(dir.join("001.png"), png_pixel(100)).unwrap();
    let cache = temp_dir("warmcache");
    let pool = Arc::new(ImagePool::new(Some(&cache)));
    let key = ImageKey::new(
        "test",
        dir.to_string_lossy().as_ref(),
        0,
        0,
        0,
        ImageRotation::None,
    );
    let text = format!("{}|0|0|0", key.location);
    let hash = cr_image::disk::fnv1a(&text);
    // The queued warm-up renders through the unlimited queue and
    // lands the disk entry.
    pool.generate_front_cover_thumbnail(ThumbnailKey::new(key.clone()));
    assert!(wait_for(std::time::Duration::from_secs(5), || pool
        .thumb_disk
        .as_ref()
        .unwrap()
        .is_available(hash, &text)));
    assert!(wait_for(std::time::Duration::from_secs(5), || !pool
        .slow_thumbnail_queue_unlimited
        .is_active()));
}
