//! Pure display geometry — the port of `ImageDisplayControl` nested
//! types `DisplayOutputConfig` and `DisplayOutput`
//! (`ComicRack.Engine.Display.Forms/ImageDisplayControl.cs`). No GTK
//! types here: the widget feeds a `DisplayConfig` + view size and
//! consumes the resulting transform/part data.
//!
//! Matrix convention: GDI+ `[m11, m12, m21, m22, dx, dy]`,
//! row-vector (`p' = p·M`). Cairo's `Matrix::new(xx, yx, xy, yy, x0,
//! y0)` consumes the same element order, so the six floats pass
//! through unchanged.

use cr_core::model::enums::ImageRotation;

/// `ImageFitMode` (`Engine/Display/ImageFitMode.cs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFitMode {
    Original,
    #[default]
    Fit,
    FitWidth,
    FitWidthAdaptive,
    FitHeight,
    BestFit,
}

/// `RightToLeftReadingMode` (`Engine/Display/RightToLeftReadingMode.cs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RtlReadingMode {
    #[default]
    FlipParts,
    FlipPages,
}

/// `PartPageToDisplay` (`Engine/Display/PartPageToDisplay.cs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartPageToDisplay {
    First,
    Previous,
    Next,
    Last,
}

/// `ImagePartInfo` (`Engine/ImagePartInfo.cs`): the visible part
/// index plus a pixel offset inside that part. `part` stays signed —
/// navigation computes `part - 1` before clamping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImagePartInfo {
    pub part: i32,
    pub offset: (i32, i32),
}

impl Default for ImagePartInfo {
    fn default() -> Self {
        ImagePartInfo::EMPTY
    }
}

impl ImagePartInfo {
    pub const EMPTY: ImagePartInfo = ImagePartInfo {
        part: 0,
        offset: (0, 0),
    };

    pub fn new(part: i32, offset: (i32, i32)) -> Self {
        ImagePartInfo { part, offset }
    }
}

/// GDI+ `Rectangle` subset used by the geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// `Rectangle.Intersect` — an empty rect on no overlap.
    pub fn intersect(a: Rect, b: Rect) -> Rect {
        let left = a.x.max(b.x);
        let top = a.y.max(b.y);
        let right = a.right().min(b.right());
        let bottom = a.bottom().min(b.bottom());
        if right <= left || bottom <= top {
            return Rect::default();
        }
        Rect::new(left, top, right - left, bottom - top)
    }

    pub fn offset_by(&mut self, dx: i32, dy: i32) {
        self.x += dx;
        self.y += dy;
    }
}

/// GDI+ `Matrix` subset in element order `[m11, m12, m21, m22, dx,
/// dy]` (row-vector: `x' = x·m11 + y·m21 + dx`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat {
    pub e: [f32; 6],
}

impl Default for Mat {
    fn default() -> Self {
        Mat {
            e: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }
}

impl Mat {
    pub fn rotation(degrees: i32) -> Mat {
        let rad = degrees as f32 * std::f32::consts::PI / 180.0;
        let (s, c) = rad.sin_cos();
        // GDI+ Rotate: positive = clockwise in the y-down space.
        Mat {
            e: [c, s, -s, c, 0.0, 0.0],
        }
    }

    pub fn translation(dx: f32, dy: f32) -> Mat {
        Mat {
            e: [1.0, 0.0, 0.0, 1.0, dx, dy],
        }
    }

    pub fn scaling(sx: f32, sy: f32) -> Mat {
        Mat {
            e: [sx, 0.0, 0.0, sy, 0.0, 0.0],
        }
    }

    /// `MatrixOrder.Append`: `self = self·op` (op applies after).
    pub fn append(&mut self, op: &Mat) {
        let [a11, a12, a21, a22, adx, ady] = self.e;
        let [b11, b12, b21, b22, bdx, bdy] = op.e;
        self.e = [
            a11 * b11 + a12 * b21,
            a11 * b12 + a12 * b22,
            a21 * b11 + a22 * b21,
            a21 * b12 + a22 * b22,
            adx * b11 + ady * b21 + bdx,
            adx * b12 + ady * b22 + bdy,
        ];
    }

    pub fn transform_point(&self, x: f32, y: f32) -> (f32, f32) {
        (
            x * self.e[0] + y * self.e[2] + self.e[4],
            x * self.e[1] + y * self.e[3] + self.e[5],
        )
    }
}

/// `MatrixUtility.GetRotationMatrix`: rotate around `anchor`.
pub fn rotation_around(anchor: (i32, i32), degrees: i32) -> Mat {
    let mut m = Mat::default();
    if degrees % 360 == 0 {
        return m;
    }
    m.append(&Mat::translation(-anchor.0 as f32, -anchor.1 as f32));
    m.append(&Mat::rotation(degrees));
    m.append(&Mat::translation(anchor.0 as f32, anchor.1 as f32));
    m
}

/// `Rectangle.Rotate(matrix)` — the bounding box of the rotated
/// corners.
fn rotate_bounds(w: i32, h: i32, degrees: i32) -> Rect {
    let m = rotation_around((w / 2, h / 2), degrees);
    let pts = [
        m.transform_point(0.0, 0.0),
        m.transform_point(w as f32, 0.0),
        m.transform_point(0.0, h as f32),
        m.transform_point(w as f32, h as f32),
    ];
    let min_x = pts.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
    let min_y = pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
    let max_x = pts.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
    let max_y = pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
    Rect::new(
        min_x as i32,
        min_y as i32,
        (max_x - min_x) as i32,
        (max_y - min_y) as i32,
    )
}

/// `DisplayOutputConfig` — the widget inputs for one rendered frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayConfig {
    pub view_size: (i32, i32),
    pub image_size: (i32, i32),
    pub fit_mode: ImageFitMode,
    pub fit_only_if_oversized: bool,
    pub rtl_mode: RtlReadingMode,
    pub rtl: bool,
    pub part: ImagePartInfo,
    pub image_zoom: f32,
    pub zoom: f32,
    pub rotation: ImageRotation,
    pub two_page_auto_scroll: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        DisplayConfig {
            view_size: (0, 0),
            image_size: (0, 0),
            fit_mode: ImageFitMode::Original,
            fit_only_if_oversized: true,
            rtl_mode: RtlReadingMode::FlipPages,
            rtl: false,
            part: ImagePartInfo::EMPTY,
            image_zoom: 1.0,
            zoom: 1.0,
            rotation: ImageRotation::None,
            two_page_auto_scroll: true,
        }
    }
}

/// `DisplayOutput` — the resolved geometry for one frame.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayOutput {
    pub config: DisplayConfig,
    pub mat: Mat,
    pub scale: (f32, f32),
    pub part_bounds: Rect,
    pub part: i32,
    pub part_count: i32,
    pub parts: Vec<Rect>,
    pub image_zoom: f32,
}

impl Default for DisplayOutput {
    fn default() -> Self {
        DisplayOutput {
            config: DisplayConfig::default(),
            mat: Mat::default(),
            scale: (1.0, 1.0),
            part_bounds: Rect::default(),
            part: 0,
            part_count: 0,
            parts: Vec::new(),
            image_zoom: 1.0,
        }
    }
}

impl DisplayOutput {
    pub fn is_empty(&self) -> bool {
        self.part_count == 0
    }

    pub fn is_all_visible(&self) -> bool {
        self.parts.len() < 2
    }

    pub fn output_bounds(&self) -> Rect {
        Rect::new(0, 0, self.part_bounds.w, self.part_bounds.h)
    }

    /// `DisplayOutput.Create` — resolve config to transform + parts.
    pub fn create(config: &DisplayConfig, anamorphic_tolerance: f32) -> DisplayOutput {
        let mut out = DisplayOutput {
            config: *config,
            image_zoom: config.image_zoom,
            ..Default::default()
        };
        if config.image_size.0 == 0 || config.image_size.1 == 0 {
            return out;
        }
        let degrees = match config.rotation {
            ImageRotation::None => 0,
            ImageRotation::Rotate90 => 90,
            ImageRotation::Rotate180 => 180,
            ImageRotation::Rotate270 => 270,
        };
        // The fit math runs against the rotated view size.
        let rotated_view = rotate_bounds(config.view_size.0, config.view_size.1, degrees);
        let client = (rotated_view.w, rotated_view.h);
        let real_zoom = get_scale(
            client,
            config.image_size,
            config.fit_mode,
            config.fit_only_if_oversized,
            config.zoom,
            anamorphic_tolerance,
        );
        let double_spread = !config.two_page_auto_scroll;
        let grid = get_part_grid_size(client, config.image_size, real_zoom, double_spread);
        let count = grid.0 * grid.1;
        let part = config.part.part.clamp(0, count - 1);
        let parts: Vec<Rect> = (0..count)
            .map(|i| {
                get_part_rectangle(
                    client,
                    config.image_size,
                    real_zoom,
                    i,
                    grid,
                    double_spread,
                    config.rtl_mode,
                    config.rtl,
                )
            })
            .collect();
        let mut bounds = parts[part as usize];
        let off = clamped_part_offset(config.part.offset, config.image_size, bounds);
        bounds.offset_by(off.0, off.1);

        let mut m = rotation_around((bounds.w / 2, bounds.h / 2), degrees);
        let rotated_part = rotate_bounds(bounds.w, bounds.h, degrees);
        m.append(&Mat::translation(
            -rotated_part.x as f32,
            -rotated_part.y as f32,
        ));
        m.append(&Mat::scaling(real_zoom.0, real_zoom.1));
        m.append(&Mat::translation(
            (config.view_size.0 as f32 - rotated_part.w as f32 * real_zoom.0) / 2.0,
            (config.view_size.1 as f32 - rotated_part.h as f32 * real_zoom.1) / 2.0,
        ));
        out.mat = m;
        out.scale = real_zoom;
        out.part_bounds = bounds;
        out.parts = parts;
        out.part = part;
        out.part_count = count;
        out
    }

    /// `DisplayOutput.Interpolate` — animation blend of two outputs.
    pub fn interpolate(a: &DisplayOutput, b: &DisplayOutput, p: f32) -> DisplayOutput {
        let mut out = DisplayOutput {
            part: b.part,
            parts: b.parts.clone(),
            config: b.config,
            scale: (
                a.scale.0 + (b.scale.0 - a.scale.0) * p,
                a.scale.1 + (b.scale.1 - a.scale.1) * p,
            ),
            image_zoom: a.image_zoom + (b.image_zoom - a.image_zoom) * p,
            ..Default::default()
        };
        let ratio = p * (b.image_zoom / out.image_zoom);
        let dx = (b.part_bounds.x - a.part_bounds.x) as f32 * ratio;
        let dy = (b.part_bounds.y - a.part_bounds.y) as f32 * ratio;
        let dw = (b.part_bounds.w - a.part_bounds.w) as f32 * ratio;
        let dh = (b.part_bounds.h - a.part_bounds.h) as f32 * ratio;
        out.part_bounds = Rect::new(
            (a.part_bounds.x as f32 + dx) as i32,
            (a.part_bounds.y as f32 + dy) as i32,
            (a.part_bounds.w as f32 + dw) as i32,
            (a.part_bounds.h as f32 + dh) as i32,
        );
        for i in 0..6 {
            out.mat.e[i] = a.mat.e[i] + (b.mat.e[i] - a.mat.e[i]) * p;
        }
        out
    }

    pub fn get_part(&self, index: i32) -> Rect {
        if self.parts.is_empty() {
            return Rect::default();
        }
        let last = self.parts.len() as i32 - 1;
        self.parts[index.clamp(0, last) as usize]
    }

    /// `GetPartOffset` — the offset clamped inside the part.
    pub fn part_offset(&self, part: i32, offset: (i32, i32)) -> (i32, i32) {
        clamped_part_offset(offset, self.config.image_size, self.get_part(part))
    }

    pub fn get_part_with_offset(&self, part: i32, offset: (i32, i32)) -> Rect {
        let mut r = self.get_part(part);
        let off = self.part_offset(part, offset);
        r.offset_by(off.0, off.1);
        r
    }

    pub fn get_part_full(&self, ipi: ImagePartInfo) -> Rect {
        self.get_part_with_offset(ipi.part, ipi.offset)
    }

    pub fn is_start_part(&self, ipi: ImagePartInfo) -> bool {
        self.get_part_full(ipi) == self.get_part(0)
    }

    /// `IsEndPart(Rectangle)` compares against `GetPart(PartCount)` —
    /// out of range, so the clamp resolves it to the last part.
    pub fn is_end_part(&self, ipi: ImagePartInfo) -> bool {
        self.get_part_full(ipi) == self.get_part(self.part_count)
    }

    /// `GetBestPartFit` — pull a part/offset back into the legal
    /// range, keeping the most-visible overlap.
    pub fn get_best_part_fit(&self, new_part: ImagePartInfo) -> ImagePartInfo {
        let mut num = new_part.part;
        let mut offset = new_part.offset;
        if num < 0 {
            offset = (0, 0);
            num = 0;
        }
        if num >= self.part_count {
            offset = (0, 0);
            num = self.part_count - 1;
        }
        if num < 0 {
            return new_part;
        }
        offset = self.part_offset(num, offset);
        let mut test = self.get_part(num);
        test.offset_by(offset.0, offset.1);
        let best = index_of_best_fit(&self.parts, test);
        if num != best {
            let from = self.get_part(num);
            let to = self.get_part(best);
            offset = (offset.0 + from.x - to.x, offset.1 + from.y - to.y);
            num = best;
        }
        ImagePartInfo { part: num, offset }
    }
}

/// `RectangleExtensions.IndexOfBestFit` — largest intersection area,
/// first index wins ties.
fn index_of_best_fit(parts: &[Rect], test: Rect) -> i32 {
    let mut result = -1;
    let mut best_area = 0;
    for (i, part) in parts.iter().enumerate() {
        let area = {
            let isect = Rect::intersect(*part, test);
            isect.w * isect.h
        };
        if area > best_area {
            best_area = area;
            result = i as i32;
        }
    }
    result
}

/// `Numeric.CompareTo(f, t, limit)` — within an absolute limit.
fn approx(a: f32, b: f32, limit: f32) -> bool {
    (a - b).abs() < limit
}

/// `DisplayOutput.GetScale` — the fit-mode scale math, verbatim
/// including the anamorphic tolerance (`ImageFitOnlyIfOversized`
/// keeps small images at 1:1).
pub fn get_scale(
    client: (i32, i32),
    bitmap: (i32, i32),
    mode: ImageFitMode,
    only_fit_oversized: bool,
    zoom: f32,
    anamorphic_tolerance: f32,
) -> (f32, f32) {
    let wide = bitmap.0 > bitmap.1;
    let bw = bitmap.0 as f32;
    let bh = bitmap.1 as f32;
    let sx = client.0 as f32 / bw * zoom;
    let sy = client.1 as f32 / bh * zoom;
    let uniform = (zoom, zoom);
    let tol = anamorphic_tolerance;
    match mode {
        ImageFitMode::Original => uniform,
        ImageFitMode::FitWidth => {
            if only_fit_oversized && client.0 as f32 > bw {
                return uniform;
            }
            if approx(sy, sx, sx * tol) {
                (sx, sy)
            } else {
                (sx, sx)
            }
        }
        ImageFitMode::FitWidthAdaptive => {
            if only_fit_oversized && client.0 as f32 > bw {
                return uniform;
            }
            let sx = if wide { sx * 2.0 } else { sx };
            if approx(sy, sx, sx * tol) {
                (sx, sy)
            } else {
                (sx, sx)
            }
        }
        ImageFitMode::FitHeight => {
            if only_fit_oversized && client.1 as f32 > bh {
                return uniform;
            }
            if approx(sx, sy, sy * tol) {
                (sx, sy)
            } else {
                (sy, sy)
            }
        }
        ImageFitMode::Fit => {
            if only_fit_oversized && client.1 as f32 > bh && client.0 as f32 > bw {
                return uniform;
            }
            let m = sx.min(sy);
            let sy = if approx(sy, m, m * tol) { sy } else { m };
            let sx = if approx(sx, m, m * tol) { sx } else { m };
            (sx, sy)
        }
        ImageFitMode::BestFit => {
            if only_fit_oversized && (client.1 as f32 > bh || client.0 as f32 > bw) {
                return uniform;
            }
            let m = sx.max(sy);
            let sy = if approx(sy, m, m * tol) { sy } else { m };
            let sx = if approx(sx, m, m * tol) { sx } else { m };
            (sx, sy)
        }
    }
}

/// `DisplayOutput.GetPartGridSize` — how many viewport tiles the
/// scaled image spans. With `double_spread`, a width-tall grid pairs
/// columns so spreads advance two columns per part.
fn get_part_grid_size(
    client: (i32, i32),
    bitmap: (i32, i32),
    zoom: (f32, f32),
    double_spread: bool,
) -> (i32, i32) {
    let sh = (bitmap.1 as f32 * zoom.1) as i32;
    let sw = (bitmap.0 as f32 * zoom.0) as i32;
    if client.0 == 0 || client.1 == 0 {
        return (1, 1);
    }
    if sh <= client.1 && sw <= client.0 {
        return (1, 1);
    }
    if !double_spread || sh > sw {
        return ((sw - 1) / client.0 + 1, (sh - 1) / client.1 + 1);
    }
    (((sw / 2 - 1) / client.0 + 1) * 2, (sh - 1) / client.1 + 1)
}

/// `DisplayOutput.GetPartRectangle` — one tile of the part grid in
/// image coordinates, with the double-spread binding-edge logic and
/// the RTL mirror.
#[allow(clippy::too_many_arguments)] // 1:1 with the C# signature
fn get_part_rectangle(
    client: (i32, i32),
    bitmap: (i32, i32),
    zoom: (f32, f32),
    part: i32,
    grid: (i32, i32),
    double_spread: bool,
    rtl_mode: RtlReadingMode,
    rtl: bool,
) -> Rect {
    let wide = bitmap.0 > bitmap.1;
    let count = grid.0 * grid.1;
    let part = part.clamp(0, count - 1);
    if double_spread && wide {
        // Landscape page: pair the grid columns into left/right page
        // halves, then apply the binding-edge rules per half.
        let simple = get_part_grid_size(client, bitmap, zoom, false);
        let halves = (simple.0 - 1) / 2 + 1;
        let per_row = halves * simple.1;
        let in_row = part % per_row;
        let row_base = part / per_row * (simple.0 / 2);
        let col = simple.0 * (in_row / halves) + in_row % halves + row_base;
        let mut r = get_part_rectangle(client, bitmap, zoom, col, simple, false, rtl_mode, false);
        if rtl {
            match rtl_mode {
                RtlReadingMode::FlipParts => {
                    r.x = bitmap.0 - r.right();
                    if part < per_row {
                        if r.x < bitmap.0 / 2 {
                            r.x = bitmap.0 / 2;
                        }
                        if r.right() > bitmap.0 {
                            r.x -= r.right() - bitmap.0;
                        }
                    } else {
                        if r.right() > bitmap.0 / 2 {
                            r.x -= r.right() - bitmap.0 / 2;
                        }
                        if r.x < 0 {
                            r.x = 0;
                        }
                    }
                }
                RtlReadingMode::FlipPages => {
                    if part < per_row {
                        r.x = bitmap.0 / 2 - r.right();
                        if r.x < 0 {
                            r.x = 0;
                        }
                    } else {
                        r.x -= bitmap.0 / 2;
                        r.x = bitmap.0 / 2 - r.right();
                        r.x += bitmap.0 / 2;
                        if r.right() > bitmap.0 {
                            r.x -= r.right() - bitmap.0;
                        }
                    }
                }
            }
        } else if part < per_row {
            if r.right() > bitmap.0 / 2 {
                r.x -= r.right() - bitmap.0 / 2;
            }
            if r.x < 0 {
                r.x = 0;
            }
        } else {
            if r.x < bitmap.0 / 2 {
                r.x = bitmap.0 / 2;
            }
            if r.right() > bitmap.0 {
                r.x -= r.right() - bitmap.0;
            }
        }
        return r;
    }
    let mut client = client;
    client.0 = (client.0 as f32 / zoom.0) as i32;
    client.1 = (client.1 as f32 / zoom.1) as i32;
    let row_h = bitmap.1 / grid.1;
    let col_w = bitmap.0 / grid.0;
    let mut r = clamp_rect(
        bitmap,
        Rect::new(
            col_w * (part % grid.0),
            row_h * (part / grid.0),
            client.0,
            client.1,
        ),
    );
    if rtl {
        r.x = bitmap.0 - r.right();
    }
    r
}

/// `DisplayOutput.GetClampedPartOffset`.
fn clamped_part_offset(offset: (i32, i32), image_size: (i32, i32), part: Rect) -> (i32, i32) {
    let mut x = offset.0;
    let mut y = offset.1;
    if x + part.right() > image_size.0 {
        x = image_size.0 - part.right();
    }
    if y + part.bottom() > image_size.1 {
        y = image_size.1 - part.bottom();
    }
    if x + part.x < 0 {
        x = -part.x;
    }
    if y + part.y < 0 {
        y = -part.y;
    }
    (x, y)
}

/// `DisplayOutput.Clamp` — keep the tile inside the image, shrinking
/// it at the bottom/right edges.
fn clamp_rect(size: (i32, i32), mut rect: Rect) -> Rect {
    if rect.right() > size.0 {
        rect.x = size.0 - rect.w;
    }
    if rect.bottom() > size.1 {
        rect.y = size.1 - rect.h;
    }
    if rect.y < 0 {
        rect.h += rect.y;
        rect.y = 0;
    }
    if rect.x < 0 {
        rect.w += rect.x;
        rect.x = 0;
    }
    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn fit_uses_the_smaller_axis() {
        // 800x600 view, 400x800 image → the image fits by height
        // (0.75) for Fit, by width (2.0) for BestFit.
        let s = get_scale((800, 600), (400, 800), ImageFitMode::Fit, false, 1.0, 0.25);
        assert!(close(s.0, 0.75) && close(s.1, 0.75));
        let s = get_scale(
            (800, 600),
            (400, 800),
            ImageFitMode::BestFit,
            false,
            1.0,
            0.25,
        );
        assert!(close(s.0, 2.0) && close(s.1, 2.0));
        // Original keeps the zoom.
        let s = get_scale(
            (800, 600),
            (400, 800),
            ImageFitMode::Original,
            false,
            1.5,
            0.25,
        );
        assert_eq!(s, (1.5, 1.5));
    }

    #[test]
    fn fit_only_if_oversized_keeps_small_images() {
        let s = get_scale((800, 600), (400, 300), ImageFitMode::Fit, true, 1.0, 0.25);
        assert_eq!(s, (1.0, 1.0));
        let s = get_scale((800, 600), (400, 300), ImageFitMode::Fit, false, 1.0, 0.25);
        assert!(close(s.0, 2.0));
    }

    #[test]
    fn fit_width_adaptive_doubles_wide_images() {
        // A wide page halves into the window width.
        let s = get_scale(
            (800, 600),
            (1600, 600),
            ImageFitMode::FitWidthAdaptive,
            false,
            1.0,
            0.25,
        );
        assert!(close(s.0, 1.0) && close(s.1, 1.0));
        // Portrait page behaves like FitWidth.
        let s = get_scale(
            (800, 600),
            (400, 800),
            ImageFitMode::FitWidthAdaptive,
            false,
            1.0,
            0.25,
        );
        assert!(close(s.0, 2.0));
    }

    #[test]
    fn part_grid_tiles_the_viewport() {
        // 2000x1000 image at zoom 1 in 800x600 → 3x2 tiles.
        let grid = get_part_grid_size((800, 600), (2000, 1000), (1.0, 1.0), false);
        assert_eq!(grid, (3, 2));
        // Fits entirely → one part.
        let grid = get_part_grid_size((800, 600), (400, 300), (1.0, 1.0), false);
        assert_eq!(grid, (1, 1));
    }

    #[test]
    fn part_grid_double_spread_pairs_columns() {
        // A 1600x800 image in 400x600: simple grid 4 wide; double
        // spread halves the column count into pairs.
        let simple = get_part_grid_size((400, 600), (1600, 800), (1.0, 1.0), false);
        assert_eq!(simple, (4, 2));
        let spread = get_part_grid_size((400, 600), (1600, 800), (1.0, 1.0), true);
        assert_eq!(spread, (4, 2));
        // Wider case where pairing actually changes the column count.
        let simple = get_part_grid_size((300, 600), (1600, 800), (1.0, 1.0), false);
        let spread = get_part_grid_size((300, 600), (1600, 800), (1.0, 1.0), true);
        assert_eq!(simple.0 % 2, 0);
        assert!(spread.0 >= simple.0);
    }

    #[test]
    fn part_rectangles_respect_the_grid_and_rtl() {
        // The C# tiles are viewport-sized (client/zoom) windows over
        // the image, positioned by the grid — they overlap.
        let bitmap = (2000, 1000);
        let grid = (3, 2);
        let ltr = get_part_rectangle(
            (800, 600),
            bitmap,
            (1.0, 1.0),
            0,
            grid,
            false,
            RtlReadingMode::FlipPages,
            false,
        );
        assert_eq!(ltr, Rect::new(0, 0, 800, 600));
        let rtl = get_part_rectangle(
            (800, 600),
            bitmap,
            (1.0, 1.0),
            0,
            grid,
            false,
            RtlReadingMode::FlipPages,
            true,
        );
        // Mirrored: starts at the right edge.
        assert_eq!(rtl.x + rtl.w, bitmap.0);
        assert_eq!(rtl.w, ltr.w);
    }

    #[test]
    fn clamped_offset_stays_inside_the_image() {
        let part = Rect::new(1200, 0, 800, 500);
        assert_eq!(clamped_part_offset((0, 0), (2000, 1000), part), (0, 0));
        assert_eq!(
            clamped_part_offset((-200, 0), (2000, 1000), part),
            (-200, 0)
        );
        assert_eq!(clamped_part_offset((100, 0), (2000, 1000), part), (0, 0));
    }

    #[test]
    fn create_centers_a_fitting_page() {
        let config = DisplayConfig {
            view_size: (800, 600),
            image_size: (400, 600),
            fit_mode: ImageFitMode::Fit,
            fit_only_if_oversized: false,
            ..Default::default()
        };
        let out = DisplayOutput::create(&config, 0.25);
        assert_eq!(out.part_count, 1);
        // The transform maps the image corners inside the view,
        // touching the top/bottom edges (fit by height).
        let (x0, y0) = out.mat.transform_point(0.0, 0.0);
        let (x1, y1) = out.mat.transform_point(400.0, 600.0);
        assert!(x0 >= 0.0 && x1 <= 800.0);
        assert!(close(y0, 0.0) && close(y1, 600.0));
    }

    #[test]
    fn create_rotates_the_view_frame() {
        // 90° rotation: the scale math sees a 600x800 view.
        let config = DisplayConfig {
            view_size: (800, 600),
            image_size: (600, 800),
            fit_mode: ImageFitMode::Fit,
            fit_only_if_oversized: false,
            rotation: ImageRotation::Rotate90,
            ..Default::default()
        };
        let out = DisplayOutput::create(&config, 0.25);
        assert!(close(out.scale.0, 1.0) && close(out.scale.1, 1.0));
        // The rotated page must land inside the 800x600 view. The
        // C# Matrix is float32, so 90° carries ~1px of rounding —
        // tolerate it.
        let (x0, y0) = out.mat.transform_point(0.0, 0.0);
        let (x1, y1) = out.mat.transform_point(600.0, 800.0);
        let min_x = x0.min(x1);
        let max_x = x0.max(x1);
        let min_y = y0.min(y1);
        let max_y = y0.max(y1);
        assert!(min_x >= -1.5 && max_x <= 801.5);
        assert!(min_y >= -1.5 && max_y <= 601.5);
    }

    #[test]
    fn is_start_and_end_part() {
        let config = DisplayConfig {
            view_size: (800, 600),
            image_size: (2000, 1000),
            fit_mode: ImageFitMode::Original,
            fit_only_if_oversized: false,
            ..Default::default()
        };
        let out = DisplayOutput::create(&config, 0.25);
        assert_eq!(out.part_count, 6);
        assert!(out.is_start_part(ImagePartInfo::EMPTY));
        assert!(out.is_end_part(ImagePartInfo::new(5, (0, 0))));
        assert!(!out.is_end_part(ImagePartInfo::new(4, (0, 0))));
    }

    #[test]
    fn best_part_fit_pulls_out_of_range_parts_back() {
        let config = DisplayConfig {
            view_size: (800, 600),
            image_size: (2000, 1000),
            fit_mode: ImageFitMode::Original,
            fit_only_if_oversized: false,
            ..Default::default()
        };
        let out = DisplayOutput::create(&config, 0.25);
        let fit = out.get_best_part_fit(ImagePartInfo::new(99, (0, 0)));
        assert_eq!(fit.part, out.part_count - 1);
        let fit = out.get_best_part_fit(ImagePartInfo::new(-3, (10, 10)));
        assert_eq!(fit.part, 0);
        assert_eq!(fit.offset, (0, 0));
    }

    #[test]
    fn interpolate_blends_linearly() {
        let small = DisplayConfig {
            view_size: (800, 600),
            image_size: (400, 600),
            fit_mode: ImageFitMode::Fit,
            fit_only_if_oversized: false,
            ..Default::default()
        };
        let mut big = small;
        big.image_zoom = 2.0;
        big.zoom = 2.0;
        let a = DisplayOutput::create(&small, 0.25);
        let b = DisplayOutput::create(&big, 0.25);
        let mid = DisplayOutput::interpolate(&a, &b, 0.5);
        assert!(close(mid.scale.0, (a.scale.0 + b.scale.0) / 2.0));
        assert!(close(mid.image_zoom, 1.5));
        let end = DisplayOutput::interpolate(&a, &b, 1.0);
        assert_eq!(end.mat, b.mat);
    }
}
