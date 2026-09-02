//! cr-core: data model, ComicDb.xml serialization, settings, filename
//! parsing, and the property registry.
//!
//! The XML layer reproduces the output of the C# `XmlSerializer` used by
//! ComicRackCE (net48). See `xml` module docs for the exact rules.

pub mod database;
pub mod model;
pub mod registry;
pub mod xml;
