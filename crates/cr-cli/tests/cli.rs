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

#[test]
fn pages_and_extract_on_cbz() {
    let dir = std::env::temp_dir().join("comicrust-cli-cbz");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("book.cbz");
    {
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options: zip::write::SimpleFileOptions = Default::default();
        for (name, data) in [
            ("c.jpg", b"page-c".as_slice()),
            ("b.jpg", b"page-b".as_slice()),
            ("a.jpg", b"page-a".as_slice()),
            ("ComicInfo.xml", b"<ComicInfo />".as_slice()),
        ] {
            zip.start_file(name, options).unwrap();
            std::io::Write::write_all(&mut zip, data).unwrap();
        }
        zip.finish().unwrap();
    }

    let out = Command::new(env!("CARGO_BIN_EXE_cr-cli"))
        .args(["pages", &path.to_string_lossy()])
        .output()
        .expect("cr-cli runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("valid JSON");
    assert_eq!(v["Format"], "eComic (ZIP)");
    assert_eq!(v["PageCount"], 3);
    let names: Vec<&str> = v["Pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["Name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["a.jpg", "b.jpg", "c.jpg"]);

    let out_path = dir.join("page.bin");
    let out = Command::new(env!("CARGO_BIN_EXE_cr-cli"))
        .args([
            "extract",
            &path.to_string_lossy(),
            "1",
            "-o",
            &out_path.to_string_lossy(),
        ])
        .output()
        .expect("cr-cli runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read(&out_path).unwrap(), b"page-b");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn migrate_copies_and_maps_a_ce_profile() {
    let root = golden_dir();
    let realworld = root
        .join("../realworld/ComicDb.xml")
        .canonicalize()
        .expect("the real-world fixture is committed");
    let work = std::env::temp_dir().join(format!("crcli-migrate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    // The CE profile layout: <profile>/ComicDb/ComicDb.xml + an ini
    // with one consumed key and one unknown key.
    let profile = work.join("profile");
    std::fs::create_dir_all(profile.join("ComicDb")).unwrap();
    std::fs::copy(&realworld, profile.join("ComicDb").join("ComicDb.xml")).unwrap();
    std::fs::write(
        profile.join("ComicRack.ini"),
        "[Settings]\nCachePath=/tmp/ce-cache\nNotAPortKey=1\n",
    )
    .unwrap();

    // The dry run writes nothing.
    let out = Command::new(env!("CARGO_BIN_EXE_cr-cli"))
        .args([
            "migrate",
            profile.to_str().unwrap(),
            "--out",
            work.join("out").to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "{stdout}");
    assert!(stdout.contains("WOULD COPY"), "{stdout}");
    assert!(stdout.contains("CachePath=/tmp/ce-cache"), "{stdout}");
    assert!(!work.join("out").exists(), "the dry run must not write");

    // The real run: the copy lands, the ini keys map (only the
    // consumed ones), the Windows-path note prints.
    let out = Command::new(env!("CARGO_BIN_EXE_cr-cli"))
        .args([
            "migrate",
            profile.to_str().unwrap(),
            "--out",
            work.join("out").to_str().unwrap(),
        ])
        .env("XDG_CONFIG_HOME", work.join("config"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "{stdout}");
    assert!(stdout.contains("COPIED"), "{stdout}");
    assert!(stdout.contains("255 books"), "{stdout}");
    assert!(stdout.contains("Windows paths"), "{stdout}");
    assert!(
        !stdout.contains("NotAPortKey"),
        "unknown ini keys must not map: {stdout}"
    );
    let copied = std::fs::read(work.join("out")).unwrap();
    let source = std::fs::read(&realworld).unwrap();
    assert_eq!(copied, source, "the copy must be byte-identical");
    let toml =
        std::fs::read_to_string(work.join("config").join("comicrust").join("comicrust.toml"))
            .expect("the unified config written");
    assert!(toml.contains("[extended]"), "{toml}");
    assert!(toml.contains("CachePath = \"/tmp/ce-cache\""), "{toml}");
    assert!(!toml.contains("NotAPortKey"), "{toml}");

    // A second run refuses without --force.
    let out = Command::new(env!("CARGO_BIN_EXE_cr-cli"))
        .args([
            "migrate",
            profile.to_str().unwrap(),
            "--out",
            work.join("out").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!out.status.success(), "the second run must refuse");

    std::fs::remove_dir_all(&work).ok();
}
