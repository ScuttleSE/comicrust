//! The MCL interchange format (ADR-038).
//!
//! An `.mcl` file is a full snapshot of the Comic Vine volume-to-issue
//! map. Importing one seeds the cache skeleton layer with no API
//! request. The format comes from the `Update Missing` add-on
//! (`update_missing.py`, FrederikBaerentsen/ComicRack_Scripts).
//!
//! ```text
//! Missing;2026-08-26
//! 771;92469,92470,165276;1,2,3,
//! ```
//!
//! Line 1 is the header. Every other line is one volume: the volume
//! id, the issue ids, and the issue numbers. The two lists align by
//! position, and the issues are in issue-id order.
//!
//! The reader accepts what the source writer really produces, which is
//! not what that writer's own docstring promises.
//!
//! * The number list ends with a trailing comma. Only the id list is
//!   trimmed.
//! * An issue number carries the escapes `.&@1` for a comma and
//!   `.&@2` for a semicolon. The source writer never reverses them, so
//!   this reader does.
//! * The docstring promises double quotes around a number list that
//!   holds a space. That writer never emits them. This reader accepts
//!   them, and this writer does not produce them.
//! * In a quoted or unescaped list, a comma that a space follows is
//!   not a separator. Issue numbers such as `v. 1, no. 01` occur.
//!
//! This writer always escapes, so it never needs a quote.

use std::io::{BufRead, Write};

/// The comma escape in an issue number.
const ESC_COMMA: &str = ".&@1";
/// The semicolon escape in an issue number.
const ESC_SEMI: &str = ".&@2";

/// One issue of one volume.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MclIssue {
    pub issue_id: i64,
    /// The issue number, unescaped. It is text, not a number.
    pub issue_number: String,
}

/// One volume and its issues, in issue-id order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MclVolume {
    pub volume_id: i64,
    pub issues: Vec<MclIssue>,
}

/// What a read found. A bad line never stops the read, because one bad
/// line in a file of 150000 must not lose the other 149999.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MclReport {
    /// The date on the header line, or an empty string.
    pub date: String,
    pub volumes: usize,
    pub issues: usize,
    /// The number of lines the reader skipped.
    pub skipped: usize,
    /// The first few skipped lines, with their 1-based line numbers.
    pub errors: Vec<(usize, String)>,
}

/// The reader keeps at most this many error samples.
const MAX_ERRORS: usize = 20;

/// Reads an MCL file and calls `on_volume` for every volume line that
/// parses. The reader streams, so a full snapshot never has to fit in
/// memory.
pub fn read<R: BufRead>(
    reader: R,
    mut on_volume: impl FnMut(MclVolume),
) -> std::io::Result<MclReport> {
    let mut report = MclReport::default();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            continue;
        }
        if index == 0 {
            if let Some(date) = parse_header(line) {
                report.date = date.to_string();
                continue;
            }
            // No header. Read line 1 as a volume line, because a hand
            // cut file can start at the data.
        }
        match parse_volume_line(line) {
            Ok(volume) => {
                report.volumes += 1;
                report.issues += volume.issues.len();
                on_volume(volume);
            }
            Err(reason) => {
                report.skipped += 1;
                if report.errors.len() < MAX_ERRORS {
                    report.errors.push((index + 1, reason));
                }
            }
        }
    }
    Ok(report)
}

/// Returns the date of a `Missing;<date>` header line.
pub fn parse_header(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("Missing")?;
    let rest = rest.strip_prefix(';').unwrap_or(rest);
    Some(rest.trim())
}

/// Parses one volume line. The error text names the defect.
pub fn parse_volume_line(line: &str) -> Result<MclVolume, String> {
    let mut fields = line.trim().splitn(3, ';');
    let id_field = fields.next().unwrap_or("").trim();
    let issues_field = fields.next().ok_or("no issue-id field")?;
    let numbers_field = fields.next().ok_or("no issue-number field")?;

    let volume_id: i64 = id_field
        .parse()
        .map_err(|_| format!("volume id {id_field:?} is not a number"))?;

    let ids: Vec<i64> = issues_field
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<i64>()
                .map_err(|_| format!("issue id {s:?} is not a number"))
        })
        .collect::<Result<_, _>>()?;

    let numbers = split_numbers(numbers_field);

    let issues = ids
        .into_iter()
        .enumerate()
        .map(|(i, issue_id)| MclIssue {
            issue_id,
            issue_number: numbers.get(i).cloned().unwrap_or_default(),
        })
        .collect();
    Ok(MclVolume { volume_id, issues })
}

/// Splits and unescapes the issue-number list.
fn split_numbers(field: &str) -> Vec<String> {
    let field = field.trim_end_matches(['\r', '\n']);
    // The source writer leaves one trailing comma. Remove it before
    // anything else, so the quoted form below still matches.
    let field = field.strip_suffix(',').unwrap_or(field);
    // The quoted form that the source docstring promises.
    let field = match field.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Some(inner) => inner,
        None => field,
    };
    if field.is_empty() {
        return Vec::new();
    }

    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let bytes = field.as_bytes();
    for (i, ch) in field.char_indices() {
        // A comma that a space follows is part of the number, not a
        // separator (`v. 1, no. 01`).
        if ch == ',' && bytes.get(i + 1) != Some(&b' ') {
            out.push(unescape(&current));
            current.clear();
        } else {
            current.push(ch);
        }
    }
    out.push(unescape(&current));
    out
}

fn unescape(value: &str) -> String {
    value.replace(ESC_COMMA, ",").replace(ESC_SEMI, ";")
}

fn escape(value: &str) -> String {
    value.replace(',', ESC_COMMA).replace(';', ESC_SEMI)
}

/// Writes an MCL file in the form the source writer produces, so the
/// `Update Missing` add-on can read it back. The caller gives the
/// volumes in volume-id order; the writer sorts the issues by issue
/// id.
pub fn write<W: Write>(
    mut out: W,
    date: &str,
    volumes: impl IntoIterator<Item = MclVolume>,
) -> std::io::Result<usize> {
    writeln!(out, "Missing;{date}")?;
    let mut count = 0;
    for mut volume in volumes {
        volume.issues.sort_by_key(|i| i.issue_id);
        let ids = volume
            .issues
            .iter()
            .map(|i| i.issue_id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let mut numbers = String::new();
        for issue in &volume.issues {
            numbers.push_str(&escape(&issue.issue_number));
            // The trailing comma is part of the format.
            numbers.push(',');
        }
        writeln!(out, "{};{ids};{numbers}", volume.volume_id)?;
        count += 1;
    }
    Ok(count)
}

/// Seeds the cache skeleton layer from an MCL file. The import makes
/// no API request. It writes in batches, so a full snapshot never has
/// to fit in memory.
///
/// The import uses the cache merge rule: it adds volumes and issues,
/// and it erases no field that an earlier API query filled.
pub fn import<R: BufRead>(cache: &dyn super::CvCache, reader: R) -> std::io::Result<MclReport> {
    import_reporting(cache, reader, |_, _| {})
}

/// The same import, with progress. `on_progress` receives the running
/// volume and issue counts after every batch. It runs on the calling
/// thread, so it must not block.
pub fn import_reporting<R: BufRead>(
    cache: &dyn super::CvCache,
    reader: R,
    mut on_progress: impl FnMut(usize, usize),
) -> std::io::Result<MclReport> {
    /// The number of volumes the import holds before it writes.
    const BATCH: usize = 500;

    let mut volumes: Vec<super::VolumeRow> = Vec::with_capacity(BATCH);
    let mut issues: Vec<super::IssueSkeleton> = Vec::new();
    let mut failed: Option<super::CacheError> = None;

    let flush = |volumes: &mut Vec<super::VolumeRow>,
                 issues: &mut Vec<super::IssueSkeleton>,
                 failed: &mut Option<super::CacheError>| {
        if failed.is_some() {
            volumes.clear();
            issues.clear();
            return;
        }
        if let Err(e) = cache.put_volumes(volumes) {
            *failed = Some(e);
        } else if let Err(e) = cache.put_issues(issues) {
            *failed = Some(e);
        }
        volumes.clear();
        issues.clear();
    };

    let seen_volumes = std::cell::Cell::new(0usize);
    let seen_issues = std::cell::Cell::new(0usize);
    let mut report = read(reader, |volume| {
        seen_volumes.set(seen_volumes.get() + 1);
        seen_issues.set(seen_issues.get() + volume.issues.len());
        volumes.push(super::VolumeRow {
            volume_id: volume.volume_id,
            ..Default::default()
        });
        issues.extend(volume.issues.into_iter().map(|i| super::IssueSkeleton {
            issue_id: i.issue_id,
            volume_id: volume.volume_id,
            issue_number: i.issue_number,
            ..Default::default()
        }));
        if volumes.len() >= BATCH {
            flush(&mut volumes, &mut issues, &mut failed);
            on_progress(seen_volumes.get(), seen_issues.get());
        }
    })?;
    flush(&mut volumes, &mut issues, &mut failed);

    if let Some(e) = failed {
        report.errors.push((0, e.to_string()));
        return Err(std::io::Error::other(e.to_string()));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> MclVolume {
        parse_volume_line(line).expect("line parses")
    }

    #[test]
    fn the_header_gives_the_date() {
        assert_eq!(parse_header("Missing;2026-08-26"), Some("2026-08-26"));
        assert_eq!(parse_header("Missing;"), Some(""));
        assert_eq!(parse_header("771;1;1,"), None);
    }

    #[test]
    fn a_plain_line_parses() {
        let v = parse("771;92469,92470,165276;1,2,3,");
        assert_eq!(v.volume_id, 771);
        assert_eq!(
            v.issues,
            vec![
                MclIssue {
                    issue_id: 92_469,
                    issue_number: "1".into()
                },
                MclIssue {
                    issue_id: 92_470,
                    issue_number: "2".into()
                },
                MclIssue {
                    issue_id: 165_276,
                    issue_number: "3".into()
                },
            ]
        );
    }

    #[test]
    fn the_trailing_comma_adds_no_issue() {
        // The source writer trims only the id list.
        assert_eq!(parse("1;10,11;1,2,").issues.len(), 2);
        assert_eq!(parse("1;10,11;1,2").issues.len(), 2);
    }

    #[test]
    fn the_escapes_reverse_on_read() {
        // Volume 77901 holds the issue number `1,5` (ADR-038).
        let v = parse("77901;5;1.&@15,");
        assert_eq!(v.issues[0].issue_number, "1,5");
        let v = parse("1;5;a.&@2b,");
        assert_eq!(v.issues[0].issue_number, "a;b");
    }

    #[test]
    fn a_quoted_list_reads() {
        // The source docstring promises this form. That writer never
        // emits it, so only the reader supports it.
        let v = parse(r#"1;10,11;"v. 1, no. 01,v. 1, no. 02","#);
        assert_eq!(v.issues[0].issue_number, "v. 1, no. 01");
        assert_eq!(v.issues[1].issue_number, "v. 1, no. 02");
    }

    #[test]
    fn a_comma_that_a_space_follows_is_not_a_separator() {
        let v = parse("1;10;v. 1, no. 01,");
        assert_eq!(v.issues.len(), 1);
        assert_eq!(v.issues[0].issue_number, "v. 1, no. 01");
    }

    #[test]
    fn a_short_number_list_leaves_the_rest_empty() {
        let v = parse("1;10,11,12;1,");
        assert_eq!(v.issues.len(), 3);
        assert_eq!(v.issues[0].issue_number, "1");
        assert_eq!(v.issues[1].issue_number, "");
        assert_eq!(v.issues[2].issue_number, "");
    }

    #[test]
    fn a_volume_with_no_issues_parses() {
        let v = parse("1;;");
        assert_eq!(v.volume_id, 1);
        assert!(v.issues.is_empty());
    }

    #[test]
    fn a_bad_line_names_its_defect() {
        assert!(parse_volume_line("x;1;1,").is_err());
        assert!(parse_volume_line("1;a;1,").is_err());
        assert!(parse_volume_line("1;1").is_err());
        assert!(parse_volume_line("1").is_err());
    }

    #[test]
    fn a_read_skips_a_bad_line_and_keeps_the_rest() {
        let text = "Missing;2026-08-26\n1;10;1,\nbroken line\n2;20,21;1,2,\n";
        let mut got = Vec::new();
        let report = read(text.as_bytes(), |v| got.push(v)).expect("read");
        assert_eq!(report.date, "2026-08-26");
        assert_eq!(report.volumes, 2);
        assert_eq!(report.issues, 3);
        assert_eq!(report.skipped, 1);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].0, 3, "the 1-based line number");
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn a_file_with_no_header_still_reads() {
        let mut got = Vec::new();
        let report = read("1;10;1,\n".as_bytes(), |v| got.push(v)).expect("read");
        assert_eq!(report.date, "");
        assert_eq!(report.volumes, 1);
    }

    #[test]
    fn the_writer_produces_the_source_form() {
        let mut out = Vec::new();
        let volumes = vec![MclVolume {
            volume_id: 771,
            issues: vec![
                MclIssue {
                    issue_id: 165_276,
                    issue_number: "3".into(),
                },
                MclIssue {
                    issue_id: 92_469,
                    issue_number: "1".into(),
                },
            ],
        }];
        write(&mut out, "2026-09-12", volumes).expect("write");
        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            // Sorted by issue id, and the number list keeps its
            // trailing comma.
            "Missing;2026-09-12\n771;92469,165276;1,3,\n"
        );
    }

    #[test]
    fn the_writer_escapes_so_the_source_reader_can_split() {
        let mut out = Vec::new();
        write(
            &mut out,
            "2026-09-12",
            vec![MclVolume {
                volume_id: 77_901,
                issues: vec![MclIssue {
                    issue_id: 5,
                    issue_number: "1,5".into(),
                }],
            }],
        )
        .expect("write");
        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            "Missing;2026-09-12\n77901;5;1.&@15,\n"
        );
    }

    #[test]
    fn a_write_then_a_read_agrees_on_the_data() {
        let volumes = vec![
            MclVolume {
                volume_id: 1,
                issues: vec![
                    MclIssue {
                        issue_id: 10,
                        issue_number: "1,5".into(),
                    },
                    MclIssue {
                        issue_id: 11,
                        issue_number: "v. 1, no. 01".into(),
                    },
                ],
            },
            MclVolume {
                volume_id: 2,
                issues: vec![MclIssue {
                    issue_id: 20,
                    issue_number: "a;b".into(),
                }],
            },
        ];
        let mut out = Vec::new();
        write(&mut out, "2026-09-12", volumes.clone()).expect("write");
        let mut got = Vec::new();
        let report = read(out.as_slice(), |v| got.push(v)).expect("read");
        assert_eq!(report.skipped, 0);
        assert_eq!(got, volumes);
    }
}
