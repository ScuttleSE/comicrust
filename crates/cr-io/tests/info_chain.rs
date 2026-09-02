//! Metadata chain tests (T2): in-archive provider priority,
//! xattr storage (NtfsInfoStorage port), and sidecar fallback.

use std::io::Write;
use std::path::Path;

use cr_core::model::comic_book::ComicBook;
use cr_io::info::{store_stored_info, InfoLoadingMethod, XATTR_COMIC_INFO};
use cr_io::ComicProvider;

fn build_zip(path: &Path, entries: &[(&str, Vec<u8>)]) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = Default::default();
    for (name, data) in entries {
        zip.start_file((*name).to_string(), options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

fn comic_info_xml(series: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?>\r\n<ComicInfo xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\r\n  <Series>{series}</Series>\r\n  <Number>3</Number>\r\n  <Pages />\r\n</ComicInfo>"
    )
    .into_bytes()
}

fn metron_info_xml(series: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?>\r\n<MetronInfo xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\r\n  <Series>\r\n    <Name>{series}</Name>\r\n    <Volume>2</Volume>\r\n  </Series>\r\n  <Number>7</Number>\r\n</MetronInfo>"
    )
    .into_bytes()
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "comicrust-info-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn comic_info_wins_over_metron_info() {
    let dir = temp_dir("priority");
    let path = dir.join("comic.cbz");
    build_zip(
        &path,
        &[
            ("ComicInfo.xml", comic_info_xml("From ComicInfo")),
            ("MetronInfo.xml", metron_info_xml("From MetronInfo")),
            ("page1.jpg", b"x".to_vec()),
        ],
    );
    let provider = ComicProvider::open(&path).unwrap();
    let info = provider.load_info(InfoLoadingMethod::Slow).unwrap();
    assert_eq!(info.series, "From ComicInfo");
    assert_eq!(info.number, "3");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn metron_info_maps_when_no_comic_info() {
    let dir = temp_dir("metron");
    let path = dir.join("comic.cbz");
    build_zip(
        &path,
        &[
            ("MetronInfo.xml", metron_info_xml("Metron Series")),
            ("page1.jpg", b"x".to_vec()),
        ],
    );
    let provider = ComicProvider::open(&path).unwrap();
    let info = provider.load_info(InfoLoadingMethod::Fast).unwrap();
    assert_eq!(info.series, "Metron Series");
    assert_eq!(info.number, "7");
    assert_eq!(info.volume, 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn comic_book_roundtrip_through_xattr() {
    let dir = temp_dir("xattr");
    let path = dir.join("book.cbt");
    build_tar_with_book(&path);

    let provider = ComicProvider::open(&path).unwrap();
    // Nothing stored yet.
    assert!(provider.load_book(InfoLoadingMethod::Fast).is_none());

    // Store a book; both streams appear.
    let mut book = ComicBook::default();
    book.info.series = "Stored Series".into();
    book.opened_count = 5;
    assert!(provider.store_info(&book));

    // Stored xattrs are readable directly.
    assert!(xattr::get(&path, XATTR_COMIC_INFO).unwrap().is_some());

    // Loading returns the stored content.
    let loaded_info = provider.load_info(InfoLoadingMethod::Fast).unwrap();
    assert_eq!(loaded_info.series, "Stored Series");
    let loaded_book = provider.load_book(InfoLoadingMethod::Slow).unwrap();
    assert_eq!(loaded_book.opened_count, 5);

    // Re-storing identical content writes nothing new (no error, no
    // observable change — the C# skip-on-same behavior).
    let before = xattr::get(&path, XATTR_COMIC_INFO).unwrap().unwrap();
    assert!(!store_stored_info(&path, &loaded_book));
    let after = xattr::get(&path, XATTR_COMIC_INFO).unwrap().unwrap();
    assert_eq!(before, after);

    std::fs::remove_dir_all(&dir).ok();
}

fn build_tar_with_book(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut builder = tar::Builder::new(file);
    let mut header = tar::Header::new_gnu();
    let data = b"page";
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, "page1.jpg", data.as_slice())
        .unwrap();
    builder.finish().unwrap();
}

#[test]
fn sidecar_fallback() {
    let dir = temp_dir("sidecar");
    let path = dir.join("comic.cbz");
    build_zip(&path, &[("page1.jpg", b"x".to_vec())]);
    // <file>.xml sidecar.
    std::fs::write(dir.join("comic.cbz.xml"), comic_info_xml("Sidecar Series")).unwrap();

    let provider = ComicProvider::open(&path).unwrap();
    let info = provider.load_info(InfoLoadingMethod::Fast).unwrap();
    assert_eq!(info.series, "Sidecar Series");

    // Without in-archive or sidecar data: no info.
    let path2 = dir.join("plain.cbz");
    build_zip(&path2, &[("page1.jpg", b"x".to_vec())]);
    let provider2 = ComicProvider::open(&path2).unwrap();
    assert!(provider2.load_info(InfoLoadingMethod::Slow).is_none());

    std::fs::remove_dir_all(&dir).ok();
}
