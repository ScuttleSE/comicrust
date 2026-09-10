//! Port of the plugin's `matchscore.py` — how well a SeriesRef
//! matches a book. The prior-series set (the user's past choices)
//! comes from the plugin's persistent series file (the engine owns
//! the file; this type owns the state).

use std::collections::HashSet;
use std::sync::LazyLock;

use fancy_regex::Regex;

use crate::bookdata::BookData;
use crate::cv::models::SeriesRef;

#[derive(Clone, Debug, Default)]
pub struct MatchScore {
    prior_series: HashSet<String>,
}

impl MatchScore {
    pub fn new(prior_series: HashSet<String>) -> Self {
        MatchScore { prior_series }
    }

    /// The prior-series keys (for persistence).
    pub fn prior_series(&self) -> &HashSet<String> {
        &self.prior_series
    }

    /// `record_choice`: records a user series choice; returns true
    /// when the key is new (the engine then persists).
    pub fn record_choice(&mut self, series_ref: Option<&SeriesRef>) -> bool {
        let key = series_ref
            .map(|r| r.series_key.to_string())
            .unwrap_or_default();
        if key.is_empty() || self.prior_series.contains(&key) {
            return false;
        }
        self.prior_series.insert(key)
    }

    /// `compute_n`: the match score. Higher is closer; can be
    /// negative. `current_year` comes from the engine (determinism
    /// for tests).
    pub fn compute(&self, book: &BookData, series_ref: &SeriesRef, current_year: i32) -> f64 {
        // 1. the namescore: matching words in the series name
        let mut bookname = book.series.clone();
        if !bookname.is_empty() && !book.format.is_empty() {
            bookname = format!("{} {}", bookname, book.format);
        }
        let mut serieswords = split(series_ref.series_name());
        let mut namescore = 0.0;
        for word in split(&bookname) {
            if let Some(pos) = serieswords.iter().position(|w| w == &word) {
                serieswords.remove(pos);
                namescore += 5.0;
            } else {
                namescore -= 1.0;
            }
        }
        namescore -= serieswords.len() as f64;

        // 2. a small boost for series the user chose in the past
        let priorscore = if self
            .prior_series
            .contains(&series_ref.series_key.to_string())
        {
            7.0
        } else {
            0.0
        };

        // 3. international "mirror" publishers get penalized
        let publisher = series_ref.publisher.to_lowercase();
        let publisherscore = if publisher.contains("panini")
            || publisher.contains("deagostina")
            || publisher == "marvel italia"
            || publisher == "marvel uk"
            || publisher == "semic_as"
            || publisher == "abril"
        {
            -6.0
        } else {
            0.0
        };

        // 4. the bookscore: the issue number must fit the series
        //    issue count (a ±100 step function); large series always
        //    score well
        let booknumber: f64 = if book.issue_num.is_empty() {
            -1000.0
        } else {
            let digits: String = book
                .issue_num
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                .collect();
            digits.parse().unwrap_or(-999.0)
        };
        let fits =
            series_ref.issue_count > 100 || booknumber - 1.0 <= series_ref.issue_count as f64;
        let bookscore = if fits { 100.0 } else { -100.0 };

        // 5. the yearscore: a series that started after the book was
        //    published is a strong mismatch
        let is_valid_year = |y: i32| y > 1900 && y <= current_year + 1;
        let book_year = if is_valid_year(book.pub_year) {
            book.pub_year
        } else {
            book.rel_year
        };
        let mut yearscore = 0.0;
        if is_valid_year(book_year) {
            if !is_valid_year(series_ref.volume_year) {
                yearscore = -100.0;
            } else if series_ref.volume_year > book_year {
                yearscore = -500.0;
            }
        }

        // 6. the recency score: a tiny tie-breaker that prefers
        //    newer series
        let recency = if is_valid_year(series_ref.volume_year) {
            -((current_year - series_ref.volume_year) as f64) / 100.0
        } else {
            -1.0
        };

        bookscore + namescore + publisherscore + priorscore + yearscore + recency
    }
}

/// The C# `split`: lowercased word forms with the size variants
/// normalized.
fn split(name: &str) -> Vec<String> {
    static APOSTROPHE: LazyLock<Regex> = LazyLock::new(|| Regex::new("'").unwrap());
    static NON_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\W+").unwrap());
    static GIANT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)giant[- ]*sized?").unwrap());
    static KING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)king[- ]*sized?").unwrap());
    static ONESHOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)one[- ]*shot").unwrap());

    let name = name.to_lowercase();
    let name = APOSTROPHE.replace_all(&name, "");
    let name = NON_WORD.replace_all(&name, " ");
    let name = GIANT.replace_all(&name, "giant size");
    let name = KING.replace_all(&name, "king size");
    let name = ONESHOT.replace_all(&name, "one shot");
    name.split_whitespace().map(String::from).collect()
}
