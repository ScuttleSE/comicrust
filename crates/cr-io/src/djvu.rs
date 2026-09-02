//! DjVu accessor — port of `DjvuComicProvider.cs` + `DjVuImage.cs`.
//!
//! Like the 7z path, the C# uses bundled Windows exes (`djvm.exe`,
//! `ddjvu.exe`, `c44.exe`); we use the djvulibre tools from `PATH`
//! (`CR_DJVULIBRE` env var can point at a directory containing them).
//!
//! - listing: `djvm -l <file>`, lines `<size> PAGE #<n> <name>`
//!   (`rxList` in the C#; page numbers are 1-based)
//! - rendering: `ddjvu -format=ppm -size=2000x2000 -page=N` (the C#
//!   asks for TIFF and decodes it; PPM needs no decoder here — same
//!   pixel pipeline, different intermediate container)
//! - the render box is `EngineConfiguration.Default.DjVuSizeLimit`,
//!   default 2000x2000; ddjvu fits the page inside it
//! - format check: `AT&TF` magic, `true` on open errors
//! - JPEG quality 75 (GDI+ default, as in `ImageToJpegBytes`)
//!
//! The reader is registered unconditionally; when the tools are
//! missing, listing fails and the provider ends up with an empty page
//! list — the same observable behavior as a failed parse.

use std::path::{Path, PathBuf};
use std::process::Command;

use jpeg_encoder::{ColorType, Encoder};

use crate::error::{Error, Result};
use crate::provider::{ComicAccessor, ProviderImageInfo};

/// `EngineConfiguration.Default.DjVuSizeLimit` default.
const SIZE_LIMIT: u32 = 2000;

/// GDI+ default JPEG quality.
const JPEG_QUALITY: u8 = 75;

fn tool_dir() -> PathBuf {
    std::env::var("CR_DJVULIBRE")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn find_tool(name: &str) -> Option<PathBuf> {
    let direct = tool_dir().join(name);
    if direct.is_file() {
        return Some(direct);
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn run_tool(exe: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new(exe)
        .args(args)
        .output()
        .map_err(|e| Error::Access(format!("running {} failed: {e}", exe.display())))
}

pub struct DjVuAccessor;

impl ComicAccessor for DjVuAccessor {
    /// Port of `DjvuComicProvider.OnFastFormatCheck`: `AT&TF` magic,
    /// `true` on open errors.
    fn is_format(&self, source: &Path) -> bool {
        match std::fs::File::open(source) {
            Err(_) => true,
            Ok(mut file) => {
                use std::io::Read;
                let mut head = [0u8; 5];
                match file.read(&mut head) {
                    Err(_) => true,
                    Ok(n) => n == 5 && &head == b"AT&TF",
                }
            }
        }
    }

    /// `DjvuComicProvider.ReadPages` via `djvm -l`.
    fn get_entry_list(&self, source: &Path) -> Result<Vec<ProviderImageInfo>> {
        let djvm = find_tool("djvm")
            .ok_or_else(|| Error::Access("djvm not found (install djvulibre-bin)".into()))?;
        let source_str = source.to_string_lossy();
        let out = run_tool(&djvm, &["-l", &source_str])?;
        if !out.status.success() {
            return Err(Error::Access(format!(
                "djvm listing failed for {source_str} (exit {:?})",
                out.status.code()
            )));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        Ok(parse_djvm_listing(&text))
    }

    /// `DjVuImage.GetBitmap` + JPEG encode; any failure is `None`.
    fn read_byte_image(&self, source: &Path, info: &ProviderImageInfo) -> Option<Vec<u8>> {
        let ddjvu = find_tool("ddjvu")?;
        let source_str = source.to_string_lossy();
        let page = (info.index + 1).to_string();
        let size = format!("{SIZE_LIMIT}x{SIZE_LIMIT}");
        let out = run_tool(
            &ddjvu,
            &[
                "-format=ppm",
                &format!("-size={size}"),
                &format!("-page={page}"),
                &source_str,
            ],
        )
        .ok()?;
        if !out.status.success() {
            return None;
        }
        let (width, height, rgb) = parse_ppm(&out.stdout)?;
        let mut jpeg = Vec::new();
        Encoder::new(&mut jpeg, JPEG_QUALITY)
            .encode(&rgb, width, height, ColorType::Rgb)
            .ok()?;
        Some(jpeg)
    }

    /// `DjvuComicProvider` has no in-archive info support.
    fn read_info_file(&self, _source: &Path, _filename: &str) -> Option<Vec<u8>> {
        None
    }
}

/// Parses `djvm -l` output. Both the header line ("PAGE #1") and
/// document/file lines carry "PAGE #n"; the C# regex only keeps the
/// size-prefixed form, so do the same.
fn parse_djvm_listing(text: &str) -> Vec<ProviderImageInfo> {
    let mut entries = Vec::new();
    for raw_line in text.lines() {
        // The C# regex is unanchored; real output indents these lines.
        let line = raw_line.trim_end_matches('\r').trim_start();
        let Some(size_end) = line.find(" PAGE #") else {
            continue;
        };
        if !line.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        let Ok(size) = line[..size_end].trim().parse::<u64>() else {
            continue;
        };
        let rest = &line[size_end + " PAGE #".len()..];
        let Some(hash_end) = rest.find(|c: char| !c.is_ascii_digit()) else {
            continue;
        };
        let Ok(page_no) = rest[..hash_end].parse::<usize>() else {
            continue;
        };
        let name = rest[hash_end..].trim_start();
        entries.push(ProviderImageInfo::new(page_no - 1, name.to_string(), size));
    }
    entries
}

/// Minimal PPM (P6) parser for ddjvu output.
fn parse_ppm(data: &[u8]) -> Option<(u16, u16, Vec<u8>)> {
    let mut cursor = 0usize;
    let mut fields = Vec::new();
    while fields.len() < 3 {
        while cursor < data.len() && data[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor < data.len() && data[cursor] == b'#' {
            while cursor < data.len() && data[cursor] != b'\n' {
                cursor += 1;
            }
            continue;
        }
        let start = cursor;
        while cursor < data.len() && !data[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        fields.push(std::str::from_utf8(&data[start..cursor]).ok()?);
    }
    cursor += 1; // single whitespace after maxval
    let width: u16 = fields[0].parse().ok()?;
    let height: u16 = fields[1].parse().ok()?;
    let maxval: u32 = fields[2].parse().ok()?;
    if maxval != 255 {
        return None;
    }
    let len = width as usize * height as usize * 3;
    let data = data.get(cursor..cursor + len)?.to_vec();
    Some((width, height, data))
}

#[cfg(test)]
mod tests {
    use super::parse_djvm_listing;

    #[test]
    fn djvm_listing_parse() {
        let text = "\
DOCUMENT
    Paged, 2 pages
    DIRM: flags=0x000000, bgflags=0x00
    12345 PAGE #1 page1.djvu
    23456 PAGE #2 page2.djvu
";
        let entries = parse_djvm_listing(text);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].index, 0);
        assert_eq!(entries[0].name, "page1.djvu");
        assert_eq!(entries[0].size, 12345);
        assert_eq!(entries[1].index, 1);
        assert_eq!(entries[1].name, "page2.djvu");
    }
}
