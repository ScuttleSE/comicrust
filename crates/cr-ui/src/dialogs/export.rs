//! The export dialog — the `Dialogs/ExportComicsDialog.cs` port.
//!
//! The C# presets tree (ComicRack presets + user presets) becomes a
//! combo here: the ComicRack defaults (the engine-configuration
//! naming preset) plus the current session settings. The field set
//! covers the core export flow: target (New Folder / Same As
//! Source / Replace Source), the folder + naming template, the
//! format + compression, combine into one file, overwrite /
//! delete-original / add-to-library, the page format (Original /
//! JPEG / PNG / WebP — the exotic codecs report unsupported, the
//! Phase 1 gap) with quality, and keep-original-names.
//!
//! Deviations (recorded): the page filter/resize/double-page/
//! processing-slider groups stay out until a user needs them (the
//! sequential engine ignores them; the fields would lie); presets
//! persist only for the session (the C# keeps them in Config.xml —
//! the export_user_presets block joins when the settings schema
//! grows lists).

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, CheckButton, ComboBoxText, Dialog, Entry, Grid, Label,
    Orientation, SpinButton,
};

use cr_io::export::{
    export_book, export_books_combined, main_extension_for_format, target_path, ExportCompression,
    ExportNaming, ExportSetting, ExportTarget, StoragePageType,
};

/// The OK result: the settings the user settled on (the caller
/// persists them as the session default).
pub struct ExportDialogResult {
    pub setting: ExportSetting,
}

/// Opens the modal export dialog over `books` (clones with real
/// files). `on_done` runs once: `Some(result)` after the export
/// finished (the caller refreshes the library), `None` on
/// Cancel/errors-with-no-write.
pub fn show_export_dialog(
    parent: &impl IsA<gtk4::Window>,
    books: Vec<cr_core::model::comic_book::ComicBook>,
    captions: Vec<String>,
    setting: ExportSetting,
    on_done: impl Fn(Option<ExportDialogResult>) + 'static,
) {
    if books.is_empty() {
        return;
    }
    let dialog = Dialog::builder()
        .title("Export Books")
        .transient_for(parent)
        .modal(true)
        .default_width(520)
        .build();
    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_spacing(6);

    let grid = Grid::new();
    grid.set_row_spacing(4);
    grid.set_column_spacing(8);

    let mut row = 0;
    fn attach<W: IsA<gtk4::Widget>>(grid: &Grid, row: i32, caption: &str, w: &W) {
        grid.attach(&Label::new(Some(caption)), 0, row, 1, 1);
        grid.attach(w, 1, row, 1, 1);
    }

    // The target.
    let target_combo = ComboBoxText::new();
    for (id, label) in [
        ("0", "Export to new folder"),
        ("1", "Export next to source"),
        ("2", "Replace source"),
    ] {
        target_combo.append(Some(id), label);
    }
    target_combo.set_active(Some(match setting.target {
        ExportTarget::NewFolder => 0,
        ExportTarget::SameAsSource => 1,
        ExportTarget::ReplaceSource => 2,
        ExportTarget::Ask => 0,
    }));
    attach(&grid, row, "Target", &target_combo);
    row += 1;

    let folder_entry = Entry::builder()
        .text(&setting.target_folder)
        .hexpand(true)
        .build();
    let choose = Button::with_label("Choose…");
    let folder_row = GtkBox::new(Orientation::Horizontal, 6);
    folder_row.append(&folder_entry);
    folder_row.append(&choose);
    attach(&grid, row, "Folder", &folder_row);
    row += 1;

    // The format.
    let format_combo = ComboBoxText::new();
    for (i, (id, name)) in cr_io::export::EXPORT_FORMATS.iter().enumerate() {
        format_combo.append(Some(&id.to_string()), name);
        if *id == setting.format_id {
            format_combo.set_active(Some(i as u32));
        }
    }
    if format_combo.active().is_none() {
        format_combo.set_active(Some(0));
    }
    attach(&grid, row, "Format", &format_combo);
    row += 1;

    let compression_combo = ComboBoxText::new();
    for (id, label) in [("0", "Store"), ("1", "Medium"), ("2", "Strong")] {
        compression_combo.append(Some(id), label);
    }
    compression_combo.set_active(Some(setting.comic_compression as u32));
    attach(&grid, row, "Compression", &compression_combo);
    row += 1;

    // The naming.
    let naming_combo = ComboBoxText::new();
    for (id, label) in [
        ("0", "Use file name"),
        ("1", "Use display name"),
        ("2", "Use custom name"),
    ] {
        naming_combo.append(Some(id), label);
    }
    naming_combo.set_active(Some(setting.naming as u32));
    attach(&grid, row, "Naming", &naming_combo);
    row += 1;

    let custom_entry = Entry::builder()
        .text(&setting.custom_name)
        .hexpand(true)
        .build();
    attach(&grid, row, "Custom name", &custom_entry);
    row += 1;

    let custom_start = SpinButton::with_range(0.0, 10_000.0, 1.0);
    custom_start.set_value(setting.custom_naming_start as f64);
    attach(&grid, row, "Custom start index", &custom_start);
    row += 1;

    // The page format + quality.
    let page_combo = ComboBoxText::new();
    for (id, label) in [
        ("1", "Original"),
        ("2", "JPEG"),
        ("3", "PNG"),
        ("8", "WebP"),
    ] {
        page_combo.append(Some(id), label);
    }
    page_combo.set_active(Some(match setting.page_type {
        StoragePageType::Original => 0,
        StoragePageType::Jpeg => 1,
        StoragePageType::Png => 2,
        StoragePageType::Webp => 3,
        _ => 0,
    }));
    attach(&grid, row, "Page format", &page_combo);
    row += 1;

    let quality = SpinButton::with_range(0.0, 100.0, 1.0);
    quality.set_value(setting.page_compression as f64);
    attach(&grid, row, "Page quality", &quality);
    row += 1;

    // The flags.
    let flags = Grid::new();
    flags.set_row_spacing(2);
    flags.set_column_spacing(8);
    let combine_check = CheckButton::with_label("Combine into one file");
    combine_check.set_active(setting.combine);
    flags.attach(&combine_check, 0, 0, 1, 1);
    let overwrite_check = CheckButton::with_label("Overwrite existing files");
    overwrite_check.set_active(setting.overwrite);
    flags.attach(&overwrite_check, 0, 1, 1, 1);
    let delete_check = CheckButton::with_label("Delete original files after export");
    delete_check.set_active(setting.delete_original);
    flags.attach(&delete_check, 0, 2, 1, 1);
    let add_check = CheckButton::with_label("Add exported files to the library");
    add_check.set_active(setting.add_to_library);
    flags.attach(&add_check, 0, 3, 1, 1);
    let keep_names_check = CheckButton::with_label("Keep original page names");
    keep_names_check.set_active(setting.keep_original_image_names);
    flags.attach(&keep_names_check, 0, 4, 1, 1);
    attach(&grid, row, "Options", &flags);

    content.append(&grid);
    let error_label = Label::builder()
        .label("")
        .halign(Align::Start)
        .visible(false)
        .build();
    content.append(&error_label);
    let progress_label = Label::builder().label("").halign(Align::Start).build();
    content.append(&progress_label);

    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    let ok_button = dialog.add_button("Export", gtk4::ResponseType::Ok);
    ok_button.grab_focus();

    // The session settings object (edited by the widgets).
    let setting_rc = Rc::new(RefCell::new(setting));
    let sync: Rc<dyn Fn()> = Rc::new({
        let setting_rc = Rc::clone(&setting_rc);
        let target_combo = target_combo.clone();
        let folder_entry = folder_entry.clone();
        let format_combo = format_combo.clone();
        let compression_combo = compression_combo.clone();
        let naming_combo = naming_combo.clone();
        let custom_entry = custom_entry.clone();
        let custom_start = custom_start.clone();
        let page_combo = page_combo.clone();
        let quality = quality.clone();
        let combine_check = combine_check.clone();
        let overwrite_check = overwrite_check.clone();
        let delete_check = delete_check.clone();
        let add_check = add_check.clone();
        let keep_names_check = keep_names_check.clone();
        move || {
            let mut s = setting_rc.borrow_mut();
            s.target = match target_combo.active() {
                Some(1) => ExportTarget::SameAsSource,
                Some(2) => ExportTarget::ReplaceSource,
                _ => ExportTarget::NewFolder,
            };
            s.target_folder = folder_entry.text().to_string();
            s.format_id = format_combo
                .active_id()
                .and_then(|a| a.parse().ok())
                .unwrap_or(2);
            s.comic_compression = match compression_combo.active() {
                Some(1) => ExportCompression::Medium,
                Some(2) => ExportCompression::Strong,
                _ => ExportCompression::None,
            };
            s.naming = match naming_combo.active() {
                Some(1) => ExportNaming::Caption,
                Some(2) => ExportNaming::Custom,
                _ => ExportNaming::Filename,
            };
            s.custom_name = custom_entry.text().trim().to_string();
            s.custom_naming_start = custom_start.value() as i32;
            s.page_type = match page_combo.active() {
                Some(1) => StoragePageType::Jpeg,
                Some(2) => StoragePageType::Png,
                Some(3) => StoragePageType::Webp,
                _ => StoragePageType::Original,
            };
            s.page_compression = quality.value() as i32;
            s.combine = combine_check.is_active();
            s.overwrite = overwrite_check.is_active();
            s.delete_original = delete_check.is_active();
            s.add_to_library = add_check.is_active();
            s.keep_original_image_names = keep_names_check.is_active();
        }
    });
    let sync = sync;
    for combo in [
        &target_combo,
        &format_combo,
        &compression_combo,
        &naming_combo,
        &page_combo,
    ] {
        let value = Rc::clone(&sync);
        combo.connect_changed(move |_| value());
    }
    for entry in [&folder_entry, &custom_entry] {
        let value = Rc::clone(&sync);
        entry.connect_changed(move |_| value());
    }
    {
        let value = Rc::clone(&sync);
        custom_start.connect_value_changed(move |_| value());
    }
    for check in [
        &combine_check,
        &overwrite_check,
        &delete_check,
        &add_check,
        &keep_names_check,
    ] {
        let value = Rc::clone(&sync);
        check.connect_toggled(move |_| value());
    }
    sync();

    // The folder chooser (the target combo gates it — the OnIdle
    // enable/disable parity is the visibility here).
    {
        let folder_entry = folder_entry.clone();
        choose.connect_clicked(move |_| {
            let chooser = gtk4::FileChooserNative::new(
                Some("Select the export folder"),
                None::<&gtk4::Window>,
                gtk4::FileChooserAction::SelectFolder,
                Some("Select"),
                Some("Cancel"),
            );
            let folder_entry = folder_entry.clone();
            chooser.connect_response(move |dlg, resp| {
                if resp == gtk4::ResponseType::Accept {
                    if let Some(file) = dlg.file() {
                        if let Some(path) = file.path() {
                            let text = path.to_string_lossy().into_owned();
                            // The entry is the single source of truth:
                            // setting its text triggers the sync that
                            // stores it into the settings (a direct
                            // settings write would be overwritten by
                            // the next sync from the stale entry).
                            folder_entry.set_text(&text);
                        }
                    }
                }
            });
            chooser.show();
        });
    }

    // OK: run the export synchronously with progress (a local
    // export is fast; the C# queue's parallel machinery is not
    // needed — the progress label feeds per page).
    let done = Rc::new(RefCell::new(false));
    {
        let setting_rc = Rc::clone(&setting_rc);
        let progress_label = progress_label.clone();
        let error_label = error_label.clone();
        let done = Rc::clone(&done);
        let books = books.clone();
        dialog.connect_response(move |dlg, response| {
            if done.replace(true) {
                return;
            }
            match response {
                gtk4::ResponseType::Ok => {
                    let s = setting_rc.borrow().clone();
                    let total = books.len();
                    let mut errors: Vec<String> = Vec::new();
                    if s.combine {
                        progress_label.set_text("Exporting combined file…");
                        if let Err(err) =
                            export_books_combined(&s, &books, &captions, &|done, total| {
                                progress_label.set_text(&format!("Exporting… page {done}/{total}"));
                            })
                        {
                            errors.push(err.to_string());
                        }
                    } else {
                        for (i, book) in books.iter().enumerate() {
                            progress_label.set_text(&format!("Exporting {}/{}…", i + 1, total));
                            if let Err(err) =
                                export_book(&s, book, &captions[i], i, &|done, total| {
                                    progress_label.set_text(&format!(
                                        "Exporting {}/{} — page {done}/{total}",
                                        i + 1,
                                        total
                                    ));
                                })
                            {
                                errors.push(err.to_string());
                                break;
                            }
                        }
                    }
                    if errors.is_empty() {
                        dlg.close();
                        on_done(Some(ExportDialogResult { setting: s }));
                    } else {
                        error_label.set_text(&format!("Export failed: {}", errors.join("; ")));
                        error_label.set_visible(true);
                        progress_label.set_text("");
                        done.replace(false);
                    }
                }
                _ => {
                    dlg.close();
                    on_done(None);
                }
            }
        });
    }

    let _ = main_extension_for_format;
    let _ = target_path;
    dialog.present();
}
