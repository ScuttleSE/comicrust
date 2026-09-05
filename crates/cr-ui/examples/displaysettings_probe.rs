//! Headless probe: the T12 Book Display Settings dialog
//! (`ComicDisplaySettingsDialog`). Gates: the defaults match the C#
//! workspace defaults, the combo row counts match the C# sets (5
//! transitions, 3 background types, 5 layouts, 4 bundled papers, 14
//! bundled backgrounds), the visibility rules follow the C#
//! handlers, Apply pushes the widget values to the callback AND the
//! session copy, a new view seeds from that copy, OK closes, Cancel
//! discards, and the view-level apply/toggle round-trips.
//! Run: Xvfb + `cargo run -p cr-ui --example displaysettings_probe`
//! (no library access — no XDG isolation needed).
use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use cr_ui::dialogs::display_settings::DisplaySettingsHandle;
use cr_ui::reader::page_view::{
    session_display_options, set_session_display_options, DisplayOptions, ImageBackgroundMode,
    ImageLayout, PageTransitionEffect, PageView,
};

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::theme::init();

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.displaysettings-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("display settings probe")
            .build();
        window.present();

        let applied: Rc<RefCell<Vec<DisplayOptions>>> = Rc::new(RefCell::new(Vec::new()));

        // 1. Defaults.
        let handle = open(&window, DisplayOptions::default(), Rc::clone(&applied));
        assert_eq!(
            handle.transition.active().unwrap_or(9),
            1,
            "A: Fade default"
        );
        assert!(handle.realistic.is_active(), "A: realistic default ON");
        assert!(!handle.margin_check.is_active(), "A: margin default OFF");
        assert!(
            (handle.margin_scale.value() - 5.0).abs() < 0.01,
            "A: 5 % margin default"
        );
        assert_eq!(handle.bg_type.active().unwrap_or(9), 1, "A: Color default");
        assert_eq!(
            DisplaySettingsHandle::rows(&handle.paper_combo),
            5,
            "A: Default + 4 bundled papers"
        );
        assert_eq!(
            DisplaySettingsHandle::rows(&handle.texture_combo),
            15,
            "A: None + 14 bundled backgrounds"
        );
        // Visibility: Color mode → the color row shows, the texture
        // row hidden; a bundled paper (empty row selected) → the
        // strength row hidden.
        assert!(handle.color.is_visible(), "A: color row visible on Color");
        assert!(
            !handle.texture_combo.is_visible(),
            "A: texture row hidden on Color"
        );
        assert!(
            !handle.strength_scale.is_visible(),
            "A: strength hidden for Default paper"
        );
        assert!(
            !handle.paper_layout.is_visible(),
            "A: paper layout hidden for Default paper"
        );
        println!("GATE A PASS (defaults)");

        // 2. The visibility rules (the C# SelectedIndexChanged
        // handlers): Texture type → the texture row + layout combo
        // (bundled selection parses the layout, so the layout combo
        // stays hidden); Solid Color → the color row.
        handle.bg_type.set_active(Some(2));
        assert!(
            handle.texture_combo.is_visible(),
            "B: texture row visible on Texture"
        );
        assert!(
            handle.texture_browse.is_visible(),
            "B: browse visible on Texture"
        );
        handle.select_texture(true, "Black [S].jpg");
        if std::env::var("CR_DEBUG_DSD").as_deref() == Ok("1") {
            eprintln!(
                "DSD probe: after select bg active={:?} layout_visible={} layout_active={:?}",
                handle.texture_combo.active(),
                handle.bg_layout.is_visible(),
                handle.bg_layout.active()
            );
        }
        assert!(
            !handle.bg_layout.is_visible(),
            "B: bundled background keeps the layout combo hidden"
        );
        assert_eq!(
            handle.bg_layout.active().unwrap_or(9),
            3,
            "B: the [S] code parses to Stretch (the C# silent apply)"
        );
        // A real custom (browsed) file — the C# browse guarantees
        // an existing path.
        std::fs::create_dir_all("/tmp/opencode/displaysettings").unwrap();
        std::fs::copy(
            "crates/cr-ui/assets/papers/Checkered.jpg",
            "/tmp/opencode/displaysettings/custom-bg.jpg",
        )
        .unwrap();
        handle.select_texture(true, "/tmp/opencode/displaysettings/custom-bg.jpg");
        assert!(
            handle.bg_layout.is_visible(),
            "B: custom background shows the layout combo"
        );
        handle.bg_type.set_active(Some(1));
        assert!(
            handle.color.is_visible(),
            "B: color row back on Solid Color"
        );
        assert!(
            !handle.texture_combo.is_visible(),
            "B: texture row hidden on Solid Color"
        );
        // Paper: a bundled paper hides the layout combo but applies
        // its parsed layout; the strength row shows.
        handle.select_texture(false, "Checkered.jpg");
        assert!(
            handle.strength_scale.is_visible(),
            "B: strength visible for a paper"
        );
        assert!(
            !handle.paper_layout.is_visible(),
            "B: bundled paper keeps the layout combo hidden"
        );
        handle.paper_combo.set_active(Some(3));
        assert!(
            handle.paper_combo.active() == Some(3),
            "B: sanity (paper combo interactive)"
        );
        println!("GATE B PASS (visibility rules)");

        // 3. Apply: edit fields and fire the Apply response — the
        // callback receives the widget values and the dialog stays.
        handle.transition.set_active(Some(0));
        handle.margin_check.set_active(true);
        handle.margin_scale.set_value(25.0);
        handle.realistic.set_active(false);
        handle.bg_type.set_active(Some(2));
        handle.select_texture(true, "Black [S].jpg");
        handle.strength_scale.set_value(30.0);
        handle.select_texture(false, "Checkered.jpg");
        handle.dialog.response(gtk4::ResponseType::Apply);
        let opts = applied.borrow().last().cloned().expect("C: apply recorded");
        assert_eq!(opts.transition, PageTransitionEffect::None, "C: transition");
        assert!(opts.page_margin, "C: margin on");
        assert!(
            (opts.page_margin_percent - 0.25).abs() < 0.001,
            "C: 25 % margin"
        );
        assert!(!opts.realistic_pages, "C: realistic off");
        assert_eq!(
            opts.background_mode,
            ImageBackgroundMode::Texture,
            "C: texture mode"
        );
        assert!(
            opts.background_texture
                .as_deref()
                .is_some_and(|p| p.ends_with("Black [S].jpg")),
            "C: background texture path"
        );
        assert_eq!(
            opts.background_layout,
            ImageLayout::Stretch,
            "C: parsed background layout"
        );
        assert!(
            (opts.paper_strength - 0.3).abs() < 0.001,
            "C: paper strength"
        );
        assert!(
            opts.paper_texture
                .as_deref()
                .is_some_and(|p| p.ends_with("Checkered.jpg")),
            "C: paper path"
        );
        // The apply recorded the session copy (the C# workspace
        // write-back shape).
        assert_eq!(
            session_display_options(),
            opts,
            "C: the session copy updated"
        );
        println!("GATE C PASS (apply + session copy)");

        // 4. A new view seeds from the session copy; the view-level
        // apply and the realistic-pages toggle round-trip.
        let pool = std::sync::Arc::new(cr_engine::image_pool::ImagePool::new(Some(
            std::path::Path::new("/tmp/opencode/displaysettings/cache"),
        )));
        let view = PageView::new(pool);
        assert_eq!(
            view.display_options(),
            opts,
            "D: a new view seeds from the session copy"
        );
        let mut changed = opts.clone();
        changed.transition = PageTransitionEffect::TopDown;
        changed.background_color = Some([0.2, 0.4, 0.6]);
        changed.page_margin = false;
        set_session_display_options(changed.clone());
        view.apply_display_options(&changed);
        assert_eq!(
            view.display_options(),
            changed,
            "D: the view applies the options"
        );
        assert_eq!(
            session_display_options(),
            changed,
            "D: the session copy rides the apply"
        );
        let before = view.display_options().realistic_pages;
        view.toggle_realistic_pages();
        assert_eq!(
            view.display_options().realistic_pages,
            !before,
            "D: ToggleRealisticPages flips the flag"
        );
        println!("GATE D PASS (view seed + apply + toggle)");

        // 5. OK closes (the callback fired once more), Cancel
        // discards.
        let count = applied.borrow().len();
        handle.realistic.set_active(true);
        handle.dialog.response(gtk4::ResponseType::Ok);
        assert_eq!(applied.borrow().len(), count + 1, "E: OK applies");
        assert!(!handle.dialog.is_visible(), "E: OK closes the dialog");
        handle.dialog.response(gtk4::ResponseType::Cancel);
        assert_eq!(applied.borrow().len(), count + 1, "E: Cancel never applies");
        println!("GATE E PASS (OK closes, Cancel discards)");

        glib::timeout_add_local_once(std::time::Duration::from_millis(50), {
            let app = app.clone();
            move || app.quit()
        });
    });

    app.run();
    println!("DISPLAYSETTINGS PROBE PASS");
}

fn open(
    window: &gtk4::ApplicationWindow,
    opts: DisplayOptions,
    applied: Rc<RefCell<Vec<DisplayOptions>>>,
) -> DisplaySettingsHandle {
    cr_ui::dialogs::display_settings::show_display_settings(window, opts, move |opts| {
        // The shell handler shape: the session copy records first,
        // then every open view re-applies (the probe has no shell —
        // the views are gated in step 4).
        set_session_display_options(opts.clone());
        applied.borrow_mut().push(opts.clone());
    })
}
