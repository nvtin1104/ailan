//! Layout and drawing of the notch (immediate mode), modelled after a pure
//! monochrome Black & White aesthetic: pitch-black notch body (#000000) with
//! concave top-edge corner ears, crisp white/gray typography, cursor-only hover
//! feedback (no icon scaling on hover), and real-time audio-reactive sound waves.

use ailan_core::i18n::{short_date, t, Key, Lang};
use ailan_core::settings::{Tab, WaveColor};
use ailan_core::state::{BannerIcon, Collapsed, OsdKind};
use ailan_core::time::{civil, fmt_duration};
use ailan_core::timer::Phase;
use ailan_core::AppState;
use windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush1;

use crate::gfx::{Align, Canvas, Font, Rect};

// ---- Monochrome Palette (Pure Black & White + Neutral Grays) --------------------
pub const BG: u32 = 0x000000;
#[allow(dead_code)]
pub const PANEL_BG: u32 = 0x0A0A0C;
#[allow(dead_code)]
pub const SURFACE: u32 = 0x141417;
pub const SURFACE_HI: u32 = 0x1F1F24;

pub const WHITE: u32 = 0xFFFFFF;
pub const TEXT_SOFT: u32 = 0xA1A1AA;
pub const SUB: u32 = 0x71717A;
pub const SILVER: u32 = 0xE4E4E7;

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
pub const G_PIN: char = '\u{E718}';

// ---- geometry (DIP) -------------------------------------------------------------
/// Window width includes room for the 420px expanded notch + 2×22px concave ears.
pub const WIN_W: f32 = 480.0;
pub const WIN_H: f32 = 152.0;
pub const EXPANDED: (f32, f32) = (420.0, 140.0);

const RADIUS_COLLAPSED: f32 = 14.0;
const RADIUS_BANNER: f32 = 19.0;
const RADIUS_EXPANDED: f32 = 32.0;

pub const EAR_COLLAPSED: f32 = 10.0;
pub const EAR_BANNER: f32 = 13.0;
pub const EAR_EXPANDED: f32 = 22.0;

const TODO_ROWS: usize = 3;

pub fn collapsed_size(st: &AppState, mode: Collapsed) -> (f32, f32) {
    match mode {
        Collapsed::Minimal => {
            if st.settings.minimal_ui.hide_when_idle {
                (110.0, 5.0)
            } else {
                (150.0, 30.0)
            }
        }
        Collapsed::Clock | Collapsed::Media | Collapsed::Timer | Collapsed::Todo => (150.0, 30.0),
        Collapsed::PinPeek => (230.0, 36.0),
        Collapsed::Osd => (240.0, 34.0),
        Collapsed::Banner => (310.0, 60.0),
    }
}

/// Top-edge concave ear radius for a given expansion state and collapsed mode.
pub fn ear_for(expand: f32, mode: Collapsed) -> f32 {
    let base_ear = match mode {
        Collapsed::Banner => EAR_BANNER,
        Collapsed::PinPeek => 12.0,
        Collapsed::Osd => 11.0,
        _ => EAR_COLLAPSED,
    };
    base_ear + (EAR_EXPANDED - base_ear) * expand.clamp(0.0, 1.0)
}

/// Bottom corner radius for a given expansion state and collapsed mode.
fn radius_for(expand: f32, mode: Collapsed) -> f32 {
    let base_r = match mode {
        Collapsed::Banner => RADIUS_BANNER,
        Collapsed::PinPeek => 18.0,
        Collapsed::Osd => 16.0,
        _ => RADIUS_COLLAPSED,
    };
    base_r + (RADIUS_EXPANDED - base_r) * expand.clamp(0.0, 1.0)
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
    /// Mouse position in DIP while inside the notch.
    pub cursor: Option<(f32, f32)>,
    /// Real-time 5-band audio heights (0.15..=1.0) driven by system audio output.
    pub audio_bars: [f32; 5],
    /// Smoothed master audio peak (0.0..=1.0) for album art beat pulse.
    pub audio_peak: f32,
}

fn hover_amt(v: &View, action: Action) -> f32 {
    if v.hover == Some(action) {
        v.hover_t.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn scale_rect(r: Rect, factor: f32) -> Rect {
    let cx = r.x + r.w * 0.5;
    let cy = r.y + r.h * 0.5;
    let nw = (r.w * factor).max(0.0);
    let nh = (r.h * factor).max(0.0);
    Rect::new(cx - nw * 0.5, cy - nh * 0.5, nw, nh)
}

/// Renders the pure solid-black notch body with concave top ears on both sides.
fn draw_notch_shell(cv: &mut Canvas, st: &AppState, v: &View, p: Rect, radius: f32, ear: f32) {
    if v.mode == Collapsed::Minimal && st.settings.minimal_ui.hide_when_idle && v.expand < 0.02 {
        return;
    }
    cv.fill_notch(p, radius, ear, BG, 1.0);
}

pub fn draw(cv: &mut Canvas, st: &AppState, v: &View) -> Vec<Hit> {
    let mut hits = Vec::new();
    let p = v.pill;
    let radius = radius_for(v.expand, v.mode);
    let ear = ear_for(v.expand, v.mode);

    cv.alpha = 1.0;
    draw_notch_shell(cv, st, v, p, radius, ear);

    let collapsed_alpha = (1.0 - v.expand * 2.4).clamp(0.0, 1.0);
    let expanded_alpha = ((v.expand - 0.38) / 0.62).clamp(0.0, 1.0);

    if collapsed_alpha > 0.01 {
        cv.alpha = collapsed_alpha;
        draw_collapsed(cv, st, v, &mut hits);
    }
    if expanded_alpha > 0.01 {
        cv.alpha = expanded_alpha;
        let full = pill_rect(EXPANDED.0, EXPANDED.1);
        let y_offset = (1.0 - expanded_alpha) * -8.0;
        let animated_full = Rect::new(full.x, full.y + y_offset, full.w, full.h);
        draw_expanded(cv, st, v, animated_full, &mut hits);
    }
    cv.alpha = 1.0;
    if v.expand < 0.5 {
        hits.push(Hit { rect: p, action: Action::Expand });
    }
    hits
}

/// Monochrome circular white battery status ring.
fn draw_status_ring(cv: &mut Canvas, st: &AppState, cx: f32, cy: f32) {
    let pct = match st.battery {
        Some(Some(b)) => (b.percent as f32 / 100.0).clamp(0.05, 1.0),
        _ => 0.75,
    };
    let r = 5.4;
    let stroke = 2.2;
    cv.stroke_circle(cx, cy, r, stroke, WHITE, 0.24);
    cv.stroke_arc(cx, cy, r, -90.0, pct * 360.0, stroke, WHITE, 0.95);
}

fn wave_color_rgb(c: WaveColor) -> u32 {
    match c {
        WaveColor::White => WHITE,
        WaveColor::Silver => SILVER,
        WaveColor::SoftGray => TEXT_SOFT,
        WaveColor::DimGray => SUB,
    }
}

/// 5 vertical sound-wave bars (`14px` tall, `2.2px` wide, `2px` gap),
/// animated in real time by `v.audio_bars`.
fn draw_wave_bars(cv: &mut Canvas, st: &AppState, v: &View, r: Rect, playing: bool) {
    let bw = 2.2;
    let gap = 2.0;
    let total_w = bw * 5.0 + gap * 4.0;
    let start_x = r.x + (r.w - total_w) * 0.5;
    let cy = r.y + r.h * 0.5;
    let col = wave_color_rgb(st.settings.music_anim.color);
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
fn draw_arrow_btn(cv: &mut Canvas, v: &View, hits: &mut Vec<Hit>, cx: f32, cy: f32, left: bool) {
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

// ---- collapsed (Resting / Closed 150×30, Banner 310×60) -------------------------

fn draw_collapsed(cv: &mut Canvas, st: &AppState, v: &View, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let p = v.pill;
    let cy = p.y + p.h * 0.5;
    let c = civil(v.now, st.offset_min);
    let playing = st.media.as_ref().is_some_and(|m| m.playing);

    match v.mode {
        Collapsed::Minimal => {
            let m = &st.settings.minimal_ui;
            if m.hide_when_idle {
                return;
            }
            if !m.show_clock && !m.show_date && !m.show_battery {
                if m.show_dot {
                    let phase_ms = v.now.rem_euclid(4_200);
                    let blink = if (3_900..4_080).contains(&phase_ms) {
                        let t = (phase_ms - 3_900) as f32 / 90.0;
                        (1.0 - t).abs().clamp(0.12, 1.0)
                    } else {
                        1.0
                    };
                    let look_dx = v
                        .cursor
                        .map(|(mx, _)| ((mx - (p.x + p.w * 0.5)) / 40.0).clamp(-2.2, 2.2))
                        .unwrap_or(0.0);
                    let eye_w = 8.2;
                    let eye_h = 12.2 * blink;
                    let gap = 14.5;
                    let cx = p.x + p.w * 0.5 + look_dx;
                    let left_eye = Rect::new(cx - gap * 0.5 - eye_w, cy - eye_h * 0.5, eye_w, eye_h);
                    let right_eye = Rect::new(cx + gap * 0.5, cy - eye_h * 0.5, eye_w, eye_h);
                    cv.fill_round(left_eye, (eye_w * 0.5).min(eye_h * 0.5), WHITE, 0.96);
                    cv.fill_round(right_eye, (eye_w * 0.5).min(eye_h * 0.5), WHITE, 0.96);
                }
                return;
            }
            if m.show_battery {
                draw_status_ring(cv, st, p.x + 20.0, cy);
            }
            let time_s = format!("{:02}:{:02}", c.hour, c.minute);
            cv.text(&time_s, Font::text(14.5, 700, Align::Center), p, WHITE, 0.98);
            draw_wave_bars(cv, st, v, Rect::new(p.right() - 28.0, cy - 7.0, 20.0, 14.0), playing);
        }
        Collapsed::Clock | Collapsed::Media => {
            draw_status_ring(cv, st, p.x + 20.0, cy);
            let time_s = format!("{:02}:{:02}", c.hour, c.minute);
            cv.text(&time_s, Font::text(14.5, 700, Align::Center), p, WHITE, 0.98);
            draw_wave_bars(cv, st, v, Rect::new(p.right() - 28.0, cy - 7.0, 20.0, 14.0), playing);
        }
        Collapsed::Timer => {
            if st.settings.collapsed_ui.focus_show_icon {
                draw_status_ring(cv, st, p.x + 20.0, cy);
            }
            let rem = st.pomodoro.remaining(v.now);
            let label = if st.settings.collapsed_ui.focus_show_time {
                fmt_duration(rem)
            } else {
                format!("{:02}:{:02}", c.hour, c.minute)
            };
            cv.text(&label, Font::text(14.5, 700, Align::Center), p, WHITE, 0.98);
            draw_wave_bars(cv, st, v, Rect::new(p.right() - 28.0, cy - 7.0, 20.0, 14.0), playing);
        }
        Collapsed::Todo => {
            if st.settings.collapsed_ui.todo_show_icon {
                draw_status_ring(cv, st, p.x + 20.0, cy);
            }
            let total = st.data.todos.items.len();
            let done = total.saturating_sub(st.data.todos.pending_count());
            let label = if st.settings.collapsed_ui.todo_show_count && total > 0 {
                format!("{done}/{total}")
            } else {
                format!("{:02}:{:02}", c.hour, c.minute)
            };
            cv.text(&label, Font::text(14.5, 700, Align::Center), p, WHITE, 0.98);
            draw_wave_bars(cv, st, v, Rect::new(p.right() - 28.0, cy - 7.0, 20.0, 14.0), playing);
        }
        Collapsed::Osd => {
            let Some(o) = &st.osd else { return };
            let g = match o.kind {
                OsdKind::Volume if o.muted || o.level <= 0.0 => G_MUTE,
                OsdKind::Volume => G_VOLUME,
                OsdKind::Brightness => G_BRIGHT,
            };
            cv.glyph(g, 13.5, Rect::new(p.x + 12.0, p.y, 24.0, p.h), WHITE, 0.95);
            let bar = Rect::new(p.x + 42.0, cy - 2.0, p.w - 92.0, 4.0);
            let level = if o.muted { 0.0 } else { o.level.clamp(0.0, 1.0) };
            cv.fill_round(bar, 2.0, WHITE, 0.22);
            if level > 0.005 {
                let fw = (bar.w * level).max(4.0);
                cv.fill_round(Rect::new(bar.x, bar.y, fw, bar.h), 2.0, WHITE, 1.0);
            }
            let pct = format!("{}", (level * 100.0).round() as i32);
            cv.text(&pct, Font::text(12.0, 650, Align::Right), Rect::new(p.right() - 46.0, p.y, 32.0, p.h), WHITE, 0.95);
        }
        Collapsed::Banner => {
            let Some(b) = &st.banner else { return };
            let (g, sweep) = match b.icon {
                BannerIcon::Charging => {
                    let pct = st.battery.flatten().map(|x| x.percent as f32 / 100.0).unwrap_or(0.75);
                    (G_BOLT, pct * 360.0)
                }
                BannerIcon::BatteryLow => {
                    let pct = st.battery.flatten().map(|x| x.percent as f32 / 100.0).unwrap_or(0.20);
                    (G_BOLT, pct * 360.0)
                }
                BannerIcon::Headphones => (G_HEADPHONE, 252.0),
                BannerIcon::Reminder => (G_BELL, 270.0),
                BannerIcon::Focus => (G_TIMER, 300.0),
                BannerIcon::Break => (G_MOON, 270.0),
            };
            let ring_cx = p.x + 35.0;
            let ring_cy = p.y + 30.0;
            cv.stroke_circle(ring_cx, ring_cy, 11.0, 3.8, WHITE, 0.22);
            cv.stroke_arc(ring_cx, ring_cy, 11.0, -90.0, sweep, 3.8, WHITE, 0.95);
            cv.glyph(g, 11.5, Rect::new(ring_cx - 12.0, ring_cy - 12.0, 24.0, 24.0), WHITE, 1.0);

            let tx = p.x + 64.0;
            let tw = p.w - 140.0;
            cv.text(&b.title, Font::text(14.0, 650, Align::Left), Rect::new(tx, p.y + 11.0, tw, 20.0), WHITE, 1.0);
            cv.text(&b.body, Font::text(11.5, 400, Align::Left), Rect::new(tx, p.y + 31.0, tw, 18.0), WHITE, 0.58);

            match b.icon {
                BannerIcon::Charging | BannerIcon::BatteryLow => {
                    let pct_str = st
                        .battery
                        .flatten()
                        .map(|x| format!("{}%", x.percent))
                        .unwrap_or_else(|| b.body.clone());
                    cv.text(
                        &pct_str,
                        Font::text(21.0, 650, Align::Right),
                        Rect::new(p.right() - 84.0, p.y, 62.0, p.h),
                        WHITE,
                        1.0,
                    );
                }
                BannerIcon::Headphones => {
                    let badge_cx = p.right() - 35.0;
                    let badge_cy = p.y + 30.0;
                    cv.fill_circle(badge_cx, badge_cy, 15.0, SURFACE_HI, 1.0);
                    cv.stroke_circle(badge_cx, badge_cy, 15.0, 1.2, WHITE, 0.45);
                    cv.glyph(
                        G_HEADPHONE,
                        13.0,
                        Rect::new(badge_cx - 15.0, badge_cy - 15.0, 30.0, 30.0),
                        WHITE,
                        0.98,
                    );
                }
                _ => {
                    if b.todo_id.is_some() {
                        let snz_r = Rect::new(p.right() - 120.0, p.y + 17.0, 56.0, 26.0);
                        let h_snz = hover_amt(v, Action::SnoozeBanner(5));
                        cv.fill_round(snz_r, 13.0, WHITE, 0.14 + 0.10 * h_snz);
                        cv.text(t(lang, Key::Snooze5m), Font::text(10.5, 600, Align::Center), snz_r, WHITE, 0.95);
                        hits.push(Hit { rect: snz_r, action: Action::SnoozeBanner(5) });

                        let done_r = Rect::new(p.right() - 58.0, p.y + 17.0, 46.0, 26.0);
                        let h_done = hover_amt(v, Action::CompleteBanner);
                        cv.fill_round(done_r, 13.0, WHITE, 0.90 + 0.10 * h_done);
                        cv.text(t(lang, Key::DoneAction), Font::text(10.5, 700, Align::Center), done_r, BG, 1.0);
                        hits.push(Hit { rect: done_r, action: Action::CompleteBanner });
                    } else {
                        let time_s = format!("{:02}:{:02}", c.hour, c.minute);
                        cv.text(
                            &time_s,
                            Font::text(13.0, 600, Align::Right),
                            Rect::new(p.right() - 72.0, p.y, 52.0, p.h),
                            WHITE,
                            0.9,
                        );
                    }
                }
            }
            hits.push(Hit { rect: p, action: Action::DismissBanner });
        }
        Collapsed::PinPeek => {
            let Some(pin) = st.pin_anim else { return };
            let icon = match pin.tab {
                Tab::Focus => G_TIMER,
                Tab::Music => G_MUSIC,
                Tab::Todo => G_CHECKMARK,
            };
            let label = if pin.pinned {
                t(lang, Key::PinnedToIslandToast)
            } else {
                t(lang, Key::UnpinnedFromIslandToast)
            };
            let elapsed = (v.now - pin.start).max(0) as f32;
            let drop_t = (elapsed / 320.0).clamp(0.0, 1.0);
            let drop_y = -8.0 * (1.0 - drop_t);
            let icon_r = Rect::new(p.x + 14.0, p.y + 7.0 + drop_y, 22.0, 22.0);
            cv.fill_round(icon_r, 7.0, WHITE, 0.92);
            cv.glyph(icon, 12.0, icon_r, BG, 1.0);

            cv.text(
                label,
                Font::text(12.0, 650, Align::Left),
                Rect::new(p.x + 44.0, p.y, p.w - 52.0, p.h),
                WHITE,
                0.96,
            );
        }
    }
}

// ---- expanded (420×140 Compact Hover View) --------------------------------------

fn draw_expanded(cv: &mut Canvas, st: &AppState, v: &View, p: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let c = civil(v.now, st.offset_min);
    let hcfg = &st.settings.hover_ui;
    let playing = st.media.as_ref().is_some_and(|m| m.playing);

    // 1. Shared Top Status Row (y = 18): Battery ring + Date & Time + Home icon + Wave bars.
    let hdr_cy = p.y + 18.0;
    if hcfg.show_battery {
        draw_status_ring(cv, st, p.x + 146.0, hdr_cy);
    }
    let mut hx = p.x + 160.0;
    if hcfg.show_date {
        let ds = format!("{} {}", short_date(lang, &c), c.year);
        let dw = (ds.chars().count() as f32 * 6.5).max(88.0);
        cv.text(&ds, Font::text(12.0, 400, Align::Left), Rect::new(hx, p.y + 8.0, dw, 20.0), WHITE, 0.85);
        hx += dw + 4.0;
    }
    if hcfg.show_clock {
        let time_s = format!("{:02}:{:02}", c.hour, c.minute);
        cv.text(&time_s, Font::text(12.5, 700, Align::Left), Rect::new(hx, p.y + 8.0, 44.0, 20.0), WHITE, 1.0);
    }

    // Wave bars and Home icon on the top-right
    let wave_w = 20.0;
    let wave_h = 14.0;
    let wave_r = Rect::new(p.right() - 28.0 - wave_w, hdr_cy - wave_h * 0.5, wave_w, wave_h);

    let home_w = 26.0;
    let home_h = 24.0;
    let home_gap = 14.0;
    let home_r = Rect::new(wave_r.x - home_gap - home_w, hdr_cy - home_h * 0.5, home_w, home_h);
    let h_home = hover_amt(v, Action::OpenSettings);
    if h_home > 0.01 {
        cv.fill_round(home_r, 8.0, WHITE, 0.16 * h_home);
        cv.hairline_round(home_r, 8.0, WHITE, 0.22 * h_home);
    }
    cv.glyph(G_HOME, 12.0, home_r, WHITE, 0.70 + 0.30 * h_home);
    hits.push(Hit { rect: home_r.inset(-3.0, -3.0), action: Action::OpenSettings });

    // 5 audio-reactive sound-wave bars
    draw_wave_bars(cv, st, v, wave_r, playing);

    // 2. Filter visible activities on Hover UI:
    // - Music triggers automatically when playing/active, or if pinned.
    // - Focus & Todo only show when running or pinned.
    let visible = st.hover_activities();
    if visible.is_empty() {
        draw_empty_hover(cv, st, v, p, hits);
        return;
    }

    let active_tab = if visible.contains(&v.tab) { v.tab } else { visible[0] };
    let target_idx = visible.iter().position(|&t| t == active_tab).unwrap_or(0) as f32;
    let slide_dx = if visible.len() > 1 {
        (target_idx - v.tab_pos).clamp(-1.0, 1.0) * 14.0
    } else {
        0.0
    };
    let page_r = Rect::new(p.x + slide_dx, p.y, p.w, p.h);

    // 3. Left / Right Slide Arrow Buttons only when > 1 visible activity.
    if hcfg.show_slide_controls && visible.len() > 1 {
        if active_tab != Tab::Music {
            draw_arrow_btn(cv, v, hits, p.x + 22.0, p.y + 70.0, true);
        }
        draw_arrow_btn(cv, v, hits, p.x + 395.0, p.y + 70.0, false);
    }

    // 4. Main Activity Page Content.
    match active_tab {
        Tab::Music => draw_music(cv, st, v, page_r, hits),
        Tab::Focus => draw_focus(cv, st, v, page_r, hits),
        Tab::Todo => draw_todo(cv, st, v, page_r, hits),
    }

    // 5. Bottom Page Indicator Dots (only when > 1 visible activity, no hover height scaling).
    if visible.len() > 1 {
        let gap = 5.0;
        let widths: Vec<f32> = visible
            .iter()
            .enumerate()
            .map(|(idx, _)| {
                let dist = (idx as f32 - v.tab_pos).abs().clamp(0.0, 1.0);
                4.0 + (15.0 - 4.0) * (1.0 - dist)
            })
            .collect();
        let total_dots_w: f32 = widths.iter().sum::<f32>() + gap * (visible.len().saturating_sub(1) as f32);
        let mut dx = p.x + (p.w - total_dots_w) * 0.5;
        let dy = p.y + 131.2;
        for (idx, &act) in visible.iter().enumerate() {
            let dw = widths[idx];
            let dist = (idx as f32 - v.tab_pos).abs().clamp(0.0, 1.0);
            let h_dot = hover_amt(v, Action::Tab(act));
            let dh = 3.6;
            let dot_r = Rect::new(dx, dy, dw, dh);
            let alpha = 0.35 + 0.65 * (1.0 - dist).max(h_dot * 0.5);
            cv.fill_round(dot_r, dh * 0.5, WHITE, alpha);
            hits.push(Hit {
                rect: Rect::new(dx - 2.0, dy - 7.0, dw + 4.0, 16.0),
                action: Action::Tab(act),
            });
            dx += dw + gap;
        }
    }
}

/// Displayed when no activity is running or pinned on Hover UI.
fn draw_empty_hover(cv: &mut Canvas, st: &AppState, v: &View, p: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    cv.text(
        t(lang, Key::NoHoverActivity),
        Font::text(13.5, 650, Align::Center),
        Rect::new(p.x + 24.0, p.y + 42.0, p.w - 48.0, 20.0),
        WHITE,
        0.92,
    );
    cv.text(
        t(lang, Key::NoHoverActivityHint),
        Font::text(11.0, 400, Align::Center),
        Rect::new(p.x + 24.0, p.y + 62.0, p.w - 48.0, 18.0),
        TEXT_SOFT,
        0.75,
    );

    let btn_w = 132.0;
    let btn_r = Rect::new(p.x + (p.w - btn_w) * 0.5, p.y + 92.0, btn_w, 28.0);
    let h = hover_amt(v, Action::OpenSettings);
    cv.fill_round(btn_r, 14.0, WHITE, 0.12 + 0.08 * h);
    cv.stroke_round(btn_r.inset(0.5, 0.5), 13.5, 1.0, WHITE, 0.22 + 0.18 * h);
    cv.glyph(G_HOME, 11.0, Rect::new(btn_r.x + 12.0, btn_r.y, 18.0, btn_r.h), WHITE, 0.95);
    cv.text(
        t(lang, Key::OpenHubButton),
        Font::text(11.5, 600, Align::Center),
        Rect::new(btn_r.x + 24.0, btn_r.y, btn_r.w - 32.0, btn_r.h),
        WHITE,
        0.96,
    );
    hits.push(Hit { rect: btn_r, action: Action::OpenSettings });
}

// ---- Page 1: Music (`l-music` in 420×140) ---------------------------------------

fn draw_music(cv: &mut Canvas, st: &AppState, v: &View, p: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let base_art = Rect::new(p.x + 22.0, p.y + 12.0, 62.0, 62.0);

    let Some(m) = &st.media else {
        cv.fill_round(base_art, 10.0, WHITE, 0.08);
        cv.glyph(G_MUSIC, 22.0, base_art, WHITE, 0.55);
        cv.text(
            t(lang, Key::NothingPlaying),
            Font::text(15.0, 650, Align::Left),
            Rect::new(p.x + 100.0, p.y + 36.0, 270.0, 20.0),
            WHITE,
            1.0,
        );
        cv.text(
            t(lang, Key::NothingPlayingHint),
            Font::text(11.5, 400, Align::Left),
            Rect::new(p.x + 100.0, p.y + 54.0, 270.0, 18.0),
            WHITE,
            0.50,
        );
        let bar = Rect::new(p.x + 77.0, p.y + 89.0, 260.0, 4.0);
        cv.fill_round(bar, 2.0, WHITE, 0.22);
        cv.text("00:00", Font::text(12.0, 500, Align::Left), Rect::new(p.x + 22.0, p.y + 82.0, 48.0, 18.0), WHITE, 0.55);
        cv.text("-00:00", Font::text(12.0, 500, Align::Right), Rect::new(p.right() - 75.0, p.y + 82.0, 50.0, 18.0), WHITE, 0.55);
        draw_music_controls(cv, v, p, false, hits);
        return;
    };

    // 1. 62×62 Album Art at (22, 12) with subtle audio-reactive bass pulse when playing.
    let pulse = if m.playing && st.settings.music_anim.art_pulse {
        1.0 + 0.045 * v.audio_peak
    } else {
        1.0
    };
    let art = scale_rect(base_art, pulse);
    match v.art {
        Some((brush, size)) => {
            cv.image(brush, size, art, 10.0);
        }
        None => {
            cv.fill_round(art, 10.0, WHITE, 0.10 + 0.06 * v.audio_peak);
            cv.glyph(G_MUSIC, 22.0, art, WHITE, 0.90);
        }
    }

    // 2. Title & Artist
    cv.text(
        &m.title,
        Font::text(15.0, 650, Align::Left),
        Rect::new(p.x + 100.0, p.y + 36.0, 276.0, 20.0),
        WHITE,
        1.0,
    );
    let sub = if m.artist.is_empty() { m.app.clone() } else { format!("{} - {}", m.artist, m.app) };
    cv.text(
        &sub,
        Font::text(11.5, 400, Align::Left),
        Rect::new(p.x + 100.0, p.y + 54.0, 276.0, 16.0),
        WHITE,
        0.50,
    );

    // 3. Progress bar row at y = 89 (elapsed | 260px white bar | -remaining).
    let pos = if m.duration_ms > 0 { m.position_at(v.now) } else { 0 };
    let rem = (m.duration_ms - pos).max(0);
    let bar = Rect::new(p.x + 77.0, p.y + 89.0, 260.0, 4.0);
    cv.fill_round(bar, 2.0, WHITE, 0.22);
    if m.duration_ms > 0 {
        let f = (pos as f32 / m.duration_ms as f32).clamp(0.0, 1.0);
        if f > 0.002 {
            let fw = (bar.w * f).max(4.0);
            let fill = Rect::new(bar.x, bar.y, fw, bar.h);
            cv.fill_round(fill, 2.0, WHITE, 1.0);
            if m.playing {
                let head_x = fill.right().clamp(bar.x + 2.0, bar.right() - 2.0);
                let head_y = bar.y + bar.h * 0.5;
                cv.fill_circle(head_x, head_y, 2.8, WHITE, 0.98);
            }
        }
    }
    cv.text(
        &fmt_duration(pos),
        Font::text(12.0, 500, Align::Left),
        Rect::new(p.x + 22.0, p.y + 82.0, 50.0, 18.0),
        WHITE,
        0.95,
    );
    cv.text(
        &format!("-{}", fmt_duration(rem)),
        Font::text(12.0, 500, Align::Right),
        Rect::new(p.right() - 75.0, p.y + 82.0, 50.0, 18.0),
        WHITE,
        0.95,
    );

    // 4. Transport controls row at y = 115 (quick-add button removed, cursor-only hover).
    draw_music_controls(cv, v, p, m.playing, hits);
}

fn draw_music_controls(cv: &mut Canvas, v: &View, p: Rect, playing: bool, hits: &mut Vec<Hit>) {
    // Prev (x = 162.5), Play/Pause (x = 210), Next (x = 257.5) at y = 115.
    let ctl = |cv: &mut Canvas, hits: &mut Vec<Hit>, cx: f32, glyph: char, size: f32, action: Action| {
        let cy = p.y + 115.0;
        let r = Rect::new(cx - 16.0, cy - 13.0, 32.0, 26.0);
        let h = hover_amt(v, action);
        if h > 0.01 {
            cv.fill_circle(cx, cy, 12.0, WHITE, 0.14 * h);
        }
        cv.glyph(glyph, size, r, WHITE, 0.85 + 0.15 * h);
        hits.push(Hit { rect: r, action });
    };

    ctl(cv, hits, p.x + 162.5, G_PREV, 13.5, Action::MediaPrev);
    ctl(
        cv,
        hits,
        p.x + 210.0,
        if playing { G_PAUSE } else { G_PLAY },
        15.0,
        Action::MediaToggle,
    );
    ctl(cv, hits, p.x + 257.5, G_NEXT, 13.5, Action::MediaNext);
}

// ---- Page 2: Focus Timer / Pomodoro (`l-pomo` in 420×140) -----------------------

fn draw_focus(cv: &mut Canvas, st: &AppState, v: &View, p: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let pomo = &st.pomodoro;
    let rem = pomo.remaining(v.now);
    let prog = pomo.progress(v.now).clamp(0.0, 1.0);
    let total_rounds = pomo.durations.long_every.max(1).min(4);
    let cur_round = (pomo.completed % total_rounds) + 1;

    // Top-left title: "Pomodoro"
    cv.text(
        "Pomodoro",
        Font::text(15.0, 650, Align::Left),
        Rect::new(p.x + 15.0, p.y + 9.0, 115.0, 20.0),
        WHITE,
        1.0,
    );

    // ---- Left Column (x = 45..180) ----
    let phase_name = match pomo.phase {
        Phase::Focus => t(lang, Key::Focus),
        Phase::ShortBreak => t(lang, Key::ShortBreak),
        Phase::LongBreak => t(lang, Key::LongBreak),
    };
    let round_str = format!("{phase_name} · {cur_round} of {total_rounds}");
    cv.text(
        &round_str,
        Font::text(10.5, 500, Align::Left),
        Rect::new(p.x + 46.0, p.y + 28.0, 140.0, 14.0),
        WHITE,
        0.55,
    );

    // Big 28px countdown timer at (45, 41)
    cv.text(
        &fmt_duration(rem),
        Font::text(28.0, 700, Align::Left),
        Rect::new(p.x + 45.0, p.y + 41.0, 140.0, 34.0),
        WHITE,
        1.0,
    );

    // End clock time + "until done" at y = 79
    let end_civil = civil(v.now + rem, st.offset_min);
    let end_time_s = format!("{:02}:{:02}", end_civil.hour, end_civil.minute);
    cv.text(
        &end_time_s,
        Font::text(13.5, 650, Align::Left),
        Rect::new(p.x + 46.0, p.y + 77.0, 44.0, 18.0),
        WHITE,
        0.96,
    );
    let until_lbl = if lang == Lang::Vi { "đến khi xong" } else { "until done" };
    cv.text(
        until_lbl,
        Font::text(11.5, 600, Align::Left),
        Rect::new(p.x + 92.0, p.y + 78.0, 96.0, 18.0),
        TEXT_SOFT,
        0.90,
    );

    // Left 128×4 monochrome progress bar at (46, 106)
    let lbar = Rect::new(p.x + 46.0, p.y + 106.0, 128.0, 4.0);
    cv.fill_round(lbar, 2.0, WHITE, 0.16);
    if prog > 0.002 {
        let fw = (lbar.w * prog).max(4.0);
        cv.fill_round(Rect::new(lbar.x, lbar.y, fw, lbar.h), 2.0, WHITE, 0.96);
    }

    // ---- Vertical Divider at x = 192, top = 30, h = 88 ----
    cv.fill_rect(Rect::new(p.x + 192.0, p.y + 30.0, 1.0, 88.0), WHITE, 0.12);

    // ---- Right Column (x = 206..380) ----
    let this_round_lbl = if lang == Lang::Vi { "Vòng này" } else { "This round" };
    cv.text(
        this_round_lbl,
        Font::text(10.5, 500, Align::Left),
        Rect::new(p.x + 206.0, p.y + 30.0, 80.0, 18.0),
        WHITE,
        0.55,
    );

    // Pause / Start pill button at (292, 30, 60×20) without hover scaling
    let pause_r = Rect::new(p.x + 292.0, p.y + 30.0, 60.0, 20.0);
    let h_pause = hover_amt(v, Action::TimerToggle);
    if pomo.is_running() {
        cv.fill_round(pause_r, pause_r.h * 0.5, WHITE, 0.92);
        cv.text(t(lang, Key::Pause), Font::text(10.5, 700, Align::Center), pause_r, BG, 1.0);
    } else {
        cv.fill_round(pause_r, pause_r.h * 0.5, WHITE, 0.14 + 0.10 * h_pause);
        cv.text(t(lang, Key::Start), Font::text(10.5, 700, Align::Center), pause_r, WHITE, 0.98);
    }
    hits.push(Hit { rect: pause_r, action: Action::TimerToggle });

    // Reset circular button at (358, 31, 18×18) without hover scaling
    let reset_r = Rect::new(p.x + 358.0, p.y + 31.0, 18.0, 18.0);
    let h_reset = hover_amt(v, Action::TimerReset);
    cv.fill_circle(reset_r.x + 9.0, reset_r.y + 9.0, 9.0, WHITE, 0.12 + 0.12 * h_reset);
    cv.glyph(G_RESET, 9.5, reset_r, WHITE, 0.92);
    hits.push(Hit { rect: reset_r.inset(-2.0, -2.0), action: Action::TimerReset });

    // Segmented round timeline at y = 71 (monochrome white/gray)
    let segs: [(f32, f32, bool); 8] = [
        (206.00, 28.65, true),
        (237.65, 5.73, false),
        (246.38, 28.65, true),
        (278.03, 5.73, false),
        (286.76, 28.65, true),
        (318.41, 5.73, false),
        (327.14, 28.65, true),
        (358.79, 17.19, false),
    ];
    let active_seg_idx = match pomo.phase {
        Phase::Focus => ((cur_round - 1) as usize * 2).min(6),
        Phase::ShortBreak => ((cur_round.saturating_sub(1) as usize) * 2 + 1).min(5),
        Phase::LongBreak => 7,
    };

    let mut head_x = p.x + segs[active_seg_idx].0;
    for (i, &(sx, sw, _is_focus)) in segs.iter().enumerate() {
        let sr = Rect::new(p.x + sx, p.y + 71.0, sw, 6.0);
        cv.fill_round(sr, 3.0, WHITE, 0.14);
        if i < active_seg_idx {
            cv.fill_round(sr, 3.0, WHITE, 0.85);
        } else if i == active_seg_idx {
            let fw = (sw * prog).max(3.5);
            cv.fill_round(Rect::new(sr.x, sr.y, fw, sr.h), 3.0, WHITE, 1.0);
            head_x = sr.x + fw;
        }
    }

    cv.fill_circle(head_x, p.y + 74.0, 6.5, WHITE, 0.25);
    cv.fill_circle(head_x, p.y + 74.0, 3.2, WHITE, 1.0);

    let labels: [(f32, &str, usize); 5] = [
        (220.33, "1", 0),
        (260.71, "2", 2),
        (301.08, "3", 4),
        (341.46, "4", 6),
        (367.39, "Long", 7),
    ];
    for (lx, txt, seg_i) in labels {
        let is_on = seg_i == active_seg_idx;
        let is_past = seg_i < active_seg_idx;
        let a = if is_on { 1.0 } else if is_past { 0.72 } else { 0.38 };
        let w = if is_on { 700 } else { 600 };
        cv.text(
            txt,
            Font::text(8.6, w, Align::Center),
            Rect::new(p.x + lx - 14.0, p.y + 81.0, 28.0, 12.0),
            WHITE,
            a,
        );
    }

    let total_min = pomo.durations.focus_min * total_rounds
        + pomo.durations.short_min * total_rounds.saturating_sub(1)
        + pomo.durations.long_min;
    let hrs = total_min / 60;
    let mins = total_min % 60;
    let summary = if hrs > 0 {
        format!("{hrs} h {mins} min · {total_rounds} sessions")
    } else {
        format!("{mins} min · {total_rounds} sessions")
    };
    cv.text(
        &summary,
        Font::text(9.5, 400, Align::Left),
        Rect::new(p.x + 206.0, p.y + 99.0, 172.0, 14.0),
        WHITE,
        0.50,
    );
}

// ---- Page 3: To-do (`l-todo` in 420×140) ----------------------------------------

fn draw_todo(cv: &mut Canvas, st: &AppState, v: &View, p: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let items = st.data.todos.sorted();
    let total = items.len();
    let pending = st.data.todos.pending_count();
    let done = total.saturating_sub(pending);

    // Top-left monochrome title: "Todo List"
    cv.text(
        "Todo",
        Font::text(15.0, 700, Align::Left),
        Rect::new(p.x + 15.0, p.y + 9.0, 40.0, 20.0),
        WHITE,
        1.0,
    );
    cv.text(
        "List",
        Font::text(15.0, 400, Align::Left),
        Rect::new(p.x + 53.0, p.y + 9.0, 45.0, 20.0),
        TEXT_SOFT,
        0.95,
    );

    // ---- Left Column (x = 44): Big ratio + "Task Done Today" + Add Task pill ----
    let ratio_s = format!("{done}/{total}");
    cv.text(
        &ratio_s,
        Font::text(31.0, 700, Align::Left),
        Rect::new(p.x + 44.0, p.y + 31.0, 140.0, 36.0),
        WHITE,
        1.0,
    );
    let sub_lbl = if lang == Lang::Vi { "Việc hoàn thành" } else { "Task Done Today" };
    cv.text(
        sub_lbl,
        Font::text(11.5, 400, Align::Left),
        Rect::new(p.x + 44.0, p.y + 68.0, 148.0, 16.0),
        WHITE,
        0.50,
    );

    let field = Rect::new(p.x + 41.0, p.y + 92.0, 146.0, 28.0);
    if let Some(s) = v.input {
        cv.fill_round(field, 14.0, WHITE, 0.16);
        cv.stroke_round(field.inset(0.5, 0.5), 13.5, 1.0, WHITE, 0.75);
        let txt_r = Rect::new(field.x + 10.0, field.y, field.w - 16.0, field.h);
        if s.is_empty() {
            cv.text(t(lang, Key::AddTask), Font::text(11.0, 400, Align::Left), txt_r, WHITE, 0.55);
        } else {
            cv.text(&format!("{s}▏"), Font::text(11.5, 500, Align::Left), txt_r, WHITE, 1.0);
        }
        hits.push(Hit { rect: field, action: Action::TodoInput });
    } else {
        let h_add = hover_amt(v, Action::TodoInput);
        cv.fill_round(field, 14.0, WHITE, 0.12 + 0.08 * h_add);
        cv.stroke_round(field.inset(0.5, 0.5), 13.5, 1.0, WHITE, 0.18 + 0.14 * h_add);
        cv.glyph(G_ADD, 9.5, Rect::new(field.x + 10.0, field.y, 16.0, field.h), WHITE, 0.95);
        cv.text(
            if lang == Lang::Vi { "Thêm việc mới" } else { "Add Task" },
            Font::text(11.0, 600, Align::Left),
            Rect::new(field.x + 28.0, field.y, field.w - 34.0, field.h),
            WHITE,
            0.95,
        );
        hits.push(Hit { rect: field, action: Action::TodoInput });
    }

    // ---- Right Column (x = 202, width = 176): Up to 3 task rows ----
    let list_r = Rect::new(p.x + 202.0, p.y + 34.0, 176.0, 90.0);
    if items.is_empty() {
        cv.fill_round(Rect::new(list_r.x, p.y + 48.0, list_r.w, 56.0), 10.0, WHITE, 0.05);
        cv.text(t(lang, Key::NoTasks), Font::text(11.5, 400, Align::Center), list_r, WHITE, 0.45);
        return;
    }

    hits.push(Hit { rect: list_r, action: Action::TodoScroll });
    let scroll = v.todo_scroll.min(items.len().saturating_sub(TODO_ROWS));
    for (i, item) in items.iter().skip(scroll).take(TODO_ROWS).enumerate() {
        let row = Rect::new(p.x + 202.0, p.y + 34.0 + i as f32 * 31.0, 176.0, 28.0);
        let h_toggle = hover_amt(v, Action::TodoToggle(item.id));
        let h_del = hover_amt(v, Action::TodoDelete(item.id));
        let h_any = h_toggle.max(h_del);

        if item.done {
            cv.fill_round(row, 9.0, WHITE, 0.18 + 0.06 * h_any);
        } else {
            cv.fill_round(row, 9.0, WHITE, 0.08 + 0.06 * h_any);
        }

        // Monochrome circular check indicator (16×16 circle, no hover scale)
        let chk_cx = row.x + 15.0;
        let chk_cy = row.y + row.h * 0.5;
        if item.done {
            cv.fill_circle(chk_cx, chk_cy, 7.5, WHITE, 0.95);
            cv.glyph(
                G_CHECKMARK,
                8.5,
                Rect::new(chk_cx - 8.0, chk_cy - 8.0, 16.0, 16.0),
                BG,
                1.0,
            );
        } else {
            cv.stroke_circle(chk_cx, chk_cy, 7.2, 1.3, WHITE, 0.50 + 0.35 * h_toggle);
        }

        // Delete button at right edge of row (no hover scale)
        let del_r = Rect::new(row.right() - 22.0, row.y + 2.0, 20.0, row.h - 4.0);
        if h_del > 0.01 || h_toggle > 0.01 {
            cv.glyph(
                G_DELETE,
                8.5,
                del_r,
                WHITE,
                0.45 + 0.55 * h_del,
            );
        }
        hits.push(Hit { rect: del_r, action: Action::TodoDelete(item.id) });

        let txt_r = Rect::new(row.x + 28.0, row.y, row.w - 50.0, row.h);
        cv.text(
            &item.text,
            Font::text(11.5, 500, Align::Left),
            txt_r,
            WHITE,
            if item.done { 0.55 } else { 0.95 },
        );
        hits.push(Hit {
            rect: Rect::new(row.x, row.y, row.w - 22.0, row.h),
            action: Action::TodoToggle(item.id),
        });
    }

    // Scrollbar at x = 384 when > 3 tasks
    if items.len() > TODO_ROWS {
        let track_h = 84.0;
        let frac = (TODO_ROWS as f32 / items.len() as f32).clamp(0.2, 1.0);
        let pos = scroll as f32 / items.len() as f32;
        let thumb_h = (track_h * frac).max(16.0);
        let thumb_y = p.y + 36.0 + (track_h - thumb_h) * pos;
        cv.fill_round(Rect::new(p.x + 383.0, thumb_y, 3.0, thumb_h), 1.5, WHITE, 0.40);
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
