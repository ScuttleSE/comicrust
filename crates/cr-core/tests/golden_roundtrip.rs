//! Golden-file round-trip tests for ComicDb.xml (Phase 0 gate).
//!
//! Fixtures live in `crates/tests/golden/`. `db-net-reference.xml` is
//! captured .NET output (LF→CRLF normalized); `db-small.xml` is
//! hand-written; `db-large.xml` must equal the output of [`large_db`]
//! (snapshot). Set `CR_BLESS=1` to (re)write `db-large.xml`.

use std::path::Path;

use cr_core::database::list_items::{ComicBookMatcher, ComicListItem, ValueMatcher, WatchFolder};
use cr_core::database::{load, open_with_fallback, save, save_bytes, ComicDatabase};
use cr_core::model::comic_book::ComicBook;
use cr_core::model::comic_page_info::ComicPageInfo;
use cr_core::xml::scalar::{CrDateTime, CrGuid};

fn golden_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .canonicalize()
        .expect("golden dir exists")
}

/// A rich database covering the full ComicDb.xml surface: books with
/// pages/rating/color adjustment/custom values, unparsed elements,
/// dates with all three kind suffixes, a nested list tree with
/// matchers, limits, filtered ids, watch folders, and a blacklist.
fn large_db() -> ComicDatabase {
    let mut book = ComicBook {
        id: CrGuid::parse("1b259acd-2369-4a30-be47-c8c8ff0ffb5e").unwrap(),
        checked: false,
        file_path:
            r"C:\Documents and Settings\Books\Amazing Adventures #12 (1999) - AC Publisher.cbz"
                .into(),
        rating: 3.5,
        last_page_read: 7,
        opened_count: 2,
        file_size: 12_345_678,
        book_price: 4.5,
        book_age: "Modern".into(),
        book_condition: "Fine".into(),
        book_store: "Local Shop".into(),
        book_owner: "Me".into(),
        book_collection_status: "Owned".into(),
        book_notes: "Signed".into(),
        book_location: "Shelf B".into(),
        isbn: "978-1-234-56789-0".into(),
        new_pages: 3,
        comic_info_is_dirty: true,
        custom_thumbnail_key: Some("thumbkey".into()),
        ..Default::default()
    };
    book.info.series = "Amazing Adventures".into();
    book.info.number = "12".into();
    book.info.volume = 2;
    book.info.count = 24;
    book.info.year = 1999;
    book.info.month = 6;
    book.info.day = 15;
    book.info.writer = "John Writer; Jane Writer".into();
    book.info.penciller = "P. Ciller".into();
    book.info.summary = "A story & a <great> adventure \"quoted\"".into();
    book.info.age_rating = "Everyone".into();
    book.info.black_and_white = cr_core::model::enums::YesNo::No;
    book.info.manga = cr_core::model::enums::MangaYesNo::YesAndRightToLeft;
    book.info.community_rating = 4.5;
    book.info.language_iso = "en".into();
    book.info.page_count = 3;
    book.added_time = CrDateTime::parse("2010-06-19T14:04:28+02:00").unwrap();
    book.released_time = CrDateTime::parse("1999-06-15T23:59:59Z").unwrap();
    book.opened_time = CrDateTime::parse("2020-01-02T03:04:05.1234567Z").unwrap();
    book.file_modified_time = CrDateTime::parse("2021-11-01T09:08:07Z").unwrap();
    book.file_creation_time = CrDateTime::parse("2005-01-01T00:00:00").unwrap();
    book.custom_values_store = "BookLocation=Home,LastRead=2020-01-02".into();
    book.info
        .unparsed_elements
        .push("<FutureTag some-attr=\"x\">value</FutureTag>".into());
    let mut page = ComicPageInfo::default();
    page.set_image_index(0);
    page.page_type = cr_core::model::enums::ComicPageType(1); // FrontCover
    page.image_width = 800;
    page.image_height = 1200;
    page.image_file_size = 43_781;
    book.info.pages.push(page);
    let mut page2 = ComicPageInfo::default();
    page2.set_image_index(1);
    page2.bookmark = Some("Chapter 2".into());
    book.info.pages.push(page2);
    let mut page3 = ComicPageInfo::default();
    page3.set_image_index(2);
    page3.rotation = cr_core::model::enums::ImageRotation::Rotate90;
    page3.page_position = cr_core::model::enums::ComicPagePosition::Near;
    page3.key = Some("k3".into());
    book.info.pages.push(page3);

    let color_book = ComicBook {
        id: CrGuid::parse("9e79eab2-ec18-4a30-a0b2-b87d8df96834").unwrap(),
        rating: 1.0,
        color_adjustment: cr_core::model::bitmap_adjustment::BitmapAdjustment {
            saturation: 0.5,
            brightness: 0.25,
            options: cr_core::model::enums::BitmapAdjustmentOptions(1),
            ..Default::default()
        },
        extra_sync_information: Some(cr_core::model::bitmap_adjustment::ExtraSyncInformation {
            reading_state_changed: true,
            ..Default::default()
        }),
        is_dynamic_source: true,
        ..Default::default()
    };

    let smart = ComicListItem::Smart(cr_core::database::list_items::SmartListItem {
        base: cr_core::database::list_items::ListItemBase {
            id: CrGuid::parse("69212cc2-fba3-497a-94fe-dfcbe0c356f4").unwrap(),
            name: Some("My Favorites".into()),
            favorite: true,
            book_count: 5,
            new_book_count: 1,
            new_book_count_date: CrDateTime::parse("2020-05-06T07:08:09Z").unwrap(),
            unread_book_count: 4,
            description: "Favorites".into(),
            cache_storage: Some("id1,id2".into()),
            quick_open: true,
            ..Default::default()
        },
        matcher_mode: cr_core::model::enums::MatcherMode::Or,
        matchers: vec![
            ComicBookMatcher::Value(ValueMatcher {
                type_name: "ComicBookRatingMatcher".into(),
                not: true,
                name: "Rating".into(),
                match_value: "3".into(),
                match_value_2: "5".into(),
                match_operator: 2,
                ..Default::default()
            }),
            ComicBookMatcher::Group(cr_core::database::list_items::GroupMatcher {
                not: true,
                matcher_mode: cr_core::model::enums::MatcherMode::And,
                collapsed: true,
                matchers: vec![ComicBookMatcher::Value(ValueMatcher {
                    type_name: "ComicBookAddedMatcher".into(),
                    not: true,
                    match_value: "abc".into(),
                    match_value_2: "def".into(),
                    match_operator: 2,
                    ..Default::default()
                })],
            }),
        ],
        limit: true,
        limit_type: cr_core::model::enums::ComicSmartListLimitType::MB,
        limit_value: 10,
        limit_selection_type: cr_core::model::enums::ComicSmartListLimitSelectionType::Position,
        limit_random_seed: 7,
        base_list_id: CrGuid::parse("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap(),
        not_in_base_list: true,
        filtered_ids: vec![CrGuid::parse("cccccccc-1111-2222-3333-444444444444").unwrap()],
        show_filtered: true,
    });

    let folder = ComicListItem::Folder(cr_core::database::list_items::FolderItem {
        base: cr_core::database::list_items::ListItemBase {
            id: CrGuid::parse("f53972bf-66ef-4c09-a7c3-68cc8336b74d").unwrap(),
            name: Some("Smart Lists".into()),
            ..Default::default()
        },
        collapsed: true,
        temporary: false,
        combine_mode: cr_core::model::enums::ComicFolderCombineMode::And,
        items: vec![
            smart,
            ComicListItem::IdList(cr_core::database::list_items::IdListItem {
                base: cr_core::database::list_items::ListItemBase {
                    id: CrGuid::parse("ed61e8ee-4982-47a1-a9c7-f0de997cc710").unwrap(),
                    name: Some("Reading List".into()),
                    ..Default::default()
                },
                book_ids: vec![CrGuid::parse("dddddddd-0000-0000-0000-000000000001").unwrap()],
            }),
            ComicListItem::Library(cr_core::database::list_items::LibraryListItem {
                base: cr_core::database::list_items::ListItemBase {
                    id: CrGuid::parse("b1ccc930-f5af-402a-9ebc-cab71ba61cbe").unwrap(),
                    name: Some("Library".into()),
                    ..Default::default()
                },
            }),
        ],
    });

    ComicDatabase {
        id: CrGuid::parse("11111111-2222-3333-4444-555555555555").unwrap(),
        name: Some("Large".into()),
        books: vec![book, color_book],
        comic_lists: vec![folder],
        watch_folders: vec![
            WatchFolder {
                folder: r"C:\path".into(),
                watch: true,
            },
            WatchFolder {
                folder: r"C:\p2".into(),
                watch: false,
            },
        ],
        black_list: vec![r"C:\skip".into()],
    }
}

#[test]
fn round_trip_all_fixtures_byte_identical() {
    let dir = golden_dir();
    for name in ["db-small.xml", "db-net-reference.xml", "db-large.xml"] {
        let path = dir.join(name);
        let original = std::fs::read(&path).unwrap();
        let db = load(&path).unwrap_or_else(|e| panic!("load {name}: {e}"));
        let out = save_bytes(&db).unwrap();
        assert_eq!(out, original, "{name}: round-trip bytes differ");
    }
}

#[test]
fn round_trip_realworld_db_byte_identical() {
    // Real-world regression fixture (user-approved commit; provenance in
    // tests/realworld/README.md). Skipped when the file is absent.
    let path = golden_dir().parent().unwrap().join("realworld/ComicDb.xml");
    if !path.exists() {
        return;
    }
    let original = std::fs::read(&path).unwrap();
    let db = load(&path).unwrap_or_else(|e| panic!("load realworld db: {e}"));
    let out = save_bytes(&db).unwrap();
    assert_eq!(out, original, "realworld db: round-trip bytes differ");
}

#[test]
fn large_fixture_is_snapshot_of_code() {
    let path = golden_dir().join("db-large.xml");
    let bytes = save_bytes(&large_db()).unwrap();
    if std::env::var("CR_BLESS").is_ok() {
        std::fs::write(&path, &bytes).unwrap();
    }
    let committed = std::fs::read(&path).unwrap();
    assert_eq!(bytes, committed, "db-large.xml drifted from code output");
}

#[test]
fn schema_inventory_snapshot() {
    // Element and attribute inventory of the large fixture, diffed
    // against the expected surface (catches accidental schema drift).
    let path = golden_dir().join("db-large.xml");
    let db = load(&path).unwrap();
    let bytes = save_bytes(&db).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    for elem in [
        "ComicDatabase",
        "Books",
        "Book",
        "Pages",
        "Page",
        "ComicLists",
        "Item",
        "Matchers",
        "ComicBookMatcher",
        "Items",
        "BookIds",
        "FilteredIds",
        "WatchFolders",
        "WatchFolder",
        "BlackList",
        "File",
        "Series",
        "Summary",
        "Rating",
        "ColorAdjustment",
        "Saturation",
        "Brightness",
        "Options",
        "ExtraSyncInformation",
        "CustomValuesStore",
        "Added",
        "Released",
        "Opened",
        "guid",
        "Display",
        "FutureTag",
    ] {
        let needle = if elem.starts_with(' ') {
            elem.trim().to_string()
        } else {
            elem.to_string()
        };
        assert!(
            text.contains(&needle),
            "element {needle} missing from large fixture"
        );
    }
    for attr in [
        "Id=",
        "Name=",
        "Checked=",
        "File=",
        "IsDynamicSource=",
        "Image=",
        "xsi:type=",
        "Folder=",
        "Watch=",
    ] {
        assert!(text.contains(attr), "attribute {attr} missing");
    }
}

#[test]
fn save_then_load_preserves_data() {
    let dir = golden_dir();
    let path = dir.join("db-large.xml");
    let db = load(&path).unwrap();
    let tmp = std::env::temp_dir().join("comicrust-golden-save.xml");
    save(&db, &tmp).unwrap();
    let reloaded = load(&tmp).unwrap();
    assert_eq!(db, reloaded);
    // The .bak rotation side effect exists.
    let bak = std::env::temp_dir().join("comicrust-golden-save.xml.bak");
    assert!(bak.exists());
    std::fs::remove_file(&tmp).ok();
    std::fs::remove_file(&bak).ok();
}

#[test]
fn corrupt_main_restores_from_bak() {
    let tmp = std::env::temp_dir().join("comicrust-corrupt-test");
    std::fs::create_dir_all(&tmp).unwrap();
    let main = tmp.join("ComicDb.xml");
    let bak = tmp.join("ComicDb.xml.bak");
    // Write a valid db to .bak; corrupt main.
    std::fs::write(
        &bak,
        std::fs::read(golden_dir().join("db-small.xml")).unwrap(),
    )
    .unwrap();
    std::fs::write(&main, b"<ComicDatabase><Books>truncated").unwrap();
    let (db, status) = open_with_fallback(&main).unwrap();
    assert_eq!(status, cr_core::database::OpenStatus::RestoredFromBak);
    assert_eq!(db.name.as_deref(), Some("Small"));
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn restore_file_is_consumed() {
    let tmp = std::env::temp_dir().join("comicrust-restore-test");
    std::fs::create_dir_all(&tmp).unwrap();
    let main = tmp.join("ComicDb.xml");
    let restore = tmp.join("ComicDb.restore");
    std::fs::write(
        &main,
        std::fs::read(golden_dir().join("db-small.xml")).unwrap(),
    )
    .unwrap();
    std::fs::write(
        &restore,
        std::fs::read(golden_dir().join("db-small.xml")).unwrap(),
    )
    .unwrap();
    let (_, status) = open_with_fallback(&main).unwrap();
    assert_eq!(status, cr_core::database::OpenStatus::RestoredFromRestore);
    assert!(!restore.exists(), "restore file must be consumed");
    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn negative_inputs_fail_cleanly() {
    let dir = golden_dir();
    // Truncated XML
    let full = std::fs::read(dir.join("db-small.xml")).unwrap();
    let truncated = &full[..full.len() / 2];
    let mut cur = std::io::Cursor::new(truncated.to_vec());
    let mut reader = cr_core::xml::XmlReader::new(&mut cur);
    assert!(ComicDatabase::parse(&mut reader).is_err());
    // Garbage bytes
    let mut cur = std::io::Cursor::new(vec![0xFF, 0x00, 0x13, b'<', b'>']);
    let mut reader = cr_core::xml::XmlReader::new(&mut cur);
    assert!(ComicDatabase::parse(&mut reader).is_err());
    // Empty file
    let mut cur = std::io::Cursor::new(Vec::new());
    let mut reader = cr_core::xml::XmlReader::new(&mut cur);
    assert!(ComicDatabase::parse(&mut reader).is_err());
    // Wrong root element
    let mut cur = std::io::Cursor::new(b"<?xml version=\"1.0\"?><Other />".to_vec());
    let mut reader = cr_core::xml::XmlReader::new(&mut cur);
    assert!(ComicDatabase::parse(&mut reader).is_err());
}

#[test]
fn load_real_net_reference() {
    // The .NET-generated reference parses and produces a sane summary.
    let path = golden_dir().join("db-net-reference.xml");
    let db = load(&path).unwrap();
    assert_eq!(db.books.len(), 2);
    assert_eq!(db.comic_lists.len(), 2);
    assert_eq!(db.black_list.len(), 1);
}

#[test]
fn plugin_host_matchers_round_trip_byte_identical() {
    // ADR-027: the ComicRack.Plugins matcher classes (Expression,
    // User Scripts) parse and round-trip byte-stably — the saved-query
    // compat surface — including the PluginKey XML attribute; they
    // evaluate to no-match in the port.
    let xml = r#"<?xml version="1.0"?>
<ComicDatabase xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" Id="11111111-2222-3333-4444-555555555555" Name="Script">
  <Books />
  <ComicLists>
    <Item xsi:type="ComicSmartListItem" Id="69212cc2-fba3-497a-94fe-dfcbe0c356f4" Name="Scripted" MatcherMode="And">
      <Display />
      <Matchers>
        <ComicBookMatcher xsi:type="ComicBookExpressionMatcher" MatchOperator="1">
          <MatchValue>__book.ShadowRating &gt; 3</MatchValue>
        </ComicBookMatcher>
        <ComicBookMatcher xsi:type="ComicBookPluginMatcher" PluginKey="my-list" />
      </Matchers>
    </Item>
  </ComicLists>
  <WatchFolders />
  <BlackList />
</ComicDatabase>"#;
    let dir =
        std::env::temp_dir().join(format!("comicrust-script-matchers-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("script.xml");
    std::fs::write(&path, xml).unwrap();

    let db = load(&path).unwrap();
    let out = save_bytes(&db).unwrap();
    // The writer's canonical form is stable across reloads.
    let path2 = dir.join("script2.xml");
    std::fs::write(&path2, &out).unwrap();
    let db2 = load(&path2).unwrap();
    let out2 = save_bytes(&db2).unwrap();
    assert_eq!(out, out2, "script matchers: second save drifted");

    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("xsi:type=\"ComicBookExpressionMatcher\""),
        "expression matcher kept"
    );
    assert!(
        text.contains("xsi:type=\"ComicBookPluginMatcher\""),
        "plugin matcher kept"
    );
    assert!(
        text.contains("PluginKey=\"my-list\""),
        "PluginKey attribute round-trips"
    );
    assert!(
        text.contains("__book.ShadowRating"),
        "expression value round-trips"
    );

    // The model side: the raw nodes carry the captured fields.
    let Some(cr_core::database::list_items::ComicListItem::Smart(smart)) = db
        .comic_lists
        .iter()
        .find(|i| matches!(i, cr_core::database::list_items::ComicListItem::Smart(_)))
    else {
        panic!("smart list expected");
    };
    assert_eq!(smart.matchers.len(), 2);
    let cr_core::database::list_items::ComicBookMatcher::Value(expr) = &smart.matchers[0] else {
        panic!("value matcher expected");
    };
    assert_eq!(expr.type_name, "ComicBookExpressionMatcher");
    assert_eq!(expr.match_value, "__book.ShadowRating > 3");
    let cr_core::database::list_items::ComicBookMatcher::Value(plugin) = &smart.matchers[1] else {
        panic!("value matcher expected");
    };
    assert_eq!(plugin.type_name, "ComicBookPluginMatcher");
    assert_eq!(plugin.plugin_key.as_deref(), Some("my-list"));

    std::fs::remove_dir_all(&dir).ok();
}
