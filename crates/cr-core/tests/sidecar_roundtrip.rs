//! ComicInfo.xml / ComicBook.xml document serialization tests (T2).
//! Round-trip = parse → serialize → parse, byte-identical on the
//! ComicRack form; the ComicBook sidecar strips file-derived fields.

use cr_core::model::comic_book::ComicBook;
use cr_core::model::comic_info::ComicInfo;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use cr_core::xml::XmlReader;

/// ComicRack writer form: declaration without encoding, xsd first,
/// 2-space indent (per tests/golden/README.md rules).
const COMIC_INFO: &str = "<?xml version=\"1.0\"?>\r\n<ComicInfo xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\r\n  <Series>Test Series</Series>\r\n  <Number>3</Number>\r\n  <Count>20</Count>\r\n  <Writer>A. Author, B. Person</Writer>\r\n  <PageCount>22</PageCount>\r\n  <Pages>\r\n    <Page Image=\"-1\" ImageSize=\"1234\" ImageWidth=\"800\" />\r\n  </Pages>\r\n</ComicInfo>";

fn parse_comic_info(xml: &str) -> ComicInfo {
    let mut cursor = std::io::Cursor::new(xml.as_bytes());
    let mut reader = XmlReader::new(&mut cursor);
    cr_core::model::comic_info::parse_root(&mut reader).unwrap()
}

#[test]
fn comic_info_roundtrip_is_byte_identical() {
    let info = parse_comic_info(COMIC_INFO);
    let out = info.serialize_bytes().unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), COMIC_INFO);
}

#[test]
fn comic_info_defaults_are_omitted() {
    let out = ComicInfo::default().serialize_bytes().unwrap();
    let text = String::from_utf8(out).unwrap();
    // Only the lazy <Pages> wrapper survives from an all-default info.
    assert_eq!(
        text,
        "<?xml version=\"1.0\"?>\r\n<ComicInfo xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\r\n  <Pages />\r\n</ComicInfo>"
    );
}

fn populated_book() -> ComicBook {
    let mut book = ComicBook {
        id: CrGuid::parse("0a000000-0000-0000-0000-000000000001").unwrap(),
        file_path: "/comics/Test Series 003.cbz".into(),
        file_size: 123_456,
        file_modified_time: CrDateTime::parse("2024-01-02T03:04:05").unwrap(),
        added_time: CrDateTime::parse("2023-06-01T00:00:00").unwrap(),
        opened_count: 4,
        comic_info_is_dirty: true,
        new_pages: 2,
        ..Default::default()
    };
    book.info.series = "Test Series".into();
    book.info.number = "3".into();
    book.info.page_count = 22;
    book
}

#[test]
fn comic_book_sidecar_strips_file_fields() {
    let out = populated_book().serialize_bytes().unwrap();
    let text = String::from_utf8(out).unwrap();

    assert!(text.starts_with("<?xml version=\"1.0\"?>\r\n<ComicBook xmlns:xsd="));
    // Stripped: Id/FilePath attributes, FileSize, dirty flags, NewPages.
    assert!(!text.contains("Id="));
    assert!(!text.contains("File="));
    assert!(!text.contains("FileSize"));
    assert!(!text.contains("ComicInfoIsDirty"));
    assert!(!text.contains("NewPages"));
    // Kept: ComicInfo members and reading state.
    assert!(text.contains("<Series>Test Series</Series>"));
    assert!(text.contains("<Added>2023-06-01T00:00:00</Added>"));
    assert!(text.contains("<OpenCount>4</OpenCount>"));
    assert!(text.contains("<PageCount>22</PageCount>"));
}

#[test]
fn comic_book_full_keeps_everything() {
    let out = populated_book().serialize_full_bytes().unwrap();
    let text = String::from_utf8(out).unwrap();

    assert!(text.contains("Id=\"0a000000-0000-0000-0000-000000000001\""));
    assert!(text.contains("File=\"/comics/Test Series 003.cbz\""));
    assert!(text.contains("<FileSize>123456</FileSize>"));
    assert!(text.contains("<ComicInfoIsDirty>true</ComicInfoIsDirty>"));
    assert!(text.contains("<NewPages>2</NewPages>"));
    assert!(text.contains("<FileModifiedTime>2024-01-02T03:04:05</FileModifiedTime>"));
}

#[test]
fn comic_book_sidecar_roundtrip() {
    let book = populated_book();
    let out = book.serialize_bytes().unwrap();
    let text = String::from_utf8(out).unwrap();
    let mut cursor = std::io::Cursor::new(text.as_bytes());
    let mut reader = XmlReader::new(&mut cursor);
    let parsed = ComicBook::parse_root(&mut reader).unwrap();

    // The parsed book matches the stripped form: file fields reset.
    assert!(parsed.id.is_empty());
    assert!(parsed.file_path.is_empty());
    assert_eq!(parsed.file_size, -1);
    assert_eq!(parsed.info.series, "Test Series");
    assert_eq!(parsed.opened_count, 4);
    assert_eq!(parsed.added_time, book.added_time);
    // Re-serialization of the parsed sidecar is byte-identical.
    assert_eq!(parsed.serialize_bytes().unwrap(), text.into_bytes());
}
