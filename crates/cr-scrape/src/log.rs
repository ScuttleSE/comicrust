//! Env-gated scrape logging (the C# plugin's debug-stream parity:
//! `log.debug` lines the user can turn on). `CR_SCRAPE_DEBUG=1`
//! prints each query, response, and engine decision to stderr; the
//! API key never prints.

use std::sync::OnceLock;

static ENABLED: OnceLock<bool> = OnceLock::new();

/// True when `CR_SCRAPE_DEBUG` is set to a non-empty, non-`0` value.
pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| {
        std::env::var("CR_SCRAPE_DEBUG")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    })
}

/// One debug line (no-op unless [`enabled`]).
pub fn debug(msg: &str) {
    if enabled() {
        eprintln!("[cv-scrape] {msg}");
    }
}
