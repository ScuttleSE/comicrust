//! `ComicInfo` — the Anansi metadata record; also the serialized base of
//! `ComicBook`. Field order and defaults mirror `ComicInfo.cs`.

use crate::model::comic_page_info::ComicPageInfo;
use crate::model::enums::{MangaYesNo, YesNo};
use crate::xml::reader::{XmlError, XmlResult};
use crate::xml::{Emitter, Tok};
use std::io::Write;

#[derive(Clone, Debug, PartialEq)]
pub struct ComicInfo {
    pub title: String,
    pub series: String,
    pub number: String,
    pub count: i32,
    pub volume: i32,
    pub alternate_series: String,
    pub alternate_number: String,
    pub story_arc: String,
    pub series_group: String,
    pub alternate_count: i32,
    pub summary: String,
    pub notes: String,
    pub review: String,
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub writer: String,
    pub penciller: String,
    pub inker: String,
    pub colorist: String,
    pub letterer: String,
    pub cover_artist: String,
    pub editor: String,
    pub translator: String,
    pub publisher: String,
    pub imprint: String,
    pub genre: String,
    pub web: String,
    pub page_count: i32,
    pub language_iso: String,
    pub format: String,
    pub age_rating: String,
    pub black_and_white: YesNo,
    pub manga: MangaYesNo,
    pub preferred_front_cover: i32,
    pub characters: String,
    pub teams: String,
    pub main_character_or_team: String,
    pub locations: String,
    pub community_rating: f32,
    pub scan_information: String,
    pub tags: String,
    /// Raw unknown child elements (position: after `Tags`, before `Pages`).
    pub unparsed_elements: Vec<String>,
    /// Wrapper `<Pages>` is always written (lazy non-null getter in C#).
    pub pages: Vec<ComicPageInfo>,
}

impl Default for ComicInfo {
    fn default() -> Self {
        // Field initializer defaults from ComicInfo.cs: the "unknown"
        // count/volume/date fields are -1, not 0.
        ComicInfo {
            title: String::new(),
            series: String::new(),
            number: String::new(),
            count: -1,
            volume: -1,
            alternate_series: String::new(),
            alternate_number: String::new(),
            story_arc: String::new(),
            series_group: String::new(),
            alternate_count: -1,
            summary: String::new(),
            notes: String::new(),
            review: String::new(),
            year: -1,
            month: -1,
            day: -1,
            writer: String::new(),
            penciller: String::new(),
            inker: String::new(),
            colorist: String::new(),
            letterer: String::new(),
            cover_artist: String::new(),
            editor: String::new(),
            translator: String::new(),
            publisher: String::new(),
            imprint: String::new(),
            genre: String::new(),
            web: String::new(),
            page_count: 0,
            language_iso: String::new(),
            format: String::new(),
            age_rating: String::new(),
            black_and_white: YesNo::Unknown,
            manga: MangaYesNo::Unknown,
            preferred_front_cover: 0,
            characters: String::new(),
            teams: String::new(),
            main_character_or_team: String::new(),
            locations: String::new(),
            community_rating: 0.0,
            scan_information: String::new(),
            tags: String::new(),
            unparsed_elements: Vec::new(),
            pages: Vec::new(),
        }
    }
}

impl ComicInfo {
    /// Writes all ComicInfo elements. `write_pages`/element order is
    /// fixed: Tags, then any unparsed elements, then Pages.
    pub fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        let s = |v: &str| !v.is_empty();
        macro_rules! str_elem {
            ($e:expr, $n:literal, $v:expr) => {
                if s($v) {
                    e.text_elem($n, $v)?;
                }
            };
        }
        macro_rules! int_elem {
            ($e:expr, $n:literal, $v:expr, $d:expr) => {
                if $v != $d {
                    e.text_elem($n, &$v.to_string())?;
                }
            };
        }
        str_elem!(e, "Title", &self.title);
        str_elem!(e, "Series", &self.series);
        str_elem!(e, "Number", &self.number);
        int_elem!(e, "Count", self.count, -1);
        int_elem!(e, "Volume", self.volume, -1);
        str_elem!(e, "AlternateSeries", &self.alternate_series);
        str_elem!(e, "AlternateNumber", &self.alternate_number);
        str_elem!(e, "StoryArc", &self.story_arc);
        str_elem!(e, "SeriesGroup", &self.series_group);
        int_elem!(e, "AlternateCount", self.alternate_count, -1);
        str_elem!(e, "Summary", &self.summary);
        str_elem!(e, "Notes", &self.notes);
        str_elem!(e, "Review", &self.review);
        int_elem!(e, "Year", self.year, -1);
        int_elem!(e, "Month", self.month, -1);
        int_elem!(e, "Day", self.day, -1);
        str_elem!(e, "Writer", &self.writer);
        str_elem!(e, "Penciller", &self.penciller);
        str_elem!(e, "Inker", &self.inker);
        str_elem!(e, "Colorist", &self.colorist);
        str_elem!(e, "Letterer", &self.letterer);
        str_elem!(e, "CoverArtist", &self.cover_artist);
        str_elem!(e, "Editor", &self.editor);
        str_elem!(e, "Translator", &self.translator);
        str_elem!(e, "Publisher", &self.publisher);
        str_elem!(e, "Imprint", &self.imprint);
        str_elem!(e, "Genre", &self.genre);
        str_elem!(e, "Web", &self.web);
        int_elem!(e, "PageCount", self.page_count, 0);
        str_elem!(e, "LanguageISO", &self.language_iso);
        str_elem!(e, "Format", &self.format);
        str_elem!(e, "AgeRating", &self.age_rating);
        if self.black_and_white != YesNo::Unknown {
            e.text_elem("BlackAndWhite", &self.black_and_white.to_xml())?;
        }
        if self.manga != MangaYesNo::Unknown {
            e.text_elem("Manga", &self.manga.to_xml())?;
        }
        int_elem!(e, "PreferredFrontCover", self.preferred_front_cover, 0);
        str_elem!(e, "Characters", &self.characters);
        str_elem!(e, "Teams", &self.teams);
        str_elem!(e, "MainCharacterOrTeam", &self.main_character_or_team);
        str_elem!(e, "Locations", &self.locations);
        if self.community_rating != 0.0 {
            e.text_elem(
                "CommunityRating",
                &crate::xml::scalar::net_f32(self.community_rating),
            )?;
        }
        str_elem!(e, "ScanInformation", &self.scan_information);
        str_elem!(e, "Tags", &self.tags);
        for raw in &self.unparsed_elements {
            e.raw(raw)?;
        }
        e.start("Pages")?;
        for p in &self.pages {
            p.write_xml(e)?;
        }
        e.end()?;
        Ok(())
    }

    /// Writes the standalone `ComicInfo.xml` document bytes:
    /// declaration, root with the ComicRack `xsd`/`xsi` namespaces,
    /// body (`ComicInfo.Serialize`).
    pub fn serialize_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut e = Emitter::new(&mut out)?;
        e.root("ComicInfo")?;
        self.write_xml(&mut e)?;
        e.end()?;
        e.finish()?;
        Ok(out)
    }

    /// Reads ComicInfo child elements. `end_name` is the element name
    /// that terminates the struct (ComicInfo, or Book for ComicBook).
    /// Returns on the matching end token (consumed).
    pub fn read_children(
        r: &mut crate::xml::XmlReader<'_>,
        end_name: &str,
        info: &mut ComicInfo,
    ) -> XmlResult<()> {
        loop {
            match r.next_tok()? {
                Tok::Eof => return Err(XmlError("unexpected eof in ComicInfo".into())),
                Tok::End(n) if n == end_name => return Ok(()),
                Tok::Start(s) => {
                    if !read_info_elem(r, &s, info)? {
                        // Unknown element inside ComicInfo itself: capture.
                        info.unparsed_elements.push(r.capture_raw(&s)?);
                    }
                }
                Tok::Text(_) => {
                    return Err(XmlError("unexpected text in ComicInfo".into()));
                }
                _ => {}
            }
        }
    }
}

/// Reads one ComicInfo child element (already started). Returns true if
/// handled, false if the name is not a ComicInfo member (the caller owns
/// it; nothing is consumed in that case).
pub fn read_info_elem(
    r: &mut crate::xml::XmlReader<'_>,
    s: &crate::xml::Start,
    info: &mut ComicInfo,
) -> XmlResult<bool> {
    match s.name.as_str() {
        "Title" => info.title = r.text_content("Title")?,
        "Series" => info.series = r.text_content("Series")?,
        "Number" => info.number = r.text_content("Number")?,
        "Count" => info.count = read_i32(r, "Count")?,
        "Volume" => info.volume = read_i32(r, "Volume")?,
        "AlternateSeries" => info.alternate_series = r.text_content("AlternateSeries")?,
        "AlternateNumber" => info.alternate_number = r.text_content("AlternateNumber")?,
        "StoryArc" => info.story_arc = r.text_content("StoryArc")?,
        "SeriesGroup" => info.series_group = r.text_content("SeriesGroup")?,
        "AlternateCount" => info.alternate_count = read_i32(r, "AlternateCount")?,
        "Summary" => info.summary = r.text_content("Summary")?,
        "Notes" => info.notes = r.text_content("Notes")?,
        "Review" => info.review = r.text_content("Review")?,
        "Year" => info.year = read_i32(r, "Year")?,
        "Month" => info.month = read_i32(r, "Month")?,
        "Day" => info.day = read_i32(r, "Day")?,
        "Writer" => info.writer = r.text_content("Writer")?,
        "Penciller" => info.penciller = r.text_content("Penciller")?,
        "Inker" => info.inker = r.text_content("Inker")?,
        "Colorist" => info.colorist = r.text_content("Colorist")?,
        "Letterer" => info.letterer = r.text_content("Letterer")?,
        "CoverArtist" => info.cover_artist = r.text_content("CoverArtist")?,
        "Editor" => info.editor = r.text_content("Editor")?,
        "Translator" => info.translator = r.text_content("Translator")?,
        "Publisher" => info.publisher = r.text_content("Publisher")?,
        "Imprint" => info.imprint = r.text_content("Imprint")?,
        "Genre" => info.genre = r.text_content("Genre")?,
        "Web" => info.web = r.text_content("Web")?,
        "PageCount" => info.page_count = read_i32(r, "PageCount")?,
        "LanguageISO" => info.language_iso = r.text_content("LanguageISO")?,
        "Format" => info.format = r.text_content("Format")?,
        "AgeRating" => info.age_rating = r.text_content("AgeRating")?,
        "BlackAndWhite" => {
            let v = r.text_content("BlackAndWhite")?;
            info.black_and_white =
                YesNo::from_xml(&v).ok_or_else(|| XmlError(format!("bad YesNo: {v}")))?;
        }
        "Manga" => {
            let v = r.text_content("Manga")?;
            info.manga =
                MangaYesNo::from_xml(&v).ok_or_else(|| XmlError(format!("bad MangaYesNo: {v}")))?;
        }
        "PreferredFrontCover" => info.preferred_front_cover = read_i32(r, "PreferredFrontCover")?,
        "Characters" => info.characters = r.text_content("Characters")?,
        "Teams" => info.teams = r.text_content("Teams")?,
        "MainCharacterOrTeam" => {
            info.main_character_or_team = r.text_content("MainCharacterOrTeam")?
        }
        "Locations" => info.locations = r.text_content("Locations")?,
        "CommunityRating" => {
            let v = r.text_content("CommunityRating")?;
            info.community_rating = v
                .trim()
                .parse()
                .map_err(|_| XmlError(format!("bad CommunityRating: {v}")))?;
        }
        "ScanInformation" => info.scan_information = r.text_content("ScanInformation")?,
        "Tags" => info.tags = r.text_content("Tags")?,
        "Pages" => read_pages(r, &mut info.pages)?,
        _ => return Ok(false),
    }
    Ok(true)
}

/// Parses a standalone `<ComicInfo>` document (the in-archive metadata
/// file; used by cr-cli and later cr-io).
pub fn parse_root(reader: &mut crate::xml::XmlReader<'_>) -> XmlResult<ComicInfo> {
    let start = match reader.next_tok()? {
        Tok::Start(s) => s,
        Tok::Eof => return Err(XmlError("empty document".into())),
        _ => return Err(XmlError("unexpected token before root".into())),
    };
    if start.name != "ComicInfo" {
        return Err(XmlError(format!("unexpected root <{}>", start.name)));
    }
    let mut info = ComicInfo::default();
    ComicInfo::read_children(reader, "ComicInfo", &mut info)?;
    Ok(info)
}

fn read_i32(r: &mut crate::xml::XmlReader<'_>, elem: &str) -> XmlResult<i32> {
    let v = r.text_content(elem)?;
    v.trim()
        .parse()
        .map_err(|_| XmlError(format!("bad int in <{elem}>: {v}")))
}

fn read_pages(r: &mut crate::xml::XmlReader<'_>, pages: &mut Vec<ComicPageInfo>) -> XmlResult<()> {
    loop {
        match r.next_tok()? {
            Tok::Eof => return Err(XmlError("eof in Pages".into())),
            Tok::End(n) if n == "Pages" => return Ok(()),
            Tok::Start(s) if s.name == "Page" => {
                let page = ComicPageInfo::from_attrs(&s)?;
                // Page elements are attribute-only; consume any nested
                // content defensively.
                r.skip_element("Page")?;
                pages.push(page);
            }
            Tok::Start(s) => return Err(XmlError(format!("unexpected <{}> in Pages", s.name))),
            Tok::Text(_) => {}
            _ => {}
        }
    }
}
