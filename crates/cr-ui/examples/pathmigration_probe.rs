//! Headless probe for the Phase 8 T11 Windows-path migration. A
//! seeded ComicDb carries Windows paths in the three families; a
//! mirrored Linux tree stands in for the user's files. Gates: the
//! detection + collapse (A), the boot-prompt helper opens the dialog
//! (B), the row previews live when the entries fill (C), OK applies —
//! found books re-home with a file-info refresh, a missing book goes
//! fileless, the watch folders + blacklist rewrite, the DB marks
//! dirty (D) — and with no Windows paths left the `win.migrate-paths`
//! action disables (E).
//!
//! Run: Xvfb + `cargo run -p cr-ui --example pathmigration_probe`
//! with an isolated XDG pair (the probe seeds the DB it opens).
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Dialog, Entry, Label};

use cr_core::database::comic_database::{save, ComicDatabase};
use cr_core::database::list_items::WatchFolder;
use cr_core::model::comic_book::ComicBook;

const MIRROR: &str = "/tmp/opencode/pathmigration/mirror";

fn find_toplevel(title: &str) -> Option<gtk4::Window> {
    gtk4::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::Window>().ok())
        .find(|w| w.title().as_deref().is_some_and(|t| t.starts_with(title)))
}

fn walk(widget: &gtk4::Widget, out: &mut Vec<gtk4::Widget>) {
    out.push(widget.clone());
    let mut child = widget.first_child();
    while let Some(c) = child {
        walk(&c, out);
        child = c.next_sibling();
    }
}

fn seed_db() -> ComicDatabase {
    let mut db = ComicDatabase {
        id: cr_core::xml::scalar::CrGuid::new_random(),
        ..Default::default()
    };
    for p in [
        "C:\\Comics\\Batman 001.cbz",
        "C:\\Comics\\Batman 002.cbz",
        "C:\\Comics\\Gone 001.cbz",
        "C:\\Other\\a.cbz",
    ] {
        db.books.push(ComicBook {
            file_path: p.into(),
            ..Default::default()
        });
    }
    db.watch_folders = vec![
        WatchFolder {
            folder: "C:\\Comics".into(),
            watch: true,
        },
        WatchFolder {
            folder: "C:\\Other".into(),
            watch: false,
        },
    ];
    db.black_list = vec!["C:\\Comics\\Batman 001.cbz".into()];
    db
}

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::theme::init();
    if !std::env::var("XDG_DATA_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false)
        || !std::env::var("XDG_CONFIG_HOME")
            .map(|v| v.contains("/tmp/opencode"))
            .unwrap_or(false)
    {
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> AND XDG_CONFIG_HOME=/tmp/opencode/<dir> (the probe seeds a database and can write comicrust.toml)");
        std::process::exit(1);
    }
    let work = std::path::Path::new("/tmp/opencode/pathmigration");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();

    // The mirrored Linux tree the roots map onto: the found books +
    // the blacklist file exist, `Gone 001` does not. The mapping
    // strips the Windows root (`C:\Comics` → the target), so the
    // files sit DIRECTLY in the mirror (the Comics contents land
    // there; the Other contents too).
    std::fs::create_dir_all(MIRROR).unwrap();
    for f in ["Batman 001.cbz", "Batman 002.cbz", "a.cbz"] {
        std::fs::write(format!("{MIRROR}/{f}"), b"x").unwrap();
    }

    // Seed the database BEFORE the session opens (the boot prompt
    // reads it at init).
    let xdg = std::env::var("XDG_DATA_HOME").unwrap();
    let db_dir = std::path::Path::new(&xdg).join("comicrust").join("ComicDb");
    std::fs::create_dir_all(&db_dir).unwrap();
    save(&seed_db(), &db_dir.join("ComicDb.xml")).unwrap();

    cr_ui::library::initialize().expect("session init");

    // A. The detection + collapse over the three families.
    let roots = cr_ui::library::windows_path_roots();
    for r in &roots {
        println!(
            "A root {} books={} watch={} black={}",
            r.windows_root, r.books, r.watch_folders, r.black_list
        );
    }
    assert_eq!(roots.len(), 2, "A: C:\\Comics + C:\\Other collapse");
    assert_eq!(roots[0].windows_root, "C:\\Comics");
    assert_eq!(roots[0].books, 3);
    assert_eq!(roots[0].watch_folders, 1);
    assert_eq!(roots[0].black_list, 1);
    assert_eq!(roots[1].windows_root, "C:\\Other");
    assert_eq!(roots[1].books, 1);
    assert!(cr_ui::library::has_windows_paths(), "A: detection true");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.pathmigration-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        std::mem::forget(shell.clone());

        // B. The boot-prompt helper (the boot branch shares it) opens
        //    the dialog over the shell.
        cr_ui::app::maybe_prompt_windows_path_migration(&shell);
        let dlg = find_toplevel("Migrate Windows Paths");
        let dlg = dlg.expect("B: the migration dialog opened at boot");
        let dialog = dlg.downcast::<Dialog>().unwrap();
        println!("B dialog mapped={}", dialog.is_mapped());
        let as_widget: gtk4::Widget = dialog.clone().upcast();

        // C. Fill the two target entries (row order = the sorted
        //    roots: C:\Comics, C:\Other) — the live preview labels
        //    update through the changed handlers.
        let entries: Vec<Entry> = {
            let mut all = Vec::new();
            walk(&as_widget, &mut all);
            all.into_iter()
                .filter_map(|w| w.downcast::<Entry>().ok())
                .collect()
        };
        assert_eq!(entries.len(), 2, "C: one entry per root");
        entries[0].set_text(MIRROR);
        entries[1].set_text(MIRROR);
        let labels: Vec<Label> = {
            let mut all = Vec::new();
            walk(&as_widget, &mut all);
            all.into_iter()
                .filter_map(|w| w.downcast::<Label>().ok())
                .collect()
        };
        let previews: Vec<String> = labels
            .iter()
            .map(|l| l.text().to_string())
            .filter(|t| t.contains("found"))
            .collect();
        for p in &previews {
            println!("C preview: {p}");
        }
        assert!(
            previews
                .iter()
                .any(|t| t.starts_with("2 of 3 found; 1 not found")),
            "C: the Comics root preview (2 found, 1 fileless)"
        );
        assert!(
            previews.iter().any(|t| t.starts_with("1 of 1 found")),
            "C: the Other root preview"
        );

        // D. OK applies through the real response path.
        dialog.response(gtk4::ResponseType::Ok);

        let app_c = app.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(900), {
            let shell = shell.clone();
            move || {
                let lib = cr_ui::library::session();
                let l = lib.borrow();
                let mut paths: Vec<String> = l
                    .database()
                    .books
                    .iter()
                    .map(|b| b.file_path.clone())
                    .collect();
                paths.sort();
                println!("D books: {paths:?}");
                println!(
                    "D watches: {:?} black: {:?} dirty={}",
                    l.database()
                        .watch_folders
                        .iter()
                        .map(|w| w.folder.clone())
                        .collect::<Vec<_>>(),
                    l.database().black_list,
                    l.is_dirty()
                );
                assert_eq!(
                    paths,
                    vec![
                        String::new(), // Gone 001 → fileless (sorts first)
                        format!("{MIRROR}/Batman 001.cbz"),
                        format!("{MIRROR}/Batman 002.cbz"),
                        format!("{MIRROR}/a.cbz"),
                    ],
                    "D: the books re-home under the mirror (Gone → fileless)"
                );
                let mapped = l.database().books[2].clone();
                assert!(
                    mapped.file_path.is_empty() && !mapped.file_is_missing,
                    "D: the not-found book is fileless with a clean missing flag"
                );
                let refreshed = l.database().books[0].clone();
                assert!(
                    refreshed.file_size == 1 && !refreshed.file_is_missing,
                    "D: the found book refreshed its file info (size 1)"
                );
                assert_eq!(
                    l.database().watch_folders[0].folder,
                    MIRROR,
                    "D: the Comics watch folder rewrote (C:\\Comics → the target)"
                );
                assert_eq!(
                    l.database().watch_folders[1].folder,
                    MIRROR,
                    "D: the Other watch folder rewrote"
                );
                assert_eq!(
                    l.database().black_list[0],
                    format!("{MIRROR}/Batman 001.cbz"),
                    "D: the blacklist rewrote"
                );
                assert!(l.is_dirty(), "D: the database marked dirty");
                drop(l);

                // E. No Windows paths remain: the action disables.
                assert!(
                    !cr_ui::library::has_windows_paths(),
                    "E: no Windows paths remain"
                );
                assert!(
                    !shell.state_action_enabled("migrate-paths"),
                    "E: win.migrate-paths disabled after the apply"
                );
                println!("ALL GATES PASSED");
                app_c.quit();
                glib::ControlFlow::Break
            }
        });
    });

    app.run();
}
