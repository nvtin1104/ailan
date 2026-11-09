//! Layout and drawing of the notch (immediate mode). Every draw returns the
//! clickable regions, so input handling never disagrees with what is on screen.

use ailan_core::i18n::{short_date, t, Key};
use ailan_core::settings::Tab;
use ailan_core::state::{BannerIcon, Collapsed, OsdKind};
use ailan_core::time::{civil, fmt_duration, MINUTE};
use ailan_core::timer::Phase;
use ailan_core::AppState;
use windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush1;

use crate::gfx::{Align, Canvas, Font, Rect};

// ---- palette (Adaptive Aurora & Liquid Glass on Pure Black Notch) ---------------
pub const BG: u32 = 0x000000;

const TEXT: u32 = 0xF8FAFC;
const TEXT_SOFT: u32 = 0xCBD5E1;
const SUB: u32 = 0x94A3B8;

const MINT: u32 = 0x34D399;
const MINT_HI: u32 = 0x6EE7B7;
const CYAN: u32 = 0x38BDF8;
const CORAL: u32 = 0xFB7185;
const ROSE: u32 = 0xF472B6;
const AMBER: u32 = 0xFBBF24;
const GOLD: u32 = 0xF59E0B;
const INDIGO: u32 = 0x818CF8;
const VIOLET: u32 = 0xC084FC;

// ---- glyphs (Segoe MDL2 Assets) ---------------------------------------------------
const G_PLAY: char = '\u{E768}';
const G_PAUSE: char = '\u{E769}';
const G_PREV: char = '\u{E892}';
const G_NEXT: char = '\u{E893}';
const G_VOLUME: char = '\u{E767}';
const G_MUTE: char = '\u{E74F}';
const G_BRIGHT: char = '\u{E706}';
const G_HEADPHONE: char = '\u{E7F6}';
const G_MUSIC: char = '\u{E8D6}';
const G_TIMER: char = '\u{E916}';
const G_MOON: char = '\u{E708}';
const G_BOX: char = '\u{E739}';
const G_BOX_CHECKED: char = '\u{E73A}';
const G_DELETE: char = '\u{E711}';
const G_ADD: char = '\u{E710}';
const G_RESET: char = '\u{E72C}';
const G_BELL: char = '\u{EA8F}';
const G_BOLT: char = '\u{E945}';
const G_ETHERNET: char = '\u{E839}';
const G_OFFLINE: char = '\u{F384}';
const G_WIFI: [char; 4] = ['\u{E872}', '\u{E873}', '\u{E874}', '\u{E701}'];

// ---- geometry (DIP) ---------------------------------------------------------------
pub const WIN_W: f32 = 460.0;
pub const WIN_H: f32 = 196.0;
pub const EXPANDED: (f32, f32) = (440.0, 190.0);
const RADIUS_COLLAPSED: f32 = 14.0;
const RADIUS_EXPANDED: f32 = 26.0;
const TODO_ROWS: usize = 3;

pub fn collapsed_size(mode: Collapsed) -> (f32, f32) {
    match mode {
        Collapsed::Minimal => (124.0, 20.0),
        Collapsed::Clock => (132.0, 30.0),
        Collapsed::Media => (210.0, 30.0),
        Collapsed::Timer => (200.0, 30.0),
        Collapsed::Osd => (280.0, 34.0),
        Collapsed::Banner => (360.0, 48.0),
    }
}

/// Pill rect (window coordinates, DIP) for a given animated size.
pub fn pill_rect(w: f32, h: f32) -> Rect {
    Rect::new(((WIN_W - w) / 2.0).round(), 0.0, w, h)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Expand,
    DismissBanner,
    Tab(Tab),
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
    /// 0.0 = Music, 1.0 = Focus, 2.0 = Todo (spring-animated).
    pub tab_pos: f32,
    /// Mouse position in DIP while inside the notch.
    pub cursor: Option<(f32, f32)>,
}

fn hover_amt(v: &View, action: Action) -> f32 {
    if v.hover == Some(action) {
        v.hover_t.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Context-aware aurora pair (primary, secondary) for ambient glass illumination.
fn context_aura(st: &AppState, v: &View) -> (u32, u32) {
    if let Some(b) = &st.banner {
        return match b.icon {
            BannerIcon::Charging | BannerIcon::Focus => (MINT, CYAN),
            BannerIcon::BatteryLow | BannerIcon::Reminder => (AMBER, GOLD),
            BannerIcon::Break => (CORAL, ROSE),
            BannerIcon::Headphones => (INDIGO, CYAN),
        };
    }
    if v.expand > 0.45 {
        match v.tab {
            Tab::Music => {
                if st.media.as_ref().is_some_and(|m| m.playing) {
                    (MINT, CYAN)
                } else {
                    (INDIGO, VIOLET)
                }
            }
            Tab::Focus => match st.pomodoro.phase {
                Phase::Focus => (MINT, CYAN),
                Phase::ShortBreak | Phase::LongBreak => (CORAL, ROSE),
            },
            Tab::Todo => (INDIGO, CYAN),
        }
    } else {
        match v.mode {
            Collapsed::Media => (MINT, CYAN),
            Collapsed::Timer => match st.pomodoro.phase {
                Phase::Focus => (MINT, CYAN),
                Phase::ShortBreak | Phase::LongBreak => (CORAL, ROSE),
            },
            Collapsed::Osd => (CYAN, INDIGO),
            Collapsed::Banner => (MINT, CYAN),
            Collapsed::Clock | Collapsed::Minimal => (INDIGO, VIOLET),
        }
    }
}

/// Renders the pure-black borderless notch body with subtle internal Liquid Glass
/// ambient light when expanded or hovered.
fn draw_glass_shell(cv: &mut Canvas, st: &AppState, v: &View, p: Rect, radius: f32) {
    let (aura1, aura2) = context_aura(st, v);
    let shell = Rect::new(p.x, -radius, p.w, p.h + radius);

    // 1. Pure black hardware-notch body (100% opaque, no outer border or drop shadow).
    cv.fill_round(shell, radius, BG, 1.0);

    // 2. Subtle internal ambient Aurora bloom only when expanded (keeps collapsed notch pitch black).
    if v.expand > 0.05 {
        let e = v.expand;
        cv.fill_round_radial(
            shell,
            radius,
            (p.x + p.w * 0.20, p.y + p.h * 0.25),
            p.w * 0.60,
            (p.h * 1.0).max(36.0),
            &[
                (0.0, aura1, 0.08 * e),
                (0.55, aura1, 0.03 * e),
                (1.0, aura1, 0.0),
            ],
        );
        cv.fill_round_radial(
            shell,
            radius,
            (p.x + p.w * 0.82, p.y + p.h * 0.85),
            p.w * 0.55,
            (p.h * 0.9).max(32.0),
            &[
                (0.0, aura2, 0.07 * e),
                (0.60, aura2, 0.02 * e),
                (1.0, aura2, 0.0),
            ],
        );
    }

    // 3. Soft internal cursor spotlight on the black surface when hovering.
    if let Some((mx, my)) = v.cursor {
        let spot_r = if v.expand > 0.5 { 130.0 } else { 65.0 };
        let spot_a = if v.expand > 0.5 { 0.065 } else { 0.045 };
        cv.fill_round_radial(
            shell,
            radius,
            (mx, my),
            spot_r,
            spot_r,
            &[
                (0.0, 0xFFFFFF, spot_a),
                (0.45, aura1, spot_a * 0.45),
                (1.0, 0xFFFFFF, 0.0),
            ],
        );
    }
}

/// Frosted inner glass card / button surface with specular border & cursor reactivity.
fn draw_glass_pill(
    cv: &mut Canvas,
    v: &View,
    r: Rect,
    radius: f32,
    hover: f32,
    accent: u32,
    recessed: bool,
) {
    let top_a = if recessed { 0.045 } else { 0.075 } + 0.075 * hover;
    let bot_a = if recessed { 0.025 } else { 0.035 } + 0.045 * hover;
    cv.fill_round_linear(
        r,
        radius,
        (r.x, r.y),
        (r.x, r.bottom()),
        &[
            (0.0, 0xFFFFFF, top_a),
            (1.0, 0xCBD5E1, bot_a),
        ],
    );
    if hover > 0.01 {
        cv.fill_round_linear(
            r,
            radius,
            (r.x, r.y),
            (r.right(), r.bottom()),
            &[
                (0.0, accent, 0.14 * hover),
                (1.0, accent, 0.04 * hover),
            ],
        );
    }
    if let Some((mx, my)) = v.cursor {
        if r.inset(-24.0, -24.0).contains(mx, my) {
            cv.fill_round_radial(
                r,
                radius,
                (mx, my),
                56.0,
                56.0,
                &[
                    (0.0, 0xFFFFFF, 0.08 + 0.05 * hover),
                    (0.55, accent, 0.03 * hover),
                    (1.0, 0xFFFFFF, 0.0),
                ],
            );
        }
    }
    let border = r.inset(0.5, 0.5);
    cv.stroke_round_linear(
        border,
        (radius - 0.5).max(1.0),
        1.0,
        (r.x, r.y),
        (r.right(), r.bottom()),
        &[
            (0.0, 0xFFFFFF, 0.16 + 0.24 * hover),
            (0.5, accent, 0.06 + 0.22 * hover),
            (1.0, 0xFFFFFF, 0.07 + 0.14 * hover),
        ],
    );
    if let Some((mx, my)) = v.cursor {
        if r.inset(-16.0, -16.0).contains(mx, my) {
            cv.stroke_round_radial(
                border,
                (radius - 0.5).max(1.0),
                1.15,
                (mx, my),
                44.0,
                &[
                    (0.0, 0xFFFFFF, 0.36 + 0.20 * hover),
                    (0.5, accent, 0.16 * hover),
                    (1.0, 0xFFFFFF, 0.0),
                ],
            );
        }
    }
}

pub fn draw(cv: &mut Canvas, st: &AppState, v: &View) -> Vec<Hit> {
    let mut hits = Vec::new();
    let p = v.pill;
    let radius = RADIUS_COLLAPSED + (RADIUS_EXPANDED - RADIUS_COLLAPSED) * v.expand;

    cv.alpha = 1.0;
    draw_glass_shell(cv, st, v, p, radius);

    let collapsed_alpha = (1.0 - v.expand * 2.5).clamp(0.0, 1.0);
    let expanded_alpha = ((v.expand - 0.45) / 0.55).clamp(0.0, 1.0);

    if collapsed_alpha > 0.01 {
        cv.alpha = collapsed_alpha;
        draw_collapsed(cv, st, v, &mut hits);
    }
    if expanded_alpha > 0.01 {
        cv.alpha = expanded_alpha;
        // Content is laid out for the full expanded size, anchored at the pill's top-left.
        let full = pill_rect(EXPANDED.0, EXPANDED.1);
        draw_expanded(cv, st, v, full, &mut hits);
    }
    cv.alpha = 1.0;
    if v.expand < 0.5 {
        // Whole collapsed pill is one target; specific targets above take priority.
        hits.push(Hit { rect: p, action: Action::Expand });
    }
    hits
}

// ---- collapsed ------------------------------------------------------------------

fn draw_collapsed(cv: &mut Canvas, st: &AppState, v: &View, hits: &mut Vec<Hit>) {
    let p = v.pill;
    let lang = st.lang();
    match v.mode {
        Collapsed::Minimal => {}
        Collapsed::Clock => {
            let c = civil(v.now, st.offset_min);
            cv.text(&format!("{:02}:{:02}", c.hour, c.minute), Font::text(13.0, 600, Align::Center), p, TEXT, 0.96);
        }
        Collapsed::Media => {
            let art = Rect::new(p.x + 10.0, p.y + 5.0, 20.0, 20.0);
            match v.art {
                Some((brush, size)) => {
                    cv.image(brush, size, art, 5.5);
                    cv.stroke_round(art, 5.5, 0.8, 0xFFFFFF, 0.24);
                }
                None => {
                    draw_glass_pill(cv, v, art, 5.5, 0.3, MINT, false);
                    cv.glyph(G_MUSIC, 10.0, art, MINT_HI, 1.0);
                }
            }
            if let Some(m) = &st.media {
                let r = Rect::new(art.right() + 8.0, p.y, p.w - 76.0, p.h);
                cv.text(&m.title, Font::text(12.0, 500, Align::Left), r, TEXT, 0.94);
            }
            equalizer(cv, Rect::new(p.right() - 30.0, p.y + 9.0, 18.0, 12.0));
        }
        Collapsed::Timer => {
            let (label, col1, col2) = phase_style(st.pomodoro.phase, lang);
            let cy = p.y + p.h / 2.0;
            cv.fill_circle(p.x + 18.0, cy, 6.5, col1, 0.22);
            cv.fill_circle(p.x + 18.0, cy, 3.8, col1, 1.0);
            cv.text(label, Font::text(12.0, 500, Align::Left), Rect::new(p.x + 30.0, p.y, 100.0, p.h), TEXT_SOFT, 0.92);
            let mins = (st.pomodoro.remaining(v.now) + MINUTE - 1) / MINUTE;
            let s = format!("{mins}{}", t(lang, Key::MinuteShort));
            let _ = col2;
            cv.text(&s, Font::text(13.0, 600, Align::Right), Rect::new(p.right() - 70.0, p.y, 56.0, p.h), col1, 1.0);
        }
        Collapsed::Osd => {
            let Some(o) = &st.osd else { return };
            let (g, col1, col2) = match o.kind {
                OsdKind::Volume if o.muted || o.level <= 0.0 => (G_MUTE, CORAL, ROSE),
                OsdKind::Volume => (G_VOLUME, MINT, CYAN),
                OsdKind::Brightness => (G_BRIGHT, AMBER, CYAN),
            };
            cv.glyph(g, 14.0, Rect::new(p.x + 10.0, p.y, 26.0, p.h), TEXT, 0.95);
            let bar = Rect::new(p.x + 42.0, p.y + p.h / 2.0 - 3.0, p.w - 100.0, 6.0);
            let level = if o.muted { 0.0 } else { o.level.clamp(0.0, 1.0) };
            cv.fill_round(bar, 3.0, 0xFFFFFF, 0.12);
            if level > 0.005 {
                let fill_w = (bar.w * level).max(6.0);
                let fill = Rect::new(bar.x, bar.y, fill_w, bar.h);
                cv.fill_round_linear(
                    fill,
                    3.0,
                    (fill.x, fill.y),
                    (fill.right(), fill.y),
                    &[(0.0, col1, 1.0), (1.0, col2, 1.0)],
                );
            }
            let pct = format!("{}", (level * 100.0).round() as i32);
            cv.text(&pct, Font::text(12.0, 600, Align::Right), Rect::new(p.right() - 50.0, p.y, 38.0, p.h), TEXT_SOFT, 0.95);
        }
        Collapsed::Banner => {
            let Some(b) = &st.banner else { return };
            let (g, col) = match b.icon {
                BannerIcon::Charging => (G_BOLT, MINT_HI),
                BannerIcon::BatteryLow => (G_BOLT, AMBER),
                BannerIcon::Headphones => (G_HEADPHONE, CYAN),
                BannerIcon::Reminder => (G_BELL, AMBER),
                BannerIcon::Focus => (G_TIMER, MINT_HI),
                BannerIcon::Break => (G_MOON, CORAL),
            };
            let ic = Rect::new(p.x + 12.0, p.y + 8.0, 32.0, 32.0);
            draw_glass_pill(cv, v, ic, 10.0, 0.35, col, false);
            cv.glyph(g, 15.0, ic, col, 1.0);
            let tx = ic.right() + 10.0;
            let tw = p.right() - tx - 14.0;
            cv.text(&b.title, Font::text(12.5, 600, Align::Left), Rect::new(tx, p.y + 6.0, tw, 18.0), TEXT, 1.0);
            cv.text(&b.body, Font::text(12.0, 400, Align::Left), Rect::new(tx, p.y + 24.0, tw, 18.0), TEXT_SOFT, 0.9);
            hits.push(Hit { rect: p, action: Action::DismissBanner });
        }
    }
}

/// Static "now playing" bars with smooth Mint-to-Cyan liquid gradient.
fn equalizer(cv: &mut Canvas, r: Rect) {
    let heights = [0.55, 1.0, 0.7, 0.85];
    let bw = 3.0;
    let gap = (r.w - bw * heights.len() as f32) / (heights.len() as f32 - 1.0);
    for (i, h) in heights.iter().enumerate() {
        let bh = r.h * h;
        let bar = Rect::new(r.x + i as f32 * (bw + gap), r.bottom() - bh, bw, bh);
        cv.fill_round_linear(
            bar,
            1.5,
            (bar.x, bar.y),
            (bar.x, bar.bottom()),
            &[(0.0, CYAN, 1.0), (1.0, MINT, 0.95)],
        );
    }
}

fn phase_style(phase: Phase, lang: ailan_core::i18n::Lang) -> (&'static str, u32, u32) {
    match phase {
        Phase::Focus => (t(lang, Key::Focus), MINT_HI, CYAN),
        Phase::ShortBreak => (t(lang, Key::ShortBreak), CORAL, ROSE),
        Phase::LongBreak => (t(lang, Key::LongBreak), CORAL, ROSE),
    }
}

// ---- expanded -------------------------------------------------------------------

fn draw_expanded(cv: &mut Canvas, st: &AppState, v: &View, p: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let c = civil(v.now, st.offset_min);
    let (aura1, _) = context_aura(st, v);

    // Header: time + date (left), status (right).
    let time = format!("{:02}:{:02}", c.hour, c.minute);
    cv.text(&time, Font::text(15.0, 650, Align::Left), Rect::new(p.x + 22.0, 8.0, 60.0, 26.0), TEXT, 1.0);
    cv.text(&short_date(lang, &c), Font::text(12.0, 400, Align::Left), Rect::new(p.x + 74.0, 8.0, 120.0, 26.0), TEXT_SOFT, 0.85);
    draw_status(cv, st, Rect::new(p.right() - 220.0, 8.0, 200.0, 26.0));

    // Tabs with recessed glass track + spring-sliding Liquid Glass indicator.
    let tabs = [(Tab::Music, Key::TabMusic), (Tab::Focus, Key::TabFocus), (Tab::Todo, Key::TabTodo)];
    let mut tab_rects = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    let mut x = p.x + 18.0;
    for (i, (_, key)) in tabs.iter().enumerate() {
        let label = t(lang, *key);
        let w = 28.0 + label.chars().count() as f32 * 7.2;
        tab_rects[i] = Rect::new(x, 38.0, w, 26.0);
        x += w + 4.0;
    }
    let track = Rect::new(p.x + 15.0, 35.5, (x - p.x - 15.0) + 1.0, 31.0);
    draw_glass_pill(cv, v, track, 15.5, 0.0, aura1, true);

    // Interpolate sliding active tab capsule from `v.tab_pos` (0.0..=2.0).
    let pos = v.tab_pos.clamp(0.0, 2.0);
    let i0 = (pos.floor() as usize).min(2);
    let i1 = (i0 + 1).min(2);
    let frac = pos - i0 as f32;
    let r0 = tab_rects[i0];
    let r1 = tab_rects[i1];
    let active_pill = Rect::new(
        r0.x + (r1.x - r0.x) * frac,
        38.0,
        r0.w + (r1.w - r0.w) * frac,
        26.0,
    );
    draw_glass_pill(cv, v, active_pill, 13.0, 0.85, aura1, false);

    for (i, (tab, key)) in tabs.into_iter().enumerate() {
        let r = tab_rects[i];
        let active = v.tab == tab;
        let h = hover_amt(v, Action::Tab(tab));
        if !active && h > 0.01 {
            let hr = r.scale_from_center(1.0 + 0.03 * h);
            draw_glass_pill(cv, v, hr, 13.0, 0.45 * h, aura1, false);
        }
        let label = t(lang, key);
        let col = if active || h > 0.4 { TEXT } else { TEXT_SOFT };
        let alpha = if active { 1.0 } else { 0.78 + 0.22 * h };
        cv.text(label, Font::text(12.0, if active { 600 } else { 500 }, Align::Center), r, col, alpha);
        hits.push(Hit { rect: r, action: Action::Tab(tab) });
    }

    let content = Rect::new(p.x + 16.0, 72.0, p.w - 32.0, p.h - 72.0 - 12.0);
    match v.tab {
        Tab::Music => draw_music(cv, st, v, content, hits),
        Tab::Focus => draw_focus(cv, st, v, content, hits),
        Tab::Todo => draw_todo(cv, st, v, content, hits),
    }
}

fn draw_status(cv: &mut Canvas, st: &AppState, r: Rect) {
    // Laid out right-to-left.
    let mut x = r.right();
    let cy = r.y + r.h / 2.0;
    if let Some(Some(b)) = st.battery {
        let pct = format!("{}%", b.percent);
        cv.text(&pct, Font::text(12.0, 500, Align::Right), Rect::new(x - 36.0, r.y, 36.0, r.h), TEXT_SOFT, 0.92);
        x -= 40.0;
        let (col1, col2) = if b.charging {
            (MINT_HI, CYAN)
        } else if b.percent <= 20 {
            (AMBER, CORAL)
        } else {
            (TEXT, TEXT_SOFT)
        };
        let body = Rect::new(x - 24.0, cy - 5.5, 22.0, 11.0);
        cv.stroke_round(body, 3.2, 1.15, TEXT_SOFT, 0.65);
        cv.fill_round(Rect::new(body.right() + 0.6, cy - 2.5, 2.0, 5.0), 1.0, TEXT_SOFT, 0.65);
        let inner = body.inset(2.1, 2.1);
        let fill_w = (inner.w * b.percent as f32 / 100.0).max(1.5);
        let fill = Rect::new(inner.x, inner.y, fill_w, inner.h);
        cv.fill_round_linear(fill, 1.5, (fill.x, fill.y), (fill.right(), fill.y), &[(0.0, col1, 1.0), (1.0, col2, 1.0)]);
        if b.charging {
            cv.glyph(G_BOLT, 9.0, body, BG, 1.0);
        }
        x -= 32.0;
    }
    let net = &st.network;
    let (g, col) = match &net.wifi {
        Some((_, bars)) => (G_WIFI[(*bars as usize).saturating_sub(1).min(3)], TEXT),
        None if net.connected => (G_ETHERNET, TEXT),
        None => (G_OFFLINE, SUB),
    };
    cv.glyph(g, 13.0, Rect::new(x - 22.0, r.y, 22.0, r.h), col, 0.92);
    x -= 26.0;
    if st.audio_device.as_ref().is_some_and(|(_, hp)| *hp) {
        cv.glyph(G_HEADPHONE, 13.0, Rect::new(x - 22.0, r.y, 22.0, r.h), CYAN, 0.95);
    }
}

fn media_button(
    cv: &mut Canvas,
    hits: &mut Vec<Hit>,
    v: &View,
    r: Rect,
    g: char,
    size: f32,
    action: Action,
    primary: bool,
) {
    let h = hover_amt(v, action);
    let scale = 1.0 + (if primary { 0.075 } else { 0.06 }) * h;
    let br = r.scale_from_center(scale);
    let rad = br.h / 2.0;

    if primary {
        draw_glass_pill(cv, v, br, rad, 0.45 + 0.55 * h, MINT, false);
    } else if h > 0.01 {
        draw_glass_pill(cv, v, br, rad, 0.75 * h, CYAN, false);
    }
    let icon_col = if primary || h > 0.25 { TEXT } else { TEXT_SOFT };
    cv.glyph(g, size + 1.0 * h, br, icon_col, 0.92 + 0.08 * h);
    hits.push(Hit { rect: r, action });
}

fn draw_music(cv: &mut Canvas, st: &AppState, v: &View, r: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let Some(m) = &st.media else {
        let ic = Rect::new(r.x, r.y + 4.0, 88.0, 88.0);
        draw_glass_pill(cv, v, ic, 16.0, 0.2, INDIGO, false);
        cv.glyph(G_MUSIC, 26.0, ic, SUB, 0.9);
        let tx = ic.right() + 16.0;
        cv.text(t(lang, Key::NothingPlaying), Font::text(15.0, 600, Align::Left), Rect::new(tx, r.y + 22.0, r.right() - tx, 22.0), TEXT, 1.0);
        cv.text(t(lang, Key::NothingPlayingHint), Font::text(12.0, 400, Align::Left), Rect::new(tx, r.y + 46.0, r.right() - tx, 20.0), SUB, 0.9);
        return;
    };

    let art = Rect::new(r.x, r.y + 4.0, 88.0, 88.0);
    // Soft ambient shadow behind artwork.
    cv.fill_round(Rect::new(art.x - 1.5, art.y + 1.5, art.w + 3.0, art.h + 3.0), 17.0, 0x02040A, 0.40);
    match v.art {
        Some((brush, size)) => {
            cv.image(brush, size, art, 15.0);
            cv.stroke_round_linear(
                art.inset(0.5, 0.5),
                14.5,
                1.0,
                (art.x, art.y),
                (art.right(), art.bottom()),
                &[(0.0, 0xFFFFFF, 0.34), (1.0, 0xFFFFFF, 0.10)],
            );
        }
        None => {
            draw_glass_pill(cv, v, art, 15.0, 0.4, MINT, false);
            cv.glyph(G_MUSIC, 26.0, art, MINT_HI, 1.0);
        }
    }
    let tx = art.right() + 16.0;
    let tw = r.right() - tx;
    cv.text(&m.title, Font::text(15.0, 600, Align::Left), Rect::new(tx, r.y + 2.0, tw, 22.0), TEXT, 1.0);
    let sub = if m.artist.is_empty() { m.app.clone() } else { format!("{} · {}", m.artist, m.app) };
    cv.text(&sub, Font::text(12.0, 400, Align::Left), Rect::new(tx, r.y + 24.0, tw, 18.0), TEXT_SOFT, 0.85);

    // Progress (only when the player reports a timeline).
    if m.duration_ms > 0 {
        let pos = m.position_at(v.now);
        let bar = Rect::new(tx, r.y + 49.0, tw, 4.5);
        cv.fill_round(bar, 2.25, 0xFFFFFF, 0.11);
        let f = (pos as f32 / m.duration_ms as f32).clamp(0.0, 1.0);
        if f > 0.002 {
            let fw = (bar.w * f).max(4.5);
            let fill = Rect::new(bar.x, bar.y, fw, bar.h);
            cv.fill_round_linear(
                fill,
                2.25,
                (fill.x, fill.y),
                (fill.right(), fill.y),
                &[(0.0, MINT, 0.95), (0.6, MINT_HI, 1.0), (1.0, CYAN, 1.0)],
            );
            // Glowing progress thumb.
            let tx_dot = (fill.right()).clamp(bar.x + 2.25, bar.right() - 2.25);
            let cy_dot = bar.y + bar.h * 0.5;
            cv.fill_circle(tx_dot, cy_dot, 5.0, CYAN, 0.25);
            cv.fill_circle(tx_dot, cy_dot, 2.6, TEXT, 0.98);
        }
        let small = Font::text(10.5, 500, Align::Left);
        cv.text(&fmt_duration(pos), small, Rect::new(tx, r.y + 56.0, 60.0, 14.0), SUB, 0.92);
        let small_r = Font::text(10.5, 500, Align::Right);
        cv.text(&fmt_duration(m.duration_ms), small_r, Rect::new(r.right() - 60.0, r.y + 56.0, 60.0, 14.0), SUB, 0.92);
    }

    let cy = r.y + 71.0;
    let cx = tx + tw / 2.0;
    media_button(cv, hits, v, Rect::new(cx - 68.0, cy, 38.0, 32.0), G_PREV, 13.5, Action::MediaPrev, false);
    let play = Rect::new(cx - 22.0, cy - 2.0, 44.0, 36.0);
    media_button(cv, hits, v, play, if m.playing { G_PAUSE } else { G_PLAY }, 17.0, Action::MediaToggle, true);
    media_button(cv, hits, v, Rect::new(cx + 30.0, cy, 38.0, 32.0), G_NEXT, 13.5, Action::MediaNext, false);
}

fn draw_focus(cv: &mut Canvas, st: &AppState, v: &View, r: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let pomo = &st.pomodoro;
    let (label, col1, col2) = phase_style(pomo.phase, lang);
    let rem = pomo.remaining(v.now);

    cv.text(&fmt_duration(rem), Font::text(40.0, 600, Align::Left), Rect::new(r.x + 4.0, r.y - 2.0, 210.0, 54.0), TEXT, 1.0);

    // Frosted phase badge + sessions count.
    let badge_w = 26.0 + label.chars().count() as f32 * 6.8;
    let badge = Rect::new(r.x + 4.0, r.y + 53.0, badge_w, 22.0);
    draw_glass_pill(cv, v, badge, 11.0, 0.35, col1, false);
    cv.fill_circle(badge.x + 11.0, badge.y + 11.0, 3.6, col1, 1.0);
    cv.text(label, Font::text(11.5, 600, Align::Left), Rect::new(badge.x + 20.0, badge.y, badge.w - 24.0, badge.h), col1, 1.0);

    let today = ailan_core::time::local_day(v.now, st.offset_min);
    let n = st.data.focus_count(today);
    cv.text(
        &format!("{n} {}", t(lang, Key::SessionsToday)),
        Font::text(12.0, 400, Align::Left),
        Rect::new(badge.right() + 10.0, r.y + 55.0, 150.0, 18.0),
        TEXT_SOFT,
        0.85,
    );

    let bar = Rect::new(r.x + 4.0, r.y + 84.0, 240.0, 5.0);
    cv.fill_round(bar, 2.5, 0xFFFFFF, 0.11);
    let prog = pomo.progress(v.now).clamp(0.0, 1.0);
    if prog > 0.002 {
        let fw = (bar.w * prog).max(5.0);
        let fill = Rect::new(bar.x, bar.y, fw, bar.h);
        cv.fill_round_linear(
            fill,
            2.5,
            (fill.x, fill.y),
            (fill.right(), fill.y),
            &[(0.0, col1, 1.0), (1.0, col2, 1.0)],
        );
    }

    // Primary luminous Liquid Glass CTA button.
    let main = Rect::new(r.right() - 134.0, r.y + 6.0, 134.0, 38.0);
    let running = pomo.is_running();
    let h_main = hover_amt(v, Action::TimerToggle);
    let main_draw = main.scale_from_center(1.0 + 0.045 * h_main);

    // Outer glow aura behind primary CTA.
    cv.fill_round(main_draw.inset(-2.0, -2.0), 21.0, col1, 0.18 + 0.18 * h_main);
    cv.fill_round_linear(
        main_draw,
        19.0,
        (main_draw.x, main_draw.y),
        (main_draw.right(), main_draw.bottom()),
        &[
            (0.0, col1, 0.92 + 0.08 * h_main),
            (1.0, col2, 0.88 + 0.12 * h_main),
        ],
    );
    // Top glass sheen on CTA.
    cv.fill_round_linear(
        main_draw,
        19.0,
        (main_draw.x, main_draw.y),
        (main_draw.x, main_draw.y + main_draw.h * 0.6),
        &[(0.0, 0xFFFFFF, 0.28 + 0.12 * h_main), (1.0, 0xFFFFFF, 0.0)],
    );
    cv.stroke_round_linear(
        main_draw.inset(0.5, 0.5),
        18.5,
        1.1,
        (main_draw.x, main_draw.y),
        (main_draw.right(), main_draw.bottom()),
        &[(0.0, 0xFFFFFF, 0.55), (1.0, 0xFFFFFF, 0.20)],
    );

    let main_label = if running { t(lang, Key::Pause) } else { t(lang, Key::Start) };
    cv.glyph(
        if running { G_PAUSE } else { G_PLAY },
        12.0,
        Rect::new(main_draw.x + 14.0, main_draw.y, 18.0, main_draw.h),
        BG,
        1.0,
    );
    cv.text(
        main_label,
        Font::text(13.0, 650, Align::Center),
        Rect::new(main_draw.x + 22.0, main_draw.y, main_draw.w - 30.0, main_draw.h),
        BG,
        1.0,
    );
    hits.push(Hit { rect: main, action: Action::TimerToggle });

    // Secondary frosted glass pills (Reset & Skip).
    let small = |cv: &mut Canvas, hits: &mut Vec<Hit>, rect: Rect, g: char, text: &str, action: Action| {
        let h = hover_amt(v, action);
        let dr = rect.scale_from_center(1.0 + 0.05 * h);
        draw_glass_pill(cv, v, dr, dr.h / 2.0, h, col1, false);
        cv.glyph(g, 10.0, Rect::new(dr.x + 8.0, dr.y, 16.0, dr.h), if h > 0.3 { TEXT } else { TEXT_SOFT }, 0.9 + 0.1 * h);
        cv.text(
            text,
            Font::text(11.5, 500, Align::Center),
            Rect::new(dr.x + 18.0, dr.y, dr.w - 22.0, dr.h),
            TEXT,
            0.9 + 0.1 * h,
        );
        hits.push(Hit { rect, action });
    };
    small(cv, hits, Rect::new(main.x, r.y + 54.0, 65.0, 30.0), G_RESET, t(lang, Key::Reset), Action::TimerReset);
    small(cv, hits, Rect::new(main.x + 69.0, r.y + 54.0, 65.0, 30.0), G_NEXT, t(lang, Key::Skip), Action::TimerSkip);
}

fn draw_todo(cv: &mut Canvas, st: &AppState, v: &View, r: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let field = Rect::new(r.x, r.y, r.w, 30.0);
    let editing = v.input.is_some();
    let h_input = hover_amt(v, Action::TodoInput);
    let field_glow = if editing { 0.85 } else { 0.45 * h_input };

    draw_glass_pill(cv, v, field, 10.5, field_glow, MINT, !editing);
    if editing {
        cv.stroke_round_linear(
            field.inset(0.5, 0.5),
            10.0,
            1.2,
            (field.x, field.y),
            (field.right(), field.bottom()),
            &[(0.0, MINT_HI, 0.85), (1.0, CYAN, 0.70)],
        );
    }
    let plus_col = if editing || h_input > 0.3 { MINT_HI } else { TEXT_SOFT };
    cv.glyph(G_ADD, 11.0, Rect::new(field.x + 7.0, field.y, 22.0, field.h), plus_col, 0.95);
    let text_r = Rect::new(field.x + 32.0, field.y, field.w - 40.0, field.h);
    match v.input {
        Some(s) if !s.is_empty() => cv.text(&format!("{s}▏"), Font::text(12.5, 400, Align::Left), text_r, TEXT, 1.0),
        Some(_) => cv.text(t(lang, Key::AddTaskHint), Font::text(12.0, 400, Align::Left), text_r, SUB, 0.95),
        None => cv.text(t(lang, Key::AddTask), Font::text(12.5, 400, Align::Left), text_r, TEXT_SOFT, 0.78 + 0.22 * h_input),
    }
    hits.push(Hit { rect: field, action: Action::TodoInput });

    let items = st.data.todos.sorted();
    let list = Rect::new(r.x, field.bottom() + 6.0, r.w, r.bottom() - field.bottom() - 6.0);
    if items.is_empty() {
        cv.text(t(lang, Key::NoTasks), Font::text(12.5, 400, Align::Center), list, SUB, 0.85);
        return;
    }
    hits.push(Hit { rect: list, action: Action::TodoScroll });
    let scroll = v.todo_scroll.min(items.len().saturating_sub(TODO_ROWS));
    let row_h = (list.h / TODO_ROWS as f32).floor();
    for (i, item) in items.iter().skip(scroll).take(TODO_ROWS).enumerate() {
        let row = Rect::new(list.x, list.y + i as f32 * row_h, list.w, row_h);
        let h_toggle = hover_amt(v, Action::TodoToggle(item.id));
        let h_del = hover_amt(v, Action::TodoDelete(item.id));
        let h_row = h_toggle.max(h_del * 0.65);

        if h_row > 0.01 {
            let card = row.inset(0.0, 1.0);
            let accent = if h_del > h_toggle { CORAL } else { MINT };
            draw_glass_pill(cv, v, card, 8.0, 0.55 * h_row, accent, false);
        }

        let check = Rect::new(row.x + 2.0, row.y, 26.0, row.h);
        let checked = item.done;
        let check_col = if checked || h_toggle > 0.35 { MINT_HI } else { TEXT_SOFT };
        cv.glyph(
            if checked { G_BOX_CHECKED } else { G_BOX },
            13.0 + 0.8 * h_toggle,
            check,
            check_col,
            0.90 + 0.10 * h_toggle,
        );
        hits.push(Hit { rect: check, action: Action::TodoToggle(item.id) });

        let mut right = row.right();
        let del = Rect::new(right - 26.0, row.y + 2.0, 24.0, row.h - 4.0);
        if h_del > 0.01 {
            let del_pill = del.scale_from_center(1.0 + 0.08 * h_del);
            draw_glass_pill(cv, v, del_pill, del_pill.h / 2.0, 0.85 * h_del, CORAL, false);
        }
        let del_col = if h_del > 0.25 { CORAL } else { SUB };
        let del_a = 0.45 + 0.55 * h_del.max(h_row * 0.5);
        cv.glyph(G_DELETE, 9.0 + 0.8 * h_del, del, del_col, del_a);
        hits.push(Hit { rect: del, action: Action::TodoDelete(item.id) });
        right -= 30.0;
        if let Some(due) = item.due {
            let c = civil(due, st.offset_min);
            let col = if item.notified && !item.done { AMBER } else { SUB };
            cv.text(
                &format!("{:02}:{:02}", c.hour, c.minute),
                Font::text(11.0, 500, Align::Right),
                Rect::new(right - 44.0, row.y, 44.0, row.h),
                col,
                0.95,
            );
            right -= 50.0;
        }
        let text_r = Rect::new(check.right() + 4.0, row.y, right - check.right() - 4.0, row.h);
        let txt_col = if checked { SUB } else { TEXT };
        let txt_a = if checked { 0.65 } else { 0.92 + 0.08 * h_toggle };
        cv.text(&item.text, Font::text(12.5, 400, Align::Left), text_r, txt_col, txt_a);
        hits.push(Hit { rect: text_r, action: Action::TodoToggle(item.id) });
    }
    if items.len() > TODO_ROWS {
        // Scroll indicator.
        let track = Rect::new(list.right() + 6.0, list.y + 2.0, 2.5, list.h - 4.0);
        cv.fill_round(track, 1.25, 0xFFFFFF, 0.10);
        let frac = TODO_ROWS as f32 / items.len() as f32;
        let pos = scroll as f32 / items.len() as f32;
        cv.fill_round(Rect::new(track.x, track.y + track.h * pos, track.w, track.h * frac), 1.25, CYAN, 0.70);
    }
}

/// Topmost hit at a point (later entries were drawn on top).
pub fn hit_test(hits: &[Hit], x: f32, y: f32) -> Option<Action> {
    hits.iter()
        .filter(|h| h.rect.contains(x, y))
        .find(|h| !matches!(h.action, Action::Expand | Action::TodoScroll))
        .or_else(|| hits.iter().find(|h| h.rect.contains(x, y)))
        .map(|h| h.action)
}

pub fn max_todo_scroll(st: &AppState) -> usize {
    st.data.todos.items.len().saturating_sub(TODO_ROWS)
}
