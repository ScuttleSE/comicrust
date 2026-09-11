//! The [`ExtendedSettings`] port (`ComicRack/Config/ExtendedSettings.cs`)
//! — the boot-path options: command-line switches plus the ini file.
//!
//! C# boot (`Program.ExtendedSettings` + `CommandLineParser.Parse`):
//! switches match by property name or `ShortName`
//! (case-insensitive); a bool switch TOGGLES and consumes no value;
//! other switches consume the NEXT argument; unknown switches are
//! swallowed; the remaining (non-switch) arguments land in `files`.
//! `CommandLineParserOptions.UseIni` also applies the ini by property
//! name (the `[IniFile(false)]` fields stay command-line-only).
//!
//! Deviation (documented): the C# parses the arguments twice — the
//!   first pass decides whether the alternate-config directory joins
//!   the ini search chain — while this port resolves the chain in one
//!   pass (`load`).

use super::enums::{QueryCacheMode, Themes};
use super::ini::IniValues;
use super::registry::{apply_ini, EnumValue, FieldDesc, FieldKind, Value};

/// Server port defaults (`ComicLibraryServerConfig`).
pub const DEFAULT_SERVICE_PORT: i32 = 7612;

#[derive(Clone, Debug, PartialEq)]
pub struct ExtendedSettings {
    pub register_formats: Option<String>,
    pub restart: bool,
    pub wait_pid: i32,
    pub disable_auto_tune_system: bool,
    pub disable_folders_view: bool,
    pub query_cache_mode: QueryCacheMode,
    pub do_not_load_query_caches: bool,
    pub disable_background_query_cache_update: bool,
    pub enable_group_name_compression: bool,
    pub system_tool_bars: bool,
    pub force_tan_color_schema: bool,
    pub mac_compatible_scanning: bool,
    pub show_script_console: bool,
    pub disable_script_optimization: bool,
    pub show_context_help_key: bool,
    pub data_source: Option<String>,
    /// `DatabaseBackgroundSaving` (seconds) — drives the background
    /// database save timer.
    pub database_background_saving: i32,
    /// `ScanFileTimeoutSeconds` (PORT ADDITION) — the longest time one
    /// file may take during a library scan before the scan abandons it,
    /// marks it "Timed out", and moves to the next file. 0 disables the
    /// deadline.
    pub scan_file_timeout_seconds: i32,
    /// `ScanRetryFailedFiles` (PORT ADDITION) — re-read files that
    /// already carry an unchanged scan failure. Off by default, so a
    /// rescan does not pay for the same failures again.
    pub scan_retry_failed_files: bool,
    pub load_database_in_foreground: bool,
    pub alternate_config: Option<String>,
    pub language: Option<String>,
    pub database_path: Option<String>,
    pub cache_path: Option<String>,
    pub limit_memory: i32,
    pub consolidate_database: bool,
    pub import_list: Option<String>,
    pub install_plugin: Option<String>,
    /// The `CommandLineFiles` property: the non-switch arguments.
    pub files: Vec<String>,
    pub workspace: Option<String>,
    pub page: i32,
    pub disable_hardware: bool,
    pub force_hardware: bool,
    pub disable_mip_mapping: bool,
    pub keyboard_zoom_stepping: f32,
    pub anamorphic_scaling_tolerance: f32,
    pub disable_broadcast: bool,
    pub internet_server_port: i32,
    pub private_server_port: i32,
    pub own_remote_connect: bool,
    pub disable_backup_manager: bool,
    pub disable_menu_hide_show_animation: bool,
    pub list_menu_size: i32,
    pub mouse_switches_to_full_library: bool,
    pub drag_drop_cursor_alpha: f32,
    pub auto_hide_cursor_duration: i32,
    /// clamped 0..=255
    pub comic_count_alpha: i32,
    pub quick_open_list_size: i32,
    pub replace_default_lists_in_quick_open: bool,
    pub remote_libraries_in_quick_open: bool,
    pub only_local_remote_libraries_in_quick_open: bool,
    pub hide_browser_if_shell_open: bool,
    pub disable_list_spin_buttons: bool,
    pub optimized_list_scrolling: bool,
    pub allow_copy_list_folders: bool,
    pub do_not_reset_zoom_on_book_open: bool,
    pub show_custom_script_values: bool,
    pub sort_network_folders: bool,
    pub use_local_settings: bool,
    pub use_dark_mode: bool,
    pub theme: Themes,
    pub start_hidden: bool,
    pub legacy_stack_sorting: bool,
    pub open_explorer_using_api: bool,
}

impl Default for ExtendedSettings {
    fn default() -> Self {
        ExtendedSettings {
            register_formats: None,
            restart: false,
            wait_pid: 0,
            disable_auto_tune_system: false,
            disable_folders_view: false,
            query_cache_mode: QueryCacheMode::InstantUpdate,
            do_not_load_query_caches: false,
            disable_background_query_cache_update: true,
            enable_group_name_compression: false,
            system_tool_bars: false,
            force_tan_color_schema: false,
            mac_compatible_scanning: true,
            show_script_console: false,
            disable_script_optimization: false,
            show_context_help_key: false,
            data_source: None,
            database_background_saving: 600,
            scan_file_timeout_seconds: 120,
            scan_retry_failed_files: false,
            load_database_in_foreground: false,
            alternate_config: None,
            language: None,
            database_path: None,
            cache_path: None,
            limit_memory: 0,
            consolidate_database: false,
            import_list: None,
            install_plugin: None,
            files: Vec::new(),
            workspace: None,
            page: 0,
            disable_hardware: false,
            force_hardware: false,
            disable_mip_mapping: false,
            keyboard_zoom_stepping: 0.5,
            anamorphic_scaling_tolerance: 0.25,
            disable_broadcast: false,
            internet_server_port: DEFAULT_SERVICE_PORT,
            private_server_port: DEFAULT_SERVICE_PORT,
            own_remote_connect: false,
            disable_backup_manager: false,
            disable_menu_hide_show_animation: false,
            list_menu_size: 25,
            mouse_switches_to_full_library: false,
            drag_drop_cursor_alpha: 0.6,
            auto_hide_cursor_duration: 5000,
            comic_count_alpha: 64,
            quick_open_list_size: 10,
            replace_default_lists_in_quick_open: false,
            remote_libraries_in_quick_open: true,
            only_local_remote_libraries_in_quick_open: true,
            hide_browser_if_shell_open: true,
            disable_list_spin_buttons: false,
            optimized_list_scrolling: false,
            allow_copy_list_folders: false,
            do_not_reset_zoom_on_book_open: false,
            show_custom_script_values: false,
            sort_network_folders: true,
            use_local_settings: false,
            use_dark_mode: false,
            theme: Themes::Default,
            start_hidden: false,
            legacy_stack_sorting: false,
            open_explorer_using_api: true,
        }
    }
}

crate::settings_fields! {
    EXTENDED_FIELDS, ExtendedSettings,
    StrOpt "RegisterFormats" => register_formats: Option<String>, cat: "", desc: "", browsable: false, ini: true;
    Bool "Restart" => restart: bool, cat: "", desc: "", browsable: false, ini: true;
    Int "WaitPid" => wait_pid: i32, cat: "", desc: "", browsable: false, ini: true;
    Bool "DisableAutoTuneSystem" => disable_auto_tune_system: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "DisableFoldersView" => disable_folders_view: bool, cat: "", desc: "", browsable: false, ini: true;
    Enum "QueryCacheMode" => query_cache_mode: QueryCacheMode, cat: "", desc: "", browsable: false, ini: true;
    Bool "DoNotLoadQueryCaches" => do_not_load_query_caches: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "DisableBackgroundQueryCacheUpdate" => disable_background_query_cache_update: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "EnableGroupNameCompression" => enable_group_name_compression: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "SystemToolBars" => system_tool_bars: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "ForceTanColorSchema" => force_tan_color_schema: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "MacCompatibleScanning" => mac_compatible_scanning: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "ShowScriptConsole" => show_script_console: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "DisableScriptOptimization" => disable_script_optimization: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "ShowContextHelpKey" => show_context_help_key: bool, cat: "", desc: "", browsable: false, ini: true;
    StrOpt "DataSource" => data_source: Option<String>, cat: "", desc: "", browsable: false, ini: true;
    Int "DatabaseBackgroundSaving" => database_background_saving: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "ScanFileTimeoutSeconds" => scan_file_timeout_seconds: i32, cat: "", desc: "", browsable: false, ini: true;
    Bool "ScanRetryFailedFiles" => scan_retry_failed_files: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "LoadDatabaseInForeground" => load_database_in_foreground: bool, cat: "", desc: "", browsable: false, ini: true;
    StrOpt "AlternateConfig" => alternate_config: Option<String>, cat: "", desc: "", browsable: false, ini: false;
    StrOpt "Language" => language: Option<String>, cat: "", desc: "", browsable: false, ini: false;
    StrOpt "DatabasePath" => database_path: Option<String>, cat: "", desc: "", browsable: false, ini: true;
    StrOpt "CachePath" => cache_path: Option<String>, cat: "", desc: "", browsable: false, ini: true;
    Int "LimitMemory" => limit_memory: i32, cat: "", desc: "", browsable: false, ini: true;
    Bool "ConsolidateDatabase" => consolidate_database: bool, cat: "", desc: "", browsable: false, ini: true;
    StrOpt "ImportList" => import_list: Option<String>, cat: "", desc: "", browsable: false, ini: false;
    StrOpt "InstallPlugin" => install_plugin: Option<String>, cat: "", desc: "", browsable: false, ini: false;
    StrOpt "Workspace" => workspace: Option<String>, cat: "", desc: "", browsable: false, ini: false;
    Int "Page" => page: i32, cat: "", desc: "", browsable: false, ini: false;
    Bool "DisableHardware" => disable_hardware: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "ForceHardware" => force_hardware: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "DisableMipMapping" => disable_mip_mapping: bool, cat: "", desc: "", browsable: false, ini: true;
    Float "KeyboardZoomStepping" => keyboard_zoom_stepping: f32, cat: "", desc: "", browsable: false, ini: true;
    Float "AnamorphicScalingTolerance" => anamorphic_scaling_tolerance: f32, cat: "", desc: "", browsable: false, ini: true;
    Bool "DisableBroadcast" => disable_broadcast: bool, cat: "", desc: "", browsable: false, ini: true;
    Int "InternetServerPort" => internet_server_port: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "PrivateServerPort" => private_server_port: i32, cat: "", desc: "", browsable: false, ini: true;
    Bool "OwnRemoteConnect" => own_remote_connect: bool, cat: "", desc: "", browsable: false, ini: false;
    Bool "DisableBackupManager" => disable_backup_manager: bool, cat: "", desc: "", browsable: false, ini: false;
    Bool "DisableMenuHideShowAnimation" => disable_menu_hide_show_animation: bool, cat: "", desc: "", browsable: false, ini: true;
    Int "ListMenuSize" => list_menu_size: i32, cat: "", desc: "", browsable: false, ini: true;
    Bool "MouseSwitchesToFullLibrary" => mouse_switches_to_full_library: bool, cat: "", desc: "", browsable: false, ini: true;
    Float "DragDropCursorAlpha" => drag_drop_cursor_alpha: f32, cat: "", desc: "", browsable: false, ini: true;
    Int "AutoHideCursorDuration" => auto_hide_cursor_duration: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "ComicCountAlpha" => comic_count_alpha: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "QuickOpenListSize" => quick_open_list_size: i32, cat: "", desc: "", browsable: false, ini: true;
    Bool "ReplaceDefaultListsInQuickOpen" => replace_default_lists_in_quick_open: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "RemoteLibrariesInQuickOpen" => remote_libraries_in_quick_open: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "OnlyLocalRemoteLibrariesInQuickOpen" => only_local_remote_libraries_in_quick_open: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "HideBrowserIfShellOpen" => hide_browser_if_shell_open: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "DisableListSpinButtons" => disable_list_spin_buttons: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "OptimizedListScrolling" => optimized_list_scrolling: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "AllowCopyListFolders" => allow_copy_list_folders: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "DoNotResetZoomOnBookOpen" => do_not_reset_zoom_on_book_open: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "ShowCustomScriptValues" => show_custom_script_values: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "SortNetworkFolders" => sort_network_folders: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "UseLocalSettings" => use_local_settings: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "UseDarkMode" => use_dark_mode: bool, cat: "", desc: "", browsable: false, ini: true;
    Enum "Theme" => theme: Themes, cat: "", desc: "", browsable: false, ini: true;
    Bool "StartHidden" => start_hidden: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "LegacyStackSorting" => legacy_stack_sorting: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "OpenExplorerUsingAPI" => open_explorer_using_api: bool, cat: "", desc: "", browsable: false, ini: true;
}

/// The `CommandLineSwitch` short names, by C# property name. A field
/// missing here has no short switch.
const SHORT_NAMES: &[(&str, &str)] = &[
    ("RegisterFormats", "rf"),
    ("Restart", "restart"),
    ("WaitPid", "waitpid"),
    ("DisableAutoTuneSystem", "dats"),
    ("DisableFoldersView", "dfv"),
    ("QueryCacheMode", "qcm"),
    ("DoNotLoadQueryCaches", "dnlqc"),
    ("DisableBackgroundQueryCacheUpdate", "dbqcu"),
    ("SystemToolBars", "stb"),
    ("ForceTanColorSchema", "ftcs"),
    ("ShowScriptConsole", "ssc"),
    ("DisableScriptOptimization", "dso"),
    ("ShowContextHelpKey", "schk"),
    ("DataSource", "ds"),
    ("DatabaseBackgroundSaving", "dbs"),
    ("LoadDatabaseInForeground", "ldif"),
    ("AlternateConfig", "ac"),
    ("Language", "l"),
    ("DatabasePath", "db"),
    ("CachePath", "cp"),
    ("LimitMemory", "lm"),
    ("ConsolidateDatabase", "cdb"),
    ("ImportList", "il"),
    ("InstallPlugin", "ip"),
    ("Workspace", "ws"),
    ("Page", "p"),
    ("DisableHardware", "hwd"),
    ("ForceHardware", "hwf"),
    ("DisableMipMapping", "hwdmm"),
    ("DisableBroadcast", "dbr"),
    ("InternetServerPort", "isp"),
    ("PrivateServerPort", "psp"),
    ("OwnRemoteConnect", "orc"),
    ("DisableBackupManager", "dbm"),
    ("AllowCopyListFolders", "aclf"),
    ("UseLocalSettings", "local"),
    ("UseDarkMode", "dark"),
    ("Theme", "theme"),
    ("StartHidden", "hidden"),
    ("LegacyStackSorting", "lss"),
];

impl ExtendedSettings {
    /// The full boot parse: the ini plus the `-switch=value` argv
    /// pairs by property name, then the space-separated `-switch
    /// value` arguments (the C# `UpdateProperties(withCommandLine)`
    /// + `ParseSwitch` order — argv wins), then the clamps. Unknown
    ///   switches are swallowed.
    pub fn load(&mut self, ini: &IniValues, argv: &[String]) {
        let mut merged = ini.clone();
        for (k, v) in IniValues::read_command_line(argv).iter() {
            merged.set(k.to_string(), v.to_string());
        }
        apply_ini(&merged, self, EXTENDED_FIELDS);
        self.parse_argv(argv);
        self.normalize();
    }

    /// The `CommandLineParser.ParseSwitch` walk.
    fn parse_argv(&mut self, argv: &[String]) {
        let mut files: Vec<String> = Vec::new();
        let mut i = 0;
        while i < argv.len() {
            let arg = &argv[i];
            if !arg.starts_with('-') {
                files.push(arg.clone());
                i += 1;
                continue;
            }
            let name = arg[1..].to_ascii_lowercase();
            let field = match find_switch(&name) {
                Some(f) => f,
                None => {
                    i += 1; // swallowed, like the C# catch
                    continue;
                }
            };
            if field.kind == FieldKind::Bool {
                // A bool switch TOGGLES and consumes no value.
                let current = match (field.get)(self) {
                    Value::Bool(b) => b,
                    _ => false,
                };
                (field.set)(self, Value::Bool(!current));
                i += 1;
                continue;
            }
            if let Some(value) = argv.get(i + 1) {
                if let Some(v) = super::registry::parse_value(field.kind, value) {
                    (field.set)(self, v);
                }
                i += 2;
            } else {
                i += 1;
            }
        }
        self.files = files;
    }

    /// The handoff re-parse (the C# `StartLast`, `Program.cs:1072`):
    /// a FRESH default plus the argv switches. The ini merge is
    /// irrelevant here — `Files`, `Page` and `ImportList` are
    /// command-line-only fields, so the C# fresh parse reads the
    /// same values for everything `StartLast` consumes.
    pub fn from_argv(argv: &[String]) -> ExtendedSettings {
        let mut s = ExtendedSettings::default();
        s.parse_argv(argv);
        s.normalize();
        s
    }

    /// The C# `ComicCountAlpha` setter clamp.
    pub fn normalize(&mut self) {
        self.comic_count_alpha = self.comic_count_alpha.clamp(0, 255);
    }
}

/// The `Program.ExtendedSettings` singleton (the C# static). The C#
/// static is mutable at runtime, so the port holds it in an
/// `RwLock` — `global` for the read views, `global_mut` for the
/// runtime setters (the theme toggle). `init_global` installs the
/// parsed configuration; before that the defaults apply.
static EXTENDED_GLOBAL: std::sync::OnceLock<std::sync::RwLock<ExtendedSettings>> =
    std::sync::OnceLock::new();

impl ExtendedSettings {
    pub fn init_global(config: ExtendedSettings) {
        // WRITE-THROUGH, not `OnceLock::set`: the global may already
        // be initialized by an early `global()` reader (the library
        // open calls `Paths::new_default()` BEFORE
        // `initialize_settings` — a plain `set` would silently drop
        // the parsed ini/argv configuration; the dark-mode boot bug).
        let lock = EXTENDED_GLOBAL.get_or_init(|| std::sync::RwLock::new(config.clone()));
        *lock
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = config;
    }

    pub fn global() -> std::sync::RwLockReadGuard<'static, ExtendedSettings> {
        EXTENDED_GLOBAL
            .get_or_init(|| std::sync::RwLock::new(ExtendedSettings::default()))
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn global_mut() -> std::sync::RwLockWriteGuard<'static, ExtendedSettings> {
        EXTENDED_GLOBAL
            .get_or_init(|| std::sync::RwLock::new(ExtendedSettings::default()))
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The C# `Theme` getter (`ExtendedSettings.cs`): `UseDarkMode`
    /// forces the Dark theme over the stored `Theme` value.
    pub fn effective_theme(&self) -> Themes {
        if self.use_dark_mode {
            Themes::Dark
        } else {
            self.theme
        }
    }
}

fn find_switch(name: &str) -> Option<&'static FieldDesc<ExtendedSettings>> {
    EXTENDED_FIELDS.iter().find(|f| {
        f.name.eq_ignore_ascii_case(name)
            || SHORT_NAMES
                .iter()
                .any(|(p, s)| *p == f.name && s.eq_ignore_ascii_case(name))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_switches_toggle_without_a_value() {
        let mut s = ExtendedSettings::default();
        s.parse_argv(&["-dark".into(), "-scanstartup".into()]);
        assert!(s.use_dark_mode);
    }

    #[test]
    fn value_switches_consume_the_next_argument() {
        let mut s = ExtendedSettings::default();
        s.parse_argv(&[
            "-db".into(),
            "/comics/db".into(),
            "-dbs".into(),
            "300".into(),
        ]);
        assert_eq!(s.database_path.as_deref(), Some("/comics/db"));
        assert_eq!(s.database_background_saving, 300);
        assert_eq!(s.files, Vec::<String>::new());
    }

    #[test]
    fn short_names_match_case_insensitively() {
        let mut s = ExtendedSettings::default();
        // The space form: the C# CommandLineParser compares the arg
        // text (case-insensitively) against the switch name.
        s.parse_argv(&["-DB".into(), "/x".into()]);
        assert_eq!(s.database_path.as_deref(), Some("/x"));
    }

    #[test]
    fn unknown_switches_are_swallowed_with_their_value() {
        let mut s = ExtendedSettings::default();
        s.parse_argv(&["-bogus".into(), "stray".into(), "file.cbz".into()]);
        // The C# throws inside ParseSwitch (caught) — the VALUE was
        // not consumed there, so `stray` lands in Files.
        assert_eq!(s.files, vec!["stray".to_string(), "file.cbz".to_string()]);
    }

    #[test]
    fn plain_arguments_are_files() {
        let mut s = ExtendedSettings::default();
        s.parse_argv(&["a.cbz".into(), "b.cbz".into()]);
        assert_eq!(s.files.len(), 2);
    }

    #[test]
    fn handoff_parse_reads_files_page_and_import_list() {
        // The `StartLast` re-parse (`Program.cs:1072`): a fresh
        // default plus the argv; unknown switches do not consume
        // their value.
        let s = ExtendedSettings::from_argv(&[
            "--client".to_string(),
            "/tmp/a.cbz".to_string(),
            "-p".to_string(),
            "7".to_string(),
            "-il".to_string(),
            "/tmp/list.cbl".to_string(),
        ]);
        assert_eq!(s.files, vec!["/tmp/a.cbz".to_string()]);
        assert_eq!(s.page, 7);
        assert_eq!(s.import_list.as_deref(), Some("/tmp/list.cbl"));
    }

    #[test]
    fn default_true_bool_switch_toggles_off() {
        let mut s = ExtendedSettings::default();
        assert!(s.disable_background_query_cache_update);
        s.parse_argv(&["-dbqcu".into()]);
        assert!(!s.disable_background_query_cache_update);
    }

    #[test]
    fn argv_equals_form_applies_by_property_name() {
        // `-DatabasePath=/x` rides the IniFile.ReadCommandLine path
        // (rxCommand) and binds by property name.
        let mut s = ExtendedSettings::default();
        s.load(&IniValues::new(), &["-DatabasePath=/x".to_string()]);
        assert_eq!(s.database_path.as_deref(), Some("/x"));
    }

    #[test]
    fn ini_applies_by_property_name_but_not_cmdline_only_fields() {
        let mut s = ExtendedSettings::default();
        let ini = IniValues::read_text("databasebackgroundsaving=120\nlanguage=de", None);
        s.load(&ini, &[]);
        assert_eq!(s.database_background_saving, 120);
        // `Language` is [IniFile(false)]: command line only.
        assert_eq!(s.language, None);
        assert_eq!(s.comic_count_alpha, 64);
    }

    #[test]
    fn comic_count_alpha_clamps() {
        let mut s = ExtendedSettings::default();
        let ini = IniValues::read_text("comiccountalpha=999", None);
        s.load(&ini, &[]);
        assert_eq!(s.comic_count_alpha, 255);
    }

    #[test]
    fn effective_theme_resolves_like_the_csharp_getter() {
        let mut s = ExtendedSettings::default();
        assert_eq!(s.effective_theme(), Themes::Default);
        s.theme = Themes::Dark;
        assert_eq!(s.effective_theme(), Themes::Dark);
        // The C# getter: UseDarkMode wins over the stored Theme.
        s.theme = Themes::Default;
        s.use_dark_mode = true;
        assert_eq!(s.effective_theme(), Themes::Dark);
    }
}
