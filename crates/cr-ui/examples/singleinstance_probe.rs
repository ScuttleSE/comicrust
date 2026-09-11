//! The T1 single-instance probe (`docs/archive/phases/phase-7.md`): proves
//! the GApplication unique-mode handoff — a second launch registers
//! as remote, forwards its argv to the primary's `command-line`
//! handler and exits. The primary records every `command-line`
//! delivery into a marker file and gates:
//!
//! - A: the primary's own boot arrives as `command-line` (FIRST) and
//!   carries NO argv[0] (the exe path must never become a "file").
//! - B: the handoff (SECOND) carries the client's files AND the
//!   `-p 7` switch, and `ExtendedSettings::from_argv` parses them.
//! - C: the client process exits promptly (no second window, no
//!   hang).
//!
//! Run under Xvfb: `GDK_BACKEND=x11 DISPLAY=:99 cargo run -p cr-ui
//! --example singleinstance_probe`.

use std::cell::Cell;
use std::path::PathBuf;

use gtk4::prelude::*;
use gtk4::{gio, glib, Application};

thread_local! {
    static COMMAND_SEEN: Cell<bool> = const { Cell::new(false) };
    /// The client process handle (a zombie's /proc entry exists —
    /// the exit check goes through try_wait on the kept handle).
    static CHILD: std::cell::RefCell<Option<std::process::Child>> =
        const { std::cell::RefCell::new(None) };
    static HANDOFF_AT: Cell<Option<std::time::Instant>> = const { Cell::new(None) };
    /// The handoff argv as delivered (the B gate parses it).
    static HANDOFF: std::cell::RefCell<Option<Vec<String>>> =
        const { std::cell::RefCell::new(None) };
}

fn marker_path() -> PathBuf {
    std::env::temp_dir().join("cr-singleinstance-probe.log")
}

fn append_marker(text: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(marker_path())
    {
        let _ = writeln!(f, "{text}");
    }
}

fn read_marker() -> String {
    std::fs::read_to_string(marker_path()).unwrap_or_default()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    gtk4::init().expect("GTK init");
    let app = Application::builder()
        .application_id("org.comicrust.probe.SingleInstance")
        .flags(gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    app.register(None::<&gio::Cancellable>).expect("register");
    let remote = app.is_remote();
    eprintln!("probe: registered remote={remote} argv={args:?}");
    if !remote {
        // Only the primary owns the marker (a client-side unlink
        // would wipe the log the primary still writes to).
        let _ = std::fs::remove_file(marker_path());
    }

    app.connect_command_line(|app, command_line| {
        let argv: Vec<String> = command_line
            .arguments()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let first = !COMMAND_SEEN.with(|c| c.get());
        COMMAND_SEEN.with(|c| c.set(true));
        if first {
            append_marker(&format!("first: {argv:?}"));
            // HANDLES_COMMAND_LINE replaces the ::activate boot — the
            // window and the client spawn happen here.
            let win = gtk4::ApplicationWindow::builder()
                .application(app)
                .title("si-probe")
                .build();
            win.present();
            append_marker("primary-activated");
            // Spawn THIS binary as the second instance (the C# second
            // launch): it registers, forwards, exits.
            let exe = std::env::current_exe().expect("exe");
            let child = std::process::Command::new(exe)
                .args(["--client", "/tmp/probe-si-file.cbz", "-p", "7"])
                .spawn()
                .expect("spawn client");
            let pid = child.id();
            CHILD.with(|c| *c.borrow_mut() = Some(child));
            append_marker(&format!("spawned pid={pid}"));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let app_poll = app.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                let log = read_marker();
                let grace_over = HANDOFF_AT
                    .with(|c| c.get())
                    .is_some_and(|t| t.elapsed() > std::time::Duration::from_millis(500));
                if log.contains("handoff:") && grace_over || std::time::Instant::now() > deadline {
                    app_poll.quit();
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
        } else {
            append_marker(&format!("handoff: {argv:?}"));
            HANDOFF_AT.with(|c| c.set(Some(std::time::Instant::now())));
            HANDOFF.with(|c| *c.borrow_mut() = Some(argv));
        }
        glib::ExitCode::SUCCESS
    });

    app.connect_activate(|_app| {
        // Never expected with HANDLES_COMMAND_LINE (the probe proves
        // the gio behavior); the marker records it if it ever fires.
        append_marker("activate-fired");
    });

    let _ = app.run();

    if remote {
        eprintln!("probe: client run() returned");
        append_marker("client-exited");
        return;
    }

    // ----- gates (the primary, after run) -----
    let log = read_marker();
    let mut failed = Vec::new();

    let (Some(first), Some(handoff)) = (line_after(&log, "first: "), line_after(&log, "handoff: "))
    else {
        eprintln!("probe FAIL: missing command-line deliveries\n{log}");
        std::process::exit(1);
    };

    // A: the primary's own boot delivery carries ONLY argv[0] (the
    // gio evidence: the program path rides both deliveries; the app
    // strips exactly that element). The raw argv[0] — current_exe()
    // may differ (cargo run passes a relative path). The delivery is
    // the debug list: exactly one element, no comma.
    let raw_argv0 = std::env::args().next().unwrap_or_default();
    if !first.contains(&raw_argv0) || first.contains(", ") {
        failed.push(format!("A: boot delivery is not argv[0]-only: {first}"));
    }
    // B: the handoff argv + the from_argv parse. The delivery
    // carries the client's argv[0] — the app strips element 0 before
    // the parse, so the parse input is the tail of the handoff.
    let handoff_argv = HANDOFF.with(|c| c.borrow().clone()).unwrap_or_default();
    let stripped = if handoff_argv.len() > 1 {
        &handoff_argv[1..]
    } else {
        &handoff_argv[..]
    };
    if !stripped.contains(&"/tmp/probe-si-file.cbz".to_string()) {
        failed.push(format!("B: handoff lost the file: {handoff}"));
    }
    let ext = cr_core::settings::ExtendedSettings::from_argv(stripped);
    if ext.files != vec!["/tmp/probe-si-file.cbz".to_string()] {
        failed.push(format!(
            "B: from_argv files={:?} (handoff={handoff})",
            ext.files
        ));
    }
    if ext.page != 7 {
        failed.push(format!(
            "B: from_argv page={} (handoff={handoff})",
            ext.page
        ));
    }
    // C: the client exited — through try_wait on the kept handle (a
    // zombie's /proc entry exists until the parent reaps).
    let child = CHILD.with(|c| c.borrow_mut().take());
    match child {
        Some(mut child) => {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut status = child.try_wait().ok().flatten();
            while status.is_none() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(100));
                status = child.try_wait().ok().flatten();
            }
            if status.is_none() {
                failed.push(format!("C: client pid {} still alive", child.id()));
            }
        }
        None => failed.push("C: no client handle recorded".to_string()),
    }
    if !log.contains("primary-activated") {
        failed.push("A: the primary never activated (no window)".to_string());
    }

    if failed.is_empty() {
        println!("singleinstance_probe PASS (first={first} handoff={handoff})");
    } else {
        for f in &failed {
            eprintln!("probe FAIL: {f}");
        }
        std::process::exit(1);
    }
}

/// The text after the FIRST `prefix:` line in the marker log.
fn line_after(log: &str, prefix: &str) -> Option<String> {
    log.lines()
        .find(|l| l.starts_with(prefix))
        .map(|l| l[prefix.len()..].to_string())
}
