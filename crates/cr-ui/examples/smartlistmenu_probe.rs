//! Headless smoke: the smart-list editor's matcher rows and their
//! per-row edit machinery (the C# `btEdit` + `cmEdit` port). Gates:
//! the editor builds with a rule row; every row carries the edit
//! actions; delete/move enable states follow the C# `cmEdit_Opening`
//! rules; delete/cut/copy/paste run through the REAL action path; the
//! paste of a group payload at the MAX_LEVEL cap is rejected; the
//! smart-list dialog round trip survives OK.
use std::cell::RefCell;
use std::rc::Rc;

use cr_core::database::list_items::{ComicBookMatcher, GroupMatcher, SmartListItem, ValueMatcher};
use gtk4::glib;
use gtk4::prelude::*;

fn value(series: &str) -> ComicBookMatcher {
    ComicBookMatcher::Value(ValueMatcher {
        type_name: "ComicBookSeriesMatcher".into(),
        not: false,
        name: String::new(),
        match_value: series.into(),
        match_value_2: String::new(),
        match_operator: 3,
        ignore_case: true,
        option: None,
        plugin_key: None,
    })
}

fn group_chain(depth: usize) -> ComicBookMatcher {
    let mut node = value("deep");
    for _ in 0..depth {
        node = ComicBookMatcher::Group(GroupMatcher {
            matchers: vec![node],
            ..Default::default()
        });
    }
    node
}

fn series(m: &ComicBookMatcher) -> &str {
    match m {
        ComicBookMatcher::Value(v) => &v.match_value,
        _ => "",
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("GATE FAIL: {msg}");
    std::process::exit(1);
}

/// The "Smart List" dialog currently open (the probes run one at a
/// time — sequential editors never overlap).
fn find_editor_dialog() -> gtk4::Dialog {
    for w in gtk4::Window::list_toplevels() {
        if let Ok(d) = w.downcast::<gtk4::Dialog>() {
            if d.title()
                .map(|t| t.starts_with("Smart List"))
                .unwrap_or(false)
            {
                return d;
            }
        }
    }
    fail("the editor dialog is not open");
}

fn main() {
    gtk4::init().expect("gtk init");
    let app = gtk4::Application::builder()
        .application_id("org.comicrust.smartlistmenu-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let app2 = app.clone();
    app.connect_activate(move |_| {
        let app = app2.clone();
        let win = gtk4::ApplicationWindow::new(&app);
        win.set_default_size(900, 600);
        win.present();

        let mut item = SmartListItem::default();
        item.base.name = Some("Probe".into());
        item.matchers = vec![value("Series A"), value("Series B"), value("Series C")];
        let result = Rc::new(RefCell::new(None::<SmartListItem>));
        let done = Rc::clone(&result);
        cr_ui::dialogs::smart_list::show_smart_list_editor(&win, item, Vec::new(), move |out| {
            *done.borrow_mut() = out;
        });

        // ----- A+C. Rows carry the edit machinery; the cmEdit_Opening
        // enable states: Move Up/Down by index, Delete by siblings.
        glib::timeout_add_local(std::time::Duration::from_millis(500), {
            move || {
                if cr_ui::dialogs::smart_list::probe_row_edit_count() != 3 {
                    fail("expected 3 rows with the edit machinery");
                }
                cr_ui::dialogs::smart_list::probe_refresh_row_actions(0);
                cr_ui::dialogs::smart_list::probe_refresh_row_actions(2);
                let up0 = cr_ui::dialogs::smart_list::probe_row_action_enabled(0, "up");
                let down0 = cr_ui::dialogs::smart_list::probe_row_action_enabled(0, "down");
                let up2 = cr_ui::dialogs::smart_list::probe_row_action_enabled(2, "up");
                let down2 = cr_ui::dialogs::smart_list::probe_row_action_enabled(2, "down");
                if up0 != Some(false) || down0 != Some(true) {
                    fail("row 0 must disable Move Up and enable Move Down");
                }
                if up2 != Some(true) || down2 != Some(false) {
                    fail("row 2 must enable Move Up and disable Move Down");
                }
                let del = cr_ui::dialogs::smart_list::probe_row_action_enabled(1, "delete");
                if del != Some(true) {
                    fail("Delete must be enabled while the container holds 3");
                }
                // ----- B. Delete row 0 through the REAL action path.
                cr_ui::dialogs::smart_list::probe_fire_row_action(0, "delete");
                let m = cr_ui::dialogs::smart_list::probe_matchers().expect("the editor is open");
                if m.len() != 2 || series(&m[0]) != "Series B" {
                    fail("delete must remove the row (left: Series B, Series C)");
                }
                if cr_ui::dialogs::smart_list::probe_row_edit_count() != 2 {
                    fail("the rebuild must re-register the rows");
                }
                glib::ControlFlow::Break
            }
        });

        // ----- F. Copy row 0 → the clipboard carries the payload →
        // paste it after row 1 (the payload rides the clipboard read;
        // the probe injects it because Xvfb selection transfers stall).
        glib::timeout_add_local(std::time::Duration::from_millis(800), {
            move || {
                cr_ui::dialogs::smart_list::probe_fire_row_action(0, "copy");
                if !cr_ui::dialogs::smart_list::probe_clipboard_has_matcher() {
                    fail("Copy must put a matcher payload on the clipboard");
                }
                let copied = cr_ui::dialogs::smart_list::probe_matchers()
                    .expect("the editor is open")[0]
                    .clone();
                cr_ui::dialogs::smart_list::probe_paste_payload(1, &copied);
                glib::ControlFlow::Break
            }
        });

        // The paste lands in its async clipboard callback.
        glib::timeout_add_local(std::time::Duration::from_millis(1000), {
            move || {
                let m = cr_ui::dialogs::smart_list::probe_matchers().expect("the editor is open");
                let series_list: Vec<&str> = m.iter().map(series).collect();
                if series_list != ["Series B", "Series C", "Series B"] {
                    fail("paste must insert a clone of the copied row after row 1");
                }
                // ----- G. Cut row 0 (remove + clipboard).
                cr_ui::dialogs::smart_list::probe_fire_row_action(0, "cut");
                glib::ControlFlow::Break
            }
        });

        // ----- D. Down to ONE rule: Delete/Cut disable (the C#
        // `matchers.Count > 1` rule); Paste still works.
        glib::timeout_add_local(std::time::Duration::from_millis(1200), {
            move || {
                let m = cr_ui::dialogs::smart_list::probe_matchers().expect("the editor is open");
                if m.len() != 2 {
                    fail("cut must remove the row and keep 2");
                }
                cr_ui::dialogs::smart_list::probe_fire_row_action(0, "delete");
                let m = cr_ui::dialogs::smart_list::probe_matchers().expect("the editor is open");
                if m.len() != 1 {
                    fail("the second delete must leave a single rule");
                }
                cr_ui::dialogs::smart_list::probe_refresh_row_actions(0);
                let del = cr_ui::dialogs::smart_list::probe_row_action_enabled(0, "delete");
                let cut = cr_ui::dialogs::smart_list::probe_row_action_enabled(0, "cut");
                if del != Some(false) || cut != Some(false) {
                    fail("Delete/Cut must disable on a single-rule container");
                }
                if !cr_ui::dialogs::smart_list::probe_clipboard_has_matcher() {
                    fail("Cut must leave the payload on the clipboard");
                }
                let cut = cr_ui::dialogs::smart_list::probe_matchers().expect("the editor is open")
                    [0]
                .clone();
                cr_ui::dialogs::smart_list::probe_paste_payload(0, &cut);
                glib::ControlFlow::Break
            }
        });

        // Paste landed → commit through OK.
        glib::timeout_add_local(std::time::Duration::from_millis(1500), {
            move || {
                let m = cr_ui::dialogs::smart_list::probe_matchers().expect("the editor is open");
                if m.len() != 2 {
                    fail("the post-cut paste must restore a second rule");
                }
                find_editor_dialog().response(gtk4::ResponseType::Ok);
                glib::ControlFlow::Break
            }
        });

        // ----- Commit gate for editor 1, then editor 2: the paste of
        // a GROUP payload at the MAX_LEVEL cap is rejected while a
        // value payload pastes (the C# `level <= MaxLevel` rule).
        glib::timeout_add_local(std::time::Duration::from_millis(1800), {
            let result = Rc::clone(&result);
            let win = win.clone();
            move || {
                let committed = result.borrow().clone();
                let Some(item) = committed else {
                    fail("OK must commit the edited item");
                };
                if item.matchers.len() != 2 {
                    fail("the committed item must carry the reduced rule set");
                }
                if item.base.name.as_deref() != Some("Probe") {
                    fail("the committed item must keep its name");
                }
                let mut item2 = SmartListItem::default();
                item2.base.name = Some("Probe2".into());
                item2.matchers = vec![group_chain(5)];
                let done = Rc::new(RefCell::new(None::<SmartListItem>));
                let done2 = Rc::clone(&done);
                cr_ui::dialogs::smart_list::show_smart_list_editor(
                    &win,
                    item2,
                    Vec::new(),
                    move |out| {
                        *done2.borrow_mut() = out;
                    },
                );
                glib::ControlFlow::Break
            }
        });

        // The group chain renders 5 group frames + 1 value row. The
        // value row (index 5) sits at the cap: New Group disabled;
        // a group PASTE payload is rejected, a value payload pastes.
        glib::timeout_add_local(std::time::Duration::from_millis(2200), {
            move || {
                if cr_ui::dialogs::smart_list::probe_row_edit_count() != 6 {
                    fail("the group chain must render 5 frames + 1 value row");
                }
                cr_ui::dialogs::smart_list::probe_refresh_row_actions(5);
                let group = cr_ui::dialogs::smart_list::probe_row_action_enabled(5, "group");
                if group != Some(false) {
                    fail("New Group must be disabled at the MAX_LEVEL cap");
                }
                let paste = cr_ui::dialogs::smart_list::probe_row_action_enabled(5, "paste");
                if paste != Some(true) {
                    fail("Paste stays enabled by clipboard content (C# parity)");
                }
                let payload = ComicBookMatcher::Group(GroupMatcher {
                    matchers: vec![value("x")],
                    ..Default::default()
                });
                cr_ui::dialogs::smart_list::probe_clipboard_write_matcher(&payload);
                cr_ui::dialogs::smart_list::probe_paste_payload(5, &payload);
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_millis(2400), {
            move || {
                // The group payload was rejected at the cap: the
                // innermost group still holds exactly one matcher.
                let m = cr_ui::dialogs::smart_list::probe_matchers().expect("the editor is open");
                if walk_innermost_len(&m) != 1 {
                    fail("a group payload must NOT paste at the cap");
                }
                let value = value("pasted");
                cr_ui::dialogs::smart_list::probe_clipboard_write_matcher(&value);
                cr_ui::dialogs::smart_list::probe_paste_payload(5, &value);
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_millis(2600), {
            move || {
                let innermost = walk_innermost_len(
                    &cr_ui::dialogs::smart_list::probe_matchers().expect("the editor is open"),
                );
                if innermost != 2 {
                    fail("a value payload must paste at the cap");
                }
                find_editor_dialog().response(gtk4::ResponseType::Ok);
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_millis(2900), {
            let app = app.clone();
            move || {
                app.quit();
                glib::ControlFlow::Break
            }
        });
    });
    app.run();
}

/// The matcher count at the bottom of a single-child group chain.
fn walk_innermost_len(matchers: &[ComicBookMatcher]) -> usize {
    let mut list = matchers;
    loop {
        match list.first() {
            Some(ComicBookMatcher::Group(g)) => list = &g.matchers,
            _ => return list.len(),
        }
    }
}
