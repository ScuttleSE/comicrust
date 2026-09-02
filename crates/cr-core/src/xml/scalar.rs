//! Scalar codecs matching .NET `XmlConvert` behavior.

use chrono::{Datelike, Timelike};
use std::fmt;

/// A GUID stored as 16 bytes; formats like .NET `Guid.ToString("d")`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct CrGuid([u8; 16]);

impl CrGuid {
    pub const EMPTY: CrGuid = CrGuid([0; 16]);

    pub fn is_empty(&self) -> bool {
        self.0 == [0; 16]
    }

    pub fn from_bytes(b: [u8; 16]) -> Self {
        CrGuid(b)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Parses the common .NET Guid forms (`d`, `n`, braced). Mixed case ok.
    pub fn parse(s: &str) -> Result<Self, ScalarError> {
        let t = s.trim().trim_start_matches('{').trim_end_matches('}');
        let t = t.strip_prefix("urn:uuid:").unwrap_or(t);
        let hex: Vec<u8> = t
            .chars()
            .filter(|c| *c != '-')
            .map(|c| c.to_digit(16).map(|d| d as u8))
            .collect::<Option<_>>()
            .ok_or(ScalarError(format!("invalid guid: {s}")))?;
        if hex.len() != 32 {
            return Err(ScalarError(format!("invalid guid length: {s}")));
        }
        let mut b = [0u8; 16];
        for (i, pair) in hex.chunks(2).enumerate() {
            b[i] = pair[0] << 4 | pair[1];
        }
        Ok(CrGuid(b))
    }

    pub fn to_d_string(&self) -> String {
        let b = self.0;
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-\
             {:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0],
            b[1],
            b[2],
            b[3],
            b[4],
            b[5],
            b[6],
            b[7],
            b[8],
            b[9],
            b[10],
            b[11],
            b[12],
            b[13],
            b[14],
            b[15]
        )
    }
}

impl fmt::Display for CrGuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_d_string())
    }
}

impl fmt::Debug for CrGuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CrGuid({})", self.to_d_string())
    }
}

/// The three .NET `DateTimeKind` forms; the serialized suffix depends on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateKind {
    Unspecified,
    Utc,
    /// Local kind; carries the fixed offset present in the file.
    Offset(i32),
}

/// A DateTime with .NET kind semantics; `chrono` `NaiveDateTime` payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrDateTime {
    pub naive: chrono::NaiveDateTime,
    pub kind: DateKind,
}

impl Default for CrDateTime {
    fn default() -> Self {
        Self::min_value()
    }
}

impl CrDateTime {
    /// .NET `DateTime.MinValue` == 0001-01-01T00:00:00 (chrono's own
    /// `MIN` extends far further back, so this is built explicitly).
    pub fn min_value() -> Self {
        CrDateTime {
            naive: chrono::NaiveDate::from_ymd_opt(1, 1, 1)
                .and_then(|d| d.and_hms_nano_opt(0, 0, 0, 0))
                .expect("valid min value"),
            kind: DateKind::Unspecified,
        }
    }

    pub fn is_min_value(&self) -> bool {
        *self == Self::min_value()
    }

    /// Parses the `XmlConvert` sortable forms:
    /// `yyyy-MM-ddTHH:mm:ss[.f...]` plus optional `Z` or `±HH:mm`.
    pub fn parse(s: &str) -> Result<Self, ScalarError> {
        let t = s.trim();
        let err = || ScalarError(format!("invalid datetime: {s}"));
        let (base, suffix) = if let Some(rest) = t.strip_suffix('Z') {
            (rest, DateKind::Utc)
        } else if let Some(rest) = t.strip_suffix('z') {
            (rest, DateKind::Utc)
        } else {
            let sign_idx = t.rfind(['+', '-']).filter(|&i| i > 9 && t.len() - i == 6);
            match sign_idx {
                Some(i) => {
                    let (base, off) = t.split_at(i);
                    let mut it = off[1..].split(':');
                    let h: i32 = it.next().and_then(|v| v.parse().ok()).ok_or_else(err)?;
                    let m: i32 = it.next().and_then(|v| v.parse().ok()).ok_or_else(err)?;
                    let mins = if off.starts_with('-') {
                        -(h * 60 + m)
                    } else {
                        h * 60 + m
                    };
                    (base, DateKind::Offset(mins))
                }
                None => (t, DateKind::Unspecified),
            }
        };
        let (date_part, time_part) = base
            .split_once('T')
            .or_else(|| base.split_once('t'))
            .ok_or_else(err)?;
        let (hms, frac) = match time_part.split_once('.') {
            Some((h, f)) => (h, f),
            None => (time_part, ""),
        };
        let mut d = date_part.split('-');
        let y: i32 = d.next().and_then(|v| v.parse().ok()).ok_or_else(err)?;
        let mo: u32 = d.next().and_then(|v| v.parse().ok()).ok_or_else(err)?;
        let da: u32 = d.next().and_then(|v| v.parse().ok()).ok_or_else(err)?;
        let mut t2 = hms.split(':');
        let h: u32 = t2.next().and_then(|v| v.parse().ok()).ok_or_else(err)?;
        let mi: u32 = t2.next().and_then(|v| v.parse().ok()).ok_or_else(err)?;
        let sec: u32 = t2.next().unwrap_or("0").parse().map_err(|_| err())?;
        // fraction: up to 7 digits (ticks); pad to nanoseconds
        let frac_ns: u32 = if frac.is_empty() {
            0
        } else {
            let digits: String = frac.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.len() > 9 {
                return Err(err());
            }
            let padded = format!("{digits:0<9}");
            padded.parse().map_err(|_| err())?
        };
        let date = chrono::NaiveDate::from_ymd_opt(y, mo, da).ok_or_else(err)?;
        let time = chrono::NaiveTime::from_hms_nano_opt(h, mi, sec, frac_ns).ok_or_else(err)?;
        Ok(CrDateTime {
            naive: date.and_time(time),
            kind: suffix,
        })
    }

    /// Writes the `XmlConvert` sortable form with kind suffix.
    pub fn to_xml(&self) -> String {
        let d = self.naive.date();
        let t = self.naive.time();
        let mut out = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            d.year(),
            d.month(),
            d.day(),
            t.hour(),
            t.minute(),
            t.second()
        );
        let ns = t.nanosecond();
        if ns > 0 {
            // .NET prints up to 7 fraction digits, trailing zeros trimmed
            let mut frac = format!(".{:07}", ns / 100);
            while frac.ends_with('0') {
                frac.pop();
            }
            out.push_str(&frac);
        }
        match self.kind {
            DateKind::Unspecified => {}
            DateKind::Utc => out.push('Z'),
            DateKind::Offset(mins) => {
                let sign = if mins < 0 { '-' } else { '+' };
                let abs = mins.abs();
                out.push_str(&format!("{}{:02}:{:02}", sign, abs / 60, abs % 60));
            }
        }
        out
    }

    /// Returns a copy with the time part stripped (`.DateOnly()` in C#).
    pub fn date_only(&self) -> Self {
        CrDateTime {
            naive: self.naive.date().and_hms_opt(0, 0, 0).expect("midnight"),
            kind: self.kind,
        }
    }
}

/// Formats an f32 the way `XmlConvert.ToString(float)` mostly does:
/// shortest round-trip, scientific notation as `1E-05` / `1E+10`.
pub fn net_f32(v: f32) -> String {
    if !v.is_finite() {
        // Cannot occur in the ComicRack XML surface (values are clamped),
        // but keep the writer total.
        return if v.is_nan() {
            "NaN".to_string()
        } else if v > 0.0 {
            "INF".to_string()
        } else {
            "-INF".to_string()
        };
    }
    if v == 0.0 {
        return "0".to_string();
    }
    let s = format!("{v}");
    // .NET single formatting switches to scientific when the value needs
    // more than 7 significant digits or is very small.
    let significant: usize = s
        .chars()
        .filter(|c| c.is_ascii_digit() && *c != '0')
        .count();
    let digits: usize = s.chars().filter(|c| c.is_ascii_digit()).count();
    let use_sci = s.contains('e') || v.abs() <= 1e-5 || (digits > 7 && significant > 0);
    if !use_sci {
        return s;
    }
    // Scientific: `1e-5` → `1E-05` (.NET exponent form, min 2 digits)
    let (mantissa, exp) = match s.split_once('e') {
        Some((m, e)) => (m.to_string(), e.parse::<i32>().unwrap_or(0)),
        None => {
            // Value too small for plain form: build from {:e}
            let sci = format!("{:e}", v);
            let (m, e) = sci.split_once('e').expect("sci form");
            (m.to_string(), e.parse::<i32>().unwrap_or(0))
        }
    };
    format!(
        "{}E{}{:02}",
        mantissa,
        if exp < 0 { '-' } else { '+' },
        exp.abs()
    )
}

/// Error for scalar parse failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalarError(pub String);

impl fmt::Display for ScalarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ScalarError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guid_round_trip() {
        let g = CrGuid::parse("11111111-2222-3333-4444-555555555555").unwrap();
        assert_eq!(g.to_d_string(), "11111111-2222-3333-4444-555555555555");
        let braced = CrGuid::parse("{11111111-2222-3333-4444-555555555555}").unwrap();
        assert_eq!(g, braced);
        assert!(CrGuid::parse("nope").is_err());
        assert!(CrGuid::EMPTY.is_empty());
    }

    #[test]
    fn datetime_forms() {
        let d = CrDateTime::parse("0001-01-01T00:00:00").unwrap();
        assert!(d.is_min_value());
        assert_eq!(d.to_xml(), "0001-01-01T00:00:00");
        let d = CrDateTime::parse("2020-05-06T07:08:09Z").unwrap();
        assert_eq!(d.to_xml(), "2020-05-06T07:08:09Z");
        assert_eq!(d.kind, DateKind::Utc);
        let d = CrDateTime::parse("2010-06-19T14:04:28+02:00").unwrap();
        assert_eq!(d.to_xml(), "2010-06-19T14:04:28+02:00");
        assert_eq!(d.kind, DateKind::Offset(120));
        let d = CrDateTime::parse("2020-05-06T07:08:09.5Z").unwrap();
        assert_eq!(d.to_xml(), "2020-05-06T07:08:09.5Z");
        assert!(CrDateTime::parse("garbage").is_err());
    }

    #[test]
    fn float_format() {
        assert_eq!(net_f32(0.0), "0");
        assert_eq!(net_f32(3.5), "3.5");
        assert_eq!(net_f32(0.25), "0.25");
        assert_eq!(net_f32(0.5), "0.5");
        assert_eq!(net_f32(-1.0), "-1");
        assert_eq!(net_f32(1e-5f32), "1E-05");
        assert_eq!(net_f32(1e10f32), "1E+10");
    }
}
