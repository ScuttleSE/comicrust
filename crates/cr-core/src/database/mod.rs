//! Database layer: ComicDatabase root, ComicLists tree, display config.

pub mod comic_database;
pub mod display_config;
pub mod list_items;
pub mod reading_list;

pub use comic_database::{
    create_new, load, open_with_fallback, save, save_bytes, ComicDatabase, DbError, OpenStatus,
};
