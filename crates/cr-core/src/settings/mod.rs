//! The settings layer: the `IniFile` port, the typed field registry,
//! `EngineConfiguration`, `ExtendedSettings`, and the main `Settings`
//! object — persisted in the ONE unified config file
//! `~/.config/comicrust/comicrust.toml` (ADR-033); the database,
//! caches, and scripts live in `~/.local/share/comicrust`
//! (`cr_core::paths`).

pub mod engine_config;
pub mod enums;
pub mod extended;
pub mod ini;
pub mod registry;
// The C# Settings object (the module name mirrors the type).
#[allow(clippy::module_inception)]
pub mod settings;
// The unified config file (`comicrust.toml`, ADR-033).
pub mod unified;
// The persisted workspace (`Settings.CurrentWorkspace`).
pub mod workspace;

pub use engine_config::EngineConfiguration;
pub use extended::ExtendedSettings;
pub use ini::IniValues;
pub use settings::Settings;
pub use workspace::WorkspaceState;
