//! Env-gated trace — the shared epoch lives in `cr-core::trace`, so
//! engine and UI lines share one clock. Re-exported to keep the
//! `crate::trace::…` call sites unchanged.

pub use cr_core::trace::{enabled, trace};
