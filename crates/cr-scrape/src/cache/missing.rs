//! Which issues of a volume the library does not hold (Phase 15 T7).
//!
//! The cache skeleton knows every issue of a volume. The library knows
//! which issue numbers it holds. The difference is the gap the user
//! fills with fileless books.
//!
//! Everything here is pure. The caller reads the library, and the
//! caller creates the books.

use std::collections::BTreeMap;

use super::IssueSkeleton;

/// One issue the library does not hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MissingIssue {
    pub issue_id: i64,
    pub issue_number: String,
    pub cover_date: Option<String>,
    pub name: Option<String>,
}

/// Compares two issue numbers the way a reader does.
///
/// `1`, `01`, and `001` are the same issue. `1A` and `1a` are the same
/// issue. `1.5` and `1½` are not compared here, because Comic Vine
/// writes the half-issue form in more than one way and the caller can
/// still see both rows.
pub fn normalize_number(number: &str) -> String {
    let trimmed = number.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return String::new();
    }
    // Strip the leading zeros of the leading digit run only, so `007`
    // becomes `7` and `0.5` keeps its zero.
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    if digits.len() > 1 && !trimmed.starts_with("0.") {
        let rest = &trimmed[digits.len()..];
        let stripped = digits.trim_start_matches('0');
        let head = if stripped.is_empty() { "0" } else { stripped };
        return format!("{head}{rest}");
    }
    trimmed
}

/// The issues of `volume` that no owned number matches.
///
/// An owned number that the volume does not list is ignored, because
/// it is the user's business, not a defect.
pub fn missing_issues(volume: &[IssueSkeleton], owned_numbers: &[String]) -> Vec<MissingIssue> {
    let owned: std::collections::BTreeSet<String> = owned_numbers
        .iter()
        .map(|n| normalize_number(n))
        .filter(|n| !n.is_empty())
        .collect();

    volume
        .iter()
        .filter(|issue| {
            let key = normalize_number(&issue.issue_number);
            // An issue with no number cannot be matched, so it is
            // always offered.
            key.is_empty() || !owned.contains(&key)
        })
        .map(|issue| MissingIssue {
            issue_id: issue.issue_id,
            issue_number: issue.issue_number.clone(),
            cover_date: issue.cover_date.clone(),
            name: issue.name.clone(),
        })
        .collect()
}

/// The Comic Vine volume id that a set of books agrees on.
///
/// A library series carries a volume id only after a scrape. The
/// function returns the id that the most books name; `None` means the
/// caller must ask the user to pick the volume.
pub fn volume_id_of(series_keys: impl IntoIterator<Item = String>) -> Option<i64> {
    let mut votes: BTreeMap<i64, usize> = BTreeMap::new();
    for key in series_keys {
        if let Ok(id) = key.trim().parse::<i64>() {
            if id > 0 {
                *votes.entry(id).or_default() += 1;
            }
        }
    }
    votes
        .into_iter()
        .max_by_key(|&(id, count)| (count, std::cmp::Reverse(id)))
        .map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(id: i64, number: &str) -> IssueSkeleton {
        IssueSkeleton {
            issue_id: id,
            volume_id: 771,
            issue_number: number.to_string(),
            ..Default::default()
        }
    }

    fn numbers(missing: &[MissingIssue]) -> Vec<&str> {
        missing.iter().map(|m| m.issue_number.as_str()).collect()
    }

    #[test]
    fn leading_zeros_do_not_make_a_new_issue() {
        assert_eq!(normalize_number("1"), "1");
        assert_eq!(normalize_number("01"), "1");
        assert_eq!(normalize_number("001"), "1");
        assert_eq!(normalize_number(" 007 "), "7");
        assert_eq!(normalize_number("010"), "10");
        assert_eq!(normalize_number("0"), "0");
        assert_eq!(normalize_number("000"), "0");
    }

    #[test]
    fn a_letter_suffix_keeps_its_issue_and_loses_its_case() {
        assert_eq!(normalize_number("1A"), "1a");
        assert_eq!(normalize_number("01a"), "1a");
        assert_eq!(normalize_number("AU"), "au");
    }

    #[test]
    fn a_decimal_number_keeps_its_leading_zero() {
        assert_eq!(normalize_number("0.5"), "0.5");
        assert_eq!(normalize_number("1.5"), "1.5");
    }

    #[test]
    fn an_empty_number_normalizes_to_empty() {
        assert_eq!(normalize_number(""), "");
        assert_eq!(normalize_number("   "), "");
    }

    #[test]
    fn the_gap_is_what_the_library_does_not_hold() {
        let volume = vec![issue(1, "1"), issue(2, "2"), issue(3, "3"), issue(4, "4")];
        let owned = vec!["1".to_string(), "3".to_string()];
        assert_eq!(numbers(&missing_issues(&volume, &owned)), vec!["2", "4"]);
    }

    #[test]
    fn a_padded_owned_number_still_matches() {
        let volume = vec![issue(1, "1"), issue(2, "02"), issue(3, "3")];
        let owned = vec!["001".to_string(), "2".to_string()];
        assert_eq!(numbers(&missing_issues(&volume, &owned)), vec!["3"]);
    }

    #[test]
    fn an_owned_number_the_volume_does_not_list_changes_nothing() {
        let volume = vec![issue(1, "1")];
        let owned = vec!["1".to_string(), "99".to_string()];
        assert!(missing_issues(&volume, &owned).is_empty());
    }

    #[test]
    fn an_issue_with_no_number_is_always_offered() {
        let volume = vec![issue(1, "1"), issue(2, "")];
        let owned = vec!["1".to_string()];
        assert_eq!(numbers(&missing_issues(&volume, &owned)), vec![""]);
    }

    #[test]
    fn an_empty_library_is_missing_everything() {
        let volume = vec![issue(1, "1"), issue(2, "2")];
        assert_eq!(missing_issues(&volume, &[]).len(), 2);
    }

    #[test]
    fn the_missing_row_carries_the_detail_the_dialog_shows() {
        let volume = vec![IssueSkeleton {
            issue_id: 92_469,
            volume_id: 771,
            issue_number: "1".into(),
            cover_date: Some("2000-11-01".into()),
            name: Some("Quelque part entre les ombres".into()),
        }];
        let got = missing_issues(&volume, &[]);
        assert_eq!(got[0].issue_id, 92_469);
        assert_eq!(got[0].cover_date.as_deref(), Some("2000-11-01"));
        assert_eq!(
            got[0].name.as_deref(),
            Some("Quelque part entre les ombres")
        );
    }

    #[test]
    fn the_volume_id_is_the_one_the_most_books_name() {
        let keys = ["771", "771", "999", "", "not a number", "0", "-3"];
        assert_eq!(volume_id_of(keys.iter().map(|s| s.to_string())), Some(771));
    }

    #[test]
    fn no_scraped_book_means_no_volume_id() {
        assert_eq!(volume_id_of(Vec::<String>::new()), None);
        assert_eq!(
            volume_id_of(["", "  ", "x"].iter().map(|s| s.to_string())),
            None
        );
    }

    #[test]
    fn a_tie_picks_the_lower_id_so_the_answer_is_stable() {
        assert_eq!(
            volume_id_of(["999", "771"].iter().map(|s| s.to_string())),
            Some(771)
        );
        assert_eq!(
            volume_id_of(["771", "999"].iter().map(|s| s.to_string())),
            Some(771)
        );
    }
}
