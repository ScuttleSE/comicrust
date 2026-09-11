//! Integration tests for the provider framework: synthetic CBZ, CBT,
//! and folder fixtures built at test runtime from generated PNGs (see
//! the test data policy in docs/archive/phases/phase-1.md).

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use cr_io::ComicAccessor;
use cr_io::ComicProvider;

// --- fixture helpers -------------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xffff_ffff;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 12);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(data);
    let mut crc_data = Vec::with_capacity(4 + data.len());
    crc_data.extend_from_slice(tag);
    crc_data.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_data).to_be_bytes());
    out
}

/// A minimal valid 1x1 grayscale PNG (one stored-zlib scanline), so
/// tests need no image codec dependency.
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

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "comicrust-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn build_zip(path: &Path, entries: &[(&str, Vec<u8>)]) {
    let file = File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = Default::default();
    for (name, data) in entries {
        zip.start_file((*name).to_string(), options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

fn build_tar(path: &Path, entries: &[(&str, Vec<u8>)]) {
    let file = File::create(path).unwrap();
    let mut builder = tar::Builder::new(file);
    for (name, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, name, data.as_slice())
            .unwrap();
    }
    builder.finish().unwrap();
}

/// The classic CBZ layout: metadata entries, a macOS resource folder,
/// and pages whose names force natural-order behavior.
fn comic_entries() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("ComicInfo.xml", b"<ComicInfo />".to_vec()),
        ("cover.jpg", png_pixel(255)),
        ("pages/1.jpg", png_pixel(1)),
        ("pages/2.jpg", png_pixel(2)),
        ("pages/10.jpg", png_pixel(3)),
    ]
}

// --- tests -----------------------------------------------------------

#[test]
fn cbz_page_order_filter_and_read() {
    let dir = temp_dir("cbz");
    let path = dir.join("comic.cbz");
    build_zip(&path, &comic_entries());

    let provider = ComicProvider::open(&path).unwrap();
    assert_eq!(provider.format().name, "eComic (ZIP)");
    let names: Vec<&str> = provider.pages().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        ["cover.jpg", "pages/1.jpg", "pages/2.jpg", "pages/10.jpg"]
    );

    // Sizes match the entries.
    assert_eq!(provider.pages()[0].size, png_pixel(255).len() as u64);

    // Read by page index returns the exact entry bytes.
    let page0 = provider.read_page(0).unwrap();
    assert_eq!(page0, png_pixel(255));
    let page3 = provider.read_page(3).unwrap();
    assert_eq!(page3, png_pixel(3));

    // Cache-key hash: 32 base32 chars, stable.
    let hash = provider.create_hash();
    assert_eq!(hash.len(), 32);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cbt_page_order_and_read() {
    let dir = temp_dir("cbt");
    let path = dir.join("comic.cbt");
    build_tar(&path, &comic_entries());

    let provider = ComicProvider::open(&path).unwrap();
    assert_eq!(provider.format().name, "eComic (TAR)");
    let names: Vec<&str> = provider.pages().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        ["cover.jpg", "pages/1.jpg", "pages/2.jpg", "pages/10.jpg"]
    );
    assert_eq!(provider.read_page(1).unwrap(), png_pixel(1));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn zip_and_tar_format_checks() {
    let dir = temp_dir("fmt");
    let zip_path = dir.join("a.cbz");
    build_zip(&zip_path, &comic_entries());
    let tar_path = dir.join("a.cbt");
    build_tar(&tar_path, &comic_entries());
    let text = dir.join("a.txt");
    std::fs::write(&text, b"plain text").unwrap();

    let zip_accessor = cr_io::accessors::ZipAccessor;
    let tar_accessor = cr_io::accessors::TarAccessor;
    assert!(zip_accessor.is_format(&zip_path));
    assert!(!zip_accessor.is_format(&text));
    assert!(tar_accessor.is_format(&tar_path));
    assert!(!tar_accessor.is_format(&text));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn folder_provider_lists_recursively() {
    let dir = temp_dir("folder");
    std::fs::write(dir.join("cover.jpg"), png_pixel(9)).unwrap();
    std::fs::create_dir(dir.join("pages")).unwrap();
    std::fs::write(dir.join("pages/2.jpg"), png_pixel(2)).unwrap();
    std::fs::write(dir.join("pages/10.jpg"), png_pixel(10)).unwrap();
    std::fs::write(dir.join("ComicInfo.xml"), b"<ComicInfo />").unwrap();

    // Opened through the provider factory (FOLDER format id 100).
    let provider = ComicProvider::open(&dir).unwrap();
    assert_eq!(provider.format().name, "Image Folder");
    let names: Vec<&str> = provider.pages().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["cover.jpg", "pages/2.jpg", "pages/10.jpg"]);
    assert_eq!(provider.read_page(1).unwrap(), png_pixel(2));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn unsupported_format_is_rejected() {
    let dir = temp_dir("unsupported");
    let path = dir.join("book.txt");
    std::fs::write(&path, b"not a comic").unwrap();
    let opened = ComicProvider::open(&path);
    assert!(matches!(opened, Err(cr_io::Error::UnsupportedFormat(_))));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn zip_page_read_survives_deflate() {
    // The fixture zip uses deflate (SimpleFileOptions default); make
    // sure decompression round-trips distinct content per page.
    let dir = temp_dir("deflate");
    let path = dir.join("comic.zip");
    let mut entries = comic_entries();
    entries.push(("pages/3.jpg", png_pixel(77)));
    build_zip(&path, &entries);

    let provider = ComicProvider::open(&path).unwrap();
    let names: Vec<&str> = provider.pages().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "cover.jpg",
            "pages/1.jpg",
            "pages/2.jpg",
            "pages/3.jpg",
            "pages/10.jpg"
        ]
    );
    assert_eq!(provider.read_page(3).unwrap(), png_pixel(77));

    std::fs::remove_dir_all(&dir).ok();
}

/// Regression: the zip crate's `ArchiveOffset::Detect` searches
/// BACKWARDS from the EOCD for the first CDFH in 2045-byte windows; on
/// an archive with prepended junk over a network mount that crawl costs
/// hours (measured on a 2.85 GB CIFS file). `ZipAccessor` derives the
/// offset from one tail read instead. This fixture prepends junk and
/// pins that info reads still work through the derived offset.
#[test]
fn cbz_with_prepended_junk_opens_fast() {
    let dir = temp_dir("cbz-prepend");
    let path = dir.join("comic.cbz");
    build_zip(&path, &comic_entries());

    // Prepend 512 junk bytes in place.
    let body = std::fs::read(&path).unwrap();
    let mut with_junk = vec![0u8; 512];
    with_junk.extend_from_slice(&body);
    std::fs::write(&path, &with_junk).unwrap();

    let provider = ComicProvider::open(&path).unwrap();
    let names: Vec<&str> = provider.pages().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names.first(), Some(&"cover.jpg"));

    let info = provider.read_info_file("ComicInfo.xml").unwrap();
    assert_eq!(info, b"<ComicInfo />");

    std::fs::remove_dir_all(&dir).ok();
}

/// Regression (measured 2026-09-11, live scan): a ZIP64 archive keeps
/// its zip64 record (56 bytes) and locator (20 bytes) BETWEEN the
/// central directory and the EOCD. The old `EOCD - cd_size - cd_offset`
/// arithmetic therefore produced a non-zero archive offset for a file
/// with no prepended bytes at all (76 on the 2.8 GB omnibus), the
/// mandatory-guess open failed, and the crate fell back to a full-file
/// backward scan. The offset is now VERIFIED against the central
/// directory signature before it is used.
#[test]
fn zip64_archive_resolves_to_offset_zero() {
    let dir = temp_dir("cbz-zip64");
    let path = dir.join("comic.cbz");

    // `large_file(true)` writes the zip64 trailer regardless of size.
    let file = File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions =
        zip::write::SimpleFileOptions::default().large_file(true);
    for (name, data) in comic_entries() {
        zip.start_file(name.to_string(), options).unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap();

    let provider = ComicProvider::open(&path).unwrap();
    assert_eq!(provider.format().name, "eComic (ZIP)");
    let names: Vec<&str> = provider.pages().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        ["cover.jpg", "pages/1.jpg", "pages/2.jpg", "pages/10.jpg"],
        "the zip64 trailer must not shift the derived archive offset"
    );
    assert_eq!(
        provider.read_info_file("ComicInfo.xml").unwrap(),
        b"<ComicInfo />"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// A ZIP64 archive that ALSO carries prepended bytes: the offset comes
/// from the zip64 record position, and is verified like every other
/// candidate.
#[test]
fn zip64_archive_with_prepended_junk_resolves() {
    let dir = temp_dir("cbz-zip64-prepend");
    let path = dir.join("comic.cbz");

    let file = File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions =
        zip::write::SimpleFileOptions::default().large_file(true);
    for (name, data) in comic_entries() {
        zip.start_file(name.to_string(), options).unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap();

    let body = std::fs::read(&path).unwrap();
    let mut with_junk = vec![0u8; 512];
    with_junk.extend_from_slice(&body);
    std::fs::write(&path, &with_junk).unwrap();

    let provider = ComicProvider::open(&path).unwrap();
    let names: Vec<&str> = provider.pages().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names.first(), Some(&"cover.jpg"));

    std::fs::remove_dir_all(&dir).ok();
}

/// Regression (measured 2026-09-11): "The Boys 064.cbz" carried ONE
/// zip local entry followed by 17.4 MB of unrelated data and no
/// central directory at all. The old open walked the whole file
/// backwards looking for an EOCD. The open must now fail from the
/// bounded tail check, report the reason, and leave the page list
/// empty — never scan the body.
#[test]
fn cbz_without_central_directory_fails_from_the_tail() {
    let dir = temp_dir("cbz-no-cd");
    let path = dir.join("comic.cbz");

    // A real local file header, then junk, and no directory.
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(b"PK\x03\x04");
    bytes.extend_from_slice(&[0u8; 26]);
    bytes.extend_from_slice(&vec![0xab; 512 * 1024]);
    std::fs::write(&path, &bytes).unwrap();

    let (provider, report) = ComicProvider::open_with_report(&path).unwrap();
    assert_eq!(provider.page_count(), 0);
    assert!(!report.is_clean());
    assert!(
        report.entry_error.is_some(),
        "a directory-less zip must report why it could not be read"
    );
    assert!(!report.mismatch, "the content really is zip-flavored");

    std::fs::remove_dir_all(&dir).ok();
}

/// Regression (measured 2026-09-11): "Blacksad ... Issue 004.cbz" is a
/// RAR archive with a `.cbz` name. The zip reader drove a full-file
/// backward scan over CIFS. The provider must detect the content, route
/// to the RAR reader, and flag the mismatch — the C#
/// `ImageProviderFactory.CreateSourceProvider` fallback
/// (ImageProviderFactory.cs:18-27).
#[test]
fn rar_content_named_cbz_routes_to_the_rar_reader() {
    let dir = temp_dir("rar-as-cbz");
    let path = dir.join("comic.cbz");

    // A RAR4 signature is enough to decide the ROUTING; reading the
    // entries needs the 7z subprocess, which is gated elsewhere.
    let mut bytes = b"Rar!\x1a\x07\x00".to_vec();
    bytes.extend_from_slice(&[0u8; 256]);
    std::fs::write(&path, &bytes).unwrap();

    let (provider, report) = ComicProvider::open_with_report(&path).unwrap();
    assert_eq!(
        provider.format().id,
        cr_io::formats::ids::CBR,
        "RAR content must read through the RAR accessor, not the zip one"
    );
    assert!(
        report.mismatch,
        "the extension/content mismatch is recorded"
    );
    assert_eq!(
        report.extension_format.map(|f| f.id),
        Some(cr_io::formats::ids::CBZ)
    );
    assert_eq!(
        report.detected_format.map(|f| f.id),
        Some(cr_io::formats::ids::CBR)
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// A normal, correctly named CBZ reports no mismatch and no error, so
/// the scanner leaves it unmarked.
#[test]
fn well_formed_cbz_reports_a_clean_open() {
    let dir = temp_dir("cbz-clean");
    let path = dir.join("comic.cbz");
    build_zip(&path, &comic_entries());

    let (_, report) = ComicProvider::open_with_report(&path).unwrap();
    assert!(report.is_clean());
    assert!(!report.mismatch);
    assert!(report.entry_error.is_none());

    std::fs::remove_dir_all(&dir).ok();
}

/// A TAR archive named `.cbz`: the tar engine has no signature, so the
/// routing comes from the `ustar` magic at offset 257.
#[test]
fn tar_content_named_cbz_routes_to_the_tar_reader() {
    let dir = temp_dir("tar-as-cbz");
    let path = dir.join("comic.cbz");
    build_tar(&path, &comic_entries());

    let (provider, report) = ComicProvider::open_with_report(&path).unwrap();
    assert_eq!(provider.format().id, cr_io::formats::ids::CBT);
    assert!(report.mismatch);
    let names: Vec<&str> = provider.pages().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names.first(), Some(&"cover.jpg"));

    std::fs::remove_dir_all(&dir).ok();
}
