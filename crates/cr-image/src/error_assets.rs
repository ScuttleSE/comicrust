//! The C# error assets: `ImagePool.CreateErrorPage` /
//! `CreateErrorThumbnail` (`ComicRack.Engine/IO/Cache/ImagePool.cs`)
//! with the original resources (`Resources/ErrorPage.jpg`,
//! `Resources/RedCross.png`).

use crate::Image;

const ERROR_PAGE_JPG: &[u8] = include_bytes!("../assets/ErrorPage.jpg");
const RED_CROSS_PNG: &[u8] = include_bytes!("../assets/RedCross.png");

/// `CreateErrorPage` bitmap (without the drawn message — the reader
/// overlays the localized text at render time).
pub fn error_page_image() -> Option<Image> {
    crate::decode::decode(ERROR_PAGE_JPG).ok()
}

/// `CreateErrorThumbnail`: a white `h·2/3 × h` bitmap with the red
/// cross drawn at three times its natural size, centered (the rect
/// overflows and clips, like the C# `DrawImage`).
pub fn error_thumbnail(height: u32) -> Option<Image> {
    let height = height.max(1);
    let width = height * 2 / 3;
    let cross = crate::decode::decode(RED_CROSS_PNG).ok()?;
    let rect_w = i64::from(cross.width * 3);
    let rect_h = i64::from(cross.height * 3);
    let x0 = (i64::from(width) - rect_w) / 2;
    let y0 = (i64::from(height) - rect_h) / 2;
    let mut rgba = vec![255u8; width as usize * height as usize * 4];
    // Clip the (possibly overflowing) rect to the bitmap.
    let dy0 = y0.max(0);
    let dy1 = (y0 + rect_h).min(i64::from(height));
    let dx0 = x0.max(0);
    let dx1 = (x0 + rect_w).min(i64::from(width));
    for by in dy0..dy1 {
        for bx in dx0..dx1 {
            let ry = (by - y0) * i64::from(cross.height) / rect_h;
            let rx = (bx - x0) * i64::from(cross.width) / rect_w;
            let si = (ry as usize * cross.width as usize + rx as usize) * 4;
            let a = cross.rgba[si + 3];
            if a == 0 {
                continue;
            }
            let di = (by as usize * width as usize + bx as usize) * 4;
            let blend = |bg: u8, fg: u8| {
                ((u16::from(fg) * u16::from(a) + u16::from(bg) * (255 - u16::from(a))) / 255) as u8
            };
            rgba[di] = blend(255, cross.rgba[si]);
            rgba[di + 1] = blend(255, cross.rgba[si + 1]);
            rgba[di + 2] = blend(255, cross.rgba[si + 2]);
            rgba[di + 3] = 255;
        }
    }
    Some(Image {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_page_decodes_at_resource_size() {
        let page = error_page_image().expect("bundled error page decodes");
        assert_eq!((page.width, page.height), (1000, 1402));
    }

    #[test]
    fn error_thumbnail_is_white_with_red_cross() {
        let t = error_thumbnail(512).expect("thumbnail builds");
        assert_eq!((t.width, t.height), (341, 512));
        // The giant clipped X covers most of the thumb: both red
        // strokes and white hollows appear.
        let mut red = 0;
        let mut white = 0;
        for y in 0..t.height {
            for x in 0..t.width {
                let i = (y as usize * t.width as usize + x as usize) * 4;
                let (r, g, b) = (t.rgba[i], t.rgba[i + 1], t.rgba[i + 2]);
                if r > 150 && g < 80 && b < 80 {
                    red += 1;
                } else if (r, g, b) == (255, 255, 255) {
                    white += 1;
                }
            }
        }
        assert!(red > 1000, "no red strokes: {red}");
        assert!(white > 1000, "no white hollows: {white}");
    }
}
