//! Release probe for the Comic Vine cache manager (Phase 20, ADR-064).
//!
//! Run with isolated XDG paths under `/tmp/opencode` and Xvfb. The probe
//! opens the dialog, searches series ID 806, shows its issue rows, and saves
//! manual metadata. Mock-server integration tests cover both API update modes.

use std::rc::Rc;
use std::sync::Arc;

use cr_scrape::cache::{CvCache, IssueSkeleton, SqliteCache, VolumeRow};
use gtk4::glib;
use gtk4::prelude::*;

fn main() {
    if !std::env::var("XDG_DATA_HOME")
        .map(|value| value.contains("/tmp/opencode"))
        .unwrap_or(false)
    {
        eprintln!("REFUSED: set XDG_DATA_HOME to an isolated path under /tmp/opencode");
        std::process::exit(1);
    }

    gtk4::init().expect("GTK init");
    let cache = Arc::new(SqliteCache::in_memory().expect("cache"));
    cache
        .put_volumes(&[VolumeRow {
            volume_id: 806,
            name: Some("Initial Name".into()),
            publisher: Some("Initial Publisher".into()),
            start_year: Some(2001),
            ..Default::default()
        }])
        .expect("seed volume");
    cache
        .put_issues(&[
            IssueSkeleton {
                issue_id: 92_643,
                volume_id: 806,
                issue_number: "1".into(),
                ..Default::default()
            },
            IssueSkeleton {
                issue_id: 173_407,
                volume_id: 806,
                issue_number: "4".into(),
                ..Default::default()
            },
        ])
        .expect("seed issues");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.cache-manager-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("Cache manager probe")
            .build();
        window.present();
        let starter: cr_ui::dialogs::cache_manager::UpdateStarter = Rc::new(|_, _, _| false);
        let related_starter: cr_ui::dialogs::cache_manager::RelatedStarter = Rc::new(|_, _| false);
        let handle = cr_ui::dialogs::cache_manager::show(
            &window,
            Arc::clone(&cache),
            starter,
            related_starter,
            false,
        );
        handle.set_series_id(806);
        handle.search();

        let watchdog = window.clone();
        glib::timeout_add_local(std::time::Duration::from_secs(10), move || {
            eprintln!("FAIL WATCHDOG: cache manager probe exceeded 10 seconds");
            watchdog.close();
            std::process::exit(2);
        });

        let app = app.clone();
        let cache = Arc::clone(&cache);
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            if handle.name() != "Initial Name" {
                return glib::ControlFlow::Continue;
            }
            let issues = handle.issue_text();
            assert!(issues.contains("92643\t1"));
            assert!(issues.contains("173407\t4"));
            handle.set_metadata("Edited Name", "Edited Publisher", "2002");
            handle.save();

            let app = app.clone();
            let cache = Arc::clone(&cache);
            let done_handle = handle.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                let stored = cache.volume(806).expect("read volume").expect("volume");
                if stored.name.as_deref() != Some("Edited Name") {
                    return glib::ControlFlow::Continue;
                }
                assert_eq!(stored.publisher.as_deref(), Some("Edited Publisher"));
                assert_eq!(stored.start_year, Some(2002));
                done_handle.close();
                eprintln!("PROBE PASS: cache manager search, issue list, and metadata save");
                app.quit();
                glib::ControlFlow::Break
            });
            glib::ControlFlow::Break
        });
    });
    app.run();
}
