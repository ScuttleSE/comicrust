//! The smart-list editor — the `SmartListDialog` (visual designer) +
//! `SmartListQueryDialog` (query text) port as one dialog with a
//! Designer | Query notebook. The C# runs two dialogs and Ctrl
//! swaps them; both forms share the same model here.
//!
//! Semantics (the C# `EditSmartListItem`): the editor edits a CLONE;
//! OK commits it through the caller's callback, Cancel discards
//! (the caller removes a freshly created list). The Query tab shows
//! `ComicSmartListItem.ToString()` (the Phase-2 renderer); OK parses
//! the text back — a parse failure keeps the OLD matchers (C#
//! parity) and keeps the dialog open with the error.
//!
//! Structural matcher edits run through
//! `cr_engine::matcher::edit_ops`; every structural change rebuilds
//! the row area wholesale (the C# syncs incrementally — the rebuild
//! is the simpler port for ≤ dozens of rows).

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gdk;
use gtk4::gio;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, CheckButton, ComboBoxText, Dialog, Entry, Frame, Grid, Label,
    Notebook, Orientation, PopoverMenu, ScrolledWindow, TextView,
};

use cr_core::database::list_items::{ComicBookMatcher, GroupMatcher, SmartListItem, ValueMatcher};
use cr_core::model::enums::{ComicSmartListLimitType, MatcherMode};
use cr_core::xml::scalar::CrGuid;
use cr_engine::matcher::edit_ops;
use cr_engine::matcher::query::{parse_smart_list_query, render_smart_list_query, SmartListQuery};
use cr_engine::matcher::spec;

type StateRef = Rc<RefCell<SmartListItem>>;
type RebuildFn = Rc<dyn Fn()>;

/// Opens the editor. `on_done` runs once with the committed item
/// (OK) or `None` (Cancel).
pub fn show_smart_list_editor(
    parent: &impl IsA<gtk4::Window>,
    item: SmartListItem,
    base_options: Vec<(CrGuid, String)>,
    on_done: impl Fn(Option<SmartListItem>) + 'static,
) {
    let state: StateRef = Rc::new(RefCell::new(item.clone()));

    let dialog = Dialog::builder()
        .title("Smart List")
        .transient_for(parent)
        .modal(true)
        .default_width(880)
        .default_height(640)
        .build();
    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_spacing(6);

    // ----- The head fields (the smart-list properties) -----
    let head = Grid::new();
    head.set_row_spacing(4);
    head.set_column_spacing(8);

    let name_entry = Entry::builder()
        .text(item.base.name.as_deref().unwrap_or(""))
        .hexpand(true)
        .build();
    head.attach(&Label::new(Some("Name")), 0, 0, 1, 1);
    head.attach(&name_entry, 1, 0, 1, 1);
    {
        let state = Rc::clone(&state);
        name_entry.connect_changed(move |e| {
            state.borrow_mut().base.name = Some(e.text().trim().to_string());
        });
    }

    let notes_entry = Entry::builder()
        .text(item.base.description.as_str())
        .hexpand(true)
        .build();
    head.attach(&Label::new(Some("Notes")), 0, 1, 1, 1);
    head.attach(&notes_entry, 1, 1, 1, 1);
    {
        let state = Rc::clone(&state);
        notes_entry.connect_changed(move |e| {
            state.borrow_mut().base.description = e.text().trim().to_string();
        });
    }

    // The C# `FillBaseCombo`: every list except the ones that would
    // recurse (the editor's own id); the Library = the empty Guid.
    let base_combo = ComboBoxText::new();
    for (id, label) in &base_options {
        base_combo.append(Some(&id.to_string()), label);
    }
    {
        let current = state.borrow().base_list_id;
        let pos = base_options.iter().position(|(id, _)| *id == current);
        base_combo.set_active(pos.map(|p| p as u32));
    }
    head.attach(&Label::new(Some("Base List")), 0, 2, 1, 1);
    head.attach(&base_combo, 1, 2, 1, 1);
    {
        let state = Rc::clone(&state);
        base_combo.connect_changed(move |c| {
            let Some(active) = c.active_id() else {
                return;
            };
            if let Ok(id) = CrGuid::parse(&active) {
                state.borrow_mut().base_list_id = id;
            }
        });
    }

    let mode_combo = ComboBoxText::new();
    mode_combo.append(Some("and"), "Match ALL conditions");
    mode_combo.append(Some("or"), "Match ANY condition");
    mode_combo.set_active(Some(if state.borrow().matcher_mode == MatcherMode::And {
        0
    } else {
        1
    }));
    head.attach(&Label::new(Some("Match Mode")), 0, 3, 1, 1);
    head.attach(&mode_combo, 1, 3, 1, 1);
    {
        let state = Rc::clone(&state);
        mode_combo.connect_changed(move |c| {
            state.borrow_mut().matcher_mode = match c.active() {
                Some(1) => MatcherMode::Or,
                _ => MatcherMode::And,
            };
        });
    }

    let not_base = CheckButton::with_label("Not in Base List");
    not_base.set_active(state.borrow().not_in_base_list);
    head.attach(&not_base, 1, 4, 1, 1);
    {
        let state = Rc::clone(&state);
        not_base.connect_toggled(move |c| {
            state.borrow_mut().not_in_base_list = c.is_active();
        });
    }

    let limit_check = CheckButton::with_label("Limit to");
    limit_check.set_active(state.borrow().limit);
    let limit_type = ComboBoxText::new();
    for (id, label) in [("0", "Book Count"), ("1", "Megabytes"), ("2", "Gigabytes")] {
        limit_type.append(Some(id), label);
    }
    limit_type.set_active(Some(state.borrow().limit_type as u32));
    limit_type.set_sensitive(limit_check.is_active());
    let limit_value = Entry::builder()
        .text(state.borrow().limit_value.to_string())
        .width_chars(6)
        .build();
    let limit_row = GtkBox::new(Orientation::Horizontal, 6);
    limit_row.append(&limit_check);
    limit_row.append(&limit_type);
    limit_row.append(&limit_value);
    head.attach(&limit_row, 1, 5, 1, 1);
    {
        let state = Rc::clone(&state);
        let type_combo = limit_type.clone();
        limit_check.connect_toggled(move |c| {
            state.borrow_mut().limit = c.is_active();
            type_combo.set_sensitive(c.is_active());
        });
    }
    {
        let state = Rc::clone(&state);
        limit_type.connect_changed(move |c| {
            if let Some(id) = c.active_id() {
                state.borrow_mut().limit_type = match id.as_str() {
                    "1" => ComicSmartListLimitType::MB,
                    "2" => ComicSmartListLimitType::GB,
                    _ => ComicSmartListLimitType::Count,
                };
            }
        });
    }
    {
        let state = Rc::clone(&state);
        limit_value.connect_changed(move |e| {
            let v: i32 = e.text().trim().parse().unwrap_or(0).max(0);
            state.borrow_mut().limit_value = v;
        });
    }

    let quick_open = CheckButton::with_label("Show in Quick Open");
    quick_open.set_active(state.borrow().base.quick_open);
    head.attach(&quick_open, 1, 6, 1, 1);
    {
        let state = Rc::clone(&state);
        quick_open.connect_toggled(move |c| {
            state.borrow_mut().base.quick_open = c.is_active();
        });
    }

    // ----- The matcher rows (rebuilt wholesale on structural
    // changes; the field widgets write straight into the item) -----
    let rows_box = GtkBox::new(Orientation::Vertical, 4);
    let rows_scroll = ScrolledWindow::builder()
        .child(&rows_box)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .build();
    let add_rule_button = Button::with_label("Add Rule");
    let add_group_button = Button::with_label("Add Group");
    let buttons = GtkBox::new(Orientation::Horizontal, 6);
    buttons.append(&add_rule_button);
    buttons.append(&add_group_button);
    buttons.set_halign(Align::Start);

    let designer_page = GtkBox::new(Orientation::Vertical, 6);
    designer_page.append(&head);
    designer_page.append(&rows_scroll);
    designer_page.append(&buttons);

    // ----- The query tab -----
    let query_view = TextView::builder().monospace(true).build();
    let query_error = Label::builder()
        .label("")
        .halign(Align::Start)
        .visible(false)
        .build();
    let query_page = GtkBox::new(Orientation::Vertical, 4);
    query_page.append(
        &ScrolledWindow::builder()
            .child(&query_view)
            .vexpand(true)
            .build(),
    );
    query_page.append(&query_error);

    let notebook = Notebook::new();
    notebook.append_page(&designer_page, Some(&Label::new(Some("Designer"))));
    notebook.append_page(&query_page, Some(&Label::new(Some("Query"))));

    let error_label = Label::builder()
        .label("")
        .halign(Align::Start)
        .visible(false)
        .build();
    content.append(&notebook);
    content.append(&error_label);
    dialog.add_button("OK", gtk4::ResponseType::Ok);
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);

    // The wholesale rebuild (structural ops re-run this). The rows
    // rebuild needs ITSELF for the nested group menus — a two-slot
    // cell breaks the cycle: the outer closure reads the slot.
    let rebuild_slot: Rc<RefCell<Option<RebuildFn>>> = Rc::new(RefCell::new(None));
    let rebuild: RebuildFn = {
        let state = Rc::clone(&state);
        let rows_box = rows_box.clone();
        let dialog = dialog.clone();
        let slot = Rc::clone(&rebuild_slot);
        Rc::new(move || {
            while let Some(child) = rows_box.first_child() {
                rows_box.remove(&child);
            }
            let item = state.borrow().clone();
            let inner = slot.borrow().as_ref().expect("rebuild set").clone();
            build_rows(&state, &rows_box, &item.matchers, &[], &dialog, &inner);
        })
    };
    *rebuild_slot.borrow_mut() = Some(Rc::clone(&rebuild));

    // Add Rule / Add Group with no selection appends at the root.
    {
        let state = Rc::clone(&state);
        let rebuild = Rc::clone(&rebuild);
        add_rule_button.connect_clicked(move |_| {
            state.borrow_mut().matchers.push(default_matcher());
            rebuild();
        });
    }
    {
        let state = Rc::clone(&state);
        let rebuild = Rc::clone(&rebuild);
        add_group_button.connect_clicked(move |_| {
            state
                .borrow_mut()
                .matchers
                .push(ComicBookMatcher::Group(GroupMatcher {
                    matchers: vec![default_matcher()],
                    ..Default::default()
                }));
            rebuild();
        });
    }

    // The query text syncs on tab switches: entering Query renders
    // the item; entering Designer parses the text back (a failure
    // keeps the OLD matchers and surfaces the error on OK).
    {
        let state = Rc::clone(&state);
        let query_view = query_view.clone();
        notebook.connect_switch_page(move |_, _page, page_no| {
            if page_no == 1 {
                let q = item_to_query(&state.borrow());
                query_view.buffer().set_text(&render_smart_list_query(&q));
            }
        });
    }

    // OK: apply the query text first (it may hold edits), then
    // commit. A parse failure blocks the close with the error.
    {
        let state = Rc::clone(&state);
        let query_view = query_view.clone();
        let error_label = error_label.clone();
        let on_done = on_done;
        dialog.connect_response(move |dlg, response| match response {
            gtk4::ResponseType::Ok => {
                let text = query_text(&query_view);
                let parse = if text.trim().is_empty() {
                    Ok(())
                } else {
                    parse_smart_list_query(&text)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                };
                match parse {
                    Ok(()) => {
                        if !text.trim().is_empty() {
                            // Re-parse to move the fields over (the
                            // shape above already validated). The
                            // parser returns the ENGINE tree; the
                            // item stores the RAW model.
                            if let Ok(q) = parse_smart_list_query(&text) {
                                let mut s = state.borrow_mut();
                                s.matcher_mode = q.group.matcher_mode;
                                s.matchers = q.group.matchers.iter().map(|m| m.to_raw()).collect();
                            }
                        } else {
                            let mut s = state.borrow_mut();
                            s.matchers.clear();
                            s.matcher_mode = MatcherMode::And;
                        }
                        let committed = state.borrow().clone();
                        dlg.close();
                        on_done(Some(committed));
                    }
                    Err(err) => {
                        error_label.set_text(&format!("Bad query: {err}"));
                        error_label.set_visible(true);
                    }
                }
            }
            _ => {
                dlg.close();
                on_done(None);
            }
        });
    }

    rebuild();
    dialog.present();
}

fn query_text(view: &TextView) -> String {
    let buf = view.buffer();
    buf.text(&buf.start_iter(), &buf.end_iter(), false)
        .trim()
        .to_string()
}

/// The `ComicSmartListItem` → query prelude + group (the base list
/// renders by NAME in the C#; the dialog shows the ids — the
/// prelude carries no base reference here, the item's field stays).
fn item_to_query(item: &SmartListItem) -> SmartListQuery {
    // The raw model matchers → the engine tree (the reverse of
    // `Matcher::to_raw`), so the renderer sees its own currency. An
    // unknown matcher class drops (from_raw → None) — the same
    // tolerance as the query parser.
    let group = cr_engine::matcher::tree::GroupMatcher {
        not: false,
        matcher_mode: item.matcher_mode,
        collapsed: false,
        matchers: Vec::new(),
    };
    let mut group = group;
    for m in &item.matchers {
        if let Some(engine) = cr_engine::matcher::tree::Matcher::from_raw(m) {
            group.matchers.push(engine);
        }
    }
    SmartListQuery {
        name: item.base.name.clone(),
        base_list: None,
        not_in_base_list: item.not_in_base_list,
        group,
    }
}

fn default_matcher() -> ComicBookMatcher {
    ComicBookMatcher::Value(ValueMatcher {
        type_name: "ComicBookSeriesMatcher".into(),
        match_operator: 3,
        ..Default::default()
    })
}

/// Builds the row widgets for a matcher list (recursing into
/// groups). `path` is the caller's container path.
fn build_rows(
    state: &StateRef,
    parent_box: &GtkBox,
    matchers: &[ComicBookMatcher],
    base_path: &[usize],
    dialog: &Dialog,
    rebuild: &RebuildFn,
) {
    for (i, matcher) in matchers.iter().enumerate() {
        let mut path = base_path.to_vec();
        path.push(i);
        match matcher {
            ComicBookMatcher::Value(_) => {
                let row = build_value_row(state, &path, dialog, rebuild);
                parent_box.append(&row);
            }
            ComicBookMatcher::Group(group) => {
                let frame = build_group_row(state, &path, group, dialog, rebuild);
                parent_box.append(&frame);
            }
        }
    }
}

fn run_row_op(state: &StateRef, path: &[usize], op: RowOp, rebuild: &RebuildFn) {
    {
        let mut s = state.borrow_mut();
        let matchers = &mut s.matchers;
        match op {
            RowOp::AddRule => {
                edit_ops::add_rule(matchers, path);
            }
            RowOp::AddGroup => {
                edit_ops::add_group(matchers, path);
            }
            RowOp::Delete => {
                edit_ops::remove_node(matchers, path);
            }
            RowOp::Up => {
                edit_ops::move_node(matchers, path, -1);
            }
            RowOp::Down => {
                edit_ops::move_node(matchers, path, 1);
            }
        }
    }
    rebuild();
}

#[derive(Clone, Copy)]
enum RowOp {
    AddRule,
    AddGroup,
    Delete,
    Up,
    Down,
}

/// The per-row edit menu (New Rule / New Group / Delete / Up /
/// Down) — a right-click popover on the row (the C# `cmEdit`).
fn attach_row_menu<W: IsA<gtk4::Widget>>(
    state: &StateRef,
    row: &W,
    path: &[usize],
    rebuild: &RebuildFn,
) {
    let path = path.to_vec();
    let state = Rc::clone(state);
    let rebuild = Rc::clone(rebuild);
    let menu_model = gio::Menu::new();
    menu_model.append(Some("New Rule"), Some("row.rule"));
    menu_model.append(Some("New Group"), Some("row.group"));
    menu_model.append(Some("Delete"), Some("row.delete"));
    menu_model.append(Some("Move Up"), Some("row.up"));
    menu_model.append(Some("Move Down"), Some("row.down"));
    let group = gio::SimpleActionGroup::new();
    for (name, op) in [
        ("rule", RowOp::AddRule),
        ("group", RowOp::AddGroup),
        ("delete", RowOp::Delete),
        ("up", RowOp::Up),
        ("down", RowOp::Down),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let state = Rc::clone(&state);
        let path = path.clone();
        let rebuild = Rc::clone(&rebuild);
        action.connect_activate(move |_, _| run_row_op(&state, &path, op, &rebuild));
        group.add_action(&action);
    }
    row.insert_action_group("row", Some(&group));
    let popover = PopoverMenu::from_model(Some(&menu_model));
    popover.set_parent(row);
    let row_widget = row.clone().upcast::<gtk4::Widget>();
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3);
    gesture.connect_pressed(move |g, _n, x, y| {
        g.set_state(gtk4::EventSequenceState::Claimed);
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32 + 4, 1, 1)));
        popover.popup();
    });
    row_widget.add_controller(gesture);
}

fn build_value_row(
    state: &StateRef,
    path: &[usize],
    dialog: &Dialog,
    rebuild: &RebuildFn,
) -> GtkBox {
    let _ = dialog;
    let row = GtkBox::new(Orientation::Horizontal, 4);
    row.set_margin_start(8);
    let matcher = match walk_matcher(&state.borrow().matchers, path) {
        Some(ComicBookMatcher::Value(v)) => v.clone(),
        _ => return row,
    };
    let matcher_spec = spec::by_class_name(&matcher.type_name);

    // The type dropdown (the C# matcher menu; switching keeps the
    // values via `edit_ops::switch_type`).
    let type_combo = ComboBoxText::new();
    for s in spec::all_specs() {
        type_combo.append(Some(s.class_name), s.description);
    }
    type_combo.set_active(Some(
        spec::all_specs()
            .iter()
            .position(|s| s.class_name == matcher.type_name)
            .unwrap_or(0) as u32,
    ));
    {
        let state = Rc::clone(state);
        let path = path.to_vec();
        let rebuild = Rc::clone(rebuild);
        type_combo.connect_changed(move |c| {
            let Some(active) = c.active_id() else {
                return;
            };
            let switched = {
                let s = state.borrow();
                match walk_matcher(&s.matchers, &path) {
                    Some(ComicBookMatcher::Value(v)) => {
                        if v.type_name != active.as_str() {
                            edit_ops::switch_type(&ComicBookMatcher::Value(v.clone()), &active)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            };
            if let Some(switched) = switched {
                let mut s = state.borrow_mut();
                if let Some(node) = walk_matcher_mut(&mut s.matchers, &path) {
                    *node = switched;
                }
                drop(s);
                rebuild();
            }
        });
    }

    // The operator combo (the spec's operator list).
    let op_combo = ComboBoxText::new();
    if let Some(current) = matcher_spec {
        for (i, op) in current.operators().iter().enumerate() {
            op_combo.append(Some(&i.to_string()), op);
        }
        op_combo.set_active(Some(
            (matcher.match_operator as usize).min(current.operators().len() - 1) as u32,
        ));
    }
    {
        let state = Rc::clone(state);
        let path = path.to_vec();
        op_combo.connect_changed(move |c| {
            let Some(active) = c.active() else {
                return;
            };
            let mut s = state.borrow_mut();
            if let Some(ComicBookMatcher::Value(v)) = walk_matcher_mut(&mut s.matchers, &path) {
                v.match_operator = active as i32;
            }
        });
    }

    // The value fields (the argument count of the CURRENT operator).
    let arg_count = matcher_spec
        .map(|s| s.argument_count(matcher.match_operator as usize))
        .unwrap_or(1);
    let value1 = Entry::builder()
        .text(&matcher.match_value)
        .width_chars(18)
        .hexpand(true)
        .build();
    {
        let state = Rc::clone(state);
        let path = path.to_vec();
        value1.connect_changed(move |e| {
            let mut s = state.borrow_mut();
            if let Some(ComicBookMatcher::Value(v)) = walk_matcher_mut(&mut s.matchers, &path) {
                v.match_value = e.text().to_string();
            }
        });
    }
    let value2 = Entry::builder()
        .text(&matcher.match_value_2)
        .width_chars(18)
        .hexpand(true)
        .visible(arg_count >= 2)
        .build();
    {
        let state = Rc::clone(state);
        let path = path.to_vec();
        value2.connect_changed(move |e| {
            let mut s = state.borrow_mut();
            if let Some(ComicBookMatcher::Value(v)) = walk_matcher_mut(&mut s.matchers, &path) {
                v.match_value_2 = e.text().to_string();
            }
        });
    }

    let not_check = CheckButton::with_label("Not");
    not_check.set_active(matcher.not);
    {
        let state = Rc::clone(state);
        let path = path.to_vec();
        not_check.connect_toggled(move |c| {
            let mut s = state.borrow_mut();
            if let Some(ComicBookMatcher::Value(v)) = walk_matcher_mut(&mut s.matchers, &path) {
                v.not = c.is_active();
            }
        });
    }

    row.append(&type_combo);
    row.append(&op_combo);
    row.append(&value1);
    row.append(&value2);
    row.append(&not_check);
    attach_row_menu(state, &row, path, rebuild);
    row
}

fn build_group_row(
    state: &StateRef,
    path: &[usize],
    group: &GroupMatcher,
    dialog: &Dialog,
    rebuild: &RebuildFn,
) -> Frame {
    let frame = Frame::new(Some("Group"));
    let inner = GtkBox::new(Orientation::Vertical, 4);
    inner.set_margin_top(4);
    inner.set_margin_bottom(4);
    inner.set_margin_start(4);
    inner.set_margin_end(4);

    let mode_combo = ComboBoxText::new();
    mode_combo.append(Some("and"), "Match ALL");
    mode_combo.append(Some("or"), "Match ANY");
    mode_combo.set_active(Some(if group.matcher_mode == MatcherMode::And {
        0
    } else {
        1
    }));
    mode_combo.set_halign(Align::Start);
    {
        let state = Rc::clone(state);
        let path = path.to_vec();
        mode_combo.connect_changed(move |c| {
            let Some(active) = c.active_id() else {
                return;
            };
            let mut s = state.borrow_mut();
            if let Some(ComicBookMatcher::Group(g)) = walk_matcher_mut(&mut s.matchers, &path) {
                g.matcher_mode = match active.as_str() {
                    "or" => MatcherMode::Or,
                    _ => MatcherMode::And,
                };
            }
        });
    }
    inner.append(&mode_combo);
    build_rows(state, &inner, &group.matchers, path, dialog, rebuild);
    frame.set_child(Some(&inner));
    attach_row_menu(state, &frame, path, rebuild);
    frame
}

fn walk_matcher<'a>(
    matchers: &'a [ComicBookMatcher],
    path: &[usize],
) -> Option<&'a ComicBookMatcher> {
    let mut list = matchers;
    for (i, idx) in path.iter().enumerate() {
        let last = i + 1 == path.len();
        let node = list.get(*idx)?;
        if last {
            return Some(node);
        }
        match node {
            ComicBookMatcher::Group(g) => list = &g.matchers,
            _ => return None,
        }
    }
    None
}

fn walk_matcher_mut<'a>(
    matchers: &'a mut Vec<ComicBookMatcher>,
    path: &[usize],
) -> Option<&'a mut ComicBookMatcher> {
    let mut list = matchers;
    for (i, idx) in path.iter().enumerate() {
        let last = i + 1 == path.len();
        if last {
            return list.get_mut(*idx);
        }
        match list.get_mut(*idx) {
            Some(ComicBookMatcher::Group(g)) => list = &mut g.matchers,
            _ => return None,
        }
    }
    None
}
