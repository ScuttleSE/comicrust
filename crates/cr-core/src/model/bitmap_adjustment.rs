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

    /// `HasColorTransformations` — any of the three scales (epsilon
    /// compare) or a non-black/white white point.
    pub fn has_color_transformations(&self) -> bool {
        let eps = |v: f32| v.abs() < 1e-5;
        if eps(self.contrast) && eps(self.saturation) && eps(self.brightness) {
            let (r, g, b) = self.white_point_rgb();
            return !matches!((r, g, b), (0, 0, 0) | (255, 255, 255));
        }
        true
    }

    /// The 8-bit RGB of the white point (`Color.FromArgb(argb)`), or
    /// `None` for the unset forms (0 and -1 map to black).
    pub fn white_point_rgb(&self) -> (u8, u8, u8) {
        if self.white_point_argb == 0 || self.white_point_argb == -1 {
            return (0, 0, 0);
        }
        (
            ((self.white_point_argb >> 16) & 0xff) as u8,
            ((self.white_point_argb >> 8) & 0xff) as u8,
            (self.white_point_argb & 0xff) as u8,
        )
    }

    /// `HasAutoContrast`.
    pub fn has_auto_contrast(&self) -> bool {
        (self.options.0 & 1) != 0 // BitmapAdjustmentOptions.AutoContrast
    }

    /// `HasSharpening`.
    pub fn has_sharpening(&self) -> bool {
        self.sharpen != 0
    }

    /// `HasGamma`.
    pub fn has_gamma(&self) -> bool {
        self.gamma.abs() >= 1e-5
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
