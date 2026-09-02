//! `ComicPageInfo` — the per-page attribute bag serialized as
//! `<Page ... />` inside `<Pages>`.

use crate::model::enums::{ComicPagePosition, ComicPageType, ImageRotation};
use crate::xml::reader::{XmlError, XmlResult};
use crate::xml::{Emitter, Start};
use std::io::Write;

/// Mirrors the C# struct: several fields are `short`-backed and truncate
/// on set (`(short)value`), which must be reproduced for byte compat.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ComicPageInfo {
    /// Stored with the C# off-by-one encoding: XML `Image` = raw - 1.
    /// Default raw 0 produces `Image="-1"` (always written).
    pub image_index_raw: i16,
    /// Raw backing; 0 maps to `Story` on every read/write like the C#
    /// getter does. Normalized to `Story` after load.
    pub page_type: ComicPageType,
    pub bookmark: Option<String>,
    pub image_file_size: i32,
    pub image_width: i16,
    pub image_height: i16,
    pub rotation: ImageRotation,
    pub page_position: ComicPagePosition,
    pub key: Option<String>,
}

impl ComicPageInfo {
    pub fn image_index(&self) -> i32 {
        self.image_index_raw as i32 - 1
    }

    pub fn set_image_index(&mut self, v: i32) {
        self.image_index_raw = (v.wrapping_add(1)) as i16;
    }

    /// Effective page type: 0 maps to `Story` like the C# getter.
    fn effective_type(&self) -> ComicPageType {
        if self.page_type.0 == 0 {
            ComicPageType(8)
        } else {
            self.page_type
        }
    }

    /// XML attribute order: Image, Bookmark, ImageSize, ImageWidth,
    /// ImageHeight, Rotation, PagePosition, Key, Type.
    pub fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("Page")?;
        // Image: always written (no DefaultValue on ImageIndex).
        e.attr("Image", &self.image_index().to_string())?;
        if let Some(b) = &self.bookmark {
            e.attr("Bookmark", b)?;
        }
        if self.image_file_size != 0 {
            e.attr("ImageSize", &self.image_file_size.to_string())?;
        }
        if self.image_width != 0 {
            e.attr("ImageWidth", &self.image_width.to_string())?;
        }
        if self.image_height != 0 {
            e.attr("ImageHeight", &self.image_height.to_string())?;
        }
        if self.rotation != ImageRotation::None {
            e.attr("Rotation", &self.rotation.to_xml())?;
        }
        if self.page_position != ComicPagePosition::Default {
            e.attr("PagePosition", &self.page_position.to_xml())?;
        }
        if let Some(k) = &self.key {
            e.attr("Key", k)?;
        }
        // Type: string proxy for page_type; "Story" is the default value
        // and is suppressed.
        if self.effective_type() != ComicPageType(8) {
            e.attr("Type", &self.effective_type().to_xml())?;
        }
        e.end()
    }

    /// Parses a `<Page>` start tag; attribute parse failures are errors
    /// (the .NET reader throws on garbage); a `Type` value that fails
    /// `TryParse` keeps the current value (C# behavior).
    pub fn from_attrs(start: &Start) -> XmlResult<Self> {
        let mut p = ComicPageInfo::default();
        for (k, v) in &start.attrs {
            match k.as_str() {
                "Image" => {
                    let v: i32 = v
                        .trim()
                        .parse()
                        .map_err(|_| XmlError(format!("bad Page Image: {v}")))?;
                    p.set_image_index(v);
                }
                "Bookmark" => {
                    p.bookmark = if v.is_empty() { None } else { Some(v.clone()) };
                }
                "ImageSize" => {
                    p.image_file_size = v
                        .trim()
                        .parse()
                        .map_err(|_| XmlError(format!("bad ImageSize: {v}")))?;
                }
                "ImageWidth" => {
                    let v: i32 = v
                        .trim()
                        .parse()
                        .map_err(|_| XmlError(format!("bad ImageWidth: {v}")))?;
                    p.image_width = v as i16;
                }
                "ImageHeight" => {
                    let v: i32 = v
                        .trim()
                        .parse()
                        .map_err(|_| XmlError(format!("bad ImageHeight: {v}")))?;
                    p.image_height = v as i16;
                }
                "Rotation" => {
                    p.rotation = ImageRotation::from_xml(v)
                        .ok_or_else(|| XmlError(format!("bad Rotation: {v}")))?;
                }
                "PagePosition" => {
                    p.page_position = ComicPagePosition::from_xml(v)
                        .ok_or_else(|| XmlError(format!("bad PagePosition: {v}")))?;
                }
                "Key" => p.key = Some(v.clone()),
                "Type" => {
                    let fixed = v.replace("Advertisment", "Advertisement");
                    if let Some(t) = ComicPageType::from_xml(&fixed) {
                        p.page_type = t;
                    }
                }
                _ => {}
            }
        }
        if p.page_type.0 == 0 {
            p.page_type = ComicPageType(8);
        }
        Ok(p)
    }
}
