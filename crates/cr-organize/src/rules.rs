//! The exclude rules (`check_metadata_rules`,
//! `ExcludeRule`/`ExcludeGroup` in locommon.py).
//!
//! `Any` fires when one rule matches; `All` when every evaluable rule
//! matches. `Do not` mode skips books that qualify; `Only` mode moves
//! only books that qualify. An empty rule set lets everything
//! through. Nested groups evaluate recursively; a group with no
//! evaluable rules contributes nothing (the addon's `None`).

use cr_core::model::comic_book::ComicBook;
use cr_core::model::enums::{MangaYesNo, YesNo};

use crate::fields;
use crate::profile::{Profile, RuleNode};
use crate::series::SeriesIndex;

/// One rule's verdict. `Empty` is the addon's `None` (contributes
/// nothing, e.g. an empty group).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Matches,
    NoMatch,
    Empty,
}

/// The addon's comparison operators, on the string forms.
fn compare(operator: &str, field_data: &str, value: &str) -> Verdict {
    let hit = match operator {
        "is" => field_data == value,
        "is not" => value != field_data,
        "contains" => field_data.contains(value),
        "does not contain" => !field_data.contains(value),
        "greater than" => match (value.parse::<i64>(), field_data.parse::<i64>()) {
            (Ok(v), Ok(f)) => v < f,
            _ => value < field_data,
        },
        "less than" => match (value.parse::<i64>(), field_data.parse::<i64>()) {
            (Ok(v), Ok(f)) => v > f,
            _ => value > field_data,
        },
        _ => false,
    };
    if hit {
        Verdict::Matches
    } else {
        Verdict::NoMatch
    }
}

/// Evaluates one rule node against a book.
fn node_verdict(
    node: &RuleNode,
    book: &ComicBook,
    book_index: usize,
    series: &mut SeriesIndex,
) -> Verdict {
    match node {
        RuleNode::Rule(rule) => {
            let Some(field) = fields::rule_field(&rule.field) else {
                // A dead field name (the addon raises): contributes
                // nothing.
                return Verdict::Empty;
            };
            match field {
                "Manga" | "SeriesComplete" | "BlackAndWhite" => {
                    let Some(actual) = yes_no_value(book, field) else {
                        return Verdict::Empty;
                    };
                    let Some(expected) = parse_yes_no(field, &rule.value) else {
                        return Verdict::Empty;
                    };
                    if actual == expected {
                        Verdict::Matches
                    } else {
                        Verdict::NoMatch
                    }
                }
                "StartYear" | "StartMonth" => {
                    let start = series.earliest_book(book, book_index);
                    let data = if field == "StartMonth" {
                        start.info.month.to_string()
                    } else {
                        cr_engine::matcher::book_view::shadow_year(
                            start,
                            &cr_engine::matcher::book_view::proposed_cached(start),
                        )
                        .to_string()
                    };
                    compare(&rule.operator, &data, &rule.value)
                }
                _ => {
                    let Some(data) = fields::field_rule_text(book, field) else {
                        return Verdict::Empty;
                    };
                    compare(&rule.operator, &data, &rule.value)
                }
            }
        }
        RuleNode::Group { operator, rules } => {
            let mut count = 0usize;
            let mut total = 0usize;
            for child in rules {
                match node_verdict(child, book, book_index, series) {
                    Verdict::Empty => continue,
                    Verdict::Matches => count += 1,
                    Verdict::NoMatch => {}
                }
                total += 1;
            }
            if total == 0 {
                return Verdict::Empty;
            }
            let hit = if operator == "All" {
                count == total
            } else {
                count > 0
            };
            if hit {
                Verdict::Matches
            } else {
                Verdict::NoMatch
            }
        }
    }
}

fn yes_no_value(book: &ComicBook, field: &str) -> Option<String> {
    match field {
        "Manga" => Some(book.info.manga.to_xml()),
        "SeriesComplete" => Some(book.series_complete.to_xml()),
        "BlackAndWhite" => Some(book.info.black_and_white.to_xml()),
        _ => None,
    }
}

/// `get_yes_no_value`: the combobox value names, with the
/// "Yes (Right to Left)" special case for Manga.
fn parse_yes_no(field: &str, value: &str) -> Option<String> {
    if field == "Manga" {
        return match value {
            "Yes (Right to Left)" => Some(MangaYesNo::YesAndRightToLeft.to_xml()),
            "Yes" => Some(MangaYesNo::Yes.to_xml()),
            "No" => Some(MangaYesNo::No.to_xml()),
            "Unknown" => Some(MangaYesNo::Unknown.to_xml()),
            _ => None,
        };
    }
    match value {
        "Yes" => Some(YesNo::Yes.to_xml()),
        "No" => Some(YesNo::No.to_xml()),
        "Unknown" => Some(YesNo::Unknown.to_xml()),
        _ => None,
    }
}

/// Whether a book qualifies under the profile's rules
/// (`check_metadata_rules`).
///
/// A series-relative rule (`Start Year`/`Start Month`) needs the
/// series index over the same snapshot the run uses.
pub fn book_qualifies(
    book: &ComicBook,
    book_index: usize,
    profile: &Profile,
    series: &mut SeriesIndex,
) -> bool {
    let mut count = 0usize;
    let mut total = 0usize;
    for rule in &profile.exclude_rules {
        match node_verdict(rule, book, book_index, series) {
            Verdict::Empty => continue,
            Verdict::Matches => count += 1,
            Verdict::NoMatch => {}
        }
        total += 1;
    }

    if total == 0 {
        // No rules: the book qualifies regardless.
        return true;
    }

    let qualifies = if profile.exclude_operator == "All" {
        count == total
    } else {
        count > 0
    };

    if profile.exclude_mode == "Only" {
        qualifies
    } else if profile.exclude_mode == "Do not" {
        !qualifies
    } else {
        // Unknown mode: the addon falls off its if-chain, the caller
        // reads None as "skip".
        false
    }
}

/// `check_excluded_folders`: a book inside an excluded path is
/// skipped.
pub fn in_excluded_folder(book_path: &str, profile: &Profile) -> bool {
    profile
        .exclude_folders
        .iter()
        .any(|path| book_path.contains(path.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::ExcludeRule;

    fn book(series: &str, read: i32, pages: i32) -> ComicBook {
        let mut b = ComicBook::default();
        b.info.series = series.into();
        b.info.page_count = pages;
        b.last_page_read = read;
        b
    }

    fn rule(field: &str, operator: &str, value: &str) -> RuleNode {
        RuleNode::Rule(ExcludeRule {
            field: field.into(),
            operator: operator.into(),
            value: value.into(),
        })
    }

    fn profile_with(set: impl FnOnce(&mut Profile)) -> Profile {
        let mut p = Profile::default();
        set(&mut p);
        p
    }

    #[test]
    fn no_rules_qualify_everything() {
        let p = Profile::default();
        let books = vec![book("B", 0, 10)];
        let mut idx = SeriesIndex::new(&books);
        assert!(book_qualifies(&books[0], 0, &p, &mut idx));
    }

    #[test]
    fn any_operator_and_do_not_mode() {
        let p = profile_with(|p| {
            p.exclude_rules = vec![rule("Read Percentage", "greater than", "0")];
        });
        let books = vec![book("B", 0, 10), book("B", 5, 10)];
        let mut idx = SeriesIndex::new(&books);
        // Unread (0%): the rule does not fire → moved.
        assert!(book_qualifies(&books[0], 0, &p, &mut idx));
        // Started reading: the rule fires → skipped in Do-not mode.
        assert!(!book_qualifies(&books[1], 1, &p, &mut idx));
    }

    #[test]
    fn only_mode_inverts() {
        let p = profile_with(|p| {
            p.exclude_mode = "Only".into();
            p.exclude_rules = vec![rule("Read Percentage", "greater than", "0")];
        });
        let books = vec![book("B", 0, 10), book("B", 5, 10)];
        let mut idx = SeriesIndex::new(&books);
        // Only mode: the unread book does not match the rule -> skipped.
        assert!(!book_qualifies(&books[0], 0, &p, &mut idx));
        // The read book matches -> moved.
        assert!(book_qualifies(&books[1], 1, &p, &mut idx));
    }

    #[test]
    fn all_operator_requires_every_rule() {
        let p = profile_with(|p| {
            p.exclude_operator = "All".into();
            p.exclude_rules = vec![
                rule("Read Percentage", "greater than", "0"),
                rule("Series", "is", "B"),
            ];
        });
        let books = vec![book("B", 5, 10), book("Other", 5, 10)];
        let mut idx = SeriesIndex::new(&books);
        // Both rules fire on the B book -> skipped in Do-not mode.
        assert!(!book_qualifies(&books[0], 0, &p, &mut idx));
        // One rule fails on the Other book -> moved.
        assert!(book_qualifies(&books[1], 1, &p, &mut idx));
    }

    #[test]
    fn nested_group_and_empty_group() {
        let p = profile_with(|p| {
            p.exclude_rules = vec![RuleNode::Group {
                operator: "All".into(),
                rules: vec![
                    rule("Series", "is", "B"),
                    RuleNode::Group {
                        operator: "Any".into(),
                        rules: vec![],
                    },
                ],
            }];
        });
        let books = vec![book("B", 5, 10)];
        let mut idx = SeriesIndex::new(&books);
        // The empty inner group contributes nothing; the outer All
        // then needs only the Series rule -> fires -> skipped.
        assert!(!book_qualifies(&books[0], 0, &p, &mut idx));

        let p2 = profile_with(|p| {
            p.exclude_rules = vec![RuleNode::Group {
                operator: "Any".into(),
                rules: vec![],
            }];
        });
        let mut idx2 = SeriesIndex::new(&books);
        // An entirely empty top-level group: no evaluable rules →
        // everything qualifies.
        assert!(book_qualifies(&books[0], 0, &p2, &mut idx2));
    }

    #[test]
    fn yes_no_rule_values() {
        let p = profile_with(|p| {
            p.exclude_rules = vec![rule("Manga", "is", "Yes (Right to Left)")];
        });
        let mut b = book("B", 0, 10);
        b.info.manga = MangaYesNo::YesAndRightToLeft;
        let books = vec![b];
        let mut idx = SeriesIndex::new(&books);
        assert!(!book_qualifies(&books[0], 0, &p, &mut idx));
    }

    #[test]
    fn excluded_folders() {
        let p = profile_with(|p| {
            p.exclude_folders = vec!["/comics/incoming".into()];
        });
        assert!(in_excluded_folder("/comics/incoming/B.cbz", &p));
        assert!(!in_excluded_folder("/comics/main/B.cbz", &p));
    }
}
