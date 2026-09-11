//! DjVu tests. Gated: they run only when the djvulibre tools (`c44`,
//! `djvm`, `ddjvu`) exist — the C# bundles djvm.exe/ddjvu.exe, and we
//! never commit binaries to the repo (test data policy in
//! docs/archive/phases/phase-1.md).

use std::process::Command;

use cr_io::ComicProvider;

fn djvulibre_available() -> bool {
    ["c44", "djvm", "ddjvu"].iter().all(|tool| {
        Command::new(tool)
            .arg("-h")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// Builds a 2-page DjVu from two solid-color PPMs via c44 + djvm.
fn build_two_page_djvu(dir: &std::path::Path) -> std::path::PathBuf {
    let colors = [(1u8, 0u8, 0u8), (0, 0, 255)];
    for (i, (r, g, b)) in colors.iter().enumerate() {
        let mut raw = b"P6\n8 8\n255\n".to_vec();
        raw.extend(std::iter::repeat_n((*r, *g, *b), 8 * 8).flat_map(|(r, g, b)| [r, g, b]));
        let ppm = dir.join(format!("page{i}.ppm"));
        std::fs::write(&ppm, raw).unwrap();
        let status = Command::new("c44")
            .arg(&ppm)
            .arg(dir.join(format!("page{i}.djvu")))
            .status()
            .unwrap();
        assert!(status.success(), "c44 page{i} failed");
    }
    let out = dir.join("comic.djvu");
    let status = Command::new("djvm")
        .arg("-c")
        .arg(&out)
        .arg(dir.join("page0.djvu"))
        .arg(dir.join("page1.djvu"))
        .status()
        .unwrap();
    assert!(status.success(), "djvm -c failed");
    out
}

#[test]
fn djvu_pages_list_and_render() {
    if !djvulibre_available() {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "comicrust-djvu-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = build_two_page_djvu(&dir);

    let provider = ComicProvider::open(&path).unwrap();
    assert_eq!(provider.format().name, "DjVu Document");
    assert_eq!(provider.page_count(), 2);
    let page = provider.read_page(1).unwrap();
    assert_eq!(&page[..2], &[0xff, 0xd8], "JPEG SOI marker");
    assert_eq!(provider.create_hash().len(), 32);

    std::fs::remove_dir_all(&dir).ok();
}
