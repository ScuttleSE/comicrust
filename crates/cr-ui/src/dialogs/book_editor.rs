//! The book editor — the `Dialogs/ComicBookDialog.cs` port.
//!
//! Layout: a cover thumbnail column (front-cover page, info labels,
//! prev/next for multi-book edits) beside a notebook of Details /
//! Plot / Catalog / Pages / Colors / Custom.
//!
//! Commit semantics are the C# compiled behavior: the widgets write
//! into the working book at the SAVE POINTS only — Apply, OK, and
//! prev/next navigation — and the caller's `on_commit` callback then
//! applies the clone into the library. Cancel closes without saving
//! the current edits; edits already committed at earlier save points
//! stay (the C# edits the live database books, so Cancel never
//! reverts anything).
//!
//! Deferred within T2 (recorded in `docs/phase-5-kickoff.md`): the
//! custom-thumbnail set/clear buttons (the pool `type://` loader),
//! the white-point color pick (double-click), the file write-back
//! (checkpoint 2: the write-info queue), the library-wide custom
//! value keys, and the script button (Phase 6).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, ComboBoxText, Dialog, DrawingArea, Entry, Grid, Label, Notebook,
    Orientation, ScrolledWindow, TextView,
};

use cr_core::model::bitmap_adjustment::BitmapAdjustment;
use cr_core::model::comic_book::ComicBook;
use cr_core::model::comic_page_info::ComicPageInfo;
use cr_core::model::enums::{ComicPagePosition, ComicPageType, ImageRotation, MangaYesNo, YesNo};
use cr_core::registry::{self, PropValue};
use cr_engine::image_pool::ImagePool;

use gtk4::cairo;
use gtk4::gdk;
use gtk4::glib;

// ---------- Pure helpers (the `EditControlUtility` parity) ----------

/// `EditControlUtility.GetNumber`: parse int; negative or unparseable
/// (empty included) → -1 (the unset value).
pub fn number_from_text(text: &str) -> i32 {
    match text.trim().parse::<i32>() {
        Ok(v) if v >= 0 => v,
        _ => -1,
    }
}

/// `EditControlUtility.GetRealNumber`.
pub fn real_from_text(text: &str) -> f32 {
    match text.trim().parse::<f32>() {
        Ok(v) if v >= 0.0 => v,
        _ => -1.0,
    }
}

/// The YesNo combo rows (`InitializeYesNo`): Unknown / Yes / No.
pub const YES_NO_ITEMS: [&str; 3] = ["Unknown", "Yes", "No"];
/// The MangaYesNo combo rows (`InitializeMangaYesNo`).
pub const MANGA_ITEMS: [&str; 4] = ["Unknown", "No", "Yes", "Yes and Right to Left"];
/// The EnableProposed combo (`withEmpty: false`): No / Yes.
pub const PROPOSED_ITEMS: [&str; 2] = ["No", "Yes"];

pub fn yesno_from_combo(index: Option<u32>) -> YesNo {
    match index {
        Some(1) => YesNo::Yes,
        Some(2) => YesNo::No,
        _ => YesNo::Unknown,
    }
}

pub fn manga_from_combo(index: Option<u32>) -> MangaYesNo {
    match index {
        Some(1) => MangaYesNo::No,
        Some(2) => MangaYesNo::Yes,
        Some(3) => MangaYesNo::YesAndRightToLeft,
        _ => MangaYesNo::Unknown,
    }
}

pub fn combo_index_of_yesno(v: YesNo) -> u32 {
    match v {
        YesNo::Yes => 1,
        YesNo::No => 2,
        YesNo::Unknown => 0,
    }
}

pub fn combo_index_of_manga(v: MangaYesNo) -> u32 {
    match v {
        MangaYesNo::No => 1,
        MangaYesNo::Yes => 2,
        MangaYesNo::YesAndRightToLeft => 3,
        MangaYesNo::Unknown => 0,
    }
}

/// A rating field: parse + clamp; an unparseable text keeps the
/// current value (the C# star control cannot hold invalid text).
fn rating_from_text(text: &str, current: f32) -> f32 {
    match text.trim().parse::<f32>() {
        Ok(v) => v.clamp(0.0, 5.0),
        Err(_) => current,
    }
}

/// A date field: `YYYY-MM-DD`; an unparseable text keeps the current
/// value (the C# DateTimePicker never produces invalid text).
fn date_from_text(
    text: &str,
    current: cr_core::xml::scalar::CrDateTime,
) -> cr_core::xml::scalar::CrDateTime {
    if let Ok(d) = chrono::NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d") {
        if let Some(naive) = d.and_hms_opt(0, 0, 0) {
            return cr_core::xml::scalar::CrDateTime {
                naive,
                kind: current.kind,
            };
        }
    }
    current
}

/// The proposed-value placeholders (`SetPromptTexts`): the filename
/// parse feeds the gray cue text of the seven proposed fields while
/// `EnableProposed` is Yes.
pub fn proposed_placeholders(book: &ComicBook) -> Vec<(&'static str, String)> {
    let p = cr_engine::matcher::book_view::proposed(book);
    let num = |v: i32| if v > 0 { v.to_string() } else { String::new() };
    vec![
        ("Series", p.series.clone()),
        ("Title", p.title.clone()),
        ("Number", p.number.clone()),
        ("Count", num(p.count)),
        ("Year", num(p.year)),
        ("Volume", num(p.volume)),
        ("Format", p.format.clone()),
    ]
}

#[cfg(test)]
mod pure_tests {
    use super::*;

    #[test]
    fn number_parsing_matches_getnumber() {
        assert_eq!(number_from_text(""), -1);
        assert_eq!(number_from_text("12"), 12);
        assert_eq!(number_from_text(" 7 "), 7);
        assert_eq!(number_from_text("-3"), -1);
        assert_eq!(number_from_text("abc"), -1);
    }

    #[test]
    fn real_parsing_matches_getrealnumber() {
        assert_eq!(real_from_text(""), -1.0);
        assert_eq!(real_from_text("9.99"), 9.99);
        assert_eq!(real_from_text("-1"), -1.0);
    }

    #[test]
    fn combo_mappings_cover_the_members() {
        assert_eq!(yesno_from_combo(Some(0)), YesNo::Unknown);
        assert_eq!(yesno_from_combo(Some(2)), YesNo::No);
        assert_eq!(manga_from_combo(Some(3)), MangaYesNo::YesAndRightToLeft);
        assert_eq!(combo_index_of_yesno(YesNo::Yes), 1);
        assert_eq!(combo_index_of_manga(MangaYesNo::YesAndRightToLeft), 3);
    }

    #[test]
    fn rating_and_date_parse_leniently() {
        let cur = cr_core::xml::scalar::CrDateTime::min_value();
        assert_eq!(rating_from_text("4.5", 1.0), 4.5);
        assert_eq!(rating_from_text("bogus", 1.0), 1.0);
        assert_eq!(rating_from_text("9", 1.0), 5.0);
        let d = date_from_text("2020-03-05", cur);
        assert_eq!(d.naive.date().to_string(), "2020-03-05");
        assert_eq!(date_from_text("bogus", cur), cur);
    }

    #[test]
    fn placeholders_carry_the_seven_proposed_fields() {
        let b = ComicBook {
            file_path: "/comics/Batman 003 (2016).cbz".into(),
            enable_proposed: true,
            ..ComicBook::default()
        };
        let ph = proposed_placeholders(&b);
        let names: Vec<&str> = ph.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            vec!["Series", "Title", "Number", "Count", "Year", "Volume", "Format"]
        );
    }
}

// ---------- Widget tables ----------

/// A labeled text row bound to a registry property (the save writes
/// the TRIMMED text unconditionally — `GetText(control, comic.X)`).
struct TextRow {
    caption: &'static str,
    property: &'static str,
}

const DETAIL_ROWS: [TextRow; 14] = [
    TextRow {
        caption: "Title",
        property: "Title",
    },
    TextRow {
        caption: "Series",
        property: "Series",
    },
    TextRow {
        caption: "Number",
        property: "Number",
    },
    TextRow {
        caption: "Alternate Series",
        property: "AlternateSeries",
    },
    TextRow {
        caption: "Alternate Number",
        property: "AlternateNumber",
    },
    TextRow {
        caption: "Story Arc",
        property: "StoryArc",
    },
    TextRow {
        caption: "Series Group",
        property: "SeriesGroup",
    },
    TextRow {
        caption: "Writer",
        property: "Writer",
    },
    TextRow {
        caption: "Penciller",
        property: "Penciller",
    },
    TextRow {
        caption: "Inker",
        property: "Inker",
    },
    TextRow {
        caption: "Colorist",
        property: "Colorist",
    },
    TextRow {
        caption: "Letterer",
        property: "Letterer",
    },
    TextRow {
        caption: "Cover Artist",
        property: "CoverArtist",
    },
    TextRow {
        caption: "Editor",
        property: "Editor",
    },
];

const DETAIL_ROWS_2: [TextRow; 5] = [
    TextRow {
        caption: "Translator",
        property: "Translator",
    },
    TextRow {
        caption: "Genre",
        property: "Genre",
    },
    TextRow {
        caption: "Tags",
        property: "Tags",
    },
    TextRow {
        caption: "Language",
        property: "LanguageISO",
    },
    TextRow {
        caption: "Web Link",
        property: "Web",
    },
];

const NUM_ROWS: [(&str, &str); 5] = [
    ("Volume", "Volume"),
    ("Count", "Count"),
    ("Year", "Year"),
    ("Month", "Month"),
    ("Day", "Day"),
];

const CATALOG_ROWS: [TextRow; 9] = [
    TextRow {
        caption: "Book Age",
        property: "BookAge",
    },
    TextRow {
        caption: "Book Store",
        property: "BookStore",
    },
    TextRow {
        caption: "Book Owner",
        property: "BookOwner",
    },
    TextRow {
        caption: "Book Condition",
        property: "BookCondition",
    },
    TextRow {
        caption: "Book Price",
        property: "BookPrice",
    },
    TextRow {
        caption: "Book Location",
        property: "BookLocation",
    },
    TextRow {
        caption: "Collection Status",
        property: "BookCollectionStatus",
    },
    TextRow {
        caption: "Book Notes",
        property: "BookNotes",
    },
    TextRow {
        caption: "ISBN",
        property: "ISBN",
    },
];

const PLOT_ROWS: [TextRow; 4] = [
    TextRow {
        caption: "Characters",
        property: "Characters",
    },
    TextRow {
        caption: "Teams",
        property: "Teams",
    },
    TextRow {
        caption: "Main Character or Team",
        property: "MainCharacterOrTeam",
    },
    TextRow {
        caption: "Locations",
        property: "Locations",
    },
];

/// The single-value page-type list (the C# `EnumMenuUtility
/// (flagsMode: false)` menu omits the composite members).
const PAGE_TYPE_ITEMS: [(&str, ComicPageType); 11] = [
    ("Front Cover", ComicPageType(1)),
    ("Inner Cover", ComicPageType(2)),
    ("Roundup", ComicPageType(4)),
    ("Story", ComicPageType(8)),
    ("Advertisement", ComicPageType(16)),
    ("Editorial", ComicPageType(32)),
    ("Letters", ComicPageType(64)),
    ("Preview", ComicPageType(128)),
    ("Back Cover", ComicPageType(256)),
    ("Other", ComicPageType(512)),
    ("Deleted", ComicPageType(1024)),
];

// ---------- State ----------

/// A queue completion (the callbacks are Send; the state is not —
/// the ADR-019 mpsc + pump pattern).
enum EditorMsg {
    Cover {
        key_text: String,
        bytes: Option<Vec<u8>>,
    },
    Preview {
        key_text: String,
        img: Option<cr_image::Image>,
    },
}

struct EditorState {
    books: Vec<ComicBook>,
    current: usize,
    pool: Arc<ImagePool>,
    /// The preview page (list position).
    page_view_page: usize,
    preview: Option<cairo::ImageSurface>,
    pending_preview: Option<String>,
    cover: Option<cairo::ImageSurface>,
    pending_cover: Option<String>,
    adjustment: BitmapAdjustment,
    msg_tx: Arc<std::sync::Mutex<std::sync::mpsc::Sender<EditorMsg>>>,
}

type StateRef = Rc<RefCell<EditorState>>;

/// The commit callback: applies an edited clone into the library.
pub type CommitFn = Rc<dyn Fn(&ComicBook)>;

/// The widget handles the load/save paths share.
#[derive(Default)]
struct Fields {
    texts: HashMap<&'static str, Entry>,
    multilines: HashMap<&'static str, TextView>,
    numbers: HashMap<&'static str, Entry>,
    dates: HashMap<&'static str, Entry>,
    series_complete: Option<ComboBoxText>,
    manga: Option<ComboBoxText>,
    black_and_white: Option<ComboBoxText>,
    enable_proposed: Option<ComboBoxText>,
    rating: Option<Entry>,
    community_rating: Option<Entry>,
    color_sliders: Vec<gtk4::Scale>,
}

type FieldsRef = Rc<RefCell<Fields>>;

/// Opens the editor for `books` (clones of the library books; the
/// commit callback runs per save point).
pub fn show(parent: &impl IsA<gtk4::Window>, books: Vec<ComicBook>, on_commit: CommitFn) {
    if books.is_empty() {
        return;
    }
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<EditorMsg>();
    let msg_tx_cell = Arc::new(std::sync::Mutex::new(msg_tx));
    let state: StateRef = Rc::new(RefCell::new(EditorState {
        page_view_page: 0,
        books,
        current: 0,
        pool: Arc::new(ImagePool::new(None)),
        preview: None,
        pending_preview: None,
        cover: None,
        pending_cover: None,
        adjustment: BitmapAdjustment::default(),
        msg_tx: msg_tx_cell,
    }));
    let fields: FieldsRef = Rc::new(RefCell::new(Fields::default()));

    let dialog = Dialog::builder()
        .title("Book")
        .transient_for(parent)
        .modal(true)
        .default_width(920)
        .default_height(680)
        .build();
    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_spacing(6);

    // ----- The left column -----
    let left = GtkBox::new(Orientation::Vertical, 4);
    left.set_width_request(190);
    let cover = DrawingArea::new();
    cover.set_width_request(160);
    cover.set_height_request(230);
    {
        let state = Rc::clone(&state);
        cover.set_draw_func(move |_, ctx, w, h| {
            let surface = state.borrow().cover.clone();
            draw_fitted(ctx, &surface, w, h);
        });
    }
    left.append(&cover);
    let lbl_pages = Label::builder().label("").halign(Align::Start).build();
    let lbl_type = Label::builder().label("").halign(Align::Start).build();
    let lbl_path = Label::builder()
        .label("")
        .halign(Align::Start)
        .ellipsize(gtk4::pango::EllipsizeMode::Middle)
        .width_chars(22)
        .build();
    left.append(&lbl_pages);
    left.append(&lbl_type);
    left.append(&lbl_path);
    let nav = GtkBox::new(Orientation::Horizontal, 4);
    nav.set_halign(Align::Center);
    let prev = Button::with_label("<");
    let next = Button::with_label(">");
    nav.append(&prev);
    nav.append(&next);
    left.append(&nav);

    // ----- The notebook -----
    let notebook = Notebook::new();

    // ----- Details tab -----
    {
        let grid = Grid::new();
        grid.set_row_spacing(4);
        grid.set_column_spacing(8);
        grid.set_margin_top(8);
        grid.set_margin_bottom(8);
        grid.set_margin_start(8);
        grid.set_margin_end(8);
        let mut f = fields.borrow_mut();
        let mut row = 0;
        let mut col = 0;
        fn attach_pair<W: IsA<gtk4::Widget>>(
            grid: &Grid,
            row: i32,
            col: i32,
            caption: &str,
            w: &W,
        ) {
            let label = Label::new(Some(caption));
            grid.attach(&label, col, row, 1, 1);
            grid.attach(w, col + 1, row, 1, 1);
        }
        // Rating row (the C# first row).
        {
            let rating = Entry::new();
            rating.set_hexpand(true);
            attach_pair(&grid, row, 0, "Rating", &rating);
            f.rating = Some(rating);
            let cr = Entry::new();
            cr.set_hexpand(true);
            attach_pair(&grid, row, 2, "Community Rating", &cr);
            f.community_rating = Some(cr);
            row += 1;
        }
        // The text rows, two field pairs per grid row.
        for tr in DETAIL_ROWS.iter().take(2) {
            let entry = Entry::new();
            entry.set_hexpand(true);
            attach_pair(&grid, row, col, tr.caption, &entry);
            f.texts.insert(tr.property, entry);
            col += 2;
            if col >= 4 {
                col = 0;
                row += 1;
            }
        }
        // The numeric row: Volume / Count.
        for (caption, prop) in NUM_ROWS.iter().take(2) {
            let entry = Entry::new();
            entry.set_hexpand(true);
            attach_pair(&grid, row, col, caption, &entry);
            f.numbers.insert(prop, entry);
            col += 2;
            if col >= 4 {
                col = 0;
                row += 1;
            }
        }
        // Year / Month / Day.
        for (caption, prop) in NUM_ROWS.iter().skip(2) {
            let entry = Entry::new();
            entry.set_hexpand(true);
            attach_pair(&grid, row, col, caption, &entry);
            f.numbers.insert(prop, entry);
            col += 2;
            if col >= 4 {
                col = 0;
                row += 1;
            }
        }
        for tr in DETAIL_ROWS.iter().skip(2) {
            let entry = Entry::new();
            entry.set_hexpand(true);
            attach_pair(&grid, row, col, tr.caption, &entry);
            f.texts.insert(tr.property, entry);
            col += 2;
            if col >= 4 {
                col = 0;
                row += 1;
            }
        }
        for tr in DETAIL_ROWS_2 {
            let entry = Entry::new();
            entry.set_hexpand(true);
            attach_pair(&grid, row, col, tr.caption, &entry);
            f.texts.insert(tr.property, entry);
            col += 2;
            if col >= 4 {
                col = 0;
                row += 1;
            }
        }
        // The four combos.
        let combo_pair = |grid: &Grid, row: i32, col: i32, caption: &str, combo: &ComboBoxText| {
            grid.attach(&Label::new(Some(caption)), col, row, 1, 1);
            grid.attach(combo, col + 1, row, 1, 1);
        };
        let sc = ComboBoxText::new();
        for item in YES_NO_ITEMS {
            sc.append_text(item);
        }
        combo_pair(&grid, row, col, "Series Complete", &sc);
        f.series_complete = Some(sc);
        col += 2;
        if col >= 4 {
            col = 0;
            row += 1;
        }
        let manga = ComboBoxText::new();
        for item in MANGA_ITEMS {
            manga.append_text(item);
        }
        combo_pair(&grid, row, col, "Manga", &manga);
        f.manga = Some(manga);
        col += 2;
        if col >= 4 {
            col = 0;
            row += 1;
        }
        let bw = ComboBoxText::new();
        for item in YES_NO_ITEMS {
            bw.append_text(item);
        }
        combo_pair(&grid, row, col, "Black and White", &bw);
        f.black_and_white = Some(bw);
        col += 2;
        if col >= 4 {
            col = 0;
            row += 1;
        }
        let ep = ComboBoxText::new();
        for item in PROPOSED_ITEMS {
            ep.append_text(item);
        }
        combo_pair(&grid, row, col, "Enable Proposed", &ep);
        f.enable_proposed = Some(ep.clone());
        drop(f);
        // The proposed placeholders follow the combo (`SetPromptTexts`).
        {
            let state = Rc::clone(&state);
            let fields = Rc::clone(&fields);
            ep.connect_changed(move |cb| {
                let enabled = matches!(cb.active(), Some(1));
                let book = current_book(&state);
                for (prop, value) in proposed_placeholders(&book) {
                    if let Some(entry) = fields.borrow().texts.get(prop) {
                        entry.set_placeholder_text(if enabled {
                            Some(value.as_str())
                        } else {
                            Some("")
                        });
                    }
                }
            });
        }
        let scroll = ScrolledWindow::builder()
            .child(&grid)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vexpand(true)
            .build();
        notebook.append_page(&scroll, Some(&Label::new(Some("Details"))));
    }

    // ----- Plot tab -----
    {
        let grid = Grid::new();
        grid.set_row_spacing(4);
        grid.set_column_spacing(8);
        grid.set_margin_top(8);
        grid.set_margin_bottom(8);
        grid.set_margin_start(8);
        grid.set_margin_end(8);
        let mut f = fields.borrow_mut();
        for (i, (caption, prop)) in [
            ("Summary", "Summary"),
            ("Notes", "Notes"),
            ("Review", "Review"),
        ]
        .iter()
        .enumerate()
        {
            let label = Label::new(Some(caption));
            label.set_halign(Align::Start);
            label.set_valign(Align::Start);
            grid.attach(&label, 0, i as i32, 1, 1);
            let tv = TextView::new();
            tv.set_wrap_mode(gtk4::WrapMode::Word);
            tv.set_height_request(if *caption == "Summary" { 90 } else { 56 });
            tv.set_hexpand(true);
            let scroll = ScrolledWindow::builder().child(&tv).build();
            grid.attach(&scroll, 1, i as i32, 1, 1);
            f.multilines.insert(prop, tv);
        }
        for (i, tr) in PLOT_ROWS.iter().enumerate() {
            let entry = Entry::new();
            entry.set_hexpand(true);
            grid.attach(&Label::new(Some(tr.caption)), 0, (i + 3) as i32, 1, 1);
            grid.attach(&entry, 1, (i + 3) as i32, 1, 1);
            f.texts.insert(tr.property, entry);
        }
        let scroll = ScrolledWindow::builder()
            .child(&grid)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vexpand(true)
            .build();
        notebook.append_page(&scroll, Some(&Label::new(Some("Plot"))));
    }

    // ----- Catalog tab -----
    {
        let grid = Grid::new();
        grid.set_row_spacing(4);
        grid.set_column_spacing(8);
        grid.set_margin_top(8);
        grid.set_margin_bottom(8);
        grid.set_margin_start(8);
        grid.set_margin_end(8);
        let mut f = fields.borrow_mut();
        for (i, tr) in CATALOG_ROWS.iter().enumerate() {
            let entry = Entry::new();
            entry.set_hexpand(true);
            grid.attach(&Label::new(Some(tr.caption)), 0, i as i32, 1, 1);
            grid.attach(&entry, 1, i as i32, 1, 1);
            if tr.property == "BookPrice" {
                f.numbers.insert("BookPrice", entry);
            } else {
                f.texts.insert(tr.property, entry);
            }
        }
        for (i, (caption, prop)) in [("Added", "AddedTime"), ("Released", "ReleasedTime")]
            .iter()
            .enumerate()
        {
            let entry = Entry::new();
            entry.set_hexpand(true);
            let row = (i + CATALOG_ROWS.len()) as i32;
            grid.attach(
                &Label::new(Some(&format!("{caption} (YYYY-MM-DD)"))),
                0,
                row,
                1,
                1,
            );
            grid.attach(&entry, 1, row, 1, 1);
            f.dates.insert(prop, entry);
        }
        let scroll = ScrolledWindow::builder()
            .child(&grid)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vexpand(true)
            .build();
        notebook.append_page(&scroll, Some(&Label::new(Some("Catalog"))));
    }

    // ----- Pages tab -----
    let pages_widgets = build_pages_tab(&state, &cover);
    notebook.append_page(&pages_widgets.root, Some(&Label::new(Some("Pages"))));

    // ----- Colors tab -----
    {
        let grid = Grid::new();
        grid.set_row_spacing(6);
        grid.set_column_spacing(8);
        grid.set_margin_top(8);
        grid.set_margin_bottom(8);
        grid.set_margin_start(8);
        grid.set_margin_end(8);
        let adj = state.borrow().adjustment;
        let mut sliders: Vec<gtk4::Scale> = Vec::new();
        let rows: [(&str, f64, f64); 5] = [
            ("Saturation", adj.saturation as f64 * 100.0, -100.0),
            ("Brightness", adj.brightness as f64 * 100.0, -100.0),
            ("Contrast", adj.contrast as f64 * 100.0, -100.0),
            ("Gamma", adj.gamma as f64 * 100.0, -100.0),
            ("Sharpening", adj.sharpen as f64, 0.0),
        ];
        for (i, (caption, value, min)) in rows.iter().enumerate() {
            let label = Label::new(Some(caption));
            label.set_halign(Align::Start);
            grid.attach(&label, 0, i as i32, 1, 1);
            let max = if *caption == "Sharpening" { 3.0 } else { 100.0 };
            let scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, *min, max, 1.0);
            scale.set_value(*value);
            scale.set_hexpand(true);
            grid.attach(&scale, 1, i as i32, 1, 1);
            sliders.push(scale);
        }
        // Slider → state (the save writes the adjustment into the
        // book: `comic.ColorAdjustment = pageViewer.ColorAdjustment`).
        for (i, scale) in sliders.iter().enumerate() {
            let state = Rc::clone(&state);
            scale.connect_value_changed(move |sc| {
                let v = sc.value() as f32;
                let mut s = state.borrow_mut();
                match i {
                    0 => s.adjustment.saturation = v / 100.0,
                    1 => s.adjustment.brightness = v / 100.0,
                    2 => s.adjustment.contrast = v / 100.0,
                    3 => s.adjustment.gamma = v / 100.0,
                    _ => s.adjustment.sharpen = v as i32,
                }
            });
        }
        let reset = Button::with_label("Reset");
        reset.set_halign(Align::Start);
        grid.attach(&reset, 0, rows.len() as i32, 2, 1);
        {
            let sliders: Vec<gtk4::Scale> = sliders.clone();
            reset.connect_clicked(move |_| {
                for s in &sliders {
                    s.set_value(0.0);
                }
            });
        }
        fields.borrow_mut().color_sliders = sliders;
        let scroll = ScrolledWindow::builder()
            .child(&grid)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .build();
        notebook.append_page(&scroll, Some(&Label::new(Some("Colors"))));
    }

    // ----- Custom tab (checkpoint-1 subset: the book's own values) -----
    {
        let page = GtkBox::new(Orientation::Vertical, 6);
        page.set_margin_top(8);
        page.set_margin_bottom(8);
        page.set_margin_start(8);
        page.set_margin_end(8);
        page.append(&Label::new(Some(
            "Custom values are read-only in this checkpoint; the \
             library-wide key editor joins with the file write-back \
             step.",
        )));
        let list = gtk4::ListBox::new();
        page.append(&list);
        let state2 = Rc::clone(&state);
        let fill = move || {
            while let Some(c) = list.first_child() {
                list.remove(&c);
            }
            let book = current_book(&state2);
            for (k, v) in
                cr_core::model::comic_book::values_store::decode(&book.custom_values_store)
            {
                let row = gtk4::ListBoxRow::new();
                let box_ = GtkBox::new(Orientation::Horizontal, 8);
                let name = Label::new(Some(&k));
                name.set_halign(Align::Start);
                name.set_width_chars(20);
                let value = Label::new(Some(&v));
                value.set_halign(Align::Start);
                value.set_hexpand(true);
                box_.append(&name);
                box_.append(&value);
                row.set_child(Some(&box_));
                list.append(&row);
            }
        };
        fill();
        notebook.append_page(&page, Some(&Label::new(Some("Custom"))));
    }

    // ----- The main row + buttons -----
    let main = GtkBox::new(Orientation::Horizontal, 8);
    main.append(&left);
    main.append(&notebook);
    main.set_vexpand(true);
    content.append(&main);

    dialog.add_button("Apply", gtk4::ResponseType::Apply);
    dialog.add_button("OK", gtk4::ResponseType::Ok);
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);

    // ----- load (SetComicToEditor + SetDataToEditor) -----
    let load = {
        let state = Rc::clone(&state);
        let fields = Rc::clone(&fields);
        let dialog = dialog.clone();
        let lbl_pages = lbl_pages.clone();
        let lbl_type = lbl_type.clone();
        let lbl_path = lbl_path.clone();
        let pages_widgets = pages_widgets.clone();
        move || {
            let book = {
                let mut s = state.borrow_mut();
                s.pending_preview = None;
                s.pending_cover = None;
                s.preview = None;
                s.cover = None;
                let mut book = s.books[s.current].clone();
                // The C# editor opens the comic through a navigator
                // (`pagesView.Book.Open`): PageCount = the provider
                // count, the stored entries overlay it, and the
                // merged list persists on save (`comic.SetPages`
                // parity).
                if let Some(pages) =
                    crate::pages::merged_pages_or_none(&book, std::path::Path::new(&book.file_path))
                {
                    book.info.page_count = pages.len() as i32;
                    book.info.pages = pages;
                    let cur = s.current;
                    s.books[cur].info = book.info.clone();
                }
                s.page_view_page = book
                    .info
                    .front_cover_page_index()
                    .clamp(0, (book.info.page_count - 1).max(0))
                    .max(0) as usize;
                s.adjustment = book.color_adjustment;
                book
            };
            dialog.set_title(Some(&book_caption(&book)));
            // The info labels (`lblPages`, `lblType`, `lblPath`).
            lbl_pages.set_text(&format!(
                "Page {}/{} Page(s).",
                book.last_page_read.clamp(0, book.info.page_count - 1) + 1,
                book.info.page_count
            ));
            let ext = Path::new(&book.file_path)
                .extension()
                .map(|e| e.to_string_lossy().to_uppercase())
                .unwrap_or_default();
            lbl_type.set_text(&format!("{}/{}", ext, format_size(book.file_size)));
            lbl_path.set_text(&book.file_path);
            // The field values.
            {
                let f = fields.borrow();
                for (prop, entry) in &f.texts {
                    if let Some(PropValue::Str(v)) = registry::get(&book, prop) {
                        entry.set_text(&v);
                    }
                }
                for (prop, tv) in &f.multilines {
                    if let Some(PropValue::Str(v)) = registry::get(&book, prop) {
                        tv.buffer().set_text(&v);
                    }
                }
                for (prop, entry) in &f.numbers {
                    let v = match registry::get(&book, prop) {
                        Some(PropValue::Int(v)) => v,
                        Some(PropValue::Float(v)) => v as i64,
                        _ => -1,
                    };
                    let text = if v >= 0 { v.to_string() } else { String::new() };
                    entry.set_text(&text);
                }
                for (prop, name) in [("AddedTime", "AddedTime"), ("ReleasedTime", "ReleasedTime")] {
                    if let (Some(PropValue::Date(d)), Some(e)) =
                        (registry::get(&book, prop), f.dates.get(name))
                    {
                        e.set_text(&d.naive.date().to_string());
                    }
                }
                if let Some(cb) = &f.series_complete {
                    cb.set_active(Some(combo_index_of_yesno(book.series_complete)));
                }
                if let Some(cb) = &f.manga {
                    cb.set_active(Some(combo_index_of_manga(book.info.manga)));
                }
                if let Some(cb) = &f.black_and_white {
                    cb.set_active(Some(combo_index_of_yesno(book.info.black_and_white)));
                }
                if let Some(cb) = &f.enable_proposed {
                    cb.set_active(Some(if book.enable_proposed { 1 } else { 0 }));
                    let enabled = book.enable_proposed;
                    for (prop, value) in proposed_placeholders(&book) {
                        if let Some(entry) = f.texts.get(prop) {
                            entry.set_placeholder_text(if enabled {
                                Some(value.as_str())
                            } else {
                                Some("")
                            });
                        }
                    }
                }
                if let Some(e) = &f.rating {
                    let v = book.rating;
                    let text = if v > 0.0 {
                        v.to_string()
                    } else {
                        String::new()
                    };
                    e.set_text(&text);
                }
                if let Some(e) = &f.community_rating {
                    let v = book.info.community_rating;
                    let text = if v > 0.0 {
                        v.to_string()
                    } else {
                        String::new()
                    };
                    e.set_text(&text);
                }
                // The color sliders re-read on every load.
                for (i, scale) in f.color_sliders.iter().enumerate() {
                    let v = match i {
                        0 => book.color_adjustment.saturation * 100.0,
                        1 => book.color_adjustment.brightness * 100.0,
                        2 => book.color_adjustment.contrast * 100.0,
                        3 => book.color_adjustment.gamma * 100.0,
                        _ => book.color_adjustment.sharpen as f32,
                    };
                    scale.set_value(v as f64);
                }
            }
            // The cover + preview + the pages list.
            queue_cover(&state, &book);
            queue_preview(&state, &book);
            rebuild_pages_list(&pages_widgets, &state);
        }
    };

    // ----- save (SaveBook): widgets → working clone → commit -----
    let save = {
        let state = Rc::clone(&state);
        let fields = Rc::clone(&fields);
        let on_commit = Rc::clone(&on_commit);
        move || {
            let f = fields.borrow();
            let mut s = state.borrow_mut();
            let adjustment = s.adjustment;
            let cur = s.current;
            let book = &mut s.books[cur];
            for (prop, entry) in &f.texts {
                let v = entry.text().trim().to_string();
                let _ = registry::set(book, prop, &PropValue::Str(v));
            }
            for (prop, tv) in &f.multilines {
                let v = text_of(tv);
                let _ = registry::set(book, prop, &PropValue::Str(v));
            }
            for (prop, entry) in &f.numbers {
                if *prop == "BookPrice" {
                    book.book_price = real_from_text(&entry.text());
                } else {
                    let v = number_from_text(&entry.text());
                    let _ = registry::set(book, prop, &PropValue::Int(v as i64));
                }
            }
            if let Some(e) = &f.rating {
                book.rating = rating_from_text(&e.text(), book.rating);
            }
            if let Some(e) = &f.community_rating {
                book.info.community_rating =
                    rating_from_text(&e.text(), book.info.community_rating);
            }
            if let Some(cb) = &f.series_complete {
                book.series_complete = yesno_from_combo(cb.active());
            }
            if let Some(cb) = &f.manga {
                book.info.manga = manga_from_combo(cb.active());
            }
            if let Some(cb) = &f.black_and_white {
                book.info.black_and_white = yesno_from_combo(cb.active());
            }
            if let Some(cb) = &f.enable_proposed {
                book.enable_proposed = matches!(cb.active(), Some(1));
            }
            for (prop, entry) in &f.dates {
                if let Some(PropValue::Date(d)) = registry::get(book, prop) {
                    let parsed = date_from_text(&entry.text(), d);
                    let _ = registry::set(book, prop, &PropValue::Date(parsed));
                }
            }
            book.color_adjustment = adjustment;
            on_commit(&s.books[s.current].clone());
        }
    };

    // ----- navigation (prev/next save first — the C# flow) -----
    {
        let state = Rc::clone(&state);
        let save = save.clone();
        let load = load.clone();
        prev.connect_clicked(move |_| {
            save();
            let can = state.borrow().current > 0;
            if can {
                state.borrow_mut().current -= 1;
                load();
            }
        });
    }
    {
        let state = Rc::clone(&state);
        let save = save.clone();
        let load = load.clone();
        next.connect_clicked(move |_| {
            save();
            let can = state.borrow().current + 1 < state.borrow().books.len();
            if can {
                state.borrow_mut().current += 1;
                load();
            }
        });
    }
    prev.set_visible(state.borrow().books.len() > 1);
    next.set_visible(state.borrow().books.len() > 1);

    // ----- buttons -----
    {
        let save = save.clone();
        let load = load.clone();
        dialog.connect_response(move |dlg, response| match response {
            gtk4::ResponseType::Apply => {
                save();
                load();
            }
            gtk4::ResponseType::Ok => {
                save();
                dlg.close();
            }
            _ => dlg.close(),
        });
    }

    start_editor_pump(&state, msg_rx, &cover, &pages_widgets.preview, &dialog);
    load();
    dialog.present();
}

// ---------- Shared helpers ----------

fn current_book(state: &StateRef) -> ComicBook {
    let s = state.borrow();
    s.books[s.current].clone()
}

fn text_of(tv: &TextView) -> String {
    let buf = tv.buffer();
    buf.text(&buf.start_iter(), &buf.end_iter(), false)
        .trim()
        .to_string()
}

fn format_size(bytes: i64) -> String {
    // The C# `FileSizeAsText` (KB rounding).
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

fn book_caption(book: &ComicBook) -> String {
    if !book.info.series.is_empty() {
        if book.info.number.is_empty() {
            book.info.series.clone()
        } else {
            format!("{} {}", book.info.series, book.info.number)
        }
    } else {
        book.file_path
            .rsplit('/')
            .next()
            .unwrap_or("Book")
            .to_string()
    }
}

fn draw_fitted(ctx: &cairo::Context, surface: &Option<cairo::ImageSurface>, w: i32, h: i32) {
    ctx.set_source_rgb(0.12, 0.12, 0.13);
    ctx.paint().ok();
    let Some(srf) = surface else {
        return;
    };
    let (iw, ih) = (srf.width() as f64, srf.height() as f64);
    if iw <= 0.0 || ih <= 0.0 || w <= 0 || h <= 0 {
        return;
    }
    let scale = (w as f64 / iw).min(h as f64 / ih);
    ctx.save().ok();
    ctx.translate(
        ((w as f64) - iw * scale) / 2.0,
        ((h as f64) - ih * scale) / 2.0,
    );
    ctx.scale(scale, scale);
    ctx.set_source_surface(srf, 0.0, 0.0).ok();
    ctx.paint().ok();
    ctx.restore().ok();
}

/// `SetCoverThumbnailImage`: the front-cover thumbnail through the
/// thumb queue (`GetThumbnail(onlyMemory)` → queue + callback, the
/// ADR-019 pattern).
fn queue_cover(state: &StateRef, book: &ComicBook) {
    let path = book.file_path.clone();
    let page = book
        .info
        .front_cover_page_index()
        .clamp(0, (book.info.page_count - 1).max(0))
        .max(0) as usize;
    // The thumb reads the ENTRY's archive index (a reorder moves
    // entries, not archive slots).
    let provider_index = book
        .info
        .pages
        .get(page)
        .map(|p| p.image_index())
        .filter(|idx| *idx >= 0)
        .unwrap_or(page as i32) as usize;
    let key = cr_image::keys::ThumbnailKey::new(cr_image::keys::ImageKey::from_file(
        path.clone(),
        Path::new(&path),
        provider_index,
        ImageRotation::None,
    ));
    let key_text = format!("cover:{}#{}", path, page);
    state.borrow_mut().pending_cover = Some(key_text.clone());
    let tx = Arc::clone(&state.borrow().msg_tx);
    let pool = Arc::clone(&state.borrow().pool);
    let render = Arc::clone(&pool);
    pool.add_thumb_to_queue(key, None, move |k| {
        let bytes = render.render_thumbnail(k);
        let _ = tx.lock().map(|tx| {
            tx.send(EditorMsg::Cover {
                key_text: key_text.clone(),
                bytes,
            })
        });
    });
}

/// `SetImage`: the preview page through the page queue.
fn queue_preview(state: &StateRef, book: &ComicBook) {
    let (path, page, count) = {
        let s = state.borrow();
        (
            book.file_path.clone(),
            s.page_view_page,
            book.info.page_count,
        )
    };
    if count <= 0 {
        return;
    }
    let rotation = book
        .info
        .pages
        .get(page)
        .map(|p| p.rotation)
        .unwrap_or(ImageRotation::None);
    let key = cr_image::keys::PageKey::new(
        cr_image::keys::ImageKey::from_file(path.clone(), Path::new(&path), page, rotation),
        Default::default(),
    );
    let key_text = format!("preview:{}#{}", path, page);
    state.borrow_mut().pending_preview = Some(key_text.clone());
    let tx = Arc::clone(&state.borrow().msg_tx);
    let pool = Arc::clone(&state.borrow().pool);
    let render = Arc::clone(&pool);
    pool.add_page_to_queue(
        key,
        None,
        move |k| {
            let img = render.render_page(k);
            let _ = tx.lock().map(|tx| {
                tx.send(EditorMsg::Preview {
                    key_text: key_text.clone(),
                    img,
                })
            });
        },
        false,
    );
}

/// The completion pump: drains the channel onto the UI thread, drops
/// stale payloads by key, and redraws (the ADR-019 pattern). Lives
/// while the dialog is visible.
fn start_editor_pump(
    state: &StateRef,
    rx: std::sync::mpsc::Receiver<EditorMsg>,
    cover: &DrawingArea,
    preview: &DrawingArea,
    dialog: &Dialog,
) {
    let state = Rc::downgrade(state);
    let cover = cover.clone();
    let preview = preview.clone();
    let dialog = dialog.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(60), move || {
        if !dialog.is_visible() {
            return glib::ControlFlow::Break;
        }
        let mut drew = false;
        loop {
            let msg = rx.try_recv();
            let Ok(msg) = msg else {
                break;
            };
            let Some(st) = state.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let mut s = st.borrow_mut();
            match msg {
                EditorMsg::Cover { key_text, bytes } => {
                    if s.pending_cover.as_deref() == Some(key_text.as_str()) {
                        // The pool blob carries the ThumbnailImage
                        // serialization header — parse before decode.
                        s.cover = bytes.and_then(|b| crate::bitmap::surface_from_thumb_blob(&b));
                        s.pending_cover = None;
                        drew = true;
                    }
                }
                EditorMsg::Preview { key_text, img } => {
                    if s.pending_preview.as_deref() == Some(key_text.as_str()) {
                        s.preview = img.as_ref().map(crate::bitmap::surface_from_image);
                        s.pending_preview = None;
                        drew = true;
                    }
                }
            }
        }
        if drew {
            cover.queue_draw();
            preview.queue_draw();
        }
        glib::ControlFlow::Continue
    });
}

// ---------- The Pages tab ----------

#[derive(Clone)]
struct PagesWidgets {
    root: GtkBox,
    list: gtk4::ListBox,
    preview: DrawingArea,
    cover: DrawingArea,
}

fn build_pages_tab(state: &StateRef, cover: &DrawingArea) -> PagesWidgets {
    let root = GtkBox::new(Orientation::Horizontal, 8);
    root.set_margin_top(8);
    root.set_margin_bottom(8);
    root.set_margin_start(8);
    root.set_margin_end(8);

    // The list of pages (left). A click selects the row and shows
    // the page (`PagesViewSelectedIndexChanged` → `SetPageView`).
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::Single);
    let list_scroll = ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .width_request(240)
        .build();
    root.append(&list_scroll);

    // The preview (right).
    let right = GtkBox::new(Orientation::Vertical, 4);
    let label = Label::new(Some("Page 1"));
    right.append(&label);
    let preview = DrawingArea::new();
    preview.set_vexpand(true);
    preview.set_hexpand(true);
    {
        let state = Rc::clone(state);
        preview.set_draw_func(move |_, ctx, w, h| {
            let surface = state.borrow().preview.clone();
            draw_fitted(ctx, &surface, w, h);
            // The completions redraw via the pump below.
        });
    }
    right.append(&preview);
    let nav = GtkBox::new(Orientation::Horizontal, 4);
    nav.set_halign(Align::Center);
    let first = Button::with_label("|<");
    let prev = Button::with_label("<");
    let next = Button::with_label(">");
    let last = Button::with_label(">|");
    for b in [&first, &prev, &next, &last] {
        nav.append(b);
    }
    right.append(&nav);
    root.append(&right);

    // `btFirstPage` family. The nav also moves the list highlight
    // (select_row fires the row_selected hook, which guards on the
    // same page).
    let goto_page = {
        let state = Rc::clone(state);
        let preview = preview.clone();
        let label = label.clone();
        let list = list.clone();
        move |target: i32| {
            let count = {
                let s = state.borrow();
                s.books[s.current].info.page_count
            };
            if count <= 0 {
                return;
            }
            let page = target.clamp(0, count - 1) as usize;
            state.borrow_mut().page_view_page = page;
            label.set_text(&format!("Page {}", page + 1));
            if let Some(row) = list.row_at_index(page as i32) {
                list.select_row(Some(&row));
            }
            let book = current_book(&state);
            queue_preview(&state, &book);
            preview.queue_draw();
        }
    };
    {
        let goto_page = goto_page.clone();
        first.connect_clicked(move |_| {
            goto_page(0);
        });
    }
    {
        let state = Rc::clone(state);
        let goto_page = goto_page.clone();
        prev.connect_clicked(move |_| {
            let page = state.borrow().page_view_page as i32;
            goto_page(page - 1);
        });
    }
    {
        let state = Rc::clone(state);
        let goto_page = goto_page.clone();
        next.connect_clicked(move |_| {
            let page = state.borrow().page_view_page as i32;
            goto_page(page + 1);
        });
    }
    {
        let state = Rc::clone(state);
        let goto_page = goto_page.clone();
        last.connect_clicked(move |_| {
            let count = state.borrow().books[state.borrow().current].info.page_count;
            goto_page(count - 1);
        });
    }
    // The preview pump: redraw while a load is pending.
    {
        let state = Rc::clone(state);
        let preview = preview.clone();
        let cover_state = Rc::downgrade(&state);
        let _ = cover_state;
        glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
            let pending = state.borrow().pending_preview.is_some();
            if pending {
                preview.queue_draw();
                return glib::ControlFlow::Continue;
            }
            preview.queue_draw();
            glib::ControlFlow::Break
        });
    }

    // Selection → preview (`PagesViewSelectedIndexChanged`).
    {
        let state = Rc::clone(state);
        let preview = preview.clone();
        let label = label.clone();
        let list = list.clone();
        list.connect_row_selected(move |_, row| {
            let Some(row) = row else {
                return;
            };
            let page = row.index() as usize;
            // A rebuild re-selects the current row; the guard keeps
            // the loop trivial (same page → no re-queue).
            let changed = state.borrow().page_view_page != page;
            if !changed {
                return;
            }
            state.borrow_mut().page_view_page = page;
            label.set_text(&format!("Page {}", page + 1));
            let book = current_book(&state);
            queue_preview(&state, &book);
            preview.queue_draw();
        });
    }

    // The per-page context menu (`PagesView` commands).
    {
        let state = Rc::clone(state);
        let widgets = PagesWidgets {
            root: root.clone(),
            list: list.clone(),
            preview: preview.clone(),
            cover: cover.clone(),
        };
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3);
        gesture.connect_pressed(move |g, _n, x, y| {
            g.set_state(gtk4::EventSequenceState::Claimed);
            let hit = widgets.list.pick(x, y, gtk4::PickFlags::DEFAULT);
            let Some(row) = hit.and_downcast::<gtk4::ListBoxRow>() else {
                return;
            };
            show_page_menu(&state, &widgets, row.index() as usize, x, y);
        });
        list.add_controller(gesture);
    }

    PagesWidgets {
        root,
        list,
        preview,
        cover: cover.clone(),
    }
}

fn rebuild_pages_list(widgets: &PagesWidgets, state: &StateRef) {
    while let Some(child) = widgets.list.first_child() {
        widgets.list.remove(&child);
    }
    let book = current_book(state);
    let current = state.borrow().page_view_page;
    for (i, p) in book.info.pages.iter().enumerate() {
        let type_name = ComicPageType::MEMBERS
            .iter()
            .find(|(_, v)| *v == p.page_type.0)
            .map(|(n, _)| *n)
            .unwrap_or("Story");
        let rotated = match p.rotation {
            ImageRotation::None => String::new(),
            other => format!("  ({})", other.to_xml()),
        };
        let position = match p.page_position {
            ComicPagePosition::Default => String::new(),
            ComicPagePosition::Near => " near".to_string(),
            ComicPagePosition::Far => " far".to_string(),
        };
        let row = gtk4::ListBoxRow::new();
        let label = Label::builder()
            .label(format!("{} — {}{}{}", i + 1, type_name, rotated, position))
            .halign(Align::Start)
            .build();
        label.set_margin_start(6);
        label.set_margin_end(6);
        label.set_margin_top(2);
        label.set_margin_bottom(2);
        row.set_child(Some(&label));
        widgets.list.append(&row);
        if i == current {
            widgets.list.select_row(Some(&row));
        }
    }
}

/// The page-row context menu (`PagesView` commands): Set Page Type /
/// Rotate / Position / Mark as Deleted / Move to Top / Move to
/// Bottom / Reset Original Order. Built as a manual popover with
/// buttons — the same mechanism as the browser context menu (the
/// action-muxer route proved inert in the first user test).
fn show_page_menu(state: &StateRef, widgets: &PagesWidgets, index: usize, x: f64, y: f64) {
    let popover = gtk4::Popover::new();
    let outer = GtkBox::new(Orientation::Vertical, 0);
    outer.set_margin_top(4);
    outer.set_margin_bottom(4);
    outer.set_margin_start(4);
    outer.set_margin_end(4);

    // A page mutation: apply → rebuild the list → refresh the
    // preview (the C# `pagesView.UpdateList`).
    let after: Rc<dyn Fn()> = Rc::new({
        let state = Rc::clone(state);
        let widgets = widgets.clone();
        move || {
            rebuild_pages_list(&widgets, &state);
            let book = current_book(&state);
            // A Front Cover type change moves the cover thumbnail.
            queue_cover(&state, &book);
            widgets.cover.queue_draw();
            queue_preview(&state, &book);
            widgets.preview.queue_draw();
        }
    });
    fn add_button(
        outer: &GtkBox,
        label: &str,
        popover: &gtk4::Popover,
        run: impl Fn(&StateRef) + 'static,
        state: &StateRef,
        after: &Rc<dyn Fn()>,
    ) {
        let button = Button::with_label(label);
        button.set_has_frame(false);
        button.set_halign(Align::Fill);
        let popover = popover.clone();
        let after = Rc::clone(after);
        let state = Rc::clone(state);
        button.connect_clicked(move |_| {
            popover.popdown();
            run(&state);
            after();
        });
        outer.append(&button);
    }

    let add_section = |outer: &GtkBox, caption: &str| {
        let label = Label::builder()
            .label(caption)
            .halign(Align::Start)
            .css_classes(["heading"])
            .margin_top(6)
            .margin_start(4)
            .build();
        outer.append(&label);
    };

    // Set Page Type (the 11 single values).
    add_section(&outer, "Set Page Type");
    for (name, page_type) in PAGE_TYPE_ITEMS {
        let state = Rc::clone(state);
        add_button(
            &outer,
            name,
            &popover,
            move |state| {
                let cur = state.borrow().current;
                if let Some(b) = state.borrow_mut().books.get_mut(cur) {
                    b.info.update_page_type(index, page_type);
                }
            },
            &state,
            &after,
        );
    }
    // Rotate.
    add_section(&outer, "Rotate");
    const ROTATIONS: [(&str, ImageRotation); 4] = [
        ("None", ImageRotation::None),
        ("90°", ImageRotation::Rotate90),
        ("180°", ImageRotation::Rotate180),
        ("270°", ImageRotation::Rotate270),
    ];
    for (name, rotation) in ROTATIONS {
        let state = Rc::clone(state);
        add_button(
            &outer,
            name,
            &popover,
            move |state| {
                let cur = state.borrow().current;
                if let Some(b) = state.borrow_mut().books.get_mut(cur) {
                    b.info.update_page_rotation(index, rotation);
                }
            },
            &state,
            &after,
        );
    }
    // Position.
    add_section(&outer, "Position");
    const POSITIONS: [(&str, ComicPagePosition); 3] = [
        ("Default", ComicPagePosition::Default),
        ("Near", ComicPagePosition::Near),
        ("Far", ComicPagePosition::Far),
    ];
    for (name, position) in POSITIONS {
        let state = Rc::clone(state);
        add_button(
            &outer,
            name,
            &popover,
            move |state| {
                let cur = state.borrow().current;
                if let Some(b) = state.borrow_mut().books.get_mut(cur) {
                    b.info.update_page_position(index, position);
                }
            },
            &state,
            &after,
        );
    }
    // Commands.
    add_section(&outer, "Pages");
    {
        let state = Rc::clone(state);
        add_button(
            &outer,
            "Mark as Deleted",
            &popover,
            move |state| {
                let cur = state.borrow().current;
                let mut s = state.borrow_mut();
                let cur_type = s.books[cur].info.pages.get(index).map(|p| p.page_type);
                let next = if cur_type == Some(ComicPageType(1024)) {
                    ComicPageType(8) // Story
                } else {
                    ComicPageType(1024) // Deleted
                };
                if let Some(b) = s.books.get_mut(cur) {
                    b.info.update_page_type(index, next);
                }
            },
            &state,
            &after,
        );
    }
    {
        let state = Rc::clone(state);
        add_button(
            &outer,
            "Move to Top",
            &popover,
            move |state| {
                let cur = state.borrow().current;
                if let Some(b) = state.borrow_mut().books.get_mut(cur) {
                    b.info.move_pages(0, &[index]);
                }
            },
            &state,
            &after,
        );
    }
    {
        let state = Rc::clone(state);
        add_button(
            &outer,
            "Move to Bottom",
            &popover,
            move |state| {
                let cur = state.borrow().current;
                let count = state.borrow().books[cur].info.pages.len() as i32;
                if let Some(b) = state.borrow_mut().books.get_mut(cur) {
                    b.info.move_pages(count, &[index]);
                }
            },
            &state,
            &after,
        );
    }
    {
        let state = Rc::clone(state);
        add_button(
            &outer,
            "Reset Original Order",
            &popover,
            move |state| {
                let cur = state.borrow().current;
                if let Some(b) = state.borrow_mut().books.get_mut(cur) {
                    b.info.reset_page_sequence();
                }
            },
            &state,
            &after,
        );
    }

    let scroll = ScrolledWindow::builder()
        .child(&outer)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .max_content_height(430)
        .propagate_natural_height(true)
        .build();
    popover.set_child(Some(&scroll));
    popover.set_parent(&widgets.list);
    // set_pointing_to is in the popover PARENT's coordinates; the
    // menu parents to the list, so the pick coordinates are direct.
    let rect = gdk::Rectangle::new(x as i32, y as i32 + 4, 1, 1);
    popover.set_pointing_to(Some(&rect));
    popover.connect_closed(|p| p.unparent());
    popover.popup();
}
