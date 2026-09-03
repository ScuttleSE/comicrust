//! Port of `ContinuousPageLayout.cs` — deterministic geometry for a
//! vertically stacked page strip. Virtual coordinates are `i64`;
//! output rectangles clip to `i32` like the C# helpers.

/// `SourcePage` — the page number and its source pixel size.
#[derive(Clone, Copy, Debug)]
pub struct SourcePage {
    pub page: usize,
    pub source_size: (i32, i32),
}

/// `PageEntry` — one page's output geometry.
#[derive(Clone, Copy, Debug)]
pub struct PageEntry {
    pub page: usize,
    pub source_size: (i32, i32),
    pub bounds: crate::reader::display::Rect,
    top: i64,
    height: i64,
}

impl PageEntry {
    pub fn top(&self) -> i64 {
        self.top
    }

    pub fn height(&self) -> i64 {
        self.height
    }

    pub fn bottom(&self) -> i64 {
        saturating_add(self.top, self.height)
    }
}

/// `Anchor` — a page plus a relative offset, stable across rebuilds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub page: usize,
    pub relative_offset: f64,
}

impl Anchor {
    pub fn new(page: usize, relative_offset: f64) -> Anchor {
        Anchor {
            page,
            relative_offset: if relative_offset.is_nan() {
                0.0
            } else {
                relative_offset.clamp(0.0, 1.0)
            },
        }
    }
}

/// `ContinuousPageLayout`.
#[derive(Debug)]
pub struct ContinuousPageLayout {
    pages: Vec<PageEntry>,
    content_width: i32,
    total_height: i64,
}

impl ContinuousPageLayout {
    /// The C# ctor: pages stack with no spacing; with
    /// `preserve_source_size` (Original fit) pages keep their native
    /// width and center inside `content_width`, otherwise they scale
    /// to the shared content width.
    pub fn new(
        source_pages: &[SourcePage],
        content_width: i32,
        preserve_source_size: bool,
    ) -> ContinuousPageLayout {
        let content_width = content_width.max(1);
        let mut pages = Vec::with_capacity(source_pages.len());
        let mut top: i64 = 0;
        for source in source_pages {
            let valid = source.source_size.0 > 0 && source.source_size.1 > 0;
            let rendered_width = if valid {
                if preserve_source_size {
                    source.source_size.0
                } else {
                    content_width
                }
            } else {
                0
            };
            let height = if preserve_source_size {
                if valid {
                    i64::from(source.source_size.1)
                } else {
                    0
                }
            } else {
                scaled_height(content_width, source.source_size)
            };
            let left = if preserve_source_size {
                ((content_width - rendered_width) / 2).max(0)
            } else {
                0
            };
            pages.push(PageEntry {
                page: source.page,
                source_size: source.source_size,
                bounds: to_rectangle(left, top, rendered_width, height),
                top,
                height,
            });
            top = saturating_add(top, height);
        }
        ContinuousPageLayout {
            pages,
            content_width,
            total_height: top,
        }
    }

    /// Full virtual height (may exceed `i32::MAX`).
    pub fn total_height(&self) -> i64 {
        self.total_height
    }

    /// `TotalSize`.
    pub fn total_size(&self) -> (i32, i32) {
        (self.content_width, clamp_to_i32(self.total_height))
    }

    pub fn pages(&self) -> &[PageEntry] {
        &self.pages
    }

    /// `GetVisible` — pages intersecting the viewport (half-open
    /// intervals; binary search on `Bottom`).
    pub fn get_visible(&self, viewport: &crate::reader::display::Rect) -> Vec<&PageEntry> {
        let mut result = Vec::new();
        if self.pages.is_empty() || viewport.w <= 0 || viewport.h <= 0 {
            return result;
        }
        let viewport_left = i64::from(viewport.x);
        let viewport_right = saturating_add(viewport_left, i64::from(viewport.w));
        if viewport_right <= 0 || viewport_left >= i64::from(self.content_width) {
            return result;
        }
        let viewport_top = i64::from(viewport.y);
        let viewport_bottom = saturating_add(viewport_top, i64::from(viewport.h));
        let mut low = 0usize;
        let mut high = self.pages.len();
        while low < high {
            let middle = low + (high - low) / 2;
            if self.pages[middle].bottom() <= viewport_top {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        for page in &self.pages[low..] {
            if page.top() >= viewport_bottom {
                break;
            }
            if page.bounds.w <= 0 || page.height() <= 0 {
                continue;
            }
            if page.bounds.x < i32::try_from(viewport_right).unwrap_or(i32::MAX)
                && page.bounds.right() > i32::try_from(viewport_left).unwrap_or(i32::MIN)
                && page.top() < viewport_bottom
                && page.bottom() > viewport_top
            {
                result.push(page);
            }
        }
        result
    }

    /// `HitTest` — the page containing `y`, clamped to the strip.
    pub fn hit_test(&self, y: i64) -> Option<&PageEntry> {
        if self.pages.is_empty() {
            return None;
        }
        if self.total_height <= 0 {
            return Some(&self.pages[0]);
        }
        let coordinate = y.clamp(0, self.total_height - 1);
        let mut low: i64 = 0;
        let mut high: i64 = self.pages.len() as i64 - 1;
        while low <= high {
            let middle = low + (high - low) / 2;
            let page = &self.pages[middle as usize];
            if coordinate < page.top() {
                high = middle - 1;
            } else if coordinate >= page.bottom() || page.height() <= 0 {
                low = middle + 1;
            } else {
                return Some(page);
            }
        }
        // Zero-sized pages have no hit area — nearest in input order.
        if low >= self.pages.len() as i64 {
            return self.pages.last();
        }
        if high < 0 {
            return Some(&self.pages[0]);
        }
        if self.pages[low as usize].top() > coordinate {
            Some(&self.pages[high.max(0) as usize])
        } else {
            Some(&self.pages[(low as usize).min(self.pages.len() - 1)])
        }
    }

    /// `CaptureAnchor`.
    pub fn capture_anchor(&self, y: i64) -> Anchor {
        let Some(page) = self.hit_test(y) else {
            return Anchor::new(0, 0.0);
        };
        let coordinate = y.clamp(0, (self.total_height - 1).max(0));
        let offset = (coordinate - page.top()).clamp(0, (page.height() - 1).max(0));
        let relative = if page.height() > 0 {
            offset as f64 / page.height() as f64
        } else {
            0.0
        };
        Anchor::new(page.page, relative)
    }

    /// `ResolveAnchor` — clamped virtual y for an anchor; the nearest
    /// page by number stands in for a missing one.
    pub fn resolve_anchor(&self, anchor: Anchor) -> i64 {
        if self.pages.is_empty() {
            return 0;
        }
        let page = self.find_page(anchor.page);
        let offset = ((anchor.relative_offset * page.height() as f64) as i64)
            .clamp(0, (page.height() - 1).max(0));
        let mut coordinate = saturating_add(page.top(), offset);
        if self.total_height > 0 {
            coordinate = coordinate.clamp(0, self.total_height - 1);
        }
        coordinate
    }

    fn find_page(&self, page_number: usize) -> &PageEntry {
        if let Some(exact) = self.pages.iter().find(|p| p.page == page_number) {
            return exact;
        }
        let mut nearest = &self.pages[0];
        let mut best = distance(nearest.page, page_number);
        for candidate in &self.pages[1..] {
            let candidate_distance = distance(candidate.page, page_number);
            if candidate_distance < best {
                nearest = candidate;
                best = candidate_distance;
            }
        }
        nearest
    }
}

fn distance(left: usize, right: usize) -> i64 {
    (left as i64 - right as i64).abs()
}

/// `GetScaledHeight` — round-half-up scaling with a 1px floor.
fn scaled_height(width: i32, source_size: (i32, i32)) -> i64 {
    if source_size.0 <= 0 || source_size.1 <= 0 {
        return 0;
    }
    let numerator = saturating_multiply(i64::from(width), i64::from(source_size.1));
    let rounded = saturating_add(numerator, i64::from(source_size.0) / 2);
    let height = rounded / i64::from(source_size.0);
    if height > 0 {
        height
    } else {
        1
    }
}

fn saturating_multiply(left: i64, right: i64) -> i64 {
    if left <= 0 || right <= 0 {
        return 0;
    }
    if left > i64::MAX / right {
        return i64::MAX;
    }
    left * right
}

fn saturating_add(left: i64, right: i64) -> i64 {
    left.saturating_add(right)
}

fn clamp_to_i32(value: i64) -> i32 {
    if value <= 0 {
        0
    } else {
        value.min(i32::MAX as i64) as i32
    }
}

fn to_rectangle(left: i32, top: i64, width: i32, height: i64) -> crate::reader::display::Rect {
    let y = clamp_to_i32(top);
    let available = i64::from(i32::MAX) - i64::from(y);
    let clipped = height.min(available);
    crate::reader::display::Rect::new(left, y, width, clamp_to_i32(clipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::display::Rect;

    fn strip(preserve: bool) -> ContinuousPageLayout {
        ContinuousPageLayout::new(
            &[
                SourcePage {
                    page: 0,
                    source_size: (800, 1200),
                },
                SourcePage {
                    page: 1,
                    source_size: (1600, 1200),
                },
                SourcePage {
                    page: 2,
                    source_size: (800, 600),
                },
            ],
            800,
            preserve,
        )
    }

    #[test]
    fn pages_scale_to_the_content_width() {
        let layout = strip(false);
        // 800x1200 stays; 1600x1200 → 800x600; 800x600 stays.
        assert_eq!(layout.total_size(), (800, 1200 + 600 + 600));
        assert_eq!(layout.pages()[1].bounds, Rect::new(0, 1200, 800, 600));
        assert_eq!(layout.pages()[2].bounds, Rect::new(0, 1800, 800, 600));
    }

    #[test]
    fn preserve_keeps_native_sizes_centered() {
        let layout = strip(true);
        // Widest page defines the content width… the ctor receives
        // it; here 800 → the 1600-wide page overflows as-is (C#
        // keeps source width).
        assert_eq!(layout.pages()[1].bounds, Rect::new(0, 1200, 1600, 1200));
    }

    #[test]
    fn get_visible_returns_intersecting_pages() {
        let layout = strip(false);
        let viewport = Rect::new(0, 1100, 800, 200);
        let visible: Vec<usize> = layout
            .get_visible(&viewport)
            .iter()
            .map(|p| p.page)
            .collect();
        assert_eq!(visible, vec![0, 1]);
        // A page ending exactly on the edge is excluded (half-open
        // intervals): page 1 ends at 1800, the viewport starts there.
        let visible: Vec<usize> = layout
            .get_visible(&Rect::new(0, 1800, 800, 300))
            .iter()
            .map(|p| p.page)
            .collect();
        assert_eq!(visible, vec![2]);
    }

    #[test]
    fn anchors_round_trip() {
        let layout = strip(false);
        let anchor = layout.capture_anchor(1500);
        assert_eq!(anchor.page, 1);
        let y = layout.resolve_anchor(anchor);
        assert_eq!(y, 1500);
        // Missing page resolves to the nearest available.
        let y = layout.resolve_anchor(Anchor::new(99, 0.5));
        assert!(y >= 0 && y < layout.total_height());
    }

    #[test]
    fn hit_test_clamps_to_the_strip() {
        let layout = strip(false);
        assert_eq!(layout.hit_test(-10).unwrap().page, 0);
        assert_eq!(layout.hit_test(10_000).unwrap().page, 2);
        assert_eq!(layout.hit_test(1199).unwrap().page, 0);
        assert_eq!(layout.hit_test(1200).unwrap().page, 1);
    }
}
