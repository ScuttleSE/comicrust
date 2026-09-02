//! `BitmapAdjustment` (color adjustment) and `ExtraSyncInformation`.

use crate::model::enums::BitmapAdjustmentOptions;
use crate::xml::scalar::net_f32;
use crate::xml::Emitter;
use std::io::Write;

/// Color adjustment stored in ComicBook (`ColorAdjustment` element).
/// Written when not equal to `Empty` (the C# `ColorAdjustmentSpecified`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct BitmapAdjustment {
    pub saturation: f32,
    pub contrast: f32,
    pub brightness: f32,
    pub gamma: f32,
    pub white_point_argb: i32,
    pub options: BitmapAdjustmentOptions,
    pub sharpen: i32,
}

impl BitmapAdjustment {
    pub fn is_empty(&self) -> bool {
        *self == BitmapAdjustment::default()
    }

    pub fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("ColorAdjustment")?;
        if self.saturation != 0.0 {
            e.text_elem("Saturation", &net_f32(self.saturation))?;
        }
        if self.contrast != 0.0 {
            e.text_elem("Contrast", &net_f32(self.contrast))?;
        }
        if self.brightness != 0.0 {
            e.text_elem("Brightness", &net_f32(self.brightness))?;
        }
        if self.gamma != 0.0 {
            e.text_elem("Gamma", &net_f32(self.gamma))?;
        }
        if self.white_point_argb != 0 {
            e.text_elem("WhitePointArgb", &self.white_point_argb.to_string())?;
        }
        if self.options.0 != 0 {
            e.text_elem("Options", &self.options.to_xml())?;
        }
        if self.sharpen != 0 {
            e.text_elem("Sharpen", &self.sharpen.to_string())?;
        }
        e.end()
    }
}

/// `ComicRack.Engine.Sync.ExtraSyncInformation`. Backed by a static
/// dictionary in C#, so it never survives a load — kept as `Option` on
/// the book and written only when present.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ExtraSyncInformation {
    pub reading_state_changed: bool,
    pub information_changed: bool,
    pub bookmarks_changed: bool,
    pub page_types_changed: bool,
    pub check_changed: bool,
    pub data_changed: bool,
}

impl ExtraSyncInformation {
    pub fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("ExtraSyncInformation")?;
        for (name, v) in [
            ("ReadingStateChanged", self.reading_state_changed),
            ("InformationChanged", self.information_changed),
            ("BookmarksChanged", self.bookmarks_changed),
            ("PageTypesChanged", self.page_types_changed),
            ("CheckChanged", self.check_changed),
            ("DataChanged", self.data_changed),
        ] {
            e.text_elem(name, if v { "true" } else { "false" })?;
        }
        e.end()
    }
}
