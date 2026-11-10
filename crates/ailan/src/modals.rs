//! Unified Activity Management & Settings Hub App (`ModalKind::Settings`).
//! Designed with a pure Monochrome Black & White palette, sidebar navigation,
//! pill toggle switches (instead of checkboxes), and cursor-only hover feedback.

use ailan_core::i18n::{t, Key, Lang};
use ailan_core::settings::{NotchMode, Tab, WaveColor, WaveStyle};
use ailan_core::time::fmt_duration;
use ailan_core::AppState;

use crate::gfx::{Align, Canvas, Font, Rect};
use crate::platform::autostart;
use crate::ui::{
    CursorIcon, BG, G_ADD, G_CHECKMARK, G_DELETE, G_DOWN, G_HOME, G_MUSIC, G_NEXT, G_PAUSE,
    G_PIN, G_PLAY, G_PREV, G_RESET, G_SETTINGS, G_TIMER, G_UP, PANEL_BG, SILVER, SUB, SURFACE,
    TEXT_SOFT, WHITE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModalKind {
    #[default]
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsPage {
    #[default]
    ActivitiesAndPriority,
    GeneralAndMode,
    MinimalAndCollapsed,
    HoverAndAudio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalAction {
    Close,
    DragHeader,
    SelectPage(SettingsPage),
    // Activity Management & Priority
    TogglePin(Tab),
    PriorityUp(Tab),
    PriorityDown(Tab),
    ToggleSmartPeek,
    QuickFocusPreset(u32),
    FocusToggle,
    FocusReset,
    MediaPrev,
    MediaToggle,
    MediaNext,
    FocusTodoInput,
    SubmitTodoInput,
    TodoToggle(u64),
    TodoDelete(u64),
    // General & Mode
    SetLang(Lang),
    SetMode(NotchMode),
    ToggleAutostart,
    ToggleReduceMotion,
    // Minimal UI
    ToggleMinimalHide,
    ToggleMinimalDot,
    ToggleMinimalClock,
    ToggleMinimalDate,
    ToggleMinimalBattery,
    // Collapsed UI
    ToggleFocusIcon,
    ToggleFocusTime,
    ToggleMediaArt,
    ToggleMediaEq,
    ToggleTodoIcon,
    ToggleTodoCount,
    // Hover UI & Audio Wave
    ToggleHoverClock,
    ToggleHoverDate,
    ToggleHoverBattery,
    ToggleHoverWifi,
    ToggleHoverHeadphones,
    ToggleHoverSlide,
    SetWaveStyle(WaveStyle),
    SetWaveColor(WaveColor),
    SetWaveSensitivity(u8),
    ToggleArtPulse,
}

impl ModalAction {
    pub fn cursor_icon(&self) -> CursorIcon {
        match self {
            ModalAction::FocusTodoInput => CursorIcon::Text,
            ModalAction::DragHeader => CursorIcon::Default,
            _ => CursorIcon::Pointer,
        }
    }
}

pub struct ModalHit {
    pub rect: Rect,
    pub action: ModalAction,
}

pub struct ModalView<'a> {
    pub kind: ModalKind,
    pub page: SettingsPage,
    pub hover: Option<ModalAction>,
    pub todo_input: Option<&'a str>,
}

pub fn window_size(_kind: ModalKind) -> (f32, f32) {
    (700.0, 520.0)
}

pub fn draw_modal(cv: &mut Canvas, st: &AppState, v: &ModalView) -> Vec<ModalHit> {
    let mut hits = Vec::new();
    let (w, h) = window_size(v.kind);
    let r = Rect::new(0.0, 0.0, w, h);
    let radius = 20.0;

    // 1. Monochrome outer window shell
    cv.alpha = 1.0;
    cv.fill_round(r, radius, PANEL_BG, 1.0);

    // 2. Left Sidebar (width = 204px)
    let side_w = 204.0;
    cv.fill_round(Rect::new(0.0, 0.0, side_w + radius, h), radius, BG, 1.0);
    cv.fill_rect(Rect::new(side_w - radius, 0.0, radius, h), BG, 1.0);
    cv.fill_rect(Rect::new(side_w, 1.0, 1.0, h - 2.0), WHITE, 0.10);

    // Outer 1px crisp border
    cv.stroke_round(r.inset(0.5, 0.5), radius - 0.5, 1.0, WHITE, 0.16);

    // 3. Draggable top titlebar region + Close button
    hits.push(ModalHit { rect: Rect::new(0.0, 0.0, w - 52.0, 52.0), action: ModalAction::DragHeader });

    draw_sidebar(cv, st, v, Rect::new(0.0, 0.0, side_w, h), &mut hits);
    draw_topbar(cv, st, v, Rect::new(side_w + 1.0, 0.0, w - side_w - 1.0, 56.0), &mut hits);

    // 4. Main Content Area (right side)
    let body = Rect::new(side_w + 22.0, 64.0, w - side_w - 44.0, h - 80.0);
    match v.page {
        SettingsPage::ActivitiesAndPriority => draw_page_activities(cv, st, v, body, &mut hits),
        SettingsPage::GeneralAndMode => draw_page_general(cv, st, v, body, &mut hits),
        SettingsPage::MinimalAndCollapsed => draw_page_minimal_collapsed(cv, st, v, body, &mut hits),
        SettingsPage::HoverAndAudio => draw_page_hover(cv, st, v, body, &mut hits),
    }

    hits
}

fn draw_sidebar(cv: &mut Canvas, st: &AppState, v: &ModalView, r: Rect, hits: &mut Vec<ModalHit>) {
    let lang = st.lang();

    // Brand badge at top-left
    let badge = Rect::new(18.0, 18.0, 32.0, 32.0);
    cv.fill_round(badge, 9.0, WHITE, 0.96);
    cv.glyph(G_HOME, 14.5, badge, BG, 1.0);

    cv.text("Ailan", Font::text(16.0, 700, Align::Left), Rect::new(60.0, 16.0, 130.0, 20.0), WHITE, 1.0);
    let sub_brand = if lang == Lang::Vi { "Quản lý & Cài đặt" } else { "Control Hub" };
    cv.text(sub_brand, Font::text(11.0, 400, Align::Left), Rect::new(60.0, 34.0, 130.0, 16.0), TEXT_SOFT, 0.85);

    // Navigation items
    let nav_items = [
        (SettingsPage::ActivitiesAndPriority, G_PIN, t(lang, Key::NavActivities)),
        (SettingsPage::GeneralAndMode, G_SETTINGS, t(lang, Key::NavGeneral)),
        (SettingsPage::MinimalAndCollapsed, G_TIMER, t(lang, Key::NavMinimal)),
        (SettingsPage::HoverAndAudio, G_MUSIC, t(lang, Key::NavHover)),
    ];

    let mut ny = 74.0;
    for (pg, icon, label) in nav_items {
        let nr = Rect::new(12.0, ny, r.w - 24.0, 38.0);
        let active = v.page == pg;
        let act = ModalAction::SelectPage(pg);
        let h = if v.hover == Some(act) { 1.0 } else { 0.0 };

        if active {
            cv.fill_round(nr, 10.0, WHITE, 0.95);
            cv.glyph(icon, 13.0, Rect::new(nr.x + 10.0, nr.y, 22.0, nr.h), BG, 1.0);
            cv.text(
                label,
                Font::text(12.0, 650, Align::Left),
                Rect::new(nr.x + 36.0, nr.y, nr.w - 42.0, nr.h),
                BG,
                1.0,
            );
        } else {
            if h > 0.01 {
                cv.fill_round(nr, 10.0, WHITE, 0.08 * h);
            }
            cv.glyph(icon, 13.0, Rect::new(nr.x + 10.0, nr.y, 22.0, nr.h), TEXT_SOFT, 0.90);
            cv.text(
                label,
                Font::text(12.0, 500, Align::Left),
                Rect::new(nr.x + 36.0, nr.y, nr.w - 42.0, nr.h),
                TEXT_SOFT,
                0.92,
            );
        }
        hits.push(ModalHit { rect: nr, action: act });
        ny += 44.0;
    }

    // Bottom status summary card in sidebar
    let foot = Rect::new(14.0, r.h - 76.0, r.w - 28.0, 60.0);
    cv.fill_round(foot, 11.0, SURFACE, 1.0);
    cv.stroke_round(foot.inset(0.5, 0.5), 10.5, 1.0, WHITE, 0.08);

    let visible_cnt = st.hover_activities().len();
    let status_hdr = if lang == Lang::Vi { "Hiển thị Hover UI" } else { "Hover UI Active" };
    let status_val = if lang == Lang::Vi {
        format!("{visible_cnt}/3 hoạt động mở")
    } else {
        format!("{visible_cnt}/3 pages visible")
    };
    cv.text(status_hdr, Font::text(10.5, 500, Align::Left), Rect::new(foot.x + 12.0, foot.y + 10.0, foot.w - 24.0, 16.0), SUB, 1.0);
    cv.text(&status_val, Font::text(12.0, 650, Align::Left), Rect::new(foot.x + 12.0, foot.y + 28.0, foot.w - 24.0, 18.0), WHITE, 0.95);
}

fn draw_topbar(cv: &mut Canvas, st: &AppState, v: &ModalView, r: Rect, hits: &mut Vec<ModalHit>) {
    let lang = st.lang();
    let page_title = match v.page {
        SettingsPage::ActivitiesAndPriority => t(lang, Key::NavActivities),
        SettingsPage::GeneralAndMode => t(lang, Key::NavGeneral),
        SettingsPage::MinimalAndCollapsed => t(lang, Key::NavMinimal),
        SettingsPage::HoverAndAudio => t(lang, Key::NavHover),
    };

    cv.text(
        page_title,
        Font::text(16.0, 700, Align::Left),
        Rect::new(r.x + 22.0, r.y + 14.0, r.w - 80.0, 28.0),
        WHITE,
        1.0,
    );
    cv.fill_rect(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0), WHITE, 0.08);

    // Close button (top-right, monochrome, no hover scaling)
    let close_r = Rect::new(r.right() - 44.0, 14.0, 28.0, 28.0);
    let h_close = if v.hover == Some(ModalAction::Close) { 1.0 } else { 0.0 };
    cv.fill_round(close_r, 8.0, WHITE, 0.08 + 0.12 * h_close);
    cv.stroke_round(close_r.inset(0.5, 0.5), 7.5, 1.0, WHITE, 0.14 + 0.18 * h_close);
    cv.glyph(G_DELETE, 10.0, close_r, WHITE, 0.80 + 0.20 * h_close);
    hits.push(ModalHit { rect: close_r, action: ModalAction::Close });
}

/// Monochrome pill Toggle Switch (replaces checkboxes across the entire UI).
pub fn draw_switch(cv: &mut Canvas, r: Rect, checked: bool, hover: f32) {
    let sw = 36.0;
    let sh = 20.0;
    let sx = r.x + (r.w - sw) * 0.5;
    let sy = r.y + (r.h - sh) * 0.5;
    let track = Rect::new(sx, sy, sw, sh);
    let rad = sh * 0.5;

    if checked {
        cv.fill_round(track, rad, WHITE, 0.96);
        let knob_cx = track.right() - rad;
        let knob_cy = track.y + rad;
        cv.fill_circle(knob_cx, knob_cy, 7.2, BG, 1.0);
    } else {
        cv.fill_round(track, rad, WHITE, 0.12 + 0.06 * hover);
        cv.stroke_round(track.inset(0.5, 0.5), rad - 0.5, 1.0, WHITE, 0.22 + 0.12 * hover);
        let knob_cx = track.x + rad;
        let knob_cy = track.y + rad;
        cv.fill_circle(knob_cx, knob_cy, 6.8, TEXT_SOFT, 0.90);
    }
}

/// Draws a flat monochrome row with a label on the left and a Toggle Switch on the right.
fn draw_switch_row(
    cv: &mut Canvas,
    v: &ModalView,
    r: Rect,
    label: &str,
    checked: bool,
    action: ModalAction,
    hits: &mut Vec<ModalHit>,
) {
    let h = if v.hover == Some(action) { 1.0 } else { 0.0 };
    let bg_a = if checked { 0.09 } else { 0.04 } + 0.04 * h;
    cv.fill_round(r, 10.0, WHITE, bg_a);
    cv.stroke_round(r.inset(0.5, 0.5), 9.5, 1.0, WHITE, if checked { 0.18 } else { 0.08 } + 0.08 * h);

    cv.text(
        label,
        Font::text(12.0, if checked { 600 } else { 450 }, Align::Left),
        Rect::new(r.x + 12.0, r.y, r.w - 60.0, r.h),
        if checked { WHITE } else { TEXT_SOFT },
        0.96,
    );

    let sw_r = Rect::new(r.right() - 48.0, r.y, 40.0, r.h);
    draw_switch(cv, sw_r, checked, h);
    hits.push(ModalHit { rect: r, action });
}

fn draw_section_label(cv: &mut Canvas, x: f32, y: f32, w: f32, text: &str) {
    cv.text(text, Font::text(11.5, 650, Align::Left), Rect::new(x, y, w, 18.0), TEXT_SOFT, 0.90);
}

// ---- Page 1: Activities Management & Priority Order -----------------------------

fn draw_page_activities(cv: &mut Canvas, st: &AppState, v: &ModalView, r: Rect, hits: &mut Vec<ModalHit>) {
    let lang = st.lang();
    let mut y = r.y;

    // 1. Priority Order & Pin to Hover UI section
    draw_section_label(cv, r.x, y, r.w, t(lang, Key::PrioritySection));
    y += 22.0;

    let order = st.settings.priority.normalized_order();
    for (idx, &tab) in order.iter().enumerate() {
        let row = Rect::new(r.x, y, r.w, 44.0);
        let (icon, name) = match tab {
            Tab::Focus => (G_TIMER, t(lang, Key::TabFocus)),
            Tab::Music => (G_MUSIC, t(lang, Key::TabMusic)),
            Tab::Todo => (G_CHECKMARK, t(lang, Key::TabTodo)),
        };
        let running = st.is_activity_running(tab);
        let pinned = st.settings.is_pinned(tab);

        cv.fill_round(row, 11.0, SURFACE, 1.0);
        cv.stroke_round(row.inset(0.5, 0.5), 10.5, 1.0, WHITE, if running || pinned { 0.22 } else { 0.09 });

        // Rank badge (#1, #2, #3)
        let badge = Rect::new(row.x + 10.0, row.y + 9.0, 26.0, 26.0);
        cv.fill_round(badge, 7.0, WHITE, if idx == 0 { 0.92 } else { 0.12 });
        cv.text(
            &format!("#{}", idx + 1),
            Font::text(11.5, 700, Align::Center),
            badge,
            if idx == 0 { BG } else { WHITE },
            1.0,
        );

        cv.glyph(icon, 13.5, Rect::new(row.x + 42.0, row.y, 22.0, row.h), WHITE, 0.92);
        cv.text(name, Font::text(12.5, 650, Align::Left), Rect::new(row.x + 68.0, row.y + 5.0, 150.0, 18.0), WHITE, 0.98);

        let sub_status = match tab {
            Tab::Music if running => t(lang, Key::ActiveLabel),
            Tab::Music => t(lang, Key::AutoTriggerLabel),
            _ if running => t(lang, Key::ActiveLabel),
            _ if pinned => t(lang, Key::PinnedLabel),
            _ => t(lang, Key::IdleLabel),
        };
        cv.text(
            sub_status,
            Font::text(10.5, 450, Align::Left),
            Rect::new(row.x + 68.0, row.y + 22.0, 170.0, 16.0),
            if running { WHITE } else { SUB },
            0.90,
        );

        // Pin Switch ("Pin to Hover UI")
        let pin_act = ModalAction::TogglePin(tab);
        let h_pin = if v.hover == Some(pin_act) { 1.0 } else { 0.0 };
        let pin_lbl = if lang == Lang::Vi { "Ghim Hover" } else { "Pin Hover" };
        cv.text(
            pin_lbl,
            Font::text(11.0, 500, Align::Right),
            Rect::new(row.right() - 186.0, row.y, 70.0, row.h),
            if pinned { WHITE } else { TEXT_SOFT },
            0.90,
        );
        let sw_r = Rect::new(row.right() - 110.0, row.y, 40.0, row.h);
        draw_switch(cv, sw_r, pinned, h_pin);
        hits.push(ModalHit {
            rect: Rect::new(row.right() - 186.0, row.y, 116.0, row.h),
            action: pin_act,
        });

        // Move Up button
        let up_r = Rect::new(row.right() - 62.0, row.y + 8.0, 24.0, 28.0);
        let h_up = if v.hover == Some(ModalAction::PriorityUp(tab)) { 1.0 } else { 0.0 };
        cv.fill_round(up_r, 7.0, WHITE, if idx == 0 { 0.04 } else { 0.10 + 0.10 * h_up });
        cv.glyph(G_UP, 10.0, up_r, WHITE, if idx == 0 { 0.25 } else { 0.90 });
        if idx > 0 {
            hits.push(ModalHit { rect: up_r, action: ModalAction::PriorityUp(tab) });
        }

        // Move Down button
        let dn_r = Rect::new(row.right() - 34.0, row.y + 8.0, 24.0, 28.0);
        let h_dn = if v.hover == Some(ModalAction::PriorityDown(tab)) { 1.0 } else { 0.0 };
        cv.fill_round(dn_r, 7.0, WHITE, if idx + 1 == order.len() { 0.04 } else { 0.10 + 0.10 * h_dn });
        cv.glyph(G_DOWN, 10.0, dn_r, WHITE, if idx + 1 == order.len() { 0.25 } else { 0.90 });
        if idx + 1 < order.len() {
            hits.push(ModalHit { rect: dn_r, action: ModalAction::PriorityDown(tab) });
        }

        y += 50.0;
    }

    // Smart minute peek switch row
    let smart_r = Rect::new(r.x, y, r.w, 36.0);
    draw_switch_row(
        cv,
        v,
        smart_r,
        t(lang, Key::SmartMinutePeek),
        st.settings.priority.smart_minute_peek,
        ModalAction::ToggleSmartPeek,
        hits,
    );
    y += 46.0;

    // 2. Direct Activity Controls: Left = Pomodoro & Music, Right = To-do Manager
    let col_w = (r.w - 12.0) * 0.5;
    let left_x = r.x;
    let right_x = r.x + col_w + 12.0;

    // ---- Left Card: Pomodoro & Music Quick Control ----
    let pomo_card = Rect::new(left_x, y, col_w, 114.0);
    cv.fill_round(pomo_card, 12.0, SURFACE, 1.0);
    cv.stroke_round(pomo_card.inset(0.5, 0.5), 11.5, 1.0, WHITE, 0.10);

    cv.glyph(G_TIMER, 13.0, Rect::new(pomo_card.x + 12.0, pomo_card.y + 10.0, 20.0, 20.0), WHITE, 0.95);
    cv.text(
        "Pomodoro",
        Font::text(12.5, 650, Align::Left),
        Rect::new(pomo_card.x + 34.0, pomo_card.y + 10.0, 90.0, 20.0),
        WHITE,
        1.0,
    );
    let rem_str = fmt_duration(st.pomodoro.remaining(0));
    cv.text(
        &rem_str,
        Font::text(13.0, 700, Align::Right),
        Rect::new(pomo_card.right() - 80.0, pomo_card.y + 10.0, 68.0, 20.0),
        WHITE,
        0.95,
    );

    // Preset buttons (15m, 25m, 45m)
    let pw = (pomo_card.w - 24.0 - 12.0) / 3.0;
    for (i, mins) in [15u32, 25, 45].into_iter().enumerate() {
        let pr = Rect::new(pomo_card.x + 12.0 + i as f32 * (pw + 6.0), pomo_card.y + 38.0, pw, 28.0);
        let active = st.settings.durations.focus_min == mins;
        let act = ModalAction::QuickFocusPreset(mins);
        let h = if v.hover == Some(act) { 1.0 } else { 0.0 };
        if active {
            cv.fill_round(pr, 8.0, WHITE, 0.92);
            cv.text(&format!("{mins}{}", t(lang, Key::MinuteShort)), Font::text(11.5, 650, Align::Center), pr, BG, 1.0);
        } else {
            cv.fill_round(pr, 8.0, WHITE, 0.08 + 0.08 * h);
            cv.text(&format!("{mins}{}", t(lang, Key::MinuteShort)), Font::text(11.5, 500, Align::Center), pr, WHITE, 0.90);
        }
        hits.push(ModalHit { rect: pr, action: act });
    }

    // Start/Pause & Reset buttons
    let start_r = Rect::new(pomo_card.x + 12.0, pomo_card.y + 74.0, pomo_card.w - 62.0, 28.0);
    let h_st = if v.hover == Some(ModalAction::FocusToggle) { 1.0 } else { 0.0 };
    let running_pomo = st.pomodoro.is_running();
    if running_pomo {
        cv.fill_round(start_r, 8.0, WHITE, 0.94);
        cv.text(t(lang, Key::Pause), Font::text(11.5, 650, Align::Center), start_r, BG, 1.0);
    } else {
        cv.fill_round(start_r, 8.0, WHITE, 0.14 + 0.10 * h_st);
        cv.stroke_round(start_r.inset(0.5, 0.5), 7.5, 1.0, WHITE, 0.22);
        cv.text(t(lang, Key::Start), Font::text(11.5, 650, Align::Center), start_r, WHITE, 0.98);
    }
    hits.push(ModalHit { rect: start_r, action: ModalAction::FocusToggle });

    let rst_r = Rect::new(start_r.right() + 6.0, pomo_card.y + 74.0, 32.0, 28.0);
    let h_rst = if v.hover == Some(ModalAction::FocusReset) { 1.0 } else { 0.0 };
    cv.fill_round(rst_r, 8.0, WHITE, 0.08 + 0.10 * h_rst);
    cv.glyph(G_RESET, 11.0, rst_r, WHITE, 0.90);
    hits.push(ModalHit { rect: rst_r, action: ModalAction::FocusReset });

    // Music Mini Card below Pomodoro
    let mus_card = Rect::new(left_x, y + 122.0, col_w, 60.0);
    cv.fill_round(mus_card, 12.0, SURFACE, 1.0);
    cv.stroke_round(mus_card.inset(0.5, 0.5), 11.5, 1.0, WHITE, 0.10);
    cv.glyph(G_MUSIC, 13.0, Rect::new(mus_card.x + 12.0, mus_card.y + 10.0, 20.0, 20.0), WHITE, 0.95);
    let m_title = st
        .media
        .as_ref()
        .filter(|m| !m.title.is_empty())
        .map(|m| m.title.as_str())
        .unwrap_or_else(|| t(lang, Key::NothingPlaying));
    cv.text(
        m_title,
        Font::text(11.5, 600, Align::Left),
        Rect::new(mus_card.x + 34.0, mus_card.y + 10.0, mus_card.w - 44.0, 18.0),
        WHITE,
        0.95,
    );
    let m_playing = st.media.as_ref().is_some_and(|m| m.playing);
    for (i, (g, act)) in [
        (G_PREV, ModalAction::MediaPrev),
        (if m_playing { G_PAUSE } else { G_PLAY }, ModalAction::MediaToggle),
        (G_NEXT, ModalAction::MediaNext),
    ]
    .into_iter()
    .enumerate()
    {
        let br = Rect::new(mus_card.x + 12.0 + i as f32 * 36.0, mus_card.y + 30.0, 30.0, 22.0);
        let hm = if v.hover == Some(act) { 1.0 } else { 0.0 };
        cv.fill_round(br, 6.0, WHITE, 0.08 + 0.10 * hm);
        cv.glyph(g, 10.0, br, WHITE, 0.92);
        hits.push(ModalHit { rect: br, action: act });
    }

    // ---- Right Card: To-do Quick Manager ----
    let todo_card = Rect::new(right_x, y, col_w, 182.0);
    cv.fill_round(todo_card, 12.0, SURFACE, 1.0);
    cv.stroke_round(todo_card.inset(0.5, 0.5), 11.5, 1.0, WHITE, 0.10);

    cv.text(
        t(lang, Key::QuickTodo),
        Font::text(12.5, 650, Align::Left),
        Rect::new(todo_card.x + 12.0, todo_card.y + 10.0, todo_card.w - 24.0, 20.0),
        WHITE,
        1.0,
    );

    // Input field + Submit button
    let inp_r = Rect::new(todo_card.x + 10.0, todo_card.y + 34.0, todo_card.w - 50.0, 30.0);
    let editing = v.todo_input.is_some();
    cv.fill_round(inp_r, 8.0, WHITE, if editing { 0.14 } else { 0.07 });
    cv.stroke_round(inp_r.inset(0.5, 0.5), 7.5, 1.0, WHITE, if editing { 0.65 } else { 0.14 });
    let txt_r = Rect::new(inp_r.x + 10.0, inp_r.y, inp_r.w - 16.0, inp_r.h);
    match v.todo_input {
        Some(s) if !s.is_empty() => cv.text(&format!("{s}▏"), Font::text(11.5, 450, Align::Left), txt_r, WHITE, 1.0),
        Some(_) => cv.text(t(lang, Key::AddTask), Font::text(11.0, 400, Align::Left), txt_r, SUB, 0.95),
        None => cv.text(t(lang, Key::AddTask), Font::text(11.0, 400, Align::Left), txt_r, TEXT_SOFT, 0.85),
    }
    hits.push(ModalHit { rect: inp_r, action: ModalAction::FocusTodoInput });

    let sub_r = Rect::new(inp_r.right() + 6.0, todo_card.y + 34.0, 24.0, 30.0);
    let h_sub = if v.hover == Some(ModalAction::SubmitTodoInput) { 1.0 } else { 0.0 };
    cv.fill_round(sub_r, 8.0, WHITE, 0.88 + 0.12 * h_sub);
    cv.glyph(G_ADD, 10.5, sub_r, BG, 1.0);
    hits.push(ModalHit { rect: sub_r, action: ModalAction::SubmitTodoInput });

    // Up to 3 tasks inside the To-do card
    let items = st.data.todos.sorted();
    if items.is_empty() {
        cv.text(
            t(lang, Key::NoTasks),
            Font::text(11.0, 400, Align::Center),
            Rect::new(todo_card.x + 10.0, todo_card.y + 84.0, todo_card.w - 20.0, 60.0),
            SUB,
            0.90,
        );
    } else {
        for (i, item) in items.iter().take(3).enumerate() {
            let tr = Rect::new(todo_card.x + 10.0, todo_card.y + 72.0 + i as f32 * 33.0, todo_card.w - 20.0, 28.0);
            let ht = if v.hover == Some(ModalAction::TodoToggle(item.id)) { 1.0 } else { 0.0 };
            cv.fill_round(tr, 7.0, WHITE, if item.done { 0.14 } else { 0.06 } + 0.05 * ht);

            let cx = tr.x + 13.0;
            let cy = tr.y + tr.h * 0.5;
            if item.done {
                cv.fill_circle(cx, cy, 6.5, WHITE, 0.95);
                cv.glyph(G_CHECKMARK, 7.5, Rect::new(cx - 7.0, cy - 7.0, 14.0, 14.0), BG, 1.0);
            } else {
                cv.stroke_circle(cx, cy, 6.2, 1.2, WHITE, 0.55);
            }

            cv.text(
                &item.text,
                Font::text(11.0, 500, Align::Left),
                Rect::new(tr.x + 25.0, tr.y, tr.w - 46.0, tr.h),
                WHITE,
                if item.done { 0.50 } else { 0.95 },
            );

            let del_r = Rect::new(tr.right() - 20.0, tr.y + 2.0, 18.0, tr.h - 4.0);
            let hd = if v.hover == Some(ModalAction::TodoDelete(item.id)) { 1.0 } else { 0.0 };
            cv.glyph(G_DELETE, 8.0, del_r, WHITE, 0.45 + 0.55 * hd);
            hits.push(ModalHit { rect: del_r, action: ModalAction::TodoDelete(item.id) });
            hits.push(ModalHit {
                rect: Rect::new(tr.x, tr.y, tr.w - 20.0, tr.h),
                action: ModalAction::TodoToggle(item.id),
            });
        }
    }
}

// ---- Page 2: General & Mode Settings --------------------------------------------

fn draw_page_general(cv: &mut Canvas, st: &AppState, v: &ModalView, r: Rect, hits: &mut Vec<ModalHit>) {
    let lang = st.lang();
    let mut y = r.y;

    // 1. Operating Mode selector (Switch rows)
    draw_section_label(cv, r.x, y, r.w, t(lang, Key::ModeSection));
    y += 24.0;
    let modes = [
        (NotchMode::DynamicWhenActive, t(lang, Key::ModeDynamicActive)),
        (NotchMode::AlwaysDynamic, t(lang, Key::ModeAlwaysDynamic)),
        (NotchMode::MinimalOnly, t(lang, Key::ModeMinimalOnly)),
    ];
    for (m, label) in modes {
        let mr = Rect::new(r.x, y, r.w, 40.0);
        let active = st.settings.mode == m && (!st.settings.minimal || m == NotchMode::MinimalOnly);
        draw_switch_row(cv, v, mr, label, active, ModalAction::SetMode(m), hits);
        y += 46.0;
    }
    y += 10.0;

    // 2. Language & System section
    draw_section_label(cv, r.x, y, r.w, t(lang, Key::LanguageSection));
    y += 24.0;

    let lw = (r.w - 10.0) * 0.5;
    for (i, (l, name)) in [(Lang::Vi, "Tiếng Việt"), (Lang::En, "English")].into_iter().enumerate() {
        let lr = Rect::new(r.x + i as f32 * (lw + 10.0), y, lw, 36.0);
        let active = lang == l;
        let act = ModalAction::SetLang(l);
        let h = if v.hover == Some(act) { 1.0 } else { 0.0 };
        if active {
            cv.fill_round(lr, 10.0, WHITE, 0.95);
            cv.text(name, Font::text(12.5, 700, Align::Center), lr, BG, 1.0);
        } else {
            cv.fill_round(lr, 10.0, WHITE, 0.06 + 0.06 * h);
            cv.stroke_round(lr.inset(0.5, 0.5), 9.5, 1.0, WHITE, 0.14);
            cv.text(name, Font::text(12.5, 500, Align::Center), lr, TEXT_SOFT, 0.95);
        }
        hits.push(ModalHit { rect: lr, action: act });
    }
    y += 48.0;

    draw_switch_row(
        cv,
        v,
        Rect::new(r.x, y, r.w, 40.0),
        t(lang, Key::StartWithWindows),
        autostart::is_enabled(),
        ModalAction::ToggleAutostart,
        hits,
    );
    y += 46.0;

    draw_switch_row(
        cv,
        v,
        Rect::new(r.x, y, r.w, 40.0),
        t(lang, Key::ReduceMotion),
        st.settings.reduce_motion,
        ModalAction::ToggleReduceMotion,
        hits,
    );
}

// ---- Page 3: Minimal & Collapsed UI Appearance ----------------------------------

fn draw_page_minimal_collapsed(cv: &mut Canvas, st: &AppState, v: &ModalView, r: Rect, hits: &mut Vec<ModalHit>) {
    let lang = st.lang();
    let mut y = r.y;
    let m = &st.settings.minimal_ui;
    let c = &st.settings.collapsed_ui;
    let hw = (r.w - 10.0) * 0.5;

    // 1. Minimal UI section
    draw_section_label(cv, r.x, y, r.w, t(lang, Key::MinimalUiSection));
    y += 22.0;

    draw_switch_row(cv, v, Rect::new(r.x, y, r.w, 36.0), t(lang, Key::HideWhenIdle), m.hide_when_idle, ModalAction::ToggleMinimalHide, hits);
    y += 42.0;
    draw_switch_row(cv, v, Rect::new(r.x, y, hw, 36.0), t(lang, Key::ShowDot), m.show_dot, ModalAction::ToggleMinimalDot, hits);
    draw_switch_row(cv, v, Rect::new(r.x + hw + 10.0, y, hw, 36.0), t(lang, Key::ShowClock), m.show_clock, ModalAction::ToggleMinimalClock, hits);
    y += 42.0;
    draw_switch_row(cv, v, Rect::new(r.x, y, hw, 36.0), t(lang, Key::ShowDate), m.show_date, ModalAction::ToggleMinimalDate, hits);
    draw_switch_row(cv, v, Rect::new(r.x + hw + 10.0, y, hw, 36.0), t(lang, Key::ShowBattery), m.show_battery, ModalAction::ToggleMinimalBattery, hits);
    y += 50.0;

    // 2. Collapsed UI when activities run
    draw_section_label(cv, r.x, y, r.w, t(lang, Key::CollapsedUiSection));
    y += 22.0;

    let pairs = [
        (
            (t(lang, Key::FocusShowIcon), c.focus_show_icon, ModalAction::ToggleFocusIcon),
            (t(lang, Key::FocusShowTime), c.focus_show_time, ModalAction::ToggleFocusTime),
        ),
        (
            (t(lang, Key::MediaShowArt), c.media_show_art, ModalAction::ToggleMediaArt),
            (t(lang, Key::MediaShowEq), c.media_show_eq, ModalAction::ToggleMediaEq),
        ),
        (
            (t(lang, Key::TodoShowIcon), c.todo_show_icon, ModalAction::ToggleTodoIcon),
            (t(lang, Key::TodoShowCount), c.todo_show_count, ModalAction::ToggleTodoCount),
        ),
    ];

    for (left, right) in pairs {
        draw_switch_row(cv, v, Rect::new(r.x, y, hw, 36.0), left.0, left.1, left.2, hits);
        draw_switch_row(cv, v, Rect::new(r.x + hw + 10.0, y, hw, 36.0), right.0, right.1, right.2, hits);
        y += 42.0;
    }
}

// ---- Page 4: Hover UI & Audio-Reactive Wave Settings ----------------------------

fn draw_page_hover(cv: &mut Canvas, st: &AppState, v: &ModalView, r: Rect, hits: &mut Vec<ModalHit>) {
    let lang = st.lang();
    let mut y = r.y;
    let hcfg = &st.settings.hover_ui;
    let mcfg = &st.settings.music_anim;
    let hw = (r.w - 10.0) * 0.5;

    // 1. Hover UI toggles (2-column switch grid)
    draw_section_label(cv, r.x, y, r.w, t(lang, Key::HoverUiSection));
    y += 22.0;

    let pairs = [
        (
            (t(lang, Key::ShowClock), hcfg.show_clock, ModalAction::ToggleHoverClock),
            (t(lang, Key::ShowDate), hcfg.show_date, ModalAction::ToggleHoverDate),
        ),
        (
            (t(lang, Key::ShowBattery), hcfg.show_battery, ModalAction::ToggleHoverBattery),
            (t(lang, Key::ShowWifi), hcfg.show_wifi, ModalAction::ToggleHoverWifi),
        ),
        (
            (t(lang, Key::ShowHeadphones), hcfg.show_headphones, ModalAction::ToggleHoverHeadphones),
            (t(lang, Key::ShowSlideControls), hcfg.show_slide_controls, ModalAction::ToggleHoverSlide),
        ),
    ];
    for (left, right) in pairs {
        draw_switch_row(cv, v, Rect::new(r.x, y, hw, 36.0), left.0, left.1, left.2, hits);
        draw_switch_row(cv, v, Rect::new(r.x + hw + 10.0, y, hw, 36.0), right.0, right.1, right.2, hits);
        y += 42.0;
    }

    y += 10.0;

    // 2. Audio-Reactive Music Animation customization section
    let sec_title = if lang == Lang::Vi {
        "Tuỳ chỉnh Sóng nhạc (Audio-Reactive)"
    } else {
        "Audio-Reactive Music Wave"
    };
    draw_section_label(cv, r.x, y, r.w, sec_title);
    y += 22.0;

    // Row A: Wave Style
    let style_lbl = if lang == Lang::Vi { "Kiểu sóng:" } else { "Wave mode:" };
    cv.text(style_lbl, Font::text(11.5, 500, Align::Left), Rect::new(r.x, y, 96.0, 32.0), TEXT_SOFT, 0.95);
    let sw = (r.w - 100.0 - 12.0) / 3.0;
    let styles = [
        (WaveStyle::AudioReactive, if lang == Lang::Vi { "Theo Âm thanh" } else { "Audio Reactive" }),
        (WaveStyle::SmoothWave, if lang == Lang::Vi { "Sóng Mượt" } else { "Smooth Wave" }),
        (WaveStyle::BouncePulse, if lang == Lang::Vi { "Nhịp Bass" } else { "Bass Pulse" }),
    ];
    for (i, (st_mode, label)) in styles.into_iter().enumerate() {
        let br = Rect::new(r.x + 100.0 + i as f32 * (sw + 6.0), y, sw, 32.0);
        let active = mcfg.style == st_mode;
        let act = ModalAction::SetWaveStyle(st_mode);
        let h = if v.hover == Some(act) { 1.0 } else { 0.0 };
        if active {
            cv.fill_round(br, 8.0, WHITE, 0.94);
            cv.text(label, Font::text(11.0, 650, Align::Center), br, BG, 1.0);
        } else {
            cv.fill_round(br, 8.0, WHITE, 0.06 + 0.06 * h);
            cv.stroke_round(br.inset(0.5, 0.5), 7.5, 1.0, WHITE, 0.12);
            cv.text(label, Font::text(11.0, 500, Align::Center), br, TEXT_SOFT, 0.95);
        }
        hits.push(ModalHit { rect: br, action: act });
    }
    y += 40.0;

    // Row B: Audio Sensitivity (1..=4)
    let sens_lbl = if lang == Lang::Vi { "Độ nhạy âm:" } else { "Sensitivity:" };
    cv.text(sens_lbl, Font::text(11.5, 500, Align::Left), Rect::new(r.x, y, 96.0, 32.0), TEXT_SOFT, 0.95);
    let kw = (r.w - 100.0 - 18.0) / 4.0;
    let sens_opts = [
        (1u8, if lang == Lang::Vi { "Thấp (0.6x)" } else { "Low (0.6x)" }),
        (2u8, if lang == Lang::Vi { "Chuẩn (1.0x)" } else { "Normal (1x)" }),
        (3u8, if lang == Lang::Vi { "Cao (1.5x)" } else { "High (1.5x)" }),
        (4u8, if lang == Lang::Vi { "Cực đại (2.2x)" } else { "Max (2.2x)" }),
    ];
    for (i, (lvl, label)) in sens_opts.into_iter().enumerate() {
        let br = Rect::new(r.x + 100.0 + i as f32 * (kw + 6.0), y, kw, 32.0);
        let active = mcfg.sensitivity.clamp(1, 4) == lvl;
        let act = ModalAction::SetWaveSensitivity(lvl);
        let h = if v.hover == Some(act) { 1.0 } else { 0.0 };
        if active {
            cv.fill_round(br, 8.0, WHITE, 0.94);
            cv.text(label, Font::text(10.5, 650, Align::Center), br, BG, 1.0);
        } else {
            cv.fill_round(br, 8.0, WHITE, 0.06 + 0.06 * h);
            cv.stroke_round(br.inset(0.5, 0.5), 7.5, 1.0, WHITE, 0.12);
            cv.text(label, Font::text(10.5, 500, Align::Center), br, TEXT_SOFT, 0.95);
        }
        hits.push(ModalHit { rect: br, action: act });
    }
    y += 40.0;

    // Row C: Monochrome Wave Shade (White / Silver / SoftGray / DimGray)
    let col_lbl = if lang == Lang::Vi { "Sắc độ sóng:" } else { "Wave shade:" };
    cv.text(col_lbl, Font::text(11.5, 500, Align::Left), Rect::new(r.x, y, 96.0, 32.0), TEXT_SOFT, 0.95);
    let shades = [
        (WaveColor::White, if lang == Lang::Vi { "Trắng sáng" } else { "Pure White" }, WHITE),
        (WaveColor::Silver, if lang == Lang::Vi { "Bạc" } else { "Silver" }, SILVER),
        (WaveColor::SoftGray, if lang == Lang::Vi { "Xám nhạt" } else { "Soft Gray" }, TEXT_SOFT),
        (WaveColor::DimGray, if lang == Lang::Vi { "Xám trầm" } else { "Dim Gray" }, SUB),
    ];
    for (i, (wc, label, rgb)) in shades.into_iter().enumerate() {
        let br = Rect::new(r.x + 100.0 + i as f32 * (kw + 6.0), y, kw, 32.0);
        let active = mcfg.color == wc;
        let act = ModalAction::SetWaveColor(wc);
        let h = if v.hover == Some(act) { 1.0 } else { 0.0 };
        if active {
            cv.fill_round(br, 8.0, WHITE, 0.94);
            cv.fill_circle(br.x + 12.0, br.y + br.h * 0.5, 4.0, BG, 1.0);
            cv.text(label, Font::text(10.5, 650, Align::Left), Rect::new(br.x + 20.0, br.y, br.w - 22.0, br.h), BG, 1.0);
        } else {
            cv.fill_round(br, 8.0, WHITE, 0.06 + 0.06 * h);
            cv.stroke_round(br.inset(0.5, 0.5), 7.5, 1.0, WHITE, 0.12);
            cv.fill_circle(br.x + 12.0, br.y + br.h * 0.5, 4.0, rgb, 1.0);
            cv.text(label, Font::text(10.5, 500, Align::Left), Rect::new(br.x + 20.0, br.y, br.w - 22.0, br.h), TEXT_SOFT, 0.95);
        }
        hits.push(ModalHit { rect: br, action: act });
    }
    y += 42.0;

    // Row D: Album Art Beat Pulse toggle switch
    let pulse_lbl = if lang == Lang::Vi {
        "Nhịp đập ảnh bìa bài hát theo Bass âm thanh"
    } else {
        "Pulse album cover art with audio bass beat"
    };
    draw_switch_row(cv, v, Rect::new(r.x, y, r.w, 36.0), pulse_lbl, mcfg.art_pulse, ModalAction::ToggleArtPulse, hits);
}

pub fn hit_test_modal(hits: &[ModalHit], x: f32, y: f32) -> Option<ModalAction> {
    hits.iter().rev().find(|h| h.rect.contains(x, y)).map(|h| h.action)
}
