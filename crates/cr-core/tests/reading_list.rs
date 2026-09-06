//! The `.cbl` reading-list file tests: a hand-written fixture in the
//! net48 `XmlSerializer` shape (parse), and a write → parse round
//! trip. `ComicReadingListContainer.cs` is the C# spec.

use cr_core::database::list_items::{ComicBookMatcher, ValueMatcher};
use cr_core::database::reading_list::{ReadingListContainer, ReadingListItem};
use cr_core::model::enums::MatcherMode;
use cr_core::xml::scalar::CrGuid;

fn value_matcher(type_name: &str, value: &str) -> ComicBookMatcher {
    ComicBookMatcher::Value(ValueMatcher {
        type_name: type_name.into(),
        match_value: value.into(),
        ..Default::default()
    })
}

/// A real-shaped `.cbl` as net48 writes it: declaration without an
/// encoding attribute, `xsd`/`xsi` namespaces on the root, the
/// `MatcherMode` attribute only when not And, default-valued item
/// attributes omitted, `<Id>` always present, `<FileName>` only when
/// set, the `<Books>`/`<Matchers>` collections always present.
const FIXTURE: &str = r#"<?xml version="1.0"?>
<ReadingList xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Name>My Reading List</Name>
  <Books>
    <Book Series="Batman" Number="1" Volume="2" Year="2015" Format="Comic">
      <Id>0197ffff-8a2f-7b31-9c11-3f0d4a55b6c7</Id>
      <FileName>Batman 001 (2015)</FileName>
    </Book>
    <Book Number="5">
      <Id>00000000-0000-0000-0000-000000000000</Id>
      <FileName>Daredevil 005 (2014)</FileName>
    </Book>
    <Book>
      <Id>00000000-0000-0000-0000-000000000000</Id>
    </Book>
  </Books>
  <Matchers>
    <ComicBookMatcher xsi:type="ComicBookSeriesMatcher">
      <MatchValue>Batman</MatchValue>
    </ComicBookMatcher>
  </Matchers>
</ReadingList>"#;

#[test]
fn parses_the_net48_fixture() {
    let c = ReadingListContainer::parse(FIXTURE.as_bytes()).expect("parse");
    assert_eq!(c.name.as_deref(), Some("My Reading List"));
    assert_eq!(c.matcher_mode, MatcherMode::And);
    assert_eq!(c.items.len(), 3);

    let first = &c.items[0];
    assert_eq!(first.series, "Batman");
    assert_eq!(first.number, "1");
    assert_eq!(first.volume, 2);
    assert_eq!(first.year, 2015);
    assert_eq!(first.format, "Comic");
    assert_eq!(
        first.id,
        CrGuid::parse("0197ffff-8a2f-7b31-9c11-3f0d4a55b6c7").unwrap()
    );
    assert_eq!(first.file_name, "Batman 001 (2015)");

    // Defaults: the missing attributes fall back to the C# ctor values.
    let second = &c.items[1];
    assert_eq!(second.series, "");
    assert_eq!(second.volume, -1);
    assert!(second.id.is_empty());
    let third = &c.items[2];
    assert!(third.file_name.is_empty());

    // The matcher set parses with the ComicLists machinery.
    assert_eq!(c.matchers.len(), 1);
    match &c.matchers[0] {
        ComicBookMatcher::Value(v) => {
            assert_eq!(v.type_name, "ComicBookSeriesMatcher");
            assert_eq!(v.match_value, "Batman");
        }
        other => panic!("expected a value matcher, got {other:?}"),
    }
}

#[test]
fn parses_matcher_mode_and_empty_collections() {
    let c = ReadingListContainer::parse(
        r#"<?xml version="1.0"?>
<ReadingList xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" MatcherMode="Or">
  <Name />
  <Books />
  <Matchers />
</ReadingList>"#
            .as_bytes(),
    )
    .expect("parse");
    assert_eq!(c.matcher_mode, MatcherMode::Or);
    assert_eq!(c.name.as_deref(), Some(""));
    assert!(c.items.is_empty());
    assert!(c.matchers.is_empty());
}

#[test]
fn write_parse_round_trip() {
    let mut c = ReadingListContainer {
        name: Some("Round Trip".into()),
        matcher_mode: MatcherMode::Or,
        items: vec![
            ReadingListItem {
                series: "Batman".into(),
                number: "1".into(),
                volume: 2,
                year: 2015,
                format: "Comic".into(),
                id: CrGuid::parse("0197ffff-8a2f-7b31-9c11-3f0d4a55b6c7").unwrap(),
                file_name: "Batman 001 (2015)".into(),
            },
            ReadingListItem::default(),
        ],
        matchers: vec![
            value_matcher("ComicBookSeriesMatcher", "Batman"),
            ComicBookMatcher::Group(Default::default()),
        ],
    };
    // Write → parse → write is byte-stable (the model equality plus
    // the byte check catches writer drift).
    let bytes = c.write_bytes().expect("write");
    let parsed = ReadingListContainer::parse(&bytes).expect("re-parse");
    let bytes2 = parsed.write_bytes().expect("write 2");
    assert_eq!(bytes, bytes2);
    c.matchers.clear();
    let _ = c;
}
