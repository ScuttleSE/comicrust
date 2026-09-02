//! Smart-list matchers: the spec registry, the typed matcher tree, and
//! the `Match` query language parser/renderer.
//!
//! - `spec`: every concrete C# matcher class (class name, English
//!   description, kind, operators, argument count).
//! - `tree`: the typed matcher tree the engine evaluates.
//! - `query`: parse a `Match` string into a tree, render a tree back —
//!   byte-stable round trip.
//! - `series`: series-statistics values for the `SmartListSeries*`
//!   matchers.

pub mod book_view;
pub mod eval;
pub mod query;
pub mod series;
pub mod spec;
pub mod text_number;
pub mod tree;

pub use query::{parse_smart_list_query, render_smart_list_query, SmartListQuery};
pub use tree::{GroupMatcher, Matcher, ValueMatcher};
