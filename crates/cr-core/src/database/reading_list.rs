//! The `.cbl` reading-list file — the `ComicReadingListContainer` port
//! (`ComicRack.Engine/Database/ComicReadingListContainer.cs`, 103 LOC;
//! `ComicReadingListItem.cs`). A standalone XML document: a name, an
//! optional `MatcherMode` attribute, the `<Books>` items (series /
//! number / volume / year / format + an optional book Guid + file
//! name) and an optional `<Matchers>` set (the same matcher
//! serialization the ComicLists tree uses).
//!
//! The C# `XmlSerializer` conventions follow the ComicDb writer: value
//! members without `[DefaultValue]` always serialize (`Id`), string
//! members with `[DefaultValue("")]` only when set (`Series`,
//! `Number`, `Format`, `FileName`), collections always serialize.

use crate::database::list_items::ComicBookMatcher;
use crate::model::enums::MatcherMode;
use crate::xml::reader::XmlResult;
use crate::xml::scalar::CrGuid;
use crate::xml::{Emitter, Start, Tok};
use std::io::Write;

/// One `<Book>` entry (`ComicReadingListItem`).
#[derive(Clone, Debug, PartialEq)]
pub struct ReadingListItem {
    pub series: String,
    pub number: String,
    pub volume: i32,
    pub year: i32,
    pub format: String,
    pub id: CrGuid,
    pub file_name: String,
}

impl Default for ReadingListItem {
    fn default() -> Self {
        // The C# constructor defaults: Volume/Year -1, the rest empty.
        ReadingListItem {
            series: String::new(),
            number: String::new(),
            volume: -1,
            year: -1,
            format: String::new(),
            id: CrGuid::EMPTY,
            file_name: String::new(),
        }
    }
}

/// The parsed `.cbl` container.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReadingListContainer {
    /// `<Name>`; `None` when the element is missing (the C# leaves the
    /// string property null; the import substitutes "").
    pub name: Option<String>,
    pub matcher_mode: MatcherMode,
    pub items: Vec<ReadingListItem>,
    pub matchers: Vec<ComicBookMatcher>,
}

impl ReadingListItem {
    fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("Book")?;
        if !self.series.is_empty() {
            e.attr("Series", &self.series)?;
        }
        if !self.number.is_empty() {
            e.attr("Number", &self.number)?;
        }
        if self.volume != -1 {
            e.attr("Volume", &self.volume.to_string())?;
        }
        if self.year != -1 {
            e.attr("Year", &self.year.to_string())?;
        }
        if !self.format.is_empty() {
            e.attr("Format", &self.format)?;
        }
        e.text_elem("Id", &self.id.to_string())?;
        if !self.file_name.is_empty() {
            e.text_elem("FileName", &self.file_name)?;
        }
        e.end()
    }

    fn from_start(s: &Start, r: &mut crate::xml::XmlReader<'_>) -> XmlResult<Self> {
        let mut item = ReadingListItem::default();
        for (k, v) in &s.attrs {
            match k.as_str() {
                "Series" => item.series = v.clone(),
                "Number" => item.number = v.clone(),
                "Volume" => {
                    item.volume = v
                        .trim()
                        .parse()
                        .map_err(|_| crate::xml::reader::XmlError(format!("bad Volume: {v}")))?
                }
                "Year" => {
                    item.year = v
                        .trim()
                        .parse()
                        .map_err(|_| crate::xml::reader::XmlError(format!("bad Year: {v}")))?
                }
                "Format" => item.format = v.clone(),
                _ => {}
            }
        }
        loop {
            match r.next_tok()? {
                Tok::Eof => return Err(crate::xml::reader::XmlError("eof in Book".into())),
                Tok::End(n) if n == s.name => return Ok(item),
                Tok::Start(s2) => match s2.name.as_str() {
                    "Id" => {
                        item.id = CrGuid::parse(&r.text_content("Id")?)
                            .map_err(|e| crate::xml::reader::XmlError(e.0))?;
                    }
                    "FileName" => item.file_name = r.text_content("FileName")?,
                    _ => r.skip_element(&s2.name)?,
                },
                Tok::Text(_) => {}
                _ => {}
            }
        }
    }
}

impl ReadingListContainer {
    /// The `ComicReadingListContainer.Serialize` shape (indented, the
    /// same declaration the ComicDb writer emits).
    pub fn write_bytes(&self) -> Result<Vec<u8>, std::io::Error> {
        let mut out = Vec::new();
        let mut e = Emitter::new(&mut out)?;
        e.root("ReadingList")?;
        if self.matcher_mode != MatcherMode::And {
            e.attr("MatcherMode", &self.matcher_mode.to_xml())?;
        }
        e.text_elem("Name", self.name.as_deref().unwrap_or(""))?;
        e.start("Books")?;
        for item in &self.items {
            item.write_xml(&mut e)?;
        }
        e.end()?;
        e.start("Matchers")?;
        for m in &self.matchers {
            m.write_xml(&mut e)?;
        }
        e.end()?;
        e.end()?;
        e.finish()?;
        Ok(out)
    }

    /// The `ComicReadingListContainer.Deserialize` port. Order-tolerant;
    /// unknown elements are skipped.
    pub fn parse(bytes: &[u8]) -> XmlResult<Self> {
        let mut buf = std::io::BufReader::new(bytes);
        let mut reader = crate::xml::XmlReader::new(&mut buf);
        let mut c = ReadingListContainer {
            matcher_mode: MatcherMode::And,
            ..Default::default()
        };
        let root = loop {
            match reader.next_tok()? {
                Tok::Eof => return Err(crate::xml::reader::XmlError("empty document".into())),
                Tok::Start(s) => break s,
                _ => {}
            }
        };
        if let Some(mode) = root.attr("MatcherMode") {
            c.matcher_mode = MatcherMode::from_xml(mode)
                .ok_or_else(|| crate::xml::reader::XmlError(format!("bad MatcherMode: {mode}")))?;
        }
        loop {
            match reader.next_tok()? {
                Tok::Eof => return Err(crate::xml::reader::XmlError("eof in ReadingList".into())),
                Tok::End(n) if n == root.name => return Ok(c),
                Tok::Start(s) => match s.name.as_str() {
                    "Name" => c.name = Some(reader.text_content("Name")?),
                    "Books" => loop {
                        match reader.next_tok()? {
                            Tok::Eof => {
                                return Err(crate::xml::reader::XmlError("eof in Books".into()))
                            }
                            Tok::End(n) if n == "Books" => break,
                            Tok::Start(s2) if s2.name == "Book" => {
                                c.items.push(ReadingListItem::from_start(&s2, &mut reader)?);
                            }
                            Tok::Start(s2) => reader.skip_element(&s2.name)?,
                            _ => {}
                        }
                    },
                    "Matchers" => loop {
                        match reader.next_tok()? {
                            Tok::Eof => {
                                return Err(crate::xml::reader::XmlError("eof in Matchers".into()))
                            }
                            Tok::End(n) if n == "Matchers" => break,
                            Tok::Start(s2) if s2.attr("xsi:type").is_some() => {
                                c.matchers
                                    .push(ComicBookMatcher::from_start(&s2, &mut reader)?);
                            }
                            Tok::Start(s2) => reader.skip_element(&s2.name)?,
                            _ => {}
                        }
                    },
                    _ => reader.skip_element(&s.name)?,
                },
                Tok::Text(_) => {}
                _ => {}
            }
        }
    }
}
