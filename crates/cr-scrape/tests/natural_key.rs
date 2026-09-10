//! The plugin's `test_utils.py` natural-compare vectors drive the
//! port — same expectations, verbatim.

use cr_scrape::utils::{natural_compare, natural_key};
use std::cmp::Ordering;

fn sorted(unsorted: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = unsorted.iter().map(|s| s.to_string()).collect();
    v.sort_by(|a, b| natural_compare(a, b));
    v
}

fn assert_sorted(unsorted: &[&str], expected: &[&str]) {
    let got = sorted(unsorted);
    let expected: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
    assert_eq!(got, expected, "sorting {unsorted:?}");
}

#[test]
fn plain_numbers_sort_numerically() {
    assert_sorted(
        &["10", "1", "23", "5", "1.2", "11", "1.01", "55"],
        &["1", "1.01", "1.2", "5", "10", "11", "23", "55"],
    );
}

#[test]
fn numbers_with_letter_suffixes() {
    assert_sorted(
        &["10a", "1b", "2", "1c", "1a", "1"],
        &["1", "1a", "1b", "1c", "2", "10a"],
    );
    assert_sorted(
        &["a1", "a10", "a1.1", "aa2", "aa2.3"],
        &["a1", "a1.1", "a10", "aa2", "aa2.3"],
    );
}

#[test]
fn negative_and_mixed_decimals() {
    assert_sorted(
        &["-5", "-6", "-0.1", "-0.2", "-.11", ".3", "0.31"],
        &["-6", "-5", "-0.2", "-.11", "-0.1", ".3", "0.31"],
    );
}

#[test]
fn unicode_fractions_sort_as_values() {
    assert_sorted(
        &[
            "⅞", "⅝", "⅜", "⅛", "⅚", "⅙", "⅘", "⅗", "⅖", "⅕", "⅔", "⅓", "¾", "½", "¼",
        ],
        &[
            "⅛", "⅙", "⅕", "¼", "⅓", "⅜", "⅖", "½", "⅗", "⅝", "⅔", "¾", "⅘", "⅚", "⅞",
        ],
    );
}

#[test]
fn fractions_mix_with_decimals() {
    assert_sorted(
        &[".4", "0.6", "½", "6", "5", "5½", "5 ¾", " 5 ¼ "],
        &[".4", "½", "0.6", "5", " 5 ¼ ", "5½", "5 ¾", "6"],
    );
    assert_sorted(
        &["-.4", "-0.6", "-½", "-6", "-5", "-5½", "-5 ¾", "- 5 ¼ "],
        &["-6", "-5 ¾", "-5½", "- 5 ¼ ", "-5", "-0.6", "-½", "-.4"],
    );
}

#[test]
fn naturally_identical_keys_match() {
    let cases = [
        ("0.", "0"),
        ("0.a", "0a"),
        ("3.0", "3.00"),
        ("3.0", "3"),
        ("003", "3"),
        ("003a", "3a  "),
        ("003", "3  "),
        ("½", "0.5000"),
        ("3½", "0003.5"),
        ("6 au", "6au"),
        ("0.0 final", "0 final"),
        (".5", " 0 ½"),
        ("000.5", "0½"),
    ];
    for (a, b) in cases {
        assert_eq!(
            natural_key(a),
            natural_key(b),
            "natural_key({a:?}) != natural_key({b:?})"
        );
    }
}

#[test]
fn compare_returns_the_three_way_ordering() {
    assert_eq!(natural_compare("1", "2"), Ordering::Less);
    assert_eq!(natural_compare("2", "1"), Ordering::Greater);
    assert_eq!(natural_compare("5.0", "5"), Ordering::Equal);
}
