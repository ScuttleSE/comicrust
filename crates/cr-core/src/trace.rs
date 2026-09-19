//! Env-gated trace (`CR_TRACE=1`) — the standing env-probe pattern
//! (the `CR_DEBUG_SL` lesson). Zero output unless the variable is
//! set; safe to ship in release builds so a user report carries the
//! evidence. Output goes to stderr, one line per event, prefixed
//! `[trace]`.
//!
//! ONE monotonic epoch serves every crate that reports (cr-core,
//! cr-engine, cr-ui): the clock starts at the FIRST trace line, so
//! the gaps between events in a user log are directly readable (the
//! 2026-09-13 smart-list freeze and the 2026-09-14 startup report:
//! the seconds sat between pasted lines, invisible).

/// True when `CR_TRACE` is set in the environment.
pub fn enabled() -> bool {
    std::env::var("CR_TRACE").is_ok()
}

/// A stable diagnostic label for the current thread.
pub fn thread_label() -> String {
    let thread = std::thread::current();
    format!("{}:{:?}", thread.name().unwrap_or("unnamed"), thread.id())
}

/// One trace line (no-op unless `CR_TRACE` is set). The line carries
/// the monotonic seconds since the first trace line.
pub fn trace(msg: impl AsRef<str>) {
    if enabled() {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let t = START.get_or_init(std::time::Instant::now).elapsed();
        eprintln!("[trace t=+{:>10.3}s] {}", t.as_secs_f64(), msg.as_ref());
    }
}
