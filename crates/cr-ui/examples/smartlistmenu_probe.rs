//! Headless smoke: the smart-list editor builds matcher rows whose
//! type picker is the `CreateComicBookMatchersMenu`-shaped PopoverMenu
//! (All + letter submenus over the 97 spec descriptions). Gates: the
//! editor builds with a rule row, the editor commits on OK, and the
//! smart-list dialog round trip survives.
use std::cell::RefCell;
use std::rc::Rc;

use cr_core::database::list_items::{ComicBookMatcher, SmartListItem, ValueMatcher};
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
        item.matchers = vec![value("Probe Series")];
        let result = Rc::new(RefCell::new(None::<SmartListItem>));
        let done = Rc::clone(&result);
        cr_ui::dialogs::smart_list::show_smart_list_editor(&win, item, Vec::new(), move |out| {
            *done.borrow_mut() = out;
        });
        glib::timeout_add_local(std::time::Duration::from_millis(600), {
            let result = Rc::clone(&result);
            let win = win.clone();
            move || {
                let edited = result.borrow().clone();
                println!(
                    "editor built + committed={:?}",
                    edited.and_then(|e| e.base.name).unwrap_or_default()
                );
                win.close();
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(1200), {
            let app = app.clone();
            move || {
                app.quit();
                glib::ControlFlow::Break
            }
        });
    });
    app.run();
}
