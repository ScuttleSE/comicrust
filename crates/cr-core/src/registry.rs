//! Property registry: string name → typed getter/setter for ComicBook
//! properties, mirroring the reflection access the C# app relies on
//! (matchers, columns, remote `UpdateComic`, options panels).
//!
//! Names are the exact C# property names (`Series`, `CurrentPage`,
//! `ShadowYear`-style computed names come later with the engine).

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::model::comic_book::ComicBook;
use crate::xml::scalar::{CrDateTime, CrGuid};

/// Typed property value (`PropValue`).
#[derive(Clone, Debug, PartialEq)]
pub enum PropValue {
    Str(String),
    Int(i64),
    Float(f32),
    Bool(bool),
    Guid(CrGuid),
    Date(CrDateTime),
}

type Getter = fn(&ComicBook) -> PropValue;
type Setter = fn(&mut ComicBook, &PropValue) -> Result<(), PropError>;

/// A registry entry.
#[derive(Clone, Copy)]
pub struct PropertyDef {
    pub get: Getter,
    pub set: Setter,
}

/// Setter/getter errors (type mismatch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropError(pub String);

impl std::fmt::Display for PropError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for PropError {}

fn expect_str(v: &PropValue) -> Result<String, PropError> {
    match v {
        PropValue::Str(s) => Ok(s.clone()),
        other => Err(PropError(format!("expected string, got {other:?}"))),
    }
}

fn expect_int(v: &PropValue) -> Result<i64, PropError> {
    match v {
        PropValue::Int(i) => Ok(*i),
        other => Err(PropError(format!("expected int, got {other:?}"))),
    }
}

fn expect_f32(v: &PropValue) -> Result<f32, PropError> {
    match v {
        PropValue::Float(f) => Ok(*f),
        PropValue::Int(i) => Ok(*i as f32),
        other => Err(PropError(format!("expected float, got {other:?}"))),
    }
}

fn expect_bool(v: &PropValue) -> Result<bool, PropError> {
    match v {
        PropValue::Bool(b) => Ok(*b),
        other => Err(PropError(format!("expected bool, got {other:?}"))),
    }
}

fn expect_guid(v: &PropValue) -> Result<CrGuid, PropError> {
    match v {
        PropValue::Guid(g) => Ok(*g),
        other => Err(PropError(format!("expected guid, got {other:?}"))),
    }
}

fn expect_date(v: &PropValue) -> Result<CrDateTime, PropError> {
    match v {
        PropValue::Date(d) => Ok(*d),
        other => Err(PropError(format!("expected date, got {other:?}"))),
    }
}

macro_rules! push_str_info {
    ($v:ident; $($name:literal => $access:ident),+ $(,)?) => {
        $( $v.push(($name, PropertyDef {
            get: |b| PropValue::Str(b.info.$access.clone()),
            set: |b, val| {
                b.info.$access = expect_str(val)?;
                Ok(())
            },
        })); )+
    };
}

macro_rules! push_str_book {
    ($v:ident; $($name:literal => $access:ident),+ $(,)?) => {
        $( $v.push(($name, PropertyDef {
            get: |b| PropValue::Str(b.$access.clone()),
            set: |b, val| {
                b.$access = expect_str(val)?;
                Ok(())
            },
        })); )+
    };
}

macro_rules! push_int_info {
    ($v:ident; $($name:literal => $access:ident),+ $(,)?) => {
        $( $v.push(($name, PropertyDef {
            get: |b| PropValue::Int(b.info.$access as i64),
            set: |b, val| {
                b.info.$access = expect_int(val)? as i32;
                Ok(())
            },
        })); )+
    };
}

macro_rules! push_int_book {
    ($v:ident; $($name:literal => $access:ident),+ $(,)?) => {
        $( $v.push(($name, PropertyDef {
            get: |b| PropValue::Int(b.$access as i64),
            set: |b, val| {
                b.$access = expect_int(val)? as i32;
                Ok(())
            },
        })); )+
    };
}

/// The ordered property table (ComicInfo + ComicBook scalars). Order is
/// the C# declaration order, kept stable for serialization output.
pub fn entries() -> &'static Vec<(&'static str, PropertyDef)> {
    static ENTRIES: OnceLock<Vec<(&'static str, PropertyDef)>> = OnceLock::new();
    ENTRIES.get_or_init(build_entries)
}

/// The string-name → property map.
pub fn registry() -> &'static HashMap<&'static str, PropertyDef> {
    static REG: OnceLock<HashMap<&'static str, PropertyDef>> = OnceLock::new();
    REG.get_or_init(|| entries().iter().copied().collect())
}

fn build_entries() -> Vec<(&'static str, PropertyDef)> {
    let mut entries: Vec<(&'static str, PropertyDef)> = Vec::new();
    push_str_info!(entries;
        "Title" => title,
        "Series" => series,
        "Number" => number,
        "AlternateSeries" => alternate_series,
        "AlternateNumber" => alternate_number,
        "StoryArc" => story_arc,
        "SeriesGroup" => series_group,
        "Summary" => summary,
        "Notes" => notes,
        "Review" => review,
        "Writer" => writer,
        "Penciller" => penciller,
        "Inker" => inker,
        "Colorist" => colorist,
        "Letterer" => letterer,
        "CoverArtist" => cover_artist,
        "Editor" => editor,
        "Translator" => translator,
        "Publisher" => publisher,
        "Imprint" => imprint,
        "Genre" => genre,
        "Web" => web,
        "LanguageISO" => language_iso,
        "Format" => format,
        "AgeRating" => age_rating,
        "Characters" => characters,
        "Teams" => teams,
        "MainCharacterOrTeam" => main_character_or_team,
        "Locations" => locations,
        "ScanInformation" => scan_information,
        "Tags" => tags,
    );
    push_str_book!(entries;
        "FilePath" => file_path,
        "BookAge" => book_age,
        "BookCondition" => book_condition,
        "BookStore" => book_store,
        "BookOwner" => book_owner,
        "BookCollectionStatus" => book_collection_status,
        "BookNotes" => book_notes,
        "BookLocation" => book_location,
        "ISBN" => isbn,
    );
    push_int_info!(entries;
        "Count" => count,
        "Volume" => volume,
        "AlternateCount" => alternate_count,
        "Year" => year,
        "Month" => month,
        "Day" => day,
        "PageCount" => page_count,
        "PreferredFrontCover" => preferred_front_cover,
    );
    push_int_book!(entries;
        "OpenCount" => opened_count,
        "CurrentPage" => current_page,
        "LastPageRead" => last_page_read,
        "NewPages" => new_pages,
    );
    entries.push((
        "FileSize",
        PropertyDef {
            get: |b| PropValue::Int(b.file_size),
            set: |b, v| {
                b.file_size = expect_int(v)?;
                Ok(())
            },
        },
    ));
    entries.push((
        "Rating",
        PropertyDef {
            get: |b| PropValue::Float(b.rating),
            set: |b, v| {
                b.rating = expect_f32(v)?.clamp(0.0, 5.0);
                Ok(())
            },
        },
    ));
    entries.push((
        "CommunityRating",
        PropertyDef {
            get: |b| PropValue::Float(b.info.community_rating),
            set: |b, v| {
                b.info.community_rating = expect_f32(v)?.clamp(0.0, 5.0);
                Ok(())
            },
        },
    ));
    entries.push((
        "Id",
        PropertyDef {
            get: |b| PropValue::Guid(b.id),
            set: |b, v| {
                b.id = expect_guid(v)?;
                Ok(())
            },
        },
    ));
    entries.push((
        "AddedTime",
        PropertyDef {
            get: |b| PropValue::Date(b.added_time),
            set: |b, v| {
                b.added_time = expect_date(v)?;
                Ok(())
            },
        },
    ));
    entries.push((
        "ReleasedTime",
        PropertyDef {
            get: |b| PropValue::Date(b.released_time),
            set: |b, v| {
                b.released_time = expect_date(v)?;
                Ok(())
            },
        },
    ));
    entries.push((
        "OpenedTime",
        PropertyDef {
            get: |b| PropValue::Date(b.opened_time),
            set: |b, v| {
                b.opened_time = expect_date(v)?;
                Ok(())
            },
        },
    ));
    entries.push((
        "Checked",
        PropertyDef {
            get: |b| PropValue::Bool(b.checked),
            set: |b, v| {
                b.checked = expect_bool(v)?;
                Ok(())
            },
        },
    ));
    entries.push((
        "EnableProposed",
        PropertyDef {
            get: |b| PropValue::Bool(b.enable_proposed),
            set: |b, v| {
                b.enable_proposed = expect_bool(v)?;
                Ok(())
            },
        },
    ));
    entries.push((
        "EnableDynamicUpdate",
        PropertyDef {
            get: |b| PropValue::Bool(b.enable_dynamic_update),
            set: |b, v| {
                b.enable_dynamic_update = expect_bool(v)?;
                Ok(())
            },
        },
    ));
    entries
}

/// Gets a property by C# name. Unknown names return `None` (the C#
/// reflection layer would throw; callers decide).
pub fn get(book: &ComicBook, name: &str) -> Option<PropValue> {
    registry().get(name).map(|d| (d.get)(book))
}

/// Sets a property by C# name.
pub fn set(book: &mut ComicBook, name: &str, value: &PropValue) -> Option<Result<(), PropError>> {
    registry().get(name).map(|d| (d.set)(book, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_through_registry() {
        let mut b = ComicBook::default();
        set(&mut b, "Series", &PropValue::Str("Test".into()))
            .unwrap()
            .unwrap();
        set(&mut b, "Count", &PropValue::Int(12)).unwrap().unwrap();
        set(&mut b, "Rating", &PropValue::Float(9.0))
            .unwrap()
            .unwrap();
        assert_eq!(get(&b, "Series"), Some(PropValue::Str("Test".into())));
        assert_eq!(get(&b, "Count"), Some(PropValue::Int(12)));
        // clamped like the C# setter
        assert_eq!(get(&b, "Rating"), Some(PropValue::Float(5.0)));
        assert_eq!(get(&b, "Nonexistent"), None);
        // wrong type fails
        assert!(set(&mut b, "Series", &PropValue::Int(1)).unwrap().is_err());
    }
}
