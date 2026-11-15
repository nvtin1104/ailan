//! Layout and drawing of the island (immediate mode). Quiet charcoal capsule:
//! lifted black fill, one hairline, white type. Hover only brightens the ring.
//! Click opens. Width grows for a long banner; the window itself stays at the cap.

use ailan_core::settings::Tab;
use ailan_core::state::Collapsed;
use ailan_core::AppState;
use windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush1;

mod collapsed;
mod tabs;

use crate::gfx::{Canvas, Rect};
use collapsed::draw_collapsed;
use tabs::draw_expanded;

// ---- Grok-like charcoal (approximate; one gray, no accent fill) -----------------
pub const BG: u32 = 0x141414;
pub const SURFACE: u32 = 0x212121;
pub const WHITE: u32 = 0xFCFCFC;
pub const TEXT_SOFT: u32 = 0x9E9E9E;
pub const HAIRLINE: u32 = 0x2A2A2A;

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
/// Cap window. The pill is centred inside it; the HWND does not follow the spring.
pub const WIN_W: f32 = 560.0;
pub const WIN_H: f32 = 160.0;
pub const REST_W: f32 = 150.0;
pub const REST_H: f32 = 32.0;
pub const VERT_W: f32 = 38.0;
pub const VERT_H: f32 = 112.0;
pub const ALERT_H: f32 = 34.0;
pub const ALERT_MIN_W: f32 = 150.0;
pub const ALERT_MAX_W: f32 = 420.0;
pub const EXPANDED_W: f32 = 400.0;
pub const EXPANDED_H: f32 = 144.0;
pub const EXPANDED_MIN_W: f32 = 320.0;
pub const EXPANDED_MAX_W: f32 = 560.0;
/// Concave Dynamic Island ear / corner V size (DIP).
pub const EAR: f32 = 14.0;
/// Corner snap needs the pill near the perpendicular edge (DIP).
pub const SNAP_CORNER_DIP: f32 = 40.0;

pub(super) const TODO_ROWS: usize = 2;

/// How the island sits against the work area perimeter. Drives corner radii, ears, and orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dock {
    /// Flush to the top edge with concave V ears.
    #[default]
    Top,
    /// Flush to the bottom edge with concave V ears.
    Bottom,
    /// Flush to the left edge (vertical capsule when collapsed).
    Left,
    /// Flush to the right edge (vertical capsule when collapsed).
    Right,
    /// Flush to the top-left corner.
    TopLeft,
    /// Flush to the top-right corner.
    TopRight,
    /// Flush to the bottom-left corner.
    BottomLeft,
    /// Flush to the bottom-right corner.
    BottomRight,
}

impl Dock {
    pub fn is_vertical(self) -> bool {
        matches!(self, Dock::Left | Dock::Right)
    }

    /// Snap a desired center point `(cx, cy)` onto the nearest perimeter edge/corner of `work`.
    /// Returns `(dock, left, top)` for a pill of size `(pw, ph)`.
    pub fn snap_perimeter(
        cx: i32,
        cy: i32,
        pw: i32,
        ph: i32,
        work: (i32, i32, i32, i32),
        corner: i32,
    ) -> (Self, i32, i32) {
        let (wl, wt, wr, wb) = work;
        let max_l = (wr - pw).max(wl);
        let max_t = (wb - ph).max(wt);

        let d_top = (cy - wt).abs();
        let d_bot = (wb - cy).abs();
        let d_left = (cx - wl).abs();
        let d_right = (wr - cx).abs();
        let min_d = d_top.min(d_bot).min(d_left).min(d_right);

        if min_d == d_top {
            let left = (cx - pw / 2).clamp(wl, max_l);
            if left - wl <= corner {
                (Dock::TopLeft, wl, wt)
            } else if max_l - left <= corner {
                (Dock::TopRight, max_l, wt)
            } else {
                (Dock::Top, left, wt)
            }
        } else if min_d == d_bot {
            let left = (cx - pw / 2).clamp(wl, max_l);
            if left - wl <= corner {
                (Dock::BottomLeft, wl, max_t)
            } else if max_l - left <= corner {
                (Dock::BottomRight, max_l, max_t)
            } else {
                (Dock::Bottom, left, max_t)
            }
        } else if min_d == d_left {
            let top = (cy - ph / 2).clamp(wt, max_t);
            if top - wt <= corner {
                (Dock::TopLeft, wl, wt)
            } else if max_t - top <= corner {
                (Dock::BottomLeft, wl, max_t)
            } else {
                (Dock::Left, wl, top)
            }
        } else {
            let top = (cy - ph / 2).clamp(wt, max_t);
            if top - wt <= corner {
                (Dock::TopRight, max_l, wt)
            } else if max_t - top <= corner {
                (Dock::BottomRight, max_l, max_t)
            } else {
                (Dock::Right, max_l, top)
            }
        }
    }
}

/// One expanded size for every tab, including an empty island.
pub fn expanded_size(_st: &AppState, _tab: Tab, _now: i64) -> (f32, f32) {
    debug_assert!((EXPANDED_MIN_W..=EXPANDED_MAX_W).contains(&EXPANDED_W));
    (EXPANDED_W, EXPANDED_H)
}

/// Resting activities stay 150×32 horizontally, or 38×112 vertically on Left/Right edges.
/// Banners and the OSD always use a horizontal measured width.
pub fn collapsed_size(mode: Collapsed, alert_w: f32, dock: Dock) -> (f32, f32) {
    match mode {
        Collapsed::Osd | Collapsed::Banner => (alert_w.clamp(ALERT_MIN_W, ALERT_MAX_W), ALERT_H),
        _ if dock.is_vertical() => (VERT_W, VERT_H),
        _ => (REST_W, REST_H),
    }
}

/// `text_dip` is the measured line. `extra` reserves room for buttons or the OSD track.
pub fn alert_width(text_dip: f32, glyph: bool, extra: f32) -> f32 {
    let pad = 28.0 + if glyph { 16.0 } else { 0.0 } + extra;
    (text_dip + pad).clamp(ALERT_MIN_W, ALERT_MAX_W)
}

/// Capsule while collapsed, 16 DIP once expanded.
pub fn pill_radius(expand: f32, min_dim: f32) -> f32 {
    let collapsed = min_dim * 0.5;
    collapsed + (16.0 - collapsed) * expand.clamp(0.0, 1.0)
}

/// Per-corner radii `[TL, TR, BR, BL]` for the docked silhouette bones.
/// Flush bezel corners are 0.0; free corners use `pill_radius`.
pub fn pill_radii(dock: Dock, expand: f32, w: f32, h: f32) -> [f32; 4] {
    let r = pill_radius(expand, w.min(h));
    match dock {
        Dock::Top => [0.0, 0.0, r, r],
        Dock::Bottom => [r, r, 0.0, 0.0],
        Dock::Left => [0.0, r, r, 0.0],
        Dock::Right => [r, 0.0, 0.0, r],
        Dock::TopLeft => [0.0, 0.0, r, 0.0],
        Dock::TopRight => [0.0, 0.0, 0.0, r],
        Dock::BottomLeft => [0.0, r, 0.0, 0.0],
        Dock::BottomRight => [r, 0.0, 0.0, 0.0],
    }
}

/// Ear size for edge/corner docks.
pub fn ear_for(_dock: Dock) -> f32 {
    EAR
}

/// Pill rect (window coordinates, DIP) for a given animated size and dock.
/// Anchored against the docked edge(s) so expansion grows inward into the screen.
pub fn pill_rect(w: f32, h: f32, dock: Dock) -> Rect {
    let x = match dock {
        Dock::Left | Dock::TopLeft | Dock::BottomLeft => 0.0,
        Dock::Right | Dock::TopRight | Dock::BottomRight => (WIN_W - w).round(),
        Dock::Top | Dock::Bottom => ((WIN_W - w) / 2.0).round(),
    };
    let y = match dock {
        Dock::Top | Dock::TopLeft | Dock::TopRight => 0.0,
        Dock::Bottom | Dock::BottomLeft | Dock::BottomRight => (WIN_H - h).round(),
        Dock::Left | Dock::Right => ((WIN_H - h) / 2.0).round(),
    };
    Rect::new(x, y, w, h)
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
    pub dock: Dock,
    /// 0 = collapsed … 1 = expanded.
    pub expand: f32,
    pub mode: Collapsed,
    pub tab: Tab,
    /// Text being typed into the to-do field (`Some` while focused).
    pub input: Option<&'a str>,
    pub todo_scroll: usize,
    pub art: Option<(&'a ID2D1BitmapBrush1, (u32, u32))>,
    #[allow(dead_code)]
    pub hover: Option<Action>,
    /// 0..=1 spring-interpolated hover intensity for `hover`.
    #[allow(dead_code)]
    pub hover_t: f32,
    /// Index spring position among visible activities for slide feedback.
    pub tab_pos: f32,
    /// Pointer is over the pill. Brightens the hairline only.
    pub hot: bool,
    /// 1 → 0 glow right after something flew into the island.
    pub glow: f32,
}

pub(super) fn hover_amt(_v: &View, _action: Action) -> f32 {
    0.0
}

fn draw_shell(cv: &mut Canvas, p: Rect, dock: Dock, expand: f32, _hot: bool) {
    let radii = pill_radii(dock, expand, p.w, p.h);
    let ear = ear_for(dock);
    let ring = HAIRLINE;
    cv.fill_island(p, dock, radii, ear, BG, 1.0);
    let (ix, iy) = match dock {
        Dock::Top | Dock::Bottom => (0.5, 0.0),
        Dock::Left | Dock::Right => (0.0, 0.5),
        _ => (0.0, 0.0),
    };
    cv.stroke_island(
        p.inset(ix, iy),
        dock,
        radii.map(|r| (r - 0.5).max(0.0)),
        ear,
        1.0,
        ring,
        1.0,
    );
}

pub fn draw(cv: &mut Canvas, st: &AppState, v: &View) -> Vec<Hit> {
    let mut hits = Vec::new();
    let p = v.pill;

    cv.alpha = 1.0;
    draw_shell(cv, p, v.dock, v.expand, v.hot);
    if v.glow > 0.01 {
        let g = crate::gfx::Gradient::new(&[(0.0, WHITE, 0.18), (1.0, WHITE, 0.0)]);
        cv.fill_circle_radial(p.x + p.w * 0.5, p.y + p.h * 0.5, p.w * 0.45, g, v.glow * 0.6);
    }

    let collapsed_alpha = (1.0 - v.expand * 2.4).clamp(0.0, 1.0);
    let expanded_alpha = ((v.expand - 0.38) / 0.62).clamp(0.0, 1.0);

    if collapsed_alpha > 0.01 {
        cv.alpha = collapsed_alpha;
        draw_collapsed(cv, st, v, &mut hits);
    }
    if expanded_alpha > 0.01 {
        cv.alpha = expanded_alpha;
        let y_offset = (1.0 - expanded_alpha) * -6.0;
        let layout = Rect::new(p.x, p.y + y_offset, p.w, p.h);
        draw_expanded(cv, st, v, layout, &mut hits);
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

/// Circular previous/next control. The hit rect matches the drawn circle.
pub(super) fn draw_arrow_btn(cv: &mut Canvas, v: &View, hits: &mut Vec<Hit>, cx: f32, cy: f32, left: bool) {
    let action = if left { Action::SlidePrev } else { Action::SlideNext };
    let h = hover_amt(v, action);
    cv.fill_circle(cx, cy, 11.0, WHITE, 0.08 + 0.10 * h);
    let r = Rect::new(cx - 11.0, cy - 11.0, 22.0, 22.0);
    cv.glyph(
        if left { G_CHEVRON_LEFT } else { G_CHEVRON_RIGHT },
        9.5,
        r,
        WHITE,
        0.85 + 0.15 * h,
    );
    hits.push(Hit { rect: r, action });
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
mod geometry_tests {
    use super::{alert_width, collapsed_size, pill_radii, pill_rect, Dock, REST_H, REST_W, VERT_H, VERT_W, WIN_H, WIN_W};
    use ailan_core::state::Collapsed;

    #[test]
    fn alert_width_clamps_between_standard_and_cap() {
        assert_eq!(alert_width(10.0, false, 0.0), 150.0);
        assert_eq!(alert_width(200.0, true, 0.0), 244.0);
        assert_eq!(alert_width(900.0, true, 40.0), 420.0);
    }

    #[test]
    fn dock_pill_anchors_to_edges_and_corners() {
        let top = pill_rect(REST_W, REST_H, Dock::Top);
        let bot = pill_rect(REST_W, REST_H, Dock::Bottom);
        let left = pill_rect(VERT_W, VERT_H, Dock::Left);
        let right = pill_rect(VERT_W, VERT_H, Dock::Right);
        assert_eq!(top.y, 0.0);
        assert_eq!(bot.y, WIN_H - REST_H);
        assert_eq!(left.x, 0.0);
        assert_eq!(right.x, WIN_W - VERT_W);
    }

    #[test]
    fn vertical_docks_use_vertical_collapsed_size() {
        assert_eq!(collapsed_size(Collapsed::Minimal, REST_W, Dock::Top), (REST_W, REST_H));
        assert_eq!(collapsed_size(Collapsed::Minimal, REST_W, Dock::Left), (VERT_W, VERT_H));
        assert_eq!(collapsed_size(Collapsed::Minimal, REST_W, Dock::Right), (VERT_W, VERT_H));
        // Alerts stay horizontal so text remains readable.
        assert_eq!(collapsed_size(Collapsed::Osd, 220.0, Dock::Left), (220.0, 34.0));
    }

    #[test]
    fn dock_radii_bones() {
        assert_eq!(pill_radii(Dock::Top, 0.0, REST_W, REST_H), [0.0, 0.0, 16.0, 16.0]);
        assert_eq!(pill_radii(Dock::Bottom, 0.0, REST_W, REST_H), [16.0, 16.0, 0.0, 0.0]);
        assert_eq!(pill_radii(Dock::Left, 0.0, VERT_W, VERT_H), [0.0, 19.0, 19.0, 0.0]);
        assert_eq!(pill_radii(Dock::Right, 0.0, VERT_W, VERT_H), [19.0, 0.0, 0.0, 19.0]);
        assert_eq!(pill_radii(Dock::TopLeft, 0.0, REST_W, REST_H), [0.0, 0.0, 16.0, 0.0]);
        assert_eq!(pill_radii(Dock::TopRight, 0.0, REST_W, REST_H), [0.0, 0.0, 0.0, 16.0]);
        assert_eq!(pill_radii(Dock::BottomLeft, 0.0, REST_W, REST_H), [0.0, 16.0, 0.0, 0.0]);
        assert_eq!(pill_radii(Dock::BottomRight, 0.0, REST_W, REST_H), [16.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn snap_perimeter_picks_edges_and_corners() {
        let work = (0, 0, 1920, 1080);
        assert_eq!(Dock::snap_perimeter(20, 10, 150, 32, work, 40).0, Dock::TopLeft);
        assert_eq!(Dock::snap_perimeter(1900, 10, 150, 32, work, 40).0, Dock::TopRight);
        assert_eq!(Dock::snap_perimeter(960, 100, 150, 32, work, 40).0, Dock::Top);
        assert_eq!(Dock::snap_perimeter(960, 1000, 150, 32, work, 40).0, Dock::Bottom);
        assert_eq!(Dock::snap_perimeter(100, 540, 38, 112, work, 40).0, Dock::Left);
        assert_eq!(Dock::snap_perimeter(1850, 540, 38, 112, work, 40).0, Dock::Right);
        assert_eq!(Dock::snap_perimeter(10, 1060, 150, 32, work, 40).0, Dock::BottomLeft);
        assert_eq!(Dock::snap_perimeter(1910, 1060, 150, 32, work, 40).0, Dock::BottomRight);
    }
}
