//! The Preferences dialog shell (`Dialogs/PreferencesDialog.cs`).
//!
//! The C# hosts five tab panels (Reader, Behavior, Libraries,
//! Scripts, Advanced) driven by `tabButtons`; the Behavior page is
//! auto-filled by `FormUtility.FillPanelWithOptions`. The port keeps
//! the five-tab shape with a GTK sidebar; OK/Cancel edits a CLONE of
//! the settings and commits on OK (the C# edits `Program.Settings`
//! live but only persists + fires `SettingsChanged` on OK).
//!
//! Deviations (recorded): the Scripts page is hidden until the
//! plugin host (Phase 6); the language list is a placeholder until
//! the TR loader port; the backup/extension/file-association groups
//! are Windows-shell features (the Linux equivalents come with
//! packaging, Phase 8). The widgets stay on the GTK 4.0-era surface
//! (ADR-018): SpinButton + ComboBoxText, no SpinRow/DropDown.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, CheckButton, ComboBoxText, Dialog, Label, Orientation,
    ScrolledWindow, SpinButton, Stack, StackSidebar,
};

use crate::library;
use crate::settings::options::{section_label, OptionsPanel, SettingsRef};
use cr_core::settings::enums::RightToLeftReadingMode;
use cr_core::settings::settings::{
    MAXIMUM_MEMORY_PAGE_CACHE_COUNT, MAXIMUM_MEMORY_THUMBNAIL_CACHE_MB,
    MINIMUM_MEMORY_PAGE_CACHE_COUNT, MINIMUM_MEMORY_THUMBNAIL_CACHE_MB,
};

/// Opens the modal Preferences dialog. `on_ok` runs after the commit
/// (the host re-applies the live settings).
pub fn show_preferences(parent: &impl IsA<gtk4::Window>, on_ok: impl Fn() + 'static) {
    let session: SettingsRef = library::settings();
    // OK/Cancel semantics: edit a clone, commit on OK.
    let working: SettingsRef = Rc::new(RefCell::new(session.borrow().clone()));

    let dialog = Dialog::builder()
        .title("Preferences")
        .transient_for(parent)
        .modal(true)
        .default_width(780)
        .default_height(560)
        .build();
    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_spacing(6);

    let sidebar = StackSidebar::new();
    let stack = Stack::new();
    stack.set_vhomogeneous(false);
    stack.set_hhomogeneous(false);
    sidebar.set_stack(&stack);

    // ----- Reader (the display options the C# hand-builds) -----
    stack.add_titled(&build_reader_page(&working), Some("reader"), "Reader");

    // ----- Behavior (the auto-filled check-box groups) -----
    let behavior_panel = OptionsPanel::build();
    behavior_panel.refresh(&working);
    stack.add_titled(
        &wrap_scroll(&behavior_panel.widget().clone()),
        Some("behavior"),
        "Behavior",
    );

    // ----- Libraries (the watch folders) -----
    stack.add_titled(&build_libraries_page(), Some("libraries"), "Libraries");

    // ----- Advanced (the cache sizes + file update flow) -----
    stack.add_titled(&build_advanced_page(&working), Some("advanced"), "Advanced");

    let paned = gtk4::Paned::new(Orientation::Horizontal);
    paned.set_start_child(Some(&sidebar));
    paned.set_position(170);
    paned.set_shrink_start_child(false);
    paned.set_end_child(Some(&stack));
    paned.set_vexpand(true);
    content.append(&paned);

    dialog.add_button("OK", gtk4::ResponseType::Ok);
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);

    let working_commit = Rc::clone(&working);
    dialog.connect_response(move |dlg, response| {
        if response == gtk4::ResponseType::Ok {
            // The behavior rows read back on OK (`RetrieveOptionsFromPanel`),
            // the clone commits into the session, and the settings
            // file saves (the C# persists at app exit).
            behavior_panel.retrieve(&working_commit);
            *session.borrow_mut() = working_commit.borrow().clone();
            library::save_settings();
            on_ok();
        }
        dlg.close();
    });

    dialog.present();
}

fn wrap_scroll(w: &GtkBox) -> ScrolledWindow {
    ScrolledWindow::builder()
        .child(w)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .build()
}

/// A labeled `SpinButton` row (the WinForms NumericUpDown parity).
fn spin_row(title: &str, min: f64, max: f64, step: f64, value: f64) -> (GtkBox, SpinButton) {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    let label = Label::builder().label(title).halign(Align::Start).build();
    let spin = SpinButton::with_range(min, max, step);
    spin.set_value(value);
    spin.set_digits(1);
    row.append(&label);
    let spacer = Label::new(None);
    spacer.set_hexpand(true);
    row.append(&spacer);
    row.append(&spin);
    (row, spin)
}

/// The Reader page: the navigation options (`MainForm.UpdateSettings`
/// reads these from the settings on every change).
fn build_reader_page(settings: &SettingsRef) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 6);
    page.set_margin_top(8);
    page.set_margin_bottom(8);
    page.set_margin_start(8);
    page.set_margin_end(8);

    page.append(&section_label("Mouse"));
    let (row, wheel) = {
        let s = settings.borrow();
        spin_row(
            "Lines per mouse scrolling",
            0.5,
            10.0,
            0.5,
            s.mouse_wheel_speed as f64,
        )
    };
    {
        let settings = Rc::clone(settings);
        wheel.connect_value_changed(move |spin| {
            settings.borrow_mut().mouse_wheel_speed = spin.value() as f32;
        });
    }
    page.append(&row);

    page.append(&section_label("Right to Left"));
    let rtl_row = GtkBox::new(Orientation::Horizontal, 8);
    rtl_row.append(&Label::new(Some("Reading direction")));
    let rtl = ComboBoxText::new();
    rtl.append_text("Flip Parts");
    rtl.append_text("Flip Pages");
    {
        let s = settings.borrow();
        rtl.set_active(match s.right_to_left_reading_mode {
            RightToLeftReadingMode::FlipParts => Some(0),
            RightToLeftReadingMode::FlipPages => Some(1),
        });
    }
    {
        let settings = Rc::clone(settings);
        rtl.connect_changed(move |cb| {
            let mode = match cb.active() {
                Some(0) => RightToLeftReadingMode::FlipParts,
                _ => RightToLeftReadingMode::FlipPages,
            };
            settings.borrow_mut().right_to_left_reading_mode = mode;
        });
    }
    rtl_row.append(&rtl);
    page.append(&rtl_row);

    page.append(&section_label(
        "The reading check boxes (page-wall delay, browsing at the \
         margins, cursor hiding) live on the Behavior page.",
    ));
    page
}

/// The Libraries page: the database watch folders (`lbPaths` in the
/// C# — a folder per row with a Watch check).
fn build_libraries_page() -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 6);
    page.set_margin_top(8);
    page.set_margin_bottom(8);
    page.set_margin_start(8);
    page.set_margin_end(8);

    page.append(&section_label("Watch Folders"));
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    let mut rows: Vec<(String, CheckButton)> = Vec::new();
    {
        let lib = library::session();
        let l = lib.borrow();
        for wf in &l.database().watch_folders {
            let watch = gtk4::CheckButton::with_label(&wf.folder);
            watch.set_active(wf.watch);
            rows.push((wf.folder.clone(), watch));
        }
    }
    for (folder, watch) in &rows {
        // The Watch flag persists (the C# `lbPaths` checkbox writes
        // `watchFolder.Watch`).
        {
            let folder = folder.clone();
            let lib = library::session();
            let watch = watch.clone();
            watch.connect_toggled(move |cb| {
                let mut l = lib.borrow_mut();
                if let Some(w) = l
                    .database_mut()
                    .watch_folders
                    .iter_mut()
                    .find(|w| w.folder == folder)
                {
                    w.watch = cb.is_active();
                    l.mark_dirty();
                }
            });
        }
        list.append(watch);
    }

    let add = Button::with_label("Add Folder…");
    add.set_halign(Align::Start);
    {
        let list = list.clone();
        add.connect_clicked(move |_| {
            let list = list.clone();
            let chooser = gtk4::FileChooserNative::new(
                Some("Add Watch Folder"),
                None::<&gtk4::Window>,
                gtk4::FileChooserAction::SelectFolder,
                Some("Add"),
                Some("Cancel"),
            );
            chooser.connect_response(move |dlg, resp| {
                if resp == gtk4::ResponseType::Accept {
                    if let Some(file) = dlg.file() {
                        if let Some(path) = file.path() {
                            let folder = path.to_string_lossy().into_owned();
                            let lib = library::session();
                            let mut l = lib.borrow_mut();
                            // The C# rejects duplicates (`lbPaths` add
                            // checks the existing items).
                            if !l
                                .database()
                                .watch_folders
                                .iter()
                                .any(|w| w.folder == folder)
                            {
                                l.database_mut().watch_folders.push(
                                    cr_core::database::list_items::WatchFolder {
                                        folder: folder.clone(),
                                        watch: true,
                                    },
                                );
                                l.mark_dirty();
                                let watch = gtk4::CheckButton::with_label(&folder);
                                watch.set_active(true);
                                let folder2 = folder.clone();
                                let lib2 = library::session();
                                watch.connect_toggled(move |cb| {
                                    let mut l2 = lib2.borrow_mut();
                                    if let Some(w) = l2
                                        .database_mut()
                                        .watch_folders
                                        .iter_mut()
                                        .find(|w| w.folder == folder2)
                                    {
                                        w.watch = cb.is_active();
                                        l2.mark_dirty();
                                    }
                                });
                                list.append(&watch);
                            }
                        }
                    }
                }
            });
            chooser.show();
        });
    }
    page.append(&list);
    page.append(&add);
    page
}

/// The Advanced page: the cache sizes (the C# `numMemPageCount`
/// 20..100, `numMemThumbSize` 5..500, the disk-cache MB fields) and
/// the file-update flow (`chkUpdateComicFiles` chain).
fn build_advanced_page(settings: &SettingsRef) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 6);
    page.set_margin_top(8);
    page.set_margin_bottom(8);
    page.set_margin_start(8);
    page.set_margin_end(8);

    page.append(&section_label("Memory Caches"));
    let (row, spin) = {
        let s = settings.borrow();
        spin_row(
            "Pages to cache in memory",
            MINIMUM_MEMORY_PAGE_CACHE_COUNT as f64,
            MAXIMUM_MEMORY_PAGE_CACHE_COUNT as f64,
            1.0,
            s.memory_page_cache_count as f64,
        )
    };
    {
        let settings = Rc::clone(settings);
        spin.connect_value_changed(move |sp| {
            settings.borrow_mut().memory_page_cache_count = sp.value() as i32;
        });
    }
    page.append(&row);

    let (row, spin) = {
        let s = settings.borrow();
        spin_row(
            "Memory thumbnail cache size (MB)",
            MINIMUM_MEMORY_THUMBNAIL_CACHE_MB as f64,
            MAXIMUM_MEMORY_THUMBNAIL_CACHE_MB as f64,
            1.0,
            s.memory_thumb_cache_size_mb as f64,
        )
    };
    {
        let settings = Rc::clone(settings);
        spin.connect_value_changed(move |sp| {
            settings.borrow_mut().memory_thumb_cache_size_mb = sp.value() as i32;
        });
    }
    page.append(&row);

    page.append(&section_label("Disk Caches"));
    // The cache root (the C# `ExtendedSettings.CachePath` / the
    // `-cp` switch; a recorded ADDITION puts the editor here — the
    // C# boots it from the command line only). Takes effect on the
    // next start (the pool reads the paths once at boot).
    let cache_row = GtkBox::new(Orientation::Horizontal, 8);
    let cache_label = Label::builder()
        .label(cache_root_display())
        .ellipsize(gtk4::pango::EllipsizeMode::Start)
        .build();
    let change = Button::with_label("Change…");
    let reset = Button::with_label("Reset");
    reset.set_sensitive(cache_root_is_custom());
    cache_row.append(&cache_label);
    let spacer = Label::new(None);
    spacer.set_hexpand(true);
    cache_row.append(&spacer);
    cache_row.append(&change);
    cache_row.append(&reset);
    {
        let cache_label = cache_label.clone();
        let reset = reset.clone();
        change.connect_clicked(move |_| {
            let chooser = gtk4::FileChooserNative::new(
                Some("Cache Folder"),
                None::<&gtk4::Window>,
                gtk4::FileChooserAction::SelectFolder,
                Some("Select"),
                Some("Cancel"),
            );
            let cache_label = cache_label.clone();
            let reset = reset.clone();
            chooser.connect_response(move |dlg, resp| {
                if resp == gtk4::ResponseType::Accept {
                    if let Some(path) = dlg.file().and_then(|f| f.path()) {
                        set_cache_root(Some(&path), &cache_label, &reset);
                    }
                }
            });
            chooser.show();
        });
    }
    {
        let cache_label = cache_label.clone();
        let reset_clicked = reset.clone();
        let reset_target = reset.clone();
        reset_clicked.connect_clicked(move |_| {
            set_cache_root(None, &cache_label, &reset_target);
        });
    }
    page.append(&cache_row);
    page.append(&section_label(
        "The cache folder change takes effect on the next start.",
    ));

    page.append(&section_label("Disk Cache Sizes (MB)"));
    let (row, spin) = {
        let s = settings.borrow();
        spin_row(
            "Thumbnail cache size",
            0.0,
            100000.0,
            50.0,
            s.thumb_cache_size_mb as f64,
        )
    };
    {
        let settings = Rc::clone(settings);
        spin.connect_value_changed(move |sp| {
            settings.borrow_mut().thumb_cache_size_mb = sp.value() as i32;
        });
    }
    page.append(&row);

    let (row, spin) = {
        let s = settings.borrow();
        spin_row(
            "Page cache size",
            0.0,
            100000.0,
            50.0,
            s.page_cache_size_mb as f64,
        )
    };
    {
        let settings = Rc::clone(settings);
        spin.connect_value_changed(move |sp| {
            settings.borrow_mut().page_cache_size_mb = sp.value() as i32;
        });
    }
    page.append(&row);

    page.append(&section_label("Book File Updates"));
    // The C# `OnIdle` chain: the extra/auto boxes enable only when
    // the main update box is checked, and uncheck with it.
    let main_check = CheckButton::with_label("Update Book Files with new information");
    main_check.set_active(settings.borrow().update_comic_files);
    let extra_check = CheckButton::with_label("Update Book Files with extra information");
    extra_check.set_active(settings.borrow().update_comic_book_files);
    extra_check.set_sensitive(main_check.is_active());
    let auto_check = CheckButton::with_label("Auto update of Book files");
    auto_check.set_active(settings.borrow().auto_update_comics_files);
    auto_check.set_sensitive(main_check.is_active());
    {
        let settings = Rc::clone(settings);
        let extra = extra_check.clone();
        let auto = auto_check.clone();
        main_check.connect_toggled(move |cb| {
            let on = cb.is_active();
            let mut s = settings.borrow_mut();
            s.update_comic_files = on;
            if !on {
                s.update_comic_book_files = false;
                s.auto_update_comics_files = false;
            }
            drop(s);
            extra.set_sensitive(on);
            auto.set_sensitive(on);
            if !on {
                extra.set_active(false);
                auto.set_active(false);
            }
        });
    }
    {
        let settings = Rc::clone(settings);
        extra_check.connect_toggled(move |cb| {
            settings.borrow_mut().update_comic_book_files = cb.is_active();
        });
    }
    {
        let settings = Rc::clone(settings);
        auto_check.connect_toggled(move |cb| {
            settings.borrow_mut().auto_update_comics_files = cb.is_active();
        });
    }
    page.append(&main_check);
    page.append(&extra_check);
    page.append(&auto_check);

    page
}

/// The effective cache root (the override when set, the XDG data
/// tree otherwise).
fn cache_root_display() -> String {
    let paths = cr_core::paths::Paths::new_default();
    let root = paths
        .thumbnail_cache_path
        .parent()
        .unwrap_or(&paths.thumbnail_cache_path);
    root.to_string_lossy().into_owned()
}

fn cache_root_is_custom() -> bool {
    cr_core::settings::ExtendedSettings::global()
        .cache_path
        .as_deref()
        .is_some_and(|p| !p.is_empty())
}

/// Writes the cache root: `Some(path)` = the override (the global +
/// the ini chain's last file — the theme-persistence pattern), None
/// = back to the default. Live for `Paths::new_default()` readers
/// (the pool consumes it on the next start).
fn set_cache_root(path: Option<&std::path::Path>, label: &Label, reset: &Button) {
    {
        let mut ext = cr_core::settings::ExtendedSettings::global_mut();
        ext.cache_path = path.map(|p| p.to_string_lossy().into_owned());
    }
    match path {
        Some(p) => library::save_ini_keys(&[("CachePath", p.to_string_lossy().as_ref())]),
        None => library::save_ini_keys(&[("CachePath", "")]),
    }
    label.set_text(&cache_root_display());
    reset.set_sensitive(cache_root_is_custom());
}
