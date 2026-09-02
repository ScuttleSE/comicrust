//! Book comparers (the browser sort/group surface) and the .NET
//! `Random` port the smart-list `Randomize` selection relies on.
//!
//! The `Random` port is the .NET Framework algorithm: the C# persists
//! `LimitRandomSeed` in the database, so the shuffled selection must
//! reproduce exactly.

use cr_io::extended_compare::{
    extended_compare_ignore_articles_case, extended_compare_ignore_case,
};
use std::cmp::Ordering;

use crate::matcher::book_view;
use cr_core::model::comic_book::ComicBook;
use cr_core::model::comic_name_info::ComicNameInfo;

// ---------- .NET Framework Random (subtractive generator) ----------

/// `System.Random` (.NET Framework) — `Next()` and `Next(maxValue)`.
pub struct DotNetRandom {
    seed_array: [i32; 56],
    inext: usize,
    inextp: usize,
}

const MBIG: i32 = 2147483647;

impl DotNetRandom {
    /// `new Random(int seed)` — the Knuth subtractive shuffle, no
    /// warmup samples.
    pub fn new(seed: i32) -> Self {
        let subtraction = if seed == i32::MIN { MBIG } else { seed.abs() };
        let mut seed_array = [0i32; 56];
        let mut mj = 161803398 - subtraction;
        seed_array[55] = mj;
        let mut mk = 1;
        for i in 1..55 {
            let ii = (21 * i) % 55;
            seed_array[ii] = mk;
            mk = mj - mk;
            if mk < 0 {
                mk += MBIG;
            }
            mj = seed_array[ii];
        }
        for _ in 1..5 {
            for i in 1..56 {
                let idx = 1 + (i + 30) % 55;
                seed_array[i] -= seed_array[idx];
                if seed_array[i] < 0 {
                    seed_array[i] += MBIG;
                }
            }
        }
        DotNetRandom {
            seed_array,
            inext: 0,
            inextp: 21,
        }
    }

    fn internal_sample(&mut self) -> i32 {
        let mut loc_inext = self.inext + 1;
        if loc_inext >= 56 {
            loc_inext = 1;
        }
        let mut loc_inextp = self.inextp + 1;
        if loc_inextp >= 56 {
            loc_inextp = 1;
        }
        let mut ret = self.seed_array[loc_inext] - self.seed_array[loc_inextp];
        if ret == MBIG {
            ret -= 1;
        }
        if ret < 0 {
            ret += MBIG;
        }
        self.seed_array[loc_inext] = ret;
        self.inext = loc_inext;
        self.inextp = loc_inextp;
        ret
    }

    fn sample(&mut self) -> f64 {
        self.internal_sample() as f64 * (1.0 / MBIG as f64)
    }

    /// `Next(maxValue)` (>= 0, exclusive).
    pub fn next(&mut self, max_value: usize) -> usize {
        (self.sample() * max_value as f64) as usize
    }
}

/// `ListExtensions.Randomize(list, seed)`: `count` double-swap rounds
/// with two `Random.Next(count - 1)` draws per round.
pub fn randomize<T>(items: &mut [T], seed: i32) {
    let mut rng = DotNetRandom::new(seed);
    let count = items.len();
    for _ in 0..count {
        let index = rng.next(count.saturating_sub(1));
        let index2 = rng.next(count.saturating_sub(1));
        items.swap(index, index2);
    }
}

// ---------- book comparers ----------

/// `System.Guid.CompareTo` (Framework): the first four bytes compare as
/// a little-endian u32 (`_a`), the next two pairs as little-endian
/// u16s (`_b`, `_c` — big-endian in the byte layout, so these compare
/// via their LE u16 interpretation), then the remaining bytes
/// unsigned. The smart list's random selection sorts by Id before
/// shuffling, so this order decides which books a persisted seed picks.
pub fn guid_compare(
    a: &cr_core::xml::scalar::CrGuid,
    b: &cr_core::xml::scalar::CrGuid,
) -> Ordering {
    let x = a.as_bytes();
    let y = b.as_bytes();
    let le_u32 = |s: &[u8]| u32::from_le_bytes([s[0], s[1], s[2], s[3]]);
    let le_u16 = |s: &[u8]| u16::from_le_bytes([s[0], s[1]]);
    let ord = le_u32(&x[0..4]).cmp(&le_u32(&y[0..4]));
    if ord != Ordering::Equal {
        return ord;
    }
    let ord = le_u16(&x[4..6]).cmp(&le_u16(&y[4..6]));
    if ord != Ordering::Equal {
        return ord;
    }
    let ord = le_u16(&x[6..8]).cmp(&le_u16(&y[6..8]));
    if ord != Ordering::Equal {
        return ord;
    }
    x[8..16].cmp(&y[8..16])
}

fn prop(book: &ComicBook) -> ComicNameInfo {
    book_view::proposed(book)
}

/// `ComicBookFormatComparer` (ShadowFormat, ignore case).
pub fn compare_format(x: &ComicBook, y: &ComicBook) -> Ordering {
    book_view::shadow_format(x, &prop(x))
        .to_lowercase()
        .cmp(&book_view::shadow_format(y, &prop(y)).to_lowercase())
}

/// `ComicBookVolumeComparer` (ShadowVolume).
pub fn compare_volume(x: &ComicBook, y: &ComicBook) -> Ordering {
    book_view::shadow_volume(x, &prop(x)).cmp(&book_view::shadow_volume(y, &prop(y)))
}

/// `ComicBookNumberComparer` (ComicTextNumberFloat of ShadowNumber).
pub fn compare_number(x: &ComicBook, y: &ComicBook) -> Ordering {
    let (ix, nx) = book_view::compare_number(x, &prop(x));
    let (iy, ny) = book_view::compare_number(y, &prop(y));
    let ord = (ix, nx).partial_cmp(&(iy, ny)).unwrap_or(Ordering::Equal);
    // TextNumberFloat.CompareTo falls back to an ordinal text compare
    // when both are "equal" numerically or neither is a number.
    if ord == Ordering::Equal {
        book_view::shadow_number(x, &prop(x)).cmp(book_view::shadow_number(y, &prop(y)))
    } else {
        ord
    }
}

/// `ComicBookSeriesComparer`: series (IgnoreArticles | IgnoreCase),
/// then format, volume, number.
pub fn compare_series(x: &ComicBook, y: &ComicBook) -> Ordering {
    let ord = extended_compare_ignore_articles_case(
        book_view::shadow_series(x, &prop(x)),
        book_view::shadow_series(y, &prop(y)),
    );
    if ord != Ordering::Equal {
        return ord;
    }
    let ord = compare_format(x, y);
    if ord != Ordering::Equal {
        return ord;
    }
    let ord = compare_volume(x, y);
    if ord != Ordering::Equal {
        return ord;
    }
    compare_number(x, y)
}

/// `ComicBookComparer` used by the duplicate/series paths in the
/// browser: series (IgnoreArticles | IgnoreCase) only.
pub fn compare_series_name_only(x: &ComicBook, y: &ComicBook) -> Ordering {
    extended_compare_ignore_articles_case(
        book_view::shadow_series(x, &prop(x)),
        book_view::shadow_series(y, &prop(y)),
    )
}

/// Case-insensitive plain compare (the port's stand-in for the .NET
/// culture compare used by string sorters).
pub fn compare_ignore_case(s1: &str, s2: &str) -> Ordering {
    extended_compare_ignore_case(s1, s2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotnet_random_matches_framework_values() {
        // Reference values from an independent transliteration of
        // `CompatPrng` (dotnet/runtime Random.Net5CompatImpl.cs — the
        // same algorithm .NET Framework's `new Random(int)` uses).
        let cases: [(i32, Vec<usize>); 4] = [
            (42, vec![66, 14, 12, 52, 16]),
            (0, vec![72, 81, 76, 55, 20]),
            (12345, vec![6, 7, 77, 51, 79]),
            (199999, vec![26, 24, 66, 93, 90]),
        ];
        for (seed, expected) in cases {
            let mut rng = DotNetRandom::new(seed);
            let values: Vec<usize> = (0..5).map(|_| rng.next(100)).collect();
            assert_eq!(values, expected, "seed {seed}");
        }
    }

    #[test]
    fn randomize_deterministic() {
        let mut a: Vec<i32> = (0..10).collect();
        randomize(&mut a, 12345);
        let mut b: Vec<i32> = (0..10).collect();
        randomize(&mut b, 12345);
        assert_eq!(a, b);
        let mut c: Vec<i32> = (0..10).collect();
        randomize(&mut c, 99);
        assert_ne!(
            a, c,
            "different seeds should differ (overwhelmingly likely)"
        );
    }

    #[test]
    fn series_order() {
        let mut a = ComicBook::default();
        a.info.series = "The Batman".into();
        a.info.number = "10".into();
        a.enable_proposed = false;
        let mut b = ComicBook::default();
        b.info.series = "batman".into();
        b.info.number = "2".into();
        b.enable_proposed = false;
        // Articles are ignored for the series, numbers compare
        // numerically: "The Batman" #10 sorts after #2.
        assert_eq!(compare_series(&b, &a), Ordering::Less);
    }
}
