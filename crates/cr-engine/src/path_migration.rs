//! The Windows-path migration (Phase 8 T11): a ComicRack CE database
//! migrated from Windows carries `C:\…` / `\\server\…` paths in the
//! book file paths, the watch folders, and the blacklist. This module
//! detects those paths, collapses them into the smallest set of
//! common-prefix roots, and rewrites them under user-chosen Linux
//! targets (scanner-parity: a found file re-homes, a missing file
//! makes the book fileless with its metadata kept).
//!
//! No C# counterpart — this is the port-side migration helper (the
//! user request, scope + decisions recorded in
//! `docs/phase-8-kickoff.md` T11). Only string values change, so the
//! ComicDb.xml schema and byte-stability are untouched.

use std::path::Path;

use cr_core::database::comic_database::ComicDatabase;
use cr_core::database::list_items::WatchFolder;
use cr_core::model::comic_book::ComicBook;

/// One collapsed Windows root with the per-family counts
/// (the dialog row).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathRoot {
    /// The Windows root as first seen, e.g. `C:\Comics`
    /// (components below the drive/share root that every path under
    /// this entry shares; the bare drive appears when files sit
    /// directly on it).
    pub windows_root: String,
    pub books: usize,
    pub watch_folders: usize,
    pub black_list: usize,
}

/// One user-decided mapping: the Windows root → the Linux target
/// folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub windows_root: String,
    pub linux_target: String,
}

/// What one apply pass changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ApplyReport {
    pub books_mapped: usize,
    /// Books whose target file was not found — now fileless
    /// (empty file path, metadata kept).
    pub books_fileless: usize,
    pub watch_folders_mapped: usize,
    /// Watch folders whose target directory does not exist — left
    /// unchanged (removable in Preferences).
    pub watch_folders_left: usize,
    pub black_list_mapped: usize,
    pub black_list_left: usize,
}

impl ApplyReport {
    pub fn changed_anything(&self) -> bool {
        self.books_mapped + self.books_fileless + self.watch_folders_mapped + self.black_list_mapped
            > 0
    }
}

/// A Windows-style path: an ASCII drive-letter root (`C:`, `C:\x`,
/// `C:/x`) or a UNC path (`\\server\share\…`). Absolute Linux paths
/// start with `/`, so anything with the drive prefix is Windows-styled.
pub fn is_windows_path(p: &str) -> bool {
    let b = p.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return true;
    }
    p.starts_with("\\\\")
}

/// One parsed Windows path split at its root.
#[derive(Clone, Debug, PartialEq, Eq)]
struct WinPath {
    /// Group key (lowercased): `c:` or `\\server\share`.
    root_key: String,
    /// The root as first seen (`C:`, `\\server\share`).
    root_display: String,
    /// Directory components below the root, in order (original
    /// casing). For a file path the file name is the last component.
    comps: Vec<String>,
}

/// Splits a Windows path at its root. Returns None for non-Windows
/// paths. `has_file` drops the last component as the file name
/// (books/blacklist name files; watch folders name directories).
fn parse_win_path(p: &str, has_file: bool) -> Option<WinPath> {
    if !is_windows_path(p) {
        return None;
    }
    if p.starts_with("\\\\") {
        // `\\server\share\rest…` — the root is the first two comps.
        let rest = p.trim_start_matches('\\');
        let comps: Vec<&str> = rest.split(['\\', '/']).filter(|s| !s.is_empty()).collect();
        if comps.len() < 2 {
            return None;
        }
        let root_display = format!("\\\\{}\\{}", comps[0], comps[1]);
        let mut dirs: Vec<String> = comps[2..].iter().map(|s| s.to_string()).collect();
        if has_file {
            dirs.pop();
        }
        return Some(WinPath {
            root_key: root_display.to_ascii_lowercase(),
            root_display,
            comps: dirs,
        });
    }
    // Drive path `C:\rest…`.
    let drive = p[..1].to_ascii_uppercase();
    let rest = &p[2..];
    let mut comps: Vec<String> = rest
        .split(['\\', '/'])
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    if has_file {
        comps.pop();
    }
    Some(WinPath {
        root_key: format!("{}:", drive.to_ascii_lowercase()),
        root_display: format!("{}:", drive),
        comps,
    })
}

/// Joins the root + components back into a Windows path.
fn join_win(root_display: &str, comps: &[String]) -> String {
    let mut s = String::from(root_display);
    if s.ends_with(':') && comps.is_empty() {
        // A bare drive root displays as `C:\`.
        s.push('\\');
    }
    for c in comps {
        if !s.ends_with('\\') {
            s.push('\\');
        }
        s.push_str(c);
    }
    s
}

/// Collects the collapsed roots over the three path families. The
/// collapse rule: components merge below the drive/share root while
/// every path in the group shares them (`C:\Comics\Batman` +
/// `C:\Comics\Daredevil` → `C:\Comics`); the bare drive never
/// becomes a root unless files sit directly on it (`C:\a` +
/// `C:\b` → two roots, not `C:\`).
pub fn collect_roots(
    books: &[ComicBook],
    watch_folders: &[WatchFolder],
    black_list: &[String],
) -> Vec<PathRoot> {
    // (root_key, root_display, comps, kind) — kind 0 book, 1 watch, 2 blacklist.
    let mut entries: Vec<(String, String, Vec<String>, u8)> = Vec::new();
    let push = |p: &str, has_file: bool, kind: u8, entries: &mut Vec<_>| {
        if let Some(w) = parse_win_path(p, has_file) {
            entries.push((w.root_key, w.root_display, w.comps, kind));
        }
    };
    for b in books {
        push(&b.file_path, true, 0, &mut entries);
    }
    for w in watch_folders {
        push(&w.folder, false, 1, &mut entries);
    }
    for f in black_list {
        push(f, true, 2, &mut entries);
    }

    // Group by root key.
    let mut order: Vec<String> = Vec::new();
    let mut groups: Vec<Vec<(String, Vec<String>, u8)>> = Vec::new();
    for (key, display, comps, kind) in entries {
        match order.iter().position(|k| k == &key) {
            Some(i) => groups[i].push((display, comps, kind)),
            None => {
                order.push(key);
                groups.push(vec![(display, comps, kind)]);
            }
        }
    }

    let mut roots: Vec<PathRoot> = Vec::new();

    for group in groups.iter() {
        // The longest common component prefix (original casing from
        // the first entry).
        let mut common: Vec<String> = group[0].1.clone();
        for (_, comps, _) in group.iter().skip(1) {
            let n = common
                .iter()
                .zip(comps.iter())
                .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                .count();
            common.truncate(n);
        }
        if !common.is_empty() {
            push_root(&mut roots, group, &common);
            continue;
        }
        // Nothing common below the root: one row per first component,
        // plus the bare root for the entries sitting directly on it.
        let mut firsts: Vec<String> = Vec::new();
        for (_, comps, _) in group.iter() {
            let Some(first) = comps.first() else {
                continue;
            };
            if !firsts
                .iter()
                .any(|f: &String| f.eq_ignore_ascii_case(first))
            {
                firsts.push(first.clone());
            }
        }
        for first in &firsts {
            push_root(&mut roots, group, std::slice::from_ref(first));
        }
        if group.iter().any(|(_, comps, _)| comps.is_empty()) {
            push_root(&mut roots, group, &[]);
        }
    }
    roots.sort_by(|a, b| {
        a.windows_root
            .to_ascii_lowercase()
            .cmp(&b.windows_root.to_ascii_lowercase())
    });
    roots
}

/// Counts the group entries under `comps` and pushes the root row.
fn push_root(roots: &mut Vec<PathRoot>, group: &[(String, Vec<String>, u8)], comps: &[String]) {
    let mut books = 0;
    let mut watches = 0;
    let mut black = 0;
    for (_, gcomps, kind) in group {
        let lower: Vec<String> = gcomps.iter().map(|c| c.to_ascii_lowercase()).collect();
        let prefix: Vec<String> = comps.iter().map(|c| c.to_ascii_lowercase()).collect();
        // The bare root counts only the entries sitting directly on
        // it; a component root counts everything below it.
        let counted = if prefix.is_empty() {
            lower.is_empty()
        } else {
            lower.len() >= prefix.len() && lower.starts_with(&prefix)
        };
        if !counted {
            continue;
        }
        match kind {
            0 => books += 1,
            1 => watches += 1,
            _ => black += 1,
        }
    }
    roots.push(PathRoot {
        windows_root: join_win(&group[0].0.clone(), comps),
        books,
        watch_folders: watches,
        black_list: black,
    });
}

/// Any Windows-style path in the three families?
pub fn has_windows_paths(db: &ComicDatabase) -> bool {
    db.books.iter().any(|b| is_windows_path(&b.file_path))
        || db.watch_folders.iter().any(|w| is_windows_path(&w.folder))
        || db.black_list.iter().any(|f| is_windows_path(f))
}

/// Rewrites `path` under `target` when it lives under `root`
/// (case-insensitive, `\`/`/` both accepted). Returns the new
/// absolute path or None.
pub fn map_relative(path: &str, root: &str, target: &str) -> Option<String> {
    let w = parse_win_path(path, false)?;
    let r = parse_win_path(root, false)?;
    if w.root_key != r.root_key || w.comps.len() < r.comps.len() {
        return None;
    }
    for (a, b) in w.comps.iter().zip(r.comps.iter()) {
        if !a.eq_ignore_ascii_case(b) {
            return None;
        }
    }
    let rest = &w.comps[r.comps.len()..];
    let mut new = target.trim_end_matches('/').to_string();
    for c in rest {
        new.push('/');
        new.push_str(c);
    }
    Some(new)
}

/// The live per-root preview (the dialog row): the books under `root`
/// split into found / not-found under `target`.
pub fn preview_books(books: &[ComicBook], root: &str, target: &str) -> (usize, usize) {
    let mut found = 0;
    let mut missing = 0;
    for b in books {
        if !is_windows_path(&b.file_path) {
            continue;
        }
        match map_relative(&b.file_path, root, target) {
            Some(p) if Path::new(&p).exists() => found += 1,
            Some(_) => missing += 1,
            None => {}
        }
    }
    (found, missing)
}

/// Applies the mappings to the database (books, watch folders,
/// blacklist). Found files re-home with a file-info refresh (the
/// scanner move-recovery shape); missing files clear the file path —
/// the book becomes fileless with its metadata kept. Watch folders
/// and blacklist entries rewrite only when the target exists.
/// Direct mutation: no ComicInfo write-back (a path fix must never
/// write the files).
pub fn apply(db: &mut ComicDatabase, mappings: &[Mapping]) -> ApplyReport {
    let mut report = ApplyReport::default();
    for book in db.books.iter_mut() {
        if !is_windows_path(&book.file_path) {
            continue;
        }
        let mut mapped = None;
        for m in mappings {
            if let Some(p) = map_relative(&book.file_path, &m.windows_root, &m.linux_target) {
                mapped = Some(p);
                break;
            }
        }
        let Some(new) = mapped else {
            continue;
        };
        let p = Path::new(&new);
        let exists = p.exists();
        let is_file = p.is_file();
        if exists {
            book.file_path = new;
            book.file_is_missing = false;
            if is_file {
                // The LIGHT refresh (size/times/missing only): the
                // file content is the one the DB describes — only the
                // path changed — and the full refresh's per-book
                // page-count open on the UI thread froze the apply
                // (the user freeze report; the scanner pays the same
                // cost on its worker thread instead).
                crate::scanner::refresh_file_info_basic(book);
            }
            report.books_mapped += 1;
        } else {
            book.file_path.clear();
            book.file_is_missing = false;
            report.books_fileless += 1;
        }
    }
    for w in db.watch_folders.iter_mut() {
        if !is_windows_path(&w.folder) {
            continue;
        }
        let mut mapped = None;
        for m in mappings {
            if let Some(p) = map_relative(&w.folder, &m.windows_root, &m.linux_target) {
                mapped = Some(p);
                break;
            }
        }
        match mapped {
            Some(new) if Path::new(&new).is_dir() => {
                w.folder = new;
                report.watch_folders_mapped += 1;
            }
            _ => report.watch_folders_left += 1,
        }
    }
    for f in db.black_list.iter_mut() {
        if !is_windows_path(f) {
            continue;
        }
        let mut mapped = None;
        for m in mappings {
            if let Some(p) = map_relative(f, &m.windows_root, &m.linux_target) {
                mapped = Some(p);
                break;
            }
        }
        match mapped {
            Some(new) if Path::new(&new).exists() => {
                *f = new;
                report.black_list_mapped += 1;
            }
            _ => report.black_list_left += 1,
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(p: &str) -> ComicBook {
        ComicBook {
            file_path: p.into(),
            ..Default::default()
        }
    }

    #[test]
    fn windows_detection() {
        assert!(is_windows_path("C:\\Comics\\Batman.cbz"));
        assert!(is_windows_path("c:/comics/batman.cbz"));
        assert!(is_windows_path("D:"));
        assert!(is_windows_path("\\\\server\\share\\x.cbz"));
        assert!(!is_windows_path("/home/user/comics/x.cbz"));
        assert!(!is_windows_path("relative/path.cbz"));
        assert!(!is_windows_path(""));
    }

    #[test]
    fn roots_collapse_shared_prefix() {
        let books = vec![
            book("C:\\Comics\\Batman 001.cbz"),
            book("C:\\Comics\\Batman 002.cbz"),
            book("C:\\Other\\a.cbz"),
        ];
        let watches = vec![WatchFolder {
            folder: "C:\\Comics".into(),
            watch: false,
        }];
        let roots = collect_roots(&books, &watches, &[]);
        assert_eq!(
            roots,
            vec![
                PathRoot {
                    windows_root: "C:\\Comics".into(),
                    books: 2,
                    watch_folders: 1,
                    black_list: 0
                },
                PathRoot {
                    windows_root: "C:\\Other".into(),
                    books: 1,
                    watch_folders: 0,
                    black_list: 0
                },
            ]
        );
    }

    #[test]
    fn roots_never_collapse_to_bare_drive() {
        let books = vec![book("C:\\a\\x.cbz"), book("C:\\b\\y.cbz")];
        let roots = collect_roots(&books, &[], &[]);
        let names: Vec<&str> = roots.iter().map(|r| r.windows_root.as_str()).collect();
        assert_eq!(names, vec!["C:\\a", "C:\\b"]);
    }

    #[test]
    fn roots_drive_root_files_get_the_bare_drive() {
        let books = vec![book("C:\\Comics\\x.cbz"), book("C:\\z.cbz")];
        let roots = collect_roots(&books, &[], &[]);
        let names: Vec<&str> = roots.iter().map(|r| r.windows_root.as_str()).collect();
        assert_eq!(names, vec!["C:\\", "C:\\Comics"]);
        assert_eq!(roots[0].books, 1);
    }

    #[test]
    fn roots_collapse_case_insensitive() {
        let books = vec![
            book("C:\\comics\\A.cbz"),
            book("C:\\Comics\\B.cbz"),
            book("c:\\COMICS\\deep\\C.cbz"),
        ];
        let roots = collect_roots(&books, &[], &[]);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].windows_root, "C:\\comics");
        assert_eq!(roots[0].books, 3);
    }

    #[test]
    fn roots_unc_share() {
        let books = vec![
            book("\\\\srv\\share\\Comics\\x.cbz"),
            book("\\\\srv\\share\\Comics\\y.cbz"),
        ];
        let roots = collect_roots(&books, &[], &[]);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].windows_root, "\\\\srv\\share\\Comics");
        assert_eq!(roots[0].books, 2);
    }

    #[test]
    fn map_relative_basic_and_boundary() {
        assert_eq!(
            map_relative("C:\\Comics\\Sub\\Book.cbz", "C:\\Comics", "/t"),
            Some("/t/Sub/Book.cbz".into())
        );
        // The boundary: `C:\Comics2` does not live under `C:\Comics`.
        assert_eq!(map_relative("C:\\Comics2\\B.cbz", "C:\\Comics", "/t"), None);
        assert_eq!(
            map_relative("c:\\comics\\b.cbz", "C:\\Comics", "/t/"),
            Some("/t/b.cbz".into())
        );
        assert_eq!(map_relative("/home/x.cbz", "C:\\Comics", "/t"), None);
    }

    #[test]
    fn map_relative_unc_and_drive_root() {
        assert_eq!(
            map_relative(
                "\\\\srv\\share\\Comics\\b.cbz",
                "\\\\srv\\share\\Comics",
                "/t"
            ),
            Some("/t/b.cbz".into())
        );
        assert_eq!(
            map_relative("C:\\z.cbz", "C:", "/t"),
            Some("/t/z.cbz".into())
        );
    }

    #[test]
    fn preview_splits_found_and_missing() {
        let dir = std::env::temp_dir().join(format!("crpathmig-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("here.cbz"), b"x").unwrap();
        let books = vec![
            book("C:\\Comics\\here.cbz"),
            book("C:\\Comics\\gone.cbz"),
            book("/linux/keep.cbz"),
        ];
        let (found, missing) = preview_books(&books, "C:\\Comics", dir.to_str().unwrap());
        assert_eq!((found, missing), (1, 1));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn apply_maps_missing_and_rewrites_families() {
        let dir = std::env::temp_dir().join(format!("crpathmig-apply-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("here.cbz"), b"x").unwrap();
        let target = dir.to_str().unwrap().to_string();
        let mut db = ComicDatabase {
            books: vec![book("C:\\Comics\\here.cbz"), book("C:\\Comics\\gone.cbz")],
            ..Default::default()
        };
        db.books[0].file_is_missing = true;
        db.watch_folders = vec![
            WatchFolder {
                folder: "C:\\Comics".into(),
                watch: true,
            },
            WatchFolder {
                folder: "C:\\Elsewhere".into(),
                watch: true,
            },
        ];
        db.black_list = vec!["C:\\Comics\\here.cbz".into(), "C:\\nope\\gone.cbz".into()];
        let report = apply(
            &mut db,
            &[Mapping {
                windows_root: "C:\\Comics".into(),
                linux_target: target.clone(),
            }],
        );
        assert_eq!(report.books_mapped, 1);
        assert_eq!(report.books_fileless, 1);
        assert_eq!(report.watch_folders_mapped, 1);
        assert_eq!(report.watch_folders_left, 1);
        assert_eq!(report.black_list_mapped, 1);
        assert_eq!(report.black_list_left, 1);
        assert_eq!(db.books[0].file_path, format!("{target}/here.cbz"));
        assert!(!db.books[0].file_is_missing);
        assert_eq!(db.books[0].file_size, 1);
        // Not found → fileless, metadata (the id/series) kept.
        assert!(db.books[1].file_path.is_empty());
        assert!(!db.books[1].file_is_missing);
        assert_eq!(db.watch_folders[0].folder, target);
        assert_eq!(db.watch_folders[1].folder, "C:\\Elsewhere");
        assert_eq!(db.black_list[0], format!("{target}/here.cbz"));
        assert_eq!(db.black_list[1], "C:\\nope\\gone.cbz");
        assert!(report.changed_anything());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn apply_without_mappings_changes_nothing() {
        let mut db = ComicDatabase {
            books: vec![book("C:\\Comics\\x.cbz")],
            ..Default::default()
        };
        let report = apply(&mut db, &[]);
        assert_eq!(report, ApplyReport::default());
        assert_eq!(db.books[0].file_path, "C:\\Comics\\x.cbz");
    }
}
