//! Side-by-side comparison for selected Incoming books and their duplicates.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use cr_core::model::comic_book::ComicBook;
use cr_engine::image_pool::{front_cover_thumbnail_key, ImagePool};
use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchSource {
    Incoming,
    Library,
}

impl MatchSource {
    fn label(self) -> &'static str {
        match self {
            Self::Incoming => "Incoming duplicate",
            Self::Library => "Library duplicate",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DuplicateMatch {
    pub source: MatchSource,
    pub book: ComicBook,
}

#[derive(Clone, Debug)]
pub struct BookComparison {
    pub selected: ComicBook,
    pub matches: Vec<DuplicateMatch>,
}

/// Builds the stable comparison sequence used by the dialog.
pub fn build_comparisons(
    selected: &[ComicBook],
    incoming_books: &[ComicBook],
    library_books: &[ComicBook],
) -> Vec<BookComparison> {
    let all: Vec<&ComicBook> = incoming_books.iter().chain(library_books).collect();
    let groups = cr_engine::matcher::eval::grouped_duplicate_indexes(&all);
    let incoming_count = incoming_books.len();
    selected
        .iter()
        .map(|selected| {
            let group = incoming_books
                .iter()
                .position(|candidate| candidate.id == selected.id)
                .and_then(|index| groups.iter().find(|group| group.contains(&index)));
            let mut matches = Vec::new();
            if let Some(group) = group {
                for index in group {
                    if *index < incoming_count {
                        if let Some(book) = incoming_books
                            .get(*index)
                            .filter(|candidate| candidate.id != selected.id)
                        {
                            matches.push(DuplicateMatch {
                                source: MatchSource::Incoming,
                                book: book.clone(),
                            });
                        }
                    } else if let Some(book) = library_books.get(*index - incoming_count) {
                        matches.push(DuplicateMatch {
                            source: MatchSource::Library,
                            book: book.clone(),
                        });
                    }
                }
            }
            BookComparison {
                selected: selected.clone(),
                matches,
            }
        })
        .collect()
}

struct Pane {
    heading: gtk4::Label,
    cover_stack: gtk4::Stack,
    picture: gtk4::Picture,
    cover_status: gtk4::Label,
    details: gtk4::Label,
}

struct DialogState {
    comparisons: Vec<BookComparison>,
    book_index: Cell<usize>,
    match_indexes: RefCell<Vec<usize>>,
    generation: Cell<u64>,
    pool: Arc<ImagePool>,
    left: Pane,
    right: Pane,
    book_position: gtk4::Label,
    match_position: gtk4::Label,
    previous_book: gtk4::Button,
    next_book: gtk4::Button,
    previous_match: gtk4::Button,
    next_match: gtk4::Button,
}

#[derive(Clone, Copy)]
enum CoverSide {
    Left,
    Right,
}

struct CoverResult {
    generation: u64,
    side: CoverSide,
    bytes: Option<Vec<u8>>,
}

pub fn show(
    parent: &impl IsA<gtk4::Window>,
    pool: Arc<ImagePool>,
    comparisons: Vec<BookComparison>,
) {
    if comparisons.is_empty() {
        return;
    }
    let window = gtk4::Window::builder()
        .title("Compare Incoming")
        .transient_for(parent)
        .modal(true)
        .default_width(920)
        .default_height(680)
        .build();

    let previous_book = gtk4::Button::with_label("Previous Book");
    let next_book = gtk4::Button::with_label("Next Book");
    let book_position = gtk4::Label::new(None);
    let book_nav = navigation_row(&previous_book, &book_position, &next_book);

    let left = comparison_pane("Selected Incoming book");
    let right = comparison_pane("Matching duplicate");
    let grid = gtk4::Grid::builder()
        .column_spacing(24)
        .column_homogeneous(true)
        .hexpand(true)
        .build();
    grid.attach(&pane_widget(&left), 0, 0, 1, 1);
    grid.attach(&pane_widget(&right), 1, 0, 1, 1);

    let previous_match = gtk4::Button::with_label("Previous Match");
    let next_match = gtk4::Button::with_label("Next Match");
    let match_position = gtk4::Label::new(None);
    let match_nav = navigation_row(&previous_match, &match_position, &next_match);

    let close = gtk4::Button::with_label("Close");
    close.set_halign(gtk4::Align::End);
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&book_nav);
    content.append(&grid);
    content.append(&match_nav);
    content.append(&close);
    let scroll = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Automatic)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .child(&content)
        .build();
    window.set_child(Some(&scroll));

    let (cover_tx, cover_rx) = std::sync::mpsc::channel();
    let state = Rc::new(DialogState {
        match_indexes: RefCell::new(vec![0; comparisons.len()]),
        comparisons,
        book_index: Cell::new(0),
        generation: Cell::new(0),
        pool,
        left,
        right,
        book_position,
        match_position,
        previous_book,
        next_book,
        previous_match,
        next_match,
    });
    update_dialog(&state, &cover_tx);

    connect_navigation(&state, &cover_tx);
    {
        let window = window.clone();
        close.connect_clicked(move |_| window.close());
    }
    let state = Rc::clone(&state);
    let weak_window = window.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
        let Some(window) = weak_window.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if !window.is_visible() {
            return glib::ControlFlow::Break;
        }
        while let Ok(result) = cover_rx.try_recv() {
            if result.generation != state.generation.get() {
                continue;
            }
            let pane = match result.side {
                CoverSide::Left => &state.left,
                CoverSide::Right => &state.right,
            };
            if let Some(texture) = result.bytes.as_deref().and_then(texture_from_thumb_blob) {
                pane.picture.set_paintable(Some(&texture));
                pane.cover_stack.set_visible_child(&pane.picture);
            } else {
                pane.cover_status.set_text("No cover available");
                pane.cover_stack.set_visible_child(&pane.cover_status);
            }
        }
        glib::ControlFlow::Continue
    });
    window.present();
}

fn navigation_row(
    previous: &gtk4::Button,
    position: &gtk4::Label,
    next: &gtk4::Button,
) -> gtk4::Box {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    row.set_halign(gtk4::Align::Center);
    row.append(previous);
    row.append(position);
    row.append(next);
    row
}

fn comparison_pane(heading: &str) -> Pane {
    let heading = gtk4::Label::new(Some(heading));
    heading.add_css_class("title-3");
    heading.set_halign(gtk4::Align::Start);
    let picture = gtk4::Picture::new();
    picture.set_content_fit(gtk4::ContentFit::Contain);
    picture.set_size_request(260, 360);
    let cover_status = gtk4::Label::new(Some("Loading cover..."));
    cover_status.set_size_request(260, 360);
    let cover_stack = gtk4::Stack::new();
    cover_stack.add_child(&picture);
    cover_stack.add_child(&cover_status);
    cover_stack.set_visible_child(&cover_status);
    let details = gtk4::Label::new(None);
    details.set_halign(gtk4::Align::Start);
    details.set_valign(gtk4::Align::Start);
    details.set_xalign(0.0);
    details.set_wrap(true);
    details.set_selectable(true);
    Pane {
        heading,
        cover_stack,
        picture,
        cover_status,
        details,
    }
}

fn pane_widget(pane: &Pane) -> gtk4::Box {
    let box_ = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    box_.set_hexpand(true);
    box_.append(&pane.heading);
    box_.append(&pane.cover_stack);
    box_.append(&pane.details);
    box_
}

fn connect_navigation(state: &Rc<DialogState>, cover_tx: &std::sync::mpsc::Sender<CoverResult>) {
    {
        let button = state.previous_book.clone();
        let state = Rc::downgrade(state);
        let cover_tx = cover_tx.clone();
        button.connect_clicked(move |_| {
            let Some(state) = state.upgrade() else {
                return;
            };
            state
                .book_index
                .set(state.book_index.get().saturating_sub(1));
            update_dialog(&state, &cover_tx);
        });
    }
    {
        let button = state.next_book.clone();
        let state = Rc::downgrade(state);
        let cover_tx = cover_tx.clone();
        button.connect_clicked(move |_| {
            let Some(state) = state.upgrade() else {
                return;
            };
            let last = state.comparisons.len().saturating_sub(1);
            state.book_index.set((state.book_index.get() + 1).min(last));
            update_dialog(&state, &cover_tx);
        });
    }
    {
        let button = state.previous_match.clone();
        let state = Rc::downgrade(state);
        let cover_tx = cover_tx.clone();
        button.connect_clicked(move |_| {
            let Some(state) = state.upgrade() else {
                return;
            };
            let book_index = state.book_index.get();
            let mut indexes = state.match_indexes.borrow_mut();
            indexes[book_index] = indexes[book_index].saturating_sub(1);
            drop(indexes);
            update_dialog(&state, &cover_tx);
        });
    }
    {
        let button = state.next_match.clone();
        let state = Rc::downgrade(state);
        let cover_tx = cover_tx.clone();
        button.connect_clicked(move |_| {
            let Some(state) = state.upgrade() else {
                return;
            };
            let book_index = state.book_index.get();
            let last = state.comparisons[book_index]
                .matches
                .len()
                .saturating_sub(1);
            let mut indexes = state.match_indexes.borrow_mut();
            indexes[book_index] = (indexes[book_index] + 1).min(last);
            drop(indexes);
            update_dialog(&state, &cover_tx);
        });
    }
}

fn update_dialog(state: &Rc<DialogState>, cover_tx: &std::sync::mpsc::Sender<CoverResult>) {
    let book_index = state.book_index.get();
    let comparison = &state.comparisons[book_index];
    let match_index = state.match_indexes.borrow()[book_index];
    let generation = state.generation.get().wrapping_add(1);
    state.generation.set(generation);

    state.book_position.set_text(&format!(
        "Book {} of {}",
        book_index + 1,
        state.comparisons.len()
    ));
    state.previous_book.set_sensitive(book_index > 0);
    state
        .next_book
        .set_sensitive(book_index + 1 < state.comparisons.len());
    state.left.heading.set_text("Selected Incoming book");
    set_book(&state.left, &comparison.selected);
    queue_cover(
        state,
        CoverSide::Left,
        &comparison.selected,
        generation,
        cover_tx,
    );

    if let Some(duplicate) = comparison.matches.get(match_index) {
        state.right.heading.set_text(duplicate.source.label());
        set_book(&state.right, &duplicate.book);
        state.match_position.set_text(&format!(
            "Match {} of {}",
            match_index + 1,
            comparison.matches.len()
        ));
        state.previous_match.set_sensitive(match_index > 0);
        state
            .next_match
            .set_sensitive(match_index + 1 < comparison.matches.len());
        queue_cover(
            state,
            CoverSide::Right,
            &duplicate.book,
            generation,
            cover_tx,
        );
    } else {
        state.right.heading.set_text("No matching duplicate");
        state.right.details.set_text("");
        state.right.picture.set_paintable(gdk::Paintable::NONE);
        state.right.cover_status.set_text("No matching duplicate");
        state
            .right
            .cover_stack
            .set_visible_child(&state.right.cover_status);
        state.match_position.set_text("No matches");
        state.previous_match.set_sensitive(false);
        state.next_match.set_sensitive(false);
    }
}

fn set_book(pane: &Pane, book: &ComicBook) {
    pane.details.set_text(&book_details(book));
    pane.picture.set_paintable(gdk::Paintable::NONE);
    pane.cover_status.set_text("Loading cover...");
    pane.cover_stack.set_visible_child(&pane.cover_status);
}

fn book_details(book: &ComicBook) -> String {
    let published = match (book.info.year, book.info.month, book.info.day) {
        (year, month, day) if year > 0 && month > 0 && day > 0 => {
            format!("{year:04}-{month:02}-{day:02}")
        }
        (year, month, _) if year > 0 && month > 0 => format!("{year:04}-{month:02}"),
        (year, _, _) if year > 0 => year.to_string(),
        _ => "Unknown".to_string(),
    };
    format!(
        "{}\nSeries: {}\nVolume: {}\nNumber: {}\nPages: {}\nFile size: {}\nPublished: {}\nPath: {}",
        cr_engine::display_text::caption(book),
        book.info.series,
        book.info.volume,
        book.info.number,
        book.info.page_count,
        cr_engine::display_text::file_size_as_text(book.file_size),
        published,
        book.file_path
    )
}

fn queue_cover(
    state: &Rc<DialogState>,
    side: CoverSide,
    book: &ComicBook,
    generation: u64,
    cover_tx: &std::sync::mpsc::Sender<CoverResult>,
) {
    if book.file_path.is_empty() && book.custom_thumbnail_key.is_none() {
        let _ = cover_tx.send(CoverResult {
            generation,
            side,
            bytes: None,
        });
        return;
    }
    let key = front_cover_thumbnail_key(book);
    let pool = Arc::clone(&state.pool);
    let render = Arc::clone(&pool);
    let tx = cover_tx.clone();
    pool.add_thumb_to_queue(key, None, move |key| {
        let _ = tx.send(CoverResult {
            generation,
            side,
            bytes: render.render_thumbnail(key),
        });
    });
}

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
        &glib::Bytes::from(&*data),
        stride as usize,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::xml::scalar::CrGuid;

    fn book(id: u8, series: &str, number: &str, path: &str) -> ComicBook {
        let mut book = ComicBook {
            id: CrGuid::parse(&format!("00000000-0000-0000-0000-{id:012}")).unwrap(),
            file_path: path.into(),
            ..Default::default()
        };
        book.info.series = series.into();
        book.info.number = number.into();
        book.info.year = 2026;
        book
    }

    #[test]
    fn comparison_model_keeps_selection_match_source_and_order() {
        let first = book(1, "Alpha", "1", "/incoming/a.cbz");
        let incoming_match = book(2, "Alpha", "1", "/incoming/b.cbz");
        let library_match = book(3, "Alpha", "1", "/library/a.cbz");
        let second = book(4, "Beta", "1", "/incoming/c.cbz");
        let library_second = book(5, "Beta", "1", "/library/b.cbz");

        let comparisons = build_comparisons(
            &[second.clone(), first.clone()],
            &[first, incoming_match, second],
            &[library_match, library_second],
        );

        assert_eq!(comparisons[0].selected.id, fixed(4));
        assert_eq!(comparisons[0].matches.len(), 1);
        assert_eq!(comparisons[0].matches[0].source, MatchSource::Library);
        assert_eq!(comparisons[1].selected.id, fixed(1));
        assert_eq!(comparisons[1].matches.len(), 2);
        assert_eq!(comparisons[1].matches[0].source, MatchSource::Incoming);
        assert_eq!(comparisons[1].matches[0].book.id, fixed(2));
        assert_eq!(comparisons[1].matches[1].source, MatchSource::Library);
        assert_eq!(comparisons[1].matches[1].book.id, fixed(3));
    }

    #[test]
    fn comparison_model_excludes_self_and_keeps_no_match_selections() {
        let selected = book(1, "Alpha", "1", "/incoming/a.cbz");
        let unmatched = book(2, "Beta", "1", "/incoming/b.cbz");

        let comparisons = build_comparisons(
            &[selected.clone(), unmatched.clone()],
            &[selected, unmatched],
            &[],
        );

        assert_eq!(comparisons.len(), 2);
        assert!(comparisons
            .iter()
            .all(|comparison| comparison.matches.is_empty()));
    }

    fn fixed(id: u8) -> CrGuid {
        CrGuid::parse(&format!("00000000-0000-0000-0000-{id:012}")).unwrap()
    }
}
