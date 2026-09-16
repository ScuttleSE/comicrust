//! Smart-list parser and matchers, queue manager, scanner, watch folders,
//! backup, sync, and the remote server.

pub mod backup;
pub mod display_text;
pub mod duplicates;
pub mod gauges;
pub mod group;
pub mod image_pool;
pub mod incoming;
pub mod library;
pub mod lists;
pub mod matcher;
pub mod path_migration;
pub mod queue;
pub mod queue_manager;
pub mod reading_list;
pub mod scanner;
pub mod smart_list;
pub mod sort;
pub mod text;
pub mod tokenizer;
pub mod watch;

/// The env-gated trace — re-exported from cr-core (one shared epoch
/// with the UI lines).
pub use cr_core::trace;
