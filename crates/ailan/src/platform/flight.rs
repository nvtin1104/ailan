//! "Jump into the island" animation: when the Hub starts a focus session or adds a task,
//! a small glyph tile flies from the pressed control across the screen into the notch.
//!
//! The flight draws in a short-lived, click-through, topmost overlay window. Its GPU
//! resources exist only while a flight is in the air (~0.5 s) and are dropped on landing,
//! so an idle app keeps no extra surface and spends no CPU.

use std::time::Instant;

use windows::core::{w, Result};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateRectRgn, SetWindowRgn};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::gfx::{Gfx, Gradient, Rect};

/// Flight duration in seconds.
pub const DURATION: f32 = 0.52;
const MARGIN_DIP: f32 = 48.0;

extern "system" fn flight_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCHITTEST {
        // Never intercept the pointer.
        return LRESULT(HTTRANSPARENT as isize);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Position, scale and opacity of the flying tile at progress `t` (0..=1).
///
/// The path is a quadratic Bézier that rises steeply out of the start point, then glides
/// into the end point; eased in/out so it accelerates off the button and settles in.
pub fn flight_point(start: (f32, f32), end: (f32, f32), t: f32) -> (f32, f32, f32, f32) {
    let t = t.clamp(0.0, 1.0);
    let e = if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 };
    let ctrl = (start.0 + (end.0 - start.0) * 0.2, end.1 + (start.1 - end.1) * 0.15);
    let u = 1.0 - e;
    let x = u * u * start.0 + 2.0 * u * e * ctrl.0 + e * e * end.0;
    let y = u * u * start.1 + 2.0 * u * e * ctrl.1 + e * e * end.1;
    // Pop slightly on take-off, shrink as it is swallowed by the island.
    let scale = if t < 0.15 { 0.85 + t / 0.15 * 0.2 } else { 1.05 - (t - 0.15) / 0.85 * 0.55 };
    let alpha = ((1.0 - t) / 0.15).clamp(0.0, 1.0);
    (x, y, scale, alpha)
}

struct Run {
    /// Screen px.
    start: (f32, f32),
    end: (f32, f32),
    /// Overlay window origin on screen, px.
    origin: (i32, i32),
    scale: f32,
    t0: Instant,
    glyph: char,
    tint: u32,
}

pub struct Flight {
    hwnd: HWND,
    gfx: Option<Gfx>,
    run: Option<Run>,
}

impl Flight {
    pub fn create() -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = w!("AilanFlight");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(flight_wndproc),
                hInstance: instance.into(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_NOREDIRECTIONBITMAP,
                class,
                w!("Ailan flight"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            Ok(Self { hwnd, gfx: None, run: None })
        }
    }

    pub fn is_active(&self) -> bool {
        self.run.is_some()
    }

    /// Start a flight between two screen points (px). A flight already in the air is
    /// replaced. Fails silently (no animation) if the overlay cannot be set up.
    pub fn launch(&mut self, start: (f32, f32), end: (f32, f32), scale: f32, glyph: char, tint: u32) {
        // Bounding box of the curve (sampled) plus room for the glow.
        let m = MARGIN_DIP * scale;
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for i in 0..=24 {
            let (x, y, _, _) = flight_point(start, end, i as f32 / 24.0);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        let origin = ((x0 - m).floor() as i32, (y0 - m).floor() as i32);
        let size = (((x1 - x0) + 2.0 * m).ceil() as i32, ((y1 - y0) + 2.0 * m).ceil() as i32);
        unsafe {
            if SetWindowPos(self.hwnd, Some(HWND_TOPMOST), origin.0, origin.1, size.0, size.1, SWP_NOACTIVATE).is_err() {
                return;
            }
        }
        let gfx = match self.gfx.as_mut() {
            Some(g) => g.resize(size.0 as u32, size.1 as u32, scale).is_ok(),
            None => match Gfx::new(self.hwnd, size.0 as u32, size.1 as u32, scale) {
                Ok(g) => {
                    self.gfx = Some(g);
                    true
                }
                Err(e) => {
                    log::warn!("flight overlay: {e}");
                    false
                }
            },
        };
        if !gfx {
            return;
        }
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        self.run = Some(Run { start, end, origin, scale, t0: Instant::now(), glyph, tint });
    }

    /// Draw one frame. Returns `true` on the frame the flight lands.
    pub fn frame(&mut self) -> bool {
        let Some(run) = &self.run else { return false };
        let t = run.t0.elapsed().as_secs_f32() / DURATION;
        if t >= 1.0 {
            self.finish();
            return true;
        }
        let Some(g) = self.gfx.as_mut() else {
            self.finish();
            return true;
        };
        let to_dip = |p: (f32, f32)| ((p.0 - run.origin.0 as f32) / run.scale, (p.1 - run.origin.1 as f32) / run.scale);
        let (s, e) = (to_dip(run.start), to_dip(run.end));
        let tint = run.tint;
        let glyph = run.glyph;
        // Clip the window to the tile + its trail so only that small area is ever composited.
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for k in 0..=3 {
            let (x, y, sc, _) = flight_point(s, e, (t - k as f32 * 0.045).max(0.0));
            let r = 32.0 * sc.max(0.6);
            x0 = x0.min(x - r);
            y0 = y0.min(y - r);
            x1 = x1.max(x + r);
            y1 = y1.max(y + r);
        }
        let px = |v: f32| (v * run.scale).round() as i32;
        unsafe {
            let rgn = CreateRectRgn(px(x0), px(y0), px(x1), px(y1));
            // The system owns the region after SetWindowRgn.
            SetWindowRgn(self.hwnd, Some(rgn), false);
        }
        let glow = Gradient::new(&[(0.0, tint, 0.55), (1.0, tint, 0.0)]);
        let result = g.draw(|cv| {
            // Trail: fading echoes slightly behind the tile.
            for k in (1..=3).rev() {
                let tk = t - k as f32 * 0.045;
                if tk <= 0.0 {
                    continue;
                }
                let (x, y, sc, a) = flight_point(s, e, tk);
                let r = 6.0 * sc * (1.0 - k as f32 * 0.22);
                cv.fill_circle(x, y, r, tint, 0.28 * a / k as f32);
            }
            let (x, y, sc, a) = flight_point(s, e, t);
            cv.alpha = a;
            cv.fill_circle_radial(x, y, 30.0 * sc, glow, 1.0);
            let size = 28.0 * sc;
            let tile = Rect::new(x - size * 0.5, y - size * 0.5, size, size);
            cv.fill_round(tile, size * 0.32, tint, 0.96);
            cv.hairline_round(tile, size * 0.32, 0xFFFFFF, 0.35);
            cv.glyph(glyph, 13.0 * sc, tile, 0xFFFFFF, 1.0);
            cv.alpha = 1.0;
        });
        if result.is_err() {
            self.finish();
            return true;
        }
        false
    }

    /// Hide the overlay and release its GPU resources.
    fn finish(&mut self) {
        self.run = None;
        self.gfx = None;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::flight_point;

    #[test]
    fn path_runs_start_to_end_and_shrinks() {
        let (s, e) = ((400.0, 700.0), (960.0, 15.0));
        let (x0, y0, sc0, a0) = flight_point(s, e, 0.0);
        assert!((x0 - s.0).abs() < 1e-3 && (y0 - s.1).abs() < 1e-3);
        assert!(a0 == 1.0 && sc0 < 1.0);
        let (x1, y1, sc1, a1) = flight_point(s, e, 1.0);
        assert!((x1 - e.0).abs() < 1e-3 && (y1 - e.1).abs() < 1e-3);
        assert!(sc1 < 0.6 && a1 == 0.0, "swallowed at the end");
        // It rises faster than it moves sideways early on (a "jump", not a straight line).
        let (xm, ym, _, _) = flight_point(s, e, 0.35);
        let rise = (s.1 - ym) / (s.1 - e.1);
        let side = (xm - s.0) / (e.0 - s.0);
        assert!(rise > side, "rise {rise} vs side {side}");
        // Stays within the start/end box (no overshoot off screen).
        for i in 0..=20 {
            let (x, y, _, _) = flight_point(s, e, i as f32 / 20.0);
            assert!((s.0 - 1.0..=e.0 + 1.0).contains(&x) && (e.1 - 1.0..=s.1 + 1.0).contains(&y));
        }
    }
}
