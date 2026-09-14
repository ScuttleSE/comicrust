//! The Library Organizer configuration dialog — the port of
//! `configureform.py` + `configformcontrols.py`. One dialog edits the
//! working copy of the profiles; OK returns the whole
//! [`PluginSettings`] (profiles and last-used names), Cancel returns
//! None. The pages follow the addon's tabs: Overview (mode, copy,
//! fileless, base folder), Files (file template), Folders (folder
//! template, empty-folder text, excluded folders), Rules (the
//! exclude-rule tree with nested Any/All groups), Options (spacing,
//! multi-value, empty-folder pruning, failed-empty fields, read
//! percentage, months, illegal characters, empty-value
//! substitutions).
//!
//! Templates are typed by hand with a token picker that appends
//! tokens in the addon's grammar; the per-field Prefix/Postfix/
//! Separator/TextBox tables ride along in the profile and round-trip
//! through the XML import/export and the TOML store. Every edit
//! commits into the working copy immediately, so a page switch or a
//! profile switch never loses a value.

use gtk4::prelude::*;
use gtk4::{
    CheckButton, DropDown, Entry, Label, Notebook, Orientation, ScrolledWindow, StringList,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use cr_core::model::comic_book::ComicBook;
use cr_organize::fields;
use cr_organize::profile::{
    ExcludeRule, PluginSettings, Profile, RuleNode, EXCLUDE_ALL, EXCLUDE_ANY, MODE_COPY,
    MODE_DO_NOT, MODE_MOVE, MODE_ONLY, MODE_SIMULATE,
};
use cr_organize::series::SeriesIndex;
use cr_organize::template::{MultiValueState, TokenCtx};

/// The dialog owns the working copy and returns it on OK.
pub fn show_organize_config(
    parent: &impl IsA<gtk4::Window>,
    settings: &PluginSettings,
    sample: Option<&ComicBook>,
    on_done: impl Fn(Option<PluginSettings>) + 'static,
) {
    let dialog = gtk4::Dialog::builder()
        .title("Configure Library Organizer")
        .transient_for(parent)
        .modal(true)
        .default_width(780)
        .default_height(640)
        .build();
    DIALOG_PARENT.with(|d| {
        *d.borrow_mut() = Some(dialog.upcast_ref::<gtk4::Window>().clone());
    });

    DIALOG_PARENT.with(|d| *d.borrow_mut() = None);
    let state: Rc<RcState> = Rc::new(RcState {
        settings: RefCell::new(settings.clone()),
        selected: std::cell::Cell::new(0usize),
        sample: sample.cloned(),
    });

    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_spacing(6);

    // The profile row: selector + actions.
    let profile_bar = gtk4::Box::new(Orientation::Horizontal, 6);
    profile_bar.append(&Label::new(Some("Profile:")));
    let profile_drop = DropDown::new(Some(StringList::new(&[])), NO_EXPR);
    profile_bar.append(&profile_drop);
    let new_btn = gtk4::Button::with_label("New");
    let dup_btn = gtk4::Button::with_label("Duplicate");
    let rename_btn = gtk4::Button::with_label("Rename");
    let delete_btn = gtk4::Button::with_label("Delete");
    let import_btn = gtk4::Button::with_label("Import…");
    let export_btn = gtk4::Button::with_label("Export…");
    for w in [
        &new_btn,
        &dup_btn,
        &rename_btn,
        &delete_btn,
        &import_btn,
        &export_btn,
    ] {
        profile_bar.append(w);
    }
    content.append(&profile_bar);

    // The pages.
    let pages = Notebook::new();
    pages.set_vexpand(true);
    content.append(&pages);

    let mut handles = Handles::default();
    let overview = build_overview_page(&mut handles, &state);
    let files = build_template_page(&mut handles, &state, true);
    let folders = build_folders_page(&mut handles, &state);
    let rules = build_rules_page(&mut handles, &state);
    let options = build_options_page(&mut handles, &state);
    let widgets: Rc<Handles> = Rc::new(handles);
    WIDGETS.with(|slot| *slot.borrow_mut() = Some(std::rc::Rc::downgrade(&widgets)));
    pages.append_page(&overview, Some(&tab_label("Overview")));
    pages.append_page(&files, Some(&tab_label("Files")));
    pages.append_page(&folders, Some(&tab_label("Folders")));
    pages.append_page(&rules, Some(&tab_label("Rules")));
    pages.append_page(&options, Some(&tab_label("Options")));

    sync_profiles(&state, &profile_drop, Some(0));

    // Profile switching loads the selected profile.
    {
        let state = state.clone();
        let drop = profile_drop.clone();
        profile_drop.connect_selected_notify(move |_| {
            state.selected.set(drop.selected() as usize);
            load_profile_into_widgets(&state);
        });
    }

    // The profile actions.
    {
        let state = state.clone();
        let drop = profile_drop.clone();
        new_btn.connect_clicked(move |_| {
            let name = {
                let s = state.settings.borrow();
                unique_profile_name(&s, "New Profile")
            };
            state.settings.borrow_mut().profiles.push(Profile {
                name,
                ..Profile::builtin_default()
            });
            let last = state.settings.borrow().profiles.len() - 1;
            state.selected.set(last);
            sync_profiles(&state, &drop, Some(last));
            load_profile_into_widgets(&state);
        });
    }
    {
        let state = state.clone();
        let drop = profile_drop.clone();
        dup_btn.connect_clicked(move |_| {
            let idx = state.selected.get();
            let (clone, name) = {
                let s = state.settings.borrow();
                match s.profiles.get(idx) {
                    Some(p) => (
                        p.clone(),
                        unique_profile_name(&s, &format!("{} copy", p.name)),
                    ),
                    None => return,
                }
            };
            let mut clone = clone;
            clone.name = name;
            state.settings.borrow_mut().profiles.push(clone);
            let last = state.settings.borrow().profiles.len() - 1;
            state.selected.set(last);
            sync_profiles(&state, &drop, Some(last));
            load_profile_into_widgets(&state);
        });
    }
    {
        let state = state.clone();
        let drop = profile_drop.clone();
        let parent = dialog.clone();
        rename_btn.connect_clicked(move |_| {
            let idx = state.selected.get();
            let current = state
                .settings
                .borrow()
                .profiles
                .get(idx)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            let s = state.clone();
            let d = drop.clone();
            crate::dialogs::name_prompt::show_name_prompt(
                &parent,
                "Rename profile",
                &current,
                move |name| {
                    if let Some(p) = s.settings.borrow_mut().profiles.get_mut(idx) {
                        p.name = name;
                    }
                    sync_profiles(&s, &d, Some(idx));
                },
            );
        });
    }
    {
        let state = state.clone();
        let drop = profile_drop.clone();
        delete_btn.connect_clicked(move |_| {
            let idx = state.selected.get();
            {
                let mut s = state.settings.borrow_mut();
                if s.profiles.len() <= 1 {
                    return;
                }
                s.profiles.remove(idx);
            }
            let next = idx.saturating_sub(1);
            state.selected.set(next);
            sync_profiles(&state, &drop, Some(next));
            load_profile_into_widgets(&state);
        });
    }
    {
        let state = state.clone();
        let drop = profile_drop.clone();
        let parent = dialog.clone();
        import_btn.connect_clicked(move |_| {
            let s = state.clone();
            let d = drop.clone();
            let win = parent.clone();
            choose_file(&parent, "Import profiles", move |path| {
                let Ok(text) = std::fs::read_to_string(&path) else {
                    return;
                };
                match cr_organize::profile::import_profiles_xml(&text) {
                    Ok((mut imported, _)) => {
                        {
                            let mut store = s.settings.borrow_mut();
                            for p in imported.drain(..) {
                                let name = unique_profile_name(&store, &p.name);
                                let mut p = p;
                                p.name = name;
                                store.profiles.push(p);
                            }
                            let last = store.profiles.len() - 1;
                            s.selected.set(last);
                        }
                        sync_profiles(&s, &d, Some(s.selected.get()));
                        load_profile_into_widgets(&s);
                    }
                    Err(e) => {
                        show_error_dialog(&win, "Import failed", &e.to_string());
                    }
                }
            });
        });
    }
    {
        let state = state.clone();
        let parent = dialog.clone();
        export_btn.connect_clicked(move |_| {
            let idx = state.selected.get();
            let Some(p) = state.settings.borrow().profiles.get(idx).cloned() else {
                return;
            };
            choose_file_save(&parent, "Export profile", "profile.xml", move |path| {
                let xml = cr_organize::profile::export_single_profile_xml(&p);
                let _ = std::fs::write(&path, xml);
            });
        });
    }

    load_profile_into_widgets(&state);

    dialog.add_button("OK", gtk4::ResponseType::Ok);
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    {
        let state = state.clone();
        let done = std::rc::Rc::clone(&done);
        dialog.connect_response(move |dlg, response| {
            if done.replace(true) {
                return;
            }
            let result = if response == gtk4::ResponseType::Ok {
                collect_profile_from_widgets(&state);
                Some(state.settings.borrow().clone())
            } else {
                None
            };
            dlg.close();
            on_done(result);
        });
    }
    dialog.present();
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct RcState {
    settings: RefCell<PluginSettings>,
    selected: std::cell::Cell<usize>,
    sample: Option<ComicBook>,
}

fn commit_field(state: &RcState, f: impl FnOnce(&mut Profile)) {
    let idx = state.selected.get();
    let mut s = state.settings.borrow_mut();
    if let Some(p) = s.profiles.get_mut(idx) {
        f(p);
    }
}

fn unique_profile_name(store: &PluginSettings, want: &str) -> String {
    if !store.profiles.iter().any(|p| p.name == want) {
        return want.to_string();
    }
    for i in 1..1000 {
        let candidate = format!("{want} {i}");
        if !store.profiles.iter().any(|p| p.name == candidate) {
            return candidate;
        }
    }
    format!("{want} {}", store.profiles.len())
}

fn sync_profiles(state: &RcState, drop: &DropDown, select: Option<usize>) {
    let s = state.settings.borrow();
    let names: Vec<String> = s.profiles.iter().map(|p| p.name.clone()).collect();
    let list = StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
    drop.set_model(Some(&list));
    if let Some(idx) = select {
        drop.set_selected(idx as u32);
    }
}

/// The widget registry the load side reads. Every template entry and
/// list container the load needs is here.
#[derive(Default)]
struct Handles {
    mode_move: Option<CheckButton>,
    mode_copy: Option<CheckButton>,
    mode_simulate: Option<CheckButton>,
    copy_mode: Option<CheckButton>,
    use_file_name: Option<CheckButton>,
    use_folder: Option<CheckButton>,
    move_fileless: Option<CheckButton>,
    fileless_format: Option<DropDown>,
    base_folder: Option<Entry>,
    file_template: Option<Entry>,
    file_preview: Option<Label>,
    folder_template: Option<Entry>,
    folder_preview: Option<Label>,
    empty_folder: Option<Entry>,
    exclude_folders_list: Option<gtk4::Box>,
    rules_tree: Option<gtk4::Box>,
    exclude_mode: Option<DropDown>,
    exclude_operator: Option<DropDown>,
    replace_multiple_spaces: Option<CheckButton>,
    dont_ask_when_multi_one: Option<CheckButton>,
    remove_empty_folder: Option<CheckButton>,
    copy_read_percentage: Option<CheckButton>,
    fail_empty_values: Option<CheckButton>,
    move_failed: Option<CheckButton>,
    failed_folder: Option<Entry>,
    failed_fields_list: Option<gtk4::Box>,
    excluded_empty_list: Option<gtk4::Box>,
    months_box: Option<gtk4::Box>,
    illegal_box: Option<gtk4::Box>,
    empty_data_box: Option<gtk4::Box>,
}

thread_local! {
    static WIDGETS: RefCell<Option<std::rc::Weak<Handles>>> = const { RefCell::new(None) };
}

fn widgets() -> Option<std::rc::Rc<Handles>> {
    WIDGETS.with(|w| w.borrow().as_ref().and_then(std::rc::Weak::upgrade))
}

// ---------------------------------------------------------------------------
// Page builders
// ---------------------------------------------------------------------------

fn build_overview_page(w: &mut Handles, state: &Rc<RcState>) -> ScrolledWindow {
    let box_ = gtk4::Box::new(Orientation::Vertical, 6);
    box_.set_margin_top(8);
    box_.set_margin_bottom(8);
    box_.set_margin_start(8);
    box_.set_margin_end(8);

    let mode_group = gtk4::Frame::new(Some("Mode"));
    let mode_box = gtk4::Box::new(Orientation::Vertical, 4);
    let mode_move = CheckButton::with_label("Move");
    let mode_copy = CheckButton::with_label("Copy");
    let mode_simulate = CheckButton::with_label("Simulate (no files change)");
    mode_copy.set_group(Some(&mode_move));
    mode_simulate.set_group(Some(&mode_move));
    mode_box.append(&mode_move);
    mode_box.append(&mode_copy);
    mode_box.append(&mode_simulate);
    mode_group.set_child(Some(&mode_box));
    box_.append(&mode_group);

    let copy_mode = CheckButton::with_label("Copy mode also adds the copy to the library");
    box_.append(&copy_mode);
    let use_file_name = CheckButton::with_label("Organize file names");
    box_.append(&use_file_name);
    let use_folder = CheckButton::with_label("Organize folders");
    box_.append(&use_folder);

    let fileless_group = gtk4::Frame::new(Some("Fileless books"));
    let fileless_box = gtk4::Box::new(Orientation::Horizontal, 6);
    let move_fileless = CheckButton::with_label("Export the cover image for fileless books");
    let fileless_format = DropDown::new(Some(StringList::new(&[".jpg", ".png", ".bmp"])), NO_EXPR);
    fileless_box.append(&move_fileless);
    fileless_box.append(&fileless_format);
    fileless_group.set_child(Some(&fileless_box));
    box_.append(&fileless_group);

    let base_group = gtk4::Frame::new(Some("Base folder"));
    let base_box = gtk4::Box::new(Orientation::Horizontal, 6);
    let base_folder = Entry::new();
    base_folder.set_hexpand(true);
    let browse = gtk4::Button::with_label("Browse…");
    base_box.append(&base_folder);
    base_box.append(&browse);
    base_group.set_child(Some(&base_box));
    box_.append(&base_group);

    // The edits commit straight into the working copy.
    {
        let state = state.clone();
        mode_move.connect_toggled(move |b| {
            if b.is_active() {
                commit_field(&state, |p| p.mode = MODE_MOVE.to_string());
            }
        });
    }
    {
        let state = state.clone();
        mode_copy.connect_toggled(move |b| {
            if b.is_active() {
                commit_field(&state, |p| p.mode = MODE_COPY.to_string());
            }
        });
    }
    {
        let state = state.clone();
        mode_simulate.connect_toggled(move |b| {
            if b.is_active() {
                commit_field(&state, |p| p.mode = MODE_SIMULATE.to_string());
            }
        });
    }
    {
        let state = state.clone();
        copy_mode.connect_toggled(move |b| {
            let active = b.is_active();
            commit_field(&state, |p| p.copy_mode = active);
        });
    }
    {
        let state = state.clone();
        use_file_name.connect_toggled(move |b| {
            let active = b.is_active();
            commit_field(&state, |p| p.use_file_name = active);
        });
    }
    {
        let state = state.clone();
        use_folder.connect_toggled(move |b| {
            let active = b.is_active();
            commit_field(&state, |p| p.use_folder = active);
        });
    }
    {
        let state = state.clone();
        move_fileless.connect_toggled(move |b| {
            let active = b.is_active();
            commit_field(&state, |p| p.move_fileless = active);
        });
    }
    {
        let state = state.clone();
        let ff = fileless_format.clone();
        fileless_format.connect_selected_notify(move |_| {
            let formats = [".jpg", ".png", ".bmp"];
            if let Some(f) = formats.get(ff.selected() as usize) {
                commit_field(&state, |p| p.fileless_format = f.to_string());
            }
        });
    }
    {
        let state = state.clone();
        base_folder.connect_changed(move |e| {
            let text = e.text().to_string();
            commit_field(&state, |p| p.base_folder = text);
        });
    }
    {
        let state = state.clone();
        let entry = base_folder.clone();
        browse.connect_clicked(move |_| {
            let Some(parent_window) = DIALOG_PARENT.with(|d| d.borrow().clone()) else {
                return;
            };
            let s = state.clone();
            let entry = entry.clone();
            choose_folder(&parent_window, "Choose the base folder", move |path| {
                entry.set_text(&path.to_string_lossy());
                commit_field(&s, |p| p.base_folder = path.to_string_lossy().into_owned());
            });
        });
    }

    w.mode_move = Some(mode_move);
    w.mode_copy = Some(mode_copy);
    w.mode_simulate = Some(mode_simulate);
    w.copy_mode = Some(copy_mode);
    w.use_file_name = Some(use_file_name);
    w.use_folder = Some(use_folder);
    w.move_fileless = Some(move_fileless);
    w.fileless_format = Some(fileless_format);
    w.base_folder = Some(base_folder);
    scroll_of(box_)
}

fn build_template_page(w: &mut Handles, state: &Rc<RcState>, file: bool) -> ScrolledWindow {
    let box_ = gtk4::Box::new(Orientation::Vertical, 6);
    box_.set_margin_top(8);
    box_.set_margin_bottom(8);
    box_.set_margin_start(8);
    box_.set_margin_end(8);

    let title = if file {
        "File template"
    } else {
        "Folder template"
    };
    let group = gtk4::Frame::new(Some(title));
    let inner = gtk4::Box::new(Orientation::Vertical, 6);
    let entry = Entry::new();
    inner.append(&entry);
    let preview = Label::new(None);
    preview.set_halign(gtk4::Align::Start);
    preview.set_xalign(0.0);
    preview.set_selectable(true);
    inner.append(&preview);

    // The token picker: a drop-down of template names + Insert.
    let picker_box = gtk4::Box::new(Orientation::Horizontal, 6);
    let tokens: Vec<String> = fields::INSERT_CONTROLS
        .iter()
        .map(|(_, token, _)| token.to_string())
        .collect();
    let picker = DropDown::new(
        Some(StringList::new(
            &tokens.iter().map(String::as_str).collect::<Vec<_>>(),
        )),
        NO_EXPR,
    );
    let insert = gtk4::Button::with_label("Insert token");
    picker_box.append(&picker);
    picker_box.append(&insert);
    inner.append(&picker_box);
    group.set_child(Some(&inner));
    box_.append(&group);

    let hint = Label::new(Some(
        "Tokens: {prefix<name[args]>postfix}. Digits pad (number2). (arg) lists.\n\
         !name = insert when empty; ?name(arg) = insert when the value matches.",
    ));
    hint.set_halign(gtk4::Align::Start);
    hint.set_xalign(0.0);
    box_.append(&hint);

    {
        let state = state.clone();
        let preview = preview.clone();
        entry.connect_changed(move |e| {
            let text = e.text().to_string();
            commit_field(&state, |p| {
                if file {
                    p.file_template = text.clone();
                } else {
                    p.folder_template = text.clone();
                }
            });
            if let Some(sample) = &state.sample {
                update_preview_for(&state, sample, file, &preview);
            }
        });
    }
    {
        let entry = entry.clone();
        insert.connect_clicked(move |_| {
            let idx = picker.selected() as usize;
            let Some((_, token, _)) = fields::INSERT_CONTROLS.get(idx) else {
                return;
            };
            let token = format!("{{<{token}>}}");
            let mut text = entry.text().to_string();
            let pos = (entry.position().max(0)) as usize;
            let pos = pos.min(text.chars().count());
            let char_idx = text
                .char_indices()
                .nth(pos)
                .map(|(i, _)| i)
                .unwrap_or(text.len());
            text.insert_str(char_idx, &token);
            entry.set_text(&text);
            entry.set_position((pos + token.chars().count()) as i32);
        });
    }

    if file {
        w.file_template = Some(entry);
        w.file_preview = Some(preview);
    } else {
        w.folder_template = Some(entry);
        w.folder_preview = Some(preview);
    }
    scroll_of(box_)
}

fn tab_label(text: &str) -> gtk4::Label {
    Label::new(Some(text))
}

fn scroll_of(box_: gtk4::Box) -> ScrolledWindow {
    ScrolledWindow::builder()
        .child(&box_)
        .vexpand(true)
        .hexpand(true)
        .build()
}

/// Resolves the file/folder template against the sample book (the
/// addon's preview pane).
fn update_preview_for(state: &RcState, sample: &ComicBook, file: bool, label: &Label) {
    let idx = state.selected.get();
    let s = state.settings.borrow();
    let Some(p) = s.profiles.get(idx) else {
        return;
    };
    let mut series = SeriesIndex::new(std::slice::from_ref(sample));
    let mut failed_fields = Vec::new();
    let mut failed = false;
    let mut counter = None;
    let mut multi = MultiValueState::default();
    let mut asker = NoAsk;
    let mut ctx = TokenCtx::new(
        sample,
        0,
        p,
        &mut series,
        &mut failed_fields,
        &mut failed,
        &mut counter,
        &mut multi,
        &mut asker,
    );
    let text = if file {
        ctx.make_file_name(&p.file_template)
    } else {
        ctx.make_folder_path(&p.folder_template)
    };
    label.set_text(&format!("Preview: {text}"));
}

struct NoAsk;
impl cr_organize::template::MultiValueAsker for NoAsk {
    fn ask_multi_value(
        &mut self,
        _ask: cr_organize::template::MultiValueAsk,
    ) -> cr_organize::template::MultiValueAnswer {
        Default::default()
    }
}

fn build_folders_page(w: &mut Handles, state: &Rc<RcState>) -> ScrolledWindow {
    let box_ = gtk4::Box::new(Orientation::Vertical, 6);
    box_.set_margin_top(8);
    box_.set_margin_bottom(8);
    box_.set_margin_start(8);
    box_.set_margin_end(8);

    let empty_line = gtk4::Box::new(Orientation::Horizontal, 6);
    empty_line.append(&{
        let l = Label::new(Some("Text for empty folder parts"));
        l.set_width_chars(24);
        l.set_xalign(0.0);
        l
    });
    let empty_folder = Entry::new();
    empty_folder.set_hexpand(true);
    empty_line.append(&empty_folder);
    box_.append(&empty_line);
    {
        let state = state.clone();
        empty_folder.connect_changed(move |e| {
            let text = e.text().to_string();
            commit_field(&state, |p| p.empty_folder = text);
        });
    }
    w.empty_folder = Some(empty_folder);

    let excluded_group = gtk4::Frame::new(Some("Excluded folders (a book inside one is skipped)"));
    let excluded_box = gtk4::Box::new(Orientation::Vertical, 4);
    let add_line = gtk4::Box::new(Orientation::Horizontal, 6);
    let add_entry = Entry::new();
    add_entry.set_hexpand(true);
    let add_btn = gtk4::Button::with_label("Add");
    add_line.append(&add_entry);
    add_line.append(&add_btn);
    excluded_box.append(&add_line);
    let list = gtk4::Box::new(Orientation::Vertical, 0);
    excluded_box.append(&list);
    excluded_group.set_child(Some(&excluded_box));
    box_.append(&excluded_group);
    w.exclude_folders_list = Some(list.clone());

    {
        let state = state.clone();
        add_entry_clone(add_entry, add_btn, state);
    }

    scroll_of(box_)
}

fn add_entry_clone(add_entry: Entry, add_btn: gtk4::Button, state: Rc<RcState>) {
    add_btn.connect_clicked(move |_| {
        let path = add_entry.text().to_string();
        if path.is_empty() {
            return;
        }
        add_entry.set_text("");
        commit_field(&state, |p| p.exclude_folders.push(path));
        if let Some(list) = state_exclude_list() {
            rebuild_exclude_folders(&state, &list);
        }
    });
}

fn state_exclude_list() -> Option<gtk4::Box> {
    widgets().and_then(|w| w.exclude_folders_list.clone())
}

fn build_rules_page(w: &mut Handles, state: &Rc<RcState>) -> ScrolledWindow {
    let box_ = gtk4::Box::new(Orientation::Vertical, 6);
    box_.set_margin_top(8);
    box_.set_margin_bottom(8);
    box_.set_margin_start(8);
    box_.set_margin_end(8);

    let mode_line = gtk4::Box::new(Orientation::Horizontal, 6);
    mode_line.append(&Label::new(Some("Books matching the rules are:")));
    let exclude_mode = DropDown::new(Some(StringList::new(&[MODE_DO_NOT, MODE_ONLY])), NO_EXPR);
    mode_line.append(&exclude_mode);
    box_.append(&mode_line);

    let op_line = gtk4::Box::new(Orientation::Horizontal, 6);
    op_line.append(&Label::new(Some("Match")));
    let exclude_operator =
        DropDown::new(Some(StringList::new(&[EXCLUDE_ANY, EXCLUDE_ALL])), NO_EXPR);
    op_line.append(&exclude_operator);
    op_line.append(&Label::new(Some("of the following rules")));
    box_.append(&op_line);

    let controls = gtk4::Box::new(Orientation::Horizontal, 6);
    let add_rule = gtk4::Button::with_label("Add Rule");
    let add_group = gtk4::Button::with_label("Add Group");
    controls.append(&add_rule);
    controls.append(&add_group);
    box_.append(&controls);

    let tree = ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .height_request(280)
        .build();
    let rules_box = gtk4::Box::new(Orientation::Vertical, 4);
    tree.set_child(Some(&rules_box));
    box_.append(&tree);

    {
        let state = state.clone();
        let tree_box = rules_box.clone();
        add_rule.connect_clicked(move |_| {
            commit_field(&state, |p| {
                p.exclude_rules.push(RuleNode::Rule(ExcludeRule {
                    field: "Series".into(),
                    operator: "is".into(),
                    value: String::new(),
                }));
            });
            rebuild_rules(&state, &tree_box);
        });
    }
    {
        let state = state.clone();
        let tree_box = rules_box.clone();
        add_group.connect_clicked(move |_| {
            commit_field(&state, |p| {
                p.exclude_rules.push(RuleNode::Group {
                    operator: EXCLUDE_ANY.to_string(),
                    rules: Vec::new(),
                });
            });
            rebuild_rules(&state, &tree_box);
        });
    }
    {
        let state = state.clone();
        let d = exclude_mode.clone();
        exclude_mode.connect_selected_notify(move |_| {
            let value = if d.selected() == 1 {
                MODE_ONLY
            } else {
                MODE_DO_NOT
            };
            commit_field(&state, |p| p.exclude_mode = value.to_string());
        });
    }
    {
        let state = state.clone();
        let d = exclude_operator.clone();
        exclude_operator.connect_selected_notify(move |_| {
            let value = if d.selected() == 1 {
                EXCLUDE_ALL
            } else {
                EXCLUDE_ANY
            };
            commit_field(&state, |p| p.exclude_operator = value.to_string());
        });
    }

    w.exclude_mode = Some(exclude_mode);
    w.exclude_operator = Some(exclude_operator);
    w.rules_tree = Some(rules_box);
    scroll_of(box_)
}

/// The rule-tree renderer. Rows commit their edits straight into the
/// model by PATH (index chain through groups); structural changes
/// rebuild the tree.
fn rebuild_rules(state: &Rc<RcState>, box_: &gtk4::Box) {
    while let Some(child) = box_.first_child() {
        box_.remove(&child);
    }
    let idx = state.selected.get();
    let s = state.settings.borrow();
    let Some(p) = s.profiles.get(idx) else {
        return;
    };
    for (i, node) in p.exclude_rules.iter().enumerate() {
        render_rule_node(state, box_, node, vec![i], 0);
    }
}

fn render_rule_node(
    state: &Rc<RcState>,
    box_: &gtk4::Box,
    node: &RuleNode,
    path: Vec<usize>,
    depth: usize,
) {
    match node {
        RuleNode::Rule(rule) => {
            let state = state.clone();
            let line = gtk4::Box::new(Orientation::Horizontal, 6);
            line.set_margin_start((depth * 24) as i32);

            let field_list = StringList::new(fields::RULE_FIELD_CATALOG);
            let field_drop = DropDown::new(Some(field_list), NO_EXPR);
            if let Some(pos) = fields::RULE_FIELD_CATALOG
                .iter()
                .position(|f| *f == rule.field)
            {
                field_drop.set_selected(pos as u32);
            }

            let operators = [
                "contains",
                "does not contain",
                "greater than",
                "less than",
                "is",
                "is not",
            ];
            let operator_drop = DropDown::new(Some(StringList::new(&operators)), NO_EXPR);
            if let Some(pos) = operators.iter().position(|o| *o == rule.operator) {
                operator_drop.set_selected(pos as u32);
            }

            // Yes/No fields take a combobox value; everything else an
            // entry.
            let value_entry = Entry::new();
            value_entry.set_text(&rule.value);
            value_entry.set_hexpand(true);
            let yes_values: Vec<&str> = fields::yes_no_values(&rule.field).unwrap_or(&[]).to_vec();
            let value_drop = DropDown::new(Some(StringList::new(&yes_values)), NO_EXPR);
            let is_yes = fields::yes_no_values(&rule.field).is_some();
            value_drop.set_visible(is_yes);
            value_entry.set_visible(!is_yes);
            if is_yes {
                if let Some(pos) = yes_values.iter().position(|v| *v == rule.value) {
                    value_drop.set_selected(pos as u32);
                }
            }

            let del = gtk4::Button::with_label("-");

            line.append(&field_drop);
            line.append(&operator_drop);
            line.append(&value_entry);
            line.append(&value_drop);
            line.append(&del);
            box_.append(&line);

            // Edits write back at the path.
            {
                let state = state.clone();
                let path = path.clone();
                let d = field_drop.clone();
                field_drop.connect_selected_notify(move |_| {
                    let pos = d.selected() as usize;
                    if let Some(display) = fields::RULE_FIELD_CATALOG.get(pos) {
                        commit_path(&state, &path, &mut |rule: &mut ExcludeRule| {
                            rule.field = display.to_string();
                        });
                    }
                });
            }
            {
                let state = state.clone();
                let path = path.clone();
                let od = operator_drop.clone();
                operator_drop.connect_selected_notify(move |_| {
                    let pos = od.selected() as usize;
                    if let Some(op) = [
                        "contains",
                        "does not contain",
                        "greater than",
                        "less than",
                        "is",
                        "is not",
                    ]
                    .get(pos)
                    {
                        commit_path(&state, &path, &mut |rule: &mut ExcludeRule| {
                            rule.operator = op.to_string();
                        });
                    }
                });
            }
            {
                let state = state.clone();
                let path = path.clone();
                value_entry.connect_changed(move |e| {
                    let text = e.text().to_string();
                    commit_path(&state, &path, &mut |rule: &mut ExcludeRule| {
                        rule.value = text.clone();
                    });
                });
            }
            {
                let state = state.clone();
                let path = path.clone();
                let vd = value_drop.clone();
                value_drop.connect_selected_notify(move |_| {
                    let pos = vd.selected() as usize;
                    let Some(values) = yes_values_at(&path, &state) else {
                        return;
                    };
                    if let Some(v) = values.get(pos) {
                        commit_path(&state, &path, &mut |rule: &mut ExcludeRule| {
                            rule.value = v.to_string();
                        });
                    }
                });
            }
            {
                let state = state.clone();
                let tree = box_.clone();
                let path = path.clone();
                del.connect_clicked(move |_| {
                    remove_at_path(&state, &path);
                    rebuild_rules(&state, &tree);
                });
            }
        }
        RuleNode::Group { operator, rules } => {
            let state = state.clone();
            let group_line = gtk4::Box::new(Orientation::Horizontal, 6);
            group_line.set_margin_start((depth * 24) as i32);
            group_line.append(&Label::new(Some("Match")));
            let op_drop =
                DropDown::new(Some(StringList::new(&[EXCLUDE_ANY, EXCLUDE_ALL])), NO_EXPR);
            if operator == EXCLUDE_ALL {
                op_drop.set_selected(1);
            }
            group_line.append(&op_drop);
            group_line.append(&Label::new(Some("of the following rules")));
            box_.append(&group_line);
            let inner = gtk4::Box::new(Orientation::Vertical, 4);
            inner.set_margin_start(24);
            box_.append(&inner);
            for (i, child) in rules.iter().enumerate() {
                let mut path = path.clone();
                path.push(i);
                render_rule_node(&state, &inner, child, path, depth + 1);
            }
            {
                let state = state.clone();
                let d = op_drop.clone();
                op_drop.connect_selected_notify(move |_| {
                    let value = if d.selected() == 1 {
                        EXCLUDE_ALL
                    } else {
                        EXCLUDE_ANY
                    };
                    commit_group_op(&state, &path, value);
                });
            }
        }
    }
}

fn yes_values_at(path: &[usize], state: &Rc<RcState>) -> Option<Vec<&'static str>> {
    let s = state.settings.borrow();
    let p = s.profiles.get(state.selected.get())?;
    let node = node_at(p, path)?;
    match node {
        RuleNode::Rule(r) => fields::yes_no_values(&r.field).map(|v| v.to_vec()),
        _ => None,
    }
}

fn node_at<'a>(p: &'a Profile, path: &[usize]) -> Option<&'a RuleNode> {
    let (first, rest) = path.split_first()?;
    let mut node = p.exclude_rules.get(*first)?;
    for i in rest {
        match node {
            RuleNode::Group { rules, .. } => node = rules.get(*i)?,
            _ => return None,
        }
    }
    Some(node)
}

fn commit_path(state: &RcState, path: &[usize], f: &mut dyn FnMut(&mut ExcludeRule)) {
    let idx = state.selected.get();
    let mut s = state.settings.borrow_mut();
    let Some(p) = s.profiles.get_mut(idx) else {
        return;
    };
    let Some(node) = node_at_mut(p, path) else {
        return;
    };
    if let RuleNode::Rule(rule) = node {
        f(rule);
    }
}

fn node_at_mut<'a>(p: &'a mut Profile, path: &[usize]) -> Option<&'a mut RuleNode> {
    let (first, rest) = path.split_first()?;
    let mut node = p.exclude_rules.get_mut(*first)?;
    for i in rest {
        match node {
            RuleNode::Group { rules, .. } => node = rules.get_mut(*i)?,
            _ => return None,
        }
    }
    Some(node)
}

fn commit_group_op(state: &RcState, path: &[usize], value: &str) {
    let idx = state.selected.get();
    let mut s = state.settings.borrow_mut();
    let Some(p) = s.profiles.get_mut(idx) else {
        return;
    };
    if let Some(RuleNode::Group { operator, .. }) = node_at_mut(p, path) {
        *operator = value.to_string();
    }
}

fn remove_at_path(state: &RcState, path: &[usize]) {
    let idx = state.selected.get();
    let mut s = state.settings.borrow_mut();
    let Some(p) = s.profiles.get_mut(idx) else {
        return;
    };
    let (last, parents) = match path.split_last() {
        Some((last, parents)) => (last, parents),
        None => return,
    };
    let container = if parents.is_empty() {
        &mut p.exclude_rules
    } else {
        match node_at_mut(p, parents) {
            Some(RuleNode::Group { rules, .. }) => rules,
            _ => return,
        }
    };
    container.remove(*last);
}

fn build_options_page(w: &mut Handles, state: &Rc<RcState>) -> ScrolledWindow {
    let box_ = gtk4::Box::new(Orientation::Vertical, 6);
    box_.set_margin_top(8);
    box_.set_margin_bottom(8);
    box_.set_margin_start(8);
    box_.set_margin_end(8);

    let rmf = CheckButton::with_label("Collapse multiple spaces");
    let dma = CheckButton::with_label("Do not ask when a multi-value field has one value");
    let ref_ = CheckButton::with_label("Remove empty folders after a move");
    let crp = CheckButton::with_label("Copy the read percentage when overwriting a library book");
    for c in [&rmf, &dma, &ref_, &crp] {
        box_.append(c);
    }
    {
        let state = state.clone();
        rmf.connect_toggled(move |b| {
            commit_field(&state, |p| p.replace_multiple_spaces = b.is_active());
        });
    }
    {
        let state = state.clone();
        dma.connect_toggled(move |b| {
            commit_field(&state, |p| p.dont_ask_when_multi_one = b.is_active());
        });
    }
    {
        let state = state.clone();
        ref_.connect_toggled(move |b| {
            commit_field(&state, |p| p.remove_empty_folder = b.is_active());
        });
    }
    {
        let state = state.clone();
        crp.connect_toggled(move |b| {
            commit_field(&state, |p| p.copy_read_percentage = b.is_active());
        });
    }
    w.replace_multiple_spaces = Some(rmf);
    w.dont_ask_when_multi_one = Some(dma);
    w.remove_empty_folder = Some(ref_);
    w.copy_read_percentage = Some(crp);

    // Failed-empty handling.
    let fail_group = gtk4::Frame::new(Some("Empty watched fields"));
    let fail_box = gtk4::Box::new(Orientation::Vertical, 4);
    let fev = CheckButton::with_label("Fail when a watched field is empty");
    let mfd = CheckButton::with_label("Move failed books to the failed folder");
    let failed_line = gtk4::Box::new(Orientation::Horizontal, 6);
    failed_line.append(&Label::new(Some("Failed folder")));
    let failed_folder = Entry::new();
    failed_folder.set_hexpand(true);
    failed_line.append(&failed_folder);
    let failed_label = Label::new(Some("Watched fields (stored under the C# names):"));
    failed_label.set_halign(gtk4::Align::Start);
    failed_label.set_xalign(0.0);
    let failed_list = gtk4::Box::new(Orientation::Vertical, 0);
    fail_box.append(&fev);
    fail_box.append(&mfd);
    fail_box.append(&failed_line);
    fail_box.append(&failed_label);
    fail_box.append(&failed_list);
    fail_group.set_child(Some(&fail_box));
    box_.append(&fail_group);
    {
        let state = state.clone();
        fev.connect_toggled(move |b| {
            commit_field(&state, |p| p.fail_empty_values = b.is_active());
        });
    }
    {
        let state = state.clone();
        mfd.connect_toggled(move |b| {
            commit_field(&state, |p| p.move_failed = b.is_active());
        });
    }
    {
        let state = state.clone();
        failed_folder.connect_changed(move |e| {
            let text = e.text().to_string();
            commit_field(&state, |p| p.failed_folder = text);
        });
    }
    w.fail_empty_values = Some(fev);
    w.move_failed = Some(mfd);
    w.failed_folder = Some(failed_folder);
    w.failed_fields_list = Some(failed_list.clone());

    // Months editor.
    let months_group = gtk4::Frame::new(Some(
        "Month names (1-12, 13 Spring, 14 Summer, 15 Fall, 16 Winter)",
    ));
    let months_box = gtk4::Box::new(Orientation::Vertical, 4);
    months_group.set_child(Some(&months_box));
    box_.append(&months_group);
    w.months_box = Some(months_box);

    // Illegal characters editor.
    let illegal_group = gtk4::Frame::new(Some("Illegal characters"));
    let illegal_box = gtk4::Box::new(Orientation::Vertical, 4);
    illegal_group.set_child(Some(&illegal_box));
    box_.append(&illegal_group);
    w.illegal_box = Some(illegal_box);

    // Empty-value substitutions.
    let empty_group = gtk4::Frame::new(Some("Empty-value substitutions (per field)"));
    let empty_box = gtk4::Box::new(Orientation::Vertical, 4);
    empty_group.set_child(Some(&empty_box));
    box_.append(&empty_group);
    w.empty_data_box = Some(empty_box);

    // Empty-folder exceptions.
    let exc_group = gtk4::Frame::new(Some("Folders never pruned"));
    let exc_box = gtk4::Box::new(Orientation::Vertical, 4);
    let exc_line = gtk4::Box::new(Orientation::Horizontal, 6);
    let exc_entry = Entry::new();
    exc_entry.set_hexpand(true);
    let exc_btn = gtk4::Button::with_label("Add");
    exc_line.append(&exc_entry);
    exc_line.append(&exc_btn);
    let exc_list = gtk4::Box::new(Orientation::Vertical, 0);
    exc_box.append(&exc_line);
    exc_box.append(&exc_list);
    exc_group.set_child(Some(&exc_box));
    box_.append(&exc_group);
    {
        let state = state.clone();
        let exc_list = exc_list.clone();
        exc_btn.connect_clicked(move |_| {
            let path = exc_entry.text().to_string();
            if path.is_empty() {
                return;
            }
            exc_entry.set_text("");
            commit_field(&state, |p| p.excluded_empty_folder.push(path));
            rebuild_excluded_empty(&state, &exc_list);
        });
    }
    w.excluded_empty_list = Some(exc_list);

    scroll_of(box_)
}

fn rebuild_excluded_empty(state: &Rc<RcState>, list_box: &gtk4::Box) {
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }
    let idx = state.selected.get();
    let s = state.settings.borrow();
    let Some(p) = s.profiles.get(idx) else {
        return;
    };
    for (i, path) in p.excluded_empty_folder.iter().enumerate() {
        let line = gtk4::Box::new(Orientation::Horizontal, 6);
        let l = Label::new(Some(path));
        l.set_hexpand(true);
        l.set_xalign(0.0);
        let del = gtk4::Button::with_label("Remove");
        line.append(&l);
        line.append(&del);
        list_box.append(&line);
        let state = state.clone();
        let list_box = list_box.clone();
        del.connect_clicked(move |_| {
            commit_field(&state, |p| {
                p.excluded_empty_folder.remove(i);
            });
            rebuild_excluded_empty(&state, &list_box);
        });
    }
}

fn rebuild_failed_fields(state: &Rc<RcState>, list_box: &gtk4::Box) {
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }
    let idx = state.selected.get();
    let watched: Vec<String> = {
        let s = state.settings.borrow();
        match s.profiles.get(idx) {
            Some(p) => p.failed_fields.clone(),
            None => Vec::new(),
        }
    };
    for display in fields::RULE_FIELD_CATALOG {
        let Some(field) = fields::rule_field(display) else {
            continue;
        };
        let check = CheckButton::with_label(display);
        check.set_active(watched.iter().any(|f| f == field));
        list_box.append(&check);
        let state = state.clone();
        let display = display.to_string();
        check.connect_toggled(move |b| {
            let active = b.is_active();
            commit_field(&state, |p| {
                if let Some(field) = fields::rule_field(&display) {
                    if active {
                        if !p.failed_fields.iter().any(|f| f == field) {
                            p.failed_fields.push(field.to_string());
                        }
                    } else if let Some(pos) = p.failed_fields.iter().position(|f| f == field) {
                        p.failed_fields.remove(pos);
                    }
                }
            });
        });
    }
}

fn rebuild_months(state: &Rc<RcState>, box_: &gtk4::Box) {
    while let Some(child) = box_.first_child() {
        box_.remove(&child);
    }
    let idx = state.selected.get();
    let current: BTreeMap<String, String> = {
        let s = state.settings.borrow();
        match s.profiles.get(idx) {
            Some(p) => p.months.clone(),
            None => Profile::default().months,
        }
    };
    let numbers: Vec<String> = (1..=16).map(|n| n.to_string()).collect();
    let drop = DropDown::new(
        Some(StringList::new(
            &numbers.iter().map(String::as_str).collect::<Vec<_>>(),
        )),
        NO_EXPR,
    );
    let entry = Entry::new();
    entry.set_text(current.get("1").map(String::as_str).unwrap_or(""));
    let set_btn = gtk4::Button::with_label("Set name");
    let line = gtk4::Box::new(Orientation::Horizontal, 6);
    line.append(&drop);
    line.append(&entry);
    line.append(&set_btn);
    box_.append(&line);
    {
        let current = current.clone();
        let d = drop.clone();
        let e2 = entry.clone();
        drop.connect_selected_notify(move |_| {
            let num = (d.selected() as i32 + 1).to_string();
            e2.set_text(current.get(&num).map(String::as_str).unwrap_or(""));
        });
    }
    {
        let state = state.clone();
        let d = drop.clone();
        let e2 = entry.clone();
        set_btn.connect_clicked(move |_| {
            let num = (d.selected() as i32 + 1).to_string();
            let name = e2.text().to_string();
            commit_field(&state, |p| {
                p.months.insert(num, name);
            });
        });
    }
}

fn rebuild_illegal(state: &Rc<RcState>, box_: &gtk4::Box) {
    while let Some(child) = box_.first_child() {
        box_.remove(&child);
    }
    let idx = state.selected.get();
    let current: BTreeMap<String, String> = {
        let s = state.settings.borrow();
        match s.profiles.get(idx) {
            Some(p) => p.illegal_characters.clone(),
            None => Profile::default().illegal_characters,
        }
    };
    let add_line = gtk4::Box::new(Orientation::Horizontal, 6);
    let char_entry = Entry::new();
    char_entry.set_width_chars(4);
    char_entry.set_placeholder_text(Some("char"));
    let repl_entry = Entry::new();
    repl_entry.set_hexpand(true);
    repl_entry.set_placeholder_text(Some("replacement"));
    let add_btn = gtk4::Button::with_label("Add");
    add_line.append(&char_entry);
    add_line.append(&repl_entry);
    add_line.append(&add_btn);
    box_.append(&add_line);
    for (ch, repl) in current.iter() {
        let line = gtk4::Box::new(Orientation::Horizontal, 6);
        let l = Label::new(Some(&format!("{ch:?} → {repl:?}")));
        l.set_hexpand(true);
        l.set_xalign(0.0);
        let del = gtk4::Button::with_label("Remove");
        line.append(&l);
        line.append(&del);
        box_.append(&line);
        let state = state.clone();
        let box2 = box_.clone();
        let ch = ch.clone();
        del.connect_clicked(move |_| {
            commit_field(&state, |p| {
                p.illegal_characters.remove(&ch);
            });
            rebuild_illegal(&state, &box2);
        });
    }
    {
        let state = state.clone();
        let box2 = box_.clone();
        add_btn.connect_clicked(move |_| {
            let ch = char_entry.text().to_string();
            let repl = repl_entry.text().to_string();
            if ch.is_empty() {
                return;
            }
            char_entry.set_text("");
            repl_entry.set_text("");
            commit_field(&state, |p| {
                p.illegal_characters.insert(ch, repl);
            });
            rebuild_illegal(&state, &box2);
        });
    }
}

fn rebuild_empty_data(state: &Rc<RcState>, box_: &gtk4::Box) {
    while let Some(child) = box_.first_child() {
        box_.remove(&child);
    }
    let idx = state.selected.get();
    let current: BTreeMap<String, String> = {
        let s = state.settings.borrow();
        match s.profiles.get(idx) {
            Some(p) => p.empty_data.clone(),
            None => BTreeMap::new(),
        }
    };
    let catalog: Vec<&str> = fields::RULE_FIELD_CATALOG.to_vec();
    let drop = DropDown::new(Some(StringList::new(&catalog)), NO_EXPR);
    let entry = Entry::new();
    entry.set_hexpand(true);
    let set_btn = gtk4::Button::with_label("Set");
    let line = gtk4::Box::new(Orientation::Horizontal, 6);
    line.append(&drop);
    line.append(&entry);
    line.append(&set_btn);
    box_.append(&line);
    {
        let state = state.clone();
        let box2 = box_.clone();
        set_btn.connect_clicked(move |_| {
            let pos = drop.selected() as usize;
            let Some(display) = catalog.get(pos) else {
                return;
            };
            let Some(field) = fields::rule_field(display) else {
                return;
            };
            let value = entry.text().to_string();
            commit_field(&state, |p| {
                p.empty_data.insert(field.to_string(), value);
            });
            rebuild_empty_data(&state, &box2);
        });
    }
    for (field, value) in current.iter() {
        let display = fields::display_name_of(field);
        let line = gtk4::Box::new(Orientation::Horizontal, 6);
        let l = Label::new(Some(&format!("{display} → {value:?}")));
        l.set_hexpand(true);
        l.set_xalign(0.0);
        let del = gtk4::Button::with_label("Remove");
        line.append(&l);
        line.append(&del);
        box_.append(&line);
        let state = state.clone();
        let box2 = box_.clone();
        let field = field.clone();
        del.connect_clicked(move |_| {
            commit_field(&state, |p| {
                p.empty_data.remove(&field);
            });
            rebuild_empty_data(&state, &box2);
        });
    }
}

fn rebuild_exclude_folders(state: &Rc<RcState>, list_box: &gtk4::Box) {
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }
    let idx = state.selected.get();
    let s = state.settings.borrow();
    let Some(p) = s.profiles.get(idx) else {
        return;
    };
    for (i, path) in p.exclude_folders.iter().enumerate() {
        let line = gtk4::Box::new(Orientation::Horizontal, 6);
        let l = Label::new(Some(path));
        l.set_hexpand(true);
        l.set_xalign(0.0);
        let del = gtk4::Button::with_label("Remove");
        line.append(&l);
        line.append(&del);
        list_box.append(&line);
        let state = state.clone();
        let list_box = list_box.clone();
        del.connect_clicked(move |_| {
            commit_field(&state, |p| {
                p.exclude_folders.remove(i);
            });
            rebuild_exclude_folders(&state, &list_box);
        });
    }
}

// ---------------------------------------------------------------------------
// Load / collect
// ---------------------------------------------------------------------------

/// Loads the selected profile into every widget and rebuilds the
/// list editors.
fn load_profile_into_widgets(state: &Rc<RcState>) {
    let Some(w) = widgets() else {
        return;
    };
    let idx = state.selected.get();
    let p = {
        let s = state.settings.borrow();
        match s.profiles.get(idx) {
            Some(p) => p.clone(),
            None => return,
        }
    };
    if let Some(b) = &w.mode_move {
        b.set_active(p.mode == MODE_MOVE);
    }
    if let Some(b) = &w.mode_copy {
        b.set_active(p.mode == MODE_COPY);
    }
    if let Some(b) = &w.mode_simulate {
        b.set_active(p.mode == MODE_SIMULATE);
    }
    if let Some(b) = &w.copy_mode {
        b.set_active(p.copy_mode);
    }
    if let Some(b) = &w.use_file_name {
        b.set_active(p.use_file_name);
    }
    if let Some(b) = &w.use_folder {
        b.set_active(p.use_folder);
    }
    if let Some(b) = &w.move_fileless {
        b.set_active(p.move_fileless);
    }
    if let Some(d) = &w.fileless_format {
        let fmt_idx = match p.fileless_format.as_str() {
            ".png" => 1,
            ".bmp" => 2,
            _ => 0,
        };
        d.set_selected(fmt_idx as u32);
    }
    if let Some(e) = &w.base_folder {
        e.set_text(&p.base_folder);
    }
    if let Some(e) = &w.file_template {
        e.set_text(&p.file_template);
    }
    if let Some(e) = &w.folder_template {
        e.set_text(&p.folder_template);
    }
    if let Some(d) = &w.exclude_mode {
        d.set_selected(if p.exclude_mode == MODE_ONLY { 1 } else { 0 });
    }
    if let Some(d) = &w.exclude_operator {
        d.set_selected(if p.exclude_operator == EXCLUDE_ALL {
            1
        } else {
            0
        });
    }
    if let Some(b) = &w.replace_multiple_spaces {
        b.set_active(p.replace_multiple_spaces);
    }
    if let Some(b) = &w.dont_ask_when_multi_one {
        b.set_active(p.dont_ask_when_multi_one);
    }
    if let Some(b) = &w.remove_empty_folder {
        b.set_active(p.remove_empty_folder);
    }
    if let Some(b) = &w.copy_read_percentage {
        b.set_active(p.copy_read_percentage);
    }
    if let Some(b) = &w.fail_empty_values {
        b.set_active(p.fail_empty_values);
    }
    if let Some(b) = &w.move_failed {
        b.set_active(p.move_failed);
    }
    if let Some(e) = &w.failed_folder {
        e.set_text(&p.failed_folder);
    }

    if let (Some(sample), Some(e), Some(pv)) = (&state.sample, &w.file_template, &w.file_preview) {
        let _ = e;
        update_preview_for(state, sample, true, pv);
    }
    if let (Some(sample), Some(pv)) = (&state.sample, &w.folder_preview) {
        update_preview_for(state, sample, false, pv);
    }

    if let Some(list) = &w.failed_fields_list {
        rebuild_failed_fields(state, list);
    }
    if let Some(list) = &w.excluded_empty_list {
        rebuild_excluded_empty(state, list);
    }
    if let Some(b) = &w.months_box {
        rebuild_months(state, b);
    }
    if let Some(b) = &w.illegal_box {
        rebuild_illegal(state, b);
    }
    if let Some(b) = &w.empty_data_box {
        rebuild_empty_data(state, b);
    }
    if let Some(b) = &w.rules_tree {
        rebuild_rules(state, b);
    }
    if let Some(b) = &w.exclude_folders_list {
        rebuild_exclude_folders(state, b);
    }
}

/// The template entries commit through their changed handlers; the
/// final pass re-commits the text so a load that never fired
/// `changed` still lands in the model.
fn collect_profile_from_widgets(state: &Rc<RcState>) {
    let Some(w) = widgets() else {
        return;
    };
    if let Some(e) = &w.file_template {
        let text = e.text().to_string();
        commit_field(state, |p| p.file_template = text);
    }
    if let Some(e) = &w.folder_template {
        let text = e.text().to_string();
        commit_field(state, |p| p.folder_template = text);
    }
}

// ---------------------------------------------------------------------------
// File pickers
// ---------------------------------------------------------------------------

fn show_error_dialog(parent: &impl IsA<gtk4::Window>, heading: &str, message: &str) {
    let dialog = gtk4::MessageDialog::new(
        Some(parent.upcast_ref::<gtk4::Window>()),
        gtk4::DialogFlags::MODAL,
        gtk4::MessageType::Error,
        gtk4::ButtonsType::Ok,
        message,
    );
    dialog.set_title(Some(heading));
    dialog.connect_response(|d, _| d.close());
    dialog.present();
}

thread_local! {
    static DIALOG_PARENT: RefCell<Option<gtk4::Window>> = const { RefCell::new(None) };
}

const NO_EXPR: Option<&gtk4::Expression> = None;

fn choose_folder(
    parent: &impl IsA<gtk4::Window>,
    title: &str,
    on_pick: impl FnOnce(std::path::PathBuf) + 'static,
) {
    let dialog = gtk4::FileChooserDialog::new(
        Some(title),
        Some(parent.upcast_ref::<gtk4::Window>()),
        gtk4::FileChooserAction::SelectFolder,
        &[
            ("Cancel", gtk4::ResponseType::Cancel),
            ("Select", gtk4::ResponseType::Ok),
        ],
    );
    let on_pick = RefCell::new(Some(on_pick));
    dialog.connect_response(move |d, response| {
        if response == gtk4::ResponseType::Ok {
            if let (Some(file), Some(cb)) = (d.file(), on_pick.borrow_mut().take()) {
                if let Some(path) = file.path() {
                    cb(path);
                }
            }
        }
        d.close();
    });
    dialog.present();
}

fn choose_file(
    parent: &impl IsA<gtk4::Window>,
    title: &str,
    on_open: impl FnOnce(std::path::PathBuf) + 'static,
) {
    let dialog = gtk4::FileChooserDialog::new(
        Some(title),
        Some(parent.upcast_ref::<gtk4::Window>()),
        gtk4::FileChooserAction::Open,
        &[
            ("Cancel", gtk4::ResponseType::Cancel),
            ("Open", gtk4::ResponseType::Ok),
        ],
    );
    let on_open_cell = RefCell::new(Some(on_open));
    dialog.connect_response(move |d, response| {
        if response == gtk4::ResponseType::Ok {
            if let (Some(file), Some(cb)) = (d.file(), on_open_cell.borrow_mut().take()) {
                if let Some(path) = file.path() {
                    cb(path);
                }
            }
        }
        d.close();
    });
    dialog.present();
}

fn choose_file_save(
    parent: &impl IsA<gtk4::Window>,
    title: &str,
    default_name: &str,
    on_save: impl FnOnce(std::path::PathBuf) + 'static,
) {
    let dialog = gtk4::FileChooserDialog::new(
        Some(title),
        Some(parent.upcast_ref::<gtk4::Window>()),
        gtk4::FileChooserAction::Save,
        &[
            ("Cancel", gtk4::ResponseType::Cancel),
            ("Save", gtk4::ResponseType::Ok),
        ],
    );
    dialog.set_current_name(default_name);
    let on_save_cell = RefCell::new(Some(on_save));
    dialog.connect_response(move |d, response| {
        if response == gtk4::ResponseType::Ok {
            if let (Some(file), Some(cb)) = (d.file(), on_save_cell.borrow_mut().take()) {
                if let Some(path) = file.path() {
                    cb(path);
                }
            }
        }
        d.close();
    });
    dialog.present();
}
