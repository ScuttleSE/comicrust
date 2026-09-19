//! The Preferences dialog shell (`Dialogs/PreferencesDialog.cs`).
//!
//! The C# hosts five tab panels (Reader, Behavior, Libraries,
//! Scripts, Advanced) driven by `tabButtons`; the Behavior page is
//! auto-filled by `FormUtility.FillPanelWithOptions`. The port keeps
//! the five-tab shape with a GTK sidebar; OK/Cancel edits a CLONE of
//! the settings and commits on OK (the C# edits `Program.Settings`
//! live but only persists + fires `SettingsChanged` on OK).
//!
//! Deviations (recorded): the Scripts page is removed permanently —
//! no scripting host (ADR-027); the language list is a placeholder
//! until the TR loader port; the backup/extension/file-association
//! groups are Windows-shell features (the Linux equivalents come with
//! packaging, Phase 8). The widgets stay on the GTK 4.0-era surface
//! (ADR-018): SpinButton + ComboBoxText, no SpinRow/DropDown.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
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

/// Opens the modal Preferences dialog. `initial` selects the page
/// shown on open (the page name, e.g. `Some("scraper")` for the
/// Comic Vine Scraper entry). `on_ok` runs after the commit (the
/// host re-applies the live settings).
pub fn show_preferences(
    parent: &impl IsA<gtk4::Window>,
    initial: Option<&str>,
    on_ok: impl Fn() + 'static,
) {
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

    // ----- Libraries (the watch folders; staged, committed on OK) -----
    let (libraries_page, folder_commit, find_profile_commit, original_roles) =
        build_libraries_page();
    stack.add_titled(&libraries_page, Some("libraries"), "Libraries");

    // ----- Advanced (the cache sizes + file update flow) -----
    stack.add_titled(&build_advanced_page(&working), Some("advanced"), "Advanced");

    // ----- Duplicates (PORT ADDITION, no C# counterpart — the
    // ADR-044 cleanup rules) -----
    stack.add_titled(
        &build_duplicates_page(&working),
        Some("duplicates"),
        "Duplicates",
    );

    // ----- Comic Vine Scraper (the plugin Configuration; the C#
    // plugin carries its own config form) -----
    let scraper =
        crate::dialogs::scrape_config::ScrapeConfigWidgets::build(&library::scraper_config());
    {
        let page = GtkBox::new(Orientation::Vertical, 6);
        page.set_margin_top(8);
        page.set_margin_bottom(8);
        page.set_margin_start(8);
        page.set_margin_end(8);
        page.append(&scraper.grid);
        stack.add_titled(&wrap_scroll(&page), Some("scraper"), "Comic Vine Scraper");
    }

    if let Some(name) = initial {
        stack.set_visible_child_name(name);
    }

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
    let on_ok = Rc::new(RefCell::new(Some(Box::new(on_ok) as Box<dyn FnOnce()>)));
    dialog.connect_response(move |dlg, response| {
        if response != gtk4::ResponseType::Ok {
            dlg.close();
            return;
        }
        behavior_panel.retrieve(&working_commit);
        let roles = folder_commit.borrow().clone();
        if let Err(message) = cr_engine::incoming::validate_folder_roots(&roles) {
            show_preferences_message(
                dlg,
                gtk4::MessageType::Error,
                "Invalid folder roles",
                &message,
            );
            return;
        }
        let incoming_books = library::incoming_books_snapshot();
        if let Err(message) = cr_engine::incoming::validate_unresolved_removals(
            &original_roles,
            &roles,
            &incoming_books,
        ) {
            show_preferences_message(
                dlg,
                gtk4::MessageType::Error,
                "Folder role cannot change",
                &message,
            );
            return;
        }
        let mut old_config = library::incoming_config();
        old_config.find_in_incoming_profile = find_profile_commit.borrow().clone();
        let discovery_old_config = old_config.clone();
        let discovery_roles = roles.clone();
        let confirmation_parent = dlg.clone();
        let commit = {
            let dlg = dlg.clone();
            let working = working_commit.borrow().clone();
            let scraper_config = scraper.collect();
            let on_ok = Rc::clone(&on_ok);
            move || {
                commit_folder_roles_async(
                    &dlg,
                    working,
                    scraper_config,
                    roles,
                    old_config,
                    move || {
                        if let Some(callback) = on_ok.borrow_mut().take() {
                            callback();
                        }
                    },
                );
            }
        };
        let library_books = library::session().borrow().database().books.clone();
        discover_conversion_count_async(
            dlg,
            library_books,
            discovery_old_config,
            discovery_roles,
            move |transfer_count| {
                if transfer_count == 0 {
                    commit();
                } else {
                    confirm_conversion(&confirmation_parent, transfer_count, commit);
                }
            },
        );
    });

    // The cache-folder row writes the ini at click time (the
    // theme-persistence pattern); CANCEL restores the value the
    // dialog opened with (the OK-only commit parity).
    let open_cache_path = cr_core::settings::ExtendedSettings::global()
        .cache_path
        .clone();
    dialog.connect_response(move |dlg, response| {
        if response != gtk4::ResponseType::Ok {
            if cr_core::settings::ExtendedSettings::global().cache_path != open_cache_path {
                let mut ext = cr_core::settings::ExtendedSettings::global_mut();
                ext.cache_path = open_cache_path.clone();
                drop(ext);
                match open_cache_path.as_deref() {
                    Some(p) => library::save_ini_keys(&[("CachePath", p)]),
                    None => library::save_ini_keys(&[("CachePath", "")]),
                }
            }
            dlg.close();
        }
    });
    dialog.present();
}

fn wrap_scroll(w: &impl IsA<gtk4::Widget>) -> ScrolledWindow {
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

/// The staged watch-folder list the Libraries page edits; the OK
/// handler commits it through `Library::set_watch_folders`.
type StagedFolderRoles = Rc<RefCell<Vec<cr_engine::incoming::FolderRole>>>;
type StagedProfile = Rc<RefCell<String>>;

/// The Libraries page: the database watch folders (`lbPaths` in the
/// C# — a folder per row with a Watch check). The C# edits the list
/// box in memory and copies into `Program.Database.WatchFolders` on
/// OK (`CopyWatchFoldersToDatabase`); the port stages the same way and
/// returns the staged list for the OK commit (`show_preferences`).
/// Cancel drops the staging, so a toggle, an add, or a remove reverts.
fn build_libraries_page() -> (
    GtkBox,
    StagedFolderRoles,
    StagedProfile,
    Vec<cr_engine::incoming::FolderRole>,
) {
    let page = GtkBox::new(Orientation::Vertical, 6);
    page.set_margin_top(8);
    page.set_margin_bottom(8);
    page.set_margin_start(8);
    page.set_margin_end(8);

    page.append(&section_label("Library Folders"));
    let incoming = library::incoming_config();
    let mut original_roles = {
        let lib = library::session();
        let l = lib.borrow();
        l.database()
            .watch_folders
            .iter()
            .map(|folder| cr_engine::incoming::FolderRole {
                folder: folder.folder.clone(),
                incoming: incoming
                    .incoming_folders
                    .iter()
                    .any(|root| cr_engine::incoming::roots_equal(root, &folder.folder)),
                watch: folder.watch,
            })
            .collect::<Vec<_>>()
    };
    for root in &incoming.incoming_folders {
        if !original_roles
            .iter()
            .any(|role| cr_engine::incoming::roots_equal(&role.folder, root))
        {
            original_roles.push(cr_engine::incoming::FolderRole {
                folder: root.clone(),
                incoming: true,
                watch: true,
            });
        }
    }
    let staged: StagedFolderRoles = Rc::new(RefCell::new(original_roles.clone()));
    let list = gtk4::ListBox::new();
    // The C# `lbPaths` carries a SelectedIndex that gates btRemove
    // (`btRemoveFolder.Enabled = lbPaths.SelectedIndex != -1`).
    list.set_selection_mode(gtk4::SelectionMode::Single);
    refill_watch_rows(&list, &staged);

    let remove = Button::with_label("Remove");
    remove.set_halign(Align::Start);
    remove.set_sensitive(false);
    {
        let remove = remove.clone();
        list.connect_row_selected(move |_list, row| {
            remove.set_sensitive(row.is_some());
        });
    }
    {
        let list = list.clone();
        let staged = Rc::clone(&staged);
        remove.connect_clicked(move |_| {
            let Some(index) = list.selected_row().map(|row| row.index() as usize) else {
                return;
            };
            if index >= staged.borrow().len() {
                return;
            }
            staged.borrow_mut().remove(index);
            refill_watch_rows(&list, &staged);
        });
    }

    let add = Button::with_label("Add Folder…");
    add.set_halign(Align::Start);
    {
        let list = list.clone();
        let staged = Rc::clone(&staged);
        add.connect_clicked(move |_| {
            let list = list.clone();
            let staged = Rc::clone(&staged);
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
                            // The C# drag-drop rejects duplicates
                            // (`lbPaths_DragDrop` checks the existing
                            // items); the port rejects them on add.
                            let mut staged_list = staged.borrow_mut();
                            if !staged_list.iter().any(|w| w.folder == folder) {
                                staged_list.push(cr_engine::incoming::FolderRole {
                                    folder,
                                    incoming: false,
                                    watch: true,
                                });
                                drop(staged_list);
                                refill_watch_rows(&list, &staged);
                            }
                        }
                    }
                }
            });
            chooser.show();
        });
    }
    let buttons = GtkBox::new(Orientation::Horizontal, 6);
    buttons.append(&add);
    buttons.append(&remove);
    page.append(&list);
    page.append(&buttons);

    page.append(&section_label("Incoming"));
    let profile_row = GtkBox::new(Orientation::Horizontal, 8);
    profile_row.append(&Label::new(Some("Find in Incoming profile")));
    let profile_combo = ComboBoxText::new();
    profile_combo.append_text("Not configured");
    let move_profiles: Vec<String> = library::organize_settings()
        .profiles
        .into_iter()
        .filter(|profile| profile.mode == cr_organize::profile::MODE_MOVE)
        .map(|profile| profile.name)
        .collect();
    for name in &move_profiles {
        profile_combo.append_text(name);
    }
    let selected_profile = move_profiles
        .iter()
        .position(|name| name == &incoming.find_in_incoming_profile)
        .map(|index| index as u32 + 1)
        .unwrap_or(0);
    profile_combo.set_active(Some(selected_profile));
    profile_combo.set_hexpand(true);
    let staged_profile: StagedProfile = Rc::new(RefCell::new(if selected_profile == 0 {
        String::new()
    } else {
        move_profiles[(selected_profile - 1) as usize].clone()
    }));
    {
        let staged_profile = Rc::clone(&staged_profile);
        profile_combo.connect_changed(move |combo| {
            *staged_profile.borrow_mut() = combo
                .active()
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| move_profiles.get(index as usize))
                .cloned()
                .unwrap_or_default();
        });
    }
    profile_row.append(&profile_combo);
    page.append(&profile_row);

    (page, staged, staged_profile, original_roles)
}

/// Rebuilds the watch-folder rows from the staged list (one
/// CheckButton per folder; the check writes `WatchFolder.Watch` in
/// the staging — the C# `lbPaths` item check).
fn refill_watch_rows(list: &gtk4::ListBox, staged: &StagedFolderRoles) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    for role in staged.borrow().iter() {
        let row = GtkBox::new(Orientation::Horizontal, 8);
        let folder_label = Label::builder()
            .label(&role.folder)
            .halign(Align::Start)
            .hexpand(true)
            .build();
        let role_combo = ComboBoxText::new();
        role_combo.append_text("Library");
        role_combo.append_text("Incoming");
        role_combo.set_active(Some(u32::from(role.incoming)));
        let watch = CheckButton::with_label("Watch");
        watch.set_active(role.incoming || role.watch);
        watch.set_sensitive(!role.incoming);
        let folder = role.folder.clone();
        let staged_for_role = Rc::clone(staged);
        let watch_for_role = watch.clone();
        role_combo.connect_changed(move |combo| {
            let incoming = combo.active() == Some(1);
            if let Some(role) = staged_for_role
                .borrow_mut()
                .iter_mut()
                .find(|role| role.folder == folder)
            {
                role.incoming = incoming;
            }
            watch_for_role.set_active(true);
            watch_for_role.set_sensitive(!incoming);
        });
        let folder = role.folder.clone();
        let staged = Rc::clone(staged);
        watch.connect_toggled(move |check| {
            if !check.is_sensitive() {
                return;
            }
            if let Some(role) = staged
                .borrow_mut()
                .iter_mut()
                .find(|role| role.folder == folder)
            {
                role.watch = check.is_active();
            }
        });
        row.append(&folder_label);
        row.append(&role_combo);
        row.append(&watch);
        list.append(&row);
    }
}

fn show_preferences_message(
    parent: &impl IsA<gtk4::Window>,
    kind: gtk4::MessageType,
    heading: &str,
    message: &str,
) {
    let dialog = gtk4::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .message_type(kind)
        .text(heading)
        .secondary_text(message)
        .buttons(gtk4::ButtonsType::Close)
        .build();
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.present();
}

fn confirm_conversion(
    parent: &impl IsA<gtk4::Window>,
    count: usize,
    commit: impl FnOnce() + 'static,
) {
    let dialog = gtk4::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .message_type(gtk4::MessageType::Question)
        .text(format!(
            "Move {count} library record(s) to the Incoming catalog?"
        ))
        .secondary_text("The records keep their IDs and saved-list references.")
        .build();
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    dialog.add_button("Move and Save", gtk4::ResponseType::Ok);
    let commit = Rc::new(RefCell::new(Some(commit)));
    dialog.connect_response(move |dialog, response| {
        dialog.close();
        if response == gtk4::ResponseType::Ok {
            if let Some(commit) = commit.borrow_mut().take() {
                commit();
            }
        }
    });
    dialog.present();
}

fn discover_conversion_count_async(
    parent: &Dialog,
    books: Vec<cr_core::model::comic_book::ComicBook>,
    old_config: cr_engine::incoming::IncomingConfig,
    roles: Vec<cr_engine::incoming::FolderRole>,
    done: impl FnOnce(usize) + 'static,
) {
    let ok_button = parent.widget_for_response(gtk4::ResponseType::Ok);
    if let Some(button) = ok_button.as_ref() {
        button.set_sensitive(false);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let spawn = std::thread::Builder::new()
        .name("Incoming Conversion Discovery".into())
        .spawn(move || {
            let count = cr_engine::incoming::conversion_indexes(&books, &old_config, &roles).len();
            let _ = tx.send(count);
        });
    if let Err(error) = spawn {
        if let Some(button) = ok_button.as_ref() {
            button.set_sensitive(true);
        }
        show_preferences_message(
            parent,
            gtk4::MessageType::Error,
            "Preferences were not saved",
            &format!("The conversion discovery worker could not start: {error}"),
        );
        return;
    }
    let parent = parent.clone();
    let mut done = Some(done);
    glib::timeout_add_local(std::time::Duration::from_millis(25), move || {
        match rx.try_recv() {
            Ok(count) => {
                if let Some(button) = ok_button.as_ref() {
                    button.set_sensitive(true);
                }
                if parent.is_visible() {
                    if let Some(done) = done.take() {
                        done(count);
                    }
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if let Some(button) = ok_button.as_ref() {
                    button.set_sensitive(true);
                }
                if parent.is_visible() {
                    show_preferences_message(
                        &parent,
                        gtk4::MessageType::Error,
                        "Preferences were not saved",
                        "The conversion discovery worker stopped without a result.",
                    );
                }
                glib::ControlFlow::Break
            }
        }
    });
}

fn commit_folder_roles_async(
    dialog: &Dialog,
    settings: cr_core::settings::Settings,
    scraper: cr_scrape::config::Configuration,
    roles: Vec<cr_engine::incoming::FolderRole>,
    mut incoming_config: cr_engine::incoming::IncomingConfig,
    on_ok: impl FnOnce() + 'static,
) {
    if cr_engine::incoming_transaction::operation_active() {
        show_preferences_message(
            dialog,
            gtk4::MessageType::Error,
            "Preferences were not saved",
            "Another Incoming operation is active. Try again after it finishes.",
        );
        return;
    }
    let Some(incoming_table) = cr_core::settings::unified::serialize_plugin(&incoming_config)
    else {
        show_preferences_message(
            dialog,
            gtk4::MessageType::Error,
            "Preferences were not saved",
            "The Incoming configuration could not be serialized.",
        );
        return;
    };
    let Some(scraper_table) = cr_core::settings::unified::serialize_plugin(&scraper) else {
        show_preferences_message(
            dialog,
            gtk4::MessageType::Error,
            "Preferences were not saved",
            "The scraper configuration could not be serialized.",
        );
        return;
    };
    let paths = cr_core::paths::Paths::new_default();
    let database_file = cr_core::paths::database_file(&paths);
    let config_file = cr_core::paths::config_file(&paths);
    let replacements = vec![
        (library::INCOMING_PLUGIN.to_string(), incoming_table),
        (library::SCRAPER_PLUGIN.to_string(), scraper_table),
    ];
    let worker_settings = settings.clone();
    let mut worker_database = library::session().borrow().database().clone();
    let mut worker_incoming = library::incoming_session().borrow().clone();
    let captured_epoch = cr_engine::incoming_transaction::database_epoch();
    let (tx, rx) = std::sync::mpsc::channel();
    if !cr_engine::incoming_transaction::begin_operation() {
        show_preferences_message(
            dialog,
            gtk4::MessageType::Error,
            "Preferences were not saved",
            "Another operation is active. Try again after it finishes.",
        );
        return;
    }
    let spawn = std::thread::Builder::new()
        .name("preferences-save".into())
        .spawn(move || {
            let result = (|| -> Result<_, String> {
                let _guard = cr_engine::incoming_transaction::acquire_mutation_guard();
                if cr_engine::incoming_transaction::database_epoch() != captured_epoch {
                    return Err(
                        "The library changed before the folder conversion started. Try again."
                            .into(),
                    );
                }
                let database_before =
                    cr_core::database::comic_database::save_bytes(&worker_database)
                        .map_err(|error| error.to_string())?;
                let incoming_before = worker_incoming
                    .to_bytes()
                    .map_err(|error| error.to_string())?;
                cr_engine::incoming::transfer_new_incoming_records(
                    &mut worker_database,
                    &mut worker_incoming,
                    &incoming_config,
                    &roles,
                );
                worker_database.watch_folders = roles
                    .iter()
                    .map(|role| cr_core::database::list_items::WatchFolder {
                        folder: role.folder.clone(),
                        watch: role.incoming || role.watch,
                    })
                    .collect();
                incoming_config.incoming_folders = roles
                    .iter()
                    .filter(|role| role.incoming)
                    .map(|role| role.folder.clone())
                    .collect();
                let mut transaction = cr_engine::incoming_transaction::IncomingTransaction {
                    kind: cr_engine::incoming_transaction::TransactionKind::FolderConversion,
                    stage: cr_engine::incoming_transaction::TransactionStage::Prepared,
                    files: cr_engine::incoming_transaction::TransactionFiles {
                        incoming_catalog: Some(cr_engine::incoming_transaction::FileSnapshot {
                            before: Some(incoming_before),
                            after: worker_incoming
                                .to_bytes()
                                .map_err(|error| error.to_string())?,
                            path: cr_core::paths::incoming_file(&paths),
                            remove_after: false,
                        }),
                        comic_database: Some(cr_engine::incoming_transaction::FileSnapshot {
                            before: Some(database_before),
                            after: cr_core::database::comic_database::save_bytes(&worker_database)
                                .map_err(|error| error.to_string())?,
                            path: database_file,
                            remove_after: false,
                        }),
                        config: Some(cr_engine::incoming_transaction::FileSnapshot {
                            before: std::fs::read(&config_file).ok(),
                            after: cr_core::settings::unified::save_bytes_with_plugin_tables(
                                &worker_settings,
                                &replacements,
                            )
                            .map_err(|error| error.to_string())?,
                            path: config_file,
                            remove_after: false,
                        }),
                        auxiliary: Vec::new(),
                    },
                    external_actions: Vec::new(),
                };
                let engine = cr_engine::incoming_transaction::TransactionEngine::new(&paths);
                engine
                    .begin(&transaction)
                    .map_err(|error| error.to_string())?;
                if cr_engine::incoming_transaction::database_epoch() != captured_epoch {
                    engine.abort_prepared().map_err(|error| error.to_string())?;
                    return Err(
                        "The library changed during the folder conversion. Try again.".into(),
                    );
                }
                let committed_epoch = _guard
                    .commit_if_epoch(captured_epoch, &engine, &mut transaction)
                    .inspect_err(|error| {
                        if matches!(
                            error,
                            cr_engine::incoming_transaction::TransactionError::EpochChanged
                        ) {
                            let _ = engine.abort_prepared();
                        }
                    })
                    .map_err(|error| error.to_string())?;
                Ok((
                    worker_database,
                    worker_incoming,
                    incoming_config,
                    committed_epoch,
                ))
            })();
            let _ = tx.send(result);
        });
    if let Err(error) = spawn {
        cr_engine::incoming_transaction::end_operation();
        show_preferences_message(
            dialog,
            gtk4::MessageType::Error,
            "Preferences were not saved",
            &format!("The save worker could not start: {error}"),
        );
        return;
    }

    dialog.set_sensitive(false);
    let dialog = dialog.clone();
    let on_ok = Rc::new(RefCell::new(Some(on_ok)));
    glib::timeout_add_local(std::time::Duration::from_millis(25), move || {
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("The save worker stopped without a result.".into())
            }
        };
        cr_engine::incoming_transaction::end_operation();
        dialog.set_sensitive(true);
        match result {
            Ok((database, incoming, incoming_config, committed_epoch)) => {
                if cr_engine::incoming_transaction::database_epoch() != committed_epoch {
                    show_preferences_message(
                        &dialog,
                        gtk4::MessageType::Error,
                        "Preferences were saved",
                        "The live library changed after the save. Restart ComicRust to load the saved folder roles.",
                    );
                    return glib::ControlFlow::Break;
                }
                *library::settings().borrow_mut() = settings.clone();
                cr_core::settings::unified::set_plugin(library::INCOMING_PLUGIN, &incoming_config);
                cr_core::settings::unified::set_plugin(library::SCRAPER_PLUGIN, &scraper);
                library::session()
                    .borrow_mut()
                    .install_persisted_database(database.clone());
                library::replace_incoming_catalog(incoming);
                crate::gauges::invalidate();
                if let Some(callback) = on_ok.borrow_mut().take() {
                    callback();
                }
                dialog.close();
            }
            Err(error) => show_preferences_message(
                &dialog,
                gtk4::MessageType::Error,
                "Preferences were not saved",
                &error,
            ),
        }
        glib::ControlFlow::Break
    });
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

    // PORT ADDITION (no C# counterpart): the on-demand cover
    // generation switch — off = the grid loads cached covers only,
    // File ▸ Generate Cover Thumbnails backfills the cache.
    page.append(&section_label("Thumbnails"));
    let on_demand = CheckButton::with_label("Generate cover thumbnails on demand");
    on_demand.set_active(settings.borrow().generate_thumbnails_on_demand);
    {
        let settings = Rc::clone(settings);
        on_demand.connect_toggled(move |c| {
            settings.borrow_mut().generate_thumbnails_on_demand = c.is_active();
        });
    }
    page.append(&on_demand);

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

/// PORT ADDITION (no C# counterpart — ADR-044): the duplicate-cleanup
/// rules. Each row writes one `Settings.Duplicates*` field into the
/// working clone; the OK commit (the `show_preferences` response
/// handler) persists it.
fn build_duplicates_page(settings: &SettingsRef) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 6);
    page.set_margin_top(8);
    page.set_margin_bottom(8);
    page.set_margin_start(8);
    page.set_margin_end(8);

    page.append(&section_label(
        "Rules for the Select Worst Duplicates command (book context menu).",
    ));
    page.append(&section_label(
        "In every duplicate group a copy gets one penalty per rule it \
         loses; the command selects the copies with more penalties \
         than the group's best copy.",
    ));

    page.append(&section_label("Which copies count as worse"));
    let cbr = CheckButton::with_label("CBR copies are worse than CBZ copies");
    let smaller = CheckButton::with_label("Smaller files are worse than larger files");
    let fewer = CheckButton::with_label("Fewer pages are worse than more pages");
    let older = CheckButton::with_label("Older files are worse than newer files");
    cbr.set_active(settings.borrow().duplicates_cbr_worse_than_cbz);
    smaller.set_active(settings.borrow().duplicates_smaller_file_worse);
    fewer.set_active(settings.borrow().duplicates_fewer_pages_worse);
    older.set_active(settings.borrow().duplicates_older_file_worse);
    {
        let settings = Rc::clone(settings);
        cbr.connect_toggled(move |c| {
            settings.borrow_mut().duplicates_cbr_worse_than_cbz = c.is_active();
        });
    }
    {
        let settings = Rc::clone(settings);
        smaller.connect_toggled(move |c| {
            settings.borrow_mut().duplicates_smaller_file_worse = c.is_active();
        });
    }
    {
        let settings = Rc::clone(settings);
        fewer.connect_toggled(move |c| {
            settings.borrow_mut().duplicates_fewer_pages_worse = c.is_active();
        });
    }
    {
        let settings = Rc::clone(settings);
        older.connect_toggled(move |c| {
            settings.borrow_mut().duplicates_older_file_worse = c.is_active();
        });
    }
    page.append(&cbr);
    page.append(&smaller);
    page.append(&fewer);
    page.append(&older);

    page.append(&section_label(
        "The Views ▸ Show Duplicates filter shows the duplicate \
         groups of the current list.",
    ));
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
