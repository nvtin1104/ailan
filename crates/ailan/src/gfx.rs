//! Direct3D11 → Direct2D → composition swapchain → DirectComposition.
//! Gives a per-pixel-alpha topmost window that only costs GPU time when drawn.

use std::collections::HashMap;

use windows::core::{Interface, Result, PCWSTR};
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1BitmapBrush1, ID2D1DeviceContext, ID2D1Factory1,
    ID2D1GradientStopCollection1, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_BUFFER_PRECISION_8BPC_UNORM, D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED, D2D1_COLOR_SPACE_SRGB,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
    D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
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
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
    pub fn scale_from_center(&self, factor: f32) -> Self {
        let (cx, cy) = self.center();
        let nw = (self.w * factor).max(0.0);
        let nh = (self.h * factor).max(0.0);
        Self::new(cx - nw * 0.5, cy - nh * 0.5, nw, nh)
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
    dc: ID2D1DeviceContext,
    swapchain: IDXGISwapChain1,
    dcomp: IDCompositionDevice,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    dwrite: IDWriteFactory,
    brush: ID2D1SolidColorBrush,
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

            let mut g = Self {
                _d3d: d3d,
                dxgi,
                dc,
                swapchain,
                dcomp,
                _target: target,
                _visual: visual,
                dwrite,
                brush,
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
            let mut cv = Canvas { dc: &self.dc, brush: &self.brush, dwrite: &self.dwrite, formats: &mut self.formats, alpha: 1.0 };
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
    dc: &'a ID2D1DeviceContext,
    brush: &'a ID2D1SolidColorBrush,
    dwrite: &'a IDWriteFactory,
    formats: &'a mut HashMap<Font, IDWriteTextFormat>,
    pub alpha: f32,
}

impl Canvas<'_> {
    fn set(&self, rgb: u32, a: f32) {
        unsafe { self.brush.SetColor(&color(rgb, a * self.alpha)) };
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

    fn stops(&self, stops: &[(f32, u32, f32)]) -> Option<ID2D1GradientStopCollection1> {
        let raw: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|&(pos, rgb, a)| D2D1_GRADIENT_STOP { position: pos, color: color(rgb, a * self.alpha) })
            .collect();
        unsafe {
            self.dc
                .CreateGradientStopCollection(
                    &raw,
                    D2D1_COLOR_SPACE_SRGB,
                    D2D1_COLOR_SPACE_SRGB,
                    D2D1_BUFFER_PRECISION_8BPC_UNORM,
                    D2D1_EXTEND_MODE_CLAMP,
                    D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED,
                )
                .ok()
        }
    }

    pub fn fill_round_linear(
        &self,
        r: Rect,
        radius: f32,
        start: (f32, f32),
        end: (f32, f32),
        stops: &[(f32, u32, f32)],
    ) {
        let Some(coll) = self.stops(stops) else { return };
        unsafe {
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: Vector2 { X: start.0, Y: start.1 },
                endPoint: Vector2 { X: end.0, Y: end.1 },
            };
            if let Ok(br) = self.dc.CreateLinearGradientBrush(&props, None, &coll) {
                let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
                self.dc.FillRoundedRectangle(&rr, &br);
            }
        }
    }

    pub fn stroke_round_linear(
        &self,
        r: Rect,
        radius: f32,
        width: f32,
        start: (f32, f32),
        end: (f32, f32),
        stops: &[(f32, u32, f32)],
    ) {
        let Some(coll) = self.stops(stops) else { return };
        unsafe {
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: Vector2 { X: start.0, Y: start.1 },
                endPoint: Vector2 { X: end.0, Y: end.1 },
            };
            if let Ok(br) = self.dc.CreateLinearGradientBrush(&props, None, &coll) {
                let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
                self.dc.DrawRoundedRectangle(&rr, &br, width, None);
            }
        }
    }

    pub fn fill_round_radial(
        &self,
        r: Rect,
        radius: f32,
        center: (f32, f32),
        rx: f32,
        ry: f32,
        stops: &[(f32, u32, f32)],
    ) {
        let Some(coll) = self.stops(stops) else { return };
        unsafe {
            let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center: Vector2 { X: center.0, Y: center.1 },
                gradientOriginOffset: Vector2 { X: 0.0, Y: 0.0 },
                radiusX: rx.max(1.0),
                radiusY: ry.max(1.0),
            };
            if let Ok(br) = self.dc.CreateRadialGradientBrush(&props, None, &coll) {
                let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
                self.dc.FillRoundedRectangle(&rr, &br);
            }
        }
    }

    pub fn stroke_round_radial(
        &self,
        r: Rect,
        radius: f32,
        width: f32,
        center: (f32, f32),
        rad: f32,
        stops: &[(f32, u32, f32)],
    ) {
        let Some(coll) = self.stops(stops) else { return };
        unsafe {
            let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center: Vector2 { X: center.0, Y: center.1 },
                gradientOriginOffset: Vector2 { X: 0.0, Y: 0.0 },
                radiusX: rad.max(1.0),
                radiusY: rad.max(1.0),
            };
            if let Ok(br) = self.dc.CreateRadialGradientBrush(&props, None, &coll) {
                let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
                self.dc.DrawRoundedRectangle(&rr, &br, width, None);
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
