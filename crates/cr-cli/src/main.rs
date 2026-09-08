//! Headless verification tooling: `info`, `db-dump`, `db-roundtrip`,
//! `pages`, `extract`.

use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};

use cr_core::database::{load, save_bytes};
use cr_core::model::comic_book::ComicBook;
use cr_core::model::comic_name_info;
use cr_core::registry::{self, PropValue};
use cr_core::xml::XmlReader;
use cr_io::ComicProvider;

#[derive(Parser)]
#[command(name = "cr-cli", about = "comicrust headless verification tools")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print parsed metadata for a comic file as JSON. Reads
    /// ComicInfo.xml files directly; anything else is proposed from the
    /// filename (archive reading arrives in Phase 1).
    Info { file: String },
    /// Validate and summarize a ComicDb.xml.
    DbDump { file: String },
    /// Load and re-serialize a ComicDb.xml; report byte differences.
    DbRoundtrip { file: String },
    /// Enumerate the pages of a comic file: index, byte size, entry
    /// name, in provider page order.
    Pages { file: String },
    /// Extract one page's raw bytes (no decode yet; the decode chain
    /// lands with cr-image in T3). Prints to stdout unless -o is set.
    Extract {
        file: String,
        /// Zero-based page index.
        #[arg(default_value_t = 0)]
        page: usize,
        /// Output path; omit for stdout.
        #[arg(short, long)]
        output: Option<String>,
        /// Decode the page (raw bytes are normalized to JPEG; this
        /// fully decodes and re-encodes as JPEG).
        #[arg(long)]
        decode: bool,
    },
    /// Generate the page-0 thumbnail (512px height, JPEG q60) and
    /// write it out.
    Thumb {
        file: String,
        /// Output path; omit for stdout.
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Read the metadata of a comic file and write it back
    /// (in-archive ComicInfo.xml/ComicBook.xml + xattrs), then
    /// verify: only metadata entries may change.
    Rewrite { file: String },
    /// Parse MetronInfo.xml from a comic file and print the
    /// MetronInfo-to-ComicInfo mapping as JSON.
    Metron { file: String },
    /// Evaluate every smart list in a ComicDb.xml against its books.
    /// Prints list name, evaluated book count, and (when present) the
    /// count of the C#-cached id list for comparison.
    Lists { file: String },
    /// Migrate a ComicRack CE profile: verify its ComicDb.xml, copy it
    /// into this port's database location, and map the ini keys the
    /// port consumes. The comic FILES stay where they are; Windows
    /// paths left in the database are migrated by the app itself (the
    /// boot prompt, File ▸ Migrate Windows Paths...).
    Migrate {
        /// The ComicRack CE profile directory (containing
        /// ComicDb/ComicDb.xml) or a ComicDb.xml file directly.
        source: String,
        /// Target ComicDb.xml (default: this port's database
        /// location).
        #[arg(long)]
        out: Option<String>,
        /// Overwrite an existing target (the old file is kept as
        /// `<target>.premigrate.bak`).
        #[arg(long)]
        force: bool,
        /// Show what would happen; write nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run(command: Command) -> Result<ExitCode> {
    match command {
        Command::Info { file } => cmd_info(&file),
        Command::DbDump { file } => cmd_db_dump(&file),
        Command::DbRoundtrip { file } => cmd_db_roundtrip(&file),
        Command::Pages { file } => cmd_pages(&file),
        Command::Extract {
            file,
            page,
            output,
            decode,
        } => cmd_extract(&file, page, output.as_deref(), decode),
        Command::Thumb { file, output } => cmd_thumb(&file, output.as_deref()),
        Command::Rewrite { file } => cmd_rewrite(&file),
        Command::Metron { file } => cmd_metron(&file),
        Command::Lists { file } => cmd_lists(&file),
        Command::Migrate {
            source,
            out,
            force,
            dry_run,
        } => cmd_migrate(&source, out.as_deref(), force, dry_run),
    }
}

fn cmd_lists(file: &str) -> Result<ExitCode> {
    let db = load(Path::new(file)).with_context(|| format!("loading {file}"))?;
    let books: Vec<&ComicBook> = db.books.iter().collect();

    fn walk<'a>(
        items: &'a [cr_core::database::list_items::ComicListItem],
        out: &mut Vec<(&'a str, &'a cr_core::database::list_items::SmartListItem)>,
    ) {
        use cr_core::database::list_items::ComicListItem;
        for item in items {
            match item {
                ComicListItem::Smart(s) => out.push((s.base.name.as_deref().unwrap_or(""), s)),
                ComicListItem::Folder(f) => walk(&f.items, out),
                _ => {}
            }
        }
    }
    let mut lists = Vec::new();
    walk(&db.comic_lists, &mut lists);

    println!(
        "{:<20} {:>8} {:>10} {:>8}",
        "list", "matched", "cached", "delta"
    );
    for (name, list) in lists {
        let result = cr_engine::smart_list::evaluate_smart_list(list, &books, None);
        let cached = list
            .base
            .cache_storage
            .as_deref()
            .filter(|c| !c.is_empty() && *c != "Custom")
            .map_or(0, |c| c.split(',').filter(|g| !g.trim().is_empty()).count());
        let delta = result.len() as i64 - cached as i64;
        println!(
            "{:<20} {:>8} {:>10} {:>8}",
            name,
            result.len(),
            cached,
            delta
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn prop_json(v: &PropValue) -> Value {
    match v {
        PropValue::Str(s) => json!(s),
        PropValue::Int(i) => json!(i),
        PropValue::Float(f) => json!(f),
        PropValue::Bool(b) => json!(b),
        PropValue::Guid(g) => json!(g.to_d_string()),
        PropValue::Date(d) => json!(d.to_xml()),
    }
}

fn book_json(book: &ComicBook) -> Value {
    let mut obj = serde_json::Map::new();
    for (name, def) in registry::entries() {
        obj.insert((*name).to_string(), prop_json(&(def.get)(book)));
    }
    obj.insert(
        "Pages".into(),
        Value::Array(
            book.info
                .pages
                .iter()
                .map(|p| {
                    json!({
                        "Image": p.image_index(),
                        "ImageWidth": p.image_width,
                        "ImageHeight": p.image_height,
                        "ImageSize": p.image_file_size,
                        "Type": p.page_type.to_xml(),
                        "Bookmark": p.bookmark,
                        "Key": p.key,
                        "Rotation": p.rotation.to_xml(),
                        "PagePosition": p.page_position.to_xml(),
                    })
                })
                .collect(),
        ),
    );
    Value::Object(obj)
}

fn cmd_info(file: &str) -> Result<ExitCode> {
    let path = Path::new(file);
    let out = if path.extension().and_then(|e| e.to_str()) == Some("xml") {
        let mut buf = std::io::BufReader::new(std::fs::File::open(path)?);
        let mut reader = XmlReader::new(&mut buf);
        let info =
            cr_core::model::comic_info::parse_root(&mut reader).context("parsing ComicInfo.xml")?;
        let book = ComicBook {
            info,
            ..Default::default()
        };
        book_json(&book)
    } else {
        let name_info = comic_name_info::from_file_path(file);
        json!({
            "Source": "filename",
            "Series": name_info.series,
            "Title": name_info.title,
            "Number": name_info.number,
            "Count": name_info.count,
            "Volume": name_info.volume,
            "Year": name_info.year,
            "Format": name_info.format,
            "CoverCount": name_info.cover_count,
        })
    };
    println!("{out:#}");
    Ok(ExitCode::SUCCESS)
}

fn list_item_summary(items: &[cr_core::database::list_items::ComicListItem]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|item| {
                let base = item.base();
                let mut v = json!({
                    "Type": match item {
                        cr_core::database::list_items::ComicListItem::Smart(_) => "ComicSmartListItem",
                        cr_core::database::list_items::ComicListItem::Folder(_) => "ComicListItemFolder",
                        cr_core::database::list_items::ComicListItem::IdList(_) => "ComicIdListItem",
                        cr_core::database::list_items::ComicListItem::Library(_) => "ComicLibraryListItem",
                    },
                    "Id": base.id.to_d_string(),
                    "Name": base.name,
                });
                if let cr_core::database::list_items::ComicListItem::Folder(f) = item {
                    v["Items"] = list_item_summary(&f.items);
                }
                if let cr_core::database::list_items::ComicListItem::Smart(s) = item {
                    v["MatcherCount"] = json!(s.matchers.len());
                }
                v
            })
            .collect(),
    )
}

fn cmd_db_dump(file: &str) -> Result<ExitCode> {
    let db = load(Path::new(file)).context("loading database")?;
    let summary = json!({
        "Id": db.id.to_d_string(),
        "Name": db.name,
        "BookCount": db.books.len(),
        "ComicLists": list_item_summary(&db.comic_lists),
        "WatchFolders": db.watch_folders.iter().map(|w| json!({
            "Folder": w.folder,
            "Watch": w.watch,
        })).collect::<Vec<_>>(),
        "BlackListCount": db.black_list.len(),
        "Books": db.books.iter().map(|b| json!({
            "Id": b.id.to_d_string(),
            "File": b.file_path,
            "Series": b.info.series,
            "Number": b.info.number,
            "PageCount": b.info.page_count,
            "CustomValues": cr_core::model::comic_book::values_store::decode(&b.custom_values_store),
        })).collect::<Vec<_>>(),
    });
    println!("{summary:#}");
    Ok(ExitCode::SUCCESS)
}

fn cmd_pages(file: &str) -> Result<ExitCode> {
    let provider =
        ComicProvider::open(Path::new(file)).with_context(|| format!("opening comic {file}"))?;
    let summary = json!({
        "File": file,
        "Format": provider.format().name,
        "PageCount": provider.page_count(),
        "Hash": provider.create_hash(),
        "Pages": provider.pages().iter().enumerate().map(|(i, p)| json!({
            "Index": i,
            "Size": p.size,
            "Name": p.name,
        })).collect::<Vec<_>>(),
    });
    println!("{summary:#}");
    Ok(ExitCode::SUCCESS)
}

fn cmd_extract(file: &str, page: usize, output: Option<&str>, decode: bool) -> Result<ExitCode> {
    let provider =
        ComicProvider::open(Path::new(file)).with_context(|| format!("opening comic {file}"))?;
    let data = provider
        .read_page(page)
        .with_context(|| format!("reading page {page} of {file}"))?;
    let data = if decode {
        let image = cr_image::decode(&data).context("decoding page image")?;
        cr_image::encode_jpeg(&image, 75).context("encoding page image")?
    } else {
        data
    };
    match output {
        Some(path) => {
            std::fs::write(path, &data).with_context(|| format!("writing {path}"))?;
        }
        None => {
            std::io::stdout()
                .write_all(&data)
                .context("writing page bytes to stdout")?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_thumb(file: &str, output: Option<&str>) -> Result<ExitCode> {
    let provider =
        ComicProvider::open(Path::new(file)).with_context(|| format!("opening comic {file}"))?;
    let data = provider
        .read_page(0)
        .with_context(|| format!("reading page 0 of {file}"))?;
    let image = cr_image::decode(&data).context("decoding page image")?;
    let thumb = cr_image::thumbnail_from_image(&image, (image.width, image.height))
        .context("rendering thumbnail")?;
    match output {
        Some(path) => {
            std::fs::write(path, &thumb.data).with_context(|| format!("writing {path}"))?;
        }
        None => {
            std::io::stdout()
                .write_all(&thumb.data)
                .context("writing thumbnail bytes to stdout")?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_rewrite(file: &str) -> Result<ExitCode> {
    let path = Path::new(file);
    // Entry content fingerprints before the write.
    let before = entry_hashes(path);
    let provider = ComicProvider::open(path).with_context(|| format!("opening comic {file}"))?;
    // Read metadata; with nothing found anywhere there is nothing to
    // write back (writing defaults would destroy file metadata).
    // A found ComicInfo (without a ComicBook) becomes the book's info
    // part, mirroring what the C# caller passes to StoreInfo.
    let book = match provider.load_book(cr_io::info::InfoLoadingMethod::Slow) {
        Some(book) => Some(book),
        None => provider
            .load_info(cr_io::info::InfoLoadingMethod::Slow)
            .map(|info| ComicBook {
                info,
                ..Default::default()
            }),
    };
    let mut report = serde_json::Map::new();
    report.insert("File".into(), json!(file));
    let Some(book) = book else {
        report.insert("Wrote".into(), json!(false));
        report.insert("NoMetadataFound".into(), json!(true));
        report.insert("OnlyMetadataChanged".into(), json!(true));
        println!("{:#}", Value::Object(report));
        return Ok(ExitCode::SUCCESS);
    };
    let written = provider.store_info(&book);
    let after = entry_hashes(path);

    let mut only_metadata_changed = true;
    report.insert("Wrote".into(), json!(written));
    let mut changed = Vec::new();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    for (name, hash) in &after {
        if !before.contains_key(name) {
            added.push(name.clone());
            if !is_metadata_entry(name) {
                only_metadata_changed = false;
            }
        } else if before.get(name) != Some(hash) {
            changed.push(name.clone());
            if !is_metadata_entry(name) {
                only_metadata_changed = false;
            }
        }
    }
    for name in before.keys() {
        if !after.contains_key(name) {
            removed.push(name.clone());
            only_metadata_changed = false;
        }
    }
    report.insert("Changed".into(), json!(changed));
    report.insert("Added".into(), json!(added));
    report.insert("Removed".into(), json!(removed));
    report.insert("OnlyMetadataChanged".into(), json!(only_metadata_changed));
    println!("{:#}", Value::Object(report));
    Ok(ExitCode::SUCCESS)
}

fn is_metadata_entry(name: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name);
    base.eq_ignore_ascii_case("ComicInfo.xml") || base.eq_ignore_ascii_case("ComicBook.xml")
}

/// SHA-1 of every entry's decompressed content, keyed by entry name.
fn entry_hashes(path: &Path) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let Ok(file) = std::fs::File::open(path) else {
        return out;
    };
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let sha = |data: &[u8]| {
        use sha1::{Digest, Sha1};
        let mut h = Sha1::new();
        h.update(data);
        format!("{:x}", h.finalize())
    };
    match ext.as_str() {
        "cbz" | "zip" => {
            let Ok(mut archive) = zip::ZipArchive::new(file) else {
                return out;
            };
            for i in 0..archive.len() {
                let Ok(mut entry) = archive.by_index(i) else {
                    continue;
                };
                let mut data = Vec::new();
                if entry.read_to_end(&mut data).is_ok() {
                    out.insert(entry.name().to_string(), sha(&data));
                }
            }
        }
        "cbt" | "tar" => {
            let mut archive = tar::Archive::new(file);
            if let Ok(entries) = archive.entries() {
                for entry in entries.flatten() {
                    let mut entry = entry;
                    let Ok(name) = entry.path().map(|p| p.to_string_lossy().into_owned()) else {
                        continue;
                    };
                    let mut data = Vec::new();
                    if entry.read_to_end(&mut data).is_ok() {
                        out.insert(name, sha(&data));
                    }
                }
            }
        }
        _ => {}
    }
    out
}

fn cmd_metron(file: &str) -> Result<ExitCode> {
    let provider =
        ComicProvider::open(Path::new(file)).with_context(|| format!("opening comic {file}"))?;
    let bytes = provider
        .read_info_file("MetronInfo.xml")
        .context("no MetronInfo.xml in source")?;
    let mut cursor = std::io::Cursor::new(&bytes);
    let mut reader = cr_core::xml::XmlReader::new(&mut cursor);
    let metron = cr_core::model::metron_info::MetronInfo::parse_root(&mut reader)
        .context("parsing MetronInfo.xml")?;
    let info = metron.to_comic_info();
    let book = ComicBook {
        info,
        ..Default::default()
    };
    println!("{:#}", book_json(&book));
    Ok(ExitCode::SUCCESS)
}

fn cmd_db_roundtrip(file: &str) -> Result<ExitCode> {
    let path = Path::new(file);
    let original = std::fs::read(path).context("reading file")?;
    let db = load(path).context("loading database")?;
    let resaved = save_bytes(&db)?;
    if original == resaved {
        println!("IDENTICAL: {file} ({} bytes)", original.len());
        Ok(ExitCode::SUCCESS)
    } else {
        let first = original
            .iter()
            .zip(resaved.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(original.len().min(resaved.len()));
        println!(
            "DIFFERENT: {file} ({} bytes in, {} bytes out, first difference at byte {first})",
            original.len(),
            resaved.len()
        );
        Ok(ExitCode::FAILURE)
    }
}

/// The `migrate` report (the summary the command prints).
struct MigrateReport {
    source: std::path::PathBuf,
    target: std::path::PathBuf,
    books: usize,
    lists: usize,
    watch_folders: usize,
    black_list: usize,
    has_windows_paths: bool,
    ini_keys: Vec<String>,
    ini_written: bool,
}

/// Resolves the ComicDb.xml inside a ComicRack CE profile layout: the
/// argument may be the profile dir (`ComicDb/ComicDb.xml`), a dir with
/// the database at its top, or the XML file itself.
fn resolve_source_db(source: &Path) -> Result<std::path::PathBuf> {
    if source.is_file() {
        return Ok(source.to_path_buf());
    }
    if source.is_dir() {
        for candidate in [
            source.join("ComicDb").join("ComicDb.xml"),
            source.join("ComicDb.xml"),
        ] {
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    anyhow::bail!(
        "no ComicDb.xml found under {} (expected <profile>/ComicDb/ComicDb.xml, <dir>/ComicDb.xml, or the file itself)",
        source.display()
    )
}

/// The `migrate` engine (the command prints the report; the tests
/// call this directly).
fn migrate(source: &Path, out: Option<&Path>, force: bool, dry_run: bool) -> Result<MigrateReport> {
    let source_db = resolve_source_db(source)?;
    let db = load(&source_db).context("loading the ComicRack CE database")?;
    let report = MigrateReport {
        books: db.books.len(),
        lists: db.comic_lists.len(),
        watch_folders: db.watch_folders.len(),
        black_list: db.black_list.len(),
        has_windows_paths: cr_engine::path_migration::has_windows_paths(&db),
        source: source_db,
        target: out.map(|p| p.to_path_buf()).unwrap_or_else(|| {
            cr_core::paths::database_file(&cr_core::paths::Paths::new_default())
        }),
        ini_keys: Vec::new(),
        ini_written: false,
    };

    // The database copy (never through a symlinked overwrite; the old
    // target is preserved when forced).
    if !dry_run {
        if report.target.exists() {
            if !force {
                anyhow::bail!(
                    "{} already exists — pass --force to keep the old file as {}.premigrate.bak",
                    report.target.display(),
                    report.target.display()
                );
            }
            let backup = report.target.with_file_name(format!(
                "{}.premigrate.bak",
                report
                    .target
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("ComicDb.xml")
            ));
            std::fs::copy(&report.target, &backup).context("backing up the old target")?;
            println!("kept the previous database as {}", backup.display());
        }
        if let Some(parent) = report.target.parent() {
            std::fs::create_dir_all(parent).context("creating the database directory")?;
        }
        std::fs::copy(&report.source, &report.target).context("copying the database")?;
    }

    // The ini mapping: the ExtendedSettings keys the port consumes
    // (the `ini: true` fields), read from the profile's ComicRack.ini
    // when present, merged into the port's ini (the other keys stay).
    let profile_dir = report
        .source
        .parent()
        .and_then(|p| p.file_name())
        .filter(|n| *n == "ComicDb")
        .and_then(|_| report.source.parent()?.parent())
        .map(|p| p.to_path_buf())
        .or_else(|| report.source.parent().map(|p| p.to_path_buf()));
    let mut ini_keys = Vec::new();
    if let Some(profile) = profile_dir {
        let source_ini = profile.join("ComicRack.ini");
        if source_ini.is_file() {
            let values = cr_core::settings::ini::IniValues::read_file(&source_ini);
            for field in cr_core::settings::extended::EXTENDED_FIELDS {
                if !field.ini_enabled {
                    continue;
                }
                if let Some(v) = values.get(field.name) {
                    ini_keys.push(format!("{}={v}", field.name));
                }
            }
            if !ini_keys.is_empty() && !dry_run {
                let entries: Vec<(&str, &str)> = ini_keys
                    .iter()
                    .map(|k| {
                        let (n, v) = k.split_once('=').unwrap_or((k, ""));
                        (n, v)
                    })
                    .collect();
                let target_ini = cr_core::paths::Paths::new_default()
                    .config_path
                    .join(cr_core::paths::INI_FILE_NAME);
                if let Some(parent) = target_ini.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                cr_core::settings::ini::merge_write(&target_ini, &entries)
                    .context("writing the port ini")?;
            }
        }
    }
    Ok(MigrateReport {
        ini_written: !dry_run && !ini_keys.is_empty(),
        ini_keys,
        ..report
    })
}

fn cmd_migrate(source: &str, out: Option<&str>, force: bool, dry_run: bool) -> Result<ExitCode> {
    let report = migrate(Path::new(source), out.map(Path::new), force, dry_run)?;
    let mode = if dry_run { "WOULD COPY" } else { "COPIED" };
    println!(
        "{}: {} -> {}",
        mode,
        report.source.display(),
        report.target.display()
    );
    println!(
        "verified: {} books, {} lists, {} watch folders, {} blacklist entries",
        report.books, report.lists, report.watch_folders, report.black_list
    );
    if report.has_windows_paths {
        println!(
            "NOTE: the database carries Windows paths (C:\\... / \\\\server\\...). Start comicrust — it offers the path migration (File ▸ Migrate Windows Paths...)."
        );
    }
    if report.ini_keys.is_empty() {
        println!("ini: no ComicRack.ini keys to map (or none the port consumes)");
    } else {
        for k in &report.ini_keys {
            println!("ini: {k}");
        }
        if !report.ini_written {
            println!("ini: (dry run — nothing written)");
        }
    }
    Ok(ExitCode::SUCCESS)
}
