//! The scraper module layer: native ports of former ComicRack plugins
//! (ADR-031). The first module is the Comic Vine Scraper
//! (`https://github.com/cbanack/comic-vine-scraper`, Apache-2.0,
//! Cory Banack) — the filename parser, configuration, ComicVine API
//! client, matching, and scrape engine live here; the cr-ui crate
//! owns the dialogs and the wiring.
//!
//! Attribution: this crate is a from-scratch Rust port of the Comic
//! Vine Scraper add-on for ComicRack by Cory Banack, licensed under
//! the Apache 2.0 license
//! (`https://www.apache.org/licenses/LICENSE-2.0.html`). The ComicVine
//! API (`https://comicvine.gamespot.com/api/`) is a free service that
//! requires a user-supplied key.

pub mod bookdata;
pub mod config;
pub mod cv;
pub mod engine;
pub mod fnameparser;
pub mod log;
pub mod matching;
pub mod utils;
