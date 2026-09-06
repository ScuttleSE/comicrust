//! Env-gated trace (`CR_TRACE=1`) — the standing env-probe pattern
//! (the `CR_DEBUG_SL` lesson). Zero output unless the variable is
//! set; safe to ship in release builds so a user report carries the
//! evidence. Output goes to stderr, one line per event, prefixed
//! `[trace]`.

/// True when `CR_TRACE` is set in the environment.
pub fn enabled() -> bool {
    std::env::var("CR_TRACE").is_ok()
}

/// One trace line (no-op unless `CR_TRACE` is set).
pub fn trace(msg: impl AsRef<str>) {
    if enabled() {
        eprintln!("[trace] {}", msg.as_ref());
    }
}
