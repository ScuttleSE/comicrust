//! The export post-export surgery (`QueueManager.ExportComic`
//! QueueManager.cs:455-508 port): replace-source re-link,
//! delete-original, add-to-library, and the dirty-flag rule. The
//! trash step is injected (a fake that moves the file aside) so the
//! file side is testable on tmpfs, where `gio trash` refuses.
//! Runs against an isolated XDG pair — see the probe rules in
//! AGENTS.md.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::CrGuid;
use cr_io::export::{ExportSetting, ExportTarget};

fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "comicrust-surgery-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn build_zip(path: &Path, series: &str) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = Default::default();
    for (name, data) in [("cover.jpg", "cover".as_bytes()), ("page1.jpg", b"one")] {
        zip.start_file(name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
    let _ = series;
}

fn comic_in(dir: &Path, stem: &str, series: &str) -> ComicBook {
    let path = dir.join(format!("{stem}.cbz"));
    build_zip(&path, series);
    ComicBook {
        id: CrGuid::new_random(),
        file_path: path.to_string_lossy().into_owned(),
        info: cr_core::model::comic_info::ComicInfo {
            series: series.into(),
            page_count: 2,
            ..Default::default()
        },
        ..ComicBook::default()
    }
}

#[test]
fn export_surgery_flows() {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let _ = seq;

    // Isolated XDG (the probe rule), then the session boot.
    let data = fresh_dir("xdg-data");
    let config = fresh_dir("xdg-config");
    std::env::set_var("XDG_DATA_HOME", &data);
    std::env::set_var("XDG_CONFIG_HOME", &config);
    cr_ui::library::initialize().unwrap();

    let dir = fresh_dir("books");
    let out_dir = fresh_dir("out");
    let trash_dir = fresh_dir("trash");
    let fake_trash = |file: &str| -> bool {
        if file.is_empty() || !Path::new(file).is_file() {
            return false;
        }
        let name = Path::new(file).file_name().unwrap().to_owned();
        std::fs::rename(file, trash_dir.join(name)).is_ok()
    };

    // Case 1 — replace source: the book re-points, the original is
    // trashed, the dirty flag clears, the reading state survives.
    let a = comic_in(&dir, "Alpha", "Replace Series");
    let mut a = a;
    a.current_page = 1;
    a.set_current_page(1);
    a.comic_info_is_dirty = true;
    let a_id = a.id;
    cr_ui::library::session()
        .borrow_mut()
        .database_mut()
        .books
        .push(a.clone());
    let setting = ExportSetting {
        target: ExportTarget::ReplaceSource,
        format_id: 3,
        ..ExportSetting::default()
    };
    let out = out_dir.join("Alpha.cbt");
    std::fs::copy(dir.join("Alpha.cbz"), &out).unwrap();
    cr_ui::library::export_post_process_with(&setting, &[a], &out, &fake_trash).unwrap();
    assert!(out.exists());
    assert!(!dir.join("Alpha.cbz").exists(), "the source is trashed");
    assert!(trash_dir.join("Alpha.cbz").exists());
    {
        let lib = cr_ui::library::session();
        let l = lib.borrow();
        let book = l
            .database()
            .books
            .iter()
            .find(|b| b.id == a_id)
            .expect("the book stays in the database");
        assert_eq!(book.file_path, out.to_string_lossy());
        assert!(!book.comic_info_is_dirty, "the dirty flag cleared");
        assert_eq!(book.info.series, "Replace Series");
        assert_eq!(book.current_page, 1, "the reading position survives");
    }

    // Case 2 — delete original: the source is trashed and the book
    // leaves the database; no new book is added.
    let b = comic_in(&dir, "Beta", "Delete Series");
    let b_id = b.id;
    cr_ui::library::session()
        .borrow_mut()
        .database_mut()
        .books
        .push(b.clone());
    let setting = ExportSetting {
        target: ExportTarget::NewFolder,
        target_folder: out_dir.to_string_lossy().into_owned(),
        delete_original: true,
        ..ExportSetting::default()
    };
    let out_b = out_dir.join("Beta.cbz");
    std::fs::copy(dir.join("Beta.cbz"), &out_b).unwrap();
    cr_ui::library::export_post_process_with(&setting, &[b], &out_b, &fake_trash).unwrap();
    assert!(out_b.exists());
    assert!(!dir.join("Beta.cbz").exists());
    {
        let lib = cr_ui::library::session();
        let l = lib.borrow();
        assert!(
            !l.database().books.iter().any(|b| b.id == b_id),
            "the book left the database"
        );
    }

    // Case 3 — add to library: the source book stays, a fresh book
    // joins for the output.
    let c = comic_in(&dir, "Gamma", "Add Series");
    let c_id = c.id;
    cr_ui::library::session()
        .borrow_mut()
        .database_mut()
        .books
        .push(c.clone());
    let setting = ExportSetting {
        target: ExportTarget::NewFolder,
        target_folder: out_dir.to_string_lossy().into_owned(),
        add_to_library: true,
        ..ExportSetting::default()
    };
    let out_c = out_dir.join("Gamma.cbz");
    std::fs::copy(dir.join("Gamma.cbz"), &out_c).unwrap();
    cr_ui::library::export_post_process_with(&setting, &[c], &out_c, &fake_trash).unwrap();
    assert!(out_c.exists());
    assert!(dir.join("Gamma.cbz").exists(), "the source is kept");
    {
        let lib = cr_ui::library::session();
        let l = lib.borrow();
        assert!(l.database().books.iter().any(|b| b.id == c_id));
        let added = l
            .database()
            .books
            .iter()
            .find(|b| b.file_path == out_c.to_string_lossy())
            .expect("a book was added for the output");
        assert_eq!(added.info.series, "Add Series");
        assert_ne!(added.id, c_id, "the added book is a new one");
    }
}
