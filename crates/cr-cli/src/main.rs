//! Headless verification tooling: `info`, `db-dump`, `db-roundtrip`,
//! `pages`, `extract`.

use std::io::Write;
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
    }
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
