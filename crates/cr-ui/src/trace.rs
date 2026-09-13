//! Env-gated trace (`CR_TRACE=1`) — the standing env-probe pattern
//! (the `CR_DEBUG_SL` lesson). Zero output unless the variable is
//! set; safe to ship in release builds so a user report carries the
//! evidence. Output goes to stderr, one line per event, prefixed
//! `[trace]`.

/// True when `CR_TRACE` is set in the environment.
pub fn enabled() -> bool {
    std::env::var("CR_TRACE").is_ok()
}

/// One trace line (no-op unless `CR_TRACE` is set). The line carries
/// the monotonic seconds since the first trace line, so the GAPS
/// between events are visible in a user log (the 2026-09-13
/// smart-list freeze: the pasted lines had no times, the 25-30 s
/// sat between them, invisible).
pub fn trace(msg: impl AsRef<str>) {
    if enabled() {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let t = START.get_or_init(std::time::Instant::now).elapsed();
        eprintln!("[trace t=+{:>10.3}s] {}", t.as_secs_f64(), msg.as_ref());
    }
}
