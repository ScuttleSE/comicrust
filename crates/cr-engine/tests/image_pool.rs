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
