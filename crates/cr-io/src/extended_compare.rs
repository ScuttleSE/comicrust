//! Port of `cYo.Common.Text.ExtendedStringComparer.Compare` in the
//! `IgnoreCase` mode (the mode `ArchiveComicProvider.OnParse` uses to
//! sort pages). The full C# algorithm is reproduced, including the
//! number scanning with leading-zero tiebreaks; only the single-char
//! culture comparison is approximated (case-folded ordinal) — see the
//! tolerances in the crate docs.

use std::cmp::Ordering;

fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}

fn is_letter(c: char) -> bool {
    c.is_alphabetic()
}

/// Single-character comparison for the non-digit branch. The C# uses
/// `string.Compare(s1, i1, s2, i2, 1, ignoreCase)` (culture, one char)
/// for letters and `string.Compare(..., 1)` (culture, case-sensitive)
/// otherwise. We fold case for letters and compare ordinals otherwise.
fn compare_chars(c1: char, c2: char, ignore_case: bool) -> Ordering {
    if ignore_case && is_letter(c1) && is_letter(c2) {
        return c1.to_lowercase().cmp(c2.to_lowercase());
    }
    c1.cmp(&c2)
}

/// Port of `ExtendedStringComparer.Compare(s1, s2, IgnoreCase)`.
pub fn extended_compare_ignore_case(s1: &str, s2: &str) -> Ordering {
    if s1.is_empty() {
        return if s2.is_empty() {
            Ordering::Equal
        } else {
            Ordering::Less
        };
    }
    if s2.is_empty() {
        return Ordering::Greater;
    }
    if s1 == s2 {
        return Ordering::Equal;
    }

    let v1: Vec<char> = s1.chars().collect();
    let v2: Vec<char> = s2.chars().collect();
    let (len1, len2) = (v1.len(), v2.len());
    let mut i1 = 0usize;
    let mut i2 = 0usize;

    // Leading letter-or-digit gate: a letter-or-digit start sorts
    // after a non-letter-or-digit start.
    let lod1 = v1[0].is_alphanumeric();
    let lod2 = v2[0].is_alphanumeric();
    if lod1 && !lod2 {
        return Ordering::Greater;
    }
    if !lod1 && lod2 {
        return Ordering::Less;
    }

    loop {
        let c1 = v1[i1];
        let c2 = v2[i2];
        let digit1 = is_digit(c1);
        let digit2 = is_digit(c2);

        if !digit1 && !digit2 {
            if c1 != c2 {
                if is_letter(c1) && is_letter(c2) {
                    let ord = compare_chars(c1, c2, true);
                    if ord != Ordering::Equal {
                        return ord;
                    }
                } else if is_letter(c1) || is_letter(c2) {
                    // A letter sorts after a non-letter.
                    return if is_letter(c1) {
                        Ordering::Greater
                    } else {
                        Ordering::Less
                    };
                } else {
                    let ord = compare_chars(c1, c2, false);
                    if ord != Ordering::Equal {
                        return ord;
                    }
                }
            }
        } else if !(digit1 && digit2) {
            // A digit sorts before a non-digit.
            return if digit1 {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        } else {
            let ord = compare_numbers(&v1, &mut i1, &v2, &mut i2);
            if ord != Ordering::Equal {
                return ord;
            }
        }

        i1 += 1;
        i2 += 1;

        if i1 >= len1 {
            return if i2 >= len2 {
                Ordering::Equal
            } else {
                Ordering::Less
            };
        }
        if i2 >= len2 {
            return Ordering::Greater;
        }
    }
}

/// Port of `ExtendedStringComparer.CompareNumbers` (default mode: no
/// zeroes-first flag). On return `i1`/`i2` sit on the last digit of
/// each number; the caller advances past them.
fn compare_numbers(v1: &[char], i1: &mut usize, v2: &[char], i2: &mut usize) -> Ordering {
    let start1 = *i1;
    let start2 = *i2;
    let mut nz_start1 = *i1;
    let mut nz_start2 = *i2;
    let mut end1 = *i1;
    let mut end2 = *i2;
    scan_number(v1, *i1, &mut nz_start1, &mut end1);
    scan_number(v2, *i2, &mut nz_start2, &mut end2);

    *i1 = end1 - 1;
    *i2 = end2 - 1;

    // significandLength1 is the digit count of s2's number and vice
    // versa — the swap is in the C# source and the comparison intent.
    let significand1 = end2 - nz_start2;
    let significand2 = end1 - nz_start1;

    if significand1 == significand2 {
        let mut cursor1 = nz_start1;
        let mut cursor2 = nz_start2;
        while cursor1 <= *i1 {
            let diff = (v1[cursor1] as i32) - (v2[cursor2] as i32);
            if diff != 0 {
                return diff.cmp(&0);
            }
            cursor1 += 1;
            cursor2 += 1;
        }
        // Equal values: fewer total digits (fewer leading zeros) sort
        // first — wait, the C# tiebreak sends the longer total first:
        // "007" sorts before "07" and "7".
        let total1 = end1 - start1;
        let total2 = end2 - start2;
        if total1 == total2 {
            return Ordering::Equal;
        }
        return if total1 > total2 {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }

    if significand1 > significand2 {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

/// Port of `ExtendedStringComparer.ScanNumber`.
fn scan_number(s: &[char], start: usize, nz_start: &mut usize, end: &mut usize) {
    let length = s.len();
    *nz_start = start;
    *end = start;
    let mut parsing_leading_zeros = true;
    let mut c = s[*end];
    loop {
        if parsing_leading_zeros {
            if c == '0' {
                *nz_start += 1;
            } else {
                parsing_leading_zeros = false;
            }
        }
        *end += 1;
        if *end < length {
            c = s[*end];
            if is_digit(c) {
                continue;
            }
        }
        break;
    }
}

#[cfg(test)]
mod tests {
    use super::extended_compare_ignore_case as cmp;

    fn ordered(items: &mut [&str]) {
        items.sort_by(|a, b| cmp(a, b));
    }

    #[test]
    fn page_number_natural_order() {
        let mut names = [
            "page10.jpg",
            "page2.jpg",
            "page1.jpg",
            "Page03.jpg",
            "page011.jpg",
        ];
        ordered(&mut names);
        assert_eq!(
            names,
            [
                "page1.jpg",
                "page2.jpg",
                "Page03.jpg",
                "page10.jpg",
                "page011.jpg"
            ]
        );
    }

    #[test]
    fn zero_padding_tiebreak() {
        // Same value: more leading zeros sort first (C# total-length
        // tiebreak, longer total first).
        assert_eq!(cmp("007.jpg", "07.jpg"), std::cmp::Ordering::Less);
        assert_eq!(cmp("07.jpg", "7.jpg"), std::cmp::Ordering::Less);
    }

    #[test]
    fn digits_before_letters_and_punctuation() {
        assert_eq!(cmp("1.jpg", "a.jpg"), std::cmp::Ordering::Less);
        assert_eq!(cmp("a.jpg", "!cover.jpg"), std::cmp::Ordering::Greater);
    }

    #[test]
    fn case_insensitive_with_case_tiebreak() {
        assert_eq!(cmp("abc.jpg", "BCD.jpg"), std::cmp::Ordering::Less);
        assert_eq!(cmp("page.jpg", "PAGE.jpg"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn empty_and_equal() {
        assert_eq!(cmp("", ""), std::cmp::Ordering::Equal);
        assert_eq!(cmp("", "a"), std::cmp::Ordering::Less);
        assert_eq!(cmp("a", ""), std::cmp::Ordering::Greater);
        assert_eq!(cmp("same.jpg", "same.jpg"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn mixed_numbers_and_text() {
        let mut names = [
            "Chapter 2 - Fix.jpg",
            "Chapter 12 - Fix.jpg",
            "Chapter 1 - Intro.jpg",
        ];
        ordered(&mut names);
        assert_eq!(
            names,
            [
                "Chapter 1 - Intro.jpg",
                "Chapter 2 - Fix.jpg",
                "Chapter 12 - Fix.jpg",
            ]
        );
    }
}
