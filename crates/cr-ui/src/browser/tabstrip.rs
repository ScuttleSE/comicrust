//! The workspace tab strip (`MainView.tabStrip` — the C# `TabBar`):
//! one row directly under the menubar carrying the fixed `Library` /
//! `Pages` items, one tab per open comic slot, and the `+` new-slot
//! item. The right end hosts the reader toolbar (the C# Fill mode
//! re-parents `mainToolStrip` into the tab row —
//! `MainForm.MainToolStripVisible`).
//!
//! Full-window tabs: selecting an item swaps the whole workspace
//! below (`MainView.ShowView`); the reader keeps no tabs of its own.
//! A comic tab renders a 16 px cover thumbnail async (the C#
//! `Create tab thumbnails` background thread), the caption
//! (`Comic.Caption`), a close button, and a bold label while its
//! slot is the current one (`fileTab.FontBold`).

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use gtk4::prelude::*;
use gtk4::{gdk, glib, Button, Image, Label, Orientation};

use cr_engine::image_pool::ImagePool;

use crate::reader_shell::TabInfo;

/// Which strip item (selection + probe identities).
#[derive(Clone, PartialEq, Debug)]
pub enum TabId {
    Library,
    Folders,
    Pages,
    Comic(usize),
    Plus,
}

/// `OnGuiVisibilities` Fill-mode rule for the tab row AND the status
/// strip (`MainForm.cs:3680-3691`): visible unless MinimalGui while
/// the reader shows, always visible on the browser, and the
/// `ShowMainMenuNoComicOpen` escape.
pub fn tabstrip_visible(
    minimal: bool,
    undocked: bool,
    is_comic_viewer: bool,
    open_books: usize,
    show_no_comic: bool,
) -> bool {
    if undocked {
        return true;
    }
    !minimal || !is_comic_viewer || (show_no_comic && open_books == 0)
}

struct ComicTabItem {
    slot: usize,
    root: gtk4::Box,
    button: Button,
    label: Label,
    close: Button,
    image: Image,
}

struct TabThumb {
    slot: usize,
    bytes: Option<Vec<u8>>,
}

type SelectFn = Box<dyn Fn(&TabId)>;
type CloseFn = Box<dyn Fn(usize)>;

struct Inner {
    widget: gtk4::Box,
    host: gtk4::Box,
    comic_box: gtk4::Box,
    library_btn: Button,
    folders_btn: Button,
    pages_btn: Button,
    pages_root: gtk4::Box,
    plus_btn: Button,
    comic_tabs: RefCell<Vec<ComicTabItem>>,
    selected: RefCell<TabId>,
    on_select: RefCell<Option<SelectFn>>,
    on_close: RefCell<Option<CloseFn>>,
    pool: Arc<ImagePool>,
    thumb_tx: std::sync::mpsc::Sender<TabThumb>,
    thumb_rx: std::sync::mpsc::Receiver<TabThumb>,
    pending: Cell<usize>,
    queued: RefCell<HashSet<usize>>,
    pump_active: Cell<bool>,
}

/// The clone-able handle (the shell keeps it; closures hold a
/// `Weak` — the widget must never own a strong self reference).
#[derive(Clone)]
pub struct TabStrip {
    inner: Rc<Inner>,
}

/// The 16 px tab cover (`Create tab thumbnails` resize target).
const TAB_THUMB_PX: i32 = 16;

impl TabStrip {
    pub fn create(pool: Arc<ImagePool>) -> TabStrip {
        let widget = gtk4::Box::new(Orientation::Horizontal, 0);
        widget.add_css_class("tabstrip");
        let items_box = gtk4::Box::new(Orientation::Horizontal, 2);
        items_box.set_hexpand(true);
        items_box.set_valign(gtk4::Align::Center);
        // The right host: the reader toolbar docks here (the C#
        // `MainToolStripVisible = false` Fill rule).
        let host = gtk4::Box::new(Orientation::Horizontal, 0);
        host.set_valign(gtk4::Align::Center);
        host.set_halign(gtk4::Align::End);
        widget.append(&items_box);
        widget.append(&host);

        // tsbLibrary / tsbFolders / tsbPages (`Resources.Library` /
        // `FileBrowser` (the GIF — the text-only fallback) /
        // `ComicPage`).
        let library_btn = tab_button("Library", crate::icon::icon("Library"));
        let folders_btn = tab_button("Folders", crate::icon::icon("FileBrowser"));
        let pages_btn = tab_button("Pages", crate::icon::icon("ComicPage"));
        let pages_root = gtk4::Box::new(Orientation::Horizontal, 0);
        pages_root.append(&pages_btn);
        // `tsbPages.Visible = OpenBooks.CurrentBook != null`.
        pages_root.set_visible(false);
        // The `+` tab (`Resources.AddTab`, Tag = -1 → `AddSlot`).
        let plus_btn = Button::new();
        plus_btn.add_css_class("flat");
        plus_btn.add_css_class("tab");
        plus_btn.set_valign(gtk4::Align::Center);
        plus_btn.set_tooltip_text(Some("New Tab"));
        if let Some(texture) = crate::icon::icon("AddTab") {
            let img = Image::from_paintable(Some(&texture));
            img.set_pixel_size(TAB_THUMB_PX);
            plus_btn.set_child(Some(&img));
        }
        let comic_box = gtk4::Box::new(Orientation::Horizontal, 2);
        items_box.append(&library_btn);
        items_box.append(&folders_btn);
        items_box.append(&pages_root);
        items_box.append(&comic_box);
        items_box.append(&plus_btn);

        let (tx, rx) = std::sync::mpsc::channel::<TabThumb>();
        let inner = Rc::new(Inner {
            widget,
            host,
            comic_box,
            library_btn,
            folders_btn,
            pages_btn,
            pages_root,
            plus_btn,
            comic_tabs: RefCell::new(Vec::new()),
            selected: RefCell::new(TabId::Library),
            on_select: RefCell::new(None),
            on_close: RefCell::new(None),
            pool,
            thumb_tx: tx,
            thumb_rx: rx,
            pending: Cell::new(0),
            queued: RefCell::new(HashSet::new()),
            pump_active: Cell::new(false),
        });
        let strip = TabStrip { inner };

        // Click handlers (the caption click fires for the ALREADY
        // selected item too — the C# `tab_CaptionClick` ToggleBrowser
        // rule; the shell decides).
        {
            let weak = Rc::downgrade(&strip.inner);
            strip.inner.library_btn.connect_clicked(move |_| {
                fire_select(&weak, &TabId::Library);
            });
        }
        {
            let weak = Rc::downgrade(&strip.inner);
            strip.inner.folders_btn.connect_clicked(move |_| {
                fire_select(&weak, &TabId::Folders);
            });
        }
        {
            let weak = Rc::downgrade(&strip.inner);
            strip.inner.pages_btn.connect_clicked(move |_| {
                fire_select(&weak, &TabId::Pages);
            });
        }
        {
            let weak = Rc::downgrade(&strip.inner);
            strip.inner.plus_btn.connect_clicked(move |_| {
                fire_select(&weak, &TabId::Plus);
            });
        }
        strip
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.inner.widget
    }

    /// The right-aligned host box (the reader toolbar's docked home
    /// — the undock chrome moves in and out of it).
    pub fn host(&self) -> &gtk4::Box {
        &self.inner.host
    }

    /// The shell callback: an item was clicked (fires on the
    /// selected item too).
    pub fn connect_select<F: Fn(&TabId) + 'static>(&self, f: F) {
        *self.inner.on_select.borrow_mut() = Some(Box::new(f));
    }

    /// The shell callback: a comic tab's close button (`CloseClick`).
    pub fn connect_close<F: Fn(usize) + 'static>(&self, f: F) {
        *self.inner.on_close.borrow_mut() = Some(Box::new(f));
    }

    /// Reconciles the comic tabs with the open slots (open/close/
    /// captions/bold) and queues the cover loads for new tabs.
    pub fn set_tabs(&self, infos: &[TabInfo]) {
        {
            let mut tabs = self.inner.comic_tabs.borrow_mut();
            // Drop the closed slots — the Rust handle is refcounted,
            // so the widget must leave its parent EXPLICITLY (the
            // T9 user test: the tab outlived its slot otherwise).
            tabs.retain(|t| {
                let keep = infos.iter().any(|i| i.slot == t.slot);
                if !keep {
                    self.inner.comic_box.remove(&t.root);
                }
                keep
            });
            for info in infos {
                match tabs.iter_mut().find(|t| t.slot == info.slot) {
                    Some(item) => {
                        if item.label.text() != info.caption {
                            item.label.set_text(&info.caption);
                        }
                        if info.current {
                            item.label.set_css_classes(&["tab-bold"]);
                        } else {
                            item.label.set_css_classes(&[]);
                        }
                    }
                    None => {
                        let item = self.build_comic_tab(info);
                        tabs.push(item);
                    }
                }
            }
        }
        // The slot order (an undock re-dock re-inserts mid-strip).
        // gtk_box_append asserts on a parented child — an item that
        // already sits in place stays; an out-of-order one moves
        // (remove + append).
        let tabs = self.inner.comic_tabs.borrow();
        for (i, info) in infos.iter().enumerate() {
            let Some(item) = tabs.iter().find(|t| t.slot == info.slot) else {
                continue;
            };
            let here = self
                .inner
                .comic_box
                .observe_children()
                .item(i as u32)
                .and_downcast::<gtk4::Widget>();
            let in_place = here.as_ref().is_some_and(|w| {
                std::ptr::eq(
                    w.upcast_ref::<glib::Object>().as_ptr(),
                    item.root.upcast_ref::<glib::Object>().as_ptr(),
                )
            });
            if in_place {
                continue;
            }
            if item.root.parent().is_some() {
                self.inner.comic_box.remove(&item.root);
            }
            self.inner.comic_box.append(&item.root);
        }
        drop(tabs);
        self.sync_classes();
        self.ensure_pump();
    }

    fn build_comic_tab(&self, info: &TabInfo) -> ComicTabItem {
        // The tab BOX carries the look (one bordered box); the
        // caption click and the close button live INSIDE it (the C#
        // TabBar shape — the T9 user test: a sibling X looked
        // disconnected).
        let root = gtk4::Box::new(Orientation::Horizontal, 0);
        root.add_css_class("tab");
        root.set_valign(gtk4::Align::Center);
        let child = gtk4::Box::new(Orientation::Horizontal, 6);
        let image = Image::new();
        image.set_pixel_size(TAB_THUMB_PX);
        child.append(&image);
        let label = Label::new(Some(&info.caption));
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        label.set_width_chars(12);
        label.set_max_width_chars(28);
        if info.current {
            label.set_css_classes(&["tab-bold"]);
        }
        child.append(&label);
        let button = Button::new();
        button.add_css_class("flat");
        button.add_css_class("tab-inner");
        button.set_child(Some(&child));
        button.set_tooltip_text(Some(&info.caption));
        let close = Button::from_icon_name("window-close-symbolic");
        close.add_css_class("flat");
        close.add_css_class("tab-close");
        close.set_tooltip_text(Some("Close"));
        root.append(&button);
        root.append(&close);
        let slot = info.slot;
        {
            let weak = Rc::downgrade(&self.inner);
            button.connect_clicked(move |_| fire_select(&weak, &TabId::Comic(slot)));
        }
        {
            let weak = Rc::downgrade(&self.inner);
            close.connect_clicked(move |_| {
                if let Some(inner) = weak.upgrade() {
                    if let Some(f) = inner.on_close.borrow().as_ref() {
                        f(slot);
                    }
                }
            });
        }
        // The async cover (`Create tab thumbnails`): one request per
        // slot; the result lands whenever the tab still exists. The
        // FRONT-COVER key (the shell's shared cover slot) — a plain
        // index-0 key would decode a second entry for every tab.
        if let Some(key) = info.cover_key.clone() {
            let mut queued = self.inner.queued.borrow_mut();
            if queued.insert(slot) {
                self.inner.pending.set(self.inner.pending.get() + 1);
                let pool = Arc::clone(&self.inner.pool);
                let tx = self.inner.thumb_tx.clone();
                self.inner
                    .pool
                    .add_thumb_to_queue(key.clone(), None, move |k| {
                        let bytes = pool.render_thumbnail(k);
                        let _ = tx.send(TabThumb { slot, bytes });
                    });
            }
        }
        ComicTabItem {
            slot,
            root,
            button,
            label,
            close,
            image,
        }
    }

    /// Marks the one selected item (the raised tab).
    pub fn set_selected(&self, id: &TabId) {
        *self.inner.selected.borrow_mut() = id.clone();
        self.sync_classes();
    }

    fn sync_classes(&self) {
        let sel = self.inner.selected.borrow();
        let mark = |button: &Button, active: bool| {
            if active {
                button.add_css_class("tab-active");
            } else {
                button.remove_css_class("tab-active");
            }
        };
        mark(&self.inner.library_btn, *sel == TabId::Library);
        mark(&self.inner.folders_btn, *sel == TabId::Folders);
        mark(&self.inner.pages_btn, *sel == TabId::Pages);
        let tabs = self.inner.comic_tabs.borrow();
        for item in tabs.iter() {
            mark(&item.button, *sel == TabId::Comic(item.slot));
        }
    }

    /// `tsbPages.Visible` (a book must be open).
    pub fn set_pages_visible(&self, visible: bool) {
        self.inner.pages_root.set_visible(visible);
    }

    /// `tsbFolders` removed under `DisableFoldersView` (the C#
    /// removes the tab item from the strip).
    pub fn set_folders_visible(&self, visible: bool) {
        self.inner.folders_btn.set_visible(visible);
    }

    /// The comic tabs + `+` hide while the reader is undocked (the
    /// C# `fileTab.Visible = BrowserDock == Fill && !ReaderUndocked`).
    pub fn set_comic_tabs_visible(&self, visible: bool) {
        self.inner.comic_box.set_visible(visible);
        self.inner.plus_btn.set_visible(visible);
    }

    fn ensure_pump(&self) {
        if self.inner.pump_active.get() || self.inner.pending.get() == 0 {
            return;
        }
        self.inner.pump_active.set(true);
        let weak = Rc::downgrade(&self.inner);
        glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
            let Some(inner) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            while let Ok(thumb) = inner.thumb_rx.try_recv() {
                inner.pending.set(inner.pending.get().saturating_sub(1));
                let Some(bytes) = thumb.bytes else {
                    continue;
                };
                let Some(texture) = texture_from_thumb_blob(&bytes) else {
                    continue;
                };
                let tabs = inner.comic_tabs.borrow();
                if let Some(item) = tabs.iter().find(|t| t.slot == thumb.slot) {
                    item.image.set_paintable(Some(&texture));
                }
            }
            if inner.pending.get() == 0 {
                inner.pump_active.set(false);
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    }

    // --- Probe accessors (the headless gates walk the real
    // widget path) ---

    pub fn comic_slots(&self) -> Vec<usize> {
        self.inner
            .comic_tabs
            .borrow()
            .iter()
            .map(|t| t.slot)
            .collect()
    }

    /// The comic-tab WIDGETS in the row (the model/widget agreement
    /// gate — dropping the Rust handle does not unparent a GTK
    /// widget, which the first probe missed).
    pub fn comic_tab_widgets(&self) -> usize {
        self.inner.comic_box.observe_children().n_items() as usize
    }

    pub fn selected(&self) -> TabId {
        self.inner.selected.borrow().clone()
    }

    pub fn tab_visible(&self, id: &TabId) -> bool {
        match id {
            TabId::Library => self.inner.library_btn.is_visible(),
            TabId::Folders => self.inner.folders_btn.is_visible(),
            TabId::Pages => self.inner.pages_root.is_visible(),
            TabId::Plus => self.inner.plus_btn.is_visible(),
            TabId::Comic(slot) => self
                .inner
                .comic_tabs
                .borrow()
                .iter()
                .find(|t| t.slot == *slot)
                .map(|t| t.root.is_visible())
                .unwrap_or(false),
        }
    }

    /// Clicks through the real widget path (`emit_clicked` — the
    /// strip's own handler runs).
    pub fn click(&self, id: &TabId) {
        match id {
            TabId::Library => self.inner.library_btn.emit_clicked(),
            TabId::Folders => self.inner.folders_btn.emit_clicked(),
            TabId::Pages => self.inner.pages_btn.emit_clicked(),
            TabId::Plus => self.inner.plus_btn.emit_clicked(),
            TabId::Comic(slot) => {
                let button = self
                    .inner
                    .comic_tabs
                    .borrow()
                    .iter()
                    .find(|t| t.slot == *slot)
                    .map(|t| t.button.clone());
                if let Some(button) = button {
                    button.emit_clicked();
                }
            }
        }
    }

    pub fn click_close(&self, slot: usize) {
        let close = self
            .inner
            .comic_tabs
            .borrow()
            .iter()
            .find(|t| t.slot == slot)
            .map(|t| t.close.clone());
        if let Some(close) = close {
            close.emit_clicked();
        }
    }

    pub fn comic_caption(&self, slot: usize) -> Option<String> {
        self.inner
            .comic_tabs
            .borrow()
            .iter()
            .find(|t| t.slot == slot)
            .map(|t| t.label.text().to_string())
    }

    /// The `FontBold` current-slot marker.
    pub fn tab_bold(&self, slot: usize) -> bool {
        self.inner
            .comic_tabs
            .borrow()
            .iter()
            .find(|t| t.slot == slot)
            .map(|t| t.label.has_css_class("tab-bold"))
            .unwrap_or(false)
    }
}

fn fire_select(weak: &Weak<Inner>, id: &TabId) {
    if let Some(inner) = weak.upgrade() {
        // The callback runs INSIDE the borrow: it re-enters the strip
        // (set_selected) but never connect_select.
        if let Some(f) = inner.on_select.borrow().as_ref() {
            f(id);
        }
    }
}

fn tab_button(label: &str, texture: Option<gdk::Texture>) -> Button {
    let button = Button::new();
    button.add_css_class("flat");
    button.add_css_class("tab");
    button.set_valign(gtk4::Align::Center);
    let child = gtk4::Box::new(Orientation::Horizontal, 6);
    if let Some(texture) = texture {
        let img = Image::from_paintable(Some(&texture));
        img.set_pixel_size(TAB_THUMB_PX);
        child.append(&img);
    }
    child.append(&Label::new(Some(label)));
    button.set_child(Some(&child));
    button
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
        &glib::Bytes::from(&*data),
        stride as usize,
    ))
}

#[cfg(test)]
mod tests {
    use super::tabstrip_visible;

    #[test]
    fn tabstrip_visible_follows_the_fill_rule() {
        // The browser shows: always visible (even MinimalGui).
        assert!(tabstrip_visible(true, false, false, 3, false));
        // The reader shows: hidden under MinimalGui.
        assert!(!tabstrip_visible(true, false, true, 3, false));
        // The reader shows without MinimalGui: visible.
        assert!(tabstrip_visible(false, false, true, 3, false));
        // MinimalGui + reader + ShowMainMenuNoComicOpen and NO open
        // books: the escape keeps the bar.
        assert!(tabstrip_visible(true, false, true, 0, true));
        // Same but a book IS open: hidden.
        assert!(!tabstrip_visible(true, false, true, 1, true));
        // Undocked: the C# sets TabBarVisible = true unconditionally.
        assert!(tabstrip_visible(true, true, true, 3, false));
    }
}
