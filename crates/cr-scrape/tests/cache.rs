//! Gates for the Comic Vine disk cache (ADR-037, Phase 15 T1).

use cr_scrape::cache::{CvCache, IssueSkeleton, SqliteCache, SweepState, VolumeRow};

fn cache() -> SqliteCache {
    SqliteCache::in_memory().expect("in-memory cache opens")
}

fn vol(id: i64) -> VolumeRow {
    VolumeRow {
        volume_id: id,
        ..Default::default()
    }
}

fn issue(id: i64, volume_id: i64, number: &str) -> IssueSkeleton {
    IssueSkeleton {
        issue_id: id,
        volume_id,
        issue_number: number.to_string(),
        ..Default::default()
    }
}

#[test]
fn schema_migrates_from_empty_and_is_idempotent() {
    let dir = tempdir();
    let path = dir.join("cvcache.sqlite");
    {
        let c = SqliteCache::open(&path).expect("first open creates the file");
        c.put_volumes(&[vol(771)]).expect("write");
    }
    // A second open must migrate nothing away and must keep the data.
    let c = SqliteCache::open(&path).expect("second open");
    assert_eq!(c.volume(771).expect("read").map(|v| v.volume_id), Some(771));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn open_creates_the_parent_directory() {
    let dir = tempdir();
    let path = dir.join("a").join("b").join("cvcache.sqlite");
    SqliteCache::open(&path).expect("open creates the directory chain");
    assert!(path.exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn unknown_volume_and_issue_read_as_none() {
    let c = cache();
    assert_eq!(c.volume(1).expect("read"), None);
    assert!(c.issues_of_volume(1).expect("read").is_empty());
    assert_eq!(c.issue_count(1).expect("count"), 0);
    assert_eq!(c.issue_detail(1).expect("read"), None);
    assert_eq!(c.image("u").expect("read"), None);
    assert_eq!(c.search("t").expect("read"), None);
    assert_eq!(c.sweep_state().expect("read"), None);
}

#[test]
fn a_cheap_volume_write_does_not_erase_an_expensive_one() {
    let c = cache();
    // The detail query found everything.
    c.put_volumes(&[VolumeRow {
        volume_id: 771,
        name: Some("Blacksad".into()),
        publisher: Some("Dark Horse".into()),
        start_year: Some(2010),
        count_of_issues: Some(6),
        date_last_updated: Some("2020-01-02 03:04:05".into()),
        last_cover_date: Some("2013-06-01".into()),
        fetched_at: 500,
    }])
    .expect("write");
    // An MCL import knows only the id. It must erase nothing.
    c.put_volumes(&[vol(771)]).expect("write");
    let v = c.volume(771).expect("read").expect("present");
    assert_eq!(v.name.as_deref(), Some("Blacksad"));
    assert_eq!(v.publisher.as_deref(), Some("Dark Horse"));
    assert_eq!(v.start_year, Some(2010));
    assert_eq!(v.count_of_issues, Some(6));
    assert_eq!(v.last_cover_date.as_deref(), Some("2013-06-01"));
    // `fetched_at` keeps the newer of the two.
    assert_eq!(v.fetched_at, 500);
}

#[test]
fn a_newer_volume_write_wins_field_by_field() {
    let c = cache();
    c.put_volumes(&[vol(771)]).expect("write");
    c.put_volumes(&[VolumeRow {
        volume_id: 771,
        count_of_issues: Some(7),
        fetched_at: 900,
        ..Default::default()
    }])
    .expect("write");
    let v = c.volume(771).expect("read").expect("present");
    assert_eq!(v.count_of_issues, Some(7));
    assert_eq!(v.fetched_at, 900);
    assert_eq!(v.name, None);
}

#[test]
fn issues_return_in_issue_id_order() {
    let c = cache();
    // The MCL order is by issue id, not by issue number (ADR-038).
    c.put_issues(&[
        issue(165_276, 771, "3"),
        issue(92_469, 771, "1"),
        issue(92_470, 771, "2"),
        issue(11, 999, "1"),
    ])
    .expect("write");
    let got = c.issues_of_volume(771).expect("read");
    assert_eq!(
        got.iter().map(|i| i.issue_id).collect::<Vec<_>>(),
        vec![92_469, 92_470, 165_276]
    );
    assert_eq!(
        got.iter()
            .map(|i| i.issue_number.as_str())
            .collect::<Vec<_>>(),
        vec!["1", "2", "3"]
    );
    assert_eq!(c.issue_count(771).expect("count"), 3);
    assert_eq!(c.issue_count(999).expect("count"), 1);
}

#[test]
fn an_issue_number_keeps_its_text_form() {
    let c = cache();
    // Volume 77901 holds the issue number `1,5` (ADR-038).
    c.put_issues(&[issue(1, 77_901, "1,5"), issue(2, 77_901, "v. 1, no. 01")])
        .expect("write");
    let got = c.issues_of_volume(77_901).expect("read");
    assert_eq!(got[0].issue_number, "1,5");
    assert_eq!(got[1].issue_number, "v. 1, no. 01");
}

#[test]
fn an_issue_can_move_to_another_volume() {
    let c = cache();
    c.put_issues(&[issue(5, 100, "1")]).expect("write");
    c.put_issues(&[issue(5, 200, "1a")]).expect("write");
    assert_eq!(c.issue_count(100).expect("count"), 0);
    let got = c.issues_of_volume(200).expect("read");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].issue_number, "1a");
}

#[test]
fn a_cheap_issue_write_does_not_erase_the_cover_date() {
    let c = cache();
    c.put_issues(&[IssueSkeleton {
        issue_id: 5,
        volume_id: 100,
        issue_number: "1".into(),
        cover_date: Some("1998-03-01".into()),
        name: Some("Somewhere Within the Shadows".into()),
    }])
    .expect("write");
    // The sweep asks for `id,issue_number,volume` only (ADR-038).
    c.put_issues(&[issue(5, 100, "1")]).expect("write");
    let got = c.issues_of_volume(100).expect("read");
    assert_eq!(got[0].cover_date.as_deref(), Some("1998-03-01"));
    assert_eq!(got[0].name.as_deref(), Some("Somewhere Within the Shadows"));
}

#[test]
fn detail_image_and_search_round_trip_and_overwrite() {
    let c = cache();
    c.put_issue_detail(5, r#"{"id":5}"#).expect("write");
    let (json, at) = c.issue_detail(5).expect("read").expect("present");
    assert_eq!(json, r#"{"id":5}"#);
    assert!(at > 0, "fetched_at is stamped");
    c.put_issue_detail(5, r#"{"id":5,"name":"x"}"#)
        .expect("overwrite");
    assert_eq!(
        c.issue_detail(5).expect("read").expect("present").0,
        r#"{"id":5,"name":"x"}"#
    );

    c.put_image("http://cv/a.jpg", &[1, 2, 3]).expect("write");
    assert_eq!(
        c.image("http://cv/a.jpg").expect("read"),
        Some(vec![1, 2, 3])
    );
    c.put_image("http://cv/a.jpg", &[9]).expect("overwrite");
    assert_eq!(c.image("http://cv/a.jpg").expect("read"), Some(vec![9]));

    c.put_search("blacksad", "[]").expect("write");
    assert_eq!(
        c.search("blacksad").expect("read").map(|(j, _)| j),
        Some("[]".to_string())
    );
}

#[test]
fn request_accounting_counts_per_resource() {
    let c = cache();
    for at in [100, 200, 300] {
        c.log_request("issues", at).expect("log");
    }
    c.log_request("volumes", 250).expect("log");
    assert_eq!(c.requests_since("issues", 0).expect("count"), 3);
    assert_eq!(c.requests_since("issues", 200).expect("count"), 2);
    assert_eq!(c.requests_since("issues", 400).expect("count"), 0);
    // The budget is per resource, not a total.
    assert_eq!(c.requests_since("volumes", 0).expect("count"), 1);
    assert_eq!(c.requests_since("search", 0).expect("count"), 0);
}

#[test]
fn the_oldest_request_gives_the_resume_time() {
    let c = cache();
    assert_eq!(c.oldest_request_since("issues", 0).expect("read"), None);
    for at in [300, 100, 200] {
        c.log_request("issues", at).expect("log");
    }
    assert_eq!(
        c.oldest_request_since("issues", 0).expect("read"),
        Some(100)
    );
    assert_eq!(
        c.oldest_request_since("issues", 150).expect("read"),
        Some(200)
    );
    assert_eq!(c.oldest_request_since("issues", 999).expect("read"), None);
}

#[test]
fn pruning_removes_only_the_old_records() {
    let c = cache();
    for at in [100, 200, 300] {
        c.log_request("issues", at).expect("log");
    }
    c.prune_requests(250).expect("prune");
    assert_eq!(c.requests_since("issues", 0).expect("count"), 1);
    assert_eq!(
        c.oldest_request_since("issues", 0).expect("read"),
        Some(300)
    );
}

#[test]
fn the_sweep_state_holds_one_row_and_resumes() {
    let c = cache();
    let first = SweepState {
        start_date: "2026-08-26".into(),
        end_date: "2026-09-12".into(),
        offset: 100,
        total: 4200,
        updated_at: 1_000,
    };
    c.put_sweep_state(&first).expect("write");
    assert_eq!(c.sweep_state().expect("read"), Some(first));

    let resumed = SweepState {
        start_date: "2026-08-26".into(),
        end_date: "2026-09-12".into(),
        offset: 200,
        total: 4200,
        updated_at: 1_100,
    };
    c.put_sweep_state(&resumed).expect("write");
    assert_eq!(c.sweep_state().expect("read"), Some(resumed));
}

/// A private directory for one test. The crate has no `tempfile`
/// dependency and this keeps it that way. The counter makes the name
/// unique inside the process, so two tests never share a directory.
fn tempdir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "cr-scrape-cache-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::remove_dir_all(&p).ok();
    std::fs::create_dir_all(&p).expect("temp dir");
    p
}
