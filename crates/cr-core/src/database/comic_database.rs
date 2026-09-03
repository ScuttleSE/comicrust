//! `ComicDatabase` root: the ComicDb.xml document, load/save with the
//! `.bak` rotation and the corrupt-file fallback chain of
//! `DatabaseManager.Open`.

use crate::database::list_items::{
    ComicBookMatcher, ComicListItem, FolderItem, LibraryListItem, ListItemBase, SmartListItem,
    ValueMatcher, WatchFolder,
};
use crate::model::comic_book::ComicBook;
use crate::xml::reader::{XmlError, XmlResult};
use crate::xml::scalar::CrGuid;
use crate::xml::{Emitter, Tok, XmlReader};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ComicDatabase {
    /// IdComponent.Id: always written.
    pub id: CrGuid,
    pub name: Option<String>,
    pub books: Vec<ComicBook>,
    pub comic_lists: Vec<ComicListItem>,
    pub watch_folders: Vec<WatchFolder>,
    pub black_list: Vec<String>,
}

impl ComicDatabase {
    pub fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.root("ComicDatabase")?;
        e.attr("Id", &self.id.to_d_string())?;
        if let Some(n) = &self.name {
            e.attr("Name", n)?;
        }
        e.start("Books")?;
        for b in &self.books {
            b.write_xml(e)?;
        }
        e.end()?;
        e.start("ComicLists")?;
        for item in &self.comic_lists {
            item.write_xml(e)?;
        }
        e.end()?;
        e.start("WatchFolders")?;
        for w in &self.watch_folders {
            w.write_xml(e)?;
        }
        e.end()?;
        e.start("BlackList")?;
        for f in &self.black_list {
            e.text_elem("File", f)?;
        }
        e.end()?;
        e.end()
    }

    pub fn parse(reader: &mut XmlReader<'_>) -> XmlResult<Self> {
        let start = match reader.next_tok()? {
            Tok::Start(s) => s,
            Tok::Eof => return Err(XmlError("empty document".into())),
            _ => return Err(XmlError("unexpected token before root".into())),
        };
        if start.name != "ComicDatabase" {
            return Err(XmlError(format!("unexpected root <{}>", start.name)));
        }
        let mut db = ComicDatabase::default();
        for (k, v) in &start.attrs {
            match k.as_str() {
                "Id" => db.id = CrGuid::parse(v)?,
                "Name" => db.name = Some(v.clone()),
                _ => {}
            }
        }
        loop {
            match reader.next_tok()? {
                Tok::Eof => return Err(XmlError("eof before </ComicDatabase>".into())),
                Tok::End(n) if n == "ComicDatabase" => return Ok(db),
                Tok::Start(s) => match s.name.as_str() {
                    "Books" => loop {
                        match reader.next_tok()? {
                            Tok::Eof => return Err(XmlError("eof in Books".into())),
                            Tok::End(n) if n == "Books" => break,
                            Tok::Start(bs) if bs.name == "Book" => {
                                db.books.push(ComicBook::read_xml(&bs, reader)?);
                            }
                            Tok::Start(bs) => reader.skip_element(&bs.name)?,
                            Tok::Text(_) => {}
                            _ => {}
                        }
                    },
                    "ComicLists" => loop {
                        match reader.next_tok()? {
                            Tok::Eof => return Err(XmlError("eof in ComicLists".into())),
                            Tok::End(n) if n == "ComicLists" => break,
                            Tok::Start(is) if is.name == "Item" => {
                                db.comic_lists.push(ComicListItem::from_start(&is, reader)?);
                            }
                            Tok::Start(is) => reader.skip_element(&is.name)?,
                            Tok::Text(_) => {}
                            _ => {}
                        }
                    },
                    "WatchFolders" => loop {
                        match reader.next_tok()? {
                            Tok::Eof => return Err(XmlError("eof in WatchFolders".into())),
                            Tok::End(n) if n == "WatchFolders" => break,
                            Tok::Start(ws) if ws.name == "WatchFolder" => {
                                let mut w = WatchFolder::default();
                                for (k, v) in &ws.attrs {
                                    match k.as_str() {
                                        "Folder" => w.folder = v.clone(),
                                        "Watch" => w.watch = v.trim() == "true" || v.trim() == "1",
                                        _ => {}
                                    }
                                }
                                reader.skip_element("WatchFolder")?;
                                db.watch_folders.push(w);
                            }
                            Tok::Start(ws) => reader.skip_element(&ws.name)?,
                            Tok::Text(_) => {}
                            _ => {}
                        }
                    },
                    "BlackList" => loop {
                        match reader.next_tok()? {
                            Tok::Eof => return Err(XmlError("eof in BlackList".into())),
                            Tok::End(n) if n == "BlackList" => break,
                            Tok::Start(fs) if fs.name == "File" => {
                                db.black_list.push(reader.text_content("File")?);
                            }
                            Tok::Start(fs) => reader.skip_element(&fs.name)?,
                            Tok::Text(_) => {}
                            _ => {}
                        }
                    },
                    // Unknown elements ignored like the .NET reader.
                    _ => reader.skip_element(&s.name)?,
                },
                Tok::Text(_) => {}
                _ => {}
            }
        }
    }
}

/// Outcome of [`open_with_fallback`] (the C# `OpenMessage`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenStatus {
    Loaded,
    RestoredFromRestore,
    RestoredFromBak,
    /// The database file was corrupt (and no `.bak` existed) — a new
    /// empty database replaced it. The C# shows the "problem"
    /// message for this case.
    NewEmpty,
    /// The database file does not exist yet (a fresh install): the
    /// C# `LoadXml` returns `CreateNew()` with no message.
    FreshEmpty,
}

impl OpenStatus {
    /// The exact C# `TR.Messages` default strings.
    pub fn message(self) -> Option<&'static str> {
        match self {
            OpenStatus::Loaded | OpenStatus::FreshEmpty => None,
            OpenStatus::RestoredFromRestore => Some(
                "A previous database backup has been successfully restored!",
            ),
            OpenStatus::RestoredFromBak => Some(
                "There was a problem with the Database. The last version to be known good has been restored. You may have lost some entires.",
            ),
            OpenStatus::NewEmpty => Some(
                "There was a problem opening the Database. A new empty Database has been created.",
            ),
        }
    }
}

/// Error type for database load/save.
#[derive(Debug)]
pub enum DbError {
    Io(std::io::Error),
    Xml(XmlError),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::Io(e) => write!(f, "io error: {e}"),
            DbError::Xml(e) => write!(f, "xml error: {e}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<std::io::Error> for DbError {
    fn from(e: std::io::Error) -> Self {
        DbError::Io(e)
    }
}

impl From<XmlError> for DbError {
    fn from(e: XmlError) -> Self {
        DbError::Xml(e)
    }
}

/// Loads a ComicDb.xml. Strict: the file must exist and parse.
pub fn load(path: &Path) -> Result<ComicDatabase, DbError> {
    let f = std::fs::File::open(path)?;
    let mut buf = BufReader::new(f);
    let mut reader = XmlReader::new(&mut buf);
    Ok(ComicDatabase::parse(&mut reader)?)
}

/// Serializes to the exact ComicDb.xml byte form.
pub fn save_bytes(db: &ComicDatabase) -> Result<Vec<u8>, DbError> {
    let mut out = Vec::new();
    let mut e = Emitter::new(&mut out)?;
    db.write_xml(&mut e)?;
    e.finish()?;
    Ok(out)
}

/// Saves like `ComicDatabase.Save`: serialize to `path.bak`, then copy
/// over `path`. The `.bak` file is kept (C# behavior). On failure the
/// `.bak` is removed and the error rethrown.
pub fn save(db: &ComicDatabase, path: &Path) -> Result<(), DbError> {
    let bak = path_with_suffix(path, "bak");
    let bytes = save_bytes(db)?;
    {
        let mut f = std::fs::File::create(&bak)?;
        f.write_all(&bytes)?;
    }
    match std::fs::copy(&bak, path) {
        Ok(_) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&bak);
            Err(DbError::Io(e))
        }
    }
}

fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".");
    s.push(suffix);
    PathBuf::from(s)
}

/// The `DatabaseManager.Open` fallback chain for `ComicDb.xml`:
/// `.restore` first (consumed), then the file itself, then `.bak`, then
/// quarantine the corrupt file as `Corrupt Database Backup [<now>].xml`
/// and start with an empty database.
pub fn open_with_fallback(path: &Path) -> Result<(ComicDatabase, OpenStatus), DbError> {
    // 1. .restore — the C# `DatabaseFile + ".restore"`: the database
    // file name WITHOUT the ".xml" (`ComicDb.restore`).
    let restore = path.with_extension("restore");
    if restore.exists() {
        let loaded = load(&restore);
        let _ = std::fs::remove_file(&restore);
        if let Ok(db) = loaded {
            return Ok((db, OpenStatus::RestoredFromRestore));
        }
    }
    // 2. main file
    if let Ok(db) = load(path) {
        return Ok((db, OpenStatus::Loaded));
    }
    // 3. .bak
    let bak = path_with_suffix(path, "bak");
    if bak.exists() {
        if let Ok(db) = load(&bak) {
            return Ok((db, OpenStatus::RestoredFromBak));
        }
    }
    // 4. quarantine + fresh DB — the C# fallback uses
    // `ComicDatabase.CreateNew()` (the default list tree). A missing
    // file is a silent fresh start (`LoadXml` returns `CreateNew()`);
    // only a corrupt file shows the "problem" message.
    if path.exists() {
        let now = chrono::Local::now();
        let name = format!(
            "Corrupt Database Backup [{}].xml",
            now.format("%Y-%m-%d %H-%M-%S")
        );
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let _ = std::fs::copy(path, parent.join(name));
        Ok((create_new(), OpenStatus::NewEmpty))
    } else {
        Ok((create_new(), OpenStatus::FreshEmpty))
    }
}

/// Round-trip helper: load, re-serialize, byte-compare. Returns the
/// original and the re-serialized bytes.
pub fn round_trip(path: &Path) -> Result<(Vec<u8>, Vec<u8>), DbError> {
    let mut original = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut original)?;
    let db = load(path)?;
    Ok((original, save_bytes(&db)?))
}

/// `ComicDatabase.CreateNew` — a fresh database with the default list
/// tree (the localized names are English defaults; TR lands in Phase 5).
/// A fresh database with the C# default list tree
/// (`ComicLibrary.InitializeDefaultLists`, English names — the TR
/// localized names are a Phase 5 concern).
pub fn create_new() -> ComicDatabase {
    let mut db = ComicDatabase {
        id: CrGuid::new_random(),
        ..Default::default()
    };

    let library = ComicListItem::Library(LibraryListItem {
        base: ListItemBase {
            id: CrGuid::new_random(),
            name: Some("Library".into()),
            ..Default::default()
        },
    });

    // The "Smart Lists" folder children, in InitializeDefaultLists
    // order, with the engine-configuration default values
    // (IsRecentInDays 14, IsRead 95, IsNotRead 10).
    let smart = |name: &str, matchers: Vec<ComicBookMatcher>| {
        ComicListItem::Smart(SmartListItem {
            base: ListItemBase {
                id: CrGuid::new_random(),
                name: Some(name.into()),
                ..Default::default()
            },
            matchers,
            ..Default::default()
        })
    };
    let value_matcher = |type_name: &str, op: i32, v1: &str, v2: &str| {
        ComicBookMatcher::Value(ValueMatcher {
            type_name: type_name.into(),
            match_operator: op,
            match_value: v1.into(),
            match_value_2: v2.into(),
            ..Default::default()
        })
    };

    let items = vec![
        smart(
            "My Favorites",
            vec![value_matcher("ComicBookRatingMatcher", 1, "3", "")],
        ),
        smart(
            "Recently Added",
            vec![value_matcher("ComicBookAddedMatcher", 3, "14", "")],
        ),
        smart(
            "Recently Read",
            vec![value_matcher("ComicBookOpenedMatcher", 3, "14", "")],
        ),
        smart(
            "Never Read",
            vec![value_matcher("ComicBookReadPercentageMatcher", 2, "10", "")],
        ),
        smart(
            "Reading",
            vec![value_matcher(
                "ComicBookReadPercentageMatcher",
                3,
                "10",
                "95",
            )],
        ),
        smart(
            "Read",
            vec![value_matcher("ComicBookReadPercentageMatcher", 1, "95", "")],
        ),
        smart(
            "Files to update",
            vec![value_matcher("ComicBookModifiedInfoMatcher", 0, "", "")],
        ),
    ];

    let folder = ComicListItem::Folder(FolderItem {
        base: ListItemBase {
            id: CrGuid::new_random(),
            name: Some("Smart Lists".into()),
            ..Default::default()
        },
        items,
        ..Default::default()
    });

    db.comic_lists.push(library);
    db.comic_lists.push(folder);
    db
}
