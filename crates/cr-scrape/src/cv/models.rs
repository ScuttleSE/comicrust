//! Port of the plugin's `dbmodels.py` — the lightweight immutable
//! reference models the database layer returns. Equality and hashing
//! ride the database key only (C# `_cmpkey_s` semantics), which is
//! what the plugin's ref sets rely on.

/// Minimal reference details about one issue (`IssueRef`).
#[derive(Clone, Debug)]
pub struct IssueRef {
    pub issue_num: String,
    pub issue_key: i64,
    pub title: String,
    pub thumb_url: Option<String>,
}

impl IssueRef {
    /// `issue_key` must be non-empty (a database memento).
    pub fn new(issue_num: &str, issue_key: i64, title: &str, thumb_url: Option<String>) -> Self {
        IssueRef {
            issue_num: issue_num.trim().to_string(),
            issue_key,
            title: title.trim().to_string(),
            thumb_url: thumb_url.filter(|u| !u.trim().is_empty()),
        }
    }
}

impl PartialEq for IssueRef {
    fn eq(&self, other: &Self) -> bool {
        self.issue_key == other.issue_key
    }
}

impl Eq for IssueRef {}

impl std::hash::Hash for IssueRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.issue_key.hash(state);
    }
}

impl std::fmt::Display for IssueRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Issue #{} ({})", self.issue_num, self.issue_key)
    }
}

/// Minimal reference details about one series (`SeriesRef`).
#[derive(Clone, Debug)]
pub struct SeriesRef {
    pub series_key: i64,
    series_name: String,
    pub volume_year: i32,
    pub publisher: String,
    pub issue_count: i32,
    pub thumb_url: Option<String>,
}

impl SeriesRef {
    /// A sparse ref: just the key, everything unknown (used when a
    /// previous scrape identified the series).
    pub fn sparse(series_key: i64) -> Self {
        SeriesRef::new(series_key, "", -1, "", -1, None).expect("sparse ref always valid")
    }

    /// `series_name` falls back to `"Series <key>"` when blank, and
    /// `&amp;` decodes (C# constructor).
    pub fn new(
        series_key: i64,
        series_name: &str,
        volume_year: i32,
        publisher: &str,
        issue_count: i32,
        thumb_url: Option<String>,
    ) -> Result<Self, String> {
        let mut name = series_name.trim().replace("&amp;", "&");
        if name.is_empty() {
            name = format!("Series {series_key}");
        }
        Ok(SeriesRef {
            series_key,
            series_name: name,
            volume_year: volume_year.max(-1),
            publisher: publisher.trim().to_string(),
            issue_count: issue_count.max(0),
            thumb_url: thumb_url.filter(|u| !u.trim().is_empty()),
        })
    }

    pub fn series_name(&self) -> &str {
        &self.series_name
    }
}

impl PartialEq for SeriesRef {
    fn eq(&self, other: &Self) -> bool {
        self.series_key == other.series_key
    }
}

impl Eq for SeriesRef {}

impl std::hash::Hash for SeriesRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.series_key.hash(state);
    }
}

impl std::fmt::Display for SeriesRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.series_name, self.series_key)
    }
}

/// All the data obtainable about a single comic book issue (the
/// plugin's `Issue` class). Fields start with the C# defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub issue_key: i64,
    pub series_key: String,
    pub issue_num: String,
    pub title: String,
    pub series_name: String,
    pub publisher: String,
    pub imprint: String,
    pub summary: String,
    pub webpage: String,
    pub pub_day: i32,
    pub pub_month: i32,
    pub pub_year: i32,
    pub rel_day: i32,
    pub rel_month: i32,
    pub rel_year: i32,
    pub volume_year: i32,
    /// Always 0.0 in the C# — the API value is never parsed
    /// (`SCRAPE_RATING` actually gates the associated-images fetch).
    pub rating: f32,
    pub crossovers: Vec<String>,
    pub characters: Vec<String>,
    pub teams: Vec<String>,
    pub locations: Vec<String>,
    pub writers: Vec<String>,
    pub pencillers: Vec<String>,
    pub inkers: Vec<String>,
    pub cover_artists: Vec<String>,
    pub editors: Vec<String>,
    pub colorists: Vec<String>,
    pub letterers: Vec<String>,
    pub image_urls: Vec<String>,
}

impl Issue {
    pub fn new(issue_key: i64) -> Self {
        Issue {
            issue_key,
            series_key: String::new(),
            issue_num: String::new(),
            title: String::new(),
            series_name: String::new(),
            publisher: String::new(),
            imprint: String::new(),
            summary: String::new(),
            webpage: String::new(),
            pub_day: -1,
            pub_month: -1,
            pub_year: -1,
            rel_day: -1,
            rel_month: -1,
            rel_year: -1,
            volume_year: -1,
            rating: 0.0,
            crossovers: Vec::new(),
            characters: Vec::new(),
            teams: Vec::new(),
            locations: Vec::new(),
            writers: Vec::new(),
            pencillers: Vec::new(),
            inkers: Vec::new(),
            cover_artists: Vec::new(),
            editors: Vec::new(),
            colorists: Vec::new(),
            letterers: Vec::new(),
            image_urls: Vec::new(),
        }
    }
}
