//! The plugin's own test vector file (`test_fnameparser.data`, 218
//! cases) drives the port — the same loader semantics as the
//! plugin's `test_fnameparser.py`.

use std::path::Path;

use cr_scrape::fnameparser;

fn load_cases() -> Vec<[String; 4]> {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/fnameparser.data"),
    )
    .expect("test fixture missing");
    let mut cases = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // the three/four strings on a line share one quote character
        let quote = match line.chars().find(|c| *c == '"' || *c == '\'') {
            Some(q) => q,
            None => continue,
        };
        let mut items: Vec<String> = Vec::new();
        let mut rest = line;
        while let Some(i) = rest.find(quote) {
            let after = &rest[i + 1..];
            match after.find(quote) {
                Some(j) => {
                    items.push(after[..j].to_string());
                    rest = &after[j + 1..];
                }
                None => break,
            }
        }
        if items.len() == 3 {
            items.push(String::new());
        }
        assert_eq!(items.len(), 4, "badly formatted test data: {line}");
        cases.push([
            items[0].clone(),
            items[1].clone(),
            items[2].clone(),
            items[3].clone(),
        ]);
    }
    cases
}

#[test]
fn parses_the_plugin_test_vectors() {
    let cases = load_cases();
    assert_eq!(cases.len(), 218, "unexpected fixture size");
    let mut failures = 0;
    for case in &cases {
        let [filename, expected_series, expected_issue, expected_year] = case;
        let actual = fnameparser::extract(filename);
        let ok = actual[0] == *expected_series
            && actual[1] == *expected_issue
            && actual[2] == *expected_year;
        if !ok {
            failures += 1;
            eprintln!(
                "error parsing filename {filename:?}\n   --> got series {:?}, issue {:?} and year {:?} (expected {:?}, {:?}, {:?})",
                actual[0], actual[1], actual[2], expected_series, expected_issue, expected_year
            );
        }
    }
    assert_eq!(failures, 0, "{failures} case(s) failed; see stderr");
}
#[test]
fn user_regex_extracts_the_named_groups() {
    // Python re.match semantics: anchored at the start, partial match
    let case = r"(?P<series>.*?)\s*(?P<num>\d+)\s*\((?P<year>\d{4})\)";
    let got = fnameparser::regex("Batman 12 (2011)", case).unwrap();
    assert_eq!(
        got,
        ["Batman".to_string(), "12".to_string(), "2011".to_string()]
    );

    // num and year are optional; only a non-blank series is required
    let case = r"(?P<series>[^-]+)";
    let got = fnameparser::regex("Abe Sapien - Dark and Terrible", case).unwrap();
    assert_eq!(
        got,
        ["Abe Sapien ".to_string(), "".to_string(), "".to_string()]
    );

    // no series group -> None
    assert!(fnameparser::regex("Batman 12", r"\d+").is_none());
    // no match at the start -> None
    assert!(fnameparser::regex("Batman 12 (2011)", r"\((?P<year>\d{4})\)").is_none());
}

#[test]
fn failed_regexes_are_remembered() {
    // unique marker so parallel tests cannot interfere with the
    // module-level failed-regex cache
    let broken = "((broken-regex-cache-42-";
    assert!(fnameparser::regex("x", broken).is_none());
    // a regex that failed once returns None immediately afterwards
    assert!(fnameparser::regex("Batman 12", broken).is_none());
    // a different regex still works
    let got = fnameparser::regex("Batman 12", r"(?P<series>\w+)\s+(?P<num>\d+)").unwrap();
    assert_eq!(
        got,
        ["Batman".to_string(), "12".to_string(), "".to_string()]
    );
}
