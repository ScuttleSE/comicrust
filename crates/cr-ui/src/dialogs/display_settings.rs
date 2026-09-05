//! The Book Display Settings dialog — the
//! `ComicDisplaySettingsDialog` port (F9). The C# snapshots the live
//! `ComicDisplay`'s display options into a workspace copy
//! (`EditWorkspaceDisplaySettings` → `StoreWorkspace(ws)`), edits
//! that copy, and OK/Apply push it back through the `apply` callback
//! (`SetWorkspaceDisplayOptions`). The port snapshots the current
//! reader view (or the session copy with no view) and the shell
//! applies to every open view.
//!
//! Widget parity notes: the C# texture combos are owner-drawn
//! `ComboBoxSkinner` items with a sample swatch — the port lists the
//! display names only (recorded deviation). The percent trackbars
//! show a tooltip in the C#; the port carries a "N %" label. The
//! color picker is a `SimpleColorPicker` of known colors; the port
//! uses a `ColorButton` (free-form color, recorded deviation).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Dialog, Frame, Grid};

use crate::reader::page_view::{
    bundled_texture_files, parse_texture_file_name, texture_asset_path, DisplayOptions,
    ImageBackgroundMode, ImageLayout, PageTransitionEffect,
};

/// One texture combo row (the `TextureFileItem` shape: the first row
/// is the "Default"/"None" empty item; custom rows come from the
/// browse buttons — at most one custom row per combo, replaced on
/// every browse, the `SelectTextureFile` shape).
#[derive(Clone, PartialEq)]
pub struct TextureItem {
    pub path: Option<String>,
    pub custom: bool,
    pub label: String,
}

impl TextureItem {
    fn empty(label: &str) -> Self {
        TextureItem {
            path: None,
            custom: false,
            label: label.to_string(),
        }
    }

    fn bundled(file: &str, backgrounds: bool) -> Self {
        // The bundled file names parse their `[C]`/`[S]`/`[Z]` layout
        // code (`ParseFileName`); the display name drops the code.
        let (name, _) = parse_texture_file_name(file);
        TextureItem {
            path: texture_asset_path(backgrounds, file).map(|p| p.to_string_lossy().into_owned()),
            custom: false,
            label: name,
        }
    }

    fn custom(path: &str) -> Self {
        let label = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path)
            .to_string();
        TextureItem {
            path: Some(path.to_string()),
            custom: true,
            label,
        }
    }
}

/// A texture combo + its item model + the browse button.
struct TextureCombo {
    combo: gtk4::ComboBoxText,
    browse: gtk4::Button,
    items: Rc<RefCell<Vec<TextureItem>>>,
}

impl TextureCombo {
    fn new(
        empty_label: &str,
        bundled: &[String],
        backgrounds: bool,
        browse_title: &str,
        parent: &Dialog,
    ) -> Self {
        let combo = gtk4::ComboBoxText::new();
        let items = Rc::new(RefCell::new(vec![TextureItem::empty(empty_label)]));
        for file in bundled {
            items
                .borrow_mut()
                .push(TextureItem::bundled(file, backgrounds));
        }
        rebuild_combo(&combo, &items.borrow(), 0);
        combo.set_active(Some(0));
        let browse = gtk4::Button::with_label("...");
        {
            let items = Rc::clone(&items);
            let combo = combo.clone();
            let parent = parent.clone();
            let title = browse_title.to_string();
            browse.connect_clicked(move |_| {
                let chooser = gtk4::FileChooserNative::new(
                    Some(&title),
                    Some(&parent),
                    gtk4::FileChooserAction::Open,
                    Some("Select"),
                    Some("Cancel"),
                );
                let filter = gtk4::FileFilter::new();
                filter.set_name(Some("Images"));
                for ext in ["jpg", "jpeg", "bmp", "png", "gif", "tif", "tiff"] {
                    filter.add_pattern(&format!("*.{ext}"));
                }
                chooser.add_filter(&filter);
                let items = Rc::clone(&items);
                let combo = combo.clone();
                chooser.connect_response(move |dlg, resp| {
                    if resp != gtk4::ResponseType::Accept {
                        return;
                    }
                    if let Some(path) = dlg.file().and_then(|f| f.path()) {
                        dialogs_select_texture(&items, &combo, &path.to_string_lossy());
                    }
                });
                chooser.show();
            });
        }
        Self {
            combo,
            browse,
            items,
        }
    }

    fn is_selected_custom(&self) -> bool {
        let index = self.combo.active().unwrap_or_default() as usize;
        self.items
            .borrow()
            .get(index)
            .is_some_and(|item| item.custom)
    }

    fn is_empty_row_selected(&self) -> bool {
        self.combo.active().unwrap_or_default() == 0
    }

    /// The bundled file name behind a row (the layout-code parse
    /// input).
    fn file_name_at(&self, index: usize) -> Option<String> {
        let list = self.items.borrow();
        let path = list.get(index)?.path.as_deref()?;
        std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
    }
}

/// `SelectTextureFile`: find the path (case-insensitive), else
/// replace the last custom row / append one and select it. The combo
/// rebuilds from the model.
/// The selected item's path for a combo + model pair.
fn selected_path(
    items: &Rc<RefCell<Vec<TextureItem>>>,
    combo: &gtk4::ComboBoxText,
) -> Option<String> {
    let index = combo.active().unwrap_or_default() as usize;
    items.borrow().get(index).and_then(|item| item.path.clone())
}

fn dialogs_select_texture(
    items: &Rc<RefCell<Vec<TextureItem>>>,
    combo: &gtk4::ComboBoxText,
    path: &str,
) {
    {
        let list = items.borrow();
        if let Some(index) = list.iter().position(|item| {
            item.path
                .as_deref()
                .is_some_and(|p| p.eq_ignore_ascii_case(path))
        }) {
            combo.set_active(Some(index as u32));
            return;
        }
    }
    let mut list = items.borrow_mut();
    let previous = list.len();
    let new_item = TextureItem::custom(path);
    let index = match list.iter().rposition(|item| item.custom) {
        Some(last) => {
            list[last] = new_item;
            last
        }
        None => {
            list.push(new_item);
            list.len() - 1
        }
    };
    let rebuilt = list.clone();
    drop(list);
    // The rebuild + selection run OUTSIDE the borrow: set_active
    // fires the changed handlers, and they re-borrow the items (the
    // glib-reentrancy lesson from the view state).
    rebuild_combo(combo, &rebuilt, previous);
    combo.set_active(Some(index as u32));
}

fn rebuild_combo(combo: &gtk4::ComboBoxText, list: &[TextureItem], previous: usize) {
    // Remove every old row from the end (remove(i) shifts the rest).
    for i in (0..previous).rev() {
        combo.remove(i as i32);
    }
    for item in list {
        combo.append_text(&item.label);
    }
}
/// The probe/test handle over the dialog widgets (the menubar
/// `click_row` precedent — headless gates walk the real widgets).
#[derive(Clone)]
pub struct DisplaySettingsHandle {
    pub dialog: Dialog,
    pub transition: gtk4::ComboBoxText,
    pub realistic: gtk4::CheckButton,
    pub margin_check: gtk4::CheckButton,
    pub margin_scale: gtk4::Scale,
    pub bg_type: gtk4::ComboBoxText,
    pub color: gtk4::ColorButton,
    pub texture_combo: gtk4::ComboBoxText,
    pub texture_items: Rc<RefCell<Vec<TextureItem>>>,
    pub bg_layout: gtk4::ComboBoxText,
    pub paper_combo: gtk4::ComboBoxText,
    pub paper_items: Rc<RefCell<Vec<TextureItem>>>,
    pub strength_scale: gtk4::Scale,
    pub paper_layout: gtk4::ComboBoxText,
    pub texture_browse: gtk4::Button,
    pub paper_browse: gtk4::Button,
    /// The snapshot's background color (the fallback when the user
    /// never touches the picker).
    pub base_color: Option<[f32; 3]>,
    /// Set when the user picks a color (the picker's "color-set").
    /// Until then the snapshot's color rides (the ADR-025
    /// theme-following surround keeps working when the user leaves
    /// the picker alone).
    pub color_dirty: Rc<Cell<bool>>,
}

impl DisplaySettingsHandle {
    /// The row count of a texture combo.
    pub fn rows(combo: &gtk4::ComboBoxText) -> usize {
        combo.model().map_or(0, |m| {
            let mut n = 0;
            if let Some(mut it) = m.iter_first() {
                loop {
                    n += 1;
                    if !m.iter_next(&mut it) {
                        break;
                    }
                }
            }
            n
        })
    }

    /// The `SelectTextureFile` path for the probes: a bundled file
    /// name resolves through the asset roots and matches the bundled
    /// row; anything else rides as a custom (browsed) absolute path.
    pub fn select_texture(&self, background: bool, path: &str) {
        let (items, combo) = if background {
            (&self.texture_items, &self.texture_combo)
        } else {
            (&self.paper_items, &self.paper_combo)
        };
        let resolved = texture_asset_path(background, path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        dialogs_select_texture(items, combo, &resolved);
    }
}

/// Opens the dialog. `on_apply` runs for Apply and OK with the
/// widget-built options (the C# `Apply` delegate — the shell pushes
/// the options onto the open views). Returns the widget handle for
/// the headless gates.
pub fn show_display_settings(
    parent: &impl IsA<gtk4::Window>,
    opts: DisplayOptions,
    on_apply: impl Fn(&DisplayOptions) + 'static,
) -> DisplaySettingsHandle {
    let dialog = Dialog::builder()
        .title("Book Display Settings")
        .transient_for(parent)
        .modal(true)
        .default_width(440)
        .resizable(false)
        .build();
    dialog.add_button("OK", gtk4::ResponseType::Ok);
    dialog.add_button("Apply", gtk4::ResponseType::Apply);
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);

    let content = dialog.content_area();
    content.set_margin_top(10);
    content.set_margin_bottom(10);
    content.set_margin_start(10);
    content.set_margin_end(10);
    content.set_spacing(8);

    // ----- General -----
    let realistic = gtk4::CheckButton::with_label("Realistic Book Display");
    realistic.set_active(opts.realistic_pages);
    let margin_check = gtk4::CheckButton::with_label("Leave margins around the pages");
    margin_check.set_active(opts.page_margin);
    let margin_scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 50.0, 1.0);
    margin_scale.set_value((opts.page_margin_percent * 100.0).round().into());
    margin_scale.set_hexpand(true);
    let margin_label = gtk4::Label::new(Some(&format!("{} %", margin_scale.value() as i32)));
    {
        let margin_label = margin_label.clone();
        margin_scale.connect_value_changed(move |scale| {
            margin_label.set_text(&format!("{} %", scale.value() as i32));
        });
    }
    let (general_frame, general) = framed_grid("General");
    general.attach(&realistic, 0, 0, 3, 1);
    general.attach(&margin_check, 0, 1, 1, 1);
    general.attach(&margin_scale, 1, 1, 1, 1);
    general.attach(&margin_label, 2, 1, 1, 1);
    content.append(&general_frame);

    // ----- Effects -----
    let transition = gtk4::ComboBoxText::new();
    for item in [
        "No Page Transition Effect",
        "New Page fades in",
        "New Page scrolls in horizontally",
        "New Page scrolls in vertically",
        "Page Turn Effect",
    ] {
        transition.append_text(item);
    }
    transition.set_active(Some(opts.transition.as_index() as u32));
    let papers = bundled_texture_files(false);
    let paper = TextureCombo::new("Default", &papers, false, "Select a paper texture", &dialog);
    let strength_scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 100.0, 1.0);
    strength_scale.set_value((opts.paper_strength * 100.0).round().into());
    let strength_label = gtk4::Label::new(Some(&format!("{} %", strength_scale.value() as i32)));
    {
        let strength_label = strength_label.clone();
        strength_scale.connect_value_changed(move |scale| {
            strength_label.set_text(&format!("{} %", scale.value() as i32));
        });
    }
    let strength_caption = label("Strength:");
    let paper_layout = layout_combo(opts.paper_layout);
    let paper_layout_caption = label("Layout:");
    let (effects_frame, effects) = framed_grid("Effects");
    effects.attach(&label("Page Transition:"), 0, 0, 1, 1);
    effects.attach(&transition, 1, 0, 2, 1);
    effects.attach(&label("Paper:"), 0, 1, 1, 1);
    effects.attach(&paper.combo, 1, 1, 1, 1);
    effects.attach(&paper.browse, 2, 1, 1, 1);
    effects.attach(&strength_caption, 0, 2, 1, 1);
    effects.attach(&strength_scale, 1, 2, 1, 1);
    effects.attach(&strength_label, 2, 2, 1, 1);
    effects.attach(&paper_layout_caption, 0, 3, 1, 1);
    effects.attach(&paper_layout, 1, 3, 2, 1);
    content.append(&effects_frame);

    // ----- Background -----
    let bg_type = gtk4::ComboBoxText::new();
    for item in ["Adjust Color to current Page", "Solid Color", "Texture"] {
        bg_type.append_text(item);
    }
    bg_type.set_active(Some(opts.background_mode.as_index() as u32));
    let color = gtk4::ColorButton::default();
    {
        // The effective surround (the C# picker shows the stored
        // color; the port's None = the theme base).
        let (r, g, b) = match opts.background_color {
            Some(c) => (f64::from(c[0]), f64::from(c[1]), f64::from(c[2])),
            None => {
                let base = crate::theme::palette(&dialog).base;
                (base.0, base.1, base.2)
            }
        };
        color.set_rgba(&gtk4::gdk::RGBA::new(r as f32, g as f32, b as f32, 1.0));
    }
    let color_caption = label("Color:");
    let backgrounds = bundled_texture_files(true);
    let texture = TextureCombo::new(
        "None",
        &backgrounds,
        true,
        "Select a background texture",
        &dialog,
    );
    if let Some(path) = &opts.background_texture {
        dialogs_select_texture(&texture.items, &texture.combo, path);
    }
    if let Some(path) = &opts.paper_texture {
        dialogs_select_texture(&paper.items, &paper.combo, path);
    }
    let texture_caption = label("Texture:");
    let bg_layout = layout_combo(opts.background_layout);
    let bg_layout_caption = label("Layout:");
    let (background_frame, background) = framed_grid("Background");
    background.attach(&label("Type:"), 0, 0, 1, 1);
    background.attach(&bg_type, 1, 0, 2, 1);
    background.attach(&color_caption, 0, 1, 1, 1);
    background.attach(&color, 1, 1, 2, 1);
    background.attach(&texture_caption, 0, 2, 1, 1);
    background.attach(&texture.combo, 1, 2, 1, 1);
    background.attach(&texture.browse, 2, 2, 1, 1);
    background.attach(&bg_layout_caption, 0, 3, 1, 1);
    background.attach(&bg_layout, 1, 3, 2, 1);
    content.append(&background_frame);

    // ----- The visibility rules (the three SelectedIndexChanged
    // handlers) -----
    // Paper: the strength row shows for any non-empty selection; the
    // layout combo shows for CUSTOM paper only (a bundled paper's
    // layout parses from its file name and applies silently).
    {
        paper.combo.connect_changed({
            let strength_scale = strength_scale.clone();
            let strength_caption = strength_caption.clone();
            let strength_label = strength_label.clone();
            let paper_layout = paper_layout.clone();
            let paper_layout_caption = paper_layout_caption.clone();
            let paper = TextureCombo {
                combo: paper.combo.clone(),
                browse: paper.browse.clone(),
                items: Rc::clone(&paper.items),
            };
            move |combo| {
                let active = combo.active().unwrap_or_default();
                let visible = active != 0;
                strength_scale.set_visible(visible);
                strength_caption.set_visible(visible);
                strength_label.set_visible(visible);
                let custom = paper.is_selected_custom();
                paper_layout.set_visible(custom);
                paper_layout_caption.set_visible(custom);
                if !custom && active != 0 {
                    // The bundled layout applies from the file name
                    // (`cbPaperLayout.SelectedIndex = item.Layout`).
                    if let Some(file) = paper.file_name_at(active as usize) {
                        let (_, layout) = parse_texture_file_name(&file);
                        paper_layout.set_active(Some(layout.as_index() as u32));
                    }
                }
            }
        });
        // Initial state (the C# handlers fire from Update's
        // SelectedIndex sets).
        let empty = paper.is_empty_row_selected();
        let custom = paper.is_selected_custom();
        strength_scale.set_visible(!empty);
        strength_caption.set_visible(!empty);
        strength_label.set_visible(!empty);
        paper_layout.set_visible(custom);
        paper_layout_caption.set_visible(custom);
    }
    // Background: the color row on Solid Color, the texture row on
    // Texture, the layout combo on Texture + custom.
    {
        let update = {
            let bg_type = bg_type.clone();
            let texture_combo = texture.combo.clone();
            let texture_browse = texture.browse.clone();
            let texture_items = Rc::clone(&texture.items);
            let texture_caption = texture_caption.clone();
            let color = color.clone();
            let color_caption = color_caption.clone();
            let bg_layout = bg_layout.clone();
            let bg_layout_caption = bg_layout_caption.clone();
            move || {
                let mode = bg_type.active().unwrap_or_default();
                let color_visible = mode == 1;
                let texture_visible = mode == 2;
                texture_combo.set_visible(texture_visible);
                texture_browse.set_visible(texture_visible);
                texture_caption.set_visible(texture_visible);
                color.set_visible(color_visible);
                color_caption.set_visible(color_visible);
                let active = texture_combo.active().unwrap_or_default() as usize;
                let custom = texture_items
                    .borrow()
                    .get(active)
                    .is_some_and(|item| item.custom);
                let layout_visible = texture_visible && custom;
                bg_layout.set_visible(layout_visible);
                bg_layout_caption.set_visible(layout_visible);
                if std::env::var("CR_DEBUG_DSD").as_deref() == Ok("1") {
                    eprintln!(
                        "DSD update: mode={mode} active={active} custom={custom} layout_visible={layout_visible}"
                    );
                }
                if texture_visible && !custom {
                    // The bundled layout applies from the file name
                    // (`cbBackgroundTexture_SelectedIndexChanged`).
                    let texture = TextureCombo {
                        combo: texture_combo.clone(),
                        browse: texture_browse.clone(),
                        items: Rc::clone(&texture_items),
                    };
                    if let Some(file) = texture.file_name_at(active) {
                        let (_, layout) = parse_texture_file_name(&file);
                        bg_layout.set_active(Some(layout.as_index() as u32));
                    }
                }
            }
        };
        bg_type.connect_changed({
            let update = update.clone();
            move |_| update()
        });
        texture.combo.connect_changed({
            let update = update.clone();
            move |_| update()
        });
        update();
    }

    // ----- Apply (the `done` guard: a dialog that ends in close()
    // gets a RE-ENTRANT response — Phase 5 lesson) -----
    // The color rides only when the user actually picked one
    // (otherwise the ADR-025 theme-following surround keeps working).
    // The dirty flag rides the shared handle.
    let color_dirty = Rc::new(Cell::new(false));
    {
        let color_dirty = Rc::clone(&color_dirty);
        color.connect_color_set(move |_| color_dirty.set(true));
    }
    let on_apply = Rc::new(on_apply);
    let handle = DisplaySettingsHandle {
        dialog: dialog.clone(),
        transition: transition.clone(),
        realistic: realistic.clone(),
        margin_check: margin_check.clone(),
        margin_scale: margin_scale.clone(),
        bg_type: bg_type.clone(),
        color: color.clone(),
        texture_combo: texture.combo.clone(),
        texture_items: Rc::clone(&texture.items),
        bg_layout: bg_layout.clone(),
        paper_combo: paper.combo.clone(),
        paper_items: Rc::clone(&paper.items),
        strength_scale: strength_scale.clone(),
        paper_layout: paper_layout.clone(),
        texture_browse: texture.browse.clone(),
        paper_browse: paper.browse.clone(),
        base_color: opts.background_color,
        color_dirty: Rc::clone(&color_dirty),
    };
    {
        let done = Rc::new(Cell::new(false));
        let on_apply = Rc::clone(&on_apply);
        let handle = handle.clone();
        dialog.connect_response(move |dlg, response| match response {
            gtk4::ResponseType::Apply => {
                let options = build_options(&handle);
                on_apply(&options);
            }
            gtk4::ResponseType::Ok => {
                if done.replace(true) {
                    return;
                }
                let options = build_options(&handle);
                on_apply(&options);
                dlg.close();
            }
            gtk4::ResponseType::Cancel => {
                if done.replace(true) {
                    return;
                }
                dlg.close();
            }
            _ => {}
        });
    }
    dialog.present();
    handle
}

fn build_options(handle: &DisplaySettingsHandle) -> DisplayOptions {
    // The color rides only when the user actually picked one — the
    // dirty flag rides the dialog's object data (the response closure
    // works through a handle clone).
    let color_dirty = handle.color_dirty.get();
    let background_color: Option<[f32; 3]> = if color_dirty {
        let rgba = handle.color.rgba();
        Some([rgba.red(), rgba.green(), rgba.blue()])
    } else {
        handle.base_color
    };
    DisplayOptions {
        transition: PageTransitionEffect::from_index(handle.transition.active().unwrap_or(1) as i32),
        realistic_pages: handle.realistic.is_active(),
        page_margin: handle.margin_check.is_active(),
        page_margin_percent: handle.margin_scale.value() as f32 / 100.0,
        background_mode: ImageBackgroundMode::from_index(
            handle.bg_type.active().unwrap_or(1) as i32
        ),
        background_color,
        background_texture: selected_path(&handle.texture_items, &handle.texture_combo),
        background_layout: ImageLayout::from_index(handle.bg_layout.active().unwrap_or(1) as i32),
        paper_texture: selected_path(&handle.paper_items, &handle.paper_combo),
        paper_strength: handle.strength_scale.value() as f32 / 100.0,
        paper_layout: ImageLayout::from_index(handle.paper_layout.active().unwrap_or(1) as i32),
    }
}

// ----- widget helpers -----

fn label(text: &str) -> gtk4::Label {
    gtk4::Label::builder()
        .label(text)
        .halign(gtk4::Align::Start)
        .build()
}

fn layout_combo(initial: ImageLayout) -> gtk4::ComboBoxText {
    let combo = gtk4::ComboBoxText::new();
    for item in ["None", "Tile", "Center", "Stretch", "Zoom"] {
        combo.append_text(item);
    }
    combo.set_active(Some(initial.as_index() as u32));
    combo
}

/// A Frame (the C# GroupBox shape) + its inner Grid.
fn framed_grid(title: &str) -> (Frame, Grid) {
    let frame = Frame::new(Some(title));
    let grid = Grid::builder()
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .row_spacing(6)
        .column_spacing(6)
        .build();
    frame.set_child(Some(&grid));
    (frame, grid)
}
