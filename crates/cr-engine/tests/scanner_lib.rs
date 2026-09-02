//! Unattended scanner run against a real synthetic library: build CBZ
//! files on disk, scan into a fresh `ComicDatabase`, mutate the disk
//! (add / move / delete), rescan, and assert the database diff.

use std::io::Write;
use std::path::Path;

use cr_core::database::comic_database::create_new;
use cr_core::xml::scalar::CrDateTime;
use cr_engine::scanner::{scan_database, ScanItem};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "comicrust-scanner-lib-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

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

fn build_cbz(path: &Path, pages: usize) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = Default::default();
    for i in 0..pages {
        zip.start_file(format!("{i:03}.png"), options).unwrap();
        zip.write_all(&png_pixel(100 + i as u8)).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn unattended_library_scan() {
    let lib = temp_dir("root");
    let series_a = lib.join("Series A");
    std::fs::create_dir_all(&series_a).unwrap();
    build_cbz(&series_a.join("001.cbz"), 3);
    build_cbz(&series_a.join("002.cbz"), 5);

    let mut db = create_new();
    assert_eq!(db.books.len(), 0, "fresh database");
    let now = CrDateTime::min_value();
    let items = vec![ScanItem {
        location: lib.to_string_lossy().into_owned(),
        all: true,
        remove_missing: true,
        force_refresh_info: false,
    }];

    // First scan: both books added with page counts from the archives.
    let result = scan_database(&mut db, &items, &now);
    assert_eq!(result.added.len(), 2, "{result:?}");
    assert_eq!(db.books.len(), 2);
    let mut page_counts: Vec<i32> = db.books.iter().map(|b| b.info.page_count).collect();
    page_counts.sort();
    assert_eq!(page_counts, vec![3, 5]);
    assert!(db.books.iter().all(|b| !b.file_is_missing));
    assert!(db.books.iter().all(|b| b.file_size > 0));
    assert!(db
        .books
        .iter()
        .all(|b| b.id.to_d_string() != "00000000-0000-0000-0000-000000000000"));

    // Rescan without changes: everything just updates.
    let result = scan_database(&mut db, &items, &now);
    assert!(result.added.is_empty() && result.moved.is_empty() && result.removed.is_empty());
    assert_eq!(result.updated.len(), 2);

    // Add a book and delete another.
    build_cbz(&series_a.join("003.cbz"), 2);
    std::fs::remove_file(series_a.join("001.cbz")).unwrap();
    let result = scan_database(&mut db, &items, &now);
    assert_eq!(result.added.len(), 1, "{result:?}");
    assert_eq!(result.removed.len(), 1, "{result:?}");
    assert_eq!(db.books.len(), 2);
    assert!(db.books.iter().any(|b| b.file_path.ends_with("003.cbz")));
    assert!(!db.books.iter().any(|b| b.file_path.ends_with("001.cbz")));
}
