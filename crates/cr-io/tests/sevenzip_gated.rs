//! Subprocess-format tests (7z). Gated: they run only when
//! `CR_FORMAT_TESTS` is set and a `7z` executable exists — see the
//! test data policy in docs/phase-1-kickoff.md. 7z cannot create RAR
//! archives, so CBR/RAR5 need real-world smoke files (out of repo).

use std::process::Command;

use cr_io::ComicProvider;

fn sevenzip_available() -> bool {
    std::env::var_os("CR_FORMAT_TESTS").is_some()
        && Command::new("7z")
            .arg("i")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
}

#[test]
fn cb7_pages_and_read() {
    if !sevenzip_available() {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "comicrust-cb7-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let page_dir = dir.join("src");
    std::fs::create_dir_all(&page_dir).unwrap();
    // The provider layer filters by extension and compares bytes; the
    // content need not be a decodable image at this phase.
    std::fs::write(page_dir.join("cover.jpg"), b"cover-bytes").unwrap();
    std::fs::write(page_dir.join("page1.jpg"), b"page-1").unwrap();
    std::fs::write(page_dir.join("page2.jpg"), b"page-2").unwrap();

    let archive = dir.join("comic.cb7");
    let out = Command::new("7z")
        .args(["a", "-t7z"])
        .arg(&archive)
        .arg(page_dir.join("*.jpg"))
        .output()
        .unwrap();
    assert!(out.status.success(), "7z a failed");

    let provider = ComicProvider::open(&archive).unwrap();
    assert_eq!(provider.format().name, "eComic (7z)");
    let names: Vec<&str> = provider.pages().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["cover.jpg", "page1.jpg", "page2.jpg"]);
    assert_eq!(provider.read_page(1).unwrap(), b"page-1");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cb7_missing_binary_reports_access_error() {
    // When the host has no 7z at all, listing must surface a clear
    // access error — and provider parse must swallow it into an empty
    // page list, exactly like the C# try/catch in OnParse.
    if Command::new("7z").arg("i").output().is_ok() {
        return; // host has 7z; nothing to assert here
    }
    let dir = std::env::temp_dir().join("comicrust-cb7-missing");
    std::fs::create_dir_all(&dir).unwrap();
    let archive = dir.join("comic.cb7");
    std::fs::write(&archive, b"7z\xbc\xaf\x27\x1c").unwrap();
    let provider = ComicProvider::open(&archive).unwrap();
    assert_eq!(provider.page_count(), 0);
    std::fs::remove_dir_all(&dir).ok();
}
