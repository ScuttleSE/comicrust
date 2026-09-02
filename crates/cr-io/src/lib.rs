//! Comic file IO: provider framework, archive readers, and (later in
//! Phase 1) metadata write-back.
//!
//! The C# specification is `ComicRack.Engine/IO/Provider/`. The port
//! mirrors its structure:
//!
//! - `formats` — `KnownFileFormats` + `FileFormat`/`FileFormatAttribute`
//!   and the provider factory's extension lookup.
//! - `extended_compare` — `cYo.Common.Text.ExtendedStringComparer` with
//!   the `IgnoreCase` mode that `ArchiveComicProvider.OnParse` uses to
//!   define page order.
//! - `provider` — `IComicAccessor`, `ProviderImageInfo`, and the
//!   archive-comic-provider page semantics (supported-image filter +
//!   natural sort).
//! - `accessors` — `ZipSharpZipEngine` and `TarSharpZipEngine` ports.
//! - `hash` — `ImageProvider.CreateHashFromImageList` (SHA-1, Base32).
//!
//! Documented tolerances against the .NET original:
//!
//! - The single-character culture comparisons inside
//!   `ExtendedStringComparer` become case-folded ordinal comparisons.
//!   For ASCII filenames (the overwhelming case) the order is
//!   identical; exotic Unicode may order differently.
//! - `char.IsDigit` becomes `is_ascii_digit` (ASCII digits decide page
//!   order; fullwidth or other Unicode decimal digits sort as
//!   ordinary characters).
//! - `FileUtility.MakeValidFilename` before extension matching is a
//!   no-op here; archive entry names rarely contain the Windows
//!   reserved characters it sanitizes.

pub mod accessors;
pub mod error;
pub mod extended_compare;
pub mod formats;
pub mod hash;
pub mod pdf;
pub mod provider;
pub mod sevenzip;

pub use error::{Error, Result};
pub use formats::FileFormat;
pub use provider::{ComicAccessor, ComicProvider, ProviderImageInfo};
