//! Layout and drawing of the notch (immediate mode), modelled after a pure
//! monochrome Black & White aesthetic: pitch-black notch body (#000000) with
//! concave top-edge corner ears, crisp white/gray typography, cursor-only hover
//! feedback (no icon scaling on hover), and real-time audio-reactive sound waves.

use ailan_core::settings::Tab;
use ailan_core::state::Collapsed;
use ailan_core::AppState;
use windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush1;

mod collapsed;
mod tabs;

use crate::gfx::{Canvas, Rect};
use collapsed::draw_collapsed;
use tabs::draw_expanded;

// ---- Monochrome Palette (Pure Black & White + Neutral Grays) --------------------
pub const BG: u32 = 0x000000;
#[allow(dead_code)]
pub const PANEL_BG: u32 = 0x0A0A0C;
#[allow(dead_code)]
pub const SURFACE: u32 = 0x141417;

pub const WHITE: u32 = 0xFFFFFF;
pub const TEXT_SOFT: u32 = 0xA1A1AA;

// ---- glyphs (Segoe MDL2 Assets) -------------------------------------------------
pub const G_PLAY: char = '\u{E768}';
pub const G_PAUSE: char = '\u{E769}';
pub const G_PREV: char = '\u{E892}';
pub const G_NEXT: char = '\u{E893}';
pub const G_VOLUME: char = '\u{E767}';
pub const G_MUTE: char = '\u{E74F}';
pub const G_BRIGHT: char = '\u{E706}';
pub const G_HEADPHONE: char = '\u{E7F6}';
pub const G_MUSIC: char = '\u{E8D6}';
pub const G_TIMER: char = '\u{E916}';
pub const G_MOON: char = '\u{E708}';
pub const G_CHECKMARK: char = '\u{E73E}';
pub const G_DELETE: char = '\u{E711}';
pub const G_ADD: char = '\u{E710}';
pub const G_RESET: char = '\u{E72C}';
pub const G_BELL: char = '\u{EA8F}';
pub const G_BOLT: char = '\u{E945}';
pub const G_CHEVRON_LEFT: char = '\u{E76B}';
pub const G_CHEVRON_RIGHT: char = '\u{E76C}';
pub const G_SETTINGS: char = '\u{E713}';
pub const G_HOME: char = '\u{E80F}';

// ---- geometry (DIP) -------------------------------------------------------------
/// Window width includes room for the expanded notch + its concave ears.
pub const WIN_W: f32 = 480.0;
pub const WIN_H: f32 = 152.0;
/// On-screen size of the expanded island. All tabs share the same width (380 DIP)
/// so switching between Music, Focus, and Todo never jumps horizontally.
pub fn expanded_size(st: &AppState, tab: Tab, now: i64) -> (f32, f32) {
    if st.hover_activities(now).is_empty() {
        return (380.0, 40.0);
    }
    match tab {
        Tab::Todo => (380.0, 114.0),
        Tab::Music | Tab::Focus => (380.0, 127.0),
    }
}

pub(super) fn expanded_layout(st: &AppState, tab: Tab, now: i64) -> (f32, f32) {
    if st.hover_activities(now).is_empty() {
        return (420.0, 44.0);
    }
    match tab {
        Tab::Todo => (420.0, 126.0),
        Tab::Music | Tab::Focus => (420.0, 140.0),
    }
}

const RADIUS_COLLAPSED: f32 = 14.0;
const RADIUS_EXPANDED: f32 = 29.0;

pub const EAR_COLLAPSED: f32 = 10.0;
pub const EAR_EXPANDED: f32 = 20.0;

pub(super) const TODO_ROWS: usize = 2;

pub fn collapsed_size(mode: Collapsed) -> (f32, f32) {
    match mode {
        Collapsed::Minimal | Collapsed::Media | Collapsed::Timer | Collapsed::Todo => (150.0, 30.0),
        Collapsed::Countdown | Collapsed::Stopwatch => (150.0, 30.0),
        // Alerts stay a slim one-line pill instead of a big card.
        Collapsed::Osd => (240.0, 34.0),
        Collapsed::Banner => (250.0, 34.0),
    }
}

/// Top-edge concave ear radius for a given expansion state and collapsed mode.
pub fn ear_for(expand: f32, mode: Collapsed) -> f32 {
    let base_ear = match mode {
        Collapsed::Osd | Collapsed::Banner => 11.0,
        _ => EAR_COLLAPSED,
    };
    base_ear + (EAR_EXPANDED - base_ear) * expand.clamp(0.0, 1.0)
}

/// Bottom corner radius for a given expansion state and collapsed mode.
fn radius_for(expand: f32, mode: Collapsed) -> f32 {
    let base_r = match mode {
        Collapsed::Osd | Collapsed::Banner => 16.0,
        _ => RADIUS_COLLAPSED,
    };
    base_r + (RADIUS_EXPANDED - base_r) * expand.clamp(0.0, 1.0)
}

/// Gaze offset (DIP) of the idle eyes toward the pointer. `center` and `cursor` are in
/// screen pixels, `scale` is the monitor DPI scale. Deflection eases out with distance
/// and saturates at ±3.5 px horizontally / ±2.5 px vertically.
pub fn eye_offset(center: (f32, f32), cursor: (f32, f32), scale: f32) -> (f32, f32) {
    let s = scale.max(0.5);
    let dx = (cursor.0 - center.0) / s;
    let dy = (cursor.1 - center.1) / s;
    let dist = (dx * dx + dy * dy).sqrt();
    if dist < 1.0 {
        return (0.0, 0.0);
    }
    let reach = (dist / 240.0).min(1.0);
    let ease = reach * (2.0 - reach);
    (dx / dist * 3.5 * ease, dy / dist * 2.5 * ease)
}

/// Pill rect (window coordinates, DIP) for a given animated size.
pub fn pill_rect(w: f32, h: f32) -> Rect {
    Rect::new(((WIN_W - w) / 2.0).round(), 0.0, w, h)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorIcon {
    #[default]
    Default,
    Pointer,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Action {
    Expand,
    DismissBanner,
    Tab(Tab),
    SlidePrev,
    SlideNext,
    OpenSettings,
    MediaPrev,
    MediaToggle,
    MediaNext,
    TimerToggle,
    TimerReset,
    TimerSkip,
    TodoInput,
    TodoToggle(u64),
    TodoDelete(u64),
    TodoScroll,
    SnoozeBanner(u32),
    CompleteBanner,
}

impl Action {
    pub fn cursor_icon(&self, input_active: bool) -> CursorIcon {
        match self {
            Action::TodoInput if input_active => CursorIcon::Text,
            Action::TodoScroll => CursorIcon::Default,
            _ => CursorIcon::Pointer,
        }
    }
}

pub struct Hit {
    pub rect: Rect,
    pub action: Action,
}

pub struct View<'a> {
    pub now: i64,
    pub pill: Rect,
    /// 0 = collapsed … 1 = expanded.
    pub expand: f32,
    pub mode: Collapsed,
    pub tab: Tab,
    /// Text being typed into the to-do field (`Some` while focused).
    pub input: Option<&'a str>,
    pub todo_scroll: usize,
    pub art: Option<(&'a ID2D1BitmapBrush1, (u32, u32))>,
    pub hover: Option<Action>,
    /// 0..=1 spring-interpolated hover intensity for `hover`.
    pub hover_t: f32,
    /// Index spring position among visible hover activities for smooth slide feedback.
    pub tab_pos: f32,
    /// Smoothed gaze offset of the idle eyes in DIP (follows the pointer anywhere on screen).
    pub eye: (f32, f32),
    /// 1 → 0 glow right after something flew into the island.
    pub glow: f32,
    /// Real-time 5-band audio heights (0.15..=1.0) driven by system audio output.
    pub audio_bars: [f32; 5],
    /// Smoothed master audio peak (0.0..=1.0) for album art beat pulse.
    pub audio_peak: f32,
}

pub(super) fn hover_amt(v: &View, action: Action) -> f32 {
    if v.hover == Some(action) {
        v.hover_t.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub(super) fn scale_rect(r: Rect, factor: f32) -> Rect {
    let cx = r.x + r.w * 0.5;
    let cy = r.y + r.h * 0.5;
    let nw = (r.w * factor).max(0.0);
    let nh = (r.h * factor).max(0.0);
    Rect::new(cx - nw * 0.5, cy - nh * 0.5, nw, nh)
}

/// Renders the pure solid-black notch body with concave top ears on both sides.
fn draw_notch_shell(cv: &mut Canvas, p: Rect, radius: f32, ear: f32) {
    cv.fill_notch(p, radius, ear, BG, 1.0);
}

pub fn draw(cv: &mut Canvas, st: &AppState, v: &View) -> Vec<Hit> {
    let mut hits = Vec::new();
    let p = v.pill;
    let radius = radius_for(v.expand, v.mode);
    let ear = ear_for(v.expand, v.mode);

    cv.alpha = 1.0;
    draw_notch_shell(cv, p, radius, ear);
    if v.glow > 0.01 {
        let g = crate::gfx::Gradient::new(&[(0.0, WHITE, 0.30), (1.0, WHITE, 0.0)]);
        cv.fill_circle_radial(p.x + p.w * 0.5, p.y + p.h * 0.5, p.w * 0.55, g, v.glow);
    }

    let collapsed_alpha = (1.0 - v.expand * 2.4).clamp(0.0, 1.0);
    let expanded_alpha = ((v.expand - 0.38) / 0.62).clamp(0.0, 1.0);

    if collapsed_alpha > 0.01 {
        cv.alpha = collapsed_alpha;
        draw_collapsed(cv, st, v, &mut hits);
    }
    if expanded_alpha > 0.01 {
        cv.alpha = expanded_alpha;
        let visible = st.hover_activities(v.now);
        let active_tab = if visible.contains(&v.tab) {
            v.tab
        } else {
            visible.first().copied().unwrap_or(v.tab)
        };
        let exp = expanded_size(st, active_tab, v.now);
        let lay = expanded_layout(st, active_tab, v.now);
        // Draw the page layout scaled into the smaller on-screen island, then map
        // its hit rects back to window coordinates.
        let full = pill_rect(exp.0, exp.1);
        let s = exp.0 / lay.0;
        let y_offset = (1.0 - expanded_alpha) * -8.0;
        let layout = Rect::new(0.0, y_offset / s, lay.0, lay.1);
        let mut local = Vec::new();
        cv.set_transform(full.x, full.y, s);
        draw_expanded(cv, st, v, layout, &mut local);
        cv.reset_transform();
        hits.extend(local.into_iter().map(|h| Hit {
            rect: Rect::new(full.x + h.rect.x * s, full.y + h.rect.y * s, h.rect.w * s, h.rect.h * s),
            action: h.action,
        }));
    }
    cv.alpha = 1.0;
    if v.expand < 0.5 {
        hits.push(Hit { rect: p, action: Action::Expand });
    }
    hits
}

/// Monochrome circular white battery status ring.
pub(super) fn draw_status_ring(cv: &mut Canvas, st: &AppState, cx: f32, cy: f32) {
    let pct = match st.battery {
        Some(Some(b)) => (b.percent as f32 / 100.0).clamp(0.05, 1.0),
        _ => 0.75,
    };
    let r = 5.4;
    let stroke = 2.2;
    cv.stroke_circle(cx, cy, r, stroke, WHITE, 0.24);
    cv.stroke_arc(cx, cy, r, -90.0, pct * 360.0, stroke, WHITE, 0.95);
}

/// 5 vertical sound-wave bars (`14px` tall, `2.2px` wide, `2px` gap),
/// animated in real time by `v.audio_bars`.
pub(super) fn draw_wave_bars(cv: &mut Canvas, v: &View, r: Rect, playing: bool) {
    let bw = 2.2;
    let gap = 2.0;
    let total_w = bw * 5.0 + gap * 4.0;
    let start_x = r.x + (r.w - total_w) * 0.5;
    let cy = r.y + r.h * 0.5;
    let col = WHITE;
    let alpha = if playing { 0.96 } else { 0.40 };

    for (i, &bar_val) in v.audio_bars.iter().enumerate() {
        let h_frac = if playing {
            bar_val.clamp(0.20, 1.0)
        } else {
            [0.28, 0.46, 0.34, 0.42, 0.28][i]
        };
        let bh = (r.h * h_frac).max(3.0);
        let bx = start_x + i as f32 * (bw + gap);
        let bar = Rect::new(bx, cy - bh * 0.5, bw, bh);
        cv.fill_round(bar, 1.1, col, alpha);
    }
}

/// Draws the circular left/right slide arrow button (22×22 circle)
/// without hover scaling (cursor change only).
pub(super) fn draw_arrow_btn(cv: &mut Canvas, v: &View, hits: &mut Vec<Hit>, cx: f32, cy: f32, left: bool) {
    let action = if left { Action::SlidePrev } else { Action::SlideNext };
    let h = hover_amt(v, action);
    cv.fill_circle(cx, cy, 11.0, WHITE, 0.16 + 0.14 * h);
    let r = Rect::new(cx - 11.0, cy - 11.0, 22.0, 22.0);
    cv.glyph(
        if left { G_CHEVRON_LEFT } else { G_CHEVRON_RIGHT },
        9.5,
        r,
        WHITE,
        0.85 + 0.15 * h,
    );
    hits.push(Hit { rect: Rect::new(cx - 14.0, cy - 14.0, 28.0, 28.0), action });
}


/// Topmost hit at a point (later entries were drawn on top).
pub fn hit_test(hits: &[Hit], x: f32, y: f32) -> Option<Action> {
    hits.iter()
        .filter(|h| h.rect.contains(x, y))
        .find(|h| !matches!(h.action, Action::Expand | Action::TodoScroll))
        .or_else(|| hits.iter().find(|h| h.rect.contains(x, y)))
        .map(|h| h.action)
}

pub fn max_todo_scroll(st: &AppState, now: i64) -> usize {
    st.data.todos.today_pending(now, st.offset_min).len().saturating_sub(TODO_ROWS)
}

#[cfg(test)]
mod gaze_tests {
    use super::eye_offset;

    #[test]
    fn eyes_look_toward_the_pointer_within_bounds() {
        let c = (1000.0, 15.0);
        let (dx, dy) = eye_offset(c, (2000.0, 15.0), 1.0);
        assert!((dx - 3.5).abs() < 1e-3 && dy.abs() < 1e-3, "far right → full right");
        let (dx, dy) = eye_offset(c, (0.0, 900.0), 1.0);
        assert!(dx < 0.0 && dy > 0.0, "bottom-left → down-left");
        assert!(dx.abs() <= 3.5 && dy.abs() <= 2.5);
        assert_eq!(eye_offset(c, c, 1.0), (0.0, 0.0));
        // Closer pointer → smaller deflection; DPI scale works in DIP.
        let near = eye_offset(c, (1060.0, 15.0), 1.0).0;
        assert!(near > 0.0 && near < 3.5);
        assert!((eye_offset(c, (1120.0, 15.0), 2.0).0 - near).abs() < 1e-3);
    }
}
