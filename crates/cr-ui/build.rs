// The About version (`0.0.<commits>` per ADR-020): the release
// workflow sets `VERSION`; a dev build falls back to the local git
// commit count, then to a plain marker.
fn main() {
    println!("cargo:rerun-if-env-changed=VERSION");
    let version = match std::env::var("VERSION") {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => git_count_version(),
    };
    println!("cargo:rustc-env=COMICRUST_VERSION={version}");
}

fn git_count_version() -> String {
    let count = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    match count {
        Some(n) => format!("0.0.{n}"),
        None => "0.0.dev".to_string(),
    }
}
