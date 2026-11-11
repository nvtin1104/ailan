//! HubController: layout engine (Full sidebar ↔ Compact dock), springs, focus/typing,
//! pointer tracking and hit testing for the AiLan Personal Hub window.
//!
//! Rendering stays event-driven: the hub only asks for frames while [`HubController::animating`]
//! is true (a spring has not converged); otherwise it redraws on input or state changes only.

use std::time::Instant;

use ailan_core::anim::Spring;
use ailan_core::i18n::{t, Key};
use ailan_core::reminder::{day_bucket, DayBucket};
use ailan_core::settings::HubPage;
use ailan_core::time::fmt_duration;
use ailan_core::timer::Phase;
use ailan_core::AppState;

use crate::gfx::{Align, Canvas, Font, Rect, TextStyle};
use crate::hub::pages;
use crate::hub::widgets::*;
use crate::hub::{Field, HubAction, HubHit, PageCtx, TodoFilter};
use crate::ui::{G_CHECKMARK, G_HOME, G_MUSIC, G_NEXT, G_PAUSE, G_PLAY, G_PREV, G_SETTINGS, G_TIMER};

pub const COMPACT_WIDTH: i32 = 380;
pub const COMPACT_HEIGHT: i32 = 600;
pub const FULL_WIDTH: i32 = 1000;
pub const FULL_HEIGHT: i32 = 680;

pub const TITLE_H: f32 = 44.0;
pub const SIDEBAR_W: f32 = 220.0;
const NAV_Y0: f32 = 128.0;
const NAV_STEP: f32 = 42.0;
const NAV_H: f32 = 38.0;
const DOCK_H: f32 = 50.0;
const MAX_FIELD_CHARS: usize = 200;

pub const NAV_PAGES: [HubPage; 5] =
    [HubPage::Today, HubPage::Todos, HubPage::Reminders, HubPage::Settings, HubPage::Ai];

fn nav_index(p: HubPage) -> usize {
    NAV_PAGES.iter().position(|x| *x == p).unwrap_or(0)
}

/// Result of a key press routed to the hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    /// Not consumed (e.g. Esc with no focused field → caller closes the hub).
    Ignored,
    /// Consumed; redraw.
    Handled,
    /// Consumed and produced an action.
    Action(HubAction),
}

pub struct HubController {
    pub active_page: HubPage,
    pub is_compact: bool,
    pub hover: Option<HubAction>,
    pub mouse: Option<(f32, f32)>,
    pub focus: Option<Field>,
    pub capture: String,
    pub command: String,
    pub search: String,
    pub copilot: String,
    pub todo_filter: TodoFilter,
    /// Collapsed reminder buckets (Overdue, Today, Tomorrow, Week, Later).
    pub collapsed: [bool; 5],
    pub hits: Vec<HubHit>,
    /// Where the last activating input happened (hub DIP): the pressed control, or the
    /// field that was submitted with Enter. Flights into the island start here.
    pub last_press: Option<(f32, f32)>,
    spots: Vec<Rect>,
    over_spot: bool,
    nav: Indicator,
    filter_ind: Indicator,
    /// Page entrance fade 0..100.
    fade: Spring,
    scroll: Spring,
    scroll_max: f32,
    pops: Vec<CheckPop>,
    reduce_motion: bool,
    last_step: Instant,
}

impl Default for HubController {
    fn default() -> Self {
        Self::new()
    }
}

impl HubController {
    pub fn new() -> Self {
        Self {
            active_page: HubPage::Today,
            is_compact: false,
            hover: None,
            mouse: None,
            focus: None,
            capture: String::new(),
            command: String::new(),
            search: String::new(),
            copilot: String::new(),
            todo_filter: TodoFilter::All,
            collapsed: [false; 5],
            hits: Vec::new(),
            last_press: None,
            spots: Vec::new(),
            over_spot: false,
            nav: Indicator::new(0),
            filter_ind: Indicator::new(0),
            fade: Spring::new(100.0).with(260.0, 32.0),
            scroll: Spring::new(0.0).with(520.0, 46.0),
            scroll_max: 0.0,
            pops: Vec::new(),
            reduce_motion: false,
            last_step: Instant::now(),
        }
    }

    pub fn target_size(&self) -> (i32, i32) {
        if self.is_compact {
            (COMPACT_WIDTH, COMPACT_HEIGHT)
        } else {
            (FULL_WIDTH, FULL_HEIGHT)
        }
    }

    pub fn toggle_mode(&mut self) -> (i32, i32) {
        self.is_compact = !self.is_compact;
        self.restart_fade();
        self.scroll_to(0.0, true);
        self.target_size()
    }

    pub fn select_page(&mut self, page: HubPage) {
        if page != self.active_page {
            self.restart_fade();
        }
        self.active_page = page;
        self.nav.go(nav_index(page));
        self.scroll_to(0.0, true);
        if self.focus != Some(Field::Command) {
            self.focus = None;
        }
    }

    pub fn set_filter(&mut self, f: TodoFilter) {
        self.todo_filter = f;
        self.filter_ind.go(f.index());
        self.scroll_to(0.0, true);
    }

    /// Kick off the tactile checkbox animation after a todo became done.
    pub fn note_toggled(&mut self, id: u64, now_done: bool) {
        self.pops.retain(|p| p.id != id);
        if now_done && !self.reduce_motion {
            self.pops.push(CheckPop::new(id));
        }
    }

    fn restart_fade(&mut self) {
        if !self.reduce_motion {
            self.fade.value = 0.0;
            self.fade.velocity = 0.0;
            self.fade.set_target(100.0);
        }
    }

    fn scroll_to(&mut self, y: f32, snap: bool) {
        self.scroll.set_target(y.clamp(0.0, self.scroll_max.max(0.0)));
        if snap || self.reduce_motion {
            self.scroll.snap();
        }
    }

    pub fn on_wheel(&mut self, delta: i16) -> bool {
        if self.scroll_max <= 0.5 {
            return false;
        }
        let before = self.scroll.target;
        self.scroll_to(before - delta as f32 / 120.0 * 56.0, false);
        (self.scroll.target - before).abs() > 0.01
    }

    /// True while any spring is still moving (the caller keeps presenting frames).
    pub fn animating(&self) -> bool {
        !(self.nav.is_settled()
            && self.filter_ind.is_settled()
            && self.fade.is_settled()
            && self.scroll.is_settled()
            && self.pops.is_empty())
    }

    /// Advance all springs by the wall-clock time since the previous step.
    pub fn advance(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_step).as_secs_f32();
        self.last_step = now;
        let dt = if elapsed > 0.1 { 1.0 / 60.0 } else { elapsed.max(1.0 / 240.0) };
        if self.reduce_motion {
            self.nav.snap();
            self.filter_ind.snap();
            self.fade.snap();
            self.scroll.snap();
            self.pops.clear();
            return;
        }
        self.nav.step(dt);
        self.filter_ind.step(dt);
        self.fade.step(dt);
        self.scroll.step(dt);
        self.pops.retain_mut(|p| !p.step(dt));
    }

    // ---- pointer -------------------------------------------------------------------------

    fn hit_at(&self, x: f32, y: f32) -> Option<HubAction> {
        self.hits.iter().find(|h| h.rect.contains(x, y)).map(|h| h.action)
    }

    /// Titlebar drag region: top band minus every interactive element.
    pub fn is_caption(&self, x: f32, y: f32) -> bool {
        y < TITLE_H && self.hit_at(x, y).is_none()
    }

    /// Returns true when a redraw is needed (hover changed, or spotlight moved).
    pub fn on_mouse_move(&mut self, x: f32, y: f32) -> bool {
        self.mouse = Some((x, y));
        let hover = self.hit_at(x, y);
        let over_spot = self.spots.iter().any(|r| r.contains(x, y));
        let redraw = hover != self.hover || over_spot || self.over_spot;
        self.hover = hover;
        self.over_spot = over_spot;
        redraw
    }

    pub fn on_mouse_leave(&mut self) {
        self.mouse = None;
        self.hover = None;
        self.over_spot = false;
    }

    pub fn on_mouse_down(&mut self, x: f32, y: f32) -> Option<HubAction> {
        let act = self.hit_at(x, y);
        self.last_press = Some((x, y));
        let keeps_focus = matches!(act, Some(HubAction::FocusField(_) | HubAction::SubmitField(_) | HubAction::ClearSearch | HubAction::UseExample(_)));
        if !keeps_focus {
            self.focus = None;
        }
        act
    }

    // ---- keyboard ------------------------------------------------------------------------

    pub fn field_mut(&mut self, f: Field) -> &mut String {
        match f {
            Field::Capture => &mut self.capture,
            Field::Command => &mut self.command,
            Field::Search => &mut self.search,
            Field::Copilot => &mut self.copilot,
        }
    }

    /// Take (and clear) a field's text.
    pub fn take_field(&mut self, f: Field) -> String {
        std::mem::take(self.field_mut(f))
    }

    pub fn on_char(&mut self, c: char) -> bool {
        let Some(f) = self.focus else { return false };
        if c.is_control() {
            return false;
        }
        let buf = self.field_mut(f);
        if buf.chars().count() >= MAX_FIELD_CHARS {
            return false;
        }
        buf.push(c);
        if f == Field::Search {
            self.scroll_to(0.0, true);
        }
        true
    }

    pub fn on_key_down(&mut self, vk: u32, ctrl: bool) -> KeyOutcome {
        const VK_BACK: u32 = 0x08;
        const VK_RETURN: u32 = 0x0D;
        const VK_ESCAPE: u32 = 0x1B;
        const VK_K: u32 = 0x4B;
        if ctrl && vk == VK_K {
            return KeyOutcome::Action(HubAction::FocusField(Field::Command));
        }
        let Some(f) = self.focus else { return KeyOutcome::Ignored };
        match vk {
            VK_BACK => {
                let buf = self.field_mut(f);
                if ctrl {
                    let trimmed = buf.trim_end().len();
                    let cut = buf[..trimmed].rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0);
                    buf.truncate(cut);
                } else {
                    buf.pop();
                }
                KeyOutcome::Handled
            }
            VK_ESCAPE => {
                self.field_mut(f).clear();
                self.focus = None;
                KeyOutcome::Handled
            }
            VK_RETURN => {
                self.last_press = self
                    .hits
                    .iter()
                    .find(|h| h.action == HubAction::FocusField(f))
                    .map(|h| h.rect.center());
                KeyOutcome::Action(HubAction::SubmitField(f))
            }
            _ => KeyOutcome::Handled,
        }
    }

    /// `/today`, `/việc`, `/nhắc`… → page to jump to.
    pub fn command_page(cmd: &str) -> Option<HubPage> {
        let c = cmd.trim().strip_prefix('/')?.trim().to_lowercase();
        if c.is_empty() {
            return None;
        }
        let table: [(HubPage, &[&str]); 5] = [
            (HubPage::Today, &["today", "hôm nay", "home", "nay"]),
            (HubPage::Todos, &["todos", "tasks", "việc", "viec"]),
            (HubPage::Reminders, &["reminders", "nhắc", "nhac", "lịch"]),
            (HubPage::Settings, &["settings", "cài đặt", "cai dat", "prefs"]),
            (HubPage::Ai, &["ai", "copilot", "brief"]),
        ];
        table.iter().find(|(_, words)| words.iter().any(|w| w.starts_with(&c))).map(|(p, _)| *p)
    }

    // ---- drawing -------------------------------------------------------------------------

    pub fn draw(&mut self, cv: &mut Canvas, st: &AppState, now: i64, w: f32, h: f32) {
        self.reduce_motion = st.settings.reduce_motion;
        self.hits.clear();
        self.spots.clear();
        draw_window_backdrop(cv, Rect::new(0.0, 0.0, w, h));
        let counts = NavCounts::of(st, now);
        if self.is_compact {
            self.draw_compact(cv, st, now, w, h, &counts);
        } else {
            self.draw_full(cv, st, now, w, h, &counts);
        }
    }

    fn hover_t(&self, a: HubAction) -> f32 {
        if self.hover == Some(a) {
            1.0
        } else {
            0.0
        }
    }

    fn push_hit(&mut self, rect: Rect, action: HubAction) {
        self.hits.push(HubHit { rect, action });
    }

    /// Command bar + window controls (both layouts).
    fn draw_titlebar(&mut self, cv: &mut Canvas, st: &AppState, w: f32, bar: Rect) {
        let lang = st.lang();
        let focused = self.focus == Some(Field::Command);
        let act = HubAction::FocusField(Field::Command);
        let hv = self.hover_t(act);
        let hint = if self.is_compact { "Ctrl K" } else { t(lang, Key::CommandHint) };
        let reserve = if focused || !self.command.is_empty() { 22.0 } else if self.is_compact { 0.0 } else { 56.0 };
        draw_text_input(cv, bar, &self.command, hint, focused, hv, Font::text(12.5, 450, Align::Left), Some(G_SEARCH), reserve);
        if !focused && self.command.is_empty() && !self.is_compact {
            let kbd = "Ctrl K";
            let kw = cv.text_width(kbd, Font::text(10.5, 600, Align::Center), TextStyle::default()) + 12.0;
            let kr = Rect::new(bar.right() - kw - 6.0, bar.y + 5.0, kw, bar.h - 10.0);
            cv.fill_round(kr, 5.0, WHITE, 0.06);
            cv.hairline_round(kr, 5.0, WHITE, 0.12);
            cv.text(kbd, Font::text(10.5, 600, Align::Center), kr, TEXT_BODY, 0.9);
        } else if focused && !self.command.is_empty() {
            let rr = Rect::new(bar.right() - 26.0, bar.y + 4.0, 20.0, bar.h - 8.0);
            cv.glyph(G_RETURN, 10.0, rr, ACCENT_PRIMARY_HI, 0.9);
        }
        self.push_hit(bar, act);

        let rects = window_control_rects(w, 0.0);
        let acts = [HubAction::Minimize, HubAction::ToggleMode, HubAction::Close];
        let hover_idx = acts.iter().position(|a| self.hover == Some(*a));
        draw_window_controls(cv, rects, hover_idx, self.is_compact);
        for (r, a) in rects.iter().zip(acts) {
            self.push_hit(*r, a);
        }
    }

    fn draw_full(&mut self, cv: &mut Canvas, st: &AppState, now: i64, w: f32, h: f32, counts: &NavCounts) {
        let lang = st.lang();

        // Sidebar: square fill (DWM clips the window's left corners), then a fading divider.
        cv.fill_rect(Rect::new(0.0, 0.0, SIDEBAR_W, h), BG_SIDEBAR, BG_SIDEBAR_ALPHA);
        draw_divider_v(cv, SIDEBAR_W - 1.0, 0.0, h);

        // Brand
        let tile = Rect::new(16.0, 10.0, 28.0, 28.0);
        cv.fill_circle_radial(tile.x + 14.0, tile.y + 14.0, 30.0, GLOW_ACCENT, 0.5);
        cv.fill_round_linear(tile, RADIUS_BUTTON, ACCENT_FILL, (tile.x, tile.y), (tile.right(), tile.bottom()), 1.0);
        cv.hairline_round_linear(tile, RADIUS_BUTTON, RIM_HOT, (tile.x, tile.y), (tile.x, tile.bottom()), 1.0);
        cv.text("✦", Font::text(14.0, 400, Align::Center), tile, WHITE, 1.0);
        cv.text("AiLan", Font::text(15.0, 700, Align::Left), Rect::new(52.0, 9.0, 140.0, 18.0), TEXT_HERO, 1.0);
        cv.text("Personal Hub", Font::text(11.0, 450, Align::Left), Rect::new(52.0, 26.0, 140.0, 14.0), TEXT_SECTION, 1.0);

        // Island status (read-only mirror of what the notch shows)
        let isl = Rect::new(12.0, 54.0, SIDEBAR_W - 24.0, 60.0);
        self.spots.push(isl);
        draw_specular_card(cv, isl, RADIUS_CARD, 0.0, self.mouse);
        draw_island_mini(cv, st, now, isl);

        // Navigation with sliding indicator
        let ind_y = NAV_Y0 + self.nav.pos() * NAV_STEP;
        let ind = Rect::new(12.0, ind_y, SIDEBAR_W - 24.0, NAV_H);
        cv.fill_round_linear(ind, RADIUS_BUTTON, ACCENT_TINT, (ind.x, ind.y), (ind.right(), ind.bottom()), 1.0);
        cv.hairline_round_linear(ind, RADIUS_BUTTON, RIM_ACCENT, (ind.x, ind.y), (ind.x, ind.bottom()), 1.0);
        let bar = Rect::new(ind.x + 5.0, ind.y + 11.0, 3.0, 16.0);
        cv.fill_round_linear(bar, 1.5, ACCENT_FILL, (bar.x, bar.y), (bar.x, bar.bottom()), 1.0);

        for (i, page) in NAV_PAGES.iter().enumerate() {
            let nr = Rect::new(12.0, NAV_Y0 + i as f32 * NAV_STEP, SIDEBAR_W - 24.0, NAV_H);
            let act = HubAction::SelectPage(*page);
            let near = 1.0 - (self.nav.pos() - i as f32).abs().min(1.0);
            let hv = self.hover_t(act);
            if hv > 0.0 && self.active_page != *page {
                cv.fill_round(nr, RADIUS_BUTTON, WHITE, 0.045);
            }
            let fg = mix(if hv > 0.0 { TEXT_TITLE } else { TEXT_BODY }, TEXT_HERO, near);
            let icon_r = Rect::new(nr.x + 14.0, nr.y, 20.0, nr.h);
            match nav_glyph(*page) {
                Some(g) => cv.glyph(g, 14.0, icon_r, mix(fg, ACCENT_PRIMARY_HI, near * 0.6), 1.0),
                None => cv.text("✦", Font::text(14.0, 400, Align::Center), icon_r, mix(fg, ACCENT_PRIMARY_HI, near * 0.6), 1.0),
            }
            cv.text(
                nav_label(lang, *page),
                Font::text(13.0, if near > 0.5 { 600 } else { 500 }, Align::Left),
                Rect::new(nr.x + 42.0, nr.y, nr.w - 90.0, nr.h),
                fg,
                1.0,
            );
            let (n, rgb) = counts.for_page(*page);
            draw_count_badge(cv, nr.right() - 10.0, nr.y + nr.h * 0.5, n, rgb);
            self.push_hit(nr, act);
        }

        // Mini Pomodoro & Media dock pinned to the sidebar bottom
        let dock = Rect::new(12.0, h - 12.0 - 132.0, SIDEBAR_W - 24.0, 132.0);
        self.spots.push(dock);
        draw_specular_card(cv, dock, RADIUS_CARD, 0.0, self.mouse);
        self.draw_mini_dock(cv, st, now, dock);

        // Titlebar over the content column; command bar centred on the window.
        let bar_w = 380.0f32.min(w - SIDEBAR_W - 160.0);
        let bar = Rect::new(((w - bar_w) * 0.5).max(SIDEBAR_W + 16.0), 8.0, bar_w, 28.0);
        self.draw_titlebar(cv, st, w, bar);

        let content = Rect::new(SIDEBAR_W + 28.0, TITLE_H + 12.0, w - SIDEBAR_W - 56.0, h - TITLE_H - 12.0 - 20.0);
        self.render_page(cv, st, now, content);
    }

    fn draw_compact(&mut self, cv: &mut Canvas, st: &AppState, now: i64, w: f32, h: f32, counts: &NavCounts) {
        let tile = Rect::new(12.0, 10.0, 24.0, 24.0);
        cv.fill_round_linear(tile, RADIUS_BUTTON, ACCENT_FILL, (tile.x, tile.y), (tile.right(), tile.bottom()), 1.0);
        cv.text("✦", Font::text(12.0, 400, Align::Center), tile, WHITE, 1.0);
        cv.text("AiLan", Font::text(13.5, 700, Align::Left), Rect::new(42.0, 10.0, 60.0, 24.0), TEXT_HERO, 1.0);

        let bar = Rect::new(100.0, 8.0, (w - 100.0 - 140.0).max(60.0), 28.0);
        self.draw_titlebar(cv, st, w, bar);

        // Floating glass dock spanning the window width (minus the 16px gutters).
        let n = NAV_PAGES.len() as f32;
        let pad = 6.0;
        let dw = w - 32.0;
        let item_w = (dw - pad * 2.0) / n;
        let dock = Rect::new(16.0, h - 14.0 - DOCK_H, dw, DOCK_H);
        let content = Rect::new(16.0, TITLE_H + 6.0, w - 32.0, dock.y - 12.0 - (TITLE_H + 6.0));
        self.render_page(cv, st, now, content);

        let drad = DOCK_H * 0.5;
        cv.fill_circle_radial(dock.x + dock.w * 0.5, dock.bottom(), dock.w * 0.6, GLOW_ACCENT, 0.18);
        draw_specular_rounded_rect(cv, dock, drad, Surface::Elevated, 0.0, self.mouse);
        let pill_rad = concentric(drad, pad);
        let pill = Rect::new(dock.x + pad + self.nav.pos() * item_w, dock.y + pad, item_w, DOCK_H - pad * 2.0);
        cv.fill_round_linear(pill, pill_rad, ACCENT_TINT, (pill.x, pill.y), (pill.right(), pill.bottom()), 1.0);
        cv.hairline_round_linear(pill, pill_rad, RIM_ACCENT, (pill.x, pill.y), (pill.x, pill.bottom()), 1.0);
        for (i, page) in NAV_PAGES.iter().enumerate() {
            let ir = Rect::new(dock.x + pad + i as f32 * item_w, dock.y + pad, item_w, DOCK_H - pad * 2.0);
            let act = HubAction::SelectPage(*page);
            let near = 1.0 - (self.nav.pos() - i as f32).abs().min(1.0);
            let hv = self.hover_t(act);
            let fg = mix(if hv > 0.0 { TEXT_TITLE } else { TEXT_BODY }, TEXT_HERO, near);
            // Icon-only dock: the sliding pill and icon tint carry the active state.
            let icon_col = mix(fg, ACCENT_PRIMARY_HI, near * 0.5);
            match nav_glyph(*page) {
                Some(g) => cv.glyph(g, 17.0, ir, icon_col, 1.0),
                None => cv.text("✦", Font::text(17.0, 400, Align::Center), ir, icon_col, 1.0),
            }
            let (cnt, rgb) = counts.for_page(*page);
            if cnt > 0 {
                let (dx, dy) = (ir.x + ir.w * 0.5 + 10.0, ir.y + ir.h * 0.5 - 9.0);
                cv.fill_circle(dx, dy, 3.5, rgb, 1.0);
                cv.stroke_circle(dx, dy, 4.5, 1.5, BG_SURFACE_ELEVATED, 1.0);
            }
            self.push_hit(ir, act);
        }
    }

    fn draw_mini_dock(&mut self, cv: &mut Canvas, st: &AppState, now: i64, r: Rect) {
        let lang = st.lang();
        let p = &st.pomodoro;
        let (phase_lbl, tint) = phase_label(lang, p.phase);
        draw_section_header(cv, Rect::new(r.x + 14.0, r.y + 10.0, r.w - 28.0, 16.0), phase_lbl);
        let time = fmt_duration(p.remaining(now));
        cv.text_styled(&time, Font::text(22.0, 700, Align::Left), Rect::new(r.x + 14.0, r.y + 26.0, 100.0, 28.0), TEXT_HERO, 1.0, TextStyle::TABULAR);

        let play = HubAction::PomoToggle;
        let (pcx, pcy) = (r.right() - 30.0, r.y + 38.0);
        draw_round_button(cv, pcx, pcy, 16.0, if p.is_running() { G_PAUSE } else { G_PLAY }, self.hover_t(play), true);
        self.push_hit(Rect::new(pcx - 16.0, pcy - 16.0, 32.0, 32.0), play);
        let skip = HubAction::PomoSkip;
        let sr = Rect::new(pcx - 50.0, pcy - 12.0, 24.0, 24.0);
        draw_icon_button(cv, sr, G_SKIP, 10.0, self.hover_t(skip), false, ACCENT_PRIMARY);
        self.push_hit(sr, skip);

        let track = Rect::new(r.x + 14.0, r.y + 62.0, r.w - 28.0, 3.0);
        cv.fill_round(track, 1.5, WHITE, 0.07);
        let prog = p.progress(now).clamp(0.0, 1.0);
        if prog > 0.0 {
            let f = Rect::new(track.x, track.y, track.w * prog, track.h);
            cv.fill_round_linear(f, 1.5, ACCENT_FILL, (f.x, f.y), (track.right(), f.y), 1.0);
            let _ = tint;
        }
        draw_divider_h(cv, r.x + 10.0, r.y + 76.0, r.w - 20.0);

        // Media row
        let my = r.y + 84.0;
        match &st.media {
            Some(m) if !m.title.is_empty() => {
                cv.glyph(G_MUSIC, 12.0, Rect::new(r.x + 10.0, my, 22.0, 38.0), if m.playing { ACCENT_PRIMARY_HI } else { TEXT_BODY }, 1.0);
                cv.text(&m.title, Font::text(12.0, 600, Align::Left), Rect::new(r.x + 36.0, my + 3.0, r.w - 120.0, 16.0), TEXT_TITLE, 1.0);
                cv.text(&m.artist, Font::text(10.5, 400, Align::Left), Rect::new(r.x + 36.0, my + 19.0, r.w - 120.0, 14.0), TEXT_SECTION, 1.0);
                let acts = [(HubAction::MediaPrev, G_PREV), (HubAction::MediaToggle, if m.playing { G_PAUSE } else { G_PLAY }), (HubAction::MediaNext, G_NEXT)];
                for (i, (a, g)) in acts.iter().enumerate() {
                    let br = Rect::new(r.right() - 82.0 + i as f32 * 24.0, my + 7.0, 24.0, 24.0);
                    draw_icon_button(cv, br, *g, 10.0, self.hover_t(*a), false, ACCENT_PRIMARY);
                    self.push_hit(br, *a);
                }
            }
            _ => {
                cv.glyph(G_MUSIC, 12.0, Rect::new(r.x + 10.0, my, 22.0, 38.0), TEXT_SECTION, 1.0);
                cv.text(t(lang, Key::NothingPlaying), Font::text(12.0, 450, Align::Left), Rect::new(r.x + 36.0, my, r.w - 46.0, 38.0), TEXT_SECTION, 1.0);
            }
        }
    }

    fn render_page(&mut self, cv: &mut Canvas, st: &AppState, now: i64, r: Rect) {
        let fade = (self.fade.value / 100.0).clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - fade) * (1.0 - fade);
        let pr = Rect::new(r.x, r.y + (1.0 - ease) * 10.0, r.w, r.h);
        let prev_alpha = cv.alpha;
        cv.alpha = prev_alpha * ease;
        let first_hit = self.hits.len();
        let scroll = self.scroll.value;

        // Allow ring glows to bleed a little sideways; vertically clip under the titlebar.
        // Scrolled content fades out at the top edge; in Compact it also fades out above the
        // dock (opacity mask, so the acrylic backdrop is never tinted).
        let view = Rect::new(r.x - 16.0, r.y - 8.0, r.w + 32.0, r.h + 16.0);
        cv.push_clip(view);
        let fade_top = scroll > 0.5;
        let fade_bottom = self.is_compact;
        let layered = (fade_top || fade_bottom) && {
            let span = view.h;
            let edge = 22.0 / span;
            let top = if fade_top { (0.0, edge) } else { (0.0, 0.0) };
            let bot = if fade_bottom { (1.0 - edge, 1.0) } else { (1.0, 1.0) };
            let g = crate::gfx::Gradient::new(&[
                (top.0, WHITE, if fade_top { 0.0 } else { 1.0 }),
                (top.1, WHITE, 1.0),
                (bot.0, WHITE, 1.0),
                (bot.1, WHITE, if fade_bottom { 0.0 } else { 1.0 }),
            ]);
            cv.push_fade_layer(view, g, (view.x, view.y), (view.x, view.bottom()))
        };
        let content_h = {
            let mut ctx = PageCtx {
                st,
                now,
                hover: self.hover,
                mouse: self.mouse,
                focus: self.focus,
                capture: &self.capture,
                search: &self.search,
                copilot: &self.copilot,
                filter: self.todo_filter,
                filter_pos: self.filter_ind.pos(),
                scroll,
                collapsed: self.collapsed,
                compact: self.is_compact,
                pops: &self.pops,
                hits: &mut self.hits,
                spots: &mut self.spots,
                content_h: None,
            };
            match self.active_page {
                HubPage::Today => pages::today::draw(cv, &mut ctx, pr),
                HubPage::Todos => pages::todos::draw(cv, &mut ctx, pr),
                HubPage::Reminders => pages::reminders::draw(cv, &mut ctx, pr),
                HubPage::Settings => pages::settings::draw(cv, st, pr, now, scroll, self.hover, ctx.hits),
                HubPage::Ai => pages::ai::draw(cv, &mut ctx, pr),
            }
            ctx.content_h
        };
        if layered {
            cv.pop_layer();
        }
        cv.pop_clip();
        cv.alpha = prev_alpha;

        // Scroll extent: explicit from the page, else inferred from the lowest hit target.
        let max_bottom = self.hits[first_hit..].iter().map(|h| h.rect.bottom()).fold(r.y, f32::max);
        // Extra tail room in Compact so the last item can scroll clear of the dock fade.
        let tail = if self.is_compact { 18.0 } else { 0.0 };
        let full_h = content_h.unwrap_or(max_bottom + scroll - r.y + 8.0) + tail;
        self.scroll_max = (full_h - r.h).max(0.0);
        if self.scroll.target > self.scroll_max {
            self.scroll_to(self.scroll_max, false);
        }

        // Page hits never leak outside the content viewport (e.g. under the titlebar/dock).
        let mut i = first_hit;
        while i < self.hits.len() {
            match self.hits[i].rect.intersect(&r) {
                Some(cr) => {
                    self.hits[i].rect = cr;
                    i += 1;
                }
                None => {
                    self.hits.remove(i);
                }
            }
        }
        self.spots.retain(|s| s.intersect(&r).is_some() || s.x < SIDEBAR_W);
    }
}

/// Mini Dynamic Island mirror: what the notch is showing right now.
fn draw_island_mini(cv: &mut Canvas, st: &AppState, now: i64, card: Rect) {
    let lang = st.lang();
    draw_section_header(cv, Rect::new(card.x + 14.0, card.y + 8.0, card.w - 40.0, 16.0), "Dynamic Island");
    let pill = Rect::new(card.x + 12.0, card.y + 28.0, card.w - 24.0, 22.0);
    cv.fill_round(pill, 11.0, 0x000000, 1.0);
    cv.hairline_round(pill, 11.0, WHITE, 0.14);
    let p = &st.pomodoro;
    let live;
    if p.is_active() {
        live = true;
        let (_, tint) = phase_label(lang, p.phase);
        cv.glyph(G_TIMER, 10.0, Rect::new(pill.x + 6.0, pill.y, 18.0, pill.h), tint, 1.0);
        let time = fmt_duration(p.remaining(now));
        cv.text_styled(&time, Font::text(11.0, 650, Align::Left), Rect::new(pill.x + 26.0, pill.y, 60.0, pill.h), TEXT_HERO, 1.0, TextStyle::TABULAR);
        draw_progress_ring(cv, pill.right() - 12.0, pill.y + 11.0, 5.5, 2.0, p.progress(now), tint, false);
    } else if let Some(m) = st.media.as_ref().filter(|m| m.playing) {
        live = true;
        cv.glyph(G_MUSIC, 10.0, Rect::new(pill.x + 6.0, pill.y, 18.0, pill.h), ACCENT_PRIMARY_HI, 1.0);
        cv.text(&m.title, Font::text(11.0, 600, Align::Left), Rect::new(pill.x + 26.0, pill.y, pill.w - 54.0, pill.h), TEXT_HERO, 1.0);
        for (i, bh) in [6.0f32, 10.0, 7.0].iter().enumerate() {
            let bx = pill.right() - 20.0 + i as f32 * 4.0;
            cv.fill_round(Rect::new(bx, pill.y + 11.0 - bh * 0.5, 2.0, *bh), 1.0, ACCENT_PRIMARY_HI, 0.9);
        }
    } else {
        live = false;
        cv.fill_circle(pill.x + pill.w * 0.5, pill.y + 11.0, 2.5, TEXT_SECTION, 0.9);
        cv.text(t(lang, Key::IdleLabel), Font::text(10.5, 500, Align::Left), Rect::new(pill.x + pill.w * 0.5 + 8.0, pill.y, 80.0, pill.h), TEXT_SECTION, 1.0);
    }
    let (dot, a) = if live { (ACCENT_MINT, 1.0) } else { (TEXT_SECTION, 0.7) };
    if live {
        cv.fill_circle_radial(card.right() - 16.0, card.y + 16.0, 8.0, GLOW_MINT, 1.0);
    }
    cv.fill_circle(card.right() - 16.0, card.y + 16.0, 3.0, dot, a);
}

fn phase_label(lang: ailan_core::i18n::Lang, phase: Phase) -> (&'static str, u32) {
    match phase {
        Phase::Focus => (t(lang, Key::Focus), ACCENT_PRIMARY_HI),
        Phase::ShortBreak => (t(lang, Key::ShortBreak), ACCENT_MINT),
        Phase::LongBreak => (t(lang, Key::LongBreak), ACCENT_MINT),
    }
}

fn nav_glyph(p: HubPage) -> Option<char> {
    match p {
        HubPage::Today => Some(G_HOME),
        HubPage::Todos => Some(G_CHECKMARK),
        HubPage::Reminders => Some(G_TIMER),
        HubPage::Settings => Some(G_SETTINGS),
        HubPage::Ai => None,
    }
}

fn nav_label(lang: ailan_core::i18n::Lang, p: HubPage) -> &'static str {
    match p {
        HubPage::Today => t(lang, Key::NavToday),
        HubPage::Todos => t(lang, Key::NavTodos),
        HubPage::Reminders => t(lang, Key::NavReminders),
        HubPage::Settings => t(lang, Key::NavSettings),
        HubPage::Ai => t(lang, Key::NavAi),
    }
}

/// Counter badges for the navigation.
struct NavCounts {
    today: usize,
    pending: usize,
    overdue: usize,
    due_today: usize,
}

impl NavCounts {
    fn of(st: &AppState, now: i64) -> Self {
        let today = st.data.todos.today_items(now, st.offset_min).iter().filter(|t| !t.done).count();
        let pending = st.data.todos.pending_count();
        let (mut overdue, mut due_today) = (0, 0);
        for t in st.data.todos.items.iter().filter(|t| !t.done) {
            if let Some(d) = t.snoozed_until.or(t.due) {
                match day_bucket(d, now, st.offset_min) {
                    DayBucket::Overdue => overdue += 1,
                    DayBucket::Today => due_today += 1,
                    _ => {}
                }
            }
        }
        Self { today, pending, overdue, due_today }
    }

    fn for_page(&self, p: HubPage) -> (usize, u32) {
        match p {
            HubPage::Today => (self.today, ACCENT_PRIMARY_HI),
            HubPage::Todos => (self.pending, TEXT_BODY),
            HubPage::Reminders if self.overdue > 0 => (self.overdue, ACCENT_ROSE),
            HubPage::Reminders => (self.due_today, ACCENT_AMBER),
            _ => (0, TEXT_BODY),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_jumps_to_pages() {
        assert_eq!(HubController::command_page("/today"), Some(HubPage::Today));
        assert_eq!(HubController::command_page("/nhắc"), Some(HubPage::Reminders));
        assert_eq!(HubController::command_page("/set"), Some(HubPage::Settings));
        assert_eq!(HubController::command_page("buy milk"), None);
        assert_eq!(HubController::command_page("/"), None);
    }

    #[test]
    fn typing_and_keys() {
        let mut c = HubController::new();
        assert!(!c.on_char('x'), "no field focused");
        c.focus = Some(Field::Capture);
        for ch in "call mom @9h".chars() {
            assert!(c.on_char(ch));
        }
        assert_eq!(c.capture, "call mom @9h");
        assert_eq!(c.on_key_down(0x08, true), KeyOutcome::Handled);
        assert_eq!(c.capture, "call mom ");
        assert_eq!(c.on_key_down(0x0D, false), KeyOutcome::Action(HubAction::SubmitField(Field::Capture)));
        assert_eq!(c.on_key_down(0x1B, false), KeyOutcome::Handled);
        assert!(c.capture.is_empty() && c.focus.is_none());
        assert_eq!(c.on_key_down(0x1B, false), KeyOutcome::Ignored);
        assert_eq!(c.on_key_down(0x4B, true), KeyOutcome::Action(HubAction::FocusField(Field::Command)));
    }

    #[test]
    fn page_switch_animates_then_settles() {
        let mut c = HubController::new();
        assert!(!c.animating());
        c.select_page(HubPage::Reminders);
        assert!(c.animating());
        for _ in 0..600 {
            c.last_step = Instant::now() - std::time::Duration::from_millis(16);
            c.advance();
            if !c.animating() {
                break;
            }
        }
        assert!(!c.animating(), "springs must converge so the hub returns to 0% idle CPU");
        assert_eq!(c.nav.pos(), 2.0);
    }
}
