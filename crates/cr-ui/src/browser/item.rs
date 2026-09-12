//! The comic item drawing — the `CoverViewItem`/`ThumbRenderer`
//! visuals in cairo: the cover (border 4, shadow reserve, the
//! right-keeping crop, 1 px frame, selection tint), the read markers
//! (`DrawBookmarkV` ribbons at the right edge — Orange = CurrentPage,
//! Green = LastPageRead), the numeric rating tags (the default
//! `NumericRatingThumbnails` mode — personal gold, community blue at
//! the bottom-right), the file-missing marker (bottom-left strip),
//! the caption (the exact `ComicBook.Caption` format through
//! `display_text::caption`, centered, wrapping in the 3-line strip),
//! and the Tile text lines (`ComicTextElements.DefaultFileComic`).
//!
//! Deviations: the state markers beyond file-missing (dirty/open/
//! last/new-pages) need the unported PNG resources; the dog-ear page
//! curl and the bow shadow are later polish. The tag/bookmark
//! shapes approximate the C# PNG assets.

use gtk4::cairo;
use gtk4::cairo::Context;

use cr_core::model::comic_book::ComicBook;
use cr_core::model::comic_name_info::ComicNameInfo;
use cr_engine::display_text;
use cr_engine::matcher::book_view;

// Colors (the C# assets/config defaults approximated).
const BOOKMARK_COLORS: [(f64, f64, f64); 4] = [
    (1.0, 0.6, 0.0),   // Orange (current page)
    (0.1, 0.75, 0.2),  // Green (last page read)
    (0.85, 0.1, 0.1),  // Red
    (0.15, 0.35, 0.9), // Blue
];
const TAG_PERSONAL: (f64, f64, f64) = (0.95, 0.78, 0.14);
const TAG_COMMUNITY: (f64, f64, f64) = (0.35, 0.55, 0.9);
const TAG_TEXT: (f64, f64, f64) = (0.4, 0.4, 0.4);
const SHADOW: (f64, f64, f64, f64) = (0.0, 0.0, 0.0, 0.33);
const SELECTION_TINT: (f64, f64, f64, f64) = (0.2, 0.38, 0.62, 41.0 / 255.0);

/// `ComicBook.ArtistInfo` — the unique names of writer/penciller/
/// inker/colorist/letterer/cover artist/translator joined by "/".
pub fn artist_info(book: &ComicBook) -> String {
    let mut seen: Vec<String> = Vec::new();
    for field in [
        &book.info.writer,
        &book.info.penciller,
        &book.info.inker,
        &book.info.colorist,
        &book.info.letterer,
        &book.info.cover_artist,
        &book.info.translator,
    ] {
        for name in field.split(',') {
            let name = name.trim();
            if name.is_empty() || seen.iter().any(|s| s == name) {
                continue;
            }
            seen.push(name.to_string());
        }
    }
    seen.join("/")
}

/// The cover image into `box_` (`ThumbRenderer.DrawThumbnail`
/// essentials): border-4 inset, shadow reserve right/bottom, the
/// fill-scale crop that keeps the RIGHT part of a landscape cover in
/// a portrait box, 1 px black frame, the selection tint fill.
pub fn draw_cover(
    ctx: &Context,
    surface: Option<&cairo::ImageSurface>,
    box_: (f64, f64, f64, f64),
    selected: bool,
) {
    let (x, y, w, h) = box_;
    // Border: 4 px per side.
    let (bx, by, mut bw, mut bh) = (x + 4.0, y + 4.0, w - 8.0, h - 8.0);
    // The shadow reserve eats 4 px right/bottom.
    bw -= 4.0;
    bh -= 4.0;
    // Shadow: black at 33% in the reserved band (the C# blurs a 4 px
    // band around the expanded rect).
    ctx.set_source_rgba(SHADOW.0, SHADOW.1, SHADOW.2, SHADOW.3);
    ctx.rectangle(bx + 1.0, by + 1.0, bw + 3.0, bh + 3.0);
    ctx.fill().ok();
    if let Some(surface) = surface {
        let (iw, ih) = (surface.width() as f64, surface.height() as f64);
        ctx.save().ok();
        ctx.rectangle(bx, by, bw, bh);
        ctx.clip();
        // Fill-scale (scales UP too); the crop keeps the RIGHT part
        // of the source (`src.X = srcW - src.Width`).
        let scale = (bw / iw).max(bh / ih);
        let dw = iw * scale;
        let dh = ih * scale;
        ctx.translate(bx + bw - dw, by + bh - dh);
        ctx.scale(scale, scale);
        ctx.set_source_surface(surface, 0.0, 0.0).ok();
        ctx.rectangle(0.0, 0.0, iw, ih);
        ctx.fill().ok();
        ctx.restore().ok();
    }
    // The selection tint fills the cover (the no-dog-ear branch).
    if selected {
        ctx.set_source_rgba(
            SELECTION_TINT.0,
            SELECTION_TINT.1,
            SELECTION_TINT.2,
            SELECTION_TINT.3,
        );
        ctx.rectangle(bx, by, bw, bh);
        ctx.fill().ok();
    }
    // The 1 px black border.
    ctx.set_source_rgb(0.0, 0.0, 0.0);
    ctx.set_line_width(1.0);
    ctx.rectangle(bx, by, bw, bh);
    ctx.stroke().ok();
}

/// `DrawBookmarkV` — the read markers: a 16×8 swallowtail ribbon
/// sliding down the right edge with the page percent. `pages` =
/// (CurrentPage, LastPageRead) over the PageCount denominator.
pub fn draw_bookmarks(
    ctx: &Context,
    box_: (f64, f64, f64, f64),
    pages: (i32, i32),
    page_count: i32,
) {
    let (x, y, w, h) = box_;
    let inner = (w - 8.0, h - 8.0); // border 4 + shadow reserve 4
    let (right, top) = (x + 4.0 + inner.0, y + 4.0);
    if page_count <= 0 {
        return;
    }
    let denominator = page_count as f64;
    let thickness = 8.0_f64.min(inner.1 - 2.0);
    let length = 16.0_f64.min(inner.0 - 2.0);
    for (i, page) in [pages.0, pages.1].iter().enumerate() {
        if *page < 0 {
            continue;
        }
        let percent = ((*page as f64) / denominator).clamp(0.0, 1.0);
        let col1 = BOOKMARK_COLORS[i % 4];
        let col2 = (col1.0 / 2.0, col1.1 / 2.0, col1.2 / 2.0);
        let ry = top + (inner.1 - thickness) * percent;
        let lx = right - length;
        // Shadow (+1,+1).
        ribbon_path(ctx, lx + 1.0, ry + 1.0, length, thickness);
        ctx.set_source_rgba(0.0, 0.0, 0.0, 1.0);
        ctx.fill().ok();
        // Fill: vertical gradient col1 → col2.
        ribbon_path(ctx, lx, ry, length, thickness);
        let gradient = cairo::LinearGradient::new(lx, ry, lx, ry + thickness);
        gradient.add_color_stop_rgb(0.0, col1.0, col1.1, col1.2);
        gradient.add_color_stop_rgb(1.0, col2.0, col2.1, col2.2);
        ctx.set_source(&gradient).ok();
        ctx.fill().ok();
        // 1 px black outline.
        ribbon_path(ctx, lx, ry, length, thickness);
        ctx.set_source_rgb(0.0, 0.0, 0.0);
        ctx.set_line_width(1.0);
        ctx.stroke().ok();
    }
}

fn ribbon_path(ctx: &Context, x: f64, y: f64, length: f64, thickness: f64) {
    // TL → TR → BR → BL → notch (half depth into the inner end).
    ctx.move_to(x, y);
    ctx.line_to(x + length, y);
    ctx.line_to(x + length, y + thickness);
    ctx.line_to(x, y + thickness);
    ctx.line_to(x + thickness, y + thickness / 2.0);
    ctx.close_path();
}

/// `DrawTagRating` — the numeric tags (the default rating mode): a
/// square of clamp(H/5,16,64) at the cover's bottom-right; the
/// community tag first, the personal tag to its left; the number
/// scaled bottom-center.
pub fn draw_rating_tags(ctx: &Context, box_: (f64, f64, f64, f64), personal: f32, community: f32) {
    let (x, y, w, h) = box_;
    let inner = (w - 8.0, h - 8.0);
    let tag = (inner.1 / 5.0).clamp(16.0, 64.0);
    let mut right = x + 4.0 + inner.0;
    for (value, color) in [(community, TAG_COMMUNITY), (personal, TAG_PERSONAL)] {
        if value <= 0.0 {
            continue;
        }
        let tx = right - tag;
        let ty = y + 4.0 + inner.1 - tag;
        // The tag plate (the C# uses a shaped PNG).
        ctx.set_source_rgb(color.0, color.1, color.2);
        rounded_rect(ctx, tx, ty, tag, tag, tag * 0.18);
        ctx.fill().ok();
        ctx.set_source_rgba(0.0, 0.0, 0.0, 0.35);
        ctx.set_line_width(1.0);
        rounded_rect(ctx, tx, ty, tag, tag, tag * 0.18);
        ctx.stroke().ok();
        // The number ("N.N"), bold italic, scaled to ~0.9 of the
        // inner width, bottom-center.
        let text = format!("{value:.1}");
        ctx.select_font_face("Sans", cairo::FontSlant::Italic, cairo::FontWeight::Bold);
        ctx.set_font_size(12.0);
        if let Ok(ext) = ctx.text_extents(&text) {
            let scale = ((tag - 4.0) / ext.width() * 0.9).min(1.0);
            ctx.set_font_size(12.0 * scale);
            if let Ok(ext) = ctx.text_extents(&text) {
                ctx.set_source_rgb(TAG_TEXT.0, TAG_TEXT.1, TAG_TEXT.2);
                ctx.move_to(
                    tx + (tag - ext.width()) / 2.0 - ext.x_bearing(),
                    ty + tag - 3.0,
                );
                ctx.show_text(&text).ok();
            }
        }
        right = tx;
    }
}

fn rounded_rect(ctx: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    ctx.new_sub_path();
    ctx.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    ctx.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    ctx.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    ctx.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    ctx.close_path();
}

/// The bottom-left state marker (strip height = clamp(H/10,16,32)):
/// the file-missing red cross, or the fileless icon (the C# state
/// row order — `!Comic.IsLinked` → `MarkerIsFileLessImage` before
/// the missing `DeletedStateImage`; the two never co-occur). The
/// strip shape is the port's recorded deviation from the C#'s
/// centered `DrawImageList` row.
pub fn draw_state_marker(
    ctx: &Context,
    box_: (f64, f64, f64, f64),
    surface: Option<&cairo::ImageSurface>,
) {
    let (x, y, w, h) = box_;
    let inner = (w - 8.0, h - 8.0);
    let strip = (inner.1 / 10.0).clamp(16.0, 32.0);
    let sy = y + 4.0 + inner.1 - strip;
    if let Some(surface) = surface {
        let (iw, ih) = (surface.width() as f64, surface.height() as f64);
        let scale = (strip / iw).min(strip / ih);
        let dh = ih * scale;
        ctx.save().ok();
        ctx.translate(x + 4.0, sy + (strip - dh) / 2.0);
        ctx.scale(scale, scale);
        ctx.set_source_surface(surface, 0.0, 0.0).ok();
        ctx.rectangle(0.0, 0.0, iw, ih);
        ctx.fill().ok();
        ctx.restore().ok();
    } else {
        ctx.set_source_rgb(0.85, 0.1, 0.1);
        ctx.rectangle(x + 4.0, sy, strip, strip);
        ctx.fill().ok();
    }
}

/// One wrapped, centered text block inside the strip (`DrawText`
/// with a single wrapping CaptionId): words wrap at `width`, drawn
/// centered, at most `max_lines` lines, an ellipsis on a truncated
/// tail. Returns the lines drawn.
pub fn draw_wrapped_centered(
    ctx: &Context,
    text: &str,
    x: f64,
    y: f64,
    width: f64,
    max_lines: usize,
    color: (f64, f64, f64),
) -> usize {
    ctx.set_source_rgb(color.0, color.1, color.2);
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in &words {
        let candidate = if current.is_empty() {
            (*word).to_string()
        } else {
            format!("{current} {word}")
        };
        let extent = ctx.text_extents(&candidate).ok();
        let fits = extent.is_none_or(|e| e.width() <= width);
        if fits {
            current = candidate;
        } else if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            current = (*word).to_string();
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    let mut drawn = 0usize;
    let line_h = font_height(ctx) * 1.2;
    for (i, line) in lines.iter().enumerate() {
        if drawn >= max_lines {
            break;
        }
        let mut text = line.clone();
        if i == max_lines - 1 && lines.len() > max_lines {
            text.push('…');
        }
        if let Ok(e) = ctx.text_extents(&text) {
            // Ellipsize the final visible line when it overflows.
            let mut text = text;
            if e.width() > width && i == max_lines - 1 {
                while text.len() > 1 {
                    text.pop();
                    let e2 = ctx.text_extents(&format!("{text}…")).ok();
                    if e2.is_none_or(|e2| e2.width() <= width) {
                        text.push('…');
                        break;
                    }
                }
            }
            let e = ctx.text_extents(&text).ok().unwrap_or(e);
            ctx.move_to(
                x + (width - e.width()) / 2.0 - e.x_bearing(),
                y + line_h * (drawn as f64 + 0.85),
            );
            ctx.show_text(&text).ok();
        }
        drawn += 1;
    }
    drawn
}

fn font_height(ctx: &Context) -> f64 {
    let ext = ctx.font_extents().ok();
    ext.map(|e| e.height()).unwrap_or(12.0)
}

/// The current font's line height (the text helpers share it).
pub fn line_height(ctx: &Context) -> f64 {
    font_height(ctx)
}

/// The Tile text lines (`ComicTextElements.DefaultFileComic`):
/// (text, font scale, bold). The tab-stop lines carry a `"\t"` that
/// the caller renders as a two-column block.
pub fn tile_text_lines(book: &ComicBook) -> Vec<(String, f64, bool)> {
    let prop: ComicNameInfo = book_view::proposed_cached(book);
    let mut lines: Vec<(String, f64, bool)> = vec![
        (display_text::caption_without_title(book), 1.0, true),
        (book_view::shadow_title(book, &prop).to_string(), 1.0, true),
        (artist_info(book), 0.95, false),
        (String::new(), 0.95, false), // 4 px spacer
    ];
    let summary = book.info.summary.replace('\t', " ");
    if !summary.is_empty() {
        lines.push((summary, 0.95, false));
        lines.push((String::new(), 0.95, false)); // 6 px spacer
    }
    let size = format!(
        "Size:\t{}/{}",
        display_text::column_text(book, "FileSizeAsText"),
        display_text::column_text(book, "PagesAsTextSimple")
    );
    lines.push((size, 0.9, false));
    lines.push((
        format!("Opened:\t{}", display_text::column_text(book, "OpenedTime")),
        0.9,
        false,
    ));
    lines.push((
        format!("Added:\t{}", display_text::column_text(book, "AddedTime")),
        0.9,
        false,
    ));
    lines.push((
        format!("Format:\t{}", book_view::shadow_format(book, &prop)),
        0.9,
        false,
    ));
    let file_name = std::path::Path::new(&book.file_path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    lines.push((format!("File:\t{file_name}"), 0.9, false));
    lines
}

/// `DrawPageNumber` — the 1-based page badge: a rounded black-75%
/// rect at the top-right with white text (Arial 7 pt in the C#;
/// scaled here to the cell).
pub fn draw_page_number(ctx: &Context, box_: (f64, f64, f64, f64), page: usize) {
    let (x, y, w, _h) = box_;
    let text = page.to_string();
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    ctx.set_font_size(10.0);
    let Ok(ext) = ctx.text_extents(&text) else {
        return;
    };
    // Badge rect: min width 20, inflated 2 px around the text.
    let bw = (ext.width() + 8.0).max(20.0);
    let bh = 16.0;
    let bx = x + w - bw - 4.0;
    let by = y + 4.0;
    ctx.set_source_rgba(0.0, 0.0, 0.0, 0.75);
    rounded_rect(ctx, bx, by, bw, bh, 3.0);
    ctx.fill().ok();
    ctx.set_source_rgb(1.0, 1.0, 1.0);
    ctx.move_to(
        bx + (bw - ext.width()) / 2.0 - ext.x_bearing(),
        by + bh * 0.78,
    );
    ctx.show_text(&text).ok();
}

/// `DrawBookmarkH` — the bookmarked-page pennant: a small red ribbon
/// attached to the TOP edge (display-only; the editor is Phase 5).
pub fn draw_bookmark_h(ctx: &Context, box_: (f64, f64, f64, f64)) {
    let (x, y, _w, _h) = box_;
    // The 8 px tall × 16 px wide ribbon at the top-left of the cover
    // area, notch cut into the bottom edge.
    let thickness = 8.0;
    let length = 16.0;
    let rx = x + 4.0;
    let ry = y + 4.0;
    // Shadow (+1,+1).
    ctx.move_to(rx + 1.0, ry + 1.0);
    ctx.line_to(rx + length + 1.0, ry + 1.0);
    ctx.line_to(rx + length + 1.0, ry + thickness + 1.0);
    ctx.line_to(rx + length / 2.0 + 1.0, ry + thickness / 2.0 + 1.0);
    ctx.line_to(rx + 1.0, ry + thickness + 1.0);
    ctx.close_path();
    ctx.set_source_rgba(0.0, 0.0, 0.0, 1.0);
    ctx.fill().ok();
    // The red gradient fill (red → dark red).
    let gradient = cairo::LinearGradient::new(rx, ry, rx, ry + thickness);
    gradient.add_color_stop_rgb(0.0, 0.85, 0.1, 0.1);
    gradient.add_color_stop_rgb(1.0, 0.42, 0.05, 0.05);
    ctx.move_to(rx, ry);
    ctx.line_to(rx + length, ry);
    ctx.line_to(rx + length, ry + thickness);
    ctx.line_to(rx + length / 2.0, ry + thickness / 2.0);
    ctx.line_to(rx, ry + thickness);
    ctx.close_path();
    ctx.set_source(&gradient).ok();
    ctx.fill().ok();
    ctx.set_source_rgb(0.0, 0.0, 0.0);
    ctx.set_line_width(1.0);
    ctx.stroke().ok();
}

/// The "no metadata" condition (PORT ADDITION, user request — no C#
/// counterpart): a file-backed, present comic whose descriptive
/// metadata is all empty — the scan/open found nothing to import
/// (the `create_book`/`apply_info_chain` chain read nothing). A
/// fileless or missing-file book carries its own state marker and
/// never shows the tag. The tag clears the moment any key field
/// fills: an editor commit and the Comic Vine scrape both land in
/// these fields, so no persisted flag is needed.
pub fn metadata_missing(book: &ComicBook) -> bool {
    if book.file_path.is_empty() || book.file_is_missing {
        return false;
    }
    let i = &book.info;
    i.series.is_empty()
        && i.title.is_empty()
        && i.number.is_empty()
        && i.volume == -1
        && i.writer.is_empty()
        && i.publisher.is_empty()
        && i.summary.is_empty()
}

/// The "no metadata" tag: a small translucent dark chip with a "?"
/// at the top-left of the cover — subtle in both themes and against
/// any cover art.
pub fn draw_metadata_tag(ctx: &Context, box_: (f64, f64, f64, f64)) {
    draw_chip(ctx, box_, 0, "?", (0.05, 0.05, 0.05, 0.62), (1.0, 1.0, 1.0));
}

/// The scan marker a book carries after a scan (PORT ADDITION, user
/// request 2026-09-11 — no C# counterpart).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMarker {
    /// Red "!": the file could not be read, timed out, or was skipped.
    Failed,
    /// Amber "≠": the archive content is not the format the file name
    /// claims. The book IS readable through the detected reader.
    Mismatch,
}

impl ScanMarker {
    pub(crate) fn glyph(self) -> &'static str {
        match self {
            ScanMarker::Failed => "!",
            ScanMarker::Mismatch => "≠",
        }
    }

    /// The chip fill. Both keep the translucent-dark base of the "?"
    /// chip so they read the same way over any cover art.
    fn fill(self) -> (f64, f64, f64, f64) {
        match self {
            ScanMarker::Failed => (0.62, 0.09, 0.09, 0.82),
            ScanMarker::Mismatch => (0.68, 0.45, 0.05, 0.82),
        }
    }
}

/// The scan marker for a book, read from the stored
/// `comicrust.scan.status` custom value. A book with no verdict, and
/// a fileless one, carry no marker.
pub fn scan_marker(book: &ComicBook) -> Option<ScanMarker> {
    if book.file_path.is_empty() {
        return None;
    }
    match cr_core::scan_status::status(book)? {
        s if s.is_failure() => Some(ScanMarker::Failed),
        cr_core::scan_status::ScanStatus::FormatMismatch => Some(ScanMarker::Mismatch),
        _ => None,
    }
}

/// The tooltip for EVERY chip a book carries, in the order the chips
/// are drawn: the "?" no-metadata tag first, then the scan marker.
///
/// A book can carry both. The tooltip used to describe the scan
/// marker alone, so a book with both chips explained only one of them
/// and a book with only the "?" chip explained nothing.
pub fn chip_tooltip(book: &ComicBook) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if metadata_missing(book) {
        parts.push(
            "?  No metadata was found for this book.\n\
             Edit a key field in Properties, or scrape it, to clear this."
                .to_string(),
        );
    }
    if let (Some(marker), Some(scan)) = (scan_marker(book), scan_marker_tooltip(book)) {
        parts.push(format!("{}  {scan}", marker.glyph()));
    }
    if parts.is_empty() {
        None
    } else {
        // A blank line between the chips, so two reasons do not read
        // as one.
        Some(parts.join("\n\n"))
    }
}

/// The tooltip text for a book's scan marker: the verdict, the format
/// disagreement when there is one, and the stored reason.
pub fn scan_marker_tooltip(book: &ComicBook) -> Option<String> {
    let status = cr_core::scan_status::status(book)?;
    let mut text = String::from(status.as_text());
    let detected = cr_core::scan_status::detected_format(book);
    let expected = custom_value(book, cr_core::scan_status::EXPECTED_FORMAT_KEY);
    match (&detected, &expected) {
        (Some(detected), Some(expected)) => {
            text.push_str(&format!(
                "\nContent is {detected}, the name says {expected}"
            ));
        }
        (Some(detected), None) => text.push_str(&format!("\nContent is {detected}")),
        _ => {}
    }
    if let Some(error) = cr_core::scan_status::error_text(book) {
        text.push('\n');
        text.push_str(&error);
    }
    Some(text)
}

fn custom_value(book: &ComicBook, key: &str) -> Option<String> {
    cr_core::model::comic_book::values_store::decode(&book.custom_values_store)
        .into_iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
        .filter(|v| !v.is_empty())
}

/// Draws a book's scan marker. `slot` shifts the chip right so it does
/// not cover the "no metadata" chip when a book carries both.
pub fn draw_scan_tag(ctx: &Context, box_: (f64, f64, f64, f64), marker: ScanMarker, slot: usize) {
    draw_chip(
        ctx,
        box_,
        slot,
        marker.glyph(),
        marker.fill(),
        (1.0, 1.0, 1.0),
    );
}

/// The shared chip geometry of every top-left cover tag. `slot` is the
/// zero-based position in the row of chips.
fn draw_chip(
    ctx: &Context,
    box_: (f64, f64, f64, f64),
    slot: usize,
    glyph: &str,
    fill: (f64, f64, f64, f64),
    ink: (f64, f64, f64),
) {
    let (x, y, w, h) = box_;
    let size = (w.min(h) * 0.13).clamp(12.0, 20.0);
    let bx = x + 8.0 + slot as f64 * (size + 4.0);
    let by = y + 8.0;
    ctx.set_source_rgba(fill.0, fill.1, fill.2, fill.3);
    rounded_rect(ctx, bx, by, size, size, size * 0.28);
    ctx.fill().ok();
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    ctx.set_font_size(size * 0.62);
    if let Ok(ext) = ctx.text_extents(glyph) {
        ctx.set_source_rgba(ink.0, ink.1, ink.2, 0.92);
        ctx.move_to(
            bx + (size - ext.width()) / 2.0 - ext.x_bearing(),
            by + size * 0.76,
        );
        ctx.show_text(glyph).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::xml::scalar::CrGuid;

    fn book() -> ComicBook {
        let mut b = ComicBook {
            id: CrGuid::from_bytes([5; 16]),
            ..Default::default()
        };
        b.info.writer = "Grant Morrison, Frank Quitely".into();
        b.info.penciller = "Frank Quitely".into();
        b
    }

    #[test]
    fn artist_info_dedups_and_joins_with_slash() {
        assert_eq!(artist_info(&book()), "Grant Morrison/Frank Quitely");
    }

    #[test]
    fn tile_lines_follow_the_default_file_comic_order() {
        let mut b = book();
        b.info.title = "The Cape".into();
        b.info.series = "Batman".into();
        b.info.number = "1".into();
        b.info.summary = "A summary.".into();
        let lines = tile_text_lines(&b);
        assert_eq!(lines[0].0, "Batman #1");
        assert_eq!(lines[1].0, "The Cape");
        assert_eq!(lines[2].0, "Grant Morrison/Frank Quitely");
        // Spacers + the wrapped summary, then the tab lines.
        assert!(lines[4].0 == "A summary.");
        // The size line carries the tab marker.
        assert!(lines[6].0.starts_with("Size:\t"));
        // No summary: the summary lines drop entirely.
        b.info.summary = String::new();
        let lines = tile_text_lines(&b);
        assert!(lines[4].0.starts_with("Size:\t"));
    }

    #[test]
    fn metadata_missing_tracks_the_key_fields() {
        // A file-backed comic with all descriptive fields empty.
        let empty = ComicBook {
            file_path: "/comics/magazine 2024-05.cbz".into(),
            ..Default::default()
        };
        assert!(metadata_missing(&empty));
        // Any key field filled clears the tag.
        let fills: [fn(&mut ComicBook); 7] = [
            |b| b.info.series = "S".into(),
            |b| b.info.title = "T".into(),
            |b| b.info.number = "1".into(),
            |b| b.info.volume = 2,
            |b| b.info.writer = "W".into(),
            |b| b.info.publisher = "P".into(),
            |b| b.info.summary = "Sum".into(),
        ];
        for fill in fills {
            let mut b = ComicBook {
                file_path: "/comics/x.cbz".into(),
                ..Default::default()
            };
            fill(&mut b);
            assert!(!metadata_missing(&b));
        }
        // Fileless and missing-file books carry their own markers.
        let fileless = ComicBook::default();
        assert!(!metadata_missing(&fileless));
        let missing = ComicBook {
            file_path: "/comics/gone.cbz".into(),
            file_is_missing: true,
            ..Default::default()
        };
        assert!(!metadata_missing(&missing));
    }

    /// A book with metadata, so only the scan chip can show.
    fn scanned(status: cr_core::scan_status::ScanStatus, error: &str) -> ComicBook {
        let mut book = ComicBook {
            file_path: "/comics/x.cbz".into(),
            ..Default::default()
        };
        book.info.series = "Blacksad".into();
        cr_core::scan_status::apply(
            &mut book,
            &cr_core::scan_status::ScanVerdict {
                status: Some(status),
                error: Some(error.to_string()),
                ..Default::default()
            },
            "2026-09-12",
        );
        book
    }

    #[test]
    fn a_book_with_no_chip_has_no_tooltip() {
        let mut clean = ComicBook {
            file_path: "/comics/x.cbz".into(),
            ..Default::default()
        };
        clean.info.series = "Blacksad".into();
        assert_eq!(chip_tooltip(&clean), None);
    }

    #[test]
    fn the_no_metadata_chip_explains_itself() {
        // It used to explain NOTHING: the tooltip read the scan
        // status only, and this book has none.
        let empty = ComicBook {
            file_path: "/comics/magazine 2024-05.cbz".into(),
            ..Default::default()
        };
        let text = chip_tooltip(&empty).expect("the ? chip has a tooltip");
        assert!(text.starts_with("?  No metadata"), "{text}");
        assert!(text.contains("Properties"), "{text}");
    }

    #[test]
    fn the_scan_chip_keeps_its_reason_and_gains_its_glyph() {
        let book = scanned(
            cr_core::scan_status::ScanStatus::Unreadable,
            "bad central directory",
        );
        let text = chip_tooltip(&book).expect("the ! chip has a tooltip");
        assert!(text.starts_with("!  Unreadable"), "{text}");
        assert!(text.contains("bad central directory"), "{text}");
    }

    #[test]
    fn a_book_with_both_chips_explains_both() {
        // The defect the user found: only the "!" reason showed.
        let mut book = scanned(
            cr_core::scan_status::ScanStatus::Unreadable,
            "unreadable header",
        );
        // Strip the metadata back out, so both chips apply.
        book.info.series = String::new();
        assert!(metadata_missing(&book), "the ? chip applies");
        assert!(scan_marker(&book).is_some(), "the ! chip applies");

        let text = chip_tooltip(&book).expect("both chips have a tooltip");
        // Both reasons, in the order the chips are drawn.
        let question = text.find("?  No metadata").expect("the ? reason is there");
        let bang = text.find("!  Unreadable").expect("the ! reason is there");
        assert!(question < bang, "the ? chip is drawn first: {text}");
        assert!(text.contains("unreadable header"), "{text}");
        // A blank line keeps the two reasons apart.
        assert!(text.contains("\n\n"), "{text}");
    }

    #[test]
    fn the_mismatch_chip_carries_its_own_glyph() {
        let mut book = scanned(cr_core::scan_status::ScanStatus::FormatMismatch, "");
        cr_core::scan_status::apply(
            &mut book,
            &cr_core::scan_status::ScanVerdict {
                status: Some(cr_core::scan_status::ScanStatus::FormatMismatch),
                detected_format: Some("rar".into()),
                expected_format: Some("zip".into()),
                ..Default::default()
            },
            "2026-09-12",
        );
        let text = chip_tooltip(&book).expect("the mismatch chip has a tooltip");
        assert!(text.starts_with("\u{2260}  Format mismatch"), "{text}");
        assert!(text.contains("Content is rar, the name says zip"), "{text}");
    }
}
