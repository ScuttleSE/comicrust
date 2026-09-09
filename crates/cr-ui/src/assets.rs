//! Runtime asset lookup for the bundled tree (`icons`, `papers`,
//! `backgrounds`) shared by `icon.rs` and the reader texture loaders.
//!
//! Root order (first hit wins):
//! 1. CWD-relative `assets` — the portable tarball layout,
//! 2. CWD-relative `crates/cr-ui/assets` — the dev workspace,
//! 3. `$XDG_DATA_HOME/comicrust/assets` — a user override,
//! 4. `<exe dir>/../share/comicrust/assets` — the system install
//!    (`/usr/bin` → `/usr/share`; flatpak `/app/bin` → `/app/share`),
//! 5. every `$XDG_DATA_DIRS` entry + `/comicrust/assets`.
//!
//! Roots 1-2 preserve the pre-packaging behavior exactly; 3-5 are the
//! Phase 11 packaging additions.

use std::ffi::OsString;
use std::path::PathBuf;

/// The asset roots for the bundled tree, in lookup order.
pub fn asset_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = ["assets", "crates/cr-ui/assets"]
        .iter()
        .map(PathBuf::from)
        .collect();
    if let Some(root) = data_home_root(std::env::var_os("XDG_DATA_HOME")) {
        roots.push(root);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin) = exe.parent() {
            roots.push(bin.join("../share/comicrust/assets"));
        }
    }
    roots.extend(data_dir_roots(std::env::var_os("XDG_DATA_DIRS")));
    roots
}

/// The `$XDG_DATA_HOME` root (`None` when absent or empty).
fn data_home_root(data_home: Option<OsString>) -> Option<PathBuf> {
    data_home
        .filter(|v| !v.is_empty())
        .map(|home| PathBuf::from(home).join("comicrust/assets"))
}

/// The `$XDG_DATA_DIRS` roots (the XDG default when absent or empty;
/// empty path components skipped).
fn data_dir_roots(data_dirs: Option<OsString>) -> Vec<PathBuf> {
    let dirs = match data_dirs {
        Some(v) if !v.is_empty() => v,
        _ => OsString::from("/usr/local/share:/usr/share"),
    };
    std::env::split_paths(&dirs)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join("comicrust/assets"))
        .collect()
}

/// Resolves `rel` (e.g. `icons/Sort.png`) against the live roots; the
/// first existing FILE wins.
pub fn find(rel: &str) -> Option<PathBuf> {
    find_with(&asset_roots(), rel)
}

/// The pure resolution core (tests pass synthetic roots).
pub fn find_with(roots: &[PathBuf], rel: &str) -> Option<PathBuf> {
    roots
        .iter()
        .map(|root| root.join(rel))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("comicrust-assets-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &PathBuf) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn first_root_with_the_file_wins() {
        let a = scratch("a");
        let b = scratch("b");
        touch(&a.join("icons/Sort.png"));
        touch(&b.join("icons/Sort.png"));
        let hit = find_with(&[a.clone(), b.clone()], "icons/Sort.png").unwrap();
        assert!(hit.starts_with(&a));
    }

    #[test]
    fn later_root_serves_when_earlier_misses() {
        let a = scratch("miss");
        let b = scratch("hit");
        touch(&b.join("papers/Checkered.jpg"));
        let want = b.join("papers/Checkered.jpg");
        assert_eq!(find_with(&[a, b], "papers/Checkered.jpg"), Some(want));
    }

    #[test]
    fn missing_file_is_none() {
        let a = scratch("none");
        assert_eq!(find_with(&[a], "icons/Absent.png"), None);
    }

    #[test]
    fn data_home_root_maps_into_comicrust() {
        assert_eq!(
            data_home_root(Some("/xdg/home".into())),
            Some(PathBuf::from("/xdg/home/comicrust/assets"))
        );
    }

    #[test]
    fn empty_data_home_is_skipped() {
        assert_eq!(data_home_root(Some("".into())), None);
        assert_eq!(data_home_root(None), None);
    }

    #[test]
    fn data_dirs_split_and_map_each_entry() {
        assert_eq!(
            data_dir_roots(Some("/opt/share:/usr/share".into())),
            vec![
                PathBuf::from("/opt/share/comicrust/assets"),
                PathBuf::from("/usr/share/comicrust/assets"),
            ]
        );
    }

    #[test]
    fn empty_data_dirs_falls_back_to_the_xdg_default() {
        assert_eq!(
            data_dir_roots(Some("".into())),
            vec![
                PathBuf::from("/usr/local/share/comicrust/assets"),
                PathBuf::from("/usr/share/comicrust/assets"),
            ]
        );
        assert_eq!(
            data_dir_roots(None),
            vec![
                PathBuf::from("/usr/local/share/comicrust/assets"),
                PathBuf::from("/usr/share/comicrust/assets"),
            ]
        );
    }

    #[test]
    fn empty_path_components_are_skipped() {
        assert_eq!(
            data_dir_roots(Some("a::b".into())),
            vec![
                PathBuf::from("a/comicrust/assets"),
                PathBuf::from("b/comicrust/assets"),
            ]
        );
    }
}
