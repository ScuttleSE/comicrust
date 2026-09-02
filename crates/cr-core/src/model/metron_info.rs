//! `MetronInfo` — port of the generated `MetronInfo.cs` schema
//! (XmlSchemaClassGenerator output). Element order, `*Specified`
//! omission rules, and defaults mirror the C# classes exactly.
//!
//! Serialization notes (net48 `XmlSerializer` semantics):
//! - a member with a `XxxSpecified` companion is omitted when the
//!   flag is false (collections: `Count != 0`);
//! - members with `[DefaultValue]` are omitted when equal to it
//!   (PageCount 0, AgeRating Unknown, Series lang "en");
//! - `xs:date` text is `yyyy-MM-dd`, `xs:dateTime` uses the .NET
//!   round-trip forms (`CrDateTime`).

use crate::model::comic_info::ComicInfo;
use crate::xml::reader::{XmlError, XmlResult};
use crate::xml::scalar::CrDateTime;
use crate::xml::{Emitter, Start, Tok};
use chrono::Datelike;
use std::io::Write;

// --- enums ------------------------------------------------------------

/// `InformationSource` — `[XmlEnum]` names are the serialized text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InformationSource {
    AniList,
    ComicVine,
    GrandComicsDatabase,
    Kitsu,
    MangaDex,
    MangaUpdates,
    Marvel,
    Metron,
    MyAnimeList,
    LeagueOfComicGeeks,
}

impl InformationSource {
    pub const ALL: [Self; 10] = [
        Self::AniList,
        Self::ComicVine,
        Self::GrandComicsDatabase,
        Self::Kitsu,
        Self::MangaDex,
        Self::MangaUpdates,
        Self::Marvel,
        Self::Metron,
        Self::MyAnimeList,
        Self::LeagueOfComicGeeks,
    ];

    pub fn xml_name(self) -> &'static str {
        match self {
            Self::AniList => "AniList",
            Self::ComicVine => "Comic Vine",
            Self::GrandComicsDatabase => "Grand Comics Database",
            Self::Kitsu => "Kitsu",
            Self::MangaDex => "MangaDex",
            Self::MangaUpdates => "MangaUpdates",
            Self::Marvel => "Marvel",
            Self::Metron => "Metron",
            Self::MyAnimeList => "MyAnimeList",
            Self::LeagueOfComicGeeks => "League of Comic Geeks",
        }
    }

    pub fn from_xml(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.xml_name() == name)
    }
}

/// `FormatType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatType {
    Annual,
    DigitalChapter,
    GraphicNovel,
    Hardcover,
    LimitedSeries,
    Omnibus,
    OneShot,
    SingleIssue,
    TradePaperback,
}

impl FormatType {
    pub const ALL: [Self; 9] = [
        Self::Annual,
        Self::DigitalChapter,
        Self::GraphicNovel,
        Self::Hardcover,
        Self::LimitedSeries,
        Self::Omnibus,
        Self::OneShot,
        Self::SingleIssue,
        Self::TradePaperback,
    ];

    pub fn xml_name(self) -> &'static str {
        match self {
            Self::Annual => "Annual",
            Self::DigitalChapter => "Digital Chapter",
            Self::GraphicNovel => "Graphic Novel",
            Self::Hardcover => "Hardcover",
            Self::LimitedSeries => "Limited Series",
            Self::Omnibus => "Omnibus",
            Self::OneShot => "One-Shot",
            Self::SingleIssue => "Single Issue",
            Self::TradePaperback => "Trade Paperback",
        }
    }

    pub fn from_xml(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.xml_name() == name)
    }

    /// English default of `LocalizeUtility.LocalizeEnum`:
    /// `Enum.GetName(...)` through `PascalToSpaced`.
    pub fn localized_default(self) -> &'static str {
        match self {
            Self::Annual => "Annual",
            Self::DigitalChapter => "Digital Chapter",
            Self::GraphicNovel => "Graphic Novel",
            Self::Hardcover => "Hardcover",
            Self::LimitedSeries => "Limited Series",
            Self::Omnibus => "Omnibus",
            Self::OneShot => "One Shot",
            Self::SingleIssue => "Single Issue",
            Self::TradePaperback => "Trade Paperback",
        }
    }
}

/// `AgeRatingType` (Metron variant list, not the ComicInfo one).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AgeRatingType {
    #[default]
    Unknown,
    Everyone,
    Teen,
    TeenPlus,
    Mature,
    Explicit,
    Adult,
}

impl AgeRatingType {
    pub const ALL: [Self; 7] = [
        Self::Unknown,
        Self::Everyone,
        Self::Teen,
        Self::TeenPlus,
        Self::Mature,
        Self::Explicit,
        Self::Adult,
    ];

    pub fn xml_name(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Everyone => "Everyone",
            Self::Teen => "Teen",
            Self::TeenPlus => "Teen Plus",
            Self::Mature => "Mature",
            Self::Explicit => "Explicit",
            Self::Adult => "Adult",
        }
    }

    pub fn from_xml(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.xml_name() == name)
    }

    /// English default of `LocalizeUtility.LocalizeEnum`.
    pub fn localized_default(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Everyone => "Everyone",
            Self::Teen => "Teen",
            Self::TeenPlus => "Teen Plus",
            Self::Mature => "Mature",
            Self::Explicit => "Explicit",
            Self::Adult => "Adult",
        }
    }
}

/// `RoleValues`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleValues {
    Writer,
    Script,
    Story,
    Plot,
    Interviewer,
    Artist,
    Penciller,
    Breakdowns,
    Illustrator,
    Layouts,
    Inker,
    Embellisher,
    Finishes,
    InkAssists,
    Colorist,
    ColorSeparations,
    ColorAssists,
    ColorFlats,
    DigitalArtTechnician,
    GrayTone,
    Letterer,
    Cover,
    Editor,
    ConsultingEditor,
    AssistantEditor,
    AssociateEditor,
    GroupEditor,
    SeniorEditor,
    ManagingEditor,
    CollectionEditor,
    Production,
    Designer,
    LogoDesign,
    Translator,
    SupervisingEditor,
    ExecutiveEditor,
    EditorInChief,
    President,
    Publisher,
    ChiefCreativeOfficer,
    ExecutiveProducer,
    Other,
}

impl RoleValues {
    pub const ALL: [Self; 42] = [
        Self::Writer,
        Self::Script,
        Self::Story,
        Self::Plot,
        Self::Interviewer,
        Self::Artist,
        Self::Penciller,
        Self::Breakdowns,
        Self::Illustrator,
        Self::Layouts,
        Self::Inker,
        Self::Embellisher,
        Self::Finishes,
        Self::InkAssists,
        Self::Colorist,
        Self::ColorSeparations,
        Self::ColorAssists,
        Self::ColorFlats,
        Self::DigitalArtTechnician,
        Self::GrayTone,
        Self::Letterer,
        Self::Cover,
        Self::Editor,
        Self::ConsultingEditor,
        Self::AssistantEditor,
        Self::AssociateEditor,
        Self::GroupEditor,
        Self::SeniorEditor,
        Self::ManagingEditor,
        Self::CollectionEditor,
        Self::Production,
        Self::Designer,
        Self::LogoDesign,
        Self::Translator,
        Self::SupervisingEditor,
        Self::ExecutiveEditor,
        Self::EditorInChief,
        Self::President,
        Self::Publisher,
        Self::ChiefCreativeOfficer,
        Self::ExecutiveProducer,
        Self::Other,
    ];

    pub fn xml_name(self) -> &'static str {
        match self {
            Self::Writer => "Writer",
            Self::Script => "Script",
            Self::Story => "Story",
            Self::Plot => "Plot",
            Self::Interviewer => "Interviewer",
            Self::Artist => "Artist",
            Self::Penciller => "Penciller",
            Self::Breakdowns => "Breakdowns",
            Self::Illustrator => "Illustrator",
            Self::Layouts => "Layouts",
            Self::Inker => "Inker",
            Self::Embellisher => "Embellisher",
            Self::Finishes => "Finishes",
            Self::InkAssists => "Ink Assists",
            Self::Colorist => "Colorist",
            Self::ColorSeparations => "Color Separations",
            Self::ColorAssists => "Color Assists",
            Self::ColorFlats => "Color Flats",
            Self::DigitalArtTechnician => "Digital Art Technician",
            Self::GrayTone => "Gray Tone",
            Self::Letterer => "Letterer",
            Self::Cover => "Cover",
            Self::Editor => "Editor",
            Self::ConsultingEditor => "Consulting Editor",
            Self::AssistantEditor => "Assistant Editor",
            Self::AssociateEditor => "Associate Editor",
            Self::GroupEditor => "Group Editor",
            Self::SeniorEditor => "Senior Editor",
            Self::ManagingEditor => "Managing Editor",
            Self::CollectionEditor => "Collection Editor",
            Self::Production => "Production",
            Self::Designer => "Designer",
            Self::LogoDesign => "Logo Design",
            Self::Translator => "Translator",
            Self::SupervisingEditor => "Supervising Editor",
            Self::ExecutiveEditor => "Executive Editor",
            Self::EditorInChief => "Editor In Chief",
            Self::President => "President",
            Self::Publisher => "Publisher",
            Self::ChiefCreativeOfficer => "Chief Creative Officer",
            Self::ExecutiveProducer => "Executive Producer",
            Self::Other => "Other",
        }
    }

    pub fn from_xml(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.xml_name() == name)
    }
}

// --- schema types ------------------------------------------------------

/// `ResourceType` — text value with optional `id` attribute.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResourceType {
    pub value: String,
    pub id: Option<String>,
}

/// `IdType` — text + `source` (required) + `primary` attributes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IdType {
    pub value: String,
    pub source: Option<InformationSource>,
    pub primary: bool,
    pub primary_specified: bool,
}

/// `NameType` — text + `id` + `lang` (default "en") attributes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NameType {
    pub value: String,
    pub id: Option<String>,
    pub lang: Option<String>,
}

/// `PriceType` — decimal text + `country` attribute. Text is kept raw
/// to preserve decimal formatting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PriceType {
    pub value: String,
    pub country: Option<String>,
}

/// `UrlType` — text + `primary`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IdUrlType {
    pub value: String,
    pub primary: bool,
    pub primary_specified: bool,
}

/// `ArcType`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArcType {
    pub name: String,
    pub number: i32,
    pub number_specified: bool,
    pub id: Option<String>,
}

/// `UniverseType`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UniverseType {
    pub name: String,
    pub designation: String,
    pub id: Option<String>,
}

/// `GtinType` — ISBN/UPC are `object` in the C#; raw text here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GtinType {
    pub isbn: Option<String>,
    pub upc: Option<String>,
}

/// `CreditType`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CreditType {
    pub creator: Option<ResourceType>,
    pub roles: Vec<RoleValues>,
}

/// `SeriesType`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeriesType {
    pub name: String,
    pub sort_name: String,
    pub volume: i32,
    pub volume_specified: bool,
    pub format: Option<FormatType>,
    pub format_specified: bool,
    pub start_year: String,
    pub issue_count: i32,
    pub issue_count_specified: bool,
    pub volume_count: i32,
    pub volume_count_specified: bool,
    pub alternative_names: Vec<NameType>,
    pub lang: Option<String>,
    pub id: Option<String>,
}

/// `PublisherType`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PublisherType {
    pub name: String,
    pub imprint: Option<ResourceType>,
    pub id: Option<String>,
}

/// `MetronInfo` root.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MetronInfo {
    pub ids: Vec<IdType>,
    pub publisher: Option<PublisherType>,
    pub series: Option<SeriesType>,
    pub manga_volume: String,
    pub collection_title: String,
    pub number: String,
    pub stories: Vec<ResourceType>,
    pub summary: String,
    pub prices: Vec<PriceType>,
    pub cover_date: Option<CrDateTime>,
    pub store_date: Option<CrDateTime>,
    pub page_count: i32,
    pub notes: String,
    pub genres: Vec<ResourceType>,
    pub tags: Vec<ResourceType>,
    pub arcs: Vec<ArcType>,
    pub characters: Vec<ResourceType>,
    pub teams: Vec<ResourceType>,
    pub universes: Vec<UniverseType>,
    pub locations: Vec<ResourceType>,
    pub reprints: Vec<ResourceType>,
    pub gtin: Option<GtinType>,
    pub age_rating: AgeRatingType,
    pub urls: Vec<IdUrlType>,
    pub credits: Vec<CreditType>,
    pub last_modified: Option<CrDateTime>,
}

// --- serialization ------------------------------------------------------

fn opt_attr<W: Write>(
    e: &mut Emitter<W>,
    name: &str,
    value: &Option<String>,
) -> std::io::Result<()> {
    if let Some(v) = value {
        e.attr(name, v)?;
    }
    Ok(())
}

impl MetronInfo {
    /// Writes the full document bytes: declaration, root with the
    /// ComicRack `xsd`/`xsi` namespaces, body. The root element name
    /// and namespace come from `XmlRootAttribute("MetronInfo")`.
    pub fn serialize_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut e = Emitter::new(&mut out)?;
        e.root("MetronInfo")?;
        self.write_children(&mut e)?;
        e.end()?;
        e.finish()?;
        Ok(out)
    }

    fn write_children<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        // IDS wrapper (XmlArray "IDS", items "ID"); omitted when empty.
        if !self.ids.is_empty() {
            e.start("IDS")?;
            for id in &self.ids {
                e.start("ID")?;
                if let Some(source) = id.source {
                    e.attr("source", source.xml_name())?;
                }
                if id.primary_specified {
                    e.attr("primary", if id.primary { "true" } else { "false" })?;
                }
                e.text(&id.value)?;
                e.end()?;
            }
            e.end()?;
        }
        if let Some(publisher) = &self.publisher {
            e.start("Publisher")?;
            opt_attr(e, "id", &publisher.id)?;
            e.text_elem("Name", &publisher.name)?;
            if let Some(imprint) = &publisher.imprint {
                e.start("Imprint")?;
                opt_attr(e, "id", &imprint.id)?;
                e.text(&imprint.value)?;
                e.end()?;
            }
            e.end()?;
        }
        if let Some(series) = &self.series {
            e.start("Series")?;
            opt_attr(e, "id", &series.id)?;
            if let Some(lang) = &series.lang {
                e.attr("lang", lang)?;
            }
            e.text_elem("Name", &series.name)?;
            if !series.sort_name.is_empty() {
                e.text_elem("SortName", &series.sort_name)?;
            }
            if series.volume_specified {
                e.text_elem("Volume", &series.volume.to_string())?;
            }
            if series.format_specified {
                if let Some(format) = series.format {
                    e.text_elem("Format", format.xml_name())?;
                }
            }
            if !series.start_year.is_empty() {
                e.text_elem("StartYear", &series.start_year)?;
            }
            if series.issue_count_specified {
                e.text_elem("IssueCount", &series.issue_count.to_string())?;
            }
            if series.volume_count_specified {
                e.text_elem("VolumeCount", &series.volume_count.to_string())?;
            }
            if !series.alternative_names.is_empty() {
                e.start("AlternativeNames")?;
                for name in &series.alternative_names {
                    e.start("AlternativeName")?;
                    opt_attr(e, "id", &name.id)?;
                    if let Some(lang) = &name.lang {
                        e.attr("lang", lang)?;
                    }
                    e.text(&name.value)?;
                    e.end()?;
                }
                e.end()?;
            }
            e.end()?;
        }
        if !self.manga_volume.is_empty() {
            e.text_elem("MangaVolume", &self.manga_volume)?;
        }
        if !self.collection_title.is_empty() {
            e.text_elem("CollectionTitle", &self.collection_title)?;
        }
        if !self.number.is_empty() {
            e.text_elem("Number", &self.number)?;
        }
        if !self.stories.is_empty() {
            e.start("Stories")?;
            for story in &self.stories {
                e.start("Story")?;
                opt_attr(e, "id", &story.id)?;
                e.text(&story.value)?;
                e.end()?;
            }
            e.end()?;
        }
        if !self.summary.is_empty() {
            e.text_elem("Summary", &self.summary)?;
        }
        if !self.prices.is_empty() {
            e.start("Prices")?;
            for price in &self.prices {
                e.start("Price")?;
                opt_attr(e, "country", &price.country)?;
                e.text(&price.value)?;
                e.end()?;
            }
            e.end()?;
        }
        if let Some(cover_date) = &self.cover_date {
            e.text_elem("CoverDate", &cover_date.to_date_xml())?;
        }
        if let Some(store_date) = &self.store_date {
            e.text_elem("StoreDate", &store_date.to_date_xml())?;
        }
        if self.page_count != 0 {
            e.text_elem("PageCount", &self.page_count.to_string())?;
        }
        if !self.notes.is_empty() {
            e.text_elem("Notes", &self.notes)?;
        }
        if !self.genres.is_empty() {
            e.start("Genres")?;
            for genre in &self.genres {
                e.start("Genre")?;
                opt_attr(e, "id", &genre.id)?;
                e.text(&genre.value)?;
                e.end()?;
            }
            e.end()?;
        }
        if !self.tags.is_empty() {
            e.start("Tags")?;
            for tag in &self.tags {
                e.start("Tag")?;
                opt_attr(e, "id", &tag.id)?;
                e.text(&tag.value)?;
                e.end()?;
            }
            e.end()?;
        }
        if !self.arcs.is_empty() {
            e.start("Arcs")?;
            for arc in &self.arcs {
                e.start("Arc")?;
                opt_attr(e, "id", &arc.id)?;
                e.text_elem("Name", &arc.name)?;
                if arc.number_specified {
                    e.text_elem("Number", &arc.number.to_string())?;
                }
                e.end()?;
            }
            e.end()?;
        }
        if !self.characters.is_empty() {
            e.start("Characters")?;
            for character in &self.characters {
                e.start("Character")?;
                opt_attr(e, "id", &character.id)?;
                e.text(&character.value)?;
                e.end()?;
            }
            e.end()?;
        }
        if !self.teams.is_empty() {
            e.start("Teams")?;
            for team in &self.teams {
                e.start("Team")?;
                opt_attr(e, "id", &team.id)?;
                e.text(&team.value)?;
                e.end()?;
            }
            e.end()?;
        }
        if !self.universes.is_empty() {
            e.start("Universes")?;
            for universe in &self.universes {
                e.start("Universe")?;
                opt_attr(e, "id", &universe.id)?;
                e.text_elem("Name", &universe.name)?;
                if !universe.designation.is_empty() {
                    e.text_elem("Designation", &universe.designation)?;
                }
                e.end()?;
            }
            e.end()?;
        }
        if !self.locations.is_empty() {
            e.start("Locations")?;
            for location in &self.locations {
                e.start("Location")?;
                opt_attr(e, "id", &location.id)?;
                e.text(&location.value)?;
                e.end()?;
            }
            e.end()?;
        }
        if !self.reprints.is_empty() {
            e.start("Reprints")?;
            for reprint in &self.reprints {
                e.start("Reprint")?;
                opt_attr(e, "id", &reprint.id)?;
                e.text(&reprint.value)?;
                e.end()?;
            }
            e.end()?;
        }
        if let Some(gtin) = &self.gtin {
            e.start("GTIN")?;
            if let Some(isbn) = &gtin.isbn {
                e.text_elem("ISBN", isbn)?;
            }
            if let Some(upc) = &gtin.upc {
                e.text_elem("UPC", upc)?;
            }
            e.end()?;
        }
        if self.age_rating != AgeRatingType::Unknown {
            e.text_elem("AgeRating", self.age_rating.xml_name())?;
        }
        if !self.urls.is_empty() {
            e.start("URLs")?;
            for url in &self.urls {
                e.start("URL")?;
                if url.primary_specified {
                    e.attr("primary", if url.primary { "true" } else { "false" })?;
                }
                e.text(&url.value)?;
                e.end()?;
            }
            e.end()?;
        }
        if !self.credits.is_empty() {
            e.start("Credits")?;
            for credit in &self.credits {
                e.start("Credit")?;
                if let Some(creator) = &credit.creator {
                    e.start("Creator")?;
                    opt_attr(e, "id", &creator.id)?;
                    e.text(&creator.value)?;
                    e.end()?;
                }
                if !credit.roles.is_empty() {
                    e.start("Roles")?;
                    for role in &credit.roles {
                        e.start("Role")?;
                        e.text(role.xml_name())?;
                        e.end()?;
                    }
                    e.end()?;
                }
                e.end()?;
            }
            e.end()?;
        }
        if let Some(last_modified) = &self.last_modified {
            e.text_elem("LastModified", &last_modified.to_xml())?;
        }
        Ok(())
    }
}

// --- mapping -------------------------------------------------------------

impl MetronInfo {
    /// Port of `MetronInfoProvider.ToXml` — the read path that turns a
    /// MetronInfo.xml into a `ComicInfo`. Field-for-field from the C#,
    /// including the quirk that a `RoleValues` string check uses
    /// substring matches ("Color", "Editor", "Translator").
    ///
    /// `LocalizeUtility.LocalizeEnum` defaults (English, no TR data):
    /// PascalCase member names converted to spaced text.
    pub fn to_comic_info(&self) -> ComicInfo {
        const DELIMITER: &str = ", ";

        let empty = String::new();
        let mut info = ComicInfo::default();

        // Credits: per role filter, in credit then role order.
        let creators = |predicate: &dyn Fn(RoleValues) -> bool| -> String {
            let mut names: Vec<&str> = Vec::new();
            for credit in &self.credits {
                for role in &credit.roles {
                    if predicate(*role) {
                        if let Some(creator) = &credit.creator {
                            names.push(&creator.value);
                        }
                    }
                }
            }
            names.join(DELIMITER)
        };
        info.writer = creators(&|r| matches!(r, RoleValues::Writer | RoleValues::Plot));
        info.penciller = creators(&|r| r == RoleValues::Penciller);
        info.inker = creators(&|r| matches!(r, RoleValues::Inker | RoleValues::InkAssists));
        info.colorist = creators(&|r| r.xml_name().contains("Color"));
        info.editor = creators(&|r| r.xml_name().contains("Editor"));
        info.translator = creators(&|r| r.xml_name().contains("Translator"));
        info.letterer = creators(&|r| r == RoleValues::Letterer);
        info.cover_artist = creators(&|r| r == RoleValues::Cover);

        info.publisher = self
            .publisher
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| empty.clone());
        info.imprint = self
            .publisher
            .as_ref()
            .and_then(|p| p.imprint.as_ref())
            .map(|i| i.value.clone())
            .unwrap_or_else(|| empty.clone());
        info.series = self
            .series
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| empty.clone());
        info.number = self.number.clone();
        info.count = match self.series.as_ref() {
            Some(s) if s.issue_count_specified => s.issue_count,
            _ => -1,
        };
        info.alternate_series = self
            .arcs
            .first()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| empty.clone());
        info.alternate_number = self
            .arcs
            .first()
            .map(|a| a.number.to_string())
            .unwrap_or_else(|| empty.clone());
        // The C# uses the 1st Story as Title and the 2nd as StoryArc.
        info.title = self
            .stories
            .first()
            .map(|s| s.value.clone())
            .unwrap_or_else(|| empty.clone());
        info.story_arc = self
            .stories
            .get(1)
            .map(|s| s.value.clone())
            .unwrap_or_else(|| empty.clone());
        info.summary = self.summary.clone();
        info.volume = match self.series.as_ref() {
            None => -1,
            Some(s) if s.volume_specified => s.volume,
            Some(s) => s.start_year.trim().parse().unwrap_or(-1),
        };
        if let Some(cover_date) = &self.cover_date {
            let date = cover_date.naive.date();
            info.year = date.year();
            info.month = date.month() as i32;
            info.day = date.day() as i32;
        }
        info.notes = self.notes.clone();
        info.genre = self
            .genres
            .iter()
            .map(|g| g.value.clone())
            .collect::<Vec<_>>()
            .join(DELIMITER);
        info.web = self
            .urls
            .iter()
            .find(|u| u.primary)
            .or_else(|| self.urls.first())
            .map(|u| u.value.clone())
            .unwrap_or_else(|| empty.clone());
        info.page_count = self.page_count;
        info.language_iso = self
            .series
            .as_ref()
            .and_then(|s| s.lang.clone())
            .unwrap_or_else(|| empty.clone());
        info.age_rating = if self.age_rating == AgeRatingType::Unknown {
            empty.clone()
        } else {
            self.age_rating.localized_default().to_string()
        };
        info.characters = self
            .characters
            .iter()
            .map(|c| c.value.clone())
            .collect::<Vec<_>>()
            .join(DELIMITER);
        info.teams = self
            .teams
            .iter()
            .map(|t| t.value.clone())
            .collect::<Vec<_>>()
            .join(DELIMITER);
        info.locations = self
            .locations
            .iter()
            .map(|l| l.value.clone())
            .collect::<Vec<_>>()
            .join(DELIMITER);
        info.tags = self
            .tags
            .iter()
            .map(|t| t.value.clone())
            .collect::<Vec<_>>()
            .join(DELIMITER);
        info.format = match self.series.as_ref() {
            Some(s) if s.format_specified => match s.format {
                Some(FormatType::TradePaperback) => "TPB".to_string(),
                Some(f) => f.localized_default().to_string(),
                None => empty.clone(),
            },
            _ => empty.clone(),
        };
        info
    }
}

// --- parsing -----------------------------------------------------------

/// `xs:date` with an `xs:dateTime` fallback.
fn parse_date_or_datetime(text: &str) -> Option<CrDateTime> {
    CrDateTime::parse_date(text)
        .or_else(|_| CrDateTime::parse(text))
        .ok()
}

struct Parser<'a, 'b> {
    reader: &'b mut crate::xml::XmlReader<'a>,
}

impl<'a, 'b> Parser<'a, 'b> {
    /// Next direct child of the current element; stops (and consumes)
    /// at any end tag.
    fn next_child(&mut self) -> XmlResult<Option<Start>> {
        loop {
            match self.reader.next_tok()? {
                Tok::Start(s) => return Ok(Some(s)),
                Tok::End(_) => return Ok(None),
                Tok::Eof => return Err(XmlError("eof before root close".into())),
                _ => {}
            }
        }
    }

    fn text_of(&mut self, start: &Start, parent: &str) -> XmlResult<String> {
        let mut text = String::new();
        let mut depth = 0usize;
        loop {
            match self.reader.next_tok()? {
                Tok::Text(t) => text.push_str(&t),
                Tok::Start(_) => depth += 1,
                Tok::End(n) => {
                    if depth == 0 && n == start.name {
                        return Ok(text);
                    }
                    if depth == 0 && n == parent {
                        return Err(XmlError(format!("unclosed <{}>", start.name)));
                    }
                    depth = depth.saturating_sub(1);
                }
                Tok::Eof => return Err(XmlError("eof in element".into())),
            }
        }
    }
}

impl MetronInfo {
    /// Parses a full MetronInfo document (root `<MetronInfo>`).
    pub fn parse_root(reader: &mut crate::xml::XmlReader<'_>) -> XmlResult<MetronInfo> {
        loop {
            match reader.next_tok()? {
                Tok::Start(start) if start.name == "MetronInfo" => {
                    let mut p = Parser { reader };
                    let mut info = MetronInfo::default();
                    while let Some(child) = p.next_child()? {
                        match child.name.as_str() {
                            "IDS" => {
                                while let Some(id) = p.next_child()? {
                                    if id.name != "ID" {
                                        p.reader.skip_element(&id.name)?;
                                    } else {
                                        let value = p.text_of(&id, "IDS")?;
                                        info.ids.push(IdType {
                                            value,
                                            source: id
                                                .attr("source")
                                                .and_then(InformationSource::from_xml),
                                            primary: id.attr("primary") == Some("true"),
                                            primary_specified: id.attr("primary").is_some(),
                                        });
                                    }
                                }
                            }
                            "Publisher" => {
                                let mut publisher = PublisherType {
                                    id: child.attr("id").map(String::from),
                                    ..Default::default()
                                };
                                while let Some(elem) = p.next_child()? {
                                    match elem.name.as_str() {
                                        "Name" => publisher.name = p.text_of(&elem, "Publisher")?,
                                        "Imprint" => {
                                            publisher.imprint = Some(ResourceType {
                                                id: elem.attr("id").map(String::from),
                                                value: p.text_of(&elem, "Publisher")?,
                                            });
                                        }
                                        _ => {
                                            p.reader.skip_element(&elem.name)?;
                                        }
                                    }
                                }
                                info.publisher = Some(publisher);
                            }
                            "Series" => {
                                let mut series = SeriesType {
                                    id: child.attr("id").map(String::from),
                                    lang: child.attr("lang").map(String::from),
                                    ..Default::default()
                                };
                                while let Some(elem) = p.next_child()? {
                                    match elem.name.as_str() {
                                        "Name" => series.name = p.text_of(&elem, "Series")?,
                                        "SortName" => {
                                            series.sort_name = p.text_of(&elem, "Series")?
                                        }
                                        "Volume" => {
                                            series.volume =
                                                p.text_of(&elem, "Series")?.parse().unwrap_or(0);
                                            series.volume_specified = true;
                                        }
                                        "Format" => {
                                            let text = p.text_of(&elem, "Series")?;
                                            series.format = FormatType::from_xml(&text);
                                            series.format_specified = true;
                                        }
                                        "StartYear" => {
                                            series.start_year = p.text_of(&elem, "Series")?
                                        }
                                        "IssueCount" => {
                                            series.issue_count =
                                                p.text_of(&elem, "Series")?.parse().unwrap_or(0);
                                            series.issue_count_specified = true;
                                        }
                                        "VolumeCount" => {
                                            series.volume_count =
                                                p.text_of(&elem, "Series")?.parse().unwrap_or(0);
                                            series.volume_count_specified = true;
                                        }
                                        "AlternativeNames" => {
                                            while let Some(name) = p.next_child()? {
                                                if name.name == "AlternativeName" {
                                                    series.alternative_names.push(NameType {
                                                        id: name.attr("id").map(String::from),
                                                        lang: name.attr("lang").map(String::from),
                                                        value: p
                                                            .text_of(&name, "AlternativeNames")?,
                                                    });
                                                }
                                            }
                                        }
                                        _ => {
                                            p.reader.skip_element(&elem.name)?;
                                        }
                                    }
                                }
                                info.series = Some(series);
                            }
                            "MangaVolume" => info.manga_volume = p.text_of(&child, "MetronInfo")?,
                            "CollectionTitle" => {
                                info.collection_title = p.text_of(&child, "MetronInfo")?
                            }
                            "Number" => info.number = p.text_of(&child, "MetronInfo")?,
                            "Stories" => {
                                while let Some(story) = p.next_child()? {
                                    if story.name != "Story" {
                                        p.reader.skip_element(&story.name)?;
                                    } else {
                                        info.stories.push(ResourceType {
                                            id: story.attr("id").map(String::from),
                                            value: p.text_of(&story, "Stories")?,
                                        });
                                    }
                                }
                            }
                            "Summary" => info.summary = p.text_of(&child, "MetronInfo")?,
                            "Prices" => {
                                while let Some(price) = p.next_child()? {
                                    if price.name != "Price" {
                                        p.reader.skip_element(&price.name)?;
                                    } else {
                                        info.prices.push(PriceType {
                                            country: price.attr("country").map(String::from),
                                            value: p.text_of(&price, "Prices")?,
                                        });
                                    }
                                }
                            }
                            "CoverDate" => {
                                let text = p.text_of(&child, "MetronInfo")?;
                                info.cover_date = parse_date_or_datetime(&text);
                            }
                            "StoreDate" => {
                                let text = p.text_of(&child, "MetronInfo")?;
                                info.store_date = parse_date_or_datetime(&text);
                            }
                            "PageCount" => {
                                info.page_count =
                                    p.text_of(&child, "MetronInfo")?.parse().unwrap_or(0)
                            }
                            "Notes" => info.notes = p.text_of(&child, "MetronInfo")?,
                            "Genres" => {
                                while let Some(genre) = p.next_child()? {
                                    if genre.name != "Genre" {
                                        p.reader.skip_element(&genre.name)?;
                                    } else {
                                        info.genres.push(ResourceType {
                                            id: genre.attr("id").map(String::from),
                                            value: p.text_of(&genre, "Genres")?,
                                        });
                                    }
                                }
                            }
                            "Tags" => {
                                while let Some(tag) = p.next_child()? {
                                    if tag.name != "Tag" {
                                        p.reader.skip_element(&tag.name)?;
                                    } else {
                                        info.tags.push(ResourceType {
                                            id: tag.attr("id").map(String::from),
                                            value: p.text_of(&tag, "Tags")?,
                                        });
                                    }
                                }
                            }
                            "Arcs" => {
                                while let Some(arc) = p.next_child()? {
                                    if arc.name != "Arc" {
                                        p.reader.skip_element(&arc.name)?;
                                    } else {
                                        let mut value = ArcType {
                                            id: arc.attr("id").map(String::from),
                                            ..Default::default()
                                        };
                                        while let Some(elem) = p.next_child()? {
                                            match elem.name.as_str() {
                                                "Name" => value.name = p.text_of(&elem, "Arcs")?,
                                                "Number" => {
                                                    value.number = p
                                                        .text_of(&elem, "Arcs")?
                                                        .parse()
                                                        .unwrap_or(0);
                                                    value.number_specified = true;
                                                }
                                                _ => {
                                                    p.reader.skip_element(&elem.name)?;
                                                }
                                            }
                                        }
                                        info.arcs.push(value);
                                    }
                                }
                            }
                            "Characters" => {
                                while let Some(c) = p.next_child()? {
                                    if c.name != "Character" {
                                        p.reader.skip_element(&c.name)?;
                                    } else {
                                        info.characters.push(ResourceType {
                                            id: c.attr("id").map(String::from),
                                            value: p.text_of(&c, "Characters")?,
                                        });
                                    }
                                }
                            }
                            "Teams" => {
                                while let Some(t) = p.next_child()? {
                                    if t.name != "Team" {
                                        p.reader.skip_element(&t.name)?;
                                    } else {
                                        info.teams.push(ResourceType {
                                            id: t.attr("id").map(String::from),
                                            value: p.text_of(&t, "Teams")?,
                                        });
                                    }
                                }
                            }
                            "Universes" => {
                                while let Some(u) = p.next_child()? {
                                    if u.name != "Universe" {
                                        p.reader.skip_element(&u.name)?;
                                    } else {
                                        let mut value = UniverseType {
                                            id: u.attr("id").map(String::from),
                                            ..Default::default()
                                        };
                                        while let Some(elem) = p.next_child()? {
                                            match elem.name.as_str() {
                                                "Name" => {
                                                    value.name = p.text_of(&elem, "Universes")?
                                                }
                                                "Designation" => {
                                                    value.designation =
                                                        p.text_of(&elem, "Universes")?
                                                }
                                                _ => {
                                                    p.reader.skip_element(&elem.name)?;
                                                }
                                            }
                                        }
                                        info.universes.push(value);
                                    }
                                }
                            }
                            "Locations" => {
                                while let Some(l) = p.next_child()? {
                                    if l.name != "Location" {
                                        p.reader.skip_element(&l.name)?;
                                    } else {
                                        info.locations.push(ResourceType {
                                            id: l.attr("id").map(String::from),
                                            value: p.text_of(&l, "Locations")?,
                                        });
                                    }
                                }
                            }
                            "Reprints" => {
                                while let Some(r) = p.next_child()? {
                                    if r.name != "Reprint" {
                                        p.reader.skip_element(&r.name)?;
                                    } else {
                                        info.reprints.push(ResourceType {
                                            id: r.attr("id").map(String::from),
                                            value: p.text_of(&r, "Reprints")?,
                                        });
                                    }
                                }
                            }
                            "GTIN" => {
                                let mut gtin = GtinType::default();
                                while let Some(elem) = p.next_child()? {
                                    match elem.name.as_str() {
                                        "ISBN" => gtin.isbn = Some(p.text_of(&elem, "GTIN")?),
                                        "UPC" => gtin.upc = Some(p.text_of(&elem, "GTIN")?),
                                        _ => {
                                            p.reader.skip_element(&elem.name)?;
                                        }
                                    }
                                }
                                info.gtin = Some(gtin);
                            }
                            "AgeRating" => {
                                let text = p.text_of(&child, "MetronInfo")?;
                                if let Some(rating) = AgeRatingType::from_xml(&text) {
                                    info.age_rating = rating;
                                }
                            }
                            "URLs" => {
                                while let Some(url) = p.next_child()? {
                                    if url.name != "URL" {
                                        p.reader.skip_element(&url.name)?;
                                    } else {
                                        info.urls.push(IdUrlType {
                                            value: p.text_of(&url, "URLs")?,
                                            primary: url.attr("primary") == Some("true"),
                                            primary_specified: url.attr("primary").is_some(),
                                        });
                                    }
                                }
                            }
                            "Credits" => {
                                while let Some(credit) = p.next_child()? {
                                    if credit.name != "Credit" {
                                        p.reader.skip_element(&credit.name)?;
                                    } else {
                                        let mut value = CreditType::default();
                                        while let Some(elem) = p.next_child()? {
                                            match elem.name.as_str() {
                                                "Creator" => {
                                                    value.creator = Some(ResourceType {
                                                        id: elem.attr("id").map(String::from),
                                                        value: p.text_of(&elem, "Credits")?,
                                                    });
                                                }
                                                "Roles" => {
                                                    while let Some(role) = p.next_child()? {
                                                        if role.name == "Role" {
                                                            let text = p.text_of(&role, "Roles")?;
                                                            if let Some(role_value) =
                                                                RoleValues::from_xml(&text)
                                                            {
                                                                value.roles.push(role_value);
                                                            }
                                                        }
                                                    }
                                                }
                                                _ => {
                                                    p.reader.skip_element(&elem.name)?;
                                                }
                                            }
                                        }
                                        info.credits.push(value);
                                    }
                                }
                            }
                            "LastModified" => {
                                let text = p.text_of(&child, "MetronInfo")?;
                                info.last_modified = CrDateTime::parse(&text).ok();
                            }
                            _ => {
                                p.reader.skip_element(&child.name)?;
                            }
                        }
                    }
                    return Ok(info);
                }
                Tok::Eof => return Err(XmlError("no MetronInfo root".into())),
                _ => {}
            }
        }
    }
}
