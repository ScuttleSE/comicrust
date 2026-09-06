//! The [`EngineConfiguration`] port (`ComicRack.Engine/EngineConfiguration.cs`).
//!
//! The C# registers this object against `IniFile.Default`
//! (`EngineConfiguration.Default`), so a `comicrust.ini` in the search
//! chain (plus command-line `-switch=value` pairs) feeds every field
//! by its C# property name. The C# clamps inside the property
//! setters; the port clamps in [`EngineConfiguration::normalize`],
//! which the loader calls after the values are applied — the
//! observable values are the same.
//!
//! The defaults are the C# CONSTRUCTOR values (the effective instance
//! state); the stale `[DefaultValue]` attributes that disagree with
//! the constructor (`BlendDuration` 250 vs 400, `ParallelConversions`
//! 4 vs 32) are recorded here for traceability.

use super::enums::{BitmapResampling, CbEngine, PdfEngine};
use super::ini::IniValues;
use super::registry::{apply_ini, EnumValue};
use crate::model::comic_book::{DEFAULT_CAPTION_FORMAT, DEFAULT_COMIC_EXPORT_FILE_NAME_FORMAT};

/// An 8-bit RGB color (the .NET `Color` subset the config carries).
pub type Rgb = (u8, u8, u8);

#[derive(Clone, Debug, PartialEq)]
pub struct EngineConfiguration {
    pub enable_parallel_queries: bool,
    pub ignored_articles: Option<String>,
    pub of_values: Option<String>,
    pub legacy_filename_parser: bool,
    pub page_scrolling_duration: i32,
    pub animation_duration: i32,
    /// ctor 400 (the `[DefaultValue(250)]` attribute is stale).
    pub blend_duration: i32,
    pub software_filter_delay: i32,
    pub list_cover_size: (i32, i32),
    pub list_cover_alpha: f32,
    pub navigation_panel_width: f32,
    pub temp_path: String,
    pub bookmark_colors: [Rgb; 4],
    pub cache_thumbnail_pages: bool,
    pub thumbnail_resampling: BitmapResampling,
    pub thumbnail_quality: i32,
    pub export_resampling: BitmapResampling,
    pub sync_resamping: BitmapResampling,
    pub software_filter: BitmapResampling,
    pub comic_caption_format: String,
    pub comic_export_file_name_format: String,
    pub pdf_engine_to_use: PdfEngine,
    pub pdfium_image_size: (i32, i32),
    pub ghostscript_executable: Option<String>,
    pub djvu_libre_install: Option<String>,
    pub djvu_size_limit: (i32, i32),
    pub mirrored_page_turn_animation: bool,
    /// clamped 0.01..=0.5
    pub page_bow_width: f32,
    /// clamped 0..=255
    pub page_bow_from_alpha: i32,
    /// clamped 0..=255
    pub page_bow_to_alpha: i32,
    pub page_bow_color: Rgb,
    pub page_bow_center: bool,
    pub page_bow_border: bool,
    pub software_filter_min_scale: f32,
    /// clamped 1..=32
    pub maximum_queue_threads: i32,
    /// clamped 1..=32
    pub maximum_update_threads: i32,
    pub page_shadow_width_percentage: f32,
    pub page_shadow_opacity: f32,
    pub is_recent_in_days: i32,
    pub is_read_completion_percentage: i32,
    pub is_not_read_completion_percentage: i32,
    pub operation_timeout: i32,
    pub server_provider_cache_size: i32,
    pub gesture_area_size: i32,
    pub show_gesture_hint: bool,
    pub html_info_context_menu: bool,
    pub enable_html_script_errors: bool,
    pub hide_visible_part_overlay_close: bool,
    pub aero_full_screen_workaround: bool,
    /// ARGB; 0 = `Color.Empty`
    pub thumbnail_page_curl_color: i32,
    pub thumbnail_page_bow: bool,
    pub blank_page_color: Rgb,
    pub search_browser_case_sensitive: bool,
    pub rating_stars_below_thumbnails: bool,
    pub sync_optimize_quality: i32,
    pub sync_optimize_max_height: i32,
    pub sync_optimize_sharpen: bool,
    pub sync_web_p: bool,
    pub sync_optimize_web_p: bool,
    pub sync_create_thumbnails: bool,
    pub extra_wifi_device_addresses: Option<String>,
    pub sync_queue_length: i32,
    pub sync_keep_read_comics: i32,
    pub page_caching_delay: i32,
    pub cbz_uses: CbEngine,
    pub cbr_uses: CbEngine,
    pub cb7_uses: CbEngine,
    pub cbt_uses: CbEngine,
    pub free_device_memory_mb: i32,
    /// ctor 32 (the `[DefaultValue(4)]` attribute is stale).
    pub parallel_conversions: i32,
    pub wifi_sync_receive_timeout: i32,
    pub wifi_sync_send_timeout: i32,
    pub wifi_sync_connection_timeout: i32,
    pub wifi_sync_connection_retries: i32,
    pub disable_ntfs: bool,
    pub jpeg_xl_encoder_effort: i32,
    pub force_jpeg_reconstruction: bool,
    pub use_legacy_zip_configuration: bool,
    pub ignore_embedded_comic_book_xml: bool,
}

impl Default for EngineConfiguration {
    fn default() -> Self {
        EngineConfiguration {
            enable_parallel_queries: true,
            ignored_articles: None,
            of_values: None,
            legacy_filename_parser: false,
            page_scrolling_duration: 1000,
            animation_duration: 250,
            blend_duration: 400,
            software_filter_delay: 1000,
            list_cover_size: (512, 512),
            list_cover_alpha: 0.3,
            navigation_panel_width: 0.9,
            temp_path: std::env::temp_dir().to_string_lossy().into_owned(),
            bookmark_colors: [(255, 165, 0), (0, 128, 0), (255, 0, 0), (0, 0, 255)],
            cache_thumbnail_pages: false,
            thumbnail_resampling: BitmapResampling::FastBilinear,
            thumbnail_quality: 60,
            export_resampling: BitmapResampling::GdiPlusHQ,
            sync_resamping: BitmapResampling::GdiPlus,
            software_filter: BitmapResampling::GdiPlusHQ,
            comic_caption_format: DEFAULT_CAPTION_FORMAT.to_string(),
            comic_export_file_name_format: DEFAULT_COMIC_EXPORT_FILE_NAME_FORMAT.to_string(),
            pdf_engine_to_use: PdfEngine::Pdfium,
            pdfium_image_size: (1920, 2540),
            ghostscript_executable: None,
            djvu_libre_install: None,
            djvu_size_limit: (2000, 2000),
            mirrored_page_turn_animation: false,
            page_bow_width: 0.07,
            page_bow_from_alpha: 92,
            page_bow_to_alpha: 0,
            page_bow_color: (0, 0, 0),
            page_bow_center: true,
            page_bow_border: true,
            software_filter_min_scale: 0.05,
            maximum_queue_threads: 4,
            maximum_update_threads: 2,
            page_shadow_width_percentage: 1.0,
            page_shadow_opacity: 0.6,
            is_recent_in_days: 14,
            is_read_completion_percentage: 95,
            is_not_read_completion_percentage: 10,
            operation_timeout: 300,
            server_provider_cache_size: 100,
            gesture_area_size: 80,
            show_gesture_hint: true,
            html_info_context_menu: false,
            enable_html_script_errors: false,
            hide_visible_part_overlay_close: false,
            aero_full_screen_workaround: true,
            thumbnail_page_curl_color: 0,
            thumbnail_page_bow: true,
            blank_page_color: (255, 255, 255),
            search_browser_case_sensitive: false,
            rating_stars_below_thumbnails: true,
            sync_optimize_quality: 65,
            sync_optimize_max_height: 1500,
            sync_optimize_sharpen: false,
            sync_web_p: false,
            sync_optimize_web_p: true,
            sync_create_thumbnails: true,
            extra_wifi_device_addresses: None,
            sync_queue_length: 50,
            sync_keep_read_comics: 1,
            page_caching_delay: 1000,
            cbz_uses: CbEngine::SevenZip,
            cbr_uses: CbEngine::SevenZip,
            cb7_uses: CbEngine::SevenZip,
            cbt_uses: CbEngine::SevenZip,
            free_device_memory_mb: 128,
            parallel_conversions: 32,
            wifi_sync_receive_timeout: 5000,
            wifi_sync_send_timeout: 5000,
            wifi_sync_connection_timeout: 2500,
            wifi_sync_connection_retries: 1,
            disable_ntfs: false,
            jpeg_xl_encoder_effort: 7,
            force_jpeg_reconstruction: false,
            use_legacy_zip_configuration: false,
            ignore_embedded_comic_book_xml: false,
        }
    }
}

crate::settings_fields! {
    ENGINE_CONFIG_FIELDS, EngineConfiguration,
    Bool "EnableParallelQueries" => enable_parallel_queries: bool, cat: "", desc: "", browsable: false, ini: true;
    StrOpt "IgnoredArticles" => ignored_articles: Option<String>, cat: "", desc: "", browsable: false, ini: true;
    StrOpt "OfValues" => of_values: Option<String>, cat: "", desc: "", browsable: false, ini: true;
    Bool "LegacyFilenameParser" => legacy_filename_parser: bool, cat: "", desc: "", browsable: false, ini: true;
    Int "PageScrollingDuration" => page_scrolling_duration: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "AnimationDuration" => animation_duration: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "BlendDuration" => blend_duration: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "SoftwareFilterDelay" => software_filter_delay: i32, cat: "", desc: "", browsable: false, ini: true;
    // ListCoverSize / PdfiumImageSize / DjVuSizeLimit and the colors
    // (PageBowColor, BlankPageColor, BookmarkColors) go through the
    // .NET SizeConverter/ColorConverter semantics in `apply_special`.
    Float "ListCoverAlpha" => list_cover_alpha: f32, cat: "", desc: "", browsable: false, ini: true;
    Float "NavigationPanelWidth" => navigation_panel_width: f32, cat: "", desc: "", browsable: false, ini: true;
    Str "TempPath" => temp_path: String, cat: "", desc: "", browsable: false, ini: true;
    Bool "CacheThumbnailPages" => cache_thumbnail_pages: bool, cat: "", desc: "", browsable: false, ini: true;
    Enum "ThumbnailResampling" => thumbnail_resampling: BitmapResampling, cat: "", desc: "", browsable: false, ini: true;
    Int "ThumbnailQuality" => thumbnail_quality: i32, cat: "", desc: "", browsable: false, ini: true;
    Enum "ExportResampling" => export_resampling: BitmapResampling, cat: "", desc: "", browsable: false, ini: true;
    Enum "SyncResamping" => sync_resamping: BitmapResampling, cat: "", desc: "", browsable: false, ini: true;
    Enum "SoftwareFilter" => software_filter: BitmapResampling, cat: "", desc: "", browsable: false, ini: true;
    Str "ComicCaptionFormat" => comic_caption_format: String, cat: "", desc: "", browsable: false, ini: true;
    Str "ComicExportFileNameFormat" => comic_export_file_name_format: String, cat: "", desc: "", browsable: false, ini: true;
    Enum "PdfEngineToUse" => pdf_engine_to_use: PdfEngine, cat: "", desc: "", browsable: false, ini: true;
    StrOpt "GhostscriptExecutable" => ghostscript_executable: Option<String>, cat: "", desc: "", browsable: false, ini: true;
    StrOpt "DjVuLibreInstall" => djvu_libre_install: Option<String>, cat: "", desc: "", browsable: false, ini: true;
    Bool "MirroredPageTurnAnimation" => mirrored_page_turn_animation: bool, cat: "", desc: "", browsable: false, ini: true;
    Float "PageBowWidth" => page_bow_width: f32, cat: "", desc: "", browsable: false, ini: true;
    Int "PageBowFromAlpha" => page_bow_from_alpha: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "PageBowToAlpha" => page_bow_to_alpha: i32, cat: "", desc: "", browsable: false, ini: true;
    Bool "PageBowCenter" => page_bow_center: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "PageBowBorder" => page_bow_border: bool, cat: "", desc: "", browsable: false, ini: true;
    Float "SoftwareFilterMinScale" => software_filter_min_scale: f32, cat: "", desc: "", browsable: false, ini: true;
    Int "MaximumQueueThreads" => maximum_queue_threads: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "MaximumUpdateThreads" => maximum_update_threads: i32, cat: "", desc: "", browsable: false, ini: true;
    Float "PageShadowWidthPercentage" => page_shadow_width_percentage: f32, cat: "", desc: "", browsable: false, ini: true;
    Float "PageShadowOpacity" => page_shadow_opacity: f32, cat: "", desc: "", browsable: false, ini: true;
    Int "IsRecentInDays" => is_recent_in_days: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "IsReadCompletionPercentage" => is_read_completion_percentage: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "IsNotReadCompletionPercentage" => is_not_read_completion_percentage: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "OperationTimeout" => operation_timeout: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "ServerProviderCacheSize" => server_provider_cache_size: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "GestureAreaSize" => gesture_area_size: i32, cat: "", desc: "", browsable: false, ini: true;
    Bool "ShowGestureHint" => show_gesture_hint: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "HtmlInfoContextMenu" => html_info_context_menu: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "EnableHtmlScriptErrors" => enable_html_script_errors: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "HideVisiblePartOverlayClose" => hide_visible_part_overlay_close: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "AeroFullScreenWorkaround" => aero_full_screen_workaround: bool, cat: "", desc: "", browsable: false, ini: true;
    Int "ThumbnailPageCurlColor" => thumbnail_page_curl_color: i32, cat: "", desc: "", browsable: false, ini: true;
    Bool "ThumbnailPageBow" => thumbnail_page_bow: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "SearchBrowserCaseSensitive" => search_browser_case_sensitive: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "RatingStarsBelowThumbnails" => rating_stars_below_thumbnails: bool, cat: "", desc: "", browsable: false, ini: true;
    Int "SyncOptimizeQuality" => sync_optimize_quality: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "SyncOptimizeMaxHeight" => sync_optimize_max_height: i32, cat: "", desc: "", browsable: false, ini: true;
    Bool "SyncOptimizeSharpen" => sync_optimize_sharpen: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "SyncWebP" => sync_web_p: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "SyncOptimizeWebP" => sync_optimize_web_p: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "SyncCreateThumbnails" => sync_create_thumbnails: bool, cat: "", desc: "", browsable: false, ini: true;
    StrOpt "ExtraWifiDeviceAddresses" => extra_wifi_device_addresses: Option<String>, cat: "", desc: "", browsable: false, ini: true;
    Int "SyncQueueLength" => sync_queue_length: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "SyncKeepReadComics" => sync_keep_read_comics: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "PageCachingDelay" => page_caching_delay: i32, cat: "", desc: "", browsable: false, ini: true;
    Enum "CbzUses" => cbz_uses: CbEngine, cat: "", desc: "", browsable: false, ini: true;
    Enum "CbrUses" => cbr_uses: CbEngine, cat: "", desc: "", browsable: false, ini: true;
    Enum "Cb7Uses" => cb7_uses: CbEngine, cat: "", desc: "", browsable: false, ini: true;
    Enum "CbtUses" => cbt_uses: CbEngine, cat: "", desc: "", browsable: false, ini: true;
    Int "FreeDeviceMemoryMB" => free_device_memory_mb: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "ParallelConversions" => parallel_conversions: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "WifiSyncReceiveTimeout" => wifi_sync_receive_timeout: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "WifiSyncSendTimeout" => wifi_sync_send_timeout: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "WifiSyncConnectionTimeout" => wifi_sync_connection_timeout: i32, cat: "", desc: "", browsable: false, ini: true;
    Int "WifiSyncConnectionRetries" => wifi_sync_connection_retries: i32, cat: "", desc: "", browsable: false, ini: true;
    Bool "DisableNTFS" => disable_ntfs: bool, cat: "", desc: "", browsable: false, ini: true;
    Int "JpegXLEncoderEffort" => jpeg_xl_encoder_effort: i32, cat: "", desc: "", browsable: false, ini: true;
    Bool "ForceJpegReconstruction" => force_jpeg_reconstruction: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "UseLegacyZipConfiguration" => use_legacy_zip_configuration: bool, cat: "", desc: "", browsable: false, ini: true;
    Bool "IgnoreEmbeddedComicBookXml" => ignore_embedded_comic_book_xml: bool, cat: "", desc: "", browsable: false, ini: true;
}

impl EngineConfiguration {
    /// Apply an ini (plus its command-line overlay already merged in
    /// by the caller), then the converter-only fields, then the
    /// setter clamps — the C# `Register<EngineConfiguration>` flow.
    pub fn load(&mut self, values: &IniValues) {
        apply_ini(values, self, ENGINE_CONFIG_FIELDS);
        self.apply_special(values);
        self.normalize();
    }

    /// The fields the generic table cannot express: `Size`/`Color`
    /// converter texts and the directory-checked `TempPath`.
    fn apply_special(&mut self, values: &IniValues) {
        if let Some(t) = values.get("ListCoverSize").and_then(parse_size) {
            self.list_cover_size = t;
        }
        if let Some(t) = values.get("PdfiumImageSize").and_then(parse_size) {
            self.pdfium_image_size = t;
        }
        if let Some(t) = values.get("DjVuSizeLimit").and_then(parse_size) {
            self.djvu_size_limit = t;
        }
        if let Some(c) = values.get("PageBowColor").and_then(parse_color) {
            self.page_bow_color = c;
        }
        if let Some(c) = values.get("BlankPageColor").and_then(parse_color) {
            self.blank_page_color = c;
        }
        if let Some(t) = values.get("BookmarkColors") {
            let colors: Vec<Rgb> = t.split(',').filter_map(parse_color).collect();
            if colors.len() == 4 {
                self.bookmark_colors = [colors[0], colors[1], colors[2], colors[3]];
            }
        }
        if let Some(p) = values.get("TempPath") {
            // The C# setter accepts only existing directories.
            if std::path::Path::new(p).is_dir() {
                self.temp_path = p.to_string();
            }
        }
    }

    /// The C# clamps the values inside the property setters; here the
    /// same clamps run after the load (`value.Clamp(...)` parity).
    pub fn normalize(&mut self) {
        self.page_bow_width = self.page_bow_width.clamp(0.01, 0.5);
        self.page_bow_from_alpha = self.page_bow_from_alpha.clamp(0, 255);
        self.page_bow_to_alpha = self.page_bow_to_alpha.clamp(0, 255);
        self.maximum_queue_threads = self.maximum_queue_threads.clamp(1, 32);
        self.maximum_update_threads = self.maximum_update_threads.clamp(1, 32);
    }

    /// `OfValues ?? "of,von,de"` (`ComicNameInfo`).
    pub fn of_values_or_default(&self) -> String {
        self.of_values
            .clone()
            .unwrap_or_else(|| "of,von,de".to_string())
    }
}

/// `SizeConverter`: `"512, 512"`.
fn parse_size(text: &str) -> Option<(i32, i32)> {
    let mut it = text.split(',');
    let w = it.next()?.trim().parse().ok()?;
    let h = it.next()?.trim().parse().ok()?;
    Some((w, h))
}

/// `ColorConverter`: the named subset the configs use, else
/// `"r, g, b"`.
fn parse_color(text: &str) -> Option<Rgb> {
    let named = match text.trim().to_ascii_lowercase().as_str() {
        "orange" => Some((255, 165, 0)),
        "green" => Some((0, 128, 0)),
        "red" => Some((255, 0, 0)),
        "blue" => Some((0, 0, 255)),
        "black" => Some((0, 0, 0)),
        "white" => Some((255, 255, 255)),
        _ => None,
    };
    named.or_else(|| {
        let mut it = text.split(',');
        let r = it.next()?.trim().parse().ok()?;
        let g = it.next()?.trim().parse().ok()?;
        let b = it.next()?.trim().parse().ok()?;
        Some((r, g, b))
    })
}

/// The `EngineConfiguration.Default` singleton (a write-through
/// global; the C# static). `EngineConfiguration::init_global`
/// installs a configuration loaded from the ini chain; before that
/// the defaults apply.
static GLOBAL: std::sync::OnceLock<std::sync::RwLock<EngineConfiguration>> =
    std::sync::OnceLock::new();

impl EngineConfiguration {
    pub fn init_global(config: EngineConfiguration) {
        // WRITE-THROUGH (the ExtendedSettings boot bug): an early
        // `global()` reader must not freeze the defaults over the
        // parsed ini.
        let lock = GLOBAL.get_or_init(|| std::sync::RwLock::new(config.clone()));
        *lock
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = config;
    }

    pub fn global() -> EngineConfigurationGuard<'static> {
        let lock = GLOBAL.get_or_init(|| std::sync::RwLock::new(EngineConfiguration::default()));
        let guard = lock
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        EngineConfigurationGuard(guard)
    }
}

/// The read guard for the [`EngineConfiguration`] global (the
/// `RwLock` snapshot — callers used the `&'static` view before).
pub struct EngineConfigurationGuard<'a>(std::sync::RwLockReadGuard<'a, EngineConfiguration>);

impl std::ops::Deref for EngineConfigurationGuard<'_> {
    type Target = EngineConfiguration;
    fn deref(&self) -> &EngineConfiguration {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ini::IniValues;

    #[test]
    fn defaults_match_the_ctor() {
        let c = EngineConfiguration::default();
        assert_eq!(c.is_recent_in_days, 14);
        assert_eq!(c.is_read_completion_percentage, 95);
        assert_eq!(c.is_not_read_completion_percentage, 10);
        assert_eq!(c.blend_duration, 400);
        assert_eq!(c.parallel_conversions, 32);
        assert_eq!(c.thumbnail_quality, 60);
        assert_eq!(c.maximum_queue_threads, 4);
        assert_eq!(c.comic_caption_format, DEFAULT_CAPTION_FORMAT);
        assert_eq!(
            c.of_values_or_default(),
            "of,von,de",
            "the ComicNameInfo fallback"
        );
    }

    #[test]
    fn ini_keys_bind_case_insensitively_by_property_name() {
        let mut c = EngineConfiguration::default();
        c.load(&IniValues::read_text(
            "isrecentindays = 30\nOFVALUES=of,de\nMaximumQueueThreads=64\nThumbnailQuality=abc",
            None,
        ));
        assert_eq!(c.is_recent_in_days, 30);
        assert_eq!(c.of_values.as_deref(), Some("of,de"));
        // The clamp fires after the load (the C# setter clamp).
        assert_eq!(c.maximum_queue_threads, 32);
        // Parse failure keeps the default.
        assert_eq!(c.thumbnail_quality, 60);
    }

    #[test]
    fn size_and_color_converters() {
        let mut c = EngineConfiguration::default();
        c.load(&IniValues::read_text(
            "ListCoverSize = 256, 384\nBookmarkColors=Red, Green, Blue, White\nBlankPageColor=Black",
            None,
        ));
        assert_eq!(c.list_cover_size, (256, 384));
        assert_eq!(c.bookmark_colors[0], (255, 0, 0));
        assert_eq!(c.bookmark_colors[3], (255, 255, 255));
        assert_eq!(c.blank_page_color, (0, 0, 0));
    }
}
