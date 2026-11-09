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

// ---- palette (Ailan brand) ------------------------------------------------------
pub const BG: u32 = 0x000000;
const SURFACE: u32 = 0x1C1C1F;
const SURFACE_HI: u32 = 0x2A2A2F;
const TEXT: u32 = 0xF5F5F7;
const SUB: u32 = 0x8E8E96;
const MINT: u32 = 0x6EE7B7;
const CORAL: u32 = 0xFDA4AF;
const AMBER: u32 = 0xFBBF24;

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
}

pub fn draw(cv: &mut Canvas, st: &AppState, v: &View) -> Vec<Hit> {
    let mut hits = Vec::new();
    let p = v.pill;
    let radius = RADIUS_COLLAPSED + (RADIUS_EXPANDED - RADIUS_COLLAPSED) * v.expand;
    // Notch body: extend above the screen edge so only the bottom corners show.
    cv.alpha = 1.0;
    cv.fill_round(Rect::new(p.x, -radius, p.w, p.h + radius), radius, BG, 1.0);

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
            cv.text(&format!("{:02}:{:02}", c.hour, c.minute), Font::text(13.0, 600, Align::Center), p, TEXT, 1.0);
        }
        Collapsed::Media => {
            let art = Rect::new(p.x + 10.0, p.y + 5.0, 20.0, 20.0);
            match v.art {
                Some((brush, size)) => cv.image(brush, size, art, 5.0),
                None => {
                    cv.fill_round(art, 5.0, SURFACE_HI, 1.0);
                    cv.glyph(G_MUSIC, 10.0, art, MINT, 1.0);
                }
            }
            if let Some(m) = &st.media {
                let r = Rect::new(art.right() + 8.0, p.y, p.w - 76.0, p.h);
                cv.text(&m.title, Font::text(12.0, 500, Align::Left), r, TEXT, 0.9);
            }
            equalizer(cv, Rect::new(p.right() - 30.0, p.y + 9.0, 18.0, 12.0));
        }
        Collapsed::Timer => {
            let (label, col) = phase_style(st.pomodoro.phase, lang);
            cv.fill_circle(p.x + 18.0, p.y + p.h / 2.0, 4.0, col, 1.0);
            cv.text(label, Font::text(12.0, 500, Align::Left), Rect::new(p.x + 30.0, p.y, 100.0, p.h), SUB, 1.0);
            let mins = (st.pomodoro.remaining(v.now) + MINUTE - 1) / MINUTE;
            let s = format!("{mins}{}", t(lang, Key::MinuteShort));
            cv.text(&s, Font::text(13.0, 600, Align::Right), Rect::new(p.right() - 70.0, p.y, 56.0, p.h), col, 1.0);
        }
        Collapsed::Osd => {
            let Some(o) = &st.osd else { return };
            let (g, label) = match o.kind {
                OsdKind::Volume if o.muted || o.level <= 0.0 => (G_MUTE, t(lang, Key::Muted)),
                OsdKind::Volume => (G_VOLUME, t(lang, Key::Volume)),
                OsdKind::Brightness => (G_BRIGHT, t(lang, Key::Brightness)),
            };
            let _ = label;
            cv.glyph(g, 14.0, Rect::new(p.x + 10.0, p.y, 26.0, p.h), TEXT, 1.0);
            let bar = Rect::new(p.x + 42.0, p.y + p.h / 2.0 - 3.0, p.w - 100.0, 6.0);
            let level = if o.muted { 0.0 } else { o.level };
            cv.fill_round(bar, 3.0, SURFACE_HI, 1.0);
            cv.fill_round(Rect::new(bar.x, bar.y, bar.w * level, bar.h), 3.0, TEXT, 1.0);
            let pct = format!("{}", (level * 100.0).round() as i32);
            cv.text(&pct, Font::text(12.0, 600, Align::Right), Rect::new(p.right() - 50.0, p.y, 38.0, p.h), SUB, 1.0);
        }
        Collapsed::Banner => {
            let Some(b) = &st.banner else { return };
            let (g, col) = match b.icon {
                BannerIcon::Charging => (G_BOLT, MINT),
                BannerIcon::BatteryLow => (G_BOLT, AMBER),
                BannerIcon::Headphones => (G_HEADPHONE, TEXT),
                BannerIcon::Reminder => (G_BELL, AMBER),
                BannerIcon::Focus => (G_TIMER, MINT),
                BannerIcon::Break => (G_MOON, CORAL),
            };
            let ic = Rect::new(p.x + 12.0, p.y + 8.0, 32.0, 32.0);
            cv.fill_round(ic, 10.0, SURFACE, 1.0);
            cv.glyph(g, 15.0, ic, col, 1.0);
            let tx = ic.right() + 10.0;
            let tw = p.right() - tx - 14.0;
            cv.text(&b.title, Font::text(12.5, 600, Align::Left), Rect::new(tx, p.y + 6.0, tw, 18.0), TEXT, 1.0);
            cv.text(&b.body, Font::text(12.0, 400, Align::Left), Rect::new(tx, p.y + 24.0, tw, 18.0), SUB, 1.0);
            hits.push(Hit { rect: p, action: Action::DismissBanner });
        }
    }
}

/// Static "now playing" bars — deliberately not animated so playback costs 0 frames.
fn equalizer(cv: &mut Canvas, r: Rect) {
    let heights = [0.55, 1.0, 0.7, 0.85];
    let bw = 3.0;
    let gap = (r.w - bw * heights.len() as f32) / (heights.len() as f32 - 1.0);
    for (i, h) in heights.iter().enumerate() {
        let bh = r.h * h;
        cv.fill_round(Rect::new(r.x + i as f32 * (bw + gap), r.bottom() - bh, bw, bh), 1.5, MINT, 1.0);
    }
}

fn phase_style(phase: Phase, lang: ailan_core::i18n::Lang) -> (&'static str, u32) {
    match phase {
        Phase::Focus => (t(lang, Key::Focus), MINT),
        Phase::ShortBreak => (t(lang, Key::ShortBreak), CORAL),
        Phase::LongBreak => (t(lang, Key::LongBreak), CORAL),
    }
}

// ---- expanded -------------------------------------------------------------------

fn draw_expanded(cv: &mut Canvas, st: &AppState, v: &View, p: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let c = civil(v.now, st.offset_min);

    // Header: time + date (left), status (right).
    let time = format!("{:02}:{:02}", c.hour, c.minute);
    cv.text(&time, Font::text(15.0, 650, Align::Left), Rect::new(p.x + 22.0, 8.0, 60.0, 26.0), TEXT, 1.0);
    cv.text(&short_date(lang, &c), Font::text(12.0, 400, Align::Left), Rect::new(p.x + 72.0, 8.0, 120.0, 26.0), SUB, 1.0);
    draw_status(cv, st, Rect::new(p.right() - 220.0, 8.0, 200.0, 26.0));

    // Tabs.
    let tabs = [(Tab::Music, Key::TabMusic), (Tab::Focus, Key::TabFocus), (Tab::Todo, Key::TabTodo)];
    let mut x = p.x + 16.0;
    for (tab, key) in tabs {
        let label = t(lang, key);
        let w = 26.0 + label.chars().count() as f32 * 7.2;
        let r = Rect::new(x, 38.0, w, 26.0);
        let active = v.tab == tab;
        if active {
            cv.fill_round(r, 13.0, SURFACE_HI, 1.0);
        } else if v.hover == Some(Action::Tab(tab)) {
            cv.fill_round(r, 13.0, SURFACE, 1.0);
        }
        cv.text(label, Font::text(12.0, if active { 600 } else { 500 }, Align::Center), r, if active { TEXT } else { SUB }, 1.0);
        hits.push(Hit { rect: r, action: Action::Tab(tab) });
        x += w + 4.0;
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
        cv.text(&pct, Font::text(12.0, 500, Align::Right), Rect::new(x - 36.0, r.y, 36.0, r.h), TEXT, 1.0);
        x -= 40.0;
        let col = if b.charging { MINT } else if b.percent <= 20 { AMBER } else { TEXT };
        let body = Rect::new(x - 24.0, cy - 5.5, 22.0, 11.0);
        cv.stroke_round(body, 3.0, 1.2, SUB, 1.0);
        cv.fill_round(Rect::new(body.right() + 0.5, cy - 2.5, 2.0, 5.0), 1.0, SUB, 1.0);
        let inner = body.inset(2.2, 2.2);
        cv.fill_round(Rect::new(inner.x, inner.y, inner.w * b.percent as f32 / 100.0, inner.h), 1.5, col, 1.0);
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
    cv.glyph(g, 13.0, Rect::new(x - 22.0, r.y, 22.0, r.h), col, 1.0);
    x -= 26.0;
    if st.audio_device.as_ref().is_some_and(|(_, hp)| *hp) {
        cv.glyph(G_HEADPHONE, 13.0, Rect::new(x - 22.0, r.y, 22.0, r.h), TEXT, 1.0);
    }
}

fn button(cv: &mut Canvas, hits: &mut Vec<Hit>, v: &View, r: Rect, g: char, size: f32, action: Action) {
    if v.hover == Some(action) {
        cv.fill_round(r, r.h / 2.0, SURFACE_HI, 1.0);
    }
    cv.glyph(g, size, r, TEXT, 1.0);
    hits.push(Hit { rect: r, action });
}

fn draw_music(cv: &mut Canvas, st: &AppState, v: &View, r: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let Some(m) = &st.media else {
        let ic = Rect::new(r.x, r.y + 4.0, 88.0, 88.0);
        cv.fill_round(ic, 14.0, SURFACE, 1.0);
        cv.glyph(G_MUSIC, 26.0, ic, SUB, 1.0);
        let tx = ic.right() + 16.0;
        cv.text(t(lang, Key::NothingPlaying), Font::text(15.0, 600, Align::Left), Rect::new(tx, r.y + 22.0, r.right() - tx, 22.0), TEXT, 1.0);
        cv.text(t(lang, Key::NothingPlayingHint), Font::text(12.0, 400, Align::Left), Rect::new(tx, r.y + 46.0, r.right() - tx, 20.0), SUB, 1.0);
        return;
    };

    let art = Rect::new(r.x, r.y + 4.0, 88.0, 88.0);
    match v.art {
        Some((brush, size)) => cv.image(brush, size, art, 14.0),
        None => {
            cv.fill_round(art, 14.0, SURFACE, 1.0);
            cv.glyph(G_MUSIC, 26.0, art, MINT, 1.0);
        }
    }
    let tx = art.right() + 16.0;
    let tw = r.right() - tx;
    cv.text(&m.title, Font::text(15.0, 600, Align::Left), Rect::new(tx, r.y + 2.0, tw, 22.0), TEXT, 1.0);
    let sub = if m.artist.is_empty() { m.app.clone() } else { format!("{} · {}", m.artist, m.app) };
    cv.text(&sub, Font::text(12.0, 400, Align::Left), Rect::new(tx, r.y + 24.0, tw, 18.0), SUB, 1.0);

    // Progress (only when the player reports a timeline).
    if m.duration_ms > 0 {
        let pos = m.position_at(v.now);
        let bar = Rect::new(tx, r.y + 50.0, tw, 4.0);
        cv.fill_round(bar, 2.0, SURFACE_HI, 1.0);
        let f = (pos as f32 / m.duration_ms as f32).clamp(0.0, 1.0);
        cv.fill_round(Rect::new(bar.x, bar.y, bar.w * f, bar.h), 2.0, TEXT, 1.0);
        let small = Font::text(10.5, 500, Align::Left);
        cv.text(&fmt_duration(pos), small, Rect::new(tx, r.y + 56.0, 60.0, 14.0), SUB, 1.0);
        let small_r = Font::text(10.5, 500, Align::Right);
        cv.text(&fmt_duration(m.duration_ms), small_r, Rect::new(r.right() - 60.0, r.y + 56.0, 60.0, 14.0), SUB, 1.0);
    }

    let cy = r.y + 72.0;
    let cx = tx + tw / 2.0;
    button(cv, hits, v, Rect::new(cx - 66.0, cy, 36.0, 32.0), G_PREV, 14.0, Action::MediaPrev);
    let play = Rect::new(cx - 20.0, cy - 2.0, 40.0, 36.0);
    button(cv, hits, v, play, if m.playing { G_PAUSE } else { G_PLAY }, 18.0, Action::MediaToggle);
    button(cv, hits, v, Rect::new(cx + 30.0, cy, 36.0, 32.0), G_NEXT, 14.0, Action::MediaNext);
}

fn draw_focus(cv: &mut Canvas, st: &AppState, v: &View, r: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let pomo = &st.pomodoro;
    let (label, col) = phase_style(pomo.phase, lang);
    let rem = pomo.remaining(v.now);

    cv.text(&fmt_duration(rem), Font::text(40.0, 600, Align::Left), Rect::new(r.x + 4.0, r.y, 200.0, 54.0), TEXT, 1.0);
    cv.fill_circle(r.x + 9.0, r.y + 64.0, 4.0, col, 1.0);
    cv.text(label, Font::text(12.5, 600, Align::Left), Rect::new(r.x + 20.0, r.y + 55.0, 100.0, 18.0), col, 1.0);
    let today = ailan_core::time::local_day(v.now, st.offset_min);
    let n = st.data.focus_count(today);
    cv.text(&format!("{n} {}", t(lang, Key::SessionsToday)), Font::text(12.0, 400, Align::Left), Rect::new(r.x + 120.0, r.y + 55.0, 140.0, 18.0), SUB, 1.0);
    let bar = Rect::new(r.x + 4.0, r.y + 84.0, 240.0, 4.0);
    cv.fill_round(bar, 2.0, SURFACE_HI, 1.0);
    cv.fill_round(Rect::new(bar.x, bar.y, bar.w * pomo.progress(v.now), bar.h), 2.0, col, 1.0);

    // Controls.
    let main = Rect::new(r.right() - 132.0, r.y + 6.0, 132.0, 38.0);
    let running = pomo.is_running();
    let hover = v.hover == Some(Action::TimerToggle);
    cv.fill_round(main, 19.0, col, if hover { 0.85 } else { 1.0 });
    let main_label = if running { t(lang, Key::Pause) } else { t(lang, Key::Start) };
    cv.glyph(if running { G_PAUSE } else { G_PLAY }, 12.0, Rect::new(main.x + 14.0, main.y, 18.0, main.h), BG, 1.0);
    cv.text(main_label, Font::text(13.0, 650, Align::Center), Rect::new(main.x + 22.0, main.y, main.w - 30.0, main.h), BG, 1.0);
    hits.push(Hit { rect: main, action: Action::TimerToggle });

    let small = |cv: &mut Canvas, hits: &mut Vec<Hit>, rect: Rect, g: char, text: &str, action: Action| {
        cv.fill_round(rect, rect.h / 2.0, if v.hover == Some(action) { SURFACE_HI } else { SURFACE }, 1.0);
        cv.glyph(g, 10.0, Rect::new(rect.x + 8.0, rect.y, 16.0, rect.h), SUB, 1.0);
        cv.text(text, Font::text(11.5, 500, Align::Center), Rect::new(rect.x + 18.0, rect.y, rect.w - 22.0, rect.h), TEXT, 1.0);
        hits.push(Hit { rect, action });
    };
    small(cv, hits, Rect::new(main.x, r.y + 54.0, 64.0, 30.0), G_RESET, t(lang, Key::Reset), Action::TimerReset);
    small(cv, hits, Rect::new(main.x + 68.0, r.y + 54.0, 64.0, 30.0), G_NEXT, t(lang, Key::Skip), Action::TimerSkip);
}

fn draw_todo(cv: &mut Canvas, st: &AppState, v: &View, r: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let field = Rect::new(r.x, r.y, r.w, 30.0);
    let editing = v.input.is_some();
    cv.fill_round(field, 10.0, if editing { SURFACE_HI } else { SURFACE }, 1.0);
    if editing {
        cv.stroke_round(field.inset(0.5, 0.5), 10.0, 1.0, MINT, 0.6);
    }
    cv.glyph(G_ADD, 11.0, Rect::new(field.x + 6.0, field.y, 22.0, field.h), if editing { MINT } else { SUB }, 1.0);
    let text_r = Rect::new(field.x + 32.0, field.y, field.w - 40.0, field.h);
    match v.input {
        Some(s) if !s.is_empty() => cv.text(&format!("{s}▏"), Font::text(12.5, 400, Align::Left), text_r, TEXT, 1.0),
        Some(_) => cv.text(t(lang, Key::AddTaskHint), Font::text(12.0, 400, Align::Left), text_r, SUB, 1.0),
        None => cv.text(t(lang, Key::AddTask), Font::text(12.5, 400, Align::Left), text_r, SUB, 1.0),
    }
    hits.push(Hit { rect: field, action: Action::TodoInput });

    let items = st.data.todos.sorted();
    let list = Rect::new(r.x, field.bottom() + 6.0, r.w, r.bottom() - field.bottom() - 6.0);
    if items.is_empty() {
        cv.text(t(lang, Key::NoTasks), Font::text(12.5, 400, Align::Center), list, SUB, 1.0);
        return;
    }
    hits.push(Hit { rect: list, action: Action::TodoScroll });
    let scroll = v.todo_scroll.min(items.len().saturating_sub(TODO_ROWS));
    let row_h = (list.h / TODO_ROWS as f32).floor();
    for (i, item) in items.iter().skip(scroll).take(TODO_ROWS).enumerate() {
        let row = Rect::new(list.x, list.y + i as f32 * row_h, list.w, row_h);
        let check = Rect::new(row.x, row.y, 26.0, row.h);
        let checked = item.done;
        cv.glyph(if checked { G_BOX_CHECKED } else { G_BOX }, 13.0, check, if checked { MINT } else { SUB }, 1.0);
        hits.push(Hit { rect: check, action: Action::TodoToggle(item.id) });

        let mut right = row.right();
        let del = Rect::new(right - 24.0, row.y, 24.0, row.h);
        let del_a = if v.hover == Some(Action::TodoDelete(item.id)) { 1.0 } else { 0.45 };
        cv.glyph(G_DELETE, 9.0, del, SUB, del_a);
        hits.push(Hit { rect: del, action: Action::TodoDelete(item.id) });
        right -= 28.0;
        if let Some(due) = item.due {
            let c = civil(due, st.offset_min);
            let col = if item.notified && !item.done { AMBER } else { SUB };
            cv.text(&format!("{:02}:{:02}", c.hour, c.minute), Font::text(11.0, 500, Align::Right), Rect::new(right - 44.0, row.y, 44.0, row.h), col, 1.0);
            right -= 50.0;
        }
        let text_r = Rect::new(check.right() + 4.0, row.y, right - check.right() - 4.0, row.h);
        cv.text(&item.text, Font::text(12.5, 400, Align::Left), text_r, if checked { SUB } else { TEXT }, 1.0);
        hits.push(Hit { rect: text_r, action: Action::TodoToggle(item.id) });
    }
    if items.len() > TODO_ROWS {
        // Scroll indicator.
        let track = Rect::new(list.right() + 6.0, list.y + 2.0, 2.0, list.h - 4.0);
        let frac = TODO_ROWS as f32 / items.len() as f32;
        let pos = scroll as f32 / items.len() as f32;
        cv.fill_round(Rect::new(track.x, track.y + track.h * pos, track.w, track.h * frac), 1.0, SUB, 0.6);
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
