//! Core 2D geometry, font, gradient, and rich-text types for `gfx`.

use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D_RECT_F};

pub fn color(rgb: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 0xFF) as f32 / 255.0,
        g: ((rgb >> 8) & 0xFF) as f32 / 255.0,
        b: (rgb & 0xFF) as f32 / 255.0,
        a,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
    pub fn inset(&self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy, (self.w - 2.0 * dx).max(0.0), (self.h - 2.0 * dy).max(0.0))
    }
    /// Overlap of two rects, or `None` when they don't touch.
    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
    pub(super) fn d2d(&self) -> D2D_RECT_F {
        D2D_RECT_F { left: self.x, top: self.y, right: self.right(), bottom: self.bottom() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Font {
    /// Size in DIP × 2 (allows half sizes while staying hashable).
    pub size2: u16,
    pub weight: u16,
    pub align: Align,
    pub icon: bool,
}

impl Font {
    pub const fn text(size: f32, weight: u16, align: Align) -> Self {
        Self { size2: (size * 2.0) as u16, weight, align, icon: false }
    }
    pub const fn icon(size: f32) -> Self {
        Self { size2: (size * 2.0) as u16, weight: 400, align: Align::Center, icon: true }
    }
}

/// A gradient stop table (up to 4 stops) usable as a hash key, so each distinct gradient
/// creates exactly one `ID2D1GradientStopCollection` + brush for the device's lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Gradient {
    n: u8,
    pos: [u16; 4],
    rgb: [u32; 4],
    alpha: [u16; 4],
}

impl Gradient {
    /// `stops` = `(position 0..1, 0xRRGGBB, alpha 0..1)`; stops beyond the 4th are ignored.
    pub const fn new(stops: &[(f32, u32, f32)]) -> Self {
        let mut g = Gradient { n: 0, pos: [0; 4], rgb: [0; 4], alpha: [0; 4] };
        let mut i = 0;
        while i < stops.len() && i < 4 {
            g.pos[i] = (stops[i].0 * 1000.0) as u16;
            g.rgb[i] = stops[i].1;
            g.alpha[i] = (stops[i].2 * 1000.0) as u16;
            i += 1;
        }
        g.n = i as u8;
        g
    }

    pub(super) fn stops(&self) -> Vec<D2D1_GRADIENT_STOP> {
        (0..self.n as usize)
            .map(|i| D2D1_GRADIENT_STOP {
                position: self.pos[i] as f32 / 1000.0,
                color: color(self.rgb[i], self.alpha[i] as f32 / 1000.0),
            })
            .collect()
    }
}

/// Extra text shaping options that need an `IDWriteTextLayout`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TextStyle {
    /// Extra spacing after each character, DIP.
    pub tracking: f32,
    /// OpenType `tnum`: fixed-width digits so clocks never jitter.
    pub tabular: bool,
    pub strike: bool,
}

impl TextStyle {
    pub const TABULAR: TextStyle = TextStyle { tracking: 0.0, tabular: true, strike: false };
    pub const STRIKE: TextStyle = TextStyle { tracking: 0.0, tabular: false, strike: true };
    pub const fn tracked(tracking: f32) -> Self {
        TextStyle { tracking, tabular: false, strike: false }
    }
}

/// A coloured run inside [`Canvas::text_rich`], in UTF-8 byte offsets of the string.
#[derive(Debug, Clone)]
pub struct RichSpan {
    pub bytes: std::ops::Range<usize>,
    pub rgb: u32,
    /// Optional soft pill drawn behind the run: `(rgb, alpha)`.
    pub bg: Option<(u32, f32)>,
}

