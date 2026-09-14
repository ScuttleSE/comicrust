//! cr-organize — the Library Organizer native module (ADR-031).
//!
//! A port of the ComicRack Library Organizer plugin
//! (Stonepaw, v2.1, Apache-2.0): rule- and token-based file renaming
//! and moving, driven by per-profile templates. Phase 17.
//!
//! The crate is engine-only: no GTK, no session access. The UI side
//! (cr-ui) drives it on a worker thread and applies the results on the
//! main thread.

pub mod engine;
pub mod fields;
pub mod mover;
pub mod profile;
pub mod rules;
pub mod series;
pub mod template;
