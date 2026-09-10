// The About version (`0.0.<commits>` per ADR-020): the release
// workflow sets `VERSION`; a dev build falls back to the local git
// commit count, then to a plain marker.
//
// The build script MUST watch the git ref files: with any
// rerun-if-* directive emitted, cargo drops the default "rerun when
// any package file changes" and reruns only on declared triggers —
// so without these the stamped count FREEZES at the last full
// rebuild (measured: 0.0.233 rode along through 70+ commits while
// every incremental rebuild reused the cached build-script output).
fn main() {
    println!("cargo:rerun-if-env-changed=VERSION");
    watch_head();
    let version = match std::env::var("VERSION") {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => git_count_version(),
    };
    println!("cargo:rustc-env=COMICRUST_VERSION={version}");
}

/// Declares the git-dir ref files as build-script inputs: HEAD
/// (branch switches, detached checkouts), the current branch's ref
/// (new commits, pulls), and packed-refs (post-gc ref packing).
/// Missing files watch fine (a later creation counts as a change).
fn watch_head() {
    let dir = std::process::Command::new("git")
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(dir) = dir else {
        return;
    };
    println!("cargo:rerun-if-changed={dir}/HEAD");
    println!("cargo:rerun-if-changed={dir}/packed-refs");
    let branch = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty() && s != "HEAD");
    if let Some(branch) = branch {
        println!("cargo:rerun-if-changed={dir}/refs/heads/{branch}");
    }
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
