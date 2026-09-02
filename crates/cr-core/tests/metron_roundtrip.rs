//! MetronInfo round-trip and mapping tests (T2, docs/phase-1-kickoff.md).
//! Round-trip = serialize → parse → serialize must be byte-identical.

use cr_core::model::metron_info::*;
use cr_core::xml::scalar::CrDateTime;
use cr_core::xml::XmlReader;

fn sample() -> MetronInfo {
    MetronInfo {
        ids: vec![IdType {
            value: "194531".into(),
            source: Some(InformationSource::ComicVine),
            primary: true,
            primary_specified: true,
        }],
        publisher: Some(PublisherType {
            name: "Dark Horse Comics".into(),
            imprint: Some(ResourceType {
                value: "Imprint!".into(),
                id: Some("imp-1".into()),
            }),
            id: Some("pub-1".into()),
        }),
        series: Some(SeriesType {
            name: "Sample Series".into(),
            sort_name: "Sample Series, A".into(),
            volume: 2,
            volume_specified: true,
            format: Some(FormatType::TradePaperback),
            format_specified: true,
            start_year: "2019".into(),
            issue_count: 12,
            issue_count_specified: true,
            volume_count: 3,
            volume_count_specified: true,
            alternative_names: vec![NameType {
                value: "Serie Exemple".into(),
                id: None,
                lang: Some("fr".into()),
            }],
            lang: Some("en".into()),
            id: Some("ser-1".into()),
        }),
        manga_volume: String::new(),
        collection_title: "Collection".into(),
        number: "7".into(),
        stories: vec![
            ResourceType {
                value: "First Story".into(),
                id: None,
            },
            ResourceType {
                value: "Second Story".into(),
                id: None,
            },
        ],
        summary: "A summary.".into(),
        prices: vec![PriceType {
            value: "3.99".into(),
            country: Some("US".into()),
        }],
        cover_date: CrDateTime::parse_date("2021-03-24").ok(),
        store_date: None,
        page_count: 32,
        notes: "Notes.".into(),
        genres: vec![ResourceType {
            value: "Science Fiction".into(),
            id: None,
        }],
        tags: vec![ResourceType {
            value: "tag".into(),
            id: None,
        }],
        arcs: vec![ArcType {
            name: "Story Arc".into(),
            number: 2,
            number_specified: true,
            id: None,
        }],
        characters: vec![ResourceType {
            value: "Hero".into(),
            id: None,
        }],
        teams: vec![ResourceType {
            value: "Team".into(),
            id: None,
        }],
        universes: vec![UniverseType {
            name: "Universe".into(),
            designation: "Earth-2".into(),
            id: None,
        }],
        locations: vec![ResourceType {
            value: "Location".into(),
            id: None,
        }],
        reprints: vec![],
        gtin: Some(GtinType {
            isbn: Some("978-1-5697-7731-9".into()),
            upc: None,
        }),
        age_rating: AgeRatingType::TeenPlus,
        urls: vec![IdUrlType {
            value: "https://example.com".into(),
            primary: true,
            primary_specified: true,
        }],
        credits: vec![
            CreditType {
                creator: Some(ResourceType {
                    value: "Jane Writer".into(),
                    id: None,
                }),
                roles: vec![RoleValues::Writer],
            },
            CreditType {
                creator: Some(ResourceType {
                    value: "Joe Ink".into(),
                    id: None,
                }),
                roles: vec![RoleValues::Inker, RoleValues::InkAssists],
            },
            CreditType {
                creator: Some(ResourceType {
                    value: "Jay Color".into(),
                    id: None,
                }),
                roles: vec![RoleValues::Colorist],
            },
        ],
        last_modified: CrDateTime::parse("2024-01-02T03:04:05").ok(),
    }
}

#[test]
fn roundtrip_is_byte_identical() {
    let info = sample();
    let bytes1 = info.serialize_bytes().unwrap();
    let mut cursor = std::io::Cursor::new(&bytes1);
    let mut reader = XmlReader::new(&mut cursor);
    let parsed = MetronInfo::parse_root(&mut reader).unwrap();
    assert_eq!(parsed, info);
    let bytes2 = parsed.serialize_bytes().unwrap();
    assert_eq!(bytes1, bytes2);
}

#[test]
fn serialization_form() {
    let bytes = sample().serialize_bytes().unwrap();
    let text = String::from_utf8(bytes).unwrap();
    // ComicRack form: declaration without encoding, xsd namespace first.
    assert!(text.starts_with("<?xml version=\"1.0\"?>\r\n<MetronInfo xmlns:xsd="));
    assert!(text.contains("xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi="));
    // Unspecified members are omitted.
    assert!(!text.contains("StoreDate"));
    assert!(!text.contains("Reprints"));
    // Specified members and wrappers are present, in schema order.
    for name in [
        "IDS",
        "Publisher",
        "Series",
        "CollectionTitle",
        "Number",
        "Stories",
        "Summary",
        "Prices",
        "CoverDate",
        "PageCount",
        "Notes",
        "Genres",
        "Tags",
        "Arcs",
        "Characters",
        "Teams",
        "Universes",
        "Locations",
        "GTIN",
        "AgeRating",
        "URLs",
        "Credits",
        "LastModified",
    ] {
        assert!(
            text.contains(&format!("<{name}")),
            "missing <{name}> in {text}"
        );
    }
    // xs:date form.
    assert!(text.contains("<CoverDate>2021-03-24</CoverDate>"));
    // Default-valued members omitted.
    assert!(!text.contains("<MangaVolume"));
}

#[test]
fn maps_to_comic_info() {
    let info = sample().to_comic_info();
    assert_eq!(info.publisher, "Dark Horse Comics");
    assert_eq!(info.imprint, "Imprint!");
    assert_eq!(info.series, "Sample Series");
    assert_eq!(info.count, 12);
    assert_eq!(info.volume, 2);
    assert_eq!(info.title, "First Story");
    assert_eq!(info.story_arc, "Second Story");
    assert_eq!(info.alternate_series, "Story Arc");
    assert_eq!(info.alternate_number, "2");
    assert_eq!((info.year, info.month, info.day), (2021, 3, 24));
    assert_eq!(info.writer, "Jane Writer");
    assert_eq!(info.inker, "Joe Ink, Joe Ink"); // Inker + Ink Assists, C# parity
    assert_eq!(info.colorist, "Jay Color");
    assert_eq!(info.age_rating, "Teen Plus");
    assert_eq!(info.format, "TPB");
    assert_eq!(info.language_iso, "en");
    assert_eq!(info.page_count, 32);
    assert_eq!(info.web, "https://example.com");
}

#[test]
fn mapping_defaults() {
    // Empty MetronInfo maps to C# defaults: -1 counts, empty strings.
    let info = MetronInfo::default().to_comic_info();
    assert_eq!(info.count, -1);
    assert_eq!(info.volume, -1);
    assert_eq!(info.year, -1);
    assert_eq!(info.format, "");
    assert_eq!(info.age_rating, "");
    assert_eq!(info.series, "");

    // OneShot maps through PascalToSpaced, not the XmlEnum hyphen form.
    let m = MetronInfo {
        series: Some(SeriesType {
            format: Some(FormatType::OneShot),
            format_specified: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(m.to_comic_info().format, "One Shot");
}
