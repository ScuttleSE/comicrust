//! The [`Settings`] port (`ComicRack/Config/Settings.cs`) — the main
//! user settings object, persisted as the `[settings]` section of the
//! unified config file `comicrust.toml` (ADR-033; the C# persists it
//! as `Config.xml` — the Linux port replaced that store).
//!
//! Scope: every SCALAR field plus the string lists the app keeps are
//! ported, in the C# property declaration order. The complex members
//! (ListConfigurations, ExternalPrograms, devices, remote shares,
//! export presets, VirtualTags, BackupManager) belong to later phases.
//!
//! The serde names are the C# member names (`rename_all` PascalCase;
//! the spelling quirks the C# serializer carries are pinned with
//! explicit renames). Unknown TOML keys are skipped on load, so a
//! hand-edited file keeps working.
//!
//! The C# `[DefaultValue]` attributes on Settings are designer
//! metadata — the effective defaults are the FIELD INITIALIZERS, and
//! those are what `Default` reproduces.

use super::enums::{
    HiddenMessageBoxes, ImageDisplayOptions, LibraryGauges, MagnifierStyle, RightToLeftReadingMode,
    TabLayouts,
};
use crate::model::enums::ComicPageType;
use crate::xml::scalar::CrGuid;

/// `Settings.RecentFileCount`.
pub const RECENT_FILE_COUNT: i32 = 20;
/// `Settings.MinimumMemoryPageCacheCount`.
pub const MINIMUM_MEMORY_PAGE_CACHE_COUNT: i32 = 20;
/// `Settings.MaximumMemoryPageCacheCount`.
pub const MAXIMUM_MEMORY_PAGE_CACHE_COUNT: i32 = 100;
/// `Settings.DefaultMemoryPageCacheCount`.
pub const DEFAULT_MEMORY_PAGE_CACHE_COUNT: i32 = 25;
/// `Settings.MinimumMemoryThumbnailCacheMB`.
pub const MINIMUM_MEMORY_THUMBNAIL_CACHE_MB: i32 = 5;
/// `Settings.MaximumMemoryThumbnailCacheMB`.
pub const MAXIMUM_MEMORY_THUMBNAIL_CACHE_MB: i32 = 500;
/// `Settings.DefaultMemoryThumbnailCacheMB`.
pub const DEFAULT_MEMORY_THUMBNAIL_CACHE_MB: i32 = 50;
/// `Settings.DefaultInternetCacheSizeMB`.
pub const DEFAULT_INTERNET_CACHE_SIZE_MB: i32 = 1000;
/// `Settings.DefaultHelpSystem`.
pub const DEFAULT_HELP_SYSTEM: &str = "ComicRack Online Manual";
/// `Settings.UnlimitedSystemMemory`.
pub const UNLIMITED_SYSTEM_MEMORY: i32 = 4096;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Settings {
    pub run_count: i32,
    pub paste_properties: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_browser: Option<String>,
    pub page_filter: ComicPageType,
    pub last_explorer_folder: String,
    pub explorer_include_sub_folders: bool,
    pub last_library_item: CrGuid,
    pub last_open_filter_index: i32,
    pub last_save_filter_index: i32,
    pub last_export_page_filter_index: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugins_states: Option<String>,
    pub move_files_to_recycle_bin: bool,
    pub also_remove_from_library: bool,
    pub also_remove_from_library_filtered: bool,
    /// `Settings.FavoriteFolders` (the Files browser's favorites).
    pub favorite_folders: Vec<String>,
    /// The C# member carries the lowercase-`f` quirk
    /// (`RemoveFilesfromDatabase`) — pinned.
    #[serde(rename = "RemoveFilesfromDatabase")]
    pub remove_files_from_database: bool,
    pub tab_layouts: TabLayouts,
    pub quick_open_thumbnail_size: i32,
    pub external_server_address: String,
    pub private_listing_password: String,
    pub look_for_shared: bool,
    pub auto_connect_shares: bool,
    pub extra_wifi_device_addresses: String,
    pub page_image_display_options: ImageDisplayOptions,
    pub overlay_scaling: i32,
    pub magnify_size: (i32, i32),
    #[serde(with = "crate::settings::unified::f32_shortest")]
    pub magnify_opaque: f32,
    #[serde(with = "crate::settings::unified::f32_shortest")]
    pub magnify_zoom: f32,
    pub magnify_style: MagnifierStyle,
    pub auto_magnifier: bool,
    pub hardware_acceleration: bool,
    pub display_change_animation: bool,
    pub flowing_mouse_scrolling: bool,
    pub software_filtering: bool,
    pub hardware_filtering: bool,
    #[serde(with = "crate::settings::unified::f32_shortest")]
    pub mouse_wheel_speed: f32,
    pub reader_keyboard_mapping: Vec<(String, String)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignored_cover_images: Option<String>,
    pub auto_scrolling: bool,
    pub hidden_message_boxes: HiddenMessageBoxes,
    pub update_comic_files: bool,
    pub update_comic_book_files: bool,
    pub auto_update_comics_files: bool,
    pub help_system: String,
    pub scripting: bool,
    pub scripting_libraries: String,
    pub hide_sample_scripts: bool,
    pub show_splash: bool,
    pub open_last_file: bool,
    pub scan_startup: bool,
    pub update_web_comics_startup: bool,
    pub news_startup: bool,
    pub last_open_files: Vec<String>,
    pub show_quick_manual: bool,
    pub open_last_page: bool,
    pub close_browser_on_open: bool,
    pub add_to_library_on_open: bool,
    pub open_in_new_tab: bool,
    pub hide_cursor_full_screen: bool,
    pub auto_navigate_comics: bool,
    pub show_current_page_overlay: bool,
    pub show_visible_page_part_overlay: bool,
    pub show_status_overlay: bool,
    pub show_navigation_overlay: bool,
    pub navigation_overlay_on_top: bool,
    pub current_page_shows_name: bool,
    pub auto_hide_magnifier: bool,
    pub page_change_delay: bool,
    pub scrolling_does_browse: bool,
    pub reset_zoom_on_page_change: bool,
    pub zoom_in_out_on_page_change: bool,
    pub smooth_scrolling: bool,
    pub blend_while_paging: bool,
    pub track_current_page: bool,
    pub right_to_left_reading_mode: RightToLeftReadingMode,
    pub true_right_to_left_reading: bool,
    pub left_right_movement_reversed: bool,
    pub show_tool_tips: bool,
    pub show_search_links: bool,
    pub fade_in_thumbnails: bool,
    pub dog_ear_thumbnails: bool,
    pub numeric_rating_thumbnails: bool,
    pub local_quick_search: bool,
    pub cover_thumbnails_same_size: bool,
    pub common_list_stack_layout: bool,
    pub show_quick_open: bool,
    pub catalog_only_for_fileless: bool,
    pub show_custom_book_fields: bool,
    pub minimize_to_tray: bool,
    pub close_minimizes_to_tray: bool,
    pub auto_minimal_gui: bool,
    pub animate_panels: bool,
    pub always_display_browser_docking_grip: bool,
    pub disable_drag_drop: bool,
    pub auto_hide_main_menu: bool,
    pub show_main_menu_no_comic_open: bool,
    /// The `3D` spelling is pinned (PascalCase would give `3d`).
    #[serde(rename = "InformationCover3D")]
    pub information_cover3d: bool,
    pub display_library_gauges: bool,
    pub library_gauges_format: LibraryGauges,
    pub new_books_checked: bool,
    pub thumb_cache_enabled: bool,
    /// PORT ADDITION (no C# counterpart): when false the grid loads
    /// only already-cached covers; the File ▸ Generate Cover
    /// Thumbnails command backfills the cache instead.
    pub generate_thumbnails_on_demand: bool,
    /// The `MB` spellings are pinned (the C# member names carry the
    /// uppercase pair).
    #[serde(rename = "ThumbCacheSizeMB")]
    pub thumb_cache_size_mb: i32,
    pub page_cache_enabled: bool,
    #[serde(rename = "PageCacheSizeMB")]
    pub page_cache_size_mb: i32,
    pub internet_cache_enabled: bool,
    #[serde(rename = "InternetCacheSizeMB")]
    pub internet_cache_size_mb: i32,
    #[serde(rename = "MemoryThumbCacheSizeMB")]
    pub memory_thumb_cache_size_mb: i32,
    pub memory_page_cache_count: i32,
    pub memory_thumb_cache_optimized: bool,
    pub memory_page_cache_optimized: bool,
    #[serde(rename = "MaximumMemoryMB")]
    pub maximum_memory_mb: i32,
    pub remove_missing_files_on_full_scan: bool,
    pub dont_add_remove_files: bool,
    pub overwrite_associations: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub culture_name: Option<String>,
    pub exported_lists_contain_filenames: bool,
    pub quick_search_list: Vec<String>,
    pub library_quick_search_list: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_remote_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_remote_password: Option<String>,
    pub auto_show_quick_review: bool,
    /// `Settings.CurrentWorkspace` (the T14 persisted layout; None =
    /// never saved — the defaults apply).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_workspace: Option<super::workspace::WorkspaceState>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            run_count: 0,
            paste_properties: "Series".to_string(),
            selected_browser: None,
            page_filter: ComicPageType(1023), // ComicPageType.All
            last_explorer_folder: String::new(),
            explorer_include_sub_folders: false,
            last_library_item: CrGuid::EMPTY,
            last_open_filter_index: 1,
            last_save_filter_index: 1,
            last_export_page_filter_index: 1,
            plugins_states: None,
            move_files_to_recycle_bin: false,
            also_remove_from_library: false,
            also_remove_from_library_filtered: false,
            favorite_folders: Vec::new(),
            remove_files_from_database: false,
            tab_layouts: TabLayouts(0), // TabLayouts.None
            quick_open_thumbnail_size: 128,
            external_server_address: String::new(),
            private_listing_password: String::new(),
            look_for_shared: true,
            auto_connect_shares: true,
            extra_wifi_device_addresses: String::new(),
            page_image_display_options: ImageDisplayOptions(1), // HighQuality
            overlay_scaling: 100,
            magnify_size: (300, 200),
            magnify_opaque: 1.0,
            magnify_zoom: 2.0,
            magnify_style: MagnifierStyle::Glass,
            auto_magnifier: true,
            hardware_acceleration: true,
            display_change_animation: true,
            flowing_mouse_scrolling: true,
            software_filtering: true,
            hardware_filtering: false,
            mouse_wheel_speed: 2.0,
            reader_keyboard_mapping: Vec::new(),
            ignored_cover_images: None,
            auto_scrolling: false,
            hidden_message_boxes: HiddenMessageBoxes(0), // None
            update_comic_files: false,
            update_comic_book_files: false,
            auto_update_comics_files: false,
            help_system: DEFAULT_HELP_SYSTEM.to_string(),
            scripting: true,
            scripting_libraries: String::new(),
            hide_sample_scripts: false,
            show_splash: true,
            open_last_file: true,
            scan_startup: false,
            update_web_comics_startup: false,
            news_startup: true,
            last_open_files: Vec::new(),
            show_quick_manual: true,
            open_last_page: true,
            close_browser_on_open: false,
            add_to_library_on_open: false,
            open_in_new_tab: false,
            hide_cursor_full_screen: true,
            auto_navigate_comics: true,
            show_current_page_overlay: true,
            show_visible_page_part_overlay: true,
            show_status_overlay: true,
            show_navigation_overlay: true,
            navigation_overlay_on_top: false,
            current_page_shows_name: false,
            auto_hide_magnifier: true,
            page_change_delay: true,
            scrolling_does_browse: true,
            reset_zoom_on_page_change: false,
            zoom_in_out_on_page_change: true,
            smooth_scrolling: true,
            blend_while_paging: false,
            track_current_page: true,
            right_to_left_reading_mode: RightToLeftReadingMode::FlipPages,
            true_right_to_left_reading: false,
            left_right_movement_reversed: false,
            show_tool_tips: false,
            show_search_links: true,
            fade_in_thumbnails: true,
            dog_ear_thumbnails: true,
            numeric_rating_thumbnails: true,
            local_quick_search: true,
            cover_thumbnails_same_size: false,
            common_list_stack_layout: false,
            show_quick_open: true,
            catalog_only_for_fileless: true,
            show_custom_book_fields: false,
            minimize_to_tray: false,
            close_minimizes_to_tray: true,
            auto_minimal_gui: false,
            animate_panels: true,
            always_display_browser_docking_grip: false,
            disable_drag_drop: false,
            auto_hide_main_menu: true,
            show_main_menu_no_comic_open: true,
            information_cover3d: true,
            display_library_gauges: true,
            library_gauges_format: LibraryGauges(0x1007), // Default
            new_books_checked: true,
            thumb_cache_enabled: true,
            generate_thumbnails_on_demand: true,
            thumb_cache_size_mb: 500,
            page_cache_enabled: true,
            page_cache_size_mb: 500,
            internet_cache_enabled: true,
            internet_cache_size_mb: DEFAULT_INTERNET_CACHE_SIZE_MB,
            memory_thumb_cache_size_mb: DEFAULT_MEMORY_THUMBNAIL_CACHE_MB,
            memory_page_cache_count: DEFAULT_MEMORY_PAGE_CACHE_COUNT,
            memory_thumb_cache_optimized: true,
            memory_page_cache_optimized: true,
            maximum_memory_mb: UNLIMITED_SYSTEM_MEMORY,
            remove_missing_files_on_full_scan: false,
            dont_add_remove_files: false,
            overwrite_associations: false,
            culture_name: None,
            exported_lists_contain_filenames: false,
            quick_search_list: Vec::new(),
            library_quick_search_list: Vec::new(),
            open_remote_filter: None,
            open_remote_password: None,
            auto_show_quick_review: false,
            current_workspace: None,
        }
    }
}

crate::settings_fields! {
    SETTINGS_FIELDS, Settings,
    Bool "LookForShared" => look_for_shared: bool, cat: "Network", desc: "Look for locally shared comic libraries on the network", browsable: false, ini: true;
    Bool "AutoConnectShares" => auto_connect_shares: bool, cat: "Network", desc: "Autoconnect shares", browsable: false, ini: true;
    Bool "AutoScrolling" => auto_scrolling: bool, cat: "Behavior", desc: "Turns autoscrolling on", browsable: false, ini: true;
    Bool "UpdateComicFiles" => update_comic_files: bool, cat: "Behavior", desc: "Update Book Files with new information", browsable: false, ini: true;
    Bool "UpdateComicBookFiles" => update_comic_book_files: bool, cat: "Behavior", desc: "Update Book Files with extra information", browsable: false, ini: true;
    Bool "AutoUpdateComicsFiles" => auto_update_comics_files: bool, cat: "Behavior", desc: "Auto update of Book files", browsable: false, ini: true;
    Bool "Scripting" => scripting: bool, cat: "Scripting", desc: "Enable or disable Scripting", browsable: false, ini: true;
    Bool "ShowSplash" => show_splash: bool, cat: "Starting ComicRack", desc: "Show Splash Screen", browsable: true, ini: true;
    Bool "OpenLastFile" => open_last_file: bool, cat: "Starting ComicRack", desc: "Reopen Books from last session", browsable: true, ini: true;
    Bool "ScanStartup" => scan_startup: bool, cat: "Starting ComicRack", desc: "Rescan the Book Folders for new Books", browsable: true, ini: true;
    Bool "UpdateWebComicsStartup" => update_web_comics_startup: bool, cat: "Starting ComicRack", desc: "Update Web Comics", browsable: true, ini: true;
    Bool "NewsStartup" => news_startup: bool, cat: "Starting ComicRack", desc: "Check for latest news on ComicRack", browsable: true, ini: true;
    Bool "OpenLastPage" => open_last_page: bool, cat: "Opening a Book", desc: "Open the Book at the page where it was closed", browsable: true, ini: true;
    Bool "CloseBrowserOnOpen" => close_browser_on_open: bool, cat: "Opening a Book", desc: "Close the Browser when a new Book is opened", browsable: true, ini: true;
    Bool "AddToLibraryOnOpen" => add_to_library_on_open: bool, cat: "Opening a Book", desc: "Opened Files are added to the Library", browsable: true, ini: true;
    Bool "OpenInNewTab" => open_in_new_tab: bool, cat: "Opening a Book", desc: "Open in new Tab", browsable: true, ini: true;
    Bool "HideCursorFullScreen" => hide_cursor_full_screen: bool, cat: "Reading", desc: "Hide the mouse cursor when reading in Full Screen Mode", browsable: true, ini: true;
    Bool "AutoNavigateComics" => auto_navigate_comics: bool, cat: "Reading", desc: "Reading beyond the start or end opens the next Book", browsable: true, ini: true;
    Bool "ShowCurrentPageOverlay" => show_current_page_overlay: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "ShowVisiblePagePartOverlay" => show_visible_page_part_overlay: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "ShowStatusOverlay" => show_status_overlay: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "ShowNavigationOverlay" => show_navigation_overlay: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "NavigationOverlayOnTop" => navigation_overlay_on_top: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "CurrentPageShowsName" => current_page_shows_name: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "AutoHideMagnifier" => auto_hide_magnifier: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "PageChangeDelay" => page_change_delay: bool, cat: "Reading", desc: "Mouse Wheel and Cursor Keys delay on page transitions", browsable: true, ini: true;
    Bool "ScrollingDoesBrowse" => scrolling_does_browse: bool, cat: "Reading", desc: "Scrolling to page margin browses to new pages", browsable: true, ini: true;
    Bool "ResetZoomOnPageChange" => reset_zoom_on_page_change: bool, cat: "Reading", desc: "Zoom is reset to 100% on page change", browsable: true, ini: true;
    Bool "ZoomInOutOnPageChange" => zoom_in_out_on_page_change: bool, cat: "Reading", desc: "During page change a zoom out is done", browsable: true, ini: true;
    Bool "SmoothScrolling" => smooth_scrolling: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "BlendWhilePaging" => blend_while_paging: bool, cat: "Reading", desc: "Blend animation while fast paging", browsable: true, ini: true;
    Bool "TrackCurrentPage" => track_current_page: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "TrueRightToLeftReading" => true_right_to_left_reading: bool, cat: "Right to Left", desc: "True right to left reading", browsable: true, ini: true;
    Bool "LeftRightMovementReversed" => left_right_movement_reversed: bool, cat: "Right to Left", desc: "Left/right movement is also reversed", browsable: true, ini: true;
    Bool "ShowToolTips" => show_tool_tips: bool, cat: "Browser", desc: "Show Tooltips for Books in the Browser", browsable: true, ini: true;
    Bool "ShowSearchLinks" => show_search_links: bool, cat: "Browser", desc: "Show Search Links", browsable: true, ini: true;
    Bool "FadeInThumbnails" => fade_in_thumbnails: bool, cat: "Browser", desc: "New loaded Thumbnails slowly fade in", browsable: true, ini: true;
    Bool "DogEarThumbnails" => dog_ear_thumbnails: bool, cat: "Browser", desc: "Selected Thumbnails have a dog-ear", browsable: true, ini: true;
    Bool "NumericRatingThumbnails" => numeric_rating_thumbnails: bool, cat: "Browser", desc: "Thumbnails display numeric ratings", browsable: true, ini: true;
    Bool "LocalQuickSearch" => local_quick_search: bool, cat: "Browser", desc: "Each List has own Quick Search settings", browsable: true, ini: true;
    Bool "CoverThumbnailsSameSize" => cover_thumbnails_same_size: bool, cat: "Browser", desc: "All Cover Thumbnails have the same Size", browsable: true, ini: true;
    Bool "CommonListStackLayout" => common_list_stack_layout: bool, cat: "Browser", desc: "All Stacks in a List have the same Layout", browsable: true, ini: true;
    Bool "ShowQuickOpen" => show_quick_open: bool, cat: "Application", desc: "Show Quick Open when no book is open", browsable: true, ini: true;
    Bool "CatalogOnlyForFileless" => catalog_only_for_fileless: bool, cat: "Application", desc: "Show Catalog fields only for fileless Books", browsable: true, ini: true;
    Bool "ShowCustomBookFields" => show_custom_book_fields: bool, cat: "Application", desc: "Show custom Book fields", browsable: true, ini: true;
    Bool "MinimizeToTray" => minimize_to_tray: bool, cat: "Application", desc: "Minimize moves ComicRack into the Notification Area", browsable: true, ini: true;
    Bool "CloseMinimizesToTray" => close_minimizes_to_tray: bool, cat: "Application", desc: "Close moves ComicRack into the Notification Area", browsable: true, ini: true;
    Bool "AutoMinimalGui" => auto_minimal_gui: bool, cat: "Reading", desc: "Fullscreen also toggles Minimal User Interface", browsable: true, ini: true;
    Bool "AnimatePanels" => animate_panels: bool, cat: "Application", desc: "Animate expanding and collapsing Panels", browsable: true, ini: true;
    Bool "AlwaysDisplayBrowserDockingGrip" => always_display_browser_docking_grip: bool, cat: "Browser", desc: "Always display Browser Docking Grip", browsable: true, ini: true;
    Bool "DisableDragDrop" => disable_drag_drop: bool, cat: "Browser", desc: "Disable opening files via drag-and-drop", browsable: true, ini: true;
    Bool "AutoHideMainMenu" => auto_hide_main_menu: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "ShowMainMenuNoComicOpen" => show_main_menu_no_comic_open: bool, cat: "Application", desc: "Show Main Menu if no Book is open", browsable: true, ini: true;
    Bool "InformationCover3D" => information_cover3d: bool, cat: "Application", desc: "3D display of covers in Book Info Dialog", browsable: true, ini: true;
    Bool "DisplayLibraryGauges" => display_library_gauges: bool, cat: "Application", desc: "", browsable: false, ini: true;
    Bool "NewBooksChecked" => new_books_checked: bool, cat: "Application", desc: "Newly added Books are checked", browsable: true, ini: true;
    Bool "ThumbCacheEnabled" => thumb_cache_enabled: bool, cat: "Caching", desc: "Turn thumbnail caching on or off", browsable: false, ini: true;
    // PORT ADDITION (no C# counterpart): the on-demand cover
    // generation switch (the Preferences caching page row).
    Bool "GenerateThumbnailsOnDemand" => generate_thumbnails_on_demand: bool, cat: "Caching", desc: "Generate cover thumbnails on demand when books are displayed", browsable: false, ini: true;
    Bool "PageCacheEnabled" => page_cache_enabled: bool, cat: "Caching", desc: "Turn page caching on or off", browsable: false, ini: true;
    Bool "InternetCacheEnabled" => internet_cache_enabled: bool, cat: "Caching", desc: "Turn Internet caching on or off", browsable: false, ini: true;
    Bool "MemoryThumbCacheOptimized" => memory_thumb_cache_optimized: bool, cat: "", desc: "Optimize Memory Thumbnail cache", browsable: false, ini: true;
    Bool "MemoryPageCacheOptimized" => memory_page_cache_optimized: bool, cat: "", desc: "Optimize Memory Page cache", browsable: false, ini: true;
    Bool "RemoveMissingFilesOnFullScan" => remove_missing_files_on_full_scan: bool, cat: "", desc: "", browsable: true, ini: true;
    Bool "DontAddRemoveFiles" => dont_add_remove_files: bool, cat: "", desc: "", browsable: true, ini: true;
    Bool "OverwriteAssociations" => overwrite_associations: bool, cat: "", desc: "", browsable: true, ini: true;
    Bool "ExportedListsContainFilenames" => exported_lists_contain_filenames: bool, cat: "Import & Export", desc: "Exported Book Lists contain filenames", browsable: true, ini: true;
    Bool "AutoShowQuickReview" => auto_show_quick_review: bool, cat: "Reading", desc: "Show Quick Review Dialog after finishing Book", browsable: true, ini: true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::unified::{settings_from_value, settings_to_value};

    fn round_trip(s: &Settings) -> Settings {
        settings_from_value(&settings_to_value(s)).unwrap()
    }

    #[test]
    fn default_round_trips() {
        let s = Settings::default();
        assert_eq!(round_trip(&s), s);
    }

    #[test]
    fn mutated_fields_round_trip() {
        let s = Settings {
            track_current_page: false,
            quick_open_thumbnail_size: 256,
            mouse_wheel_speed: 3.5,
            magnify_size: (320, 240),
            right_to_left_reading_mode: RightToLeftReadingMode::FlipPages,
            last_library_item: CrGuid::parse("01234567-89ab-cdef-0123-456789abcdef").unwrap(),
            last_open_files: vec!["/a.cbz".into(), "/b.cbz".into()],
            favorite_folders: vec!["/comics".into()],
            selected_browser: Some(String::new()), // empty string, not None
            plugins_states: None,                  // None omits the key
            ..Settings::default()
        };
        let value = settings_to_value(&s);
        assert_eq!(round_trip(&s), s);
        let text = toml::to_string_pretty(&value).unwrap();
        assert!(!text.contains("PluginsStates"));
        assert!(text.contains("SelectedBrowser"));
        assert!(text.contains("LastLibraryItem"));
        assert!(text.contains("MagnifySize"));
    }

    #[test]
    fn unknown_keys_are_skipped() {
        // A hand-edited or forward-version file may carry keys this
        // build does not know — they are skipped, the known ones
        // land.
        let mut doc = toml::Table::new();
        doc.insert("TrackCurrentPage".into(), toml::Value::Boolean(false));
        doc.insert("QuickOpenThumbnailSize".into(), toml::Value::Integer(256));
        doc.insert("SomeFutureMember".into(), toml::Value::String("x".into()));
        let s = <Settings as serde::Deserialize>::deserialize(toml::Value::Table(doc)).unwrap();
        assert!(!s.track_current_page);
        assert_eq!(s.quick_open_thumbnail_size, 256);
    }

    #[test]
    fn current_workspace_round_trips() {
        let mut s = Settings::default();
        let mut ws = crate::settings::workspace::WorkspaceState::default();
        ws.view.mode = crate::model::enums::ItemViewMode::Detail;
        ws.view.sort_key = Some("ShadowSeries".to_string());
        ws.reader.rtl = true;
        ws.display.transition = "TopDown".to_string();
        s.current_workspace = Some(ws.clone());
        let value = settings_to_value(&s);
        assert_eq!(round_trip(&s), s);
        assert_eq!(settings_to_value(&round_trip(&s)), value);
    }

    #[test]
    fn the_options_surface_matches_the_c_panel_filter() {
        // The FillPanelWithOptions filter: browsable bools with a
        // non-empty description. The known categories appear.
        let mut cats: Vec<&str> = SETTINGS_FIELDS
            .iter()
            .filter(|f| f.is_options_checkbox())
            .map(|f| f.category)
            .collect();
        cats.sort();
        cats.dedup();
        for expected in [
            "Starting ComicRack",
            "Opening a Book",
            "Reading",
            "Right to Left",
            "Browser",
            "Application",
        ] {
            assert!(cats.contains(&expected), "missing {expected}");
        }
        // The reader-gating fields carry the right defaults.
        assert!(Settings::default().track_current_page);
        assert!(!Settings::default().add_to_library_on_open);
    }
}
