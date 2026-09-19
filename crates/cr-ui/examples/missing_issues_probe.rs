//! Release probe for the Missing Issues gap view (Phase 19, ADR-059).
//!
//! Gates: the navigator entry forces Detail mode with no Cover column
//! and exactly the five report columns; a whole-library refresh shows
//! every series' gaps; scoping to a smart list narrows the row set to
//! that series' gaps; and no book is ever added to the library.
//!
//! Run under Xvfb with `XDG_DATA_HOME` set to an isolated directory
//! below `/tmp/opencode` (the probe seeds books, a smart list, and the
//! Comic Vine cache into that session).

use cr_core::database::list_items::{
    ComicBookMatcher, ComicListItem, ListItemBase, SmartListItem, ValueMatcher,
};
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use cr_scrape::cache::{CvCache, IssueSkeleton, SqliteCache};
use gtk4::glib;
use gtk4::prelude::*;

const ALPHA_VOLUME_ID: i64 = 501;
const BETA_VOLUME_ID: i64 = 502;

fn series_book(series: &str, volume_id: i64, number: &str) -> ComicBook {
    let mut book = ComicBook {
        id: CrGuid::new_random(),
        file_path: format!("/tmp/opencode/missing-issues/{series}-{number}.cbz"),
        added_time: CrDateTime::now(),
        ..Default::default()
    };
    book.info.series = series.to_string();
    book.info.volume = 2020;
    book.info.number = number.to_string();
    cr_scrape::bookdata::set_custom_value(&mut book, "comicvine_volume", &volume_id.to_string());
    book
}

fn issue(volume_id: i64, number: &str) -> IssueSkeleton {
    IssueSkeleton {
        issue_id: volume_id * 1000 + number.parse::<i64>().unwrap_or(0),
        volume_id,
        issue_number: number.to_string(),
        ..Default::default()
    }
}

type Shell = cr_ui::browser::shell::BrowserShell;

/// Waits for the grid's book count to reach `expected` (polling every
/// 50 ms, up to 100 ticks), then runs `then`. Polling the actual UI
/// effect — not the library-level `missing_issues_refresh_active()`
/// flag — sidesteps the shell's own 200 ms poll lag (`shell.rs`'s
/// Refresh handler applies a landed pass to the ItemView on its own
/// timer, which can still be pending after the worker-level flag
/// already reads settled).
fn wait_for_count(shell: Shell, expected: usize, then: impl FnOnce(Shell) + 'static, ticks: u32) {
    if shell.state_grid_book_count() == expected {
        then(shell);
        return;
    }
    if ticks >= 100 {
        eprintln!(
            "FAIL WATCHDOG: grid book count never reached {expected} (stayed at {}) after 100 ticks",
            shell.state_grid_book_count()
        );
        std::process::exit(2);
    }
    let mut then = Some(then);
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        wait_for_count(shell.clone(), expected, then.take().unwrap(), ticks + 1);
        glib::ControlFlow::Break
    });
}

fn main() {
    if !std::env::var("XDG_DATA_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false)
    {
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> (the probe seeds the library and cache it opens)");
        std::process::exit(1);
    }

    // The cache skeleton (ADR-037/038): Alpha has issues 1-3, Beta has
    // issues 1-2. Seeded before `library::initialize()` so the app's
    // lazily-opened `cv_cache()` reads the same file.
    let cache = SqliteCache::open(&cr_scrape::cache::default_cache_path()).expect("open cv cache");
    cache
        .put_issues(&[
            issue(ALPHA_VOLUME_ID, "1"),
            issue(ALPHA_VOLUME_ID, "2"),
            issue(ALPHA_VOLUME_ID, "3"),
            issue(BETA_VOLUME_ID, "1"),
            issue(BETA_VOLUME_ID, "2"),
        ])
        .expect("seed issue skeletons");
    drop(cache);

    // The library: Alpha owns #1 only (missing #2, #3); Beta owns #1
    // only (missing #2). A smart list narrows the scope to Alpha.
    let alpha_smart_list_id = CrGuid::new_random();
    let (mut lib, _) = cr_engine::library::Library::open_at_default_location().expect("open lib");
    lib.database_mut()
        .books
        .push(series_book("Alpha", ALPHA_VOLUME_ID, "1"));
    lib.database_mut()
        .books
        .push(series_book("Beta", BETA_VOLUME_ID, "1"));
    let starting_book_count = lib.database().books.len();
    lib.database_mut()
        .comic_lists
        .push(ComicListItem::Smart(SmartListItem {
            base: ListItemBase {
                id: alpha_smart_list_id,
                name: Some("Alpha Only".into()),
                ..Default::default()
            },
            matchers: vec![ComicBookMatcher::Value(ValueMatcher {
                type_name: "ComicBookSeriesMatcher".into(),
                match_value: "Alpha".into(),
                match_operator: 0,
                ..Default::default()
            })],
            ..Default::default()
        }));
    lib.save().expect("save seeded library");

    gtk4::init().expect("gtk init");
    cr_ui::theme::init();
    cr_ui::library::initialize().expect("session init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.missing-issues-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        std::mem::forget(shell.clone());

        let watchdog = window.clone();
        glib::timeout_add_local(std::time::Duration::from_secs(45), move || {
            eprintln!("FAIL WATCHDOG: Missing Issues probe exceeded 45 seconds");
            watchdog.close();
            std::process::exit(2);
        });

        shell.state_select_list(&cr_ui::browser::navigator::missing_issues_id());

        // The navigator selection debounces 200 ms (`SELECT_DEBOUNCE_MS`)
        // before the shell actually evaluates the new list and applies
        // its forced view config.
        let shell = shell.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(400), move || {
            gate_a(shell.clone(), starting_book_count, alpha_smart_list_id);
            glib::ControlFlow::Break
        });
    });
    app.run();
}

fn gate_a(shell: Shell, starting_book_count: usize, alpha_smart_list_id: CrGuid) {
    if !shell.state_missing_issues_bar_visible() {
        eprintln!("GATE A FAILED: the scope/Refresh bar did not show for the Missing Issues node");
        std::process::exit(1);
    }
    if shell.state_grid_mode() != "detail" {
        eprintln!("GATE A FAILED: Missing Issues did not force Detail mode");
        std::process::exit(1);
    }
    let columns = shell.state_detail_columns();
    let visible: Vec<&str> = columns
        .iter()
        .filter(|(_, _, visible)| *visible)
        .map(|(_, name, _)| name.as_str())
        .collect();
    let mut sorted_visible = visible.clone();
    sorted_visible.sort_unstable();
    let mut expected = vec!["Series", "Number", "Title", "Year", "Comic Vine Issue Id"];
    expected.sort_unstable();
    if sorted_visible != expected {
        eprintln!("GATE A FAILED: visible columns were {visible:?}, expected exactly {expected:?}");
        std::process::exit(1);
    }
    println!("GATE A OK: forced Detail mode, no Cover, exactly the five report columns");

    shell.state_missing_issues_refresh();
    wait_for_count(
        shell,
        3,
        move |shell| gate_b(shell, starting_book_count, alpha_smart_list_id),
        0,
    );
}

fn gate_b(shell: Shell, starting_book_count: usize, alpha_smart_list_id: CrGuid) {
    let lib_count = cr_ui::library::session().borrow().database().books.len();
    if lib_count != starting_book_count {
        eprintln!(
            "GATE B FAILED: library book count changed from {starting_book_count} to {lib_count}"
        );
        std::process::exit(1);
    }
    println!("GATE B OK: whole-library refresh shows all 3 gap rows, no book created");

    shell.state_missing_issues_set_scope(Some(alpha_smart_list_id));
    shell.state_missing_issues_refresh();
    wait_for_count(shell, 2, move |shell| gate_c(shell, starting_book_count), 0);
}

fn gate_c(shell: Shell, starting_book_count: usize) {
    let rows = cr_ui::library::missing_issues_snapshot();
    if !rows.iter().all(|b| b.info.series == "Alpha") {
        eprintln!("GATE C FAILED: a Beta row leaked into the Alpha-only scope");
        std::process::exit(1);
    }
    let lib_count = cr_ui::library::session().borrow().database().books.len();
    if lib_count != starting_book_count {
        eprintln!(
            "GATE C FAILED: library book count changed from {starting_book_count} to {lib_count}"
        );
        std::process::exit(1);
    }
    let _ = &shell;
    println!("GATE C OK: smart-list scope narrows to 2 Alpha-only gap rows, no book created");
    println!("MISSING ISSUES PROBE DONE");
    std::process::exit(0);
}
