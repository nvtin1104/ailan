//! "Obsidian Liquid Glass 2.0" design tokens and drawing primitives for the Personal Hub.
//!
//! Every surface is built from the same three layers so the hub reads as one material:
//! 1. a diagonal gradient body (`BG_SURFACE_*`),
//! 2. an optional cursor spotlight (radial gradient following the pointer),
//! 3. a dual-stop specular rim (linear gradient stroke, bright on top, fading downward).
//!
//! Corner radii follow concentric harmony: `R_inner = max(R_outer - padding, 4)`.

use ailan_core::anim::Spring;
use ailan_core::todo::due_token;

use crate::gfx::{Align, Canvas, Font, Gradient, Rect, RichSpan, TextStyle};
use crate::ui::{G_CHECKMARK, G_DELETE};

// ---- Background layers ---------------------------------------------------------------
pub const BG_DEEP_CANVAS: u32 = 0x090A0F;
pub const BG_DEEP_CANVAS_ALPHA: f32 = 0.85;
pub const BG_SIDEBAR: u32 = 0x0C0E14;
pub const BG_SIDEBAR_ALPHA: f32 = 0.60;
pub const BG_SURFACE_TOP: u32 = 0x141720;
pub const BG_SURFACE_BOTTOM: u32 = 0x0F1219;
pub const BG_SURFACE_ELEVATED: u32 = 0x1B1F2C;
pub const BG_INPUT: u32 = 0x0B0D13;
pub const BG_BADGE: u32 = 0x1E293B;

// ---- Accent & semantic colours -------------------------------------------------------
pub const ACCENT_PRIMARY: u32 = 0x6366F1;
pub const ACCENT_PRIMARY_HI: u32 = 0x818CF8;
pub const ACCENT_MINT: u32 = 0x10B981;
pub const ACCENT_AMBER: u32 = 0xF59E0B;
pub const ACCENT_ROSE: u32 = 0xF43F5E;
pub const ACCENT_SKY: u32 = 0x38BDF8;

// ---- Typography colours --------------------------------------------------------------
pub const TEXT_HERO: u32 = 0xF8FAFC;
pub const TEXT_TITLE: u32 = 0xE2E8F0;
pub const TEXT_BODY: u32 = 0x94A3B8;
pub const TEXT_SECTION: u32 = 0x64748B;
pub const TEXT_DONE: u32 = 0x475569;
pub const WHITE: u32 = 0xFFFFFF;

// ---- Legacy aliases (older pages keep compiling and pick up the new palette) ---------
pub const GLASS_BG: u32 = BG_DEEP_CANVAS;
pub const GLASS_SIDEBAR: u32 = BG_SIDEBAR;
pub const GLASS_CARD: u32 = BG_SURFACE_TOP;
pub const GLASS_CARD_HOVER: u32 = BG_SURFACE_ELEVATED;
pub const GLASS_INPUT: u32 = BG_INPUT;
pub const GLASS_BORDER: u32 = 0x2A2F3D;
pub const GLASS_SPECULAR: u32 = WHITE;
pub const TEXT_PRIMARY: u32 = TEXT_HERO;
pub const TEXT_MUTED: u32 = TEXT_BODY;
pub const TEXT_FAINT: u32 = TEXT_SECTION;

// ---- Geometry tokens -----------------------------------------------------------------
/// Matches the radius DWM clips the window to (`DWMWCP_ROUND` = 8px). Drawing a larger
/// radius leaves uncovered wedges between our curve and the OS curve at every corner.
pub const RADIUS_WINDOW: f32 = 8.0;
pub const RADIUS_CARD: f32 = 14.0;
pub const RADIUS_ITEM: f32 = 10.0;
pub const RADIUS_INPUT: f32 = 10.0;
pub const RADIUS_BUTTON: f32 = 8.0;
pub const RADIUS_PILL: f32 = 14.0;
pub const RADIUS_CHECK: f32 = 6.0;

pub const CORNER_WINDOW: f32 = RADIUS_WINDOW;
pub const CORNER_CARD: f32 = RADIUS_CARD;
pub const CORNER_BUTTON: f32 = RADIUS_BUTTON;
pub const CORNER_CHIP: f32 = RADIUS_PILL;

/// Concentric radius for a shape inset `padding` inside a parent of radius `outer`.
pub fn concentric(outer: f32, padding: f32) -> f32 {
    (outer - padding).max(4.0)
}

// ---- Hub-specific glyphs (Segoe Fluent Icons / MDL2) ---------------------------------
pub const G_SEARCH: char = '\u{E721}';
pub const G_FULLSCREEN: char = '\u{E740}';
pub const G_BACK_TO_WINDOW: char = '\u{E73F}';
pub const G_CHROME_MIN: char = '\u{E921}';
pub const G_CHROME_CLOSE: char = '\u{E8BB}';
pub const G_LIST: char = '\u{E8FD}';
pub const G_CALENDAR: char = '\u{E787}';
pub const G_REPEAT: char = '\u{E8EE}';
pub const G_CHEVRON_DOWN: char = '\u{E70D}';
pub const G_CHEVRON_RIGHT: char = '\u{E76C}';
pub const G_SKIP: char = '\u{E893}';
pub const G_RETURN: char = '\u{E751}';

// ---- Gradients (each creates one cached D2D brush) ----------------------------------
/// Dual-stop specular rim: catches light on the top edge, almost vanishes at the bottom.
pub const RIM: Gradient = Gradient::new(&[(0.0, WHITE, 0.16), (0.3, WHITE, 0.05), (1.0, WHITE, 0.02)]);
pub const RIM_HOT: Gradient = Gradient::new(&[(0.0, WHITE, 0.26), (0.3, WHITE, 0.09), (1.0, WHITE, 0.04)]);
pub const RIM_ACCENT: Gradient =
    Gradient::new(&[(0.0, ACCENT_PRIMARY_HI, 0.55), (0.4, ACCENT_PRIMARY, 0.22), (1.0, ACCENT_PRIMARY, 0.10)]);
pub const SURFACE: Gradient = Gradient::new(&[(0.0, BG_SURFACE_TOP, 1.0), (1.0, BG_SURFACE_BOTTOM, 1.0)]);
pub const SURFACE_ELEVATED: Gradient = Gradient::new(&[(0.0, BG_SURFACE_ELEVATED, 1.0), (1.0, 0x151925, 1.0)]);
pub const SPOTLIGHT: Gradient = Gradient::new(&[(0.0, WHITE, 1.0), (0.55, WHITE, 0.35), (1.0, WHITE, 0.0)]);
pub const DIVIDER_V: Gradient = Gradient::new(&[(0.0, WHITE, 0.0), (0.5, WHITE, 0.08), (1.0, WHITE, 0.0)]);
pub const DIVIDER_H: Gradient = DIVIDER_V;
pub const ACCENT_FILL: Gradient = Gradient::new(&[(0.0, ACCENT_PRIMARY, 1.0), (1.0, ACCENT_PRIMARY_HI, 1.0)]);
pub const ACCENT_TINT: Gradient = Gradient::new(&[(0.0, ACCENT_PRIMARY, 0.26), (1.0, ACCENT_PRIMARY_HI, 0.10)]);
pub const GLOW_ACCENT: Gradient = Gradient::new(&[(0.0, ACCENT_PRIMARY_HI, 0.45), (1.0, ACCENT_PRIMARY, 0.0)]);
pub const GLOW_MINT: Gradient = Gradient::new(&[(0.0, ACCENT_MINT, 0.40), (1.0, ACCENT_MINT, 0.0)]);
/// Active-tab light: transparent at the top, faint accent at the indicator.
pub const TAB_LIFT: Gradient = Gradient::new(&[(0.0, ACCENT_PRIMARY, 0.0), (1.0, ACCENT_PRIMARY, 0.14)]);
pub const GLOW_ROSE: Gradient = Gradient::new(&[(0.0, ACCENT_ROSE, 0.45), (1.0, ACCENT_ROSE, 0.0)]);
pub const GLOW_AMBER: Gradient = Gradient::new(&[(0.0, ACCENT_AMBER, 0.40), (1.0, ACCENT_AMBER, 0.0)]);

// ---- Fonts -----------------------------------------------------------------------------
pub const FONT_HERO: Font = Font::text(22.0, 700, Align::Left);
pub const FONT_SECTION: Font = Font::text(12.0, 600, Align::Left);
pub const FONT_ITEM: Font = Font::text(14.0, 500, Align::Left);
pub const FONT_META: Font = Font::text(11.5, 400, Align::Left);
pub const FONT_META_C: Font = Font::text(11.5, 500, Align::Center);
pub const SECTION_TRACKING: f32 = 1.05; // +0.8pt ≈ 1.07 DIP

/// Smoothly interpolate two 0xRRGGBB colours.
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |s: u32| -> (f32, f32) { (((a >> s) & 0xFF) as f32, ((b >> s) & 0xFF) as f32) };
    let mut out = 0u32;
    for s in [16u32, 8, 0] {
        let (x, y) = ch(s);
        out |= ((x + (y - x) * t).round() as u32 & 0xFF) << s;
    }
    out
}

// ---- Surfaces --------------------------------------------------------------------------

/// Which body gradient a specular surface uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Card,
    Elevated,
    Accent,
    Input,
}

/// Core material: diagonal gradient body + optional cursor spotlight + dual-stop specular rim.
pub fn draw_specular_rounded_rect(
    cv: &mut Canvas,
    r: Rect,
    radius: f32,
    surface: Surface,
    hover: f32,
    mouse: Option<(f32, f32)>,
) {
    let tl = (r.x, r.y);
    let br = (r.right(), r.bottom());
    match surface {
        Surface::Card => {
            cv.fill_round_linear(r, radius, SURFACE, tl, br, 0.92);
            if hover > 0.01 {
                cv.fill_round_linear(r, radius, SURFACE_ELEVATED, tl, br, 0.92 * hover);
            }
        }
        Surface::Elevated => cv.fill_round_linear(r, radius, SURFACE_ELEVATED, tl, br, 0.96),
        Surface::Accent => cv.fill_round_linear(r, radius, ACCENT_TINT, tl, br, 1.0),
        Surface::Input => cv.fill_round(r, radius, BG_INPUT, 0.88),
    }
    if let Some(m) = mouse {
        draw_radial_spotlight(cv, r, radius, m, 0.04 + 0.03 * hover);
    }
    let rim = match surface {
        Surface::Accent => RIM_ACCENT,
        _ if hover > 0.5 => RIM_HOT,
        _ => RIM,
    };
    cv.hairline_round_linear(r, radius, rim, (r.x, r.y), (r.x, r.bottom()), 1.0);
}

/// Card surface with hover tint and pointer spotlight.
pub fn draw_specular_card(cv: &mut Canvas, r: Rect, radius: f32, hover: f32, mouse: Option<(f32, f32)>) {
    draw_specular_rounded_rect(cv, r, radius, Surface::Card, hover, mouse);
}

/// Pointer-following soft light (radius 180 DIP), clipped to the rounded surface.
/// Drawn only while the pointer is over `r`, so it never costs an idle frame.
pub fn draw_radial_spotlight(cv: &mut Canvas, r: Rect, radius: f32, mouse: (f32, f32), alpha: f32) {
    if r.contains(mouse.0, mouse.1) {
        cv.fill_round_radial(r, radius, SPOTLIGHT, mouse, 180.0, alpha);
    }
}

/// Full-window acrylic backdrop.
pub fn draw_window_backdrop(cv: &mut Canvas, r: Rect) {
    // Fill the full rect: DWM rounds (and anti-aliases) the window corners itself.
    cv.fill_rect(r, BG_DEEP_CANVAS, BG_DEEP_CANVAS_ALPHA);
    cv.hairline_round_linear(r, RADIUS_WINDOW, RIM, (r.x, r.y), (r.x, r.y + r.h.min(260.0)), 1.0);
}

/// Legacy: window panel (now the deep canvas material).
pub fn draw_glass_panel(cv: &mut Canvas, r: Rect, radius: f32, bg_rgb: u32, bg_alpha: f32) {
    cv.fill_round(r, radius, bg_rgb, bg_alpha);
    cv.hairline_round_linear(r, radius, RIM, (r.x, r.y), (r.x, r.bottom()), 1.0);
}

/// Legacy: card without spotlight.
pub fn draw_glass_card(cv: &mut Canvas, r: Rect, radius: f32, hover: f32) {
    draw_specular_card(cv, r, radius, hover, None);
}

/// Vertical 1px divider that fades out at both ends.
pub fn draw_divider_v(cv: &mut Canvas, x: f32, y: f32, h: f32) {
    cv.fill_rect_linear(Rect::new(x, y, 1.0, h), DIVIDER_V, (x, y), (x, y + h), 1.0);
}

/// Horizontal 1px divider that fades out at both ends.
pub fn draw_divider_h(cv: &mut Canvas, x: f32, y: f32, w: f32) {
    cv.fill_rect_linear(Rect::new(x, y, w, 1.0), DIVIDER_H, (x, y), (x + w, y), 1.0);
}

// ---- Rings -----------------------------------------------------------------------------

/// Circular progress ring (12 o'clock start, clockwise) with optional bloom.
pub fn draw_progress_ring(
    cv: &mut Canvas,
    cx: f32,
    cy: f32,
    radius: f32,
    width: f32,
    progress: f32,
    rgb: u32,
    glow: bool,
) {
    let p = progress.clamp(0.0, 1.0);
    cv.stroke_circle(cx, cy, radius, width, WHITE, 0.07);
    if p <= 0.002 {
        return;
    }
    let sweep = 360.0 * p;
    if glow {
        // Fake bloom: wide translucent passes under the core stroke.
        cv.stroke_arc(cx, cy, radius, -90.0, sweep, width * 3.2, rgb, 0.07);
        cv.stroke_arc(cx, cy, radius, -90.0, sweep, width * 2.0, rgb, 0.12);
    }
    cv.stroke_arc(cx, cy, radius, -90.0, sweep, width, rgb, 0.98);
    if glow && p < 0.999 {
        let a = (-90.0 + sweep).to_radians();
        let (hx, hy) = (cx + radius * a.cos(), cy + radius * a.sin());
        let g = if rgb == ACCENT_MINT { GLOW_MINT } else if rgb == ACCENT_AMBER { GLOW_AMBER } else { GLOW_ACCENT };
        cv.fill_circle_radial(hx, hy, width * 3.0, g, 1.0);
        cv.fill_circle(hx, hy, width * 0.42, WHITE, 0.95);
    }
}

// ---- Typography helpers ----------------------------------------------------------------

/// Uppercase, wide-tracked section label (`12px / 600 / +0.8pt / #64748B`).
pub fn draw_section_header(cv: &mut Canvas, r: Rect, text: &str) {
    let up = text.to_uppercase();
    cv.text_styled(&up, FONT_SECTION, r, TEXT_SECTION, 1.0, TextStyle::tracked(SECTION_TRACKING));
}

/// Legacy name.
pub fn draw_section_hdr(cv: &mut Canvas, r: Rect, text: &str) {
    draw_section_header(cv, r, text);
}

/// Tabular-figure pill badge whose left edge sits at `x`. Returns its width.
pub fn draw_badge(cv: &mut Canvas, x: f32, y: f32, h: f32, text: &str, fg: u32, bg: u32, bg_a: f32) -> f32 {
    let style = TextStyle::TABULAR;
    let w = cv.text_width(text, FONT_META_C, style) + 16.0;
    let r = Rect::new(x, y, w, h);
    cv.fill_round(r, h * 0.5, bg, bg_a);
    cv.text_styled(text, FONT_META_C, r, fg, 1.0, style);
    w
}

/// Same as [`draw_badge`] but right-aligned to `right`. Returns its width.
pub fn draw_badge_right(cv: &mut Canvas, right: f32, y: f32, h: f32, text: &str, fg: u32, bg: u32, bg_a: f32) -> f32 {
    let w = cv.text_width(text, FONT_META_C, TextStyle::TABULAR) + 16.0;
    draw_badge(cv, right - w, y, h, text, fg, bg, bg_a)
}

/// Small numeric counter pill for navigation items, right-aligned to `right`.
pub fn draw_count_badge(cv: &mut Canvas, right: f32, cy: f32, n: usize, rgb: u32) {
    if n == 0 {
        return;
    }
    let s = if n > 99 { "99+".to_string() } else { n.to_string() };
    let font = Font::text(10.5, 650, Align::Center);
    let w = (cv.text_width(&s, font, TextStyle::TABULAR) + 12.0).max(20.0);
    let r = Rect::new(right - w, cy - 9.0, w, 18.0);
    cv.fill_round(r, 9.0, rgb, 0.18);
    cv.hairline_round(r, 9.0, rgb, 0.35);
    cv.text_styled(&s, font, r, mix(rgb, WHITE, 0.35), 1.0, TextStyle::TABULAR);
}

// ---- Controls --------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Ghost,
    Danger,
}

/// Labelled button (`RADIUS_BUTTON`).
pub fn draw_button(cv: &mut Canvas, r: Rect, label: &str, kind: ButtonKind, hover: f32) {
    let font = Font::text(12.0, 600, Align::Center);
    match kind {
        ButtonKind::Primary => {
            cv.fill_round_linear(r, RADIUS_BUTTON, ACCENT_FILL, (r.x, r.y), (r.right(), r.bottom()), 0.92 + 0.08 * hover);
            cv.hairline_round_linear(r, RADIUS_BUTTON, RIM_HOT, (r.x, r.y), (r.x, r.bottom()), 1.0);
            cv.text(label, font, r, WHITE, 1.0);
        }
        ButtonKind::Ghost | ButtonKind::Danger => {
            cv.fill_round(r, RADIUS_BUTTON, WHITE, 0.05 + 0.07 * hover);
            cv.hairline_round_linear(r, RADIUS_BUTTON, RIM, (r.x, r.y), (r.x, r.bottom()), 1.0);
            let fg = if kind == ButtonKind::Danger && hover > 0.5 { ACCENT_ROSE } else { TEXT_TITLE };
            cv.text(label, font, r, fg, 0.95);
        }
    }
}

/// Square icon button; `active` tints it with the accent.
pub fn draw_icon_button(cv: &mut Canvas, r: Rect, glyph: char, size: f32, hover: f32, active: bool, tint: u32) {
    if active {
        cv.fill_round(r, RADIUS_BUTTON, tint, 0.18 + 0.08 * hover);
        cv.hairline_round(r, RADIUS_BUTTON, tint, 0.40);
        cv.glyph(glyph, size, r, mix(tint, WHITE, 0.35), 1.0);
    } else {
        if hover > 0.01 {
            cv.fill_round(r, RADIUS_BUTTON, WHITE, 0.08 * hover);
        }
        cv.glyph(glyph, size, r, if hover > 0.5 { TEXT_HERO } else { TEXT_BODY }, 0.95);
    }
}

/// Round accent play/pause style button.
pub fn draw_round_button(cv: &mut Canvas, cx: f32, cy: f32, radius: f32, glyph: char, hover: f32, primary: bool) {
    let r = Rect::new(cx - radius, cy - radius, radius * 2.0, radius * 2.0);
    if primary {
        cv.fill_circle_radial(cx, cy, radius * 1.9, GLOW_ACCENT, 0.35 + 0.35 * hover);
        cv.fill_round_linear(r, radius, ACCENT_FILL, (r.x, r.y), (r.right(), r.bottom()), 1.0);
        cv.hairline_round_linear(r, radius, RIM_HOT, (r.x, r.y), (r.x, r.bottom()), 1.0);
        cv.glyph(glyph, radius * 0.72, r, WHITE, 1.0);
    } else {
        cv.fill_round(r, radius, WHITE, 0.06 + 0.08 * hover);
        cv.hairline_round_linear(r, radius, RIM, (r.x, r.y), (r.x, r.bottom()), 1.0);
        cv.glyph(glyph, radius * 0.62, r, if hover > 0.5 { TEXT_HERO } else { TEXT_TITLE }, 0.95);
    }
}

/// Window chrome: minimize / compact↔full / close. `hover` = index 0..3 of the hovered one.
pub fn draw_window_controls(cv: &mut Canvas, rects: [Rect; 3], hover: Option<usize>, is_compact: bool) {
    let glyphs = [G_CHROME_MIN, if is_compact { G_FULLSCREEN } else { G_BACK_TO_WINDOW }, G_CHROME_CLOSE];
    for (i, r) in rects.iter().enumerate() {
        let h = hover == Some(i);
        if h {
            let (rgb, a) = if i == 2 { (0xE81123, 0.92) } else { (WHITE, 0.09) };
            cv.fill_round(*r, RADIUS_BUTTON, rgb, a);
        }
        let fg = if h { TEXT_HERO } else { TEXT_BODY };
        cv.glyph(glyphs[i], 10.0, *r, fg, if h { 1.0 } else { 0.85 });
    }
}

/// Legacy titlebar signature retained for older call sites.
pub fn draw_titlebar_controls(cv: &mut Canvas, r: Rect, hover_close: bool, hover_min: bool, hover_mode: bool, is_compact: bool) {
    let rects = window_control_rects(r.right(), r.y);
    let hover = if hover_min {
        Some(0)
    } else if hover_mode {
        Some(1)
    } else if hover_close {
        Some(2)
    } else {
        None
    };
    draw_window_controls(cv, rects, hover, is_compact);
}

/// `[minimize, mode, close]` rects for a titlebar ending at `right`.
pub fn window_control_rects(right: f32, top: f32) -> [Rect; 3] {
    let (w, h, y) = (38.0, 28.0, top + 8.0);
    [
        Rect::new(right - 8.0 - w * 3.0 - 8.0, y, w, h),
        Rect::new(right - 8.0 - w * 2.0 - 4.0, y, w, h),
        Rect::new(right - 8.0 - w, y, w, h),
    ]
}

/// Tactile checkbox (`RADIUS_CHECK`). `scale` comes from a bouncy spring
/// (0.8 → 1.12 → 1.0) and `tick` (0..1) draws the check stroke progressively.
pub fn draw_checkbox(cv: &mut Canvas, r: Rect, checked: bool, ring: u32, hover: f32, scale: f32, tick: f32) {
    let s = scale.clamp(0.5, 1.4);
    let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
    let box_r = Rect::new(cx - r.w * 0.5 * s, cy - r.h * 0.5 * s, r.w * s, r.h * s);
    let rad = RADIUS_CHECK * s;
    if checked {
        cv.fill_circle_radial(cx, cy, r.w * 1.1, GLOW_MINT, 0.6 * (1.0 - tick * 0.5));
        cv.fill_round(box_r, rad, ACCENT_MINT, 0.95);
        let pts = [
            (cx - 4.2 * s, cy + 0.2 * s),
            (cx - 1.2 * s, cy + 3.2 * s),
            (cx + 4.4 * s, cy - 3.4 * s),
        ];
        cv.stroke_polyline(&pts, tick, 1.9 * s, WHITE, 1.0);
    } else {
        if hover > 0.01 {
            cv.fill_round(box_r, rad, ring, 0.12 * hover);
        }
        cv.stroke_round(box_r.inset(0.75, 0.75), rad - 0.75, 1.5, ring, 0.75 + 0.25 * hover);
    }
}

/// Underline tab strip used for every tab bar in the Hub.
///
/// * `items` — `(label, optional icon)`.
/// * `pos` — spring-animated fractional index of the active tab.
/// * `equal` — equal-width columns (Compact) instead of content-sized tabs.
///
/// States: idle = muted label; hover = brighter label/icon + faint preview underline;
/// active = bright bold label + accent underline that slides (position and width) to it.
/// Returns each tab's rect for hit testing.
pub fn draw_tab_strip(cv: &mut Canvas, r: Rect, items: &[(&str, Option<char>)], pos: f32, hover: Option<usize>, equal: bool) -> Vec<Rect> {
    let n = items.len().max(1);
    let font_on = Font::text(13.0, 650, Align::Left);
    let font_off = Font::text(13.0, 500, Align::Left);
    let icon_w = 22.0;

    // Tab rects and the underline span inside each.
    let mut tabs = Vec::with_capacity(n);
    let mut spans: Vec<(f32, f32)> = Vec::with_capacity(n);
    if equal {
        let w = r.w / n as f32;
        for (i, (label, _)) in items.iter().enumerate() {
            let tr = Rect::new(r.x + i as f32 * w, r.y, w, r.h);
            let tw = cv.text_width(label, font_on, TextStyle::default()).min(w - 8.0);
            tabs.push(tr);
            spans.push((tr.x + (w - tw) * 0.5, tw));
        }
    } else {
        let mut x = r.x;
        for (label, icon) in items {
            let iw = if icon.is_some() { icon_w } else { 0.0 };
            let tw = cv.text_width(label, font_on, TextStyle::default()) + iw;
            let w = tw + 28.0;
            tabs.push(Rect::new(x, r.y, w, r.h));
            spans.push((x + 14.0, tw));
            x += w + 4.0;
        }
    }

    draw_divider_h(cv, r.x, r.bottom() - 1.0, r.w);
    let pos = pos.clamp(0.0, (n - 1) as f32);
    let active = pos.round() as usize;
    for (i, (tr, (label, icon))) in tabs.iter().zip(items.iter()).enumerate() {
        let near = 1.0 - (pos - i as f32).abs().min(1.0);
        let hovered = hover == Some(i) && i != active;
        let base = if hovered { TEXT_TITLE } else { TEXT_BODY };
        let fg = mix(base, TEXT_HERO, near);
        let font = if near > 0.5 { font_on } else { font_off };
        let (sx, sw) = spans[i];
        let text_r = Rect::new(tr.x, tr.y, tr.w, tr.h - 3.0);
        if equal {
            cv.text(label, Font::text(13.0, font.weight, Align::Center), text_r, fg, 1.0);
        } else {
            let mut tx = sx;
            if let Some(g) = icon {
                let icol = mix(if hovered { TEXT_TITLE } else { TEXT_BODY }, ACCENT_PRIMARY_HI, near * 0.75);
                cv.glyph(*g, 11.5, Rect::new(tx, text_r.y, 16.0, text_r.h), icol, 1.0);
                tx += icon_w;
            }
            cv.text(label, font, Rect::new(tx, text_r.y, sx + sw - tx + 4.0, text_r.h), fg, 1.0);
        }
        if hovered {
            // Preview of where the indicator would go (full tab width, square).
            cv.fill_rect(Rect::new(tr.x, r.bottom() - 2.0, tr.w, 2.0), WHITE, 0.18);
        }
    }

    // Accent indicator interpolated between neighbouring tabs.
    let (i0, i1) = (pos.floor() as usize, (pos.ceil() as usize).min(n - 1));
    let f = pos - i0 as f32;
    let ux = tabs[i0].x + (tabs[i1].x - tabs[i0].x) * f;
    let uw = tabs[i0].w + (tabs[i1].w - tabs[i0].w) * f;
    // Active area spans the whole tab, square-cornered: soft light rising from the
    // indicator, and the 2px indicator sitting exactly on the divider.
    let lift = Rect::new(ux, r.y, uw, r.h);
    cv.fill_rect_linear(lift, TAB_LIFT, (lift.x, lift.y), (lift.x, lift.bottom()), 1.0);
    let ul = Rect::new(ux, r.bottom() - 2.0, uw, 2.0);
    cv.fill_rect_linear(ul, ACCENT_FILL, (ul.x, ul.y), (ul.right(), ul.y), 1.0);
    tabs
}

/// Syntax-highlight runs for an `@day time` token in task input (violet day, amber time).
pub fn due_spans(s: &str) -> Vec<RichSpan> {
    let Some(tok) = due_token(s) else { return Vec::new() };
    let mut v = vec![RichSpan { bytes: tok.span.start..tok.span.start + 1, rgb: ACCENT_PRIMARY_HI, bg: None }];
    if let Some((sp, _)) = tok.day {
        v.push(RichSpan { bytes: sp, rgb: ACCENT_PRIMARY_HI, bg: Some((ACCENT_PRIMARY, 0.20)) });
    }
    if let Some((sp, _)) = tok.time {
        v.push(RichSpan { bytes: sp, rgb: ACCENT_AMBER, bg: Some((ACCENT_AMBER, 0.16)) });
    }
    v
}

/// Text input with live `@time` highlighting. Returns the text area rect.
pub fn draw_text_input(
    cv: &mut Canvas,
    r: Rect,
    value: &str,
    hint: &str,
    focused: bool,
    hover: f32,
    font: Font,
    lead_glyph: Option<char>,
    reserve_right: f32,
) -> Rect {
    let rad = RADIUS_INPUT.min(r.h * 0.5);
    cv.fill_round(r, rad, BG_INPUT, 0.90);
    if focused {
        // Accent focus ring with a soft outer halo.
        cv.stroke_round(r.inset(-1.5, -1.5), rad + 1.5, 3.0, ACCENT_PRIMARY, 0.16);
        cv.hairline_round_linear(r, rad, RIM_ACCENT, (r.x, r.y), (r.x, r.bottom()), 1.0);
    } else {
        cv.hairline_round_linear(r, rad, if hover > 0.5 { RIM_HOT } else { RIM }, (r.x, r.y), (r.x, r.bottom()), 1.0);
    }
    let mut tx = r.x + 14.0;
    if let Some(g) = lead_glyph {
        cv.glyph(g, 12.0, Rect::new(r.x + 10.0, r.y, 20.0, r.h), if focused { ACCENT_PRIMARY_HI } else { TEXT_SECTION }, 1.0);
        tx = r.x + 36.0;
    }
    let txt = Rect::new(tx, r.y, (r.right() - tx - 12.0 - reserve_right).max(0.0), r.h);
    if value.is_empty() {
        cv.text(hint, font, txt, if focused { TEXT_SECTION } else { TEXT_BODY }, if focused { 0.9 } else { 0.75 });
        if focused {
            let ch = font.size2 as f32 / 2.0 * 1.3;
            cv.fill_round(Rect::new(txt.x, r.y + (r.h - ch) * 0.5, 1.5, ch), 0.75, ACCENT_PRIMARY_HI, 1.0);
        }
    } else {
        let spans = due_spans(value);
        cv.text_rich(value, font, txt, if focused { TEXT_HERO } else { TEXT_TITLE }, 1.0, &spans, focused.then_some(ACCENT_PRIMARY_HI));
    }
    txt
}

/// Pill toggle switch.
pub fn draw_switch(cv: &mut Canvas, r: Rect, checked: bool, hover: f32) {
    let (sw, sh) = (36.0, 20.0);
    let track = Rect::new(r.x + (r.w - sw) * 0.5, r.y + (r.h - sh) * 0.5, sw, sh);
    let rad = sh * 0.5;
    if checked {
        cv.fill_round_linear(track, rad, ACCENT_FILL, (track.x, track.y), (track.right(), track.bottom()), 1.0);
        cv.hairline_round_linear(track, rad, RIM_HOT, (track.x, track.y), (track.x, track.bottom()), 1.0);
        cv.fill_circle(track.right() - rad, track.y + rad, 7.0, WHITE, 1.0);
    } else {
        cv.fill_round(track, rad, BG_INPUT, 0.9);
        cv.hairline_round(track, rad, WHITE, 0.16 + 0.12 * hover);
        cv.fill_circle(track.x + rad, track.y + rad, 6.2, TEXT_BODY, 0.9);
    }
}

/// Settings row: label (+ optional subtitle) and a switch.
pub fn draw_switch_row(cv: &mut Canvas, r: Rect, label: &str, sub: Option<&str>, checked: bool, hover: f32) {
    draw_glass_card(cv, r, RADIUS_ITEM, hover);
    let ty = if sub.is_some() { r.y + 11.0 } else { r.y + (r.h - 18.0) * 0.5 };
    cv.text(
        label,
        Font::text(13.0, if checked { 600 } else { 500 }, Align::Left),
        Rect::new(r.x + 14.0, ty, r.w - 74.0, 18.0),
        TEXT_TITLE,
        0.98,
    );
    if let Some(s) = sub {
        cv.text(s, FONT_META, Rect::new(r.x + 14.0, ty + 18.0, r.w - 74.0, 16.0), TEXT_SECTION, 1.0);
    }
    draw_switch(cv, Rect::new(r.right() - 48.0, r.y, 40.0, r.h), checked, hover);
}

/// Chip button (filters, presets).
pub fn draw_chip(cv: &mut Canvas, r: Rect, label: &str, active: bool, hover: f32) {
    let rad = RADIUS_PILL.min(r.h * 0.5);
    if active {
        cv.fill_round_linear(r, rad, ACCENT_TINT, (r.x, r.y), (r.right(), r.bottom()), 1.0);
        cv.hairline_round_linear(r, rad, RIM_ACCENT, (r.x, r.y), (r.x, r.bottom()), 1.0);
        cv.text(label, Font::text(11.5, 650, Align::Center), r, TEXT_HERO, 1.0);
    } else {
        cv.fill_round(r, rad, WHITE, 0.04 + 0.05 * hover);
        cv.hairline_round_linear(r, rad, RIM, (r.x, r.y), (r.x, r.bottom()), 1.0);
        cv.text(label, Font::text(11.5, 500, Align::Center), r, if hover > 0.05 { TEXT_HERO } else { TEXT_BODY }, 0.95);
    }
}

/// Celebratory empty state: layered halo, orbit ring, drawn check and sparkles (pure vector).
pub fn draw_empty_state(cv: &mut Canvas, r: Rect, title: &str, sub: &str) {
    let cx = r.x + r.w * 0.5;
    let cy = r.y + (r.h * 0.5 - 30.0).max(48.0);
    cv.fill_circle_radial(cx, cy, 78.0, GLOW_ACCENT, 0.55);
    cv.stroke_circle(cx, cy, 44.0, 1.0, WHITE, 0.08);
    cv.stroke_arc(cx, cy, 44.0, -150.0, 120.0, 1.5, ACCENT_PRIMARY_HI, 0.6);
    cv.fill_circle(cx + 44.0 * (-30f32).to_radians().cos(), cy + 44.0 * (-30f32).to_radians().sin(), 2.5, ACCENT_PRIMARY_HI, 0.9);
    let disc = Rect::new(cx - 26.0, cy - 26.0, 52.0, 52.0);
    cv.fill_round_linear(disc, 26.0, SURFACE_ELEVATED, (disc.x, disc.y), (disc.right(), disc.bottom()), 1.0);
    cv.hairline_round_linear(disc, 26.0, RIM_HOT, (disc.x, disc.y), (disc.x, disc.bottom()), 1.0);
    cv.stroke_polyline(&[(cx - 10.0, cy + 1.0), (cx - 3.0, cy + 8.0), (cx + 11.0, cy - 7.0)], 1.0, 3.0, ACCENT_MINT, 1.0);
    for (dx, dy, s) in [(-58.0f32, -22.0f32, 5.0f32), (54.0, -34.0, 4.0), (48.0, 30.0, 3.0)] {
        let (sx, sy) = (cx + dx, cy + dy);
        cv.fill_round(Rect::new(sx - s, sy - 0.75, s * 2.0, 1.5), 0.75, TEXT_HERO, 0.55);
        cv.fill_round(Rect::new(sx - 0.75, sy - s, 1.5, s * 2.0), 0.75, TEXT_HERO, 0.55);
    }
    cv.text(title, Font::text(15.0, 650, Align::Center), Rect::new(r.x, cy + 54.0, r.w, 22.0), TEXT_HERO, 1.0);
    cv.text(sub, Font::text(12.0, 400, Align::Center), Rect::new(r.x, cy + 76.0, r.w, 18.0), TEXT_BODY, 0.9);
}

/// Small trash/delete affordance shown on hover.
pub fn draw_delete_button(cv: &mut Canvas, r: Rect, hover: f32) {
    if hover > 0.01 {
        cv.fill_round(r, RADIUS_BUTTON, ACCENT_ROSE, 0.16 * hover);
    }
    cv.glyph(G_DELETE, 10.0, r, if hover > 0.5 { ACCENT_ROSE } else { TEXT_BODY }, 0.95);
}

/// Done-check glyph inside a mint chip (static, for non-interactive lists).
pub fn draw_done_dot(cv: &mut Canvas, cx: f32, cy: f32) {
    cv.fill_circle(cx, cy, 7.0, ACCENT_MINT, 0.9);
    cv.glyph(G_CHECKMARK, 8.0, Rect::new(cx - 8.0, cy - 8.0, 16.0, 16.0), WHITE, 1.0);
}

// ---- Springs -----------------------------------------------------------------------------

/// Sliding active-indicator spring. Works in "index × 100" units so the settle threshold
/// of [`Spring`] (0.05) is sub-pixel.
#[derive(Debug, Clone, Copy)]
pub struct Indicator {
    spring: Spring,
}

impl Indicator {
    pub fn new(index: usize) -> Self {
        Self { spring: Spring::new(index as f32 * 100.0).with(380.0, 34.0) }
    }

    pub fn go(&mut self, index: usize) {
        self.spring.set_target(index as f32 * 100.0);
    }

    /// Index the indicator is heading to.
    pub fn target(&self) -> f32 {
        self.spring.target / 100.0
    }

    /// Fractional index for drawing.
    pub fn pos(&self) -> f32 {
        self.spring.value / 100.0
    }

    pub fn snap(&mut self) {
        self.spring.snap();
    }

    pub fn is_settled(&self) -> bool {
        self.spring.is_settled()
    }

    pub fn step(&mut self, dt: f32) -> bool {
        self.spring.step(dt)
    }
}

/// Per-todo check animation: bouncy scale (0.8 → ~1.12 → 1.0) and a progressive tick stroke.
#[derive(Debug, Clone, Copy)]
pub struct CheckPop {
    pub id: u64,
    scale: Spring,
    tick: Spring,
}

impl CheckPop {
    pub fn new(id: u64) -> Self {
        // ζ ≈ 0.16 → ~60% overshoot of the 0.8→1.0 travel ⇒ peak ≈ 1.12.
        let mut scale = Spring::new(80.0).with(600.0, 8.0);
        scale.set_target(100.0);
        let mut tick = Spring::new(0.0).with(260.0, 32.0);
        tick.set_target(100.0);
        Self { id, scale, tick }
    }

    pub fn scale(&self) -> f32 {
        self.scale.value / 100.0
    }

    pub fn tick(&self) -> f32 {
        (self.tick.value / 100.0).clamp(0.0, 1.0)
    }

    /// Returns `true` when both springs have settled.
    pub fn step(&mut self, dt: f32) -> bool {
        let a = self.scale.step(dt);
        let b = self.tick.step(dt);
        a && b
    }
}

/// Render the AiLan app icon (dark squircle + white Dynamic Island pill + `✦` sparkle)
/// matching `platform::icon_art` for all in-app logo/brand placements.
pub fn draw_app_logo(cv: &mut Canvas, r: Rect) {
    let rad = r.w * 0.24;
    cv.fill_round(r, rad, 0x0E0E0E, 1.0);
    cv.hairline_round(r, rad, WHITE, 0.18);

    // Upper Dynamic Island pill
    let pw = r.w * 0.54;
    let ph = r.h * 0.22;
    let pill = Rect::new(r.x + (r.w - pw) * 0.5, r.y + r.h * 0.38 - ph * 0.5, pw, ph);
    cv.fill_round(pill, ph * 0.5, WHITE, 1.0);

    // Lower-right 4-pointed AI sparkle
    let sp_sz = r.w * 0.40;
    let sp = Rect::new(r.x + r.w * 0.68 - sp_sz * 0.5, r.y + r.h * 0.70 - sp_sz * 0.5, sp_sz, sp_sz);
    cv.text("✦", Font::text((r.h * 0.34).max(9.0), 400, Align::Center), sp, WHITE, 1.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concentric_radius_harmony() {
        assert_eq!(concentric(RADIUS_CARD, 4.0), RADIUS_ITEM);
        assert_eq!(concentric(RADIUS_ITEM, 2.0), RADIUS_BUTTON);
        assert_eq!(concentric(RADIUS_BUTTON, 10.0), 4.0, "never below 4");
    }

    #[test]
    fn check_pop_overshoots_then_settles() {
        let mut p = CheckPop::new(1);
        let mut peak = 0.0f32;
        let mut frames = 0;
        while !p.step(1.0 / 60.0) {
            peak = peak.max(p.scale());
            frames += 1;
            assert!(frames < 600, "check pop never settled");
        }
        assert!((1.08..1.16).contains(&peak), "peak scale {peak}");
        assert_eq!(p.scale(), 1.0);
        assert_eq!(p.tick(), 1.0);
    }

    #[test]
    fn indicator_slides_to_target() {
        let mut i = Indicator::new(0);
        i.go(3);
        let mut frames = 0;
        while !i.step(1.0 / 60.0) {
            frames += 1;
            assert!(frames < 240);
        }
        assert_eq!(i.pos(), 3.0);
    }

    #[test]
    fn colour_mix_endpoints() {
        assert_eq!(mix(0x000000, 0xFFFFFF, 0.0), 0x000000);
        assert_eq!(mix(0x000000, 0xFFFFFF, 1.0), 0xFFFFFF);
        assert_eq!(mix(0x102030, 0x102030, 0.5), 0x102030);
    }

    #[test]
    fn due_spans_follow_parser() {
        let s = "Gọi mẹ @mai 9h";
        let spans = due_spans(s);
        assert_eq!(spans.len(), 3);
        assert_eq!(&s[spans[1].bytes.clone()], "mai");
        assert_eq!(&s[spans[2].bytes.clone()], "9h");
        assert!(due_spans("no token here").is_empty());
    }
}
