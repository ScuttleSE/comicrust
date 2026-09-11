//! Per-book scan status (PORT ADDITION, user request 2026-09-11 — no
//! C# counterpart).
//!
//! A scan of tens of thousands of files must never stop and wait for a
//! person. A file that cannot be read, or whose content does not match
//! its name, therefore stays in the library and carries its verdict as
//! data. The user finds those books afterwards with a smart list and
//! fixes them in bulk.
//!
//! The verdict lives in `CustomValuesStore` under a `comicrust.scan.`
//! key prefix, for four measured reasons:
//!
//! 1. `CustomValuesStore` already round-trips through `ComicDb.xml`,
//!    so the database schema does not change and ComicRack still
//!    reads the file.
//! 2. The smart-list engine already has `ComicBookCustomValuesMatcher`
//!    ("Custom Value"), so no new matcher class is needed.
//! 3. The Properties editor already lists custom values, so the
//!    verdict is visible and removable by hand.
//! 4. The user's own `Tags` field stays untouched. Scanner state is
//!    not user metadata.
//!
//! The example smart-list rule is:
//!
//! ```text
//! Custom Value "comicrust.scan.status" is "Unreadable"
//! ```

use crate::model::comic_book::{values_store, ComicBook};

/// The status key. The value is one of the [`ScanStatus`] texts.
pub const STATUS_KEY: &str = "comicrust.scan.status";
/// The accessor's own message for a failed read.
pub const ERROR_KEY: &str = "comicrust.scan.error";
/// The format the CONTENT proved, when it contradicts the name.
pub const DETECTED_FORMAT_KEY: &str = "comicrust.scan.detected-format";
/// The format the FILE NAME claimed, when the content contradicts it.
pub const EXPECTED_FORMAT_KEY: &str = "comicrust.scan.expected-format";
/// When the verdict was recorded (RFC 3339, seconds).
pub const CHECKED_KEY: &str = "comicrust.scan.checked";
/// The `size:mtime` fingerprint the verdict was taken from. A scan
/// skips re-reading a known-bad file whose fingerprint is unchanged.
pub const FINGERPRINT_KEY: &str = "comicrust.scan.fingerprint";

/// Every key this module owns. Used to clear the whole group.
const ALL_KEYS: &[&str] = &[
    STATUS_KEY,
    ERROR_KEY,
    DETECTED_FORMAT_KEY,
    EXPECTED_FORMAT_KEY,
    CHECKED_KEY,
    FINGERPRINT_KEY,
];

/// The verdict a scan reached for one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStatus {
    /// The archive opened, but its content format is not the one its
    /// extension claims. The book IS readable, through the detected
    /// reader.
    FormatMismatch,
    /// The archive could not be read at all.
    Unreadable,
    /// The per-file deadline expired before the read finished.
    TimedOut,
    /// The user abandoned this file with "Skip Current File".
    Skipped,
}

impl ScanStatus {
    /// The text stored in the custom value, and shown in a smart-list
    /// rule. Keep these stable: a user's saved query holds the text.
    pub fn as_text(self) -> &'static str {
        match self {
            ScanStatus::FormatMismatch => "Format mismatch",
            ScanStatus::Unreadable => "Unreadable",
            ScanStatus::TimedOut => "Timed out",
            ScanStatus::Skipped => "Skipped",
        }
    }

    /// Parses [`ScanStatus::as_text`], case-insensitively.
    pub fn from_text(text: &str) -> Option<ScanStatus> {
        [
            ScanStatus::FormatMismatch,
            ScanStatus::Unreadable,
            ScanStatus::TimedOut,
            ScanStatus::Skipped,
        ]
        .into_iter()
        .find(|status| text.eq_ignore_ascii_case(status.as_text()))
    }

    /// True when the file could not be turned into pages at all. These
    /// books draw the red "!" chip; a mismatch draws the amber one.
    pub fn is_failure(self) -> bool {
        matches!(
            self,
            ScanStatus::Unreadable | ScanStatus::TimedOut | ScanStatus::Skipped
        )
    }
}

/// The stored verdict, if any.
pub fn status(book: &ComicBook) -> Option<ScanStatus> {
    get(book, STATUS_KEY).and_then(|v| ScanStatus::from_text(&v))
}

/// The stored failure message, if any.
pub fn error_text(book: &ComicBook) -> Option<String> {
    get(book, ERROR_KEY).filter(|v| !v.is_empty())
}

/// The stored detected format name, if any.
pub fn detected_format(book: &ComicBook) -> Option<String> {
    get(book, DETECTED_FORMAT_KEY).filter(|v| !v.is_empty())
}

/// The stored fingerprint of the file the verdict was taken from.
pub fn fingerprint(book: &ComicBook) -> Option<String> {
    get(book, FINGERPRINT_KEY).filter(|v| !v.is_empty())
}

/// The `size:mtime` fingerprint for a book's current file facts. A
/// changed fingerprint means the file changed, so a stored failure is
/// stale and the scanner retries the file.
pub fn fingerprint_of(size: i64, modified_ticks: i64) -> String {
    format!("{size}:{modified_ticks}")
}

/// One key of the group.
fn get(book: &ComicBook, key: &str) -> Option<String> {
    values_store::decode(&book.custom_values_store)
        .into_iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

/// Writes one key; an empty value removes it. Mirrors
/// `ComicBook.SetCustomValue` / `DeleteCustomValue`.
fn put(book: &mut ComicBook, key: &str, value: &str) {
    let mut pairs = values_store::decode(&book.custom_values_store);
    pairs.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
    if !value.is_empty() {
        pairs.push((key.to_string(), value.to_string()));
    }
    book.custom_values_store = values_store::encode(&pairs);
}

/// Removes every `comicrust.scan.` key. A later clean read calls this,
/// so a fixed file loses its marker without any user action.
pub fn clear(book: &mut ComicBook) -> bool {
    let before = book.custom_values_store.clone();
    let mut pairs = values_store::decode(&book.custom_values_store);
    pairs.retain(|(k, _)| !ALL_KEYS.iter().any(|owned| k.eq_ignore_ascii_case(owned)));
    book.custom_values_store = values_store::encode(&pairs);
    book.custom_values_store != before
}

/// What the scanner learned about one file.
#[derive(Debug, Clone, Default)]
pub struct ScanVerdict {
    pub status: Option<ScanStatus>,
    pub error: Option<String>,
    pub detected_format: Option<String>,
    pub expected_format: Option<String>,
    pub fingerprint: Option<String>,
}

impl ScanVerdict {
    /// A clean read: the marker group is removed.
    pub fn clean() -> ScanVerdict {
        ScanVerdict::default()
    }
}

/// Applies a verdict to a book. A verdict with no status clears the
/// whole group. Returns true when anything changed, so the caller only
/// marks the database dirty on a real change.
pub fn apply(book: &mut ComicBook, verdict: &ScanVerdict, now: &str) -> bool {
    let Some(status) = verdict.status else {
        return clear(book);
    };
    let before = book.custom_values_store.clone();
    put(book, STATUS_KEY, status.as_text());
    put(book, ERROR_KEY, verdict.error.as_deref().unwrap_or(""));
    put(
        book,
        DETECTED_FORMAT_KEY,
        verdict.detected_format.as_deref().unwrap_or(""),
    );
    put(
        book,
        EXPECTED_FORMAT_KEY,
        verdict.expected_format.as_deref().unwrap_or(""),
    );
    put(book, CHECKED_KEY, now);
    put(
        book,
        FINGERPRINT_KEY,
        verdict.fingerprint.as_deref().unwrap_or(""),
    );
    book.custom_values_store != before
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> ComicBook {
        ComicBook {
            file_path: "/comics/x.cbz".into(),
            ..Default::default()
        }
    }

    #[test]
    fn status_texts_round_trip() {
        for s in [
            ScanStatus::FormatMismatch,
            ScanStatus::Unreadable,
            ScanStatus::TimedOut,
        ] {
            assert_eq!(ScanStatus::from_text(s.as_text()), Some(s));
        }
        // The smart-list rule text a user types is case-insensitive.
        assert_eq!(
            ScanStatus::from_text("unreadable"),
            Some(ScanStatus::Unreadable)
        );
        assert_eq!(ScanStatus::from_text("nonsense"), None);
    }

    #[test]
    fn apply_then_clear_leaves_user_values_untouched() {
        let mut b = book();
        // A user's own custom value must survive both operations.
        b.custom_values_store = values_store::encode(&[("Location".into(), "Shelf".into())]);

        let verdict = ScanVerdict {
            status: Some(ScanStatus::Unreadable),
            error: Some("zip error: no central directory".into()),
            expected_format: Some("eComic (ZIP)".into()),
            fingerprint: Some(fingerprint_of(18_560_000, 42)),
            ..Default::default()
        };
        assert!(apply(&mut b, &verdict, "2026-09-11T18:00:00Z"));
        assert_eq!(status(&b), Some(ScanStatus::Unreadable));
        assert!(status(&b).unwrap().is_failure());
        assert_eq!(
            error_text(&b).as_deref(),
            Some("zip error: no central directory")
        );
        assert_eq!(fingerprint(&b).as_deref(), Some("18560000:42"));

        // A clean re-read drops the whole group, and only that group.
        assert!(apply(&mut b, &ScanVerdict::clean(), "2026-09-11T18:05:00Z"));
        assert_eq!(status(&b), None);
        assert_eq!(
            values_store::decode(&b.custom_values_store),
            vec![("Location".to_string(), "Shelf".to_string())]
        );
    }

    #[test]
    fn apply_reports_no_change_when_the_verdict_repeats() {
        let mut b = book();
        let verdict = ScanVerdict {
            status: Some(ScanStatus::FormatMismatch),
            detected_format: Some("eComic (RAR)".into()),
            expected_format: Some("eComic (ZIP)".into()),
            fingerprint: Some(fingerprint_of(33_473_001, 7)),
            ..Default::default()
        };
        assert!(apply(&mut b, &verdict, "2026-09-11T18:00:00Z"));
        // The same verdict at the same timestamp is not a change, so a
        // repeated scan does not dirty the database.
        assert!(!apply(&mut b, &verdict, "2026-09-11T18:00:00Z"));
        assert_eq!(status(&b), Some(ScanStatus::FormatMismatch));
        assert!(!status(&b).unwrap().is_failure());
        assert_eq!(detected_format(&b).as_deref(), Some("eComic (RAR)"));
    }

    #[test]
    fn clear_on_a_clean_book_reports_no_change() {
        let mut b = book();
        assert!(!clear(&mut b));
        assert!(!apply(&mut b, &ScanVerdict::clean(), "now"));
    }
}
