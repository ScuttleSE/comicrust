//! The Quick Rating dialog — the `Dialogs/QuickRatingDialog.cs` port.
//!
//! The C# dialog: the front-cover thumbnail (left), the review text
//! (right), the star RatingControl (bottom, with the numeric text),
//! the "Show when Book read" checkbox, OK/Cancel. `Show` applies the
//! rating + review to the book and stores `AutoShowQuickReview` on
//! OK only.
//!
//! Deviations (recorded): the star image control becomes a Scale
//! (0..5, half steps, value text shown); the cover loads through the
//! thumbnail queue (the C# `GetThumbnail` + slow-queue shape) and
//! arrives async.

use std::path::Path;
use std::sync::Arc;

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, CheckButton, Dialog, Frame, Label, Orientation, Picture, ScrolledWindow, TextView,
    WrapMode,
};

use cr_core::model::comic_book::ComicBook;
use cr_engine::image_pool::ImagePool;

/// The OK result (the C# writes the fields onto the book + the
/// setting on OK only).
pub struct QuickRatingResult {
    pub rating: f32,
    pub review: String,
    pub show_when_read: bool,
}

/// Opens the modal Quick Rating dialog for `book` (`Show(parent,
/// book)`); `on_done` receives the OK fields or None on Cancel.
pub fn show_quick_rating(
    parent: &impl IsA<gtk4::Window>,
    book: &ComicBook,
    show_when_read: bool,
    pool: Arc<ImagePool>,
    on_done: impl Fn(Option<QuickRatingResult>) + 'static,
) {
    // `Text = "Quick Rating - {CaptionWithoutTitle}"`.
    let dialog = Dialog::builder()
        .title(format!(
            "Quick Rating - {}",
            cr_engine::display_text::caption_without_title(book)
        ))
        .transient_for(parent)
        .modal(true)
        .default_width(570)
        .default_height(310)
        .build();
    let content = dialog.content_area();
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_spacing(10);

    let columns = gtk4::Box::new(Orientation::Horizontal, 10);

    // The cover thumbnail (the C# ThumbnailControl 170x227, 3D
    // frame). The image letterboxes inside the fixed box.
    let cover = Picture::new();
    cover.set_size_request(170, 227);
    cover.set_can_shrink(true);
    cover.set_halign(Align::Start);
    let cover_frame = Frame::new(None);
    cover_frame.set_child(Some(&cover));
    cover_frame.set_size_request(170, 227);
    cover_frame.set_valign(Align::Start);
    columns.append(&cover_frame);

    // The review (the C# TextBoxEx, multiline, vertical scroll).
    let review = TextView::new();
    review.set_wrap_mode(WrapMode::Word);
    review.buffer().set_text(&book.info.review);
    let review_scroll = ScrolledWindow::new();
    review_scroll.set_child(Some(&review));
    review_scroll.set_vexpand(true);
    review_scroll.set_hexpand(true);
    columns.append(&review_scroll);
    content.append(&columns);

    // The rating row (the C# RatingControl: the star slider with the
    // numeric text; the Scale shows the value instead — the recorded
    // deviation).
    let rating_row = gtk4::Box::new(Orientation::Horizontal, 8);
    let rating = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 5.0, 0.5);
    rating.set_digits(1);
    rating.set_draw_value(true);
    rating.set_value(book.rating as f64);
    rating.set_hexpand(true);
    rating_row.append(&Label::new(Some("Rating:")));
    rating_row.append(&rating);
    content.append(&rating_row);

    // The auto-show flag (the C# chkShow; the shell stores it on OK).
    let show_check = CheckButton::with_label("Show when Book read");
    show_check.set_active(show_when_read);
    content.append(&show_check);

    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    let ok = dialog.add_button("OK", gtk4::ResponseType::Ok);
    dialog.set_default_widget(Some(&ok));

    // The cover load: the front-cover page (`GetThumbnailKey(
    // FrontCoverPageIndex)`) through the thumbnail queue; the pump
    // applies the result while the dialog lives.
    let (rx, has_pending) = request_cover(&pool, book);
    let rx = std::rc::Rc::new(rx);
    {
        let cover = cover.clone();
        let rx = std::rc::Rc::clone(&rx);
        glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
            let received = rx.try_recv();
            match received {
                Ok(Some(bytes)) => {
                    if let Some(texture) = texture_from_thumb_blob(&bytes) {
                        cover.set_paintable(Some(&texture));
                    }
                    glib::ControlFlow::Break
                }
                Ok(None) => {
                    // No thumbnail: the C# leaves the empty control.
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            }
        });
    }
    let _ = has_pending;

    dialog.connect_response(move |dlg, response| {
        let result = match response {
            gtk4::ResponseType::Ok => {
                let (start, end) = review.buffer().bounds();
                Some(QuickRatingResult {
                    rating: rating.value() as f32,
                    review: review.buffer().text(&start, &end, true).to_string(),
                    show_when_read: show_check.is_active(),
                })
            }
            _ => None,
        };
        dlg.close();
        on_done(result);
    });
    dialog.present();
}

/// Queues the front-cover thumbnail (`ThumbnailKey` of the cover
/// page); the worker renders through the pool chain. Returns the
/// result receiver.
fn request_cover(
    pool: &Arc<ImagePool>,
    book: &ComicBook,
) -> (std::sync::mpsc::Receiver<Option<Vec<u8>>>, bool) {
    let (tx, rx) = std::sync::mpsc::channel::<Option<Vec<u8>>>();
    let cover_index = book.info.front_cover_page_index().max(0) as usize;
    let path = book.file_path.clone();
    let key = cr_image::keys::ThumbnailKey::new(cr_image::keys::ImageKey::from_file(
        path.clone(),
        Path::new(&path),
        cover_index,
        cr_core::model::enums::ImageRotation::None,
    ));
    let pool2 = Arc::clone(pool);
    let pool_cb = Arc::clone(&pool2);
    pool2.add_thumb_to_queue(key, None, move |k| {
        let _ = tx.send(pool_cb.render_thumbnail(k));
    });
    (rx, true)
}

/// The thumbnail blob (`ThumbnailImage` serialization — parse before
/// decoding, the Phase 5 lesson) into a GDK texture.
fn texture_from_thumb_blob(bytes: &[u8]) -> Option<gdk::MemoryTexture> {
    let mut surface = crate::bitmap::surface_from_thumb_blob(bytes)?;
    let width = surface.width();
    let height = surface.height();
    let stride = surface.stride();
    let data = surface.data().ok()?;
    Some(gdk::MemoryTexture::new(
        width,
        height,
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &glib::Bytes::from(data.as_ref()),
        stride as usize,
    ))
}

/// The auto-show gate of the C# `OnBookClosing`
/// (`AutoShowQuickReview && HasBeenRead && Rating == 0`).
pub fn should_auto_show(book: &ComicBook, auto_show_quick_review: bool) -> bool {
    auto_show_quick_review
        && cr_engine::matcher::book_view::has_been_read(book)
        && book.rating == 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_show_needs_setting_read_and_unrated() {
        let mut book = ComicBook::default();
        book.info.page_count = 10;
        // Not read: no dialog.
        assert!(!should_auto_show(&book, true));
        // Read but rated: no dialog.
        book.last_page_read = 9;
        book.rating = 3.0;
        assert!(!should_auto_show(&book, true));
        // Read + unrated + the setting: the dialog shows.
        book.rating = 0.0;
        assert!(should_auto_show(&book, true));
        // The setting off: never.
        assert!(!should_auto_show(&book, false));
    }
}
