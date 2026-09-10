//! Port of the Comic Vine Scraper's `bookdata.py` + `comicbook.py` +
//! `pluginbookdata.py` — the scraped field set for one book: the read
//! side (from a `ComicBook`, with the proposed/Shadow values and the
//! filename fallback), the update massage rules, and the write side
//! (back into a `ComicBook` clone; the UI commits it through
//! `library::apply_edited`).

use std::collections::HashSet;

use chrono::Datelike;
use cr_core::model::comic_book::{values_store, ComicBook};
use cr_core::xml::scalar::{CrDateTime, DateKind};
use regex::Regex;
use std::sync::LazyLock;

use crate::config::Configuration;
use crate::cv::models::Issue;
use crate::fnameparser;

/// The magic skip flag (database independent): marks a book to never
/// scrape again.
pub const CVDBSKIP: &str = "CVDBSKIP";

const ISSUE_KEY_CUSTOM: &str = "comicvine_issue";
const SERIES_KEY_CUSTOM: &str = "comicvine_volume";

/// The property names of the C# `updated_properties` set; the cover
/// thumbnail download (fileless books only) stays the caller's job.
const UPDATABLE: &[&str] = &[
    "series_s",
    "issue_num_s",
    "volume_year_n",
    "pub_year_n",
    "pub_month_n",
    "pub_day_n",
    "rel_year_n",
    "rel_month_n",
    "rel_day_n",
    "format_s",
    "title_s",
    "crossovers_sl",
    "summary_s",
    "publisher_s",
    "imprint_s",
    "characters_sl",
    "teams_sl",
    "locations_sl",
    "writers_sl",
    "pencillers_sl",
    "inkers_sl",
    "colorists_sl",
    "letterers_sl",
    "cover_artists_sl",
    "editors_sl",
    "tags_sl",
    "notes_s",
    "path_s",
    "webpage_s",
    "cover_url_s",
    "rating_n",
    "issue_key_s",
    "series_key_s",
];

/// The scraped values for one book (`BookData`). Blank values: `""`
/// strings, `-1` numbers, `0.0` rating.
#[derive(Clone, Debug, PartialEq)]
pub struct BookData {
    pub series: String,
    pub issue_num: String,
    pub volume_year: i32,
    pub pub_year: i32,
    pub pub_month: i32,
    pub pub_day: i32,
    pub rel_year: i32,
    pub rel_month: i32,
    pub rel_day: i32,
    pub format: String,
    pub title: String,
    pub crossovers: Vec<String>,
    pub summary: String,
    pub publisher: String,
    pub imprint: String,
    pub characters: Vec<String>,
    pub teams: Vec<String>,
    pub locations: Vec<String>,
    pub writers: Vec<String>,
    pub pencillers: Vec<String>,
    pub inkers: Vec<String>,
    pub colorists: Vec<String>,
    pub letterers: Vec<String>,
    pub cover_artists: Vec<String>,
    pub editors: Vec<String>,
    pub tags: Vec<String>,
    pub notes: String,
    pub path: String,
    pub webpage: String,
    pub cover_url: String,
    pub rating: f32,
    pub page_count: i32,
    pub issue_key: String,
    pub series_key: String,
    updated: HashSet<&'static str>,
}

/// Reads the custom value (case-insensitive keys, `""` when missing).
fn get_custom_value(book: &ComicBook, key: &str) -> String {
    values_store::decode(&book.custom_values_store)
        .into_iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
        .unwrap_or_default()
}

/// Writes the custom value (empty values delete the key).
pub fn set_custom_value(book: &mut ComicBook, key: &str, value: &str) {
    let mut pairs = values_store::decode(&book.custom_values_store);
    if value.is_empty() {
        pairs.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
    } else if let Some(slot) = pairs.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
        slot.1 = value.to_string();
    } else {
        pairs.push((key.to_string(), value.to_string()));
    }
    book.custom_values_store = values_store::encode(&pairs);
}

/// `CVDB<id>` (the ComicVine key tag); None for non-positive keys.
fn key_tag_for(issue_key: i64) -> Option<String> {
    if issue_key > 0 {
        Some(format!("CVDB{issue_key}"))
    } else {
        None
    }
}

/// `cvdb._parse_key_tag`: `CVDB<number>`, or the legacy
/// `ComicVine[<number>` form.
fn parse_key_tag(text: &str) -> Option<i64> {
    static CVDB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)CVDB(\d{1,})").unwrap());
    static LEGACY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)ComicVine.?[(\[](\d{1,})").unwrap());
    if let Some(caps) = CVDB.captures(text) {
        if let Some(n) = caps.get(1) {
            if let Ok(v) = n.as_str().parse() {
                return Some(v);
            }
        }
    }
    let legacy = LEGACY.captures(text)?;
    legacy.get(1).and_then(|n| n.as_str().parse().ok())
}

/// The C# `split(s)`: comma split (the BookData list setters strip
/// items and drop blanks on read).
fn split_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(String::from)
        .collect()
}

/// The C# `cleanup`: commas inside a value become spaces (a comma
/// followed by whitespace keeps the whitespace).
static COMMA_WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r",(\s+)").unwrap());

fn cleanup_text(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let s = COMMA_WS.replace_all(s, "$1");
    s.replace(',', " ")
}

fn join_list(items: &[String]) -> String {
    items
        .iter()
        .map(|s| cleanup_text(s))
        .collect::<Vec<_>>()
        .join(", ")
}

impl BookData {
    /// The read side (`PluginBookData.__init__` +
    /// `ComicBook.__parse_extra_details_from_path`).
    pub fn from_book(book: &ComicBook, config: &Configuration) -> BookData {
        let info = &book.info;
        let released = &book.released_time.naive;
        let mut bd = BookData {
            series: info.series.clone(),
            issue_num: info.number.clone(),
            volume_year: shadow_volume(book),
            pub_year: info.year,
            pub_month: info.month,
            pub_day: info.day,
            rel_year: released.year(),
            rel_month: released.month() as i32,
            rel_day: released.day() as i32,
            format: shadow_format(book),
            title: info.title.clone(),
            crossovers: split_list(&info.alternate_series),
            summary: info.summary.clone(),
            publisher: info.publisher.clone(),
            imprint: info.imprint.clone(),
            characters: split_list(&info.characters),
            teams: split_list(&info.teams),
            locations: split_list(&info.locations),
            writers: split_list(&info.writer),
            pencillers: split_list(&info.penciller),
            inkers: split_list(&info.inker),
            colorists: split_list(&info.colorist),
            letterers: split_list(&info.letterer),
            cover_artists: split_list(&info.cover_artist),
            editors: split_list(&info.editor),
            tags: split_list(&info.tags),
            notes: info.notes.clone(),
            path: book.file_path.clone(),
            webpage: info.web.clone(),
            cover_url: String::new(),
            rating: info.community_rating,
            page_count: info.page_count,
            issue_key: get_custom_value(book, ISSUE_KEY_CUSTOM),
            series_key: get_custom_value(book, SERIES_KEY_CUSTOM),
            updated: UPDATABLE
                .iter()
                .filter(|p| **p != "page_count_n" && **p != "path_s")
                .copied()
                .collect(),
        };
        bd.parse_extra_details_from_path(config);
        bd
    }

    /// `__parse_extra_details_from_path`: when any of series/number/
    /// year is missing, fill the gaps from the filename.
    fn parse_extra_details_from_path(&mut self, config: &Configuration) {
        let no_series = self.series.is_empty();
        let no_issue = self.issue_num.is_empty();
        let no_year = self.pub_year == -1;
        if self.path.is_empty() || !(no_series || no_issue || no_year) {
            return;
        }
        let filename = basename(&self.path);
        let mut extracted = if !config.advanced().alt_search_regex.is_empty() {
            fnameparser::regex(filename, &config.advanced().alt_search_regex)
        } else {
            None
        };
        if extracted.is_none() {
            extracted = Some(fnameparser::extract(filename));
        }
        let [series, issue, year] = extracted.expect("extract never fails");
        if no_series {
            self.series = series;
        }
        if no_issue {
            self.issue_num = issue;
        }
        if no_year {
            self.pub_year = year
                .parse::<i32>()
                .ok()
                .filter(|y| *y > 0 && *y < 9999)
                .unwrap_or(-1);
        }
    }

    /// `ComicBook.update(issue)`: copies the issue data in through
    /// every per-field rule. `now` is the scrape timestamp for the
    /// Notes tag; `alt_cover_url` overrides the cover url (the C#
    /// session alt-cover choice).
    pub fn update(
        &mut self,
        issue: &Issue,
        config: &Configuration,
        now: &str,
        alt_cover_url: Option<&str>,
    ) {
        let ow = config.overwrite_existing;
        let blanks = config.ignore_blanks;

        // series: ALWAYS ignore-blanks
        match massage_string(
            &issue.series_name,
            &self.series,
            config.update_series,
            ow,
            true,
        ) {
            Some(v) => self.series = v,
            None => self.dont_update("series_s"),
        }
        // issue number: ALWAYS ignore-blanks
        match massage_string(
            &issue.issue_num,
            &self.issue_num,
            config.update_number,
            ow,
            true,
        ) {
            Some(v) => self.issue_num = v,
            None => self.dont_update("issue_num_s"),
        }
        // title
        match massage_string(&issue.title, &self.title, config.update_title, ow, blanks) {
            Some(v) => self.title = v,
            None => self.dont_update("title_s"),
        }
        // crossovers
        match massage_list(
            &issue.crossovers,
            &self.crossovers,
            config.update_crossovers,
            ow,
            blanks,
        ) {
            Some(v) => self.crossovers = v,
            None => self.dont_update("crossovers_sl"),
        }
        // summary
        match massage_string(
            &issue.summary,
            &self.summary,
            config.update_summary,
            ow,
            blanks,
        ) {
            Some(v) => self.summary = v,
            None => self.dont_update("summary_s"),
        }
        // release (in store) date
        match massage_date(
            (issue.rel_year, issue.rel_month, issue.rel_day),
            (self.rel_year, self.rel_month, self.rel_day),
            config.update_released,
            ow,
            blanks,
        ) {
            Some((y, m, d)) => {
                self.rel_year = y;
                self.rel_month = m;
                self.rel_day = d;
            }
            None => {
                self.dont_update("rel_year_n");
                self.dont_update("rel_month_n");
                self.dont_update("rel_day_n");
            }
        }
        // published (cover) date
        match massage_date(
            (issue.pub_year, issue.pub_month, issue.pub_day),
            (self.pub_year, self.pub_month, self.pub_day),
            config.update_published,
            ow,
            blanks,
        ) {
            Some((y, m, d)) => {
                self.pub_year = y;
                self.pub_month = m;
                self.pub_day = d;
            }
            None => {
                self.dont_update("pub_year_n");
                self.dont_update("pub_month_n");
                self.dont_update("pub_day_n");
            }
        }
        // volume (valid only above zero)
        match massage_number(
            issue.volume_year,
            self.volume_year,
            -1,
            config.update_volume,
            ow,
            blanks,
            &|x| x > 0,
        ) {
            Some(v) => self.volume_year = v,
            None => self.dont_update("volume_year_n"),
        }

        // publisher and imprint, through the conversions
        let (publisher, imprint) = convert_publishers(issue, config);
        match massage_string(&imprint, &self.imprint, config.update_imprint, ow, blanks) {
            Some(v) => self.imprint = v,
            None => self.dont_update("imprint_s"),
        }
        match massage_string(
            &publisher,
            &self.publisher,
            config.update_publisher,
            ow,
            blanks,
        ) {
            Some(v) => self.publisher = v,
            None => self.dont_update("publisher_s"),
        }

        // the list fields
        macro_rules! list_field {
            ($new:expr, $field:ident, $flag:expr, $name:literal) => {
                match massage_list($new, &self.$field, $flag, ow, blanks) {
                    Some(v) => self.$field = v,
                    None => self.dont_update($name),
                }
            };
        }
        list_field!(
            &issue.characters,
            characters,
            config.update_characters,
            "characters_sl"
        );
        list_field!(&issue.teams, teams, config.update_teams, "teams_sl");
        list_field!(
            &issue.locations,
            locations,
            config.update_locations,
            "locations_sl"
        );
        list_field!(&issue.writers, writers, config.update_writer, "writers_sl");
        list_field!(
            &issue.pencillers,
            pencillers,
            config.update_penciller,
            "pencillers_sl"
        );
        list_field!(&issue.inkers, inkers, config.update_inker, "inkers_sl");
        list_field!(
            &issue.colorists,
            colorists,
            config.update_colorist,
            "colorists_sl"
        );
        list_field!(
            &issue.letterers,
            letterers,
            config.update_letterer,
            "letterers_sl"
        );
        list_field!(
            &issue.cover_artists,
            cover_artists,
            config.update_cover_artist,
            "cover_artists_sl"
        );
        list_field!(&issue.editors, editors, config.update_editor, "editors_sl");

        // webpage
        match massage_string(
            &issue.webpage,
            &self.webpage,
            config.update_webpage,
            ow,
            blanks,
        ) {
            Some(v) => self.webpage = v,
            None => self.dont_update("webpage_s"),
        }
        // rating (SCRAPE_RATING gates it; the C# value is always 0.0)
        match massage_rating(
            issue.rating,
            self.rating,
            0.0,
            config.advanced().update_rating,
            ow,
            blanks,
            &|x| (0.0..=5.0).contains(&x),
        ) {
            Some(v) => self.rating = v,
            None => self.dont_update("rating_n"),
        }

        // tags and notes carry the key tag; overwrite always
        let issue_key = Some(issue.issue_key);
        let new_tags = add_key_to_tags(&self.tags.clone(), issue_key);
        match massage_list(&new_tags, &self.tags, config.rescrape_tags, true, false) {
            Some(v) => self.tags = v,
            None => self.dont_update("tags_sl"),
        }
        let new_notes = add_key_to_notes(
            &self.notes.clone(),
            issue_key,
            config.advanced().note_scrape_date,
            now,
        );
        match massage_string(&new_notes, &self.notes, config.rescrape_notes, true, false) {
            Some(v) => self.notes = v,
            None => self.dont_update("notes_s"),
        }

        // the scrape keys: always written
        match massage_string(
            &issue.issue_key.to_string(),
            &self.issue_key,
            true,
            true,
            false,
        ) {
            Some(v) => self.issue_key = v,
            None => self.dont_update("issue_key_s"),
        }
        match massage_string(&issue.series_key, &self.series_key, true, true, false) {
            Some(v) => self.series_key = v,
            None => self.dont_update("series_key_s"),
        }

        // cover url (the session alt-cover choice wins)
        match alt_cover_url
            .map(String::from)
            .or_else(|| issue.image_urls.first().cloned())
        {
            Some(url) => self.cover_url = url,
            None => self.dont_update("cover_url_s"),
        }
    }

    /// `ComicBook.skip_forever`: the magic CVDBSKIP flag goes into
    /// Tags and/or Notes (both when the two rescrape flags agree or
    /// tags is on — C# rule).
    pub fn skip_forever(&mut self, config: &Configuration, now: &str) {
        let notes = config.rescrape_notes;
        let tags = config.rescrape_tags;
        if notes == tags || tags {
            self.tags = add_key_to_tags(&self.tags.clone(), None);
        }
        if notes == tags || notes {
            self.notes = add_key_to_notes(
                &self.notes.clone(),
                None,
                config.advanced().note_scrape_date,
                now,
            );
        }
    }

    /// True when the property will be written by `apply_to`.
    pub fn will_update(&self, prop: &str) -> bool {
        self.updated.contains(prop)
    }

    /// The write side (`PluginBookData.update`): writes every still-
    /// updated property into the book. The cover thumbnail download
    /// (fileless books only) is the caller's job (T8).
    pub fn apply_to(&self, book: &mut ComicBook) {
        let u = |name: &str| self.updated.contains(name);
        if u("series_s") {
            book.info.series = self.series.clone();
        }
        if u("issue_num_s") {
            book.info.number = self.issue_num.clone();
        }
        if u("volume_year_n") {
            book.info.volume = self.volume_year;
        }
        if u("title_s") {
            book.info.title = self.title.clone();
        }
        if u("crossovers_sl") {
            book.info.alternate_series = join_list(&self.crossovers);
        }
        if u("summary_s") {
            book.info.summary = self.summary.clone();
        }
        if u("publisher_s") {
            book.info.publisher = self.publisher.clone();
        }
        if u("imprint_s") {
            book.info.imprint = self.imprint.clone();
        }
        if u("characters_sl") {
            book.info.characters = join_list(&self.characters);
        }
        if u("teams_sl") {
            book.info.teams = join_list(&self.teams);
        }
        if u("locations_sl") {
            book.info.locations = join_list(&self.locations);
        }
        if u("writers_sl") {
            book.info.writer = join_list(&self.writers);
        }
        if u("pencillers_sl") {
            book.info.penciller = join_list(&self.pencillers);
        }
        if u("inkers_sl") {
            book.info.inker = join_list(&self.inkers);
        }
        if u("colorists_sl") {
            book.info.colorist = join_list(&self.colorists);
        }
        if u("letterers_sl") {
            book.info.letterer = join_list(&self.letterers);
        }
        if u("cover_artists_sl") {
            book.info.cover_artist = join_list(&self.cover_artists);
        }
        if u("editors_sl") {
            book.info.editor = join_list(&self.editors);
        }
        if u("tags_sl") {
            book.info.tags = join_list(&self.tags);
        }
        if u("notes_s") {
            book.info.notes = self.notes.clone();
        }
        if u("webpage_s") {
            book.info.web = self.webpage.clone();
        }
        if u("rating_n") {
            book.info.community_rating = self.rating;
        }
        if u("issue_key_s") {
            set_custom_value(book, ISSUE_KEY_CUSTOM, &self.issue_key);
        }
        if u("series_key_s") {
            set_custom_value(book, SERIES_KEY_CUSTOM, &self.series_key);
        }

        // dates: ReleasedTime only when ALL three are present; the
        // published date writes progressively (year, then month, then
        // day — each gated on the previous ones)
        if u("rel_year_n")
            && u("rel_month_n")
            && u("rel_day_n")
            && self.rel_year != -1
            && self.rel_month != -1
            && self.rel_day != -1
        {
            if let Some(naive) = chrono::NaiveDate::from_ymd_opt(
                self.rel_year,
                self.rel_month as u32,
                self.rel_day as u32,
            )
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            {
                book.released_time = CrDateTime {
                    naive,
                    kind: DateKind::Unspecified,
                };
            }
        }
        if u("pub_year_n") && self.pub_year != -1 {
            book.info.year = self.pub_year;
        }
        if u("pub_year_n") && u("pub_month_n") && self.pub_year != -1 && self.pub_month != -1 {
            book.info.month = self.pub_month;
        }
        if u("pub_year_n")
            && u("pub_month_n")
            && u("pub_day_n")
            && self.pub_year != -1
            && self.pub_month != -1
            && self.pub_day != -1
        {
            book.info.day = self.pub_day;
        }
    }

    fn dont_update(&mut self, prop: &'static str) {
        self.updated.remove(prop);
    }
}

/// `__update_publishers`: the user imprints, the convert-imprints
/// rule, the publisher aliases, and the self-imprint nullify.
fn convert_publishers(issue: &Issue, config: &Configuration) -> (String, String) {
    let advanced = config.advanced();
    let mut publisher = issue.publisher.clone();
    let mut imprint = issue.imprint.clone();

    // 1. user-defined imprints override previously applied ones
    let key = if !imprint.is_empty() {
        imprint.clone()
    } else {
        publisher.clone()
    };
    if !key.is_empty() {
        if let Some(parent) = advanced.user_imprints.get(&key.to_lowercase()) {
            publisher = parent.clone();
            imprint = key;
        }
    }
    // 2. an imprint may be listed as the publisher instead
    if !config.convert_imprints && !imprint.is_empty() {
        publisher = imprint.clone();
        imprint = String::new();
    }
    // 3. publisher aliases
    if let Some(alias) = advanced.publisher_aliases.get(&publisher.to_lowercase()) {
        publisher = alias.clone();
    }
    if let Some(alias) = advanced.publisher_aliases.get(&imprint.to_lowercase()) {
        imprint = alias.clone();
    }
    // 4. an imprint equal to its publisher nullifies the imprint
    if publisher == imprint {
        imprint = String::new();
    }
    (publisher, imprint)
}

/// `__massage_new_string`: returns the new value only when
/// update ∧ (overwrite ∨ old blank) ∧ ¬(ignoreblanks ∧ new blank).
fn massage_string(
    new: &str,
    old: &str,
    update: bool,
    overwrite: bool,
    ignoreblanks: bool,
) -> Option<String> {
    let new = new.trim();
    let old = old.trim();
    if update && (overwrite || old.is_empty()) && !(ignoreblanks && new.is_empty()) {
        Some(new.to_string())
    } else {
        None
    }
}

/// `__massage_new_string_list`.
fn massage_list(
    new: &[String],
    old: &[String],
    update: bool,
    overwrite: bool,
    ignoreblanks: bool,
) -> Option<Vec<String>> {
    let filtered = |items: &[String]| -> Vec<String> {
        items
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect()
    };
    let new = filtered(new);
    let old = filtered(old);
    if update && (overwrite || old.is_empty()) && (!ignoreblanks || !new.is_empty()) {
        Some(new)
    } else {
        None
    }
}

/// `__massage_new_number` for the int fields (validity failures
/// become the blank value).
fn massage_number(
    new: i32,
    old: i32,
    blank: i32,
    update: bool,
    overwrite: bool,
    ignoreblanks: bool,
    is_valid: &dyn Fn(i32) -> bool,
) -> Option<i32> {
    let new = if is_valid(new) { new } else { blank };
    if update && (overwrite || old == blank) && !(ignoreblanks && new == blank) {
        Some(new)
    } else {
        None
    }
}

/// The float form (rating).
fn massage_rating(
    new: f32,
    old: f32,
    blank: f32,
    update: bool,
    overwrite: bool,
    ignoreblanks: bool,
    is_valid: &dyn Fn(f32) -> bool,
) -> Option<f32> {
    let new = if is_valid(new) { new } else { blank };
    if update && (overwrite || old == blank) && !(ignoreblanks && new == blank) {
        Some(new)
    } else {
        None
    }
}

/// `__massage_new_date` (blank = (-1,-1,-1)).
fn massage_date(
    new: (i32, i32, i32),
    old: (i32, i32, i32),
    update: bool,
    overwrite: bool,
    ignoreblanks: bool,
) -> Option<(i32, i32, i32)> {
    const BLANK: (i32, i32, i32) = (-1, -1, -1);
    if update && (overwrite || old == BLANK) && !(ignoreblanks && new == BLANK) {
        Some(new)
    } else {
        None
    }
}

/// `__add_key_to_tags`: a new key tag replaces the existing one
/// (case-insensitive) or appends; `None` writes CVDBSKIP.
fn add_key_to_tags(tags: &[String], issue_key: Option<i64>) -> Vec<String> {
    let key_tag = match issue_key {
        Some(k) => key_tag_for(k),
        None => Some(CVDBSKIP.to_string()),
    };
    let tagstring = tags.join(", ").trim().to_string();
    let updated = match &key_tag {
        Some(key_tag) if !tagstring.is_empty() => {
            let prev = parse_key_tag(&tagstring);
            let mut result = tagstring.clone();
            let mut replaced = false;
            if let Some(prev) = prev {
                if let Some(prev_tag) = key_tag_for(prev) {
                    if let Ok(re) = Regex::new(&format!(r"(?i){}", regex::escape(&prev_tag))) {
                        if re.is_match(&result) {
                            result = re.replace_all(&result, key_tag.as_str()).to_string();
                            replaced = true;
                        }
                    }
                }
            }
            if replaced {
                result
            } else {
                let trimmed = result.strip_suffix(',').unwrap_or(&result);
                format!("{trimmed}, {key_tag}")
            }
        }
        Some(key_tag) => key_tag.clone(),
        None => tagstring,
    };
    updated.split(", ").map(String::from).collect()
}

/// `__add_key_to_notes`: replaces an existing key-note (the full
/// "Scraped metadata…" sentence) or a bare key tag; otherwise appends
/// the key-note after a blank line.
fn add_key_to_notes(notes: &str, issue_key: Option<i64>, include_date: bool, now: &str) -> String {
    let key_tag = match issue_key {
        Some(k) => key_tag_for(k),
        None => Some(CVDBSKIP.to_string()),
    };
    let date = if include_date {
        format!(" on {now}")
    } else {
        String::new()
    };
    let key_note = key_tag
        .as_ref()
        .map(|tag| format!("Scraped metadata from ComicVine [{tag}]{date}."))
        .unwrap_or_default();
    let notestring = notes.trim();

    if !key_note.is_empty() && !notestring.is_empty() {
        let prev = parse_key_tag(notestring);
        if let Some(prev) = prev {
            if let Some(prev_tag) = key_tag_for(prev) {
                // the full key-note sentence form
                let note_re = Regex::new(&format!(
                    r"(?i)Scraped.*?{}(]\.|.*?[\d\.]{{8,}} [\d:]{{6,}}\.)",
                    regex::escape(&prev_tag)
                ));
                if let Ok(re) = note_re {
                    if re.is_match(notestring) {
                        return re.replace_all(notestring, key_note.as_str()).to_string();
                    }
                }
                // a bare tag on its own: the tag swaps for the new tag
                let bare_re = Regex::new(&format!(r"(?i){}", regex::escape(&prev_tag)));
                if let Ok(re) = bare_re {
                    if re.is_match(notestring) {
                        if let Some(new_tag) = &key_tag {
                            return re.replace_all(notestring, new_tag.as_str()).to_string();
                        }
                    }
                }
            }
        }
        format!("{notestring}\n\n{key_note}")
    } else if !key_note.is_empty() {
        key_note
    } else {
        notestring.to_string()
    }
}

/// `crbook.ShadowVolume` (the proposed volume; the cr-engine book_view
/// logic, local copy to keep cr-scrape engine-free).
fn shadow_volume(book: &ComicBook) -> i32 {
    if !book.enable_proposed || book.info.volume != -1 {
        book.info.volume
    } else {
        cr_core::model::comic_name_info::from_file_path_configured(&book.file_path).volume
    }
}

/// `crbook.ShadowFormat`.
fn shadow_format(book: &ComicBook) -> String {
    if !book.enable_proposed || !book.info.format.is_empty() {
        book.info.format.clone()
    } else {
        cr_core::model::comic_name_info::from_file_path_configured(&book.file_path).format
    }
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}
