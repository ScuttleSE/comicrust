//! Integration tests running the cr-cli binary against the golden
//! fixtures.

use std::path::Path;
use std::process::Command;

fn golden_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .canonicalize()
        .expect("golden dir exists")
}

#[test]
fn db_roundtrip_reports_identical() {
    for name in ["db-small.xml", "db-large.xml", "db-net-reference.xml"] {
        let out = Command::new(env!("CARGO_BIN_EXE_cr-cli"))
            .args(["db-roundtrip", &golden_dir().join(name).to_string_lossy()])
            .output()
            .expect("cr-cli runs");
        assert!(out.status.success(), "{name}: exit {:?}", out.status.code());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("IDENTICAL"), "{name}: {stdout}");
    }
}

#[test]
fn db_dump_summarizes() {
    let out = Command::new(env!("CARGO_BIN_EXE_cr-cli"))
        .args([
            "db-dump",
            &golden_dir().join("db-large.xml").to_string_lossy(),
        ])
        .output()
        .expect("cr-cli runs");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON summary");
    assert_eq!(v["BookCount"], 2);
    assert_eq!(v["BlackListCount"], 1);
    assert_eq!(v["ComicLists"][0]["Name"], "Smart Lists");
}

#[test]
fn info_proposes_from_filename() {
    let out = Command::new(env!("CARGO_BIN_EXE_cr-cli"))
        .args([
            "info",
            "/comics/Amazing Adventures vol 2 014 of 20 (2019).cbz",
        ])
        .output()
        .expect("cr-cli runs");
    assert!(out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("valid JSON");
    assert_eq!(v["Source"], "filename");
    assert_eq!(v["Series"], "Amazing Adventures");
    assert_eq!(v["Number"], "14");
    assert_eq!(v["Volume"], 2);
    assert_eq!(v["Count"], 20);
    assert_eq!(v["Year"], 2019);
}

#[test]
fn info_reads_comicinfo_xml() {
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<ComicInfo xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema">
  <Series>Test Series</Series>
  <Number>3</Number>
  <PageCount>22</PageCount>
</ComicInfo>"#;
    let tmp = std::env::temp_dir().join("comicrust-cli-info.xml");
    std::fs::write(&tmp, xml).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_cr-cli"))
        .args(["info", &tmp.to_string_lossy()])
        .output()
        .expect("cr-cli runs");
    std::fs::remove_file(&tmp).ok();
    assert!(out.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("valid JSON");
    assert_eq!(v["Series"], "Test Series");
    assert_eq!(v["Number"], "3");
    assert_eq!(v["PageCount"], 22);
    // Unknown fields keep the C# defaults.
    assert_eq!(v["Count"], -1);
    assert_eq!(v["Volume"], -1);
}

#[test]
fn db_roundtrip_fails_on_corrupt_file() {
    let tmp = std::env::temp_dir().join("comicrust-cli-corrupt.xml");
    std::fs::write(&tmp, b"<ComicDatabase><Books>nope").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_cr-cli"))
        .args(["db-roundtrip", &tmp.to_string_lossy()])
        .output()
        .expect("cr-cli runs");
    std::fs::remove_file(&tmp).ok();
    assert_eq!(out.status.code(), Some(2), "error exit code");
}
