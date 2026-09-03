//! Port of the `DatabaseManager` backup pieces that Phase 0 did not
//! cover: the `.crplugin`-style zip backup creation and restore flow
//! (`ComicDatabase.Backup` / `ComicDatabase.RestoreBackup`), and the
//! save-then-backup flow of `DatabaseManager.BackupTo`.
//!
//! Backup container (a plain zip — the same container the C# uses):
//!
//! - Zip comment `ComicRack Backup`.
//! - One entry named after the database file (`ComicDb.xml`) with the
//!   full database XML.
//! - One entry per custom thumbnail under `Thumbnails/<file name>`.
//!
//! Restore extracts `ComicDb.xml` into the `.restore` target path and
//! the `Thumbnails/` entries into the custom thumbnails folder; the
//! normal open chain picks the `.restore` file up on the next start
//! (cr-core `open_with_fallback`).

use std::io::Read;
use std::path::Path;

use cr_core::database::comic_database::{open_with_fallback, save, DbError, OpenStatus};
use zip::write::SimpleFileOptions;
use zip::ZipArchive;
use zip::ZipWriter;

fn io_err(e: DbError) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

/// `ComicDatabase.BackupDatabaseName`.
pub const BACKUP_DATABASE_NAME: &str = "ComicDb.xml";
/// The folder prefix for custom thumbnails inside the backup.
pub const THUMBNAILS_BACKUP_FOLDER: &str = "Thumbnails";
/// The zip comment the C# writer sets.
pub const BACKUP_COMMENT: &str = "ComicRack Backup";

/// `DatabaseManager.BackupTo`: save the database XML to
/// `database_xml_path`, then create the zip backup at `backup_file`
/// with the custom thumbnails of `thumbnails_dir` (skipped when the
/// folder does not exist).
pub fn backup_to(
    db: &cr_core::database::comic_database::ComicDatabase,
    backup_file: &Path,
    database_xml_path: &Path,
    thumbnails_dir: Option<&Path>,
) -> std::io::Result<()> {
    // Save the database normally first.
    save(db, database_xml_path).map_err(io_err)?;
    let file = std::fs::File::create(backup_file)?;
    let mut zip = ZipWriter::new(file);
    zip.set_comment(BACKUP_COMMENT);
    let options: SimpleFileOptions = Default::default();
    let xml = std::fs::read(database_xml_path)?;
    zip.start_file(BACKUP_DATABASE_NAME, options)?;
    std::io::Write::write_all(&mut zip, &xml)?;
    if let Some(dir) = thumbnails_dir {
        if dir.is_dir() {
            let mut names: Vec<_> = std::fs::read_dir(dir)?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name())
                .collect();
            names.sort();
            for name in names {
                let path = dir.join(&name);
                if path.is_file() {
                    let data = std::fs::read(&path)?;
                    zip.start_file(
                        format!("{THUMBNAILS_BACKUP_FOLDER}/{}", name.to_string_lossy()),
                        options,
                    )?;
                    std::io::Write::write_all(&mut zip, &data)?;
                }
            }
        }
    }
    zip.finish()?;
    Ok(())
}

/// `ComicDatabase.RestoreBackup`: extract the database XML to
/// `restore_target` (typically `<db>.restore`) and the thumbnails into
/// `thumbnails_dir`. The next `open_with_fallback` consumes the
/// `.restore` file.
pub fn restore_backup(
    backup_file: &Path,
    restore_target: &Path,
    thumbnails_dir: Option<&Path>,
) -> std::io::Result<()> {
    let file = std::fs::File::open(backup_file)?;
    let mut zip = ZipArchive::new(file)?;
    // Database XML.
    let mut entry = zip.by_name(BACKUP_DATABASE_NAME)?;
    let mut xml = Vec::new();
    entry.read_to_end(&mut xml)?;
    drop(entry);
    // Write atomically-ish: to a temp sibling then rename (the C#
    // writes directly and deletes on failure — same net effect).
    let tmp = restore_target.with_extension("restore.tmp");
    std::fs::write(&tmp, &xml)?;
    std::fs::rename(&tmp, restore_target)?;
    // Thumbnails.
    if let Some(dir) = thumbnails_dir {
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let name = entry.name().to_string();
            let Some(file_name) = name
                .strip_prefix(THUMBNAILS_BACKUP_FOLDER)
                .and_then(|s| s.strip_prefix('/'))
                .map(std::string::ToString::to_string)
            else {
                continue;
            };
            let mut data = Vec::new();
            entry.read_to_end(&mut data)?;
            std::fs::create_dir_all(dir)?;
            std::fs::write(dir.join(file_name), data)?;
        }
    }
    Ok(())
}

/// Full round trip used by the tests and the future UI: save → backup →
/// (disaster) → restore into the `.restore` slot → reopen.
pub fn backup_and_restore(
    db: &cr_core::database::comic_database::ComicDatabase,
    database_file: &Path, // the ComicDb.xml path (no extension suffix)
    backup_file: &Path,
    thumbnails_dir: Option<&Path>,
) -> std::io::Result<(cr_core::database::comic_database::ComicDatabase, OpenStatus)> {
    let xml_path = database_file.with_extension("xml");
    backup_to(db, backup_file, &xml_path, thumbnails_dir)?;
    // Disaster: the main database file is gone.
    let _ = std::fs::remove_file(&xml_path);
    // Restore into the .restore slot, then reopen through the chain.
    let restore_target = database_file.with_extension("restore");
    restore_backup(backup_file, &restore_target, thumbnails_dir)?;
    open_with_fallback(&database_file.with_extension("xml")).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::database::comic_database::create_new;
    use cr_core::model::comic_book::ComicBook;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "comicrust-backup-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn backup_create_destroy_restore_round_trip() {
        let dir = temp_dir("flow");
        // The database file base name (the C# `DatabaseFile`).
        let db_file = dir.join("ComicDb");
        let xml_path = dir.join("ComicDb.xml");
        let backup = dir.join("backup.zip");
        let thumbs = dir.join("thumbs");
        std::fs::create_dir_all(&thumbs).unwrap();
        std::fs::write(thumbs.join("custom.jpg"), b"jpegdata").unwrap();

        // A database with one book and the default lists.
        let mut db = create_new();
        let book = ComicBook {
            id: cr_core::xml::scalar::CrGuid::parse("11111111-2222-3333-4444-555555555555")
                .unwrap(),
            enable_proposed: false,
            info: cr_core::model::comic_info::ComicInfo {
                series: "Backup Test".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        db.books.push(book);
        let lists_before = count_lists(&db);

        backup_to(&db, &backup, &xml_path, Some(&thumbs)).expect("backup");
        assert!(xml_path.exists());
        assert!(backup.exists());

        // Destroy the main file.
        std::fs::remove_file(&xml_path).unwrap();
        // Restore into the .restore slot and reopen.
        let restore_target = db_file.with_extension("restore");
        restore_backup(&backup, &restore_target, Some(&thumbs)).expect("restore");
        assert!(restore_target.exists());
        let (restored, status) = open_with_fallback(&xml_path).expect("reopen");
        assert_eq!(status, OpenStatus::RestoredFromRestore);
        assert_eq!(restored.books.len(), 1);
        assert_eq!(restored.books[0].info.series, "Backup Test");
        assert_eq!(count_lists(&restored), lists_before);
        assert!(thumbs.join("custom.jpg").exists());
    }

    #[test]
    fn backup_zip_container_shape() {
        let dir = temp_dir("shape");
        let xml_path = dir.join("MyDb.xml");
        let backup = dir.join("b.zip");
        let db = create_new();
        backup_to(&db, &backup, &xml_path, None).expect("backup");
        let file = std::fs::File::open(&backup).unwrap();
        let mut zip = ZipArchive::new(file).unwrap();
        assert_eq!(zip.comment(), BACKUP_COMMENT.as_bytes());
        assert!(zip.by_name(BACKUP_DATABASE_NAME).is_ok());
    }

    fn count_lists(db: &cr_core::database::comic_database::ComicDatabase) -> usize {
        fn walk(items: &[cr_core::database::list_items::ComicListItem]) -> usize {
            items
                .iter()
                .map(|i| match i {
                    cr_core::database::list_items::ComicListItem::Folder(f) => 1 + walk(&f.items),
                    _ => 1,
                })
                .sum()
        }
        walk(&db.comic_lists)
    }
}
