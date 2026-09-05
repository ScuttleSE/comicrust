//! Headless probe: the T9 workspace tab strip (`MainView.tabStrip`).
//! Gates: open comics become comic tabs (Library | Pages | tabs | +),
//! the strip selection follows the workspace, a comic-tab click
//! shows the reader slot, a RE-click on the selected item toggles
//! the browser (the C# `tab_CaptionClick`), the current slot's tab
//! renders bold, the `+` adds an EMPTY slot (no QuickOpen — the
//! recorded deviation) and hides the Pages tab (no current book),
//! the close buttons close slots, and the last close lands on the
//! Library workspace with Pages hidden.
//! Run: Xvfb + `cargo run -p cr-ui --example tabstrip_probe` with an
//! isolated XDG (fresh DB → the default list tree).
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::glib;
use gtk4::prelude::*;

use cr_ui::browser::tabstrip::TabId;

fn main() {
    gtk4::init().expect("gtk init");
    let src = "tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz";
    let work = std::path::Path::new("/tmp/opencode/tabstrip");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();
    // Two seeds → two comic tabs.
    for name in ["probe a.cbz", "probe b.cbz"] {
        let comic = work.join(name);
        std::fs::copy(src, &comic).unwrap();
        let provider = cr_io::ComicProvider::open(&comic).unwrap();
        let mut book = ComicBook {
            id: CrGuid::new_random(),
            file_path: comic.to_string_lossy().into_owned(),
            added_time: CrDateTime::now(),
            ..Default::default()
        };
        book.info.page_count = provider.page_count() as i32;
        let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
        lib.database_mut().books.push(book);
        lib.save().unwrap();
    }
    cr_ui::library::initialize().expect("session init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.tabstrip-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        // The probe must keep the shell alive (the thread-local
        // lesson) — every action handler holds Weak<ShellState>.
        std::mem::forget(shell.clone());
        let strip = shell.tabstrip();

        // A. Startup: Library | (+) only — no comic tabs, no Pages
        //    (no book), Library selected, the browser not yet shown
        //    (QuickOpen startup page).
        glib::timeout_add_local(std::time::Duration::from_millis(800), {
            let shell = shell.clone();
            let strip = strip.clone();
            move || {
                println!(
                    "A page={:?} slots={:?} lib={} pages={} plus={} sel={:?} (expect quickopen/[]/true/false/true/Library)",
                    shell.state_visible_page(),
                    strip.comic_slots(),
                    strip.tab_visible(&TabId::Library),
                    strip.tab_visible(&TabId::Pages),
                    strip.tab_visible(&TabId::Plus),
                    strip.selected(),
                );
                glib::ControlFlow::Break
            }
        });

        // B. Open both comics → two comic tabs, Pages visible, the
        //    LAST tab selected, the reader workspace showing.
        glib::timeout_add_local(std::time::Duration::from_millis(1200), {
            let shell = shell.clone();
            let work = work.to_path_buf();
            move || {
                shell.open_comic(&work.join("probe a.cbz"));
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(1800), {
            let shell = shell.clone();
            let work = work.to_path_buf();
            move || {
                shell.open_comic(&work.join("probe b.cbz"));
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(2400), {
            let shell = shell.clone();
            let strip = strip.clone();
            move || {
                let slots = strip.comic_slots();
                let sel = strip.selected();
                let bold_last = slots.last().map(|s| strip.tab_bold(*s)).unwrap_or(false);
                println!(
                    "B page={:?} slots={slots:?} pages-tab={} sel={sel:?} bold-last={bold_last} (expect reader/2 slots/Pages true/Comic(_)/true)",
                    shell.state_visible_page(),
                    strip.tab_visible(&TabId::Pages),
                );
                glib::ControlFlow::Break
            }
        });

        // C. Click the Library tab → the browser workspace; the
        //    current slot's tab stays bold behind it.
        glib::timeout_add_local(std::time::Duration::from_millis(2800), {
            let shell = shell.clone();
            let strip = strip.clone();
            move || {
                strip.click(&TabId::Library);
                let current = shell.state_reader_slot();
                let bold = current.map(|s| strip.tab_bold(s)).unwrap_or(false);
                println!(
                    "C page={:?} sel={:?} bold-current={bold} (expect browser/Library/true)",
                    shell.state_visible_page(),
                    strip.selected(),
                );
                glib::ControlFlow::Break
            }
        });

        // D. Click the FIRST comic tab → its slot shows (slot 0).
        glib::timeout_add_local(std::time::Duration::from_millis(3200), {
            let shell = shell.clone();
            let strip = strip.clone();
            move || {
                let first = strip.comic_slots().first().copied();
                if let Some(first) = first {
                    strip.click(&TabId::Comic(first));
                    println!(
                        "D page={:?} slot={:?} sel={:?} (expect reader/Some({first})/Comic({first}))",
                        shell.state_visible_page(),
                        shell.state_reader_slot(),
                        strip.selected(),
                    );
                }
                glib::ControlFlow::Break
            }
        });

        // E. RE-click the selected comic tab → ToggleBrowser (the C#
        //    `tab_CaptionClick`): the browser shows again.
        glib::timeout_add_local(std::time::Duration::from_millis(3600), {
            let shell = shell.clone();
            let strip = strip.clone();
            move || {
                let first = strip.comic_slots().first().copied();
                if let Some(first) = first {
                    strip.click(&TabId::Comic(first));
                    println!(
                        "E page={:?} sel={:?} (expect browser/Library)",
                        shell.state_visible_page(),
                        strip.selected(),
                    );
                }
                glib::ControlFlow::Break
            }
        });

        // F. The Pages workspace through the strip.
        glib::timeout_add_local(std::time::Duration::from_millis(4000), {
            let shell = shell.clone();
            let strip = strip.clone();
            move || {
                strip.click(&TabId::Pages);
                println!(
                    "F page={:?} sel={:?} (expect pages/Pages)",
                    shell.state_visible_page(),
                    strip.selected(),
                );
                glib::ControlFlow::Break
            }
        });

        // G. `+` → a new EMPTY slot: the reader shows blank, the
        //    Pages tab hides (no CURRENT book — the C# rule), three
        //    comic tabs.
        glib::timeout_add_local(std::time::Duration::from_millis(4400), {
            let shell = shell.clone();
            let strip = strip.clone();
            move || {
                strip.click(&TabId::Plus);
                let slots = strip.comic_slots();
                println!(
                    "G page={:?} slots={slots:?} pages-tab={} captions-empty={} (expect reader/3 slots/Pages false/true)",
                    shell.state_visible_page(),
                    strip.tab_visible(&TabId::Pages),
                    slots
                        .last()
                        .map(|s| strip.comic_caption(*s))
                        .unwrap_or(Some("?".into()))
                        .map(|c| c.is_empty())
                        .unwrap_or(false),
                );
                glib::ControlFlow::Break
            }
        });

        // H. Close the empty slot through its close button.
        glib::timeout_add_local(std::time::Duration::from_millis(4800), {
            let shell = shell.clone();
            let strip = strip.clone();
            move || {
                let last = strip.comic_slots().last().copied();
                if let Some(last) = last {
                    strip.click_close(last);
                }
                println!(
                    "H slots={:?} page={:?} pages-tab={} (expect 2 slots/reader/Pages true — the neighbor comic slot shows)",
                    strip.comic_slots(),
                    shell.state_visible_page(),
                    strip.tab_visible(&TabId::Pages),
                );
                glib::ControlFlow::Break
            }
        });

        // I. Close ALL → the Library workspace (the C# `Close` →
        //    `ShowLibrary`), no comic tabs, Pages hidden.
        glib::timeout_add_local(std::time::Duration::from_millis(5200), {
            let shell = shell.clone();
            let strip = strip.clone();
            move || {
                shell.state_dispatch("win.close-all");
                println!(
                    "I page={:?} sel={:?} slots={:?} pages-tab={} (expect browser/Library/[]/false)",
                    shell.state_visible_page(),
                    strip.selected(),
                    strip.comic_slots(),
                    strip.tab_visible(&TabId::Pages),
                );
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_millis(5600), {
            let app = app.clone();
            move || {
                println!("PROBE COMPLETE");
                app.quit();
                glib::ControlFlow::Break
            }
        });
    });
    app.run();
}
