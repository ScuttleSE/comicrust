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
    build_cbz_entries(path, pages, &[]);
}

fn build_cbz_entries(path: &Path, pages: usize, extra: &[(&str, Vec<u8>)]) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = Default::default();
    for i in 0..pages {
        zip.start_file(format!("{i:03}.png"), options).unwrap();
        zip.write_all(&png_pixel(100 + i as u8)).unwrap();
    }
    for (name, data) in extra {
        zip.start_file((*name).to_string(), options).unwrap();
        zip.write_all(data).unwrap();
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

fn comic_info_xml(series: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?>\r\n<ComicInfo xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\r\n  <Series>{series}</Series>\r\n  <Number>3</Number>\r\n  <Writer>John Writer</Writer>\r\n  <PageCount>2</PageCount>\r\n  <Pages>\r\n    <Page ImageSize=\"10\" ImageWidth=\"1\" ImageHeight=\"1\" />\r\n    <Page ImageSize=\"10\" ImageWidth=\"1\" ImageHeight=\"1\" ImageIndex=\"1\" PageType=\"Story\" />\r\n  </Pages>\r\n</ComicInfo>"
    )
    .into_bytes()
}

fn comic_book_xml(series: &str, checked: bool) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?>\r\n<ComicBook xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" Checked=\"{}\">\r\n  <Series>{series}</Series>\r\n  <BookNotes>catalog notes</BookNotes>\r\n</ComicBook>",
        if checked { "true" } else { "false" }
    )
    .into_bytes()
}

fn metron_info_xml(series: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?>\r\n<MetronInfo xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\r\n  <Series>\r\n    <Name>{series}</Name>\r\n    <Volume>2</Volume>\r\n  </Series>\r\n  <Number>7</Number>\r\n</MetronInfo>"
    )
    .into_bytes()
}

#[test]
fn scan_imports_comic_info_metadata() {
    let dir = temp_dir("info");
    build_cbz_entries(
        &dir.join("001.cbz"),
        2,
        &[("ComicInfo.xml", comic_info_xml("Info Series"))],
    );
    let mut db = create_new();
    let now = CrDateTime::min_value();
    let items = vec![ScanItem {
        location: dir.to_string_lossy().into_owned(),
        all: true,
        remove_missing: true,
        force_refresh_info: false,
    }];
    let result = scan_database(&mut db, &items, &now);
    assert_eq!(result.added.len(), 1, "{result:?}");
    let book = &db.books[0];
    // The C# scan reads the info chain for NEW books
    // (ComicScanner.cs:222 → RefreshInfoFromFile → LoadInfo).
    assert_eq!(book.info.series, "Info Series");
    assert_eq!(book.info.number, "3");
    assert_eq!(book.info.writer, "John Writer");
    // Page metadata rides the ComicInfo Pages list; PageCount comes
    // from the provider (2 PNG pages — the C# count wins for a fresh
    // book: `base.PageCount = imageProvider.Count`).
    assert_eq!(book.info.page_count, 2);
    assert_eq!(book.info.pages.len(), 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn scan_comic_info_wins_over_comic_book_xml() {
    let dir = temp_dir("bookxml");
    build_cbz_entries(
        &dir.join("001.cbz"),
        1,
        &[
            ("ComicInfo.xml", comic_info_xml("Info Series")),
            ("ComicBook.xml", comic_book_xml("Book Series", true)),
        ],
    );
    let mut db = create_new();
    let now = CrDateTime::min_value();
    let items = vec![ScanItem {
        location: dir.to_string_lossy().into_owned(),
        all: true,
        remove_missing: true,
        force_refresh_info: false,
    }];
    scan_database(&mut db, &items, &now);
    let book = &db.books[0];
    // ComicInfo.xml has the most up-to-date info (the C# merges it
    // over the ComicBook copy: cb.SetInfo(ci, onlyUpdateEmpty: false)).
    assert_eq!(book.info.series, "Info Series");
    // ComicBook-only fields ride SetBook.
    assert!(book.checked);
    assert_eq!(book.book_notes, "catalog notes");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn scan_maps_metron_info_metadata() {
    let dir = temp_dir("metron");
    build_cbz_entries(
        &dir.join("001.cbz"),
        1,
        &[("MetronInfo.xml", metron_info_xml("Metron Series"))],
    );
    let mut db = create_new();
    let now = CrDateTime::min_value();
    let items = vec![ScanItem {
        location: dir.to_string_lossy().into_owned(),
        all: true,
        remove_missing: true,
        force_refresh_info: false,
    }];
    scan_database(&mut db, &items, &now);
    let book = &db.books[0];
    // The MetronInfo.xml source maps into the same chain (order 1 —
    // read only when no ComicInfo.xml hit).
    assert_eq!(book.info.series, "Metron Series");
    assert_eq!(book.info.number, "7");
    assert_eq!(book.info.volume, 2);
    std::fs::remove_dir_all(&dir).ok();
}
