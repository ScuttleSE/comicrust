//! Minimal [`SystemPaths`] port (Linux-native semantics).
//!
//! The full settings port (`IniFile`/`EngineConfiguration`/the settings
//! file) is still an open Phase 0 tail; this slice carries only the
//! filesystem layout T1 needs. The C# layout is
//! `%APPDATA%\{Company}\{Product}\ComicDb\ComicDb.xml` with
//! Company="cYo", Product="ComicRack Community Edition"; on Linux the
//! equivalent roaming location is the XDG data dir, and the
//! Company/Product nesting flattens to the product name `comicrust`.

use std::path::PathBuf;

/// The C# `SystemPaths` subset (field names kept for parity).
pub struct Paths {
    /// `ApplicationDataPath` — the roaming data root
    /// (`${XDG_DATA_HOME:-~/.local/share}/comicrust`).
    pub application_data_path: PathBuf,
    /// `DatabasePath` — `{ApplicationDataPath}/ComicDb` (no file
    /// extension; the C# `DatabaseManager` appends `.xml` on save).
    pub database_path: PathBuf,
}

/// The C# `DatabaseManager.DatabaseFile + ".xml"` — the actual
/// ComicDb.xml location under [`Paths::database_path`].
pub fn database_file(paths: &Paths) -> PathBuf {
    paths.database_path.join("ComicDb.xml")
}

impl Paths {
    /// `new SystemPaths(...)` with the default (non-local) locations.
    /// The directories are created on construction, like the C#
    /// `MakeApplicationPath` (`Directory.CreateDirectory`).
    pub fn new_default() -> Paths {
        Paths::from_xdg_root(&application_data_root())
    }

    /// The layout for one XDG data root (the test seam; the C# injects
    /// the overrides through the constructor arguments).
    pub fn from_xdg_root(root: &std::path::Path) -> Paths {
        let application_data_path = root.join("comicrust");
        let database_path = application_data_path.join("ComicDb");
        let _ = std::fs::create_dir_all(&database_path);
        Paths {
            application_data_path,
            database_path,
        }
    }
}

/// `Environment.GetFolderPath(SpecialFolder.ApplicationData)` with XDG
/// semantics: `XDG_DATA_HOME` when set and absolute, else
/// `$HOME/.local/share`, else `.`.
fn application_data_root() -> PathBuf {
    match std::env::var_os("XDG_DATA_HOME") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            home.join(".local").join("share")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_layout_matches_the_c_shape() {
        // The layout mirrors the C# nesting: data root / product /
        // ComicDb, and the database file is `ComicDb.xml` below it.
        let root = std::env::temp_dir().join(format!(
            "comicrust-paths-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = Paths::from_xdg_root(&root);
        assert_eq!(paths.database_path, root.join("comicrust").join("ComicDb"));
        assert_eq!(
            database_file(&paths),
            root.join("comicrust").join("ComicDb").join("ComicDb.xml")
        );
        assert!(paths.database_path.is_dir());
    }
}
