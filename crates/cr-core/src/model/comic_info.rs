//! `ComicInfo` — the Anansi metadata record; also the serialized base of
//! `ComicBook`. Field order and defaults mirror `ComicInfo.cs`.

use crate::model::comic_page_info::ComicPageInfo;
use crate::model::enums::{ComicPagePosition, ComicPageType, ImageRotation, MangaYesNo, YesNo};
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

    /// `ComicInfo.IsSameContent` — the exact C# field list (note the
    /// absent CommunityRating/PreferredFrontCover), with pages.
    pub fn is_same_content(&self, other: &ComicInfo, with_pages: bool) -> bool {
        self.writer == other.writer
            && self.publisher == other.publisher
            && self.imprint == other.imprint
            && self.inker == other.inker
            && self.penciller == other.penciller
            && self.title == other.title
            && self.number == other.number
            && self.count == other.count
            && self.summary == other.summary
            && self.series == other.series
            && self.volume == other.volume
            && self.alternate_series == other.alternate_series
            && self.alternate_number == other.alternate_number
            && self.alternate_count == other.alternate_count
            && self.story_arc == other.story_arc
            && self.series_group == other.series_group
            && self.year == other.year
            && self.month == other.month
            && self.day == other.day
            && self.notes == other.notes
            && self.review == other.review
            && self.genre == other.genre
            && self.colorist == other.colorist
            && self.editor == other.editor
            && self.translator == other.translator
            && self.letterer == other.letterer
            && self.cover_artist == other.cover_artist
            && self.web == other.web
            && self.language_iso == other.language_iso
            && self.page_count == other.page_count
            && self.format == other.format
            && self.age_rating == other.age_rating
            && self.black_and_white == other.black_and_white
            && self.manga == other.manga
            && self.characters == other.characters
            && self.teams == other.teams
            && self.main_character_or_team == other.main_character_or_team
            && self.locations == other.locations
            && self.scan_information == other.scan_information
            && self.tags == other.tags
            && (!with_pages || self.pages == other.pages)
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

// ---------- Page operations (the `ComicInfo` page-edit API) ----------
//
// The C# mutates the page list through `UpdatePageType`/
// `UpdatePageRotation`/`UpdatePagePosition`/`MovePages`/
// `ResetPageSequence`/`SortPages` (ComicInfo.cs) and reads the
// display order through `TranslateImageIndexToPage` and
// `FrontCoverPageIndex`. The Rust list has positional identity, so
// the mutations take indexes.

impl ComicInfo {
    /// `UpdatePageType(page, value)` (existing entries only; the C#
    /// `GetPage(page, add: true)` path is only used by writers that
    /// already know the page exists).
    pub fn update_page_type(&mut self, page: usize, value: ComicPageType) {
        if let Some(p) = self.pages.get_mut(page) {
            if p.page_type != value {
                p.page_type = value;
            }
        }
    }

    /// `UpdatePageRotation(page, value)`.
    pub fn update_page_rotation(&mut self, page: usize, value: ImageRotation) {
        if let Some(p) = self.pages.get_mut(page) {
            if p.rotation != value {
                p.rotation = value;
            }
        }
    }

    /// `UpdatePagePosition(page, value)`.
    pub fn update_page_position(&mut self, page: usize, value: ComicPagePosition) {
        if let Some(p) = self.pages.get_mut(page) {
            if p.page_position != value {
                p.page_position = value;
            }
        }
    }

    /// `GetPage(page, add: true)`: the entry at the list position,
    /// growing the list with sequential-index defaults (`new
    /// ComicPageInfo(Pages.Count)` parity) when out of range.
    /// Returns `None` for negative pages (the C# `Empty` page).
    pub fn get_page_mut_or_add(&mut self, page: i32) -> Option<&mut ComicPageInfo> {
        if page < 0 {
            return None;
        }
        let page = page as usize;
        while self.pages.len() <= page {
            let mut p = ComicPageInfo::default();
            p.set_image_index(self.pages.len() as i32);
            self.pages.push(p);
        }
        self.pages.get_mut(page)
    }

    /// `UpdatePageSize(page, width, height)`: the decoded pixel size
    /// into the page entry (short-truncating like the C# backing
    /// fields). Returns whether anything changed (the C# fires
    /// `PageChanged` only on a change).
    pub fn update_page_size(&mut self, page: i32, width: i32, height: i32) -> bool {
        let Some(p) = self.get_page_mut_or_add(page) else {
            return false;
        };
        // The C# `ImageWidth`/`ImageHeight` are `short` — the int
        // arguments truncate on assignment.
        let w = width as i16;
        let h = height as i16;
        if p.image_width != w || p.image_height != h {
            p.image_width = w;
            p.image_height = h;
            true
        } else {
            false
        }
    }

    /// `TranslateImageIndexToPage(imageIndex)`: the list position of
    /// the entry with that ImageIndex, else the index itself.
    pub fn translate_image_index_to_page(&self, image_index: i32) -> i32 {
        self.pages
            .iter()
            .position(|p| p.image_index() == image_index)
            .map(|p| p as i32)
            .unwrap_or(image_index)
    }

    /// `FrontCoverPageIndex`: the PreferredFrontCover-th FrontCover
    /// page, else the first page whose type is not `Other`, else 0.
    pub fn front_cover_page_index(&self) -> i32 {
        let covers: Vec<&ComicPageInfo> = self
            .pages
            .iter()
            .filter(|p| p.page_type == ComicPageType(1))
            .collect();
        let preferred = if covers.is_empty() {
            None
        } else {
            let idx = (self.preferred_front_cover as usize).min(covers.len() - 1);
            covers.get(idx).copied()
        };
        let pick = preferred.or_else(|| {
            self.pages
                .iter()
                .find(|p| p.page_type != ComicPageType(512))
        });
        match pick {
            Some(p) => self.translate_image_index_to_page(p.image_index()),
            None => 0,
        }
    }

    /// `MovePages(position, pages)` — the C# algorithm over indexes
    /// (the list position is the entry identity): each listed page is
    /// removed (a removal before the cursor shifts the cursor down),
    /// then re-inserted at the cursor and the cursor advances; a
    /// negative cursor appends (the C# `-1` quirk). `pages` lists the
    /// CURRENT indexes in list order.
    pub fn move_pages(&mut self, position: i32, pages: &[usize]) {
        // Identity = the ORIGINAL index of each entry (the C#
        // `IndexOf` reference semantics), tracked in a parallel list
        // while the interleaved remove/insert walk runs — the C#
        // moves one page per iteration, so the intermediate list
        // state matters for the insert positions.
        let mut orig: Vec<usize> = (0..self.pages.len()).collect();
        let mut cursor = position;
        for &target in pages {
            let Some(num) = orig.iter().position(|&o| o == target) else {
                continue;
            };
            let page = self.pages.remove(num);
            orig.remove(num);
            if (num as i32) < cursor {
                cursor -= 1;
            }
            if cursor < 0 {
                self.pages.push(page);
                orig.push(target);
                continue;
            }
            let at = (cursor as usize).min(self.pages.len());
            self.pages.insert(at, page);
            orig.insert(at, target);
            cursor += 1;
        }
    }

    /// `ResetPageSequence` — sort by ImageIndex.
    pub fn reset_page_sequence(&mut self) {
        self.pages.sort_by_key(|p| p.image_index());
    }

    /// `SortPages` by the stored entry key (the archive entry name):
    /// `compare` receives the two keys (ordinal, or the
    /// `ExtendedStringComparer` natural order — injected by the
    /// caller; cr-io owns that comparer).
    pub fn sort_pages_by_key(&mut self, compare: impl Fn(&str, &str) -> std::cmp::Ordering) {
        let keys: Vec<Option<String>> = self.pages.iter().map(|p| p.key.clone()).collect();
        let mut order: Vec<usize> = (0..self.pages.len()).collect();
        order.sort_by(|&a, &b| {
            let ka = keys[a].as_deref().unwrap_or("");
            let kb = keys[b].as_deref().unwrap_or("");
            compare(ka, kb)
        });
        let old = std::mem::take(&mut self.pages);
        for i in order {
            self.pages.push(old[i].clone());
        }
    }

    /// `ComicPageInfoCollection.SeekBookmark(page, direction)`:
    /// starting AT `page` (the C# callers pass `current + direction`),
    /// walk in `direction` until a page with a bookmark, else -1.
    pub fn seek_bookmark(&self, page: i32, direction: i32) -> i32 {
        let direction = direction.signum();
        let mut page = page;
        while page >= 0 && (page as usize) < self.pages.len() {
            if self.pages[page as usize]
                .bookmark
                .as_deref()
                .is_some_and(|b| !b.is_empty())
            {
                return page;
            }
            page += direction;
        }
        -1
    }
}

#[cfg(test)]
mod page_op_tests {
    use super::*;

    fn info_with(pages: &[(i32, ComicPageType)]) -> ComicInfo {
        let mut info = ComicInfo::default();
        for (idx, t) in pages {
            let mut p = ComicPageInfo::default();
            p.set_image_index(*idx);
            p.page_type = *t;
            info.pages.push(p);
        }
        info
    }

    #[test]
    fn update_ops_touch_only_the_target_page() {
        let mut info = info_with(&[(0, ComicPageType(8)), (1, ComicPageType(8))]);
        info.update_page_type(1, ComicPageType(1)); // FrontCover
        assert_eq!(info.pages[1].page_type, ComicPageType(1));
        assert_eq!(info.pages[0].page_type, ComicPageType(8));
        info.update_page_rotation(0, ImageRotation::Rotate90);
        assert_eq!(info.pages[0].rotation, ImageRotation::Rotate90);
        info.update_page_position(0, ComicPagePosition::Far);
        assert_eq!(info.pages[0].page_position, ComicPagePosition::Far);
        // Out-of-range is a no-op (the C# would create; our editor
        // never edits a page that does not exist).
        info.update_page_type(9, ComicPageType(1));
        assert_eq!(info.pages.len(), 2);
    }

    #[test]
    fn translate_image_index_prefers_the_list_hit() {
        let info = info_with(&[(5, ComicPageType(8)), (2, ComicPageType(8))]);
        assert_eq!(info.translate_image_index_to_page(2), 1);
        assert_eq!(info.translate_image_index_to_page(9), 9);
    }

    #[test]
    fn seek_bookmark_walks_the_direction_from_the_start() {
        let mut info = info_with(&[
            (0, ComicPageType(8)),
            (1, ComicPageType(8)),
            (2, ComicPageType(8)),
            (3, ComicPageType(8)),
        ]);
        info.pages[1].bookmark = Some("mid".into());
        info.pages[3].bookmark = Some("end".into());
        // `NavigateBookmark` seeks from current + direction.
        assert_eq!(info.seek_bookmark(2, -1), 1); // prev from page 2
        assert_eq!(info.seek_bookmark(0, -1), -1); // nothing before 0
        assert_eq!(info.seek_bookmark(0, 1), 1);
        assert_eq!(info.seek_bookmark(2, 1), 3);
        // Nothing after the last bookmark (the caller passes the
        // page after it).
        assert_eq!(info.seek_bookmark(4, 1), -1);
        // The start page counts (the C# callers pass current + dir,
        // so the current page's own bookmark never stops the seek).
        assert_eq!(info.seek_bookmark(1, 1), 1);
        assert_eq!(info.seek_bookmark(3, -1), 3);
    }

    #[test]
    fn front_cover_prefers_the_typed_page() {
        let mut info = info_with(&[
            (0, ComicPageType(8)), // Story
            (1, ComicPageType(1)), // FrontCover
            (2, ComicPageType(8)),
        ]);
        assert_eq!(info.front_cover_page_index(), 1);
        // PreferredFrontCover clamps into the cover count.
        info.preferred_front_cover = 99;
        assert_eq!(info.front_cover_page_index(), 1);
        // Without covers: the first page that is not Other.
        info.pages[1].page_type = ComicPageType(8);
        assert_eq!(info.front_cover_page_index(), 0);
        // All Other → 0.
        for p in &mut info.pages {
            p.page_type = ComicPageType(512);
        }
        assert_eq!(info.front_cover_page_index(), 0);
    }

    #[test]
    fn move_pages_follows_the_csharp_cursor_arithmetic() {
        // [a b c d]; move d (index 3) to the top.
        let mut info = info_with(&[
            (0, ComicPageType(8)),
            (1, ComicPageType(8)),
            (2, ComicPageType(8)),
            (3, ComicPageType(8)),
        ]);
        info.move_pages(0, &[3]);
        let order: Vec<i32> = info.pages.iter().map(|p| p.image_index()).collect();
        assert_eq!(order, vec![3, 0, 1, 2]);

        // Move [a b] (indexes 0,1) to the end (cursor 4). The C#
        // IndexOf tracks identity: a removes (cursor 4→3) and
        // inserts at 3 → [b c d a]; b then removes from the FRONT
        // (its index is 0 now, still < 4 → cursor 3) and inserts at
        // 3 — the END of [c d a] — so the pair keeps its order.
        let mut info = info_with(&[
            (0, ComicPageType(8)),
            (1, ComicPageType(8)),
            (2, ComicPageType(8)),
            (3, ComicPageType(8)),
        ]);
        info.move_pages(4, &[0, 1]);
        let order: Vec<i32> = info.pages.iter().map(|p| p.image_index()).collect();
        assert_eq!(order, vec![2, 3, 0, 1]);

        // A pair to the top keeps its order.
        let mut info = info_with(&[
            (0, ComicPageType(8)),
            (1, ComicPageType(8)),
            (2, ComicPageType(8)),
            (3, ComicPageType(8)),
        ]);
        info.move_pages(0, &[2, 3]);
        let order: Vec<i32> = info.pages.iter().map(|p| p.image_index()).collect();
        assert_eq!(order, vec![2, 3, 0, 1]);
    }

    #[test]
    fn reset_and_sort_orders() {
        let mut info = ComicInfo::default();
        for (i, name) in ["003.jpg", "0001.jpg", "002.jpg"].iter().enumerate() {
            let mut p = ComicPageInfo::default();
            p.set_image_index(i as i32);
            p.key = Some(name.to_string());
            info.pages.push(p);
        }
        // Reset: by ImageIndex (already ascending here).
        info.reset_page_sequence();
        assert_eq!(info.pages[0].image_index(), 0);
        // Ordinal by key: "0001" < "002" < "003".
        info.sort_pages_by_key(|a, b| a.cmp(b));
        let keys: Vec<String> = info.pages.iter().map(|p| p.key.clone().unwrap()).collect();
        assert_eq!(keys, vec!["0001.jpg", "002.jpg", "003.jpg"]);
    }

    #[test]
    fn update_page_size_writes_and_grows() {
        // The C# `UpdatePageSize` on a book with no page list grows
        // the list with sequential-index defaults (`GetPage(page,
        // add: true)`), then writes the short-truncating size.
        let mut info = ComicInfo::default();
        assert!(info.update_page_size(2, 800, 600));
        assert_eq!(info.pages.len(), 3);
        assert_eq!(info.pages[2].image_index(), 2);
        assert_eq!(
            (info.pages[2].image_width, info.pages[2].image_height),
            (800, 600)
        );
        // No change → false (the C# fires PageChanged only on a
        // change).
        assert!(!info.update_page_size(2, 800, 600));
        // Short truncation (the C# `short` backing fields).
        assert!(info.update_page_size(2, 70000, 600));
        assert_eq!(info.pages[2].image_width, 4464);
        // Negative pages are the C# `Empty` page — a no-op.
        assert!(!info.update_page_size(-1, 10, 10));
    }
}
