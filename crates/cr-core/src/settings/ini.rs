//! The [`IniFile`] port (`cYo.Common/Runtime/IniFile.cs`).
//!
//! comicrust keeps the ini as the place for engine-level overrides
//! (`EngineConfiguration` + `ExtendedSettings`), loaded from the
//! `comicrust.ini` search chain (ADR-023). The main user settings are
//! NOT ini data — the C# persists them as `Config.xml`.

use std::collections::BTreeMap;
use std::path::Path;

/// One parsed ini file (or merge result). Keys keep their read form;
/// lookups are case-insensitive like the C# property binding
/// (`GetProperty` uses `StringComparison.OrdinalIgnoreCase`).
///
/// The C# `values` dictionary is keyed by the exact text, and later
/// `AddRange` calls overwrite same-key entries (exact key equality).
/// The insertion order is preserved for deterministic application.
#[derive(Debug, Default, Clone)]
pub struct IniValues {
    entries: Vec<(String, String)>,
    /// Case-folded index: folded key → position in `entries`.
    index: BTreeMap<String, usize>,
}

impl IniValues {
    pub fn new() -> IniValues {
        IniValues::default()
    }

    /// `dictionary[key] = value` semantics: replaces an entry whose
    /// key matches exactly, else appends. The lookup index folds the
    /// case (the C# property binding is case-insensitive).
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();
        if let Some(&pos) = self.index.get(&fold_key(&key)) {
            self.entries[pos].1 = value;
            return;
        }
        self.index.insert(fold_key(&key), self.entries.len());
        self.entries.push((key, value));
    }

    /// Case-insensitive lookup (the C# `GetValue(values, name, def)`
    /// route goes through the case-insensitive property match).
    pub fn get(&self, name: &str) -> Option<&str> {
        let folded = fold_key(name);
        self.index
            .get(&folded)
            .and_then(|&pos| self.entries.get(pos))
            .map(|(_, v)| v.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `IniFile.GetValues(TextReader, section)`: skips `;`/`#`
    /// comments, handles `[section]` headers (a `None` section reads
    /// every section; keys keep their last value), splits at the
    /// first `=`, trims the key (empty keys drop) and trims leading
    /// whitespace of the value (the C# `TrimStart`).
    pub fn read_text(text: &str, section: Option<&str>) -> IniValues {
        let mut out = IniValues::new();
        // `flag` = section filter active; `flag2` = currently inside
        // the wanted section. The C# starts flag2 true when no filter
        // is given and never turns it off in that mode.
        let filter = section.is_some();
        let mut inside = !filter;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                if !filter {
                    continue;
                }
                if inside {
                    return out;
                }
                let name = &line[1..line.len().saturating_sub(1)];
                inside = section.is_some_and(|s| name.eq_ignore_ascii_case(s));
            }
            if !inside {
                continue;
            }
            if let Some(eq) = line.find('=') {
                let key = line[..eq].trim();
                if !key.is_empty() {
                    out.set(key.to_string(), line[eq + 1..].trim_start().to_string());
                }
            }
        }
        out
    }

    /// `IniFile.ReadFile`: the `|`-separated file chain, later files
    /// override earlier ones. Missing or unreadable files contribute
    /// nothing (the C# catches and continues).
    pub fn read_files(spec: &str) -> IniValues {
        let mut out = IniValues::new();
        for file in spec.split('|') {
            if file.is_empty() {
                continue;
            }
            let values = IniValues::read_file(Path::new(file));
            for (k, v) in values.iter() {
                out.set(k.to_string(), v.to_string());
            }
        }
        out
    }

    /// One file; any error yields an empty result (C# parity).
    pub fn read_file(path: &Path) -> IniValues {
        match std::fs::read_to_string(path) {
            Ok(text) => IniValues::read_text(&text, None),
            Err(_) => IniValues::new(),
        }
    }

    /// The C# `IniFile.ReadCommandLine` regex:
    /// `[/-](?<switch>[a-z]+)[:=](?<value>.+)`, case-insensitive. A
    /// `-switch=value` or `/switch=value` argument lands as the
    /// switch name → value. Note that the C# matches a leading single
    /// `-` or `/`; `-rf=1` becomes key `rf`.
    pub fn read_command_line(args: &[String]) -> IniValues {
        let mut out = IniValues::new();
        for arg in args {
            if let Some((key, value)) = parse_command_pair(arg) {
                out.set(key, value);
            }
        }
        out
    }

    /// `IniFile.UpdateProperties` over a registry: every ini key that
    /// matches a registered field name (case-insensitive) applies its
    /// parsed value; unknown keys and parse failures keep the current
    /// value (the C# swallows the errors).
    pub fn apply_to<T>(&self, target: &mut T, fields: &[super::registry::FieldDesc<T>]) {
        for (key, text) in self.iter() {
            if let Some(field) = fields
                .iter()
                .find(|f| f.name.eq_ignore_ascii_case(key) && f.ini_enabled)
            {
                if let Some(value) = super::registry::parse_value(field.kind, text) {
                    (field.set)(target, value);
                }
            }
        }
    }
}

/// `IniFile.rxCommand` without the regex:
/// `[/-](?<switch>[a-z]+)[:=](?<value>.+)`, case-insensitive and
/// UNANCHORED — the C# `Match` scans for the first position where a
/// `-`/`/` is followed by letters and then `=`/`:` (so `--long=x`
/// matches at the second dash, switch `long`).
fn parse_command_pair(arg: &str) -> Option<(String, String)> {
    let bytes = arg.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'-' && b != b'/' {
            continue;
        }
        let rest = &arg[i + 1..];
        let letters = rest.len()
            - rest
                .trim_start_matches(|c: char| c.is_ascii_alphabetic())
                .len();
        if letters == 0 {
            continue;
        }
        let after = &rest[letters..];
        let (value, sep) = match after.chars().next()? {
            '=' | ':' => (&after[1..], true),
            _ => continue,
        };
        if value.is_empty() {
            return None;
        }
        let _ = sep;
        return Some((rest[..letters].to_ascii_lowercase(), value.to_string()));
    }
    None
}

fn fold_key(key: &str) -> String {
    key.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_keys_values_and_comments() {
        let text = "; comment\n# comment\n\nA = 1\nB=\tc  ; trailing kept\n[S]\nC=x\n";
        let v = IniValues::read_text(text, None);
        assert_eq!(v.get("a"), Some("1"));
        assert_eq!(v.get("A"), Some("1"));
        // TrimStart only: trailing whitespace of the value stays.
        assert_eq!(v.get("b"), Some("c  ; trailing kept"));
        // No section filter: section keys are read too.
        assert_eq!(v.get("c"), Some("x"));
    }

    #[test]
    fn section_filter_stops_at_the_next_header() {
        let text = "[S]\nA=1\n[T]\nB=2\n[S]\nC=3\n";
        let v = IniValues::read_text(text, Some("s"));
        assert_eq!(v.get("A"), Some("1"));
        assert_eq!(v.get("B"), None);
        // The C# returns at the second `[S]`? No: flag2 is already
        // true, so it keeps reading — the early `return out` fires.
        assert_eq!(v.get("C"), None);
    }

    #[test]
    fn section_filter_case_insensitive_and_read_continues() {
        // In the C#, once inside the section, hitting the SAME
        // section header again does not end the read (flag2 was
        // already true → the branch returns early). Hitting a
        // DIFFERENT header only turns the flag off.
        let text = "[S]\nA=1\n[T]\nB=2\n";
        let v = IniValues::read_text(text, Some("S"));
        assert_eq!(v.get("a"), Some("1"));
        assert_eq!(v.get("b"), None);
    }

    #[test]
    fn later_files_override() {
        let dir = std::env::temp_dir().join(format!(
            "comicrust-ini-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.ini");
        let b = dir.join("b.ini");
        std::fs::write(&a, "X=1\nY=2\n").unwrap();
        std::fs::write(&b, "X=9\n").unwrap();
        let spec = format!("{}|{}", a.display(), b.display());
        let v = IniValues::read_files(&spec);
        assert_eq!(v.get("x"), Some("9"));
        assert_eq!(v.get("y"), Some("2"));
    }

    #[test]
    fn command_line_pairs() {
        let args: Vec<String> = ["-db=path two", "/RF=1", "-plain", "--long=x"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let v = IniValues::read_command_line(&args);
        assert_eq!(v.get("db"), Some("path two"));
        assert_eq!(v.get("rf"), Some("1"));
        assert_eq!(v.get("plain"), None);
        // `--long=x` is unanchored: the C# regex matches at the
        // second dash (switch `long`).
        assert_eq!(v.get("long"), Some("x"));
    }
}
