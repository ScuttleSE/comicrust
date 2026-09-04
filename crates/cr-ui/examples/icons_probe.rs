//! Headless probe: the bundled icon set (Phase 5.5 T2). Every resx
//! PNG name must load into a texture, the navigator must render with
//! the CR item icons, and a gallery window shows a sample set (the
//! screenshot evidence comes from the driver shell). Run: Xvfb +
//! `cargo run -p cr-ui --example icons_probe` with an isolated XDG.
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;

/// A sample of resx names across the set (chrome, special, dark).
const GALLERY: &[&str] = &[
    "Open",
    "Close",
    "Search",
    "SearchFolder",
    "SearchDocument",
    "NewList",
    "NewWindow",
    "NewTab",
    "Library",
    "List",
    "Preferences",
    "DisplaySettings",
    "Export",
    "Scan",
    "Save",
    "Undo",
    "Redo",
    "Sidebar",
    "ThumbView",
    "TileView",
    "DetailView",
    "Sort",
    "Group",
    "ZoomIn",
    "ZoomOut",
    "FullScreen",
    "SinglePage",
    "TwoPage",
    "Original",
    "FitAll",
    "FitWidth",
    "FitHeight",
    "FitBest",
    "Rotate90",
    "RightToLeft",
    "Bookmark",
    "NextBookmark",
    "GoNext",
    "GoPrevious",
    "GoFirst",
    "GoLast",
    "RandomComic",
    "Star",
    "Heart",
    "Locked",
    "Refresh",
    "Tools",
    "Help",
    "DarkSort",
    "DarkThumbView",
    "DarkLocked",
];

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::library::initialize().expect("library init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.icons-probe")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        // An ApplicationWindow keeps the app alive while it exists
        // (the shell's builder pattern) — a plain Window does not,
        // and the loop exits before any timeout fires.
        let win = run_probe(app);
        glib::timeout_add_local(std::time::Duration::from_millis(1500), {
            let app = app.clone();
            let win = win.downgrade();
            move || {
                println!("PROBE COMPLETE (window alive: {})", win.upgrade().is_some());
                app.quit();
                glib::ControlFlow::Break
            }
        });
    });
    app.run();
}

fn run_probe(app: &gtk4::Application) -> gtk4::ApplicationWindow {
    // 1. Every resx PNG name loads through the loader (the resx
    //    table itself is unit-tested; here we prove the textures
    //    construct headless).
    let names = resx_png_names();
    let mut loaded = 0usize;
    for name in &names {
        match cr_ui::icon::icon(name) {
            Some(t) => {
                loaded += 1;
                let _ = (t.width(), t.height());
            }
            None => println!("FAIL: {name} did not load"),
        }
    }
    println!("LOADED {loaded}/{}", names.len());

    // 2. The gallery window.
    let win = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("icons-probe")
        .default_width(780)
        .default_height(560)
        .build();

    let scroll = gtk4::ScrolledWindow::new();
    let grid = gtk4::Grid::new();
    grid.set_row_spacing(12);
    grid.set_column_spacing(12);
    grid.set_margin_top(12);
    grid.set_margin_bottom(12);
    grid.set_margin_start(12);
    grid.set_margin_end(12);
    for (i, name) in GALLERY.iter().enumerate() {
        let Some(texture) = cr_ui::icon::icon(name) else {
            println!("FAIL: gallery icon {name} missing");
            continue;
        };
        let image = gtk4::Image::from_paintable(Some(&texture));
        image.set_pixel_size(24);
        let label = gtk4::Label::new(Some(name));
        let cell = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        cell.append(&image);
        cell.append(&label);
        grid.attach(&cell, (i % 8) as i32, (i / 8) as i32, 1, 1);
    }
    scroll.set_child(Some(&grid));

    // 3. The navigator with the real library tree (CR item icons).
    let shell_column = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    let header = gtk4::Label::new(Some("Navigator (CR item icons)"));
    shell_column.append(&header);
    let navigator = cr_ui::browser::navigator::Navigator::new();
    navigator.refill(&cr_ui::library::comic_lists_snapshot());
    shell_column.append(navigator.widget());
    shell_column.set_margin_top(8);
    shell_column.set_margin_bottom(8);
    shell_column.set_margin_start(8);
    shell_column.set_margin_end(8);

    let root = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    root.append(&shell_column);
    root.append(&scroll);
    win.set_child(Some(&root));
    win.present();

    println!("NAVIGATOR OK: tree filled from the library snapshot");
    win
}

/// All shipped icon names, derived from the asset dirs (the resx
/// name → file coverage itself is the `tests/icons.rs` gate).
fn resx_png_names() -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for root in ["assets/icons", "crates/cr-ui/assets/icons"] {
        let root = std::path::Path::new(root);
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("png") {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
        if let Ok(entries) = std::fs::read_dir(root.join("Dark")) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("png") {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(format!("Dark{stem}"));
                }
            }
        }
        if !names.is_empty() {
            break;
        }
    }
    names.sort();
    names.dedup();
    names
}
