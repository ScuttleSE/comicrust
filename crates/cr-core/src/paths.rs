//! The [`SystemPaths`] port (Linux-native semantics).
//!
//! C# layout (`SystemPaths.cs`): `%APPDATA%\{Company}\{Product}` holds
//! the database, `Config.xml`, the news feeds and the secondary script
//! path; `%LOCALAPPDATA%\{Company}\{Product}\Cache` holds the disk
//! caches. On Linux the roaming/local split collapses to the XDG
//! buckets (ADR-023): configuration lives under
//! `~/.config/comicrust`, data (database, scripts, news) and the disk
//! caches live under `~/.local/share/comicrust`. The Company/Product
//! nesting flattens to the product name `comicrust`.

use std::path::PathBuf;

/// The C# `SystemPaths` subset (field names kept for parity).
pub struct Paths {
    /// `ApplicationDataPath` — the data root
    /// (`${XDG_DATA_HOME:-~/.local/share}/comicrust`).
    pub application_data_path: PathBuf,
    /// The configuration root — `~/.config/comicrust` (ADR-023;
    /// holds `Config.xml` and `comicrust.ini`).
    pub config_path: PathBuf,
    /// `LocalApplicationDataPath` — the cache root (ADR-023: the
    /// C# non-roaming cache root maps onto the same XDG data tree).
    pub local_application_data_path: PathBuf,
    /// `DatabasePath` — `{ApplicationDataPath}/ComicDb` (no file
    /// extension; the C# `DatabaseManager` appends `.xml` on save).
    pub database_path: PathBuf,
    /// `ThumbnailCachePath` — `{Cache}/Thumbnails`.
    pub thumbnail_cache_path: PathBuf,
    /// `ImageCachePath` — `{Cache}/Images`.
    pub image_cache_path: PathBuf,
    /// `FileCachePath` — `{Cache}/Files`.
    pub file_cache_path: PathBuf,
    /// `CustomThumbnailPath` — `{Cache}/CustomThumbnails`.
    pub custom_thumbnail_path: PathBuf,
    /// `ScriptPathSecondary` — `{ApplicationDataPath}/Scripts`.
    pub script_path_secondary: PathBuf,
    /// `PendingScriptsPath` — `{ScriptPathSecondary}/.Pending`.
    pub pending_scripts_path: PathBuf,
}

/// The C# `DatabaseManager.DatabaseFile + ".xml"` — the actual
/// ComicDb.xml location under [`Paths::database_path`].
pub fn database_file(paths: &Paths) -> PathBuf {
    paths.database_path.join("ComicDb.xml")
}

/// The C# `defaultSettingsFile` — `Config.xml` in the application
/// data path (`Settings.Load`/`Save`; ADR-023 puts it in the config
/// tree).
pub fn settings_file(paths: &Paths) -> PathBuf {
    paths.config_path.join("Config.xml")
}

/// The C# `defaultNewsFile` — `NewsFeeds.xml`.
pub fn news_file(paths: &Paths) -> PathBuf {
    paths.application_data_path.join("NewsFeeds.xml")
}

/// The ini file base name: the C# uses the entry assembly file name
/// (`ComicRack.ini`); the binary is `comicrust`.
pub const INI_FILE_NAME: &str = "comicrust.ini";

/// The C# `IniFile.DefaultIniFile` search chain: startup folder,
/// common (system) location, user location — later files override
/// earlier ones when read (`GetDefaultLocations` + `ReadFile`).
pub fn ini_default_locations(paths: &Paths) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join(INI_FILE_NAME));
        }
    }
    out.push(PathBuf::from("/etc").join("comicrust").join(INI_FILE_NAME));
    out.push(paths.config_path.join(INI_FILE_NAME));
    out
}

impl Paths {
    /// `new SystemPaths(...)` with the default (non-local) locations.
    /// The directories are created on construction, like the C#
    /// `MakeApplicationPath` (`Directory.CreateDirectory`).
    ///
    /// The C# `Program.Paths` constructor passes
    /// `ExtendedSettings.CachePath` (the `-cp` switch / the ini
    /// `CachePath` key): a non-empty override replaces the whole
    /// cache root — `Thumbnails`/`Images`/`Files`/`CustomThumbnails`
    /// sit under it (SystemPaths.cs:47-50). The database and config
    /// trees are NOT affected.
    pub fn new_default() -> Paths {
        let cache_root = crate::settings::ExtendedSettings::global()
            .cache_path
            .clone()
            .filter(|p| !p.is_empty())
            .map(PathBuf::from);
        Paths::from_roots_cache_override(&data_root(), &config_root(), cache_root.as_deref())
    }

    /// The layout for one XDG data root plus one config root (the
    /// test seam; the C# injects the overrides through the
    /// constructor arguments).
    pub fn from_roots(data_root: &std::path::Path, config_root: &std::path::Path) -> Paths {
        Paths::from_roots_cache_override(data_root, config_root, None)
    }

    /// `from_roots` with the `SystemPaths` cache-path override (the
    /// `-cp` switch / `CachePath` ini key parity): a non-empty
    /// override replaces the whole cache root.
    pub fn from_roots_cache_override(
        data_root: &std::path::Path,
        config_root: &std::path::Path,
        cache_override: Option<&std::path::Path>,
    ) -> Paths {
        let application_data_path = data_root.join("comicrust");
        let config_path = config_root.join("comicrust");
        let database_path = application_data_path.join("ComicDb");
        let cache = match cache_override {
            Some(p) => p.to_path_buf(),
            None => application_data_path.join("Cache"),
        };
        let script_path_secondary = application_data_path.join("Scripts");
        let pending_scripts_path = script_path_secondary.join(".Pending");
        let _ = std::fs::create_dir_all(&database_path);
        let _ = std::fs::create_dir_all(&config_path);
        let _ = std::fs::create_dir_all(&cache);
        let _ = std::fs::create_dir_all(&script_path_secondary);
        Paths {
            config_path,
            local_application_data_path: application_data_path.clone(),
            application_data_path,
            database_path,
            thumbnail_cache_path: cache.join("Thumbnails"),
            image_cache_path: cache.join("Images"),
            file_cache_path: cache.join("Files"),
            custom_thumbnail_path: cache.join("CustomThumbnails"),
            script_path_secondary,
            pending_scripts_path,
        }
    }

    /// The layout for one XDG data root (config = data root; the
    /// historical test seam from the ADR-022 slice).
    pub fn from_xdg_root(root: &std::path::Path) -> Paths {
        Paths::from_roots(root, root)
    }
}

/// `Environment.GetFolderPath(SpecialFolder.ApplicationData)` with XDG
/// semantics: `XDG_DATA_HOME` when set and absolute, else
/// `$HOME/.local/share`, else `.`.
fn data_root() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", |home| home.join(".local").join("share"))
}

/// The config root: `XDG_CONFIG_HOME` when set and absolute, else
/// `$HOME/.config`, else `.`.
fn config_root() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", |home| home.join(".config"))
}

fn xdg_dir(var: &str, home_path: impl Fn(&PathBuf) -> PathBuf) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            home_path(&home)
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

    #[test]
    fn config_files_live_in_the_config_tree() {
        // ADR-023: Config.xml and the ini live under ~/.config,
        // data and caches under the data tree.
        let data = std::env::temp_dir().join(format!(
            "comicrust-cfgdata-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cfg = std::env::temp_dir().join(format!(
            "comicrust-cfgdir-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = Paths::from_roots(&data, &cfg);
        assert_eq!(
            settings_file(&paths),
            cfg.join("comicrust").join("Config.xml")
        );
        assert_eq!(
            news_file(&paths),
            data.join("comicrust").join("NewsFeeds.xml")
        );
        assert_eq!(
            paths.custom_thumbnail_path,
            data.join("comicrust")
                .join("Cache")
                .join("CustomThumbnails")
        );
        assert!(paths.config_path.is_dir());
        let locs = ini_default_locations(&paths);
        assert_eq!(locs.len(), 3);
        assert_eq!(locs[1], PathBuf::from("/etc/comicrust/comicrust.ini"));
        assert_eq!(locs[2], cfg.join("comicrust").join("comicrust.ini"));
    }

    #[test]
    fn cache_root_override_replaces_only_the_cache_tree() {
        // The C# `SystemPaths(useLocal, alternateConfig, databasePath,
        // cachePath)` parity: a non-empty cache override replaces the
        // whole cache root; the database and config trees stay.
        let data = std::env::temp_dir().join(format!(
            "comicrust-cachedata-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cfg = std::env::temp_dir().join("comicrust-cacheovcfg");
        let over = std::env::temp_dir().join(format!(
            "comicrust-cacheov-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = Paths::from_roots_cache_override(&data, &cfg, Some(&over));
        assert_eq!(paths.thumbnail_cache_path, over.join("Thumbnails"));
        assert_eq!(paths.image_cache_path, over.join("Images"));
        assert_eq!(paths.file_cache_path, over.join("Files"));
        assert_eq!(paths.custom_thumbnail_path, over.join("CustomThumbnails"));
        // The override root itself is created (the subfolders appear
        // when the DiskCaches open — the C# creates them the same
        // lazy way).
        assert!(over.is_dir());
        // The database stays under the data root.
        assert_eq!(paths.database_path, data.join("comicrust").join("ComicDb"));
    }
}
