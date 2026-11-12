//! Direct3D11 → Direct2D → composition swapchain → DirectComposition.
//! Gives a per-pixel-alpha topmost window that only costs GPU time when drawn.

pub mod types;
pub use types::*;

use std::collections::HashMap;

use windows::core::{Interface, Result, PCWSTR};
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW,
    D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT, D2D_SIZE_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1BitmapBrush1, ID2D1DeviceContext, ID2D1Factory1, ID2D1GradientStopCollection,
    ID2D1Brush, ID2D1Layer, ID2D1LinearGradientBrush, ID2D1RadialGradientBrush, ID2D1RenderTarget, ID2D1SolidColorBrush,
    D2D1_EXTEND_MODE_CLAMP, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1,
    D2D1_GAMMA_2_2, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES,
    ID2D1StrokeStyle, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_CAP_STYLE_ROUND, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_LINE_JOIN_ROUND, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES1, D2D1_SWEEP_DIRECTION_CLOCKWISE,
    D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1,
    IDWriteTypography, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_FEATURE, DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_HIT_TEST_METRICS, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, IDXGIDevice, IDXGIDevice3, IDXGIFactory2, IDXGISurface, IDXGISwapChain1,
    DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows_numerics::{Matrix3x2, Vector2};

pub const TEXT_FONT: &str = "Segoe UI Variable Text";
pub const ICON_FONT_FLUENT: &str = "Segoe Fluent Icons";
pub const ICON_FONT_MDL2: &str = "Segoe MDL2 Assets";

fn detect_icon_font(dwrite: &IDWriteFactory) -> &'static str {
    unsafe {
        let mut collection: Option<IDWriteFontCollection> = None;
        if dwrite.GetSystemFontCollection(&mut collection, false).is_ok() {
            if let Some(col) = collection {
                let mut index = 0u32;
                let mut exists = windows::core::BOOL::default();
                let fluent = windows::core::w!("Segoe Fluent Icons");
                if col.FindFamilyName(windows::core::PCWSTR(fluent.as_ptr()), &mut index, &mut exists).is_ok() && exists.as_bool() {
                    return ICON_FONT_FLUENT;
                }
            }
        }
    }
    ICON_FONT_MDL2
}

pub fn get_icon_font(dwrite: &IDWriteFactory) -> &'static str {
    static FONT: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    *FONT.get_or_init(|| detect_icon_font(dwrite))
}

/// Device-bound resources reused across frames (released together with the `Gfx`).
#[derive(Default)]
struct GfxCache {
    stops: HashMap<Gradient, ID2D1GradientStopCollection>,
    linear: HashMap<Gradient, ID2D1LinearGradientBrush>,
    radial: HashMap<Gradient, ID2D1RadialGradientBrush>,
    /// Per-colour brushes used as DirectWrite drawing effects for rich text.
    ink: HashMap<u32, ID2D1SolidColorBrush>,
    tabular: Option<IDWriteTypography>,
}

fn pt(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

/// UTF-16 index of a UTF-8 byte offset (snapped down to a char boundary, never panics).
fn utf16_index(s: &str, byte: usize) -> u32 {
    let mut b = byte.min(s.len());
    while b > 0 && !s.is_char_boundary(b) {
        b -= 1;
    }
    s[..b].encode_utf16().count() as u32
}

pub struct Gfx {
    _d3d: ID3D11Device,
    dxgi: IDXGIDevice,
    factory: ID2D1Factory1,
    dc: ID2D1DeviceContext,
    swapchain: IDXGISwapChain1,
    dcomp: IDCompositionDevice,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    dwrite: IDWriteFactory,
    brush: ID2D1SolidColorBrush,
    round_stroke: Option<ID2D1StrokeStyle>,
    formats: HashMap<Font, IDWriteTextFormat>,
    cache: GfxCache,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}

impl Gfx {
    pub fn new(hwnd: HWND, width: u32, height: u32, scale: f32) -> Result<Self> {
        unsafe {
            let mut d3d = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                None,
            )?;
            let d3d = d3d.unwrap();
            let dxgi: IDXGIDevice = d3d.cast()?;

            let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let d2d_device = factory.CreateDevice(&dxgi)?;
            let dc = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

            let dxgi_factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width.max(1),
                Height: height.max(1),
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                ..Default::default()
            };
            let swapchain = dxgi_factory.CreateSwapChainForComposition(&dxgi, &desc, None)?;

            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
            let target = dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = dcomp.CreateVisual()?;
            visual.SetContent(&swapchain)?;
            target.SetRoot(&visual)?;
            dcomp.Commit()?;

            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let brush = dc.CreateSolidColorBrush(&color(0xFFFFFF, 1.0), None)?;
            let stroke_props = D2D1_STROKE_STYLE_PROPERTIES1 {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                ..Default::default()
            };
            let round_stroke = factory.CreateStrokeStyle(&stroke_props, None).and_then(|s| s.cast::<ID2D1StrokeStyle>()).ok();

            let mut g = Self {
                _d3d: d3d,
                dxgi,
                factory,
                dc,
                swapchain,
                dcomp,
                _target: target,
                _visual: visual,
                dwrite,
                brush,
                round_stroke,
                formats: HashMap::new(),
                cache: GfxCache::default(),
                width: width.max(1),
                height: height.max(1),
                scale,
            };
            g.bind_target()?;
            Ok(g)
        }
    }

    fn bind_target(&mut self) -> Result<()> {
        unsafe {
            let surface: IDXGISurface = self.swapchain.GetBuffer(0)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0 * self.scale,
                dpiY: 96.0 * self.scale,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            };
            let bmp = self.dc.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
            self.dc.SetTarget(&bmp);
            self.dc.SetDpi(96.0 * self.scale, 96.0 * self.scale);
        }
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32, scale: f32) -> Result<()> {
        self.scale = scale;
        if width.max(1) != self.width || height.max(1) != self.height {
            self.width = width.max(1);
            self.height = height.max(1);
            unsafe {
                self.dc.SetTarget(None);
                self.swapchain.ResizeBuffers(0, self.width, self.height, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SWAP_CHAIN_FLAG(0))?;
            }
        }
        self.bind_target()
    }

    /// Draw one frame. `Err` means the device was lost; recreate `Gfx`.
    pub fn draw(&mut self, f: impl FnOnce(&mut Canvas)) -> Result<()> {
        unsafe {
            self.dc.BeginDraw();
            self.dc.SetTransform(&Matrix3x2::identity());
            self.dc.Clear(Some(&color(0, 0.0)));
        }
        {
            let mut cv = Canvas {
                factory: &self.factory,
                dc: &self.dc,
                brush: &self.brush,
                round_stroke: self.round_stroke.as_ref(),
                dwrite: &self.dwrite,
                formats: &mut self.formats,
                cache: &mut self.cache,
                alpha: 1.0,
                scale: self.scale,
            };
            f(&mut cv);
        }
        unsafe {
            self.dc.EndDraw(None, None)?;
            self.swapchain.Present(1, DXGI_PRESENT(0)).ok()?;
        }
        Ok(())
    }

    /// Release cached GPU memory while hidden.
    pub fn trim(&self) {
        unsafe {
            if let Ok(d3) = self.dxgi.cast::<IDXGIDevice3>() {
                d3.Trim();
            }
            let _ = self.dcomp.Commit();
        }
    }

    pub fn create_bitmap(&self, w: u32, h: u32, pixels: &[u8]) -> Result<ID2D1Bitmap1> {
        unsafe {
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
                ..Default::default()
            };
            self.dc.CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(pixels.as_ptr() as _), w * 4, &props)
        }
    }

    pub fn create_bitmap_brush(&self, bmp: &ID2D1Bitmap1) -> Result<ID2D1BitmapBrush1> {
        unsafe { self.dc.CreateBitmapBrush(bmp, None, None) }
    }
}

/// Immediate-mode drawing helpers in DIPs. `alpha` multiplies every color
/// (used for cross-fading collapsed/expanded content).
pub struct Canvas<'a> {
    factory: &'a ID2D1Factory1,
    dc: &'a ID2D1DeviceContext,
    brush: &'a ID2D1SolidColorBrush,
    round_stroke: Option<&'a ID2D1StrokeStyle>,
    dwrite: &'a IDWriteFactory,
    formats: &'a mut HashMap<Font, IDWriteTextFormat>,
    cache: &'a mut GfxCache,
    pub alpha: f32,
    pub scale: f32,
}

impl Canvas<'_> {
    fn set(&self, rgb: u32, a: f32) {
        unsafe { self.brush.SetColor(&color(rgb, a * self.alpha)) };
    }

    pub fn push_clip(&self, r: Rect) {
        unsafe {
            self.dc.PushAxisAlignedClip(&r.d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
    }

    pub fn pop_clip(&self) {
        unsafe {
            self.dc.PopAxisAlignedClip();
        }
    }

    /// 1-physical-pixel crisp hairline border.
    pub fn hairline_round(&self, r: Rect, radius: f32, rgb: u32, a: f32) {
        let half_px = (0.5 / self.scale).max(0.25);
        let inset_r = r.inset(half_px, half_px);
        self.stroke_round(inset_r, (radius - half_px).max(0.0), half_px * 2.0, rgb, a);
    }

    pub fn fill_rect(&self, r: Rect, rgb: u32, a: f32) {
        self.set(rgb, a);
        unsafe { self.dc.FillRectangle(&r.d2d(), self.brush) };
    }

    pub fn fill_round(&self, r: Rect, radius: f32, rgb: u32, a: f32) {
        self.set(rgb, a);
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        unsafe { self.dc.FillRoundedRectangle(&rr, self.brush) };
    }

    pub fn stroke_round(&self, r: Rect, radius: f32, width: f32, rgb: u32, a: f32) {
        self.set(rgb, a);
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        unsafe { self.dc.DrawRoundedRectangle(&rr, self.brush, width, None) };
    }

    pub fn fill_circle(&self, cx: f32, cy: f32, radius: f32, rgb: u32, a: f32) {
        self.set(rgb, a);
        let e = D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: radius, radiusY: radius };
        unsafe { self.dc.FillEllipse(&e, self.brush) };
    }

    pub fn stroke_circle(&self, cx: f32, cy: f32, radius: f32, width: f32, rgb: u32, a: f32) {
        self.set(rgb, a);
        let e = D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: radius, radiusY: radius };
        unsafe { self.dc.DrawEllipse(&e, self.brush, width, None) };
    }

    /// Stroke a clockwise circular arc starting at `start_deg` (-90.0 = 12 o'clock)
    /// and sweeping `sweep_deg` degrees, with round line caps.
    pub fn stroke_arc(&self, cx: f32, cy: f32, radius: f32, start_deg: f32, sweep_deg: f32, width: f32, rgb: u32, a: f32) {
        let sweep = sweep_deg.clamp(0.0, 360.0);
        if sweep <= 0.5 {
            return;
        }
        if sweep >= 359.5 {
            self.stroke_circle(cx, cy, radius, width, rgb, a);
            return;
        }
        let s_rad = start_deg.to_radians();
        let e_rad = (start_deg + sweep).to_radians();
        let start_pt = Vector2 { X: cx + radius * s_rad.cos(), Y: cy + radius * s_rad.sin() };
        let end_pt = Vector2 { X: cx + radius * e_rad.cos(), Y: cy + radius * e_rad.sin() };
        let arc_size = if sweep > 180.0 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL };

        unsafe {
            let Ok(path) = self.factory.CreatePathGeometry() else { return };
            let Ok(sink) = path.Open() else { return };
            sink.BeginFigure(start_pt, D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: end_pt,
                size: D2D_SIZE_F { width: radius, height: radius },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: arc_size,
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            if sink.Close().is_ok() {
                self.set(rgb, a);
                self.dc.DrawGeometry(&path, self.brush, width, self.round_stroke);
            }
        }
    }

    /// Fill the seamless hardware-notch silhouette with concave top ears (`ear`) on both
    /// left and right sides at `y = 0` and rounded convex bottom corners (`radius`).
    pub fn fill_notch(&self, r: Rect, radius: f32, ear: f32, rgb: u32, a: f32) {
        let e = ear.max(0.0).min(r.h);
        let rad = radius.max(0.0).min(r.w * 0.5).min((r.h - e).max(0.0));
        if e < 0.5 {
            let shell = Rect::new(r.x, -rad, r.w, r.h + rad);
            self.fill_round(shell, rad, rgb, a);
            return;
        }
        unsafe {
            let Ok(path) = self.factory.CreatePathGeometry() else { return };
            let Ok(sink) = path.Open() else { return };

            let left = r.x;
            let right = r.right();
            let bot = r.bottom();

            sink.BeginFigure(Vector2 { X: left - e, Y: 0.0 }, D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLine(Vector2 { X: right + e, Y: 0.0 });
            // Right top concave ear: (right + e, 0) -> (right, e) around center (right + e, e)
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: Vector2 { X: right, Y: e },
                size: D2D_SIZE_F { width: e, height: e },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
            sink.AddLine(Vector2 { X: right, Y: bot - rad });
            // Bottom-right convex corner: (right, bot - rad) -> (right - rad, bot)
            if rad > 0.5 {
                sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: Vector2 { X: right - rad, Y: bot },
                    size: D2D_SIZE_F { width: rad, height: rad },
                    rotationAngle: 0.0,
                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: D2D1_ARC_SIZE_SMALL,
                });
            }
            sink.AddLine(Vector2 { X: left + rad, Y: bot });
            // Bottom-left convex corner: (left + rad, bot) -> (left, bot - rad)
            if rad > 0.5 {
                sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: Vector2 { X: left, Y: bot - rad },
                    size: D2D_SIZE_F { width: rad, height: rad },
                    rotationAngle: 0.0,
                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: D2D1_ARC_SIZE_SMALL,
                });
            }
            sink.AddLine(Vector2 { X: left, Y: e });
            // Left top concave ear: (left, e) -> (left - e, 0) around center (left - e, e)
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: Vector2 { X: left - e, Y: 0.0 },
                size: D2D_SIZE_F { width: e, height: e },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            if sink.Close().is_ok() {
                self.set(rgb, a);
                self.dc.FillGeometry(&path, self.brush, None);
            }
        }
    }

    fn format(&mut self, font: Font) -> Option<IDWriteTextFormat> {
        if let Some(f) = self.formats.get(&font) {
            return Some(f.clone());
        }
        let family_str = if font.icon { get_icon_font(self.dwrite) } else { TEXT_FONT };
        let family = super::platform::wide(family_str);
        let fmt = unsafe {
            self.dwrite
                .CreateTextFormat(
                    PCWSTR(family.as_ptr()),
                    None,
                    DWRITE_FONT_WEIGHT(font.weight as i32),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    font.size2 as f32 / 2.0,
                    windows::core::w!("en-us"),
                )
                .ok()?
        };
        unsafe {
            let align: DWRITE_TEXT_ALIGNMENT = match font.align {
                Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
            };
            let _ = fmt.SetTextAlignment(align);
            let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            if let Ok(sign) = self.dwrite.CreateEllipsisTrimmingSign(&fmt) {
                let trimming = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, ..Default::default() };
                let _ = fmt.SetTrimming(&trimming, &sign);
            }
        }
        self.formats.insert(font, fmt.clone());
        Some(fmt)
    }

    /// Single line of text, vertically centred in `r`, ellipsised if too long.
    pub fn text(&mut self, s: &str, font: Font, r: Rect, rgb: u32, a: f32) {
        if s.is_empty() || r.w <= 0.0 {
            return;
        }
        let Some(fmt) = self.format(font) else { return };
        self.set(rgb, a);
        let w: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            self.dc.DrawText(
                &w,
                &fmt,
                &r.d2d(),
                self.brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
    }

    /// Icon glyph from Segoe MDL2 Assets centred in `r`.
    pub fn glyph(&mut self, ch: char, size: f32, r: Rect, rgb: u32, a: f32) {
        let mut buf = [0u8; 4];
        self.text(ch.encode_utf8(&mut buf), Font::icon(size), r, rgb, a);
    }

    /// Draw subsequent primitives translated by `(tx, ty)` and uniformly scaled by `s`
    /// (DIP space). Used to render the real notch UI as a miniature live preview.
    pub fn set_transform(&self, tx: f32, ty: f32, s: f32) {
        let m = Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: tx, M32: ty };
        unsafe { self.dc.SetTransform(&m) };
    }

    pub fn reset_transform(&self) {
        unsafe { self.dc.SetTransform(&Matrix3x2::identity()) };
    }

    /// Start a layer whose content is faded by the alpha of gradient `g` running `from` → `to`
    /// (an opacity mask: content fades to transparent, the backdrop stays untouched).
    /// Must be balanced with [`Canvas::pop_layer`]. Returns false (and pushes nothing) on failure.
    pub fn push_fade_layer(&mut self, bounds: Rect, g: Gradient, from: (f32, f32), to: (f32, f32)) -> bool {
        let Some(brush) = self.linear(g, from, to, 1.0) else { return false };
        let Ok(mask) = brush.cast::<ID2D1Brush>() else { return false };
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: bounds.d2d(),
            geometricMask: std::mem::ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: std::mem::ManuallyDrop::new(Some(mask)),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        unsafe {
            self.dc.PushLayer(&params, None::<&ID2D1Layer>);
            // D2D holds its own reference while the layer is active; release ours.
            std::mem::ManuallyDrop::drop(&mut params.opacityBrush);
        }
        true
    }

    pub fn pop_layer(&self) {
        unsafe { self.dc.PopLayer() };
    }

    /// Fill a rect whose corners have individual radii `[top-left, top-right, bottom-right, bottom-left]`.
    pub fn fill_round_corners(&self, r: Rect, radii: [f32; 4], rgb: u32, a: f32) {
        let max = (r.w * 0.5).min(r.h * 0.5);
        let [tl, tr, br, bl] = radii.map(|v| v.clamp(0.0, max));
        let arc = |sink: &windows::Win32::Graphics::Direct2D::ID2D1GeometrySink, x: f32, y: f32, rad: f32| unsafe {
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: pt(x, y),
                size: D2D_SIZE_F { width: rad, height: rad },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
        };
        unsafe {
            let Ok(path) = self.factory.CreatePathGeometry() else { return };
            let Ok(sink) = path.Open() else { return };
            sink.BeginFigure(pt(r.x + tl, r.y), D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLine(pt(r.right() - tr, r.y));
            if tr > 0.0 {
                arc(&sink, r.right(), r.y + tr, tr);
            }
            sink.AddLine(pt(r.right(), r.bottom() - br));
            if br > 0.0 {
                arc(&sink, r.right() - br, r.bottom(), br);
            }
            sink.AddLine(pt(r.x + bl, r.bottom()));
            if bl > 0.0 {
                arc(&sink, r.x, r.bottom() - bl, bl);
            }
            sink.AddLine(pt(r.x, r.y + tl));
            if tl > 0.0 {
                arc(&sink, r.x + tl, r.y, tl);
            }
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            if sink.Close().is_ok() {
                self.set(rgb, a);
                self.dc.FillGeometry(&path, self.brush, None);
            }
        }
    }

    // ---- gradients ---------------------------------------------------------------

    fn stop_collection(&mut self, g: Gradient) -> Option<ID2D1GradientStopCollection> {
        if let Some(c) = self.cache.stops.get(&g) {
            return Some(c.clone());
        }
        let stops = g.stops();
        let rt: &ID2D1RenderTarget = self.dc;
        let c = unsafe { rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).ok()? };
        self.cache.stops.insert(g, c.clone());
        Some(c)
    }

    fn linear(&mut self, g: Gradient, from: (f32, f32), to: (f32, f32), a: f32) -> Option<ID2D1LinearGradientBrush> {
        let brush = match self.cache.linear.get(&g) {
            Some(b) => b.clone(),
            None => {
                let stops = self.stop_collection(g)?;
                let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(from.0, from.1), endPoint: pt(to.0, to.1) };
                let b = unsafe { self.dc.CreateLinearGradientBrush(&props, None, &stops).ok()? };
                self.cache.linear.insert(g, b.clone());
                b
            }
        };
        unsafe {
            brush.SetStartPoint(pt(from.0, from.1));
            brush.SetEndPoint(pt(to.0, to.1));
            brush.SetOpacity((a * self.alpha).clamp(0.0, 1.0));
        }
        Some(brush)
    }

    fn radial(&mut self, g: Gradient, center: (f32, f32), radius: f32, a: f32) -> Option<ID2D1RadialGradientBrush> {
        let brush = match self.cache.radial.get(&g) {
            Some(b) => b.clone(),
            None => {
                let stops = self.stop_collection(g)?;
                let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                    center: pt(center.0, center.1),
                    gradientOriginOffset: pt(0.0, 0.0),
                    radiusX: radius,
                    radiusY: radius,
                };
                let b = unsafe { self.dc.CreateRadialGradientBrush(&props, None, &stops).ok()? };
                self.cache.radial.insert(g, b.clone());
                b
            }
        };
        unsafe {
            brush.SetCenter(pt(center.0, center.1));
            brush.SetRadiusX(radius);
            brush.SetRadiusY(radius);
            brush.SetOpacity((a * self.alpha).clamp(0.0, 1.0));
        }
        Some(brush)
    }

    /// Fill a rect with a linear gradient running `from` → `to` (absolute DIPs).
    pub fn fill_rect_linear(&mut self, r: Rect, g: Gradient, from: (f32, f32), to: (f32, f32), a: f32) {
        if let Some(b) = self.linear(g, from, to, a) {
            unsafe { self.dc.FillRectangle(&r.d2d(), &b) };
        }
    }

    pub fn fill_round_linear(&mut self, r: Rect, radius: f32, g: Gradient, from: (f32, f32), to: (f32, f32), a: f32) {
        if let Some(b) = self.linear(g, from, to, a) {
            let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
            unsafe { self.dc.FillRoundedRectangle(&rr, &b) };
        }
    }

    /// Crisp 1-physical-pixel rounded border painted with a linear gradient.
    pub fn hairline_round_linear(&mut self, r: Rect, radius: f32, g: Gradient, from: (f32, f32), to: (f32, f32), a: f32) {
        let half_px = (0.5 / self.scale).max(0.25);
        let ir = r.inset(half_px, half_px);
        let rad = (radius - half_px).max(0.0);
        if let Some(b) = self.linear(g, from, to, a) {
            let rr = D2D1_ROUNDED_RECT { rect: ir.d2d(), radiusX: rad, radiusY: rad };
            unsafe { self.dc.DrawRoundedRectangle(&rr, &b, half_px * 2.0, None) };
        }
    }

    /// Fill a rounded rect with a radial gradient centred at `center` (clipped to the shape).
    pub fn fill_round_radial(&mut self, r: Rect, radius: f32, g: Gradient, center: (f32, f32), glow_r: f32, a: f32) {
        if let Some(b) = self.radial(g, center, glow_r, a) {
            let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
            unsafe { self.dc.FillRoundedRectangle(&rr, &b) };
        }
    }

    /// Soft circular glow.
    pub fn fill_circle_radial(&mut self, cx: f32, cy: f32, radius: f32, g: Gradient, a: f32) {
        if let Some(b) = self.radial(g, (cx, cy), radius, a) {
            let e = D2D1_ELLIPSE { point: pt(cx, cy), radiusX: radius, radiusY: radius };
            unsafe { self.dc.FillEllipse(&e, &b) };
        }
    }

    /// Stroke the first `progress` (0..1) of a polyline's length with round caps/joins —
    /// used for the hand-drawn check mark.
    pub fn stroke_polyline(&self, pts: &[(f32, f32)], progress: f32, width: f32, rgb: u32, a: f32) {
        if pts.len() < 2 || progress <= 0.001 {
            return;
        }
        let lens: Vec<f32> = pts.windows(2).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).collect();
        let total: f32 = lens.iter().sum();
        let mut left = total * progress.clamp(0.0, 1.0);
        unsafe {
            let Ok(path) = self.factory.CreatePathGeometry() else { return };
            let Ok(sink) = path.Open() else { return };
            sink.BeginFigure(pt(pts[0].0, pts[0].1), D2D1_FIGURE_BEGIN_HOLLOW);
            for (i, len) in lens.iter().enumerate() {
                let (p0, p1) = (pts[i], pts[i + 1]);
                if left >= *len {
                    sink.AddLine(pt(p1.0, p1.1));
                    left -= len;
                } else {
                    let f = if *len > 0.0 { left / len } else { 0.0 };
                    sink.AddLine(pt(p0.0 + (p1.0 - p0.0) * f, p0.1 + (p1.1 - p0.1) * f));
                    break;
                }
            }
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            if sink.Close().is_ok() {
                self.set(rgb, a);
                self.dc.DrawGeometry(&path, self.brush, width, self.round_stroke);
            }
        }
    }

    // ---- text layouts ----------------------------------------------------------------

    fn tabular(&mut self) -> Option<IDWriteTypography> {
        if let Some(t) = &self.cache.tabular {
            return Some(t.clone());
        }
        unsafe {
            let t = self.dwrite.CreateTypography().ok()?;
            t.AddFontFeature(DWRITE_FONT_FEATURE { nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES, parameter: 1 }).ok()?;
            self.cache.tabular = Some(t.clone());
            Some(t)
        }
    }

    fn layout(&mut self, w: &[u16], font: Font, max_w: f32, max_h: f32, style: TextStyle) -> Option<IDWriteTextLayout> {
        let fmt = self.format(font)?;
        let layout = unsafe { self.dwrite.CreateTextLayout(w, &fmt, max_w.max(1.0), max_h.max(1.0)).ok()? };
        let all = DWRITE_TEXT_RANGE { startPosition: 0, length: w.len() as u32 };
        unsafe {
            if style.strike {
                let _ = layout.SetStrikethrough(true, all);
            }
            if style.tracking != 0.0 {
                if let Ok(l1) = layout.cast::<IDWriteTextLayout1>() {
                    let _ = l1.SetCharacterSpacing(0.0, style.tracking, 0.0, all);
                }
            }
            if style.tabular {
                if let Some(t) = self.tabular() {
                    let _ = layout.SetTypography(&t, all);
                }
            }
        }
        Some(layout)
    }

    /// Single-line text with tracking / tabular figures / strikethrough.
    pub fn text_styled(&mut self, s: &str, font: Font, r: Rect, rgb: u32, a: f32, style: TextStyle) {
        if s.is_empty() || r.w <= 0.0 {
            return;
        }
        if style == TextStyle::default() {
            self.text(s, font, r, rgb, a);
            return;
        }
        let w: Vec<u16> = s.encode_utf16().collect();
        let Some(layout) = self.layout(&w, font, r.w, r.h, style) else { return };
        self.set(rgb, a);
        self.push_clip(r);
        unsafe { self.dc.DrawTextLayout(pt(r.x, r.y), &layout, self.brush, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT) };
        self.pop_clip();
    }

    /// Advance width of `s` in DIPs.
    pub fn text_width(&mut self, s: &str, font: Font, style: TextStyle) -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        let w: Vec<u16> = s.encode_utf16().collect();
        let Some(layout) = self.layout(&w, font, 4096.0, 64.0, style) else { return 0.0 };
        let mut m = DWRITE_TEXT_METRICS::default();
        if unsafe { layout.GetMetrics(&mut m) }.is_ok() {
            m.widthIncludingTrailingWhitespace
        } else {
            0.0
        }
    }

    fn ink(&mut self, rgb: u32, a: f32) -> Option<ID2D1SolidColorBrush> {
        let c = color(rgb, a * self.alpha);
        if let Some(b) = self.cache.ink.get(&rgb) {
            unsafe { b.SetColor(&c) };
            return Some(b.clone());
        }
        let b = unsafe { self.dc.CreateSolidColorBrush(&c, None).ok()? };
        self.cache.ink.insert(rgb, b.clone());
        Some(b)
    }

    /// Editable single-line text with coloured runs, optional soft run backgrounds and a
    /// caret (`Some(rgb)`) at the end. Scrolls horizontally so the caret stays visible.
    pub fn text_rich(&mut self, s: &str, font: Font, r: Rect, rgb: u32, a: f32, spans: &[RichSpan], caret: Option<u32>) {
        let w: Vec<u16> = s.encode_utf16().collect();
        let Some(layout) = self.layout(&w, font, 4096.0, r.h, TextStyle::default()) else { return };
        let mut caret_x = 0.0f32;
        if !w.is_empty() {
            let (mut x, mut y) = (0.0f32, 0.0f32);
            let mut m = DWRITE_HIT_TEST_METRICS::default();
            if unsafe { layout.HitTestTextPosition(w.len() as u32 - 1, true, &mut x, &mut y, &mut m) }.is_ok() {
                caret_x = x;
            }
        }
        let ox = r.x - (caret_x - r.w + 6.0).max(0.0);
        self.push_clip(r);
        for sp in spans {
            let start = utf16_index(s, sp.bytes.start);
            let end = utf16_index(s, sp.bytes.end);
            if end <= start {
                continue;
            }
            let range = DWRITE_TEXT_RANGE { startPosition: start, length: end - start };
            if let Some(b) = self.ink(sp.rgb, a) {
                unsafe {
                    let _ = layout.SetDrawingEffect(&b, range);
                }
            }
            if let Some((bg, ba)) = sp.bg {
                let (mut x0, mut y0, mut x1, mut y1) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
                let mut m = DWRITE_HIT_TEST_METRICS::default();
                let ok = unsafe {
                    layout.HitTestTextPosition(start, false, &mut x0, &mut y0, &mut m).is_ok()
                        && layout.HitTestTextPosition(end - 1, true, &mut x1, &mut y1, &mut m).is_ok()
                };
                if ok {
                    let pill = Rect::new(ox + x0 - 4.0, r.y + y0 - 1.0, x1 - x0 + 8.0, m.height + 2.0);
                    self.fill_round(pill, 5.0, bg, ba);
                }
            }
        }
        self.set(rgb, a);
        unsafe { self.dc.DrawTextLayout(pt(ox, r.y), &layout, self.brush, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT) };
        if let Some(caret_rgb) = caret {
            let ch = font.size2 as f32 / 2.0 * 1.3;
            self.fill_round(Rect::new(ox + caret_x + 1.0, r.y + (r.h - ch) * 0.5, 1.5, ch), 0.75, caret_rgb, a);
        }
        self.pop_clip();
    }

    /// Fill a rounded rect with a bitmap scaled to cover it.
    pub fn image(&self, brush: &ID2D1BitmapBrush1, bmp_px: (u32, u32), r: Rect, radius: f32) {
        let sx = r.w / bmp_px.0 as f32;
        let sy = r.h / bmp_px.1 as f32;
        let m = Matrix3x2 { M11: sx, M12: 0.0, M21: 0.0, M22: sy, M31: r.x, M32: r.y };
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        unsafe {
            brush.SetTransform(&m);
            brush.SetOpacity(self.alpha);
            self.dc.FillRoundedRectangle(&rr, brush);
        }
    }
}
