//! The settings layer: the `IniFile` port, the typed field registry,
//! `EngineConfiguration`, `ExtendedSettings`, and the main `Settings`
//! object (`Config.xml`).
//!
//! Layout (ADR-023): `Config.xml` and `comicrust.ini` live in
//! `~/.config/comicrust`; the database, caches, and scripts live in
//! `~/.local/share/comicrust` (`cr_core::paths`).

pub mod engine_config;
pub mod enums;
pub mod extended;
pub mod ini;
pub mod registry;
// The C# Settings object (the module name mirrors the type).
#[allow(clippy::module_inception)]
pub mod settings;

pub use engine_config::EngineConfiguration;
pub use extended::ExtendedSettings;
pub use ini::IniValues;
pub use settings::Settings;
