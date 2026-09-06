//! Headless probe: the T13 small chrome dialogs. Gates: the Tasks
//! dialog opens through `win.tasks` (single instance — a second
//! dispatch does not duplicate), the Custom Zoom dialog applies the
//! spin value to the reader (the response path), Quick Rating
//! applies the rating to the library book and the action gates on a
//! selection, the About dialog opens with the `0.0.<commits>`
//! version scheme, and the auto Quick Review on a read unrated book
//! close (`OnBookClosing` parity).
//! Run: Xvfb + `cargo run -p cr-ui --example smalldialogs_probe` with
//! an isolated XDG (the probe seeds books into the DB it opens).
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::prelude::*;
use gtk4::{glib, Dialog, SpinButton};

fn find_toplevel(title: &str) -> Option<gtk4::Window> {
    gtk4::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::Window>().ok())
        .find(|w| w.title().as_deref().is_some_and(|t| t.starts_with(title)))
}

/// Full widget-tree walk (first_child/next_sibling — no container
/// type assumptions).
fn walk(widget: &gtk4::Widget, out: &mut Vec<gtk4::Widget>) {
    out.push(widget.clone());
    let mut child = widget.first_child();
    while let Some(c) = child {
        walk(&c, out);
        child = c.next_sibling();
    }
}

fn find_dialog_spin(dialog: &gtk4::Window) -> Option<SpinButton> {
    let mut widgets = Vec::new();
    if let Some(child) = dialog.child() {
        walk(&child, &mut widgets);
    }
    widgets
        .into_iter()
        .find_map(|w| w.downcast::<SpinButton>().ok())
}

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::theme::init();
    let src = "tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz";
    // The probe SEEDS books — refuse a real home (the T8 lesson).
    if !std::env::var("XDG_DATA_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false)
    {
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> (the probe seeds books into the DB it opens)");
        std::process::exit(1);
    }
    let work = std::path::Path::new("/tmp/opencode/smalldialogs");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();
    // A plain seed + a READ unrated seed (the auto-review gate).
    for (name, read) in [("probe a.cbz", false), ("probe read.cbz", true)] {
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
        if read {
            book.last_page_read = book.info.page_count - 1;
        }
        let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
        lib.database_mut().books.push(book);
        lib.save().unwrap();
    }
    cr_ui::library::initialize().expect("session init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.smalldialogs-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        // The probe keeps the shell alive (the thread-local lesson).
        std::mem::forget(shell.clone());

        // A. Quick Rating DISABLED without a selection; Tasks opens
        //    and a second dispatch keeps ONE instance.
        glib::timeout_add_local(std::time::Duration::from_millis(800), {
            let shell = shell.clone();
            move || {
                shell.navigator().select_list(
                    &cr_ui::library::comic_lists_snapshot()[0].base().id,
                );
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(1300), {
            let shell = shell.clone();
            move || {
                let rating_disabled = !shell.state_action_enabled("quick-rating");
                let _ = shell.state_dispatch("win.tasks");
                glib::timeout_add_local(std::time::Duration::from_millis(300), {
                    let shell = shell.clone();
                    move || {
                        let _ = shell.state_dispatch("win.tasks");
                        let visible = shell.state_tasks_window_visible();
                        println!(
                            "A rating-disabled={rating_disabled} tasks-visible={visible} (expect true/true — the single instance re-presents)"
                        );
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // B. Select a book, open the comic, Custom Zoom: the spin
        //    drives the reader zoom through the response path.
        glib::timeout_add_local(std::time::Duration::from_millis(2100), {
            let shell = shell.clone();
            move || {
                shell.state_select_first_book();
                shell.open_comic(std::path::Path::new(
                    "/tmp/opencode/smalldialogs/probe a.cbz",
                ));
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(2700), {
            let shell = shell.clone();
            move || {
                let _ = shell.state_dispatch("win.zoom-custom");
                glib::timeout_add_local(std::time::Duration::from_millis(300), {
                    let shell = shell.clone();
                    move || {
                        let dialog = find_toplevel("Custom Zoom");
                        let opened = dialog.is_some();
                        if let Some(d) = dialog {
                            if let Some(spin) = find_dialog_spin(&d) {
                                spin.set_value(250.0);
                            }
                            d.downcast::<Dialog>().unwrap().response(gtk4::ResponseType::Ok);
                        }
                        glib::timeout_add_local(std::time::Duration::from_millis(300), {
                            let shell = shell.clone();
                            move || {
                                println!(
                                    "B zoom-dialog={opened} zoom={:?} (expect true/Some(2.5))",
                                    shell.state_current_zoom()
                                );
                                glib::ControlFlow::Break
                            }
                        });
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // C. Quick Rating over the selection: the Scale drives the
        //    library book's rating on OK.
        glib::timeout_add_local(std::time::Duration::from_millis(3500), {
            let shell = shell.clone();
            move || {
                let _ = shell.state_dispatch("win.quick-rating");
                glib::timeout_add_local(std::time::Duration::from_millis(300), {
                    move || {
                        let dialog = find_toplevel("Quick Rating");
                        let opened = dialog.is_some();
                        let title = dialog
                            .as_ref()
                            .and_then(|d| d.title().map(|t| t.to_string()))
                            .unwrap_or_default();
                        if let Some(d) = dialog {
                            // The rating Scale: the only gtk4::Scale.
                            let mut widgets = Vec::new();
                            if let Some(child) = d.child() {
                                walk(&child, &mut widgets);
                            }
                            if let Some(scale) = widgets
                                .iter()
                                .find_map(|w| w.downcast_ref::<gtk4::Scale>().cloned())
                            {
                                scale.set_value(4.0);
                            }
                            d.downcast::<Dialog>()
                                .unwrap()
                                .response(gtk4::ResponseType::Ok);
                        }
                        glib::timeout_add_local(std::time::Duration::from_millis(300), {
                            move || {
                                let rating = cr_ui::library::session()
                                    .borrow()
                                    .find_book(
                                        "/tmp/opencode/smalldialogs/probe a.cbz",
                                    )
                                    .map(|b| b.rating);
                                println!(
                                    "C quick-rating-dialog={opened} title='{title}' rating={rating:?} (expect true/'Quick Rating - …'/Some(4.0))"
                                );
                                glib::ControlFlow::Break
                            }
                        });
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // D. About: the dialog opens; the version scheme holds.
        glib::timeout_add_local(std::time::Duration::from_millis(4400), {
            let shell = shell.clone();
            move || {
                let _ = shell.state_dispatch("win.about");
                glib::timeout_add_local(std::time::Duration::from_millis(300), {
                    move || {
                        let about = find_toplevel("About ComicRust");
                        println!(
                            "D about-opened={} version={} (expect true/0.0.N)",
                            about.is_some(),
                            cr_ui::dialogs::about::app_version(),
                        );
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // E. The auto Quick Review on close (OnBookClosing): the
        //    read unrated seed closes → the dialog opens; Cancel
        //    leaves the rating 0.
        glib::timeout_add_local(std::time::Duration::from_millis(5000), {
            let shell = shell.clone();
            move || {
                cr_ui::library::settings().borrow_mut().auto_show_quick_review = true;
                shell.open_comic(std::path::Path::new(
                    "/tmp/opencode/smalldialogs/probe read.cbz",
                ));
                glib::timeout_add_local(std::time::Duration::from_millis(500), {
                    let shell = shell.clone();
                    move || {
                        let _ = shell.state_dispatch("win.close");
                        glib::timeout_add_local(std::time::Duration::from_millis(400), {
                            move || {
                                let dialog = find_toplevel("Quick Rating");
                                let opened = dialog.is_some();
                                if let Some(d) = dialog {
                                    d.downcast::<Dialog>()
                                        .unwrap()
                                        .response(gtk4::ResponseType::Cancel);
                                }
                                glib::timeout_add_local(std::time::Duration::from_millis(300), {
                                    move || {
                                        let rating = cr_ui::library::session()
                                            .borrow()
                                            .find_book(
                                                "/tmp/opencode/smalldialogs/probe read.cbz",
                                            )
                                            .map(|b| b.rating);
                                        println!(
                                            "E auto-review-dialog={opened} rating-after-cancel={rating:?} (expect true/Some(0.0))"
                                        );
                                        glib::ControlFlow::Break
                                    }
                                });
                                glib::ControlFlow::Break
                            }
                        });
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_millis(6500), {
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
