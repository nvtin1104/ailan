//! Direct3D11 → Direct2D → composition swapchain → DirectComposition.
//! Gives a per-pixel-alpha topmost window that only costs GPU time when drawn.

use std::collections::HashMap;

use windows::core::{Interface, Result, PCWSTR};
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW,
    D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1BitmapBrush1, ID2D1DeviceContext, ID2D1Factory1, ID2D1SolidColorBrush,
    ID2D1StrokeStyle, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_CAP_STYLE_ROUND, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_ROUNDED_RECT,
    D2D1_STROKE_STYLE_PROPERTIES1, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
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
pub const ICON_FONT: &str = "Segoe MDL2 Assets";

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
    fn d2d(&self) -> D2D_RECT_F {
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
                alpha: 1.0,
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

pub fn color(rgb: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 0xFF) as f32 / 255.0,
        g: ((rgb >> 8) & 0xFF) as f32 / 255.0,
        b: (rgb & 0xFF) as f32 / 255.0,
        a,
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
    pub alpha: f32,
}

impl Canvas<'_> {
    fn set(&self, rgb: u32, a: f32) {
        unsafe { self.brush.SetColor(&color(rgb, a * self.alpha)) };
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
        let family = super::platform::wide(if font.icon { ICON_FONT } else { TEXT_FONT });
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
