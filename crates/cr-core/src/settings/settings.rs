//! The [`Settings`] port (`ComicRack/Config/Settings.cs`) — the main
//! user settings object, persisted as `Config.xml` in the config tree
//! (ADR-023; the C# path is `%APPDATA%\...\Config.xml` via
//! `XmlUtility.Store` = plain `XmlSerializer.Serialize`).
//!
//! Scope: every SCALAR field plus the string lists the app keeps are
//! ported, in the C# property declaration order (the XmlSerializer
//! output order). The complex members (ListConfigurations,
//! CurrentWorkspace, ExternalPrograms, devices, remote shares, export
//! presets, VirtualTags, BackupManager) belong to later phases and are
//! not written; reads skip unknown elements, so a Windows Config.xml
//! loads with those members dropped (documented tolerance).
//!
//! The C# `[DefaultValue]` attributes on Settings are designer
//! metadata — the effective defaults are the FIELD INITIALIZERS, and
//! those are what `Default` reproduces.

use super::enums::{
    HiddenMessageBoxes, ImageDisplayOptions, LibraryGauges, MagnifierStyle, RightToLeftReadingMode,
    TabLayouts,
};
use crate::model::enums::ComicPageType;
use crate::xml::scalar::{net_f32, CrGuid};
use crate::xml::{Emitter, XmlReader};
use std::io::Write;

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

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub run_count: i32,
    pub paste_properties: String,
    pub selected_browser: Option<String>,
    pub page_filter: ComicPageType,
    pub last_explorer_folder: String,
    pub explorer_include_sub_folders: bool,
    pub last_library_item: CrGuid,
    pub last_open_filter_index: i32,
    pub last_save_filter_index: i32,
    pub last_export_page_filter_index: i32,
    pub plugins_states: Option<String>,
    pub move_files_to_recycle_bin: bool,
    pub also_remove_from_library: bool,
    pub also_remove_from_library_filtered: bool,
    /// `Settings.FavoriteFolders` (the Files browser's favorites).
    pub favorite_folders: Vec<String>,
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
    pub magnify_opaque: f32,
    pub magnify_zoom: f32,
    pub magnify_style: MagnifierStyle,
    pub auto_magnifier: bool,
    pub hardware_acceleration: bool,
    pub display_change_animation: bool,
    pub flowing_mouse_scrolling: bool,
    pub software_filtering: bool,
    pub hardware_filtering: bool,
    pub mouse_wheel_speed: f32,
    pub reader_keyboard_mapping: Vec<(String, String)>,
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
    pub information_cover3d: bool,
    pub display_library_gauges: bool,
    pub library_gauges_format: LibraryGauges,
    pub new_books_checked: bool,
    pub thumb_cache_enabled: bool,
    /// PORT ADDITION (no C# counterpart): when false the grid loads
    /// only already-cached covers; the File ▸ Generate Cover
    /// Thumbnails command backfills the cache instead.
    pub generate_thumbnails_on_demand: bool,
    pub thumb_cache_size_mb: i32,
    pub page_cache_enabled: bool,
    pub page_cache_size_mb: i32,
    pub internet_cache_enabled: bool,
    pub internet_cache_size_mb: i32,
    pub memory_thumb_cache_size_mb: i32,
    pub memory_page_cache_count: i32,
    pub memory_thumb_cache_optimized: bool,
    pub memory_page_cache_optimized: bool,
    pub maximum_memory_mb: i32,
    pub remove_missing_files_on_full_scan: bool,
    pub dont_add_remove_files: bool,
    pub overwrite_associations: bool,
    pub culture_name: Option<String>,
    pub exported_lists_contain_filenames: bool,
    pub quick_search_list: Vec<String>,
    pub library_quick_search_list: Vec<String>,
    pub open_remote_filter: Option<String>,
    pub open_remote_password: Option<String>,
    pub auto_show_quick_review: bool,
    /// `Settings.CurrentWorkspace` (the T14 persisted layout; None =
    /// never saved — the defaults apply).
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

impl Settings {
    /// The `XmlUtility.Store` output: the net48 `XmlSerializer`
    /// element stream in property declaration order. Null strings
    /// omit the element; empty strings write self-closing elements.
    pub fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.root("Settings")?;
        w_int(e, "RunCount", self.run_count)?;
        w_str(e, "PasteProperties", &self.paste_properties)?;
        w_opt(e, "SelectedBrowser", &self.selected_browser)?;
        w_enum(e, "PageFilter", self.page_filter.to_xml())?;
        w_str(e, "LastExplorerFolder", &self.last_explorer_folder)?;
        w_bool(
            e,
            "ExplorerIncludeSubFolders",
            self.explorer_include_sub_folders,
        )?;
        w_str(e, "LastLibraryItem", &self.last_library_item.to_d_string())?;
        w_int(e, "LastOpenFilterIndex", self.last_open_filter_index)?;
        w_int(e, "LastSaveFilterIndex", self.last_save_filter_index)?;
        w_int(
            e,
            "LastExportPageFilterIndex",
            self.last_export_page_filter_index,
        )?;
        w_opt(e, "PluginsStates", &self.plugins_states)?;
        w_bool(e, "MoveFilesToRecycleBin", self.move_files_to_recycle_bin)?;
        w_bool(e, "AlsoRemoveFromLibrary", self.also_remove_from_library)?;
        w_bool(
            e,
            "AlsoRemoveFromLibraryFiltered",
            self.also_remove_from_library_filtered,
        )?;
        w_strings(e, "FavoriteFolders", &self.favorite_folders)?;
        w_bool(
            e,
            "RemoveFilesfromDatabase",
            self.remove_files_from_database,
        )?;
        w_enum(e, "TabLayouts", self.tab_layouts.to_xml())?;
        w_int(e, "QuickOpenThumbnailSize", self.quick_open_thumbnail_size)?;
        w_str(e, "ExternalServerAddress", &self.external_server_address)?;
        w_str(e, "PrivateListingPassword", &self.private_listing_password)?;
        w_bool(e, "LookForShared", self.look_for_shared)?;
        w_bool(e, "AutoConnectShares", self.auto_connect_shares)?;
        w_str(
            e,
            "ExtraWifiDeviceAddresses",
            &self.extra_wifi_device_addresses,
        )?;
        w_enum(
            e,
            "PageImageDisplayOptions",
            self.page_image_display_options.to_xml(),
        )?;
        w_int(e, "OverlayScaling", self.overlay_scaling)?;
        w_size(e, "MagnifySize", self.magnify_size)?;
        w_f32(e, "MagnifyOpaque", self.magnify_opaque)?;
        w_f32(e, "MagnifyZoom", self.magnify_zoom)?;
        w_enum(e, "MagnifyStyle", self.magnify_style.to_xml())?;
        w_bool(e, "AutoMagnifier", self.auto_magnifier)?;
        w_bool(e, "HardwareAcceleration", self.hardware_acceleration)?;
        w_bool(e, "DisplayChangeAnimation", self.display_change_animation)?;
        w_bool(e, "FlowingMouseScrolling", self.flowing_mouse_scrolling)?;
        w_bool(e, "SoftwareFiltering", self.software_filtering)?;
        w_bool(e, "HardwareFiltering", self.hardware_filtering)?;
        w_f32(e, "MouseWheelSpeed", self.mouse_wheel_speed)?;
        w_string_pairs(e, "ReaderKeyboardMapping", &self.reader_keyboard_mapping)?;
        w_opt(e, "IgnoredCoverImages", &self.ignored_cover_images)?;
        w_bool(e, "AutoScrolling", self.auto_scrolling)?;
        w_enum(e, "HiddenMessageBoxes", self.hidden_message_boxes.to_xml())?;
        w_bool(e, "UpdateComicFiles", self.update_comic_files)?;
        w_bool(e, "UpdateComicBookFiles", self.update_comic_book_files)?;
        w_bool(e, "AutoUpdateComicsFiles", self.auto_update_comics_files)?;
        w_str(e, "HelpSystem", &self.help_system)?;
        w_bool(e, "Scripting", self.scripting)?;
        w_str(e, "ScriptingLibraries", &self.scripting_libraries)?;
        w_bool(e, "HideSampleScripts", self.hide_sample_scripts)?;
        w_bool(e, "ShowSplash", self.show_splash)?;
        w_bool(e, "OpenLastFile", self.open_last_file)?;
        w_bool(e, "ScanStartup", self.scan_startup)?;
        w_bool(e, "UpdateWebComicsStartup", self.update_web_comics_startup)?;
        w_bool(e, "NewsStartup", self.news_startup)?;
        w_strings(e, "LastOpenFiles", &self.last_open_files)?;
        w_bool(e, "ShowQuickManual", self.show_quick_manual)?;
        w_bool(e, "OpenLastPage", self.open_last_page)?;
        w_bool(e, "CloseBrowserOnOpen", self.close_browser_on_open)?;
        w_bool(e, "AddToLibraryOnOpen", self.add_to_library_on_open)?;
        w_bool(e, "OpenInNewTab", self.open_in_new_tab)?;
        w_bool(e, "HideCursorFullScreen", self.hide_cursor_full_screen)?;
        w_bool(e, "AutoNavigateComics", self.auto_navigate_comics)?;
        w_bool(e, "ShowCurrentPageOverlay", self.show_current_page_overlay)?;
        w_bool(
            e,
            "ShowVisiblePagePartOverlay",
            self.show_visible_page_part_overlay,
        )?;
        w_bool(e, "ShowStatusOverlay", self.show_status_overlay)?;
        w_bool(e, "ShowNavigationOverlay", self.show_navigation_overlay)?;
        w_bool(e, "NavigationOverlayOnTop", self.navigation_overlay_on_top)?;
        w_bool(e, "CurrentPageShowsName", self.current_page_shows_name)?;
        w_bool(e, "AutoHideMagnifier", self.auto_hide_magnifier)?;
        w_bool(e, "PageChangeDelay", self.page_change_delay)?;
        w_bool(e, "ScrollingDoesBrowse", self.scrolling_does_browse)?;
        w_bool(e, "ResetZoomOnPageChange", self.reset_zoom_on_page_change)?;
        w_bool(e, "ZoomInOutOnPageChange", self.zoom_in_out_on_page_change)?;
        w_bool(e, "SmoothScrolling", self.smooth_scrolling)?;
        w_bool(e, "BlendWhilePaging", self.blend_while_paging)?;
        w_bool(e, "TrackCurrentPage", self.track_current_page)?;
        w_enum(
            e,
            "RightToLeftReadingMode",
            self.right_to_left_reading_mode.to_xml(),
        )?;
        w_bool(e, "TrueRightToLeftReading", self.true_right_to_left_reading)?;
        w_bool(
            e,
            "LeftRightMovementReversed",
            self.left_right_movement_reversed,
        )?;
        w_bool(e, "ShowToolTips", self.show_tool_tips)?;
        w_bool(e, "ShowSearchLinks", self.show_search_links)?;
        w_bool(e, "FadeInThumbnails", self.fade_in_thumbnails)?;
        w_bool(e, "DogEarThumbnails", self.dog_ear_thumbnails)?;
        w_bool(e, "NumericRatingThumbnails", self.numeric_rating_thumbnails)?;
        w_bool(e, "LocalQuickSearch", self.local_quick_search)?;
        w_bool(
            e,
            "CoverThumbnailsSameSize",
            self.cover_thumbnails_same_size,
        )?;
        w_bool(e, "CommonListStackLayout", self.common_list_stack_layout)?;
        w_bool(e, "ShowQuickOpen", self.show_quick_open)?;
        w_bool(e, "CatalogOnlyForFileless", self.catalog_only_for_fileless)?;
        w_bool(e, "ShowCustomBookFields", self.show_custom_book_fields)?;
        w_bool(e, "MinimizeToTray", self.minimize_to_tray)?;
        w_bool(e, "CloseMinimizesToTray", self.close_minimizes_to_tray)?;
        w_bool(e, "AutoMinimalGui", self.auto_minimal_gui)?;
        w_bool(e, "AnimatePanels", self.animate_panels)?;
        w_bool(
            e,
            "AlwaysDisplayBrowserDockingGrip",
            self.always_display_browser_docking_grip,
        )?;
        w_bool(e, "DisableDragDrop", self.disable_drag_drop)?;
        w_bool(e, "AutoHideMainMenu", self.auto_hide_main_menu)?;
        w_bool(
            e,
            "ShowMainMenuNoComicOpen",
            self.show_main_menu_no_comic_open,
        )?;
        w_bool(e, "InformationCover3D", self.information_cover3d)?;
        w_bool(e, "DisplayLibraryGauges", self.display_library_gauges)?;
        w_enum(
            e,
            "LibraryGaugesFormat",
            self.library_gauges_format.to_xml(),
        )?;
        w_bool(e, "NewBooksChecked", self.new_books_checked)?;
        w_bool(e, "ThumbCacheEnabled", self.thumb_cache_enabled)?;
        // PORT ADDITION — see the field comment.
        w_bool(
            e,
            "GenerateThumbnailsOnDemand",
            self.generate_thumbnails_on_demand,
        )?;
        w_int(e, "ThumbCacheSizeMB", self.thumb_cache_size_mb)?;
        w_bool(e, "PageCacheEnabled", self.page_cache_enabled)?;
        w_int(e, "PageCacheSizeMB", self.page_cache_size_mb)?;
        w_bool(e, "InternetCacheEnabled", self.internet_cache_enabled)?;
        w_int(e, "InternetCacheSizeMB", self.internet_cache_size_mb)?;
        w_int(e, "MemoryThumbCacheSizeMB", self.memory_thumb_cache_size_mb)?;
        w_int(e, "MemoryPageCacheCount", self.memory_page_cache_count)?;
        w_bool(
            e,
            "MemoryThumbCacheOptimized",
            self.memory_thumb_cache_optimized,
        )?;
        w_bool(
            e,
            "MemoryPageCacheOptimized",
            self.memory_page_cache_optimized,
        )?;
        w_int(e, "MaximumMemoryMB", self.maximum_memory_mb)?;
        w_bool(
            e,
            "RemoveMissingFilesOnFullScan",
            self.remove_missing_files_on_full_scan,
        )?;
        w_bool(e, "DontAddRemoveFiles", self.dont_add_remove_files)?;
        w_bool(e, "OverwriteAssociations", self.overwrite_associations)?;
        w_opt(e, "CultureName", &self.culture_name)?;
        w_bool(
            e,
            "ExportedListsContainFilenames",
            self.exported_lists_contain_filenames,
        )?;
        w_strings(e, "QuickSearchList", &self.quick_search_list)?;
        w_strings(e, "LibraryQuickSearchList", &self.library_quick_search_list)?;
        w_opt(e, "OpenRemoteFilter", &self.open_remote_filter)?;
        w_opt(e, "OpenRemotePassword", &self.open_remote_password)?;
        w_bool(e, "AutoShowQuickReview", self.auto_show_quick_review)?;
        // The persisted workspace rides last (the reader is
        // order-tolerant; the C# writes it at its own property
        // position).
        if let Some(ws) = &self.current_workspace {
            ws.write_xml(e)?;
        }
        e.end()
    }
}

type IoResult<T> = std::io::Result<T>;

fn w_bool<W: Write>(e: &mut Emitter<W>, name: &str, v: bool) -> IoResult<()> {
    e.text_elem(name, if v { "true" } else { "false" })
}

fn w_int<W: Write>(e: &mut Emitter<W>, name: &str, v: i32) -> IoResult<()> {
    e.text_elem(name, &v.to_string())
}

fn w_f32<W: Write>(e: &mut Emitter<W>, name: &str, v: f32) -> IoResult<()> {
    e.text_elem(name, &net_f32(v))
}

fn w_str<W: Write>(e: &mut Emitter<W>, name: &str, v: &str) -> IoResult<()> {
    e.text_elem(name, v)
}

fn w_opt<W: Write>(e: &mut Emitter<W>, name: &str, v: &Option<String>) -> IoResult<()> {
    match v {
        Some(t) => e.text_elem(name, t),
        None => Ok(()), // a null string omits the element
    }
}

fn w_enum<W: Write>(e: &mut Emitter<W>, name: &str, text: String) -> IoResult<()> {
    e.text_elem(name, &text)
}

fn w_size<W: Write>(e: &mut Emitter<W>, name: &str, (w, h): (i32, i32)) -> IoResult<()> {
    e.start(name)?;
    e.text_elem("Width", &w.to_string())?;
    e.text_elem("Height", &h.to_string())?;
    e.end()
}

fn w_strings<W: Write>(e: &mut Emitter<W>, name: &str, list: &[String]) -> IoResult<()> {
    e.start(name)?;
    for s in list {
        e.text_elem("string", s)?;
    }
    e.end()
}

fn w_string_pairs<W: Write>(
    e: &mut Emitter<W>,
    name: &str,
    list: &[(String, String)],
) -> IoResult<()> {
    // `StringPair` serializes as `<StringPair Key="..." Value="..." />`
    // (the ValuePair members are XmlAttributes); an empty list emits
    // the self-closing container.
    e.start(name)?;
    for (a, b) in list {
        e.start("StringPair")?;
        e.attr("Key", a)?;
        e.attr("Value", b)?;
        e.end()?;
    }
    e.end()
}

// ---------- Reading (Config.xml load) ----------

use crate::xml::{Start, Tok, XmlError, XmlResult};

fn r_bool(r: &mut XmlReader<'_>, name: &str) -> XmlResult<bool> {
    Ok(r.text_content(name)? == "true")
}

fn r_i32(r: &mut XmlReader<'_>, name: &str) -> XmlResult<i32> {
    let t = r.text_content(name)?;
    Ok(t.trim().parse().unwrap_or(0))
}

fn r_f32(r: &mut XmlReader<'_>, name: &str) -> XmlResult<f32> {
    let t = r.text_content(name)?;
    Ok(t.trim().parse().unwrap_or(0.0))
}

fn r_opt(r: &mut XmlReader<'_>, name: &str) -> XmlResult<Option<String>> {
    Ok(Some(r.text_content(name)?))
}

fn r_size(r: &mut XmlReader<'_>, name: &str) -> XmlResult<(i32, i32)> {
    let (mut w, mut h) = (0, 0);
    loop {
        match r.next_tok()? {
            Tok::End(n) if n == name => return Ok((w, h)),
            Tok::Start(s) => match s.name.as_str() {
                "Width" => w = r_i32(r, "Width")?,
                "Height" => h = r_i32(r, "Height")?,
                _ => r.skip_element(&s.name)?,
            },
            Tok::Eof => return Err(XmlError("unexpected eof in size".into())),
            _ => {}
        }
    }
}

fn r_strings(r: &mut XmlReader<'_>, name: &str) -> XmlResult<Vec<String>> {
    let mut out = Vec::new();
    loop {
        match r.next_tok()? {
            Tok::End(n) if n == name => return Ok(out),
            Tok::Start(s) if s.name == "string" => out.push(r.text_content("string")?),
            Tok::Start(s) => r.skip_element(&s.name)?,
            Tok::Eof => return Err(XmlError("unexpected eof in string list".into())),
            _ => {}
        }
    }
}

fn r_string_pairs(r: &mut XmlReader<'_>, name: &str) -> XmlResult<Vec<(String, String)>> {
    let mut out = Vec::new();
    loop {
        match r.next_tok()? {
            Tok::End(n) if n == name => return Ok(out),
            Tok::Start(s) if s.name == "StringPair" => {
                let key = s.attr("Key").unwrap_or("").to_string();
                let value = s.attr("Value").unwrap_or("").to_string();
                out.push((key, value));
                r.skip_element("StringPair")?;
            }
            Tok::Start(s) => r.skip_element(&s.name)?,
            Tok::Eof => return Err(XmlError("unexpected eof in pair list".into())),
            _ => {}
        }
    }
}

/// Reads one Settings child element (already started). Returns true
/// when handled; the caller skips the rest (a Windows Config.xml may
/// carry members this port does not keep — they drop, ADR-023 note).
fn read_elem(r: &mut XmlReader<'_>, s: &Start, x: &mut Settings) -> XmlResult<bool> {
    match s.name.as_str() {
        "RunCount" => x.run_count = r_i32(r, "RunCount")?,
        "PasteProperties" => x.paste_properties = r.text_content("PasteProperties")?,
        "SelectedBrowser" => x.selected_browser = r_opt(r, "SelectedBrowser")?,
        "PageFilter" => {
            x.page_filter = ComicPageType::from_xml(&r.text_content("PageFilter")?)
                .unwrap_or(ComicPageType(1023))
        }
        "LastExplorerFolder" => x.last_explorer_folder = r.text_content("LastExplorerFolder")?,
        "ExplorerIncludeSubFolders" => {
            x.explorer_include_sub_folders = r_bool(r, "ExplorerIncludeSubFolders")?
        }
        "LastLibraryItem" => {
            x.last_library_item =
                CrGuid::parse(&r.text_content("LastLibraryItem")?).unwrap_or(CrGuid::EMPTY)
        }
        "LastOpenFilterIndex" => x.last_open_filter_index = r_i32(r, "LastOpenFilterIndex")?,
        "LastSaveFilterIndex" => x.last_save_filter_index = r_i32(r, "LastSaveFilterIndex")?,
        "LastExportPageFilterIndex" => {
            x.last_export_page_filter_index = r_i32(r, "LastExportPageFilterIndex")?
        }
        "PluginsStates" => x.plugins_states = r_opt(r, "PluginsStates")?,
        "MoveFilesToRecycleBin" => {
            x.move_files_to_recycle_bin = r_bool(r, "MoveFilesToRecycleBin")?
        }
        "AlsoRemoveFromLibrary" => x.also_remove_from_library = r_bool(r, "AlsoRemoveFromLibrary")?,
        "AlsoRemoveFromLibraryFiltered" => {
            x.also_remove_from_library_filtered = r_bool(r, "AlsoRemoveFromLibraryFiltered")?
        }
        "FavoriteFolders" => x.favorite_folders = r_strings(r, "FavoriteFolders")?,
        "RemoveFilesfromDatabase" => {
            x.remove_files_from_database = r_bool(r, "RemoveFilesfromDatabase")?
        }
        "TabLayouts" => {
            x.tab_layouts = TabLayouts::from_xml(&r.text_content("TabLayouts")?).unwrap_or_default()
        }
        "QuickOpenThumbnailSize" => {
            x.quick_open_thumbnail_size = r_i32(r, "QuickOpenThumbnailSize")?
        }
        "ExternalServerAddress" => {
            x.external_server_address = r.text_content("ExternalServerAddress")?
        }
        "PrivateListingPassword" => {
            x.private_listing_password = r.text_content("PrivateListingPassword")?
        }
        "LookForShared" => x.look_for_shared = r_bool(r, "LookForShared")?,
        "AutoConnectShares" => x.auto_connect_shares = r_bool(r, "AutoConnectShares")?,
        "ExtraWifiDeviceAddresses" => {
            x.extra_wifi_device_addresses = r.text_content("ExtraWifiDeviceAddresses")?
        }
        "PageImageDisplayOptions" => {
            x.page_image_display_options =
                ImageDisplayOptions::from_xml(&r.text_content("PageImageDisplayOptions")?)
                    .unwrap_or_default()
        }
        "OverlayScaling" => x.overlay_scaling = r_i32(r, "OverlayScaling")?,
        "MagnifySize" => x.magnify_size = r_size(r, "MagnifySize")?,
        "MagnifyOpaque" => x.magnify_opaque = r_f32(r, "MagnifyOpaque")?,
        "MagnifyZoom" => x.magnify_zoom = r_f32(r, "MagnifyZoom")?,
        "MagnifyStyle" => {
            x.magnify_style =
                MagnifierStyle::from_xml(&r.text_content("MagnifyStyle")?).unwrap_or_default()
        }
        "AutoMagnifier" => x.auto_magnifier = r_bool(r, "AutoMagnifier")?,
        "HardwareAcceleration" => x.hardware_acceleration = r_bool(r, "HardwareAcceleration")?,
        "DisplayChangeAnimation" => {
            x.display_change_animation = r_bool(r, "DisplayChangeAnimation")?
        }
        "FlowingMouseScrolling" => x.flowing_mouse_scrolling = r_bool(r, "FlowingMouseScrolling")?,
        "SoftwareFiltering" => x.software_filtering = r_bool(r, "SoftwareFiltering")?,
        "HardwareFiltering" => x.hardware_filtering = r_bool(r, "HardwareFiltering")?,
        "MouseWheelSpeed" => x.mouse_wheel_speed = r_f32(r, "MouseWheelSpeed")?,
        "ReaderKeyboardMapping" => {
            x.reader_keyboard_mapping = r_string_pairs(r, "ReaderKeyboardMapping")?
        }
        "IgnoredCoverImages" => x.ignored_cover_images = r_opt(r, "IgnoredCoverImages")?,
        "AutoScrolling" => x.auto_scrolling = r_bool(r, "AutoScrolling")?,
        "HiddenMessageBoxes" => {
            x.hidden_message_boxes =
                HiddenMessageBoxes::from_xml(&r.text_content("HiddenMessageBoxes")?)
                    .unwrap_or_default()
        }
        "UpdateComicFiles" => x.update_comic_files = r_bool(r, "UpdateComicFiles")?,
        "UpdateComicBookFiles" => x.update_comic_book_files = r_bool(r, "UpdateComicBookFiles")?,
        "AutoUpdateComicsFiles" => x.auto_update_comics_files = r_bool(r, "AutoUpdateComicsFiles")?,
        "HelpSystem" => x.help_system = r.text_content("HelpSystem")?,
        "Scripting" => x.scripting = r_bool(r, "Scripting")?,
        "ScriptingLibraries" => x.scripting_libraries = r.text_content("ScriptingLibraries")?,
        "HideSampleScripts" => x.hide_sample_scripts = r_bool(r, "HideSampleScripts")?,
        "ShowSplash" => x.show_splash = r_bool(r, "ShowSplash")?,
        "OpenLastFile" => x.open_last_file = r_bool(r, "OpenLastFile")?,
        "ScanStartup" => x.scan_startup = r_bool(r, "ScanStartup")?,
        "UpdateWebComicsStartup" => {
            x.update_web_comics_startup = r_bool(r, "UpdateWebComicsStartup")?
        }
        "NewsStartup" => x.news_startup = r_bool(r, "NewsStartup")?,
        "LastOpenFiles" => x.last_open_files = r_strings(r, "LastOpenFiles")?,
        "ShowQuickManual" => x.show_quick_manual = r_bool(r, "ShowQuickManual")?,
        "OpenLastPage" => x.open_last_page = r_bool(r, "OpenLastPage")?,
        "CloseBrowserOnOpen" => x.close_browser_on_open = r_bool(r, "CloseBrowserOnOpen")?,
        "AddToLibraryOnOpen" => x.add_to_library_on_open = r_bool(r, "AddToLibraryOnOpen")?,
        "OpenInNewTab" => x.open_in_new_tab = r_bool(r, "OpenInNewTab")?,
        "HideCursorFullScreen" => x.hide_cursor_full_screen = r_bool(r, "HideCursorFullScreen")?,
        "AutoNavigateComics" => x.auto_navigate_comics = r_bool(r, "AutoNavigateComics")?,
        "ShowCurrentPageOverlay" => {
            x.show_current_page_overlay = r_bool(r, "ShowCurrentPageOverlay")?
        }
        "ShowVisiblePagePartOverlay" => {
            x.show_visible_page_part_overlay = r_bool(r, "ShowVisiblePagePartOverlay")?
        }
        "ShowStatusOverlay" => x.show_status_overlay = r_bool(r, "ShowStatusOverlay")?,
        "ShowNavigationOverlay" => x.show_navigation_overlay = r_bool(r, "ShowNavigationOverlay")?,
        "NavigationOverlayOnTop" => {
            x.navigation_overlay_on_top = r_bool(r, "NavigationOverlayOnTop")?
        }
        "CurrentPageShowsName" => x.current_page_shows_name = r_bool(r, "CurrentPageShowsName")?,
        "AutoHideMagnifier" => x.auto_hide_magnifier = r_bool(r, "AutoHideMagnifier")?,
        "PageChangeDelay" => x.page_change_delay = r_bool(r, "PageChangeDelay")?,
        "ScrollingDoesBrowse" => x.scrolling_does_browse = r_bool(r, "ScrollingDoesBrowse")?,
        "ResetZoomOnPageChange" => {
            x.reset_zoom_on_page_change = r_bool(r, "ResetZoomOnPageChange")?
        }
        "ZoomInOutOnPageChange" => {
            x.zoom_in_out_on_page_change = r_bool(r, "ZoomInOutOnPageChange")?
        }
        "SmoothScrolling" => x.smooth_scrolling = r_bool(r, "SmoothScrolling")?,
        "BlendWhilePaging" => x.blend_while_paging = r_bool(r, "BlendWhilePaging")?,
        "TrackCurrentPage" => x.track_current_page = r_bool(r, "TrackCurrentPage")?,
        "RightToLeftReadingMode" => {
            x.right_to_left_reading_mode =
                RightToLeftReadingMode::from_xml(&r.text_content("RightToLeftReadingMode")?)
                    .unwrap_or_default()
        }
        "TrueRightToLeftReading" => {
            x.true_right_to_left_reading = r_bool(r, "TrueRightToLeftReading")?
        }
        "LeftRightMovementReversed" => {
            x.left_right_movement_reversed = r_bool(r, "LeftRightMovementReversed")?
        }
        "ShowToolTips" => x.show_tool_tips = r_bool(r, "ShowToolTips")?,
        "ShowSearchLinks" => x.show_search_links = r_bool(r, "ShowSearchLinks")?,
        "FadeInThumbnails" => x.fade_in_thumbnails = r_bool(r, "FadeInThumbnails")?,
        "DogEarThumbnails" => x.dog_ear_thumbnails = r_bool(r, "DogEarThumbnails")?,
        "NumericRatingThumbnails" => {
            x.numeric_rating_thumbnails = r_bool(r, "NumericRatingThumbnails")?
        }
        "LocalQuickSearch" => x.local_quick_search = r_bool(r, "LocalQuickSearch")?,
        "CoverThumbnailsSameSize" => {
            x.cover_thumbnails_same_size = r_bool(r, "CoverThumbnailsSameSize")?
        }
        "CommonListStackLayout" => x.common_list_stack_layout = r_bool(r, "CommonListStackLayout")?,
        "ShowQuickOpen" => x.show_quick_open = r_bool(r, "ShowQuickOpen")?,
        "CatalogOnlyForFileless" => {
            x.catalog_only_for_fileless = r_bool(r, "CatalogOnlyForFileless")?
        }
        "ShowCustomBookFields" => x.show_custom_book_fields = r_bool(r, "ShowCustomBookFields")?,
        "MinimizeToTray" => x.minimize_to_tray = r_bool(r, "MinimizeToTray")?,
        "CloseMinimizesToTray" => x.close_minimizes_to_tray = r_bool(r, "CloseMinimizesToTray")?,
        "AutoMinimalGui" => x.auto_minimal_gui = r_bool(r, "AutoMinimalGui")?,
        "AnimatePanels" => x.animate_panels = r_bool(r, "AnimatePanels")?,
        "AlwaysDisplayBrowserDockingGrip" => {
            x.always_display_browser_docking_grip = r_bool(r, "AlwaysDisplayBrowserDockingGrip")?
        }
        "DisableDragDrop" => x.disable_drag_drop = r_bool(r, "DisableDragDrop")?,
        "AutoHideMainMenu" => x.auto_hide_main_menu = r_bool(r, "AutoHideMainMenu")?,
        "ShowMainMenuNoComicOpen" => {
            x.show_main_menu_no_comic_open = r_bool(r, "ShowMainMenuNoComicOpen")?
        }
        "InformationCover3D" => x.information_cover3d = r_bool(r, "InformationCover3D")?,
        "DisplayLibraryGauges" => x.display_library_gauges = r_bool(r, "DisplayLibraryGauges")?,
        "LibraryGaugesFormat" => {
            x.library_gauges_format =
                LibraryGauges::from_xml(&r.text_content("LibraryGaugesFormat")?).unwrap_or_default()
        }
        "NewBooksChecked" => x.new_books_checked = r_bool(r, "NewBooksChecked")?,
        "ThumbCacheEnabled" => x.thumb_cache_enabled = r_bool(r, "ThumbCacheEnabled")?,
        // PORT ADDITION — see the field comment.
        "GenerateThumbnailsOnDemand" => {
            x.generate_thumbnails_on_demand = r_bool(r, "GenerateThumbnailsOnDemand")?
        }
        "ThumbCacheSizeMB" => x.thumb_cache_size_mb = r_i32(r, "ThumbCacheSizeMB")?,
        "PageCacheEnabled" => x.page_cache_enabled = r_bool(r, "PageCacheEnabled")?,
        "PageCacheSizeMB" => x.page_cache_size_mb = r_i32(r, "PageCacheSizeMB")?,
        "InternetCacheEnabled" => x.internet_cache_enabled = r_bool(r, "InternetCacheEnabled")?,
        "InternetCacheSizeMB" => x.internet_cache_size_mb = r_i32(r, "InternetCacheSizeMB")?,
        "MemoryThumbCacheSizeMB" => {
            x.memory_thumb_cache_size_mb = r_i32(r, "MemoryThumbCacheSizeMB")?
        }
        "MemoryPageCacheCount" => x.memory_page_cache_count = r_i32(r, "MemoryPageCacheCount")?,
        "MemoryThumbCacheOptimized" => {
            x.memory_thumb_cache_optimized = r_bool(r, "MemoryThumbCacheOptimized")?
        }
        "MemoryPageCacheOptimized" => {
            x.memory_page_cache_optimized = r_bool(r, "MemoryPageCacheOptimized")?
        }
        "MaximumMemoryMB" => x.maximum_memory_mb = r_i32(r, "MaximumMemoryMB")?,
        "RemoveMissingFilesOnFullScan" => {
            x.remove_missing_files_on_full_scan = r_bool(r, "RemoveMissingFilesOnFullScan")?
        }
        "DontAddRemoveFiles" => x.dont_add_remove_files = r_bool(r, "DontAddRemoveFiles")?,
        "OverwriteAssociations" => x.overwrite_associations = r_bool(r, "OverwriteAssociations")?,
        "CultureName" => x.culture_name = r_opt(r, "CultureName")?,
        "ExportedListsContainFilenames" => {
            x.exported_lists_contain_filenames = r_bool(r, "ExportedListsContainFilenames")?
        }
        "QuickSearchList" => x.quick_search_list = r_strings(r, "QuickSearchList")?,
        "LibraryQuickSearchList" => {
            x.library_quick_search_list = r_strings(r, "LibraryQuickSearchList")?
        }
        "OpenRemoteFilter" => x.open_remote_filter = r_opt(r, "OpenRemoteFilter")?,
        "OpenRemotePassword" => x.open_remote_password = r_opt(r, "OpenRemotePassword")?,
        "AutoShowQuickReview" => x.auto_show_quick_review = r_bool(r, "AutoShowQuickReview")?,
        "CurrentWorkspace" => {
            x.current_workspace = Some(super::workspace::WorkspaceState::parse(r)?)
        }
        _ => return Ok(false),
    }
    Ok(true)
}

impl Settings {
    /// `Settings.Load(file)`: any failure yields the defaults (the
    /// C# catch returns `new Settings()`).
    pub fn load(file: &std::path::Path) -> Settings {
        Self::read_file(file).unwrap_or_default()
    }

    pub fn read_file(file: &std::path::Path) -> XmlResult<Settings> {
        let text = std::fs::read(file).map_err(|e| XmlError(format!("read: {e}")))?;
        let mut cursor = std::io::Cursor::new(text);
        let mut reader = XmlReader::new(&mut cursor);
        Self::parse_root(&mut reader)
    }

    pub fn parse_root(reader: &mut XmlReader<'_>) -> XmlResult<Settings> {
        let start = match reader.next_tok()? {
            Tok::Start(s) => s,
            Tok::Eof => return Err(XmlError("empty document".into())),
            _ => return Err(XmlError("unexpected token before root".into())),
        };
        if start.name != "Settings" {
            return Err(XmlError(format!("unexpected root <{}>", start.name)));
        }
        let mut x = Settings::default();
        loop {
            match reader.next_tok()? {
                Tok::Eof => return Err(XmlError("unexpected eof in Settings".into())),
                Tok::End(n) if n == "Settings" => return Ok(x),
                Tok::Start(s) if !read_elem(reader, &s, &mut x)? => {
                    reader.skip_element(&s.name)?;
                }
                _ => {}
            }
        }
    }

    /// `Settings.Save(file)` (`XmlUtility.Store` = the Emitter form).
    pub fn save(&self, file: &std::path::Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let out = std::fs::File::create(file)?;
        let mut e = Emitter::new(std::io::BufWriter::new(out))?;
        self.write_xml(&mut e)?;
        e.finish().map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_bytes(s: &Settings) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut e = Emitter::new(&mut buf).unwrap();
        s.write_xml(&mut e).unwrap();
        e.finish().unwrap();
        buf
    }

    fn from_bytes(bytes: &[u8]) -> Settings {
        let mut cursor = std::io::Cursor::new(bytes.to_vec());
        let mut reader = XmlReader::new(&mut cursor);
        Settings::parse_root(&mut reader).unwrap()
    }

    #[test]
    fn default_round_trips_byte_stable() {
        let s = Settings::default();
        let bytes = to_bytes(&s);
        let back = from_bytes(&bytes);
        assert_eq!(back, s);
        let bytes2 = to_bytes(&back);
        assert_eq!(bytes, bytes2, "the re-save is byte-identical");
    }

    #[test]
    fn xml_head_matches_the_net48_form() {
        let bytes = to_bytes(&Settings::default());
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("<?xml version=\"1.0\"?>"));
        assert!(text.contains("<Settings xmlns:xsd="));
        // The C# declaration order: RunCount first, AutoShowQuickReview last.
        let run = text.find("<RunCount>0</RunCount>").unwrap();
        let last = text
            .find("<AutoShowQuickReview>false</AutoShowQuickReview>")
            .unwrap();
        assert!(run < last);
    }

    #[test]
    fn mutated_fields_round_trip() {
        let s = Settings {
            track_current_page: false,
            quick_open_thumbnail_size: 256,
            mouse_wheel_speed: 3.5,
            magnify_size: (320, 240),
            right_to_left_reading_mode: RightToLeftReadingMode::FlipParts,
            last_library_item: CrGuid::parse("01234567-89ab-cdef-0123-456789abcdef").unwrap(),
            last_open_files: vec!["/a.cbz".into(), "/b.cbz".into()],
            favorite_folders: vec!["/comics".into()],
            selected_browser: Some(String::new()), // empty string, not null
            plugins_states: None,                  // null → the element is omitted
            ..Settings::default()
        };
        let bytes = to_bytes(&s);
        assert_eq!(from_bytes(&bytes), s);
        // A null string omits the element; an empty string self-closes.
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains("PluginsStates"));
        assert!(text.contains("<SelectedBrowser />"));
    }

    #[test]
    fn unknown_elements_are_skipped() {
        // A Windows Config.xml carries members this port drops; the
        // read must skip them and keep the known ones. The C#
        // Workspaces LIST (named presets) is one of the dropped
        // members — the port persists only the implicit
        // CurrentWorkspace.
        let xml = b"<?xml version=\"1.0\"?>\r\n<Settings xmlns:xsd=\"x\" xmlns:xsi=\"y\">\r\n  <UnknownThing><a /></UnknownThing>\r\n  <TrackCurrentPage>false</TrackCurrentPage>\r\n  <Workspaces><DisplayWorkspace><Name>w</Name></DisplayWorkspace></Workspaces>\r\n  <QuickOpenThumbnailSize>256</QuickOpenThumbnailSize>\r\n</Settings>";
        let s = from_bytes(xml);
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
        let bytes = to_bytes(&s);
        assert_eq!(from_bytes(&bytes), s);
        assert_eq!(to_bytes(&from_bytes(&bytes)), bytes);
    }

    #[test]
    fn file_save_load_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "comicrust-settings-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Config.xml");
        let s = Settings {
            show_quick_open: false,
            ..Settings::default()
        };
        s.save(&file).unwrap();
        let back = Settings::load(&file);
        assert_eq!(back, s);
        // A corrupt file falls back to the defaults (C# parity).
        std::fs::write(&file, b"<Settings><unclosed").unwrap();
        assert_eq!(Settings::load(&file), Settings::default());
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
