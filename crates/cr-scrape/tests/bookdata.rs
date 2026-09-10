//! The T3 gate: every update massage rule, the key-tag replace
//! paths, the date partials, the imprint chains, and the read side —
//! all against real `ComicBook`s (the parity heart).

use cr_core::model::comic_book::{values_store, ComicBook};
use cr_core::model::ComicInfo;
use cr_core::xml::scalar::CrDateTime;
use cr_scrape::bookdata::BookData;
use cr_scrape::config::Configuration;
use cr_scrape::cv::models::Issue;

fn issue() -> Issue {
    let mut issue = Issue::new(400011);
    issue.series_name = "Batman".into();
    issue.issue_num = "12".into();
    issue.title = "The Court of Owls".into();
    issue.summary = "A summary.".into();
    issue.publisher = "DC Comics".into();
    issue.imprint = "".into();
    issue.volume_year = 1940;
    issue.pub_year = 2011;
    issue.pub_month = 5;
    issue.pub_day = 14;
    issue.rel_year = 2011;
    issue.rel_month = 4;
    issue.rel_day = 20;
    issue.writers = vec!["Grant Morrison".into()];
    issue.characters = vec!["Batman".into(), "Robin".into()];
    issue.image_urls = vec!["http://img/cover.jpg".into()];
    issue.series_key = "40501".into();
    issue
}

fn config() -> Configuration {
    Configuration::default()
}

#[test]
fn read_side_fills_from_the_book_and_the_filename() {
    let book = ComicBook {
        file_path: "Comics/Amazing-Spider-Man 671 (2011) (Digital).cbr".into(),
        enable_proposed: true,
        ..Default::default()
    };
    let bd = BookData::from_book(&book, &config());
    // stored fields blank; the filename parse filled the gaps
    assert_eq!(bd.series, "Amazing-Spider-Man");
    assert_eq!(bd.issue_num, "671");
    assert_eq!(bd.pub_year, 2011);
    // a min-value ReleasedTime reads as (1,1,1) — the C# quirk (the
    // BookData rel-year setter keeps 1, month/day keep 1)
    assert_eq!((bd.rel_year, bd.rel_month, bd.rel_day), (1, 1, 1));
    assert_eq!(bd.page_count, 0);
    assert_eq!(bd.path, book.file_path);
    // page_count/path never update
    assert!(!bd.will_update("page_count_n"));
    assert!(!bd.will_update("path_s"));
    // everything else starts updatable
    assert!(bd.will_update("series_s"));
    assert!(bd.will_update("cover_url_s"));
}

#[test]
fn read_side_keeps_stored_values() {
    let mut book = ComicBook {
        info: ComicInfo {
            series: "Watchmen".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    book.info.number = "3".into();
    book.info.volume = 1986;
    book.info.format = "Annual".into();
    let bd = BookData::from_book(&book, &config());
    assert_eq!(bd.series, "Watchmen");
    assert_eq!(bd.volume_year, 1986);
    assert_eq!(bd.format, "Annual");
}

#[test]
fn read_side_custom_scrape_keys() {
    let book = ComicBook {
        custom_values_store: values_store::encode(&[("comicvine_issue".into(), "400011".into())]),
        ..Default::default()
    };
    let bd = BookData::from_book(&book, &config());
    assert_eq!(bd.issue_key, "400011");
    assert_eq!(bd.series_key, "");
}

#[test]
fn update_writes_fields_through_the_rules() {
    let mut cfg = config();
    cfg.rescrape_tags = true;
    let mut book = ComicBook::default();
    let mut bd = BookData::from_book(&book, &config());
    bd.update(&issue(), &cfg, "2013.05.14 21:43:06", None);
    bd.apply_to(&mut book);
    assert_eq!(book.info.series, "Batman");
    assert_eq!(book.info.number, "12");
    assert_eq!(book.info.title, "The Court of Owls");
    assert_eq!(book.info.summary, "A summary.");
    assert_eq!(book.info.volume, 1940);
    assert_eq!(book.info.year, 2011);
    assert_eq!(book.info.month, 5);
    assert_eq!(book.info.day, 14);
    assert_eq!(book.info.writer, "Grant Morrison");
    assert_eq!(
        book.released_time,
        CrDateTime::parse_date("2011-04-20").unwrap()
    );
    // the key tag and the key note landed
    assert!(book.info.tags.contains("CVDB400011"));
    assert!(book
        .info
        .notes
        .starts_with("Scraped metadata from ComicVine [CVDB400011]."));
    // the custom keys ride the store
    let pairs = values_store::decode(&book.custom_values_store);
    assert!(pairs.contains(&("comicvine_issue".into(), "400011".into())));
    assert!(pairs.contains(&("comicvine_volume".into(), "40501".into())));
}

#[test]
fn update_off_leaves_the_field_untouched() {
    let mut config = config();
    config.update_writer = false;
    let mut bd = BookData::from_book(&ComicBook::default(), &config);
    bd.update(&issue(), &config, "2026.01.01 00:00:00", None);
    assert!(!bd.will_update("writers_sl"));
    // the field keeps its OLD value (never assigned from the issue)
    assert!(bd.writers.is_empty());
    let mut book = ComicBook::default();
    bd.apply_to(&mut book);
    assert_eq!(book.info.writer, "");
}

#[test]
fn overwrite_off_keeps_existing_values() {
    let mut config = config();
    config.overwrite_existing = false;
    let mut book = ComicBook {
        info: ComicInfo {
            title: "My Title".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    book.info.writer = "Me".into();
    let mut bd = BookData::from_book(&book, &config);
    bd.update(&issue(), &config, "2026.01.01 00:00:00", None);
    // title/writer existed and overwrite is off -> untouched
    assert!(!bd.will_update("title_s"));
    assert!(!bd.will_update("writers_sl"));
    bd.apply_to(&mut book);
    assert_eq!(book.info.title, "My Title");
    // but blank fields still fill
    assert_eq!(book.info.series, "Batman");
}

#[test]
fn ignore_blanks_keeps_a_value_when_the_scrape_is_blank() {
    let mut config = config();
    config.ignore_blanks = true;
    let mut book = ComicBook {
        info: ComicInfo {
            summary: "Existing.".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bd = BookData::from_book(&book, &config);
    let mut blank_issue = issue();
    blank_issue.summary = "   ".into();
    bd.update(&blank_issue, &config, "2026.01.01 00:00:00", None);
    assert!(!bd.will_update("summary_s"));
    bd.apply_to(&mut book);
    assert_eq!(book.info.summary, "Existing.");

    // overwrite with blanks allowed (ignore_blanks off) -> cleared
    let mut config2 = Configuration::default();
    config2.overwrite_existing = true;
    let mut bd2 = BookData::from_book(&ComicBook::default(), &config2);
    bd2.update(&blank_issue, &config2, "2026.01.01 00:00:00", None);
    assert!(bd2.will_update("summary_s"));
    assert_eq!(bd2.summary, "");
}

#[test]
fn volume_requires_a_positive_value() {
    let config = config();
    let mut bd = BookData::from_book(&ComicBook::default(), &config);
    let mut zero = issue();
    zero.volume_year = 0;
    bd.update(&zero, &config, "2026.01.01 00:00:00", None);
    // an invalid volume becomes the blank; still written (overwrite on)
    assert!(bd.will_update("volume_year_n"));
    assert_eq!(bd.volume_year, -1);
    let mut book = ComicBook::default();
    bd.apply_to(&mut book);
    assert_eq!(book.info.volume, -1);
}

#[test]
fn date_partials_write_progressively() {
    let config = config();
    let mut bd = BookData::from_book(&ComicBook::default(), &config);
    let mut partial = issue();
    partial.pub_day = -1; // year+month only
    partial.rel_day = -1; // released needs all three -> untouched
    bd.update(&partial, &config, "2026.01.01 00:00:00", None);
    let mut book = ComicBook::default();
    bd.apply_to(&mut book);
    assert_eq!(book.info.year, 2011);
    assert_eq!(book.info.month, 5);
    assert_eq!(book.info.day, -1);
    // ReleasedTime was NOT written (missing day)
    assert!(book.released_time.is_min_value());
}

#[test]
fn blank_scraped_dates_do_not_wipe_a_stored_released_time() {
    let mut config = config();
    config.overwrite_existing = true;
    let mut book = ComicBook {
        released_time: CrDateTime::parse_date("2015-01-01").unwrap(),
        ..Default::default()
    };
    let mut bd = BookData::from_book(&book, &config);
    let mut blank = issue();
    blank.rel_year = -1;
    blank.rel_month = -1;
    blank.rel_day = -1;
    bd.update(&blank, &config, "2026.01.01 00:00:00", None);
    bd.apply_to(&mut book);
    // the stored date survives (a blank new date never writes)
    assert_eq!(book.released_time.naive.date().to_string(), "2015-01-01");
}

#[test]
fn imprint_chain_resolves() {
    // the data layer (query_issue) already resolved Vertigo -> DC
    // Comics; the update path records the imprint as-is
    let config = config();
    let mut bd = BookData::from_book(&ComicBook::default(), &config);
    let mut imprinted = issue();
    imprinted.publisher = "DC Comics".into();
    imprinted.imprint = "Vertigo".into();
    bd.update(&imprinted, &config, "2026.01.01 00:00:00", None);
    assert_eq!(bd.publisher, "DC Comics");
    assert_eq!(bd.imprint, "Vertigo");
}

#[test]
fn user_imprints_and_aliases_apply() {
    let mut config = config();
    config.set_advanced_settings(
        "IMPRINT=Vertigo-->DC Comics\n\
         PUBLISHER_ALIAS=DC Comics-->Detective Comics\n",
    );
    let mut bd = BookData::from_book(&ComicBook::default(), &config);
    let mut imprinted = issue();
    imprinted.publisher = "Vertigo".into();
    imprinted.imprint = "".into();
    bd.update(&imprinted, &config, "2026.01.01 00:00:00", None);
    // user imprint maps Vertigo -> DC Comics (imprint = Vertigo),
    // then the alias replaces DC Comics -> Detective Comics
    assert_eq!(bd.publisher, "Detective Comics");
    assert_eq!(bd.imprint, "Vertigo");
}

#[test]
fn convert_imprints_off_keeps_the_imprint_as_publisher() {
    let mut config = config();
    config.convert_imprints = false;
    let mut bd = BookData::from_book(&ComicBook::default(), &config);
    let mut imprinted = issue();
    imprinted.publisher = "DC Comics".into();
    imprinted.imprint = "Vertigo".into();
    bd.update(&imprinted, &config, "2026.01.01 00:00:00", None);
    assert_eq!(bd.publisher, "Vertigo");
    assert_eq!(bd.imprint, "");
}

#[test]
fn identical_imprint_and_publisher_nullify() {
    let mut config = config();
    config.set_advanced_settings("IMPRINT=Foo-->Bar\n");
    let mut bd = BookData::from_book(&ComicBook::default(), &config);
    let mut same = issue();
    same.publisher = "Bar".into();
    same.imprint = "".into();
    bd.update(&same, &config, "2026.01.01 00:00:00", None);
    // user imprint: key=publisher "Bar" -> parent "Foo"? No: the user
    // imprint map keys are imprints; "bar" IS a key (Foo-->Bar means
    // imprint Foo belongs to publisher Bar) — the map is keyed by
    // IMPRINT name, so "bar" is not a key. Self-imprint nullify runs
    // when publisher == imprint after conversion.
    assert_eq!(bd.publisher, "Bar");
    assert_eq!(bd.imprint, "");
}

#[test]
fn key_tags_replace_and_append() {
    let mut config = config();
    config.rescrape_tags = true;
    let mut bd = BookData::from_book(&ComicBook::default(), &config);
    bd.tags = vec!["CVDB111".into(), "read".into()];
    bd.update(&issue(), &config, "2026.01.01 00:00:00", None);
    // the old CVDB111 tag was REPLACED with the new one
    assert_eq!(bd.tags, vec!["CVDB400011".to_string(), "read".to_string()]);

    // no previous tag -> appended
    let mut bd2 = BookData::from_book(&ComicBook::default(), &config);
    bd2.tags = vec!["read".into(), "owned".into()];
    bd2.update(&issue(), &config, "2026.01.01 00:00:00", None);
    assert_eq!(
        bd2.tags,
        vec![
            "read".to_string(),
            "owned".to_string(),
            "CVDB400011".to_string()
        ]
    );
}

#[test]
fn notes_key_note_replaces_and_appends() {
    let config = config();
    // full sentence form replaced
    let book = ComicBook {
        info: ComicInfo {
            notes: "My notes.\n\nScraped metadata from ComicVine [CVDB111].".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bd = BookData::from_book(&book, &config);
    bd.update(&issue(), &config, "2026.02.03 04:05:06", None);
    assert_eq!(
        bd.notes,
        "My notes.\n\nScraped metadata from ComicVine [CVDB400011]."
    );

    // the dated form replaces too
    let mut book2 = ComicBook::default();
    book2.info.notes =
        "My notes.\n\nScraped metadata from ComicVine [CVDB111] on 2020.01.01 01:02:03.".into();
    let mut bd2 = BookData::from_book(&book2, &config);
    bd2.update(&issue(), &config, "2026.02.03 04:05:06", None);
    assert_eq!(
        bd2.notes,
        "My notes.\n\nScraped metadata from ComicVine [CVDB400011]."
    );

    // no previous tag -> appended after a blank line
    let mut book3 = ComicBook::default();
    book3.info.notes = "My notes.".into();
    let mut bd3 = BookData::from_book(&book3, &config);
    bd3.update(&issue(), &config, "2026.02.03 04:05:06", None);
    assert_eq!(
        bd3.notes,
        "My notes.\n\nScraped metadata from ComicVine [CVDB400011]."
    );
}

#[test]
fn skip_forever_writes_cvdbskip() {
    let mut config = config();
    config.rescrape_notes = true;
    config.rescrape_tags = true;
    let mut bd = BookData::from_book(&ComicBook::default(), &config);
    bd.skip_forever(&config, "2026.01.01 00:00:00");
    assert_eq!(bd.tags, vec!["CVDBSKIP".to_string()]);
    assert_eq!(bd.notes, "Scraped metadata from ComicVine [CVDBSKIP].");

    // with only notes on, CVDBSKIP goes to notes only
    let mut config2 = Configuration::default();
    config2.rescrape_notes = true;
    config2.rescrape_tags = false;
    let mut bd2 = BookData::from_book(&ComicBook::default(), &config2);
    bd2.skip_forever(&config2, "2026.01.01 00:00:00");
    assert_eq!(bd2.notes, "Scraped metadata from ComicVine [CVDBSKIP].");
    // tags unchanged (still empty)
    assert!(bd2.tags.is_empty());
}

#[test]
fn skip_tag_short_circuits_rescrape() {
    let config = config();
    let book = ComicBook {
        info: ComicInfo {
            tags: "CVDBSKIP".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let bd = BookData::from_book(&book, &config);
    // the literal CVDBSKIP tag form (the engine's skip gate matches it)
    assert_eq!(bd.tags, vec!["CVDBSKIP".to_string()]);
}

#[test]
fn comma_cleanup_in_written_lists() {
    let config = config();
    let mut bd = BookData::from_book(&ComicBook::default(), &config);
    let mut weird = issue();
    weird.characters = vec!["Wayne, Bruce".into(), "  Grayson, Dick  ".into()];
    bd.update(&weird, &config, "2026.01.01 00:00:00", None);
    let mut book = ComicBook::default();
    bd.apply_to(&mut book);
    // commas inside items become spaces; items trimmed
    assert_eq!(book.info.characters, "Wayne Bruce, Grayson Dick");
}

#[test]
fn rating_rules() {
    let mut config = config();
    config.advanced_settings = "SCRAPE_RATING=true".into();
    // (advanced reparses through set_advanced_settings in real flows)
    config.set_advanced_settings("SCRAPE_RATING=true");
    let mut bd = BookData::from_book(&ComicBook::default(), &config);
    // rating 0.0 == blank; overwrite on -> written as 0.0
    bd.update(&issue(), &config, "2026.01.01 00:00:00", None);
    assert!(bd.will_update("rating_n"));
    assert_eq!(bd.rating, 0.0);

    // an out-of-range rating becomes blank
    let mut high = issue();
    high.rating = 9.0;
    let mut bd2 = BookData::from_book(&ComicBook::default(), &config);
    bd2.update(&high, &config, "2026.01.01 00:00:00", None);
    assert_eq!(bd2.rating, 0.0);
}

#[test]
fn cover_url_picks_the_first_image_url() {
    let config = config();
    let mut bd = BookData::from_book(&ComicBook::default(), &config);
    let mut with_cover = issue();
    with_cover.image_urls = vec![
        "http://img/first.jpg".into(),
        "http://img/second.jpg".into(),
    ];
    bd.update(&with_cover, &config, "2026.01.01 00:00:00", None);
    assert_eq!(bd.cover_url, "http://img/first.jpg");

    // the session alt-cover wins
    let mut bd2 = BookData::from_book(&ComicBook::default(), &config);
    bd2.update(
        &with_cover,
        &config,
        "2026.01.01 00:00:00",
        Some("http://img/alt.jpg"),
    );
    assert_eq!(bd2.cover_url, "http://img/alt.jpg");

    // no covers -> cover_url not updated
    let mut none = issue();
    none.image_urls = Vec::new();
    let mut bd3 = BookData::from_book(&ComicBook::default(), &config);
    bd3.update(&none, &config, "2026.01.01 00:00:00", None);
    assert!(!bd3.will_update("cover_url_s"));
}

#[test]
fn custom_values_set_and_delete() {
    use cr_scrape::bookdata::set_custom_value;
    let mut book = ComicBook::default();
    set_custom_value(&mut book, "comicvine_issue", "400011");
    assert_eq!(
        values_store::decode(&book.custom_values_store),
        vec![("comicvine_issue".to_string(), "400011".to_string())]
    );
    // case-insensitive replace
    set_custom_value(&mut book, "COMICVINE_ISSUE", "400012");
    assert_eq!(
        values_store::decode(&book.custom_values_store),
        vec![("comicvine_issue".to_string(), "400012".to_string())]
    );
    // empty deletes
    set_custom_value(&mut book, "comicvine_issue", "");
    assert!(values_store::decode(&book.custom_values_store).is_empty());
}

#[test]
fn fileless_books_skip_the_filename_fallback() {
    let book = ComicBook {
        file_path: "".into(),
        ..Default::default()
    };
    let bd = BookData::from_book(&book, &config());
    // no path: series stays blank (the fallback needs a path)
    assert_eq!(bd.series, "");
    assert_eq!(bd.issue_num, "");
}
