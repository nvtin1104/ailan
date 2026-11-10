//! Separate Direct2D flat dark windows for:
//! 1. Quick-Add Activity Modal (`ModalKind::AddActivity`)
//! 2. Full Settings & UI Customization App (`ModalKind::Settings`)

use ailan_core::i18n::{t, Key, Lang};
use ailan_core::settings::{NotchMode, Tab, WaveColor, WaveStyle};
use ailan_core::AppState;

use crate::gfx::{Align, Canvas, Font, Rect};
use crate::platform::autostart;
use crate::ui::{
    draw_glass_pill, AMBER, BG, CORAL, CYAN, G_ADD, G_BOX, G_BOX_CHECKED, G_DELETE, G_DOWN, G_MUSIC, G_PLAY,
    G_SETTINGS, G_TIMER, G_UP, INDIGO, MINT_HI, SUB, TEXT, TEXT_SOFT, VIOLET, WHITE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    AddActivity,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsPage {
    #[default]
    ModeAndLang,
    Priority,
    MinimalAndCollapsed,
    HoverUi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalAction {
    Close,
    DragHeader,
    SwitchToSettings,
    SwitchToAdd,
    SelectPage(SettingsPage),
    // Quick-Add actions
    QuickFocus(u32),
    QuickMedia,
    QuickTodoTogglePin,
    FocusTodoInput,
    SubmitTodoInput,
    // Settings: Mode & Language
    SetLang(Lang),
    SetMode(NotchMode),
    ToggleAutostart,
    TogglePinTodo,
    // Settings: Priority & Smart Switch
    ToggleSmartPeek,
    PriorityUp(Tab),
    PriorityDown(Tab),
    // Settings: Minimal UI
    ToggleMinimalHide,
    ToggleMinimalDot,
    ToggleMinimalClock,
    ToggleMinimalDate,
    ToggleMinimalBattery,
    // Settings: Collapsed UI
    ToggleFocusIcon,
    ToggleFocusLabel,
    ToggleFocusTime,
    ToggleMediaArt,
    ToggleMediaTitle,
    ToggleMediaEq,
    ToggleTodoIcon,
    ToggleTodoText,
    ToggleTodoCount,
    // Settings: Hover UI
    ToggleHoverClock,
    ToggleHoverDate,
    ToggleHoverBattery,
    ToggleHoverWifi,
    ToggleHoverHeadphones,
    ToggleHoverSlide,
    ToggleHoverAddBtn,
    // Settings: Audio-Reactive Music Animation
    SetWaveStyle(WaveStyle),
    SetWaveColor(WaveColor),
    SetWaveSensitivity(u8),
    ToggleArtPulse,
}

pub struct ModalHit {
    pub rect: Rect,
    pub action: ModalAction,
}

pub struct ModalView<'a> {
    pub kind: ModalKind,
    pub page: SettingsPage,
    pub cursor: Option<(f32, f32)>,
    pub hover: Option<ModalAction>,
    pub todo_input: Option<&'a str>,
}

pub fn window_size(kind: ModalKind) -> (f32, f32) {
    match kind {
        ModalKind::AddActivity => (420.0, 340.0),
        ModalKind::Settings => (540.0, 540.0),
    }
}

pub fn draw_modal(cv: &mut Canvas, st: &AppState, v: &ModalView) -> Vec<ModalHit> {
    let mut hits = Vec::new();
    let (w, h) = window_size(v.kind);
    let r = Rect::new(0.0, 0.0, w, h);
    let radius = 22.0;

    // Pure pitch-black window background (no Liquid Glass gradients) with a crisp 1px border.
    cv.alpha = 1.0;
    cv.fill_round(r, radius, BG, 1.0);
    cv.stroke_round(r.inset(0.6, 0.6), radius - 0.6, 1.0, WHITE, 0.14);

    match v.kind {
        ModalKind::AddActivity => draw_add_activity(cv, st, v, r, &mut hits),
        ModalKind::Settings => draw_settings(cv, st, v, r, &mut hits),
    }

    hits
}

fn draw_header(
    cv: &mut Canvas,
    v: &ModalView,
    r: Rect,
    icon: char,
    title: &str,
    subtitle: &str,
    hits: &mut Vec<ModalHit>,
) {
    hits.push(ModalHit { rect: Rect::new(0.0, 0.0, r.w - 52.0, 56.0), action: ModalAction::DragHeader });

    let ic = Rect::new(18.0, 14.0, 34.0, 34.0);
    draw_glass_pill(cv, v.cursor, ic, 11.0, 0.45, MINT_HI, false);
    cv.glyph(icon, 15.0, ic, MINT_HI, 1.0);

    cv.text(title, Font::text(15.5, 650, Align::Left), Rect::new(62.0, 12.0, r.w - 120.0, 22.0), WHITE, 1.0);
    cv.text(subtitle, Font::text(11.5, 400, Align::Left), Rect::new(62.0, 32.0, r.w - 120.0, 18.0), TEXT_SOFT, 0.85);

    let close_r = Rect::new(r.w - 44.0, 15.0, 28.0, 28.0);
    let h_close = if v.hover == Some(ModalAction::Close) { 1.0 } else { 0.0 };
    draw_glass_pill(cv, v.cursor, close_r, 14.0, 0.2 + 0.7 * h_close, CORAL, false);
    cv.glyph(G_DELETE, 10.5, close_r, if h_close > 0.5 { CORAL } else { TEXT_SOFT }, 0.95);
    hits.push(ModalHit { rect: close_r, action: ModalAction::Close });
}

fn draw_add_activity(cv: &mut Canvas, st: &AppState, v: &ModalView, r: Rect, hits: &mut Vec<ModalHit>) {
    let lang = st.lang();
    draw_header(
        cv,
        v,
        r,
        G_ADD,
        t(lang, Key::AddActivityTitle),
        t(lang, Key::AddActivitySubtitle),
        hits,
    );

    // 1. Focus quick buttons row (25m, 45m, 15m).
    let y1 = 68.0;
    let cw = (r.w - 36.0 - 16.0) / 3.0;
    let focus_presets = [
        (25u32, t(lang, Key::QuickFocus25), MINT_HI),
        (45u32, t(lang, Key::QuickFocus45), CYAN),
        (15u32, t(lang, Key::QuickFocus15), AMBER),
    ];
    for (i, (mins, label, col)) in focus_presets.into_iter().enumerate() {
        let card = Rect::new(18.0 + i as f32 * (cw + 8.0), y1, cw, 64.0);
        let act = ModalAction::QuickFocus(mins);
        let h = if v.hover == Some(act) { 1.0 } else { 0.0 };
        let active = st.pomodoro.is_running() && st.settings.durations.focus_min == mins;
        draw_glass_pill(cv, v.cursor, card, 14.0, if active { 0.75 } else { 0.25 + 0.65 * h }, col, false);
        cv.glyph(G_TIMER, 16.0, Rect::new(card.x + 10.0, card.y + 10.0, 22.0, 22.0), col, 1.0);
        cv.text(
            &format!("{mins}{}", t(lang, Key::MinuteShort)),
            Font::text(14.0, 650, Align::Right),
            Rect::new(card.right() - 52.0, card.y + 10.0, 42.0, 22.0),
            WHITE,
            0.95,
        );
        cv.text(label, Font::text(11.5, 500, Align::Left), Rect::new(card.x + 10.0, card.y + 36.0, card.w - 16.0, 20.0), TEXT, 0.92);
        hits.push(ModalHit { rect: card, action: act });
    }

    // 2. Quick Music Player activation card.
    let y2 = 144.0;
    let media_card = Rect::new(18.0, y2, r.w - 36.0, 52.0);
    let h_media = if v.hover == Some(ModalAction::QuickMedia) { 1.0 } else { 0.0 };
    let media_running = st.is_activity_running(Tab::Music);
    draw_glass_pill(cv, v.cursor, media_card, 14.0, if media_running { 0.65 } else { 0.25 + 0.65 * h_media }, CYAN, false);
    cv.glyph(G_MUSIC, 16.0, Rect::new(media_card.x + 12.0, media_card.y, 28.0, media_card.h), CYAN, 1.0);
    let media_title = match &st.media {
        Some(m) if !m.title.is_empty() => format!("{} — {}", t(lang, Key::QuickMedia), m.title),
        _ => t(lang, Key::QuickMedia).to_string(),
    };
    cv.text(
        &media_title,
        Font::text(13.0, 600, Align::Left),
        Rect::new(media_card.x + 44.0, media_card.y + 7.0, media_card.w - 120.0, 20.0),
        WHITE,
        0.98,
    );
    cv.text(
        t(lang, Key::NothingPlayingHint),
        Font::text(11.0, 400, Align::Left),
        Rect::new(media_card.x + 44.0, media_card.y + 26.0, media_card.w - 120.0, 18.0),
        TEXT_SOFT,
        0.8,
    );
    let status_lbl = if media_running { t(lang, Key::ActiveLabel) } else { t(lang, Key::Start) };
    cv.text(
        status_lbl,
        Font::text(11.5, 600, Align::Right),
        Rect::new(media_card.right() - 78.0, media_card.y, 64.0, media_card.h),
        if media_running { MINT_HI } else { CYAN },
        0.95,
    );
    hits.push(ModalHit { rect: media_card, action: ModalAction::QuickMedia });

    // 3. Quick To-do input + Pin To-do activity row.
    let y3 = 208.0;
    let todo_field = Rect::new(18.0, y3, r.w - 36.0 - 44.0, 42.0);
    let editing = v.todo_input.is_some();
    let h_todo = if v.hover == Some(ModalAction::FocusTodoInput) { 1.0 } else { 0.0 };
    draw_glass_pill(cv, v.cursor, todo_field, 12.0, if editing { 0.85 } else { 0.25 + 0.5 * h_todo }, INDIGO, !editing);
    cv.glyph(G_BOX_CHECKED, 13.0, Rect::new(todo_field.x + 10.0, todo_field.y, 22.0, todo_field.h), INDIGO, 1.0);
    let txt_r = Rect::new(todo_field.x + 36.0, todo_field.y, todo_field.w - 44.0, todo_field.h);
    match v.todo_input {
        Some(s) if !s.is_empty() => cv.text(&format!("{s}▏"), Font::text(12.5, 400, Align::Left), txt_r, WHITE, 1.0),
        Some(_) => cv.text(t(lang, Key::AddTaskHint), Font::text(11.5, 400, Align::Left), txt_r, SUB, 0.95),
        None => cv.text(t(lang, Key::AddTask), Font::text(12.5, 400, Align::Left), txt_r, TEXT_SOFT, 0.9),
    }
    hits.push(ModalHit { rect: todo_field, action: ModalAction::FocusTodoInput });

    let add_btn = Rect::new(todo_field.right() + 6.0, y3, 38.0, 42.0);
    let h_sub = if v.hover == Some(ModalAction::SubmitTodoInput) { 1.0 } else { 0.0 };
    draw_glass_pill(cv, v.cursor, add_btn, 12.0, 0.4 + 0.55 * h_sub, MINT_HI, false);
    cv.glyph(G_PLAY, 12.0, add_btn, WHITE, 1.0);
    hits.push(ModalHit { rect: add_btn, action: ModalAction::SubmitTodoInput });

    // 4. Bottom row: Pin To-do toggle & Open Full Settings App button.
    let y4 = 266.0;
    let pin_r = Rect::new(18.0, y4, (r.w - 44.0) * 0.56, 36.0);
    let h_pin = if v.hover == Some(ModalAction::QuickTodoTogglePin) { 1.0 } else { 0.0 };
    let pinned = st.settings.todo_active;
    draw_glass_pill(cv, v.cursor, pin_r, 12.0, if pinned { 0.65 } else { 0.2 + 0.5 * h_pin }, INDIGO, !pinned);
    cv.glyph(if pinned { G_BOX_CHECKED } else { G_BOX }, 12.5, Rect::new(pin_r.x + 8.0, pin_r.y, 20.0, pin_r.h), if pinned { MINT_HI } else { TEXT_SOFT }, 1.0);
    cv.text(
        &format!("{} ({})", t(lang, Key::QuickTodo), st.data.todos.pending_count()),
        Font::text(11.5, 500, Align::Left),
        Rect::new(pin_r.x + 30.0, pin_r.y, pin_r.w - 36.0, pin_r.h),
        WHITE,
        0.92,
    );
    hits.push(ModalHit { rect: pin_r, action: ModalAction::QuickTodoTogglePin });

    let set_r = Rect::new(pin_r.right() + 8.0, y4, r.w - 18.0 - (pin_r.right() + 8.0), 36.0);
    let h_set = if v.hover == Some(ModalAction::SwitchToSettings) { 1.0 } else { 0.0 };
    draw_glass_pill(cv, v.cursor, set_r, 12.0, 0.35 + 0.6 * h_set, CYAN, false);
    cv.glyph(G_SETTINGS, 12.5, Rect::new(set_r.x + 10.0, set_r.y, 20.0, set_r.h), CYAN, 1.0);
    cv.text(
        t(lang, Key::OpenSettings),
        Font::text(12.0, 600, Align::Center),
        Rect::new(set_r.x + 24.0, set_r.y, set_r.w - 30.0, set_r.h),
        WHITE,
        0.96,
    );
    hits.push(ModalHit { rect: set_r, action: ModalAction::SwitchToSettings });
}

fn draw_settings(cv: &mut Canvas, st: &AppState, v: &ModalView, r: Rect, hits: &mut Vec<ModalHit>) {
    let lang = st.lang();
    draw_header(
        cv,
        v,
        r,
        G_SETTINGS,
        t(lang, Key::SettingsTitle),
        "Ailan · Compact Black Notch",
        hits,
    );

    // Quick button to jump to Add Activity modal.
    let quick_add = Rect::new(r.w - 170.0, 15.0, 120.0, 28.0);
    let h_qa = if v.hover == Some(ModalAction::SwitchToAdd) { 1.0 } else { 0.0 };
    draw_glass_pill(cv, v.cursor, quick_add, 14.0, 0.3 + 0.6 * h_qa, MINT_HI, false);
    cv.glyph(G_ADD, 10.5, Rect::new(quick_add.x + 8.0, quick_add.y, 16.0, quick_add.h), MINT_HI, 1.0);
    cv.text(
        t(lang, Key::AddActivity),
        Font::text(11.0, 600, Align::Center),
        Rect::new(quick_add.x + 22.0, quick_add.y, quick_add.w - 26.0, quick_add.h),
        WHITE,
        0.95,
    );
    hits.push(ModalHit { rect: quick_add, action: ModalAction::SwitchToAdd });

    // Page navigation bar (4 sections).
    let pages = [
        (SettingsPage::ModeAndLang, if lang == Lang::Vi { "Chế độ & Ngôn ngữ" } else { "Mode & Language" }),
        (SettingsPage::Priority, if lang == Lang::Vi { "Ưu tiên & Chuyển đổi" } else { "Priority & Switch" }),
        (SettingsPage::MinimalAndCollapsed, if lang == Lang::Vi { "UI Tối giản & Thu nhỏ" } else { "Minimal & Collapsed" }),
        (SettingsPage::HoverUi, if lang == Lang::Vi { "Hover & Sóng nhạc" } else { "Hover & Music Wave" }),
    ];
    let nav_y = 64.0;
    let pw = (r.w - 36.0 - 18.0) / 4.0;
    for (i, (pg, label)) in pages.into_iter().enumerate() {
        let pr = Rect::new(18.0 + i as f32 * (pw + 6.0), nav_y, pw, 30.0);
        let active = v.page == pg;
        let act = ModalAction::SelectPage(pg);
        let h = if v.hover == Some(act) { 1.0 } else { 0.0 };
        draw_glass_pill(cv, v.cursor, pr, 11.0, if active { 0.85 } else { 0.15 + 0.45 * h }, CYAN, !active);
        cv.text(
            label,
            Font::text(11.0, if active { 650 } else { 500 }, Align::Center),
            pr,
            if active { WHITE } else { TEXT_SOFT },
            0.96,
        );
        hits.push(ModalHit { rect: pr, action: act });
    }

    let body = Rect::new(18.0, 104.0, r.w - 36.0, r.h - 118.0);
    match v.page {
        SettingsPage::ModeAndLang => draw_page_mode(cv, st, v, body, hits),
        SettingsPage::Priority => draw_page_priority(cv, st, v, body, hits),
        SettingsPage::MinimalAndCollapsed => draw_page_minimal_collapsed(cv, st, v, body, hits),
        SettingsPage::HoverUi => draw_page_hover(cv, st, v, body, hits),
    }
}

fn draw_toggle_row(
    cv: &mut Canvas,
    v: &ModalView,
    r: Rect,
    label: &str,
    checked: bool,
    action: ModalAction,
    hits: &mut Vec<ModalHit>,
) {
    let h = if v.hover == Some(action) { 1.0 } else { 0.0 };
    draw_glass_pill(cv, v.cursor, r, 10.0, if checked { 0.35 + 0.3 * h } else { 0.1 + 0.35 * h }, MINT_HI, !checked);
    cv.glyph(
        if checked { G_BOX_CHECKED } else { G_BOX },
        13.0,
        Rect::new(r.x + 10.0, r.y, 22.0, r.h),
        if checked { MINT_HI } else { TEXT_SOFT },
        0.95,
    );
    cv.text(
        label,
        Font::text(12.0, if checked { 550 } else { 450 }, Align::Left),
        Rect::new(r.x + 36.0, r.y, r.w - 44.0, r.h),
        if checked { WHITE } else { TEXT_SOFT },
        0.95,
    );
    hits.push(ModalHit { rect: r, action });
}

fn draw_page_mode(cv: &mut Canvas, st: &AppState, v: &ModalView, r: Rect, hits: &mut Vec<ModalHit>) {
    let lang = st.lang();
    let mut y = r.y;

    // 1. Language selector.
    cv.text(t(lang, Key::LanguageSection), Font::text(12.5, 650, Align::Left), Rect::new(r.x, y, r.w, 20.0), CYAN, 0.95);
    y += 24.0;
    let lw = (r.w - 8.0) * 0.5;
    for (i, (l, name)) in [(Lang::Vi, "Tiếng Việt"), (Lang::En, "English")].into_iter().enumerate() {
        let lr = Rect::new(r.x + i as f32 * (lw + 8.0), y, lw, 34.0);
        let active = lang == l;
        let act = ModalAction::SetLang(l);
        let h = if v.hover == Some(act) { 1.0 } else { 0.0 };
        draw_glass_pill(cv, v.cursor, lr, 11.0, if active { 0.8 } else { 0.15 + 0.45 * h }, MINT_HI, !active);
        cv.text(name, Font::text(12.5, if active { 650 } else { 500 }, Align::Center), lr, WHITE, 0.96);
        hits.push(ModalHit { rect: lr, action: act });
    }
    y += 46.0;

    // 2. Operating Mode selector.
    cv.text(t(lang, Key::ModeSection), Font::text(12.5, 650, Align::Left), Rect::new(r.x, y, r.w, 20.0), CYAN, 0.95);
    y += 24.0;
    let modes = [
        (NotchMode::DynamicWhenActive, t(lang, Key::ModeDynamicActive)),
        (NotchMode::AlwaysDynamic, t(lang, Key::ModeAlwaysDynamic)),
        (NotchMode::MinimalOnly, t(lang, Key::ModeMinimalOnly)),
    ];
    for (m, label) in modes {
        let mr = Rect::new(r.x, y, r.w, 36.0);
        let active = st.settings.mode == m && !st.settings.minimal;
        draw_toggle_row(cv, v, mr, label, active, ModalAction::SetMode(m), hits);
        y += 42.0;
    }
    y += 8.0;

    // 3. System & To-do activity options.
    draw_toggle_row(
        cv,
        v,
        Rect::new(r.x, y, r.w, 36.0),
        t(lang, Key::PinTodoActivity),
        st.settings.todo_active,
        ModalAction::TogglePinTodo,
        hits,
    );
    y += 42.0;
    draw_toggle_row(
        cv,
        v,
        Rect::new(r.x, y, r.w, 36.0),
        t(lang, Key::StartWithWindows),
        autostart::is_enabled(),
        ModalAction::ToggleAutostart,
        hits,
    );
}

fn draw_page_priority(cv: &mut Canvas, st: &AppState, v: &ModalView, r: Rect, hits: &mut Vec<ModalHit>) {
    let lang = st.lang();
    let mut y = r.y;

    cv.text(t(lang, Key::PrioritySection), Font::text(12.5, 650, Align::Left), Rect::new(r.x, y, r.w, 20.0), CYAN, 0.95);
    y += 24.0;

    // Smart minute peek toggle card.
    let smart_r = Rect::new(r.x, y, r.w, 54.0);
    let checked = st.settings.priority.smart_minute_peek;
    let h_sm = if v.hover == Some(ModalAction::ToggleSmartPeek) { 1.0 } else { 0.0 };
    draw_glass_pill(cv, v.cursor, smart_r, 12.0, if checked { 0.45 + 0.3 * h_sm } else { 0.15 + 0.3 * h_sm }, MINT_HI, !checked);
    cv.glyph(
        if checked { G_BOX_CHECKED } else { G_BOX },
        14.0,
        Rect::new(smart_r.x + 12.0, smart_r.y, 24.0, smart_r.h),
        if checked { MINT_HI } else { TEXT_SOFT },
        1.0,
    );
    cv.text(
        t(lang, Key::SmartMinutePeek),
        Font::text(12.5, 600, Align::Left),
        Rect::new(smart_r.x + 40.0, smart_r.y + 8.0, smart_r.w - 50.0, 20.0),
        WHITE,
        0.98,
    );
    cv.text(
        t(lang, Key::SmartMinutePeekDesc),
        Font::text(11.0, 400, Align::Left),
        Rect::new(smart_r.x + 40.0, smart_r.y + 28.0, smart_r.w - 50.0, 18.0),
        TEXT_SOFT,
        0.85,
    );
    hits.push(ModalHit { rect: smart_r, action: ModalAction::ToggleSmartPeek });
    y += 68.0;

    // Reorderable activity priority list.
    let order = st.settings.priority.normalized_order();
    for (idx, &tab) in order.iter().enumerate() {
        let row = Rect::new(r.x, y, r.w, 46.0);
        let (icon, name, col) = match tab {
            Tab::Focus => (G_TIMER, t(lang, Key::TabFocus), MINT_HI),
            Tab::Music => (G_MUSIC, t(lang, Key::TabMusic), CYAN),
            Tab::Todo => (G_BOX_CHECKED, t(lang, Key::TabTodo), INDIGO),
        };
        let running = st.is_activity_running(tab);
        draw_glass_pill(cv, v.cursor, row, 12.0, if running { 0.45 } else { 0.2 }, col, false);

        cv.text(
            &format!("#{}", idx + 1),
            Font::text(13.0, 650, Align::Center),
            Rect::new(row.x + 10.0, row.y, 28.0, row.h),
            col,
            1.0,
        );
        cv.glyph(icon, 14.0, Rect::new(row.x + 42.0, row.y, 24.0, row.h), col, 1.0);
        cv.text(name, Font::text(13.0, 600, Align::Left), Rect::new(row.x + 72.0, row.y, 180.0, row.h), WHITE, 0.96);

        let status = if running { t(lang, Key::ActiveLabel) } else { t(lang, Key::IdleLabel) };
        cv.text(
            status,
            Font::text(11.5, 500, Align::Right),
            Rect::new(row.right() - 160.0, row.y, 76.0, row.h),
            if running { MINT_HI } else { SUB },
            0.9,
        );

        // Move Up button
        let up_r = Rect::new(row.right() - 72.0, row.y + 8.0, 28.0, 30.0);
        let h_up = if v.hover == Some(ModalAction::PriorityUp(tab)) { 1.0 } else { 0.0 };
        draw_glass_pill(cv, v.cursor, up_r, 8.0, 0.2 + 0.65 * h_up, CYAN, idx == 0);
        cv.glyph(G_UP, 11.0, up_r, if idx == 0 { SUB } else { WHITE }, if idx == 0 { 0.35 } else { 0.95 });
        if idx > 0 {
            hits.push(ModalHit { rect: up_r, action: ModalAction::PriorityUp(tab) });
        }

        // Move Down button
        let dn_r = Rect::new(row.right() - 38.0, row.y + 8.0, 28.0, 30.0);
        let h_dn = if v.hover == Some(ModalAction::PriorityDown(tab)) { 1.0 } else { 0.0 };
        draw_glass_pill(cv, v.cursor, dn_r, 8.0, 0.2 + 0.65 * h_dn, CYAN, idx + 1 == order.len());
        cv.glyph(G_DOWN, 11.0, dn_r, if idx + 1 == order.len() { SUB } else { WHITE }, if idx + 1 == order.len() { 0.35 } else { 0.95 });
        if idx + 1 < order.len() {
            hits.push(ModalHit { rect: dn_r, action: ModalAction::PriorityDown(tab) });
        }

        y += 54.0;
    }
}

fn draw_page_minimal_collapsed(cv: &mut Canvas, st: &AppState, v: &ModalView, r: Rect, hits: &mut Vec<ModalHit>) {
    let lang = st.lang();
    let mut y = r.y;
    let m = &st.settings.minimal_ui;
    let c = &st.settings.collapsed_ui;
    let hw = (r.w - 8.0) * 0.5;

    // 1. Minimal UI section
    cv.text(t(lang, Key::MinimalUiSection), Font::text(12.5, 650, Align::Left), Rect::new(r.x, y, r.w, 20.0), CYAN, 0.95);
    y += 22.0;

    draw_toggle_row(cv, v, Rect::new(r.x, y, r.w, 32.0), t(lang, Key::HideWhenIdle), m.hide_when_idle, ModalAction::ToggleMinimalHide, hits);
    y += 36.0;
    draw_toggle_row(cv, v, Rect::new(r.x, y, hw, 32.0), t(lang, Key::ShowDot), m.show_dot, ModalAction::ToggleMinimalDot, hits);
    draw_toggle_row(cv, v, Rect::new(r.x + hw + 8.0, y, hw, 32.0), t(lang, Key::ShowClock), m.show_clock, ModalAction::ToggleMinimalClock, hits);
    y += 36.0;
    draw_toggle_row(cv, v, Rect::new(r.x, y, hw, 32.0), t(lang, Key::ShowDate), m.show_date, ModalAction::ToggleMinimalDate, hits);
    draw_toggle_row(cv, v, Rect::new(r.x + hw + 8.0, y, hw, 32.0), t(lang, Key::ShowBattery), m.show_battery, ModalAction::ToggleMinimalBattery, hits);
    y += 44.0;

    // 2. Collapsed UI when activities run
    cv.text(t(lang, Key::CollapsedUiSection), Font::text(12.5, 650, Align::Left), Rect::new(r.x, y, r.w, 20.0), CYAN, 0.95);
    y += 22.0;

    let tw = (r.w - 12.0) / 3.0;
    let row1 = [
        (t(lang, Key::FocusShowIcon), c.focus_show_icon, ModalAction::ToggleFocusIcon),
        (t(lang, Key::FocusShowLabel), c.focus_show_label, ModalAction::ToggleFocusLabel),
        (t(lang, Key::FocusShowTime), c.focus_show_time, ModalAction::ToggleFocusTime),
    ];
    for (i, (lbl, chk, act)) in row1.into_iter().enumerate() {
        draw_toggle_row(cv, v, Rect::new(r.x + i as f32 * (tw + 6.0), y, tw, 34.0), lbl, chk, act, hits);
    }
    y += 40.0;

    let row2 = [
        (t(lang, Key::MediaShowArt), c.media_show_art, ModalAction::ToggleMediaArt),
        (t(lang, Key::MediaShowTitle), c.media_show_title, ModalAction::ToggleMediaTitle),
        (t(lang, Key::MediaShowEq), c.media_show_eq, ModalAction::ToggleMediaEq),
    ];
    for (i, (lbl, chk, act)) in row2.into_iter().enumerate() {
        draw_toggle_row(cv, v, Rect::new(r.x + i as f32 * (tw + 6.0), y, tw, 34.0), lbl, chk, act, hits);
    }
    y += 40.0;

    let row3 = [
        (t(lang, Key::TodoShowIcon), c.todo_show_icon, ModalAction::ToggleTodoIcon),
        (t(lang, Key::TodoShowText), c.todo_show_text, ModalAction::ToggleTodoText),
        (t(lang, Key::TodoShowCount), c.todo_show_count, ModalAction::ToggleTodoCount),
    ];
    for (i, (lbl, chk, act)) in row3.into_iter().enumerate() {
        draw_toggle_row(cv, v, Rect::new(r.x + i as f32 * (tw + 6.0), y, tw, 34.0), lbl, chk, act, hits);
    }
}

fn draw_page_hover(cv: &mut Canvas, st: &AppState, v: &ModalView, r: Rect, hits: &mut Vec<ModalHit>) {
    let lang = st.lang();
    let mut y = r.y;
    let hcfg = &st.settings.hover_ui;
    let mcfg = &st.settings.music_anim;
    let hw = (r.w - 8.0) * 0.5;

    // 1. Hover UI toggles in a clean 2-column grid
    cv.text(t(lang, Key::HoverUiSection), Font::text(12.0, 650, Align::Left), Rect::new(r.x, y, r.w, 18.0), CYAN, 0.95);
    y += 20.0;

    let pairs = [
        (
            (t(lang, Key::ShowClock), hcfg.show_clock, ModalAction::ToggleHoverClock),
            Some((t(lang, Key::ShowDate), hcfg.show_date, ModalAction::ToggleHoverDate)),
        ),
        (
            (t(lang, Key::ShowBattery), hcfg.show_battery, ModalAction::ToggleHoverBattery),
            Some((t(lang, Key::ShowWifi), hcfg.show_wifi, ModalAction::ToggleHoverWifi)),
        ),
        (
            (t(lang, Key::ShowHeadphones), hcfg.show_headphones, ModalAction::ToggleHoverHeadphones),
            Some((t(lang, Key::ShowSlideControls), hcfg.show_slide_controls, ModalAction::ToggleHoverSlide)),
        ),
        (
            (t(lang, Key::ShowAddButton), hcfg.show_add_button, ModalAction::ToggleHoverAddBtn),
            None,
        ),
    ];
    for (left, right) in pairs {
        draw_toggle_row(cv, v, Rect::new(r.x, y, hw, 30.0), left.0, left.1, left.2, hits);
        if let Some(rt) = right {
            draw_toggle_row(cv, v, Rect::new(r.x + hw + 8.0, y, hw, 30.0), rt.0, rt.1, rt.2, hits);
        }
        y += 34.0;
    }

    y += 6.0;

    // 2. Audio-Reactive Music Animation customization section
    let sec_title = if lang == Lang::Vi {
        "Tuỳ chỉnh Animation Nhạc theo Âm thanh (Audio-Reactive)"
    } else {
        "Audio-Reactive Music Wave Customization"
    };
    cv.text(sec_title, Font::text(12.0, 650, Align::Left), Rect::new(r.x, y, r.w, 18.0), MINT_HI, 0.98);
    y += 22.0;

    // Row A: Wave Style (AudioReactive / SmoothWave / BouncePulse)
    let style_lbl = if lang == Lang::Vi { "Kiểu sóng nhạc:" } else { "Wave mode:" };
    cv.text(style_lbl, Font::text(11.0, 500, Align::Left), Rect::new(r.x, y, 110.0, 28.0), TEXT_SOFT, 0.9);
    let sw = (r.w - 114.0 - 12.0) / 3.0;
    let styles = [
        (WaveStyle::AudioReactive, if lang == Lang::Vi { "Theo Âm thanh" } else { "Audio Reactive" }),
        (WaveStyle::SmoothWave, if lang == Lang::Vi { "Sóng Mượt" } else { "Smooth Wave" }),
        (WaveStyle::BouncePulse, if lang == Lang::Vi { "Nhịp Bass" } else { "Bass Pulse" }),
    ];
    for (i, (st_mode, label)) in styles.into_iter().enumerate() {
        let br = Rect::new(r.x + 114.0 + i as f32 * (sw + 6.0), y, sw, 28.0);
        let active = mcfg.style == st_mode;
        let act = ModalAction::SetWaveStyle(st_mode);
        let h = if v.hover == Some(act) { 1.0 } else { 0.0 };
        draw_glass_pill(cv, v.cursor, br, 9.0, if active { 0.85 } else { 0.15 + 0.45 * h }, CYAN, !active);
        cv.text(
            label,
            Font::text(11.0, if active { 650 } else { 500 }, Align::Center),
            br,
            if active { WHITE } else { TEXT_SOFT },
            0.96,
        );
        hits.push(ModalHit { rect: br, action: act });
    }
    y += 34.0;

    // Row B: Audio Sensitivity (1..=4)
    let sens_lbl = if lang == Lang::Vi { "Độ nhạy âm:" } else { "Sensitivity:" };
    cv.text(sens_lbl, Font::text(11.0, 500, Align::Left), Rect::new(r.x, y, 110.0, 28.0), TEXT_SOFT, 0.9);
    let kw = (r.w - 114.0 - 18.0) / 4.0;
    let sens_opts = [
        (1u8, if lang == Lang::Vi { "Thấp (0.6x)" } else { "Low (0.6x)" }),
        (2u8, if lang == Lang::Vi { "Chuẩn (1.0x)" } else { "Normal (1x)" }),
        (3u8, if lang == Lang::Vi { "Cao (1.5x)" } else { "High (1.5x)" }),
        (4u8, if lang == Lang::Vi { "Cực đại (2.2x)" } else { "Max (2.2x)" }),
    ];
    for (i, (lvl, label)) in sens_opts.into_iter().enumerate() {
        let br = Rect::new(r.x + 114.0 + i as f32 * (kw + 6.0), y, kw, 28.0);
        let active = mcfg.sensitivity.clamp(1, 4) == lvl;
        let act = ModalAction::SetWaveSensitivity(lvl);
        let h = if v.hover == Some(act) { 1.0 } else { 0.0 };
        draw_glass_pill(cv, v.cursor, br, 9.0, if active { 0.85 } else { 0.15 + 0.45 * h }, MINT_HI, !active);
        cv.text(
            label,
            Font::text(10.5, if active { 650 } else { 500 }, Align::Center),
            br,
            if active { WHITE } else { TEXT_SOFT },
            0.96,
        );
        hits.push(ModalHit { rect: br, action: act });
    }
    y += 34.0;

    // Row C: Wave Color (White / Mint / Cyan / Violet)
    let col_lbl = if lang == Lang::Vi { "Màu sóng nhạc:" } else { "Wave color:" };
    cv.text(col_lbl, Font::text(11.0, 500, Align::Left), Rect::new(r.x, y, 110.0, 28.0), TEXT_SOFT, 0.9);
    let colors = [
        (WaveColor::White, if lang == Lang::Vi { "Trắng" } else { "White" }, WHITE),
        (WaveColor::Mint, if lang == Lang::Vi { "Xanh lá" } else { "Mint" }, MINT_HI),
        (WaveColor::Cyan, if lang == Lang::Vi { "Xanh dương" } else { "Cyan" }, CYAN),
        (WaveColor::Violet, if lang == Lang::Vi { "Tím" } else { "Violet" }, VIOLET),
    ];
    for (i, (wc, label, rgb)) in colors.into_iter().enumerate() {
        let br = Rect::new(r.x + 114.0 + i as f32 * (kw + 6.0), y, kw, 28.0);
        let active = mcfg.color == wc;
        let act = ModalAction::SetWaveColor(wc);
        let h = if v.hover == Some(act) { 1.0 } else { 0.0 };
        draw_glass_pill(cv, v.cursor, br, 9.0, if active { 0.85 } else { 0.15 + 0.45 * h }, rgb, !active);
        cv.fill_circle(br.x + 12.0, br.y + br.h * 0.5, 4.0, rgb, 1.0);
        cv.text(
            label,
            Font::text(10.5, if active { 650 } else { 500 }, Align::Left),
            Rect::new(br.x + 20.0, br.y, br.w - 22.0, br.h),
            if active { WHITE } else { TEXT_SOFT },
            0.96,
        );
        hits.push(ModalHit { rect: br, action: act });
    }
    y += 34.0;

    // Row D: Album Art & Playhead Beat Pulse toggle
    let pulse_lbl = if lang == Lang::Vi {
        "Nhịp đập ảnh bìa & thanh thời gian theo Bass âm thanh"
    } else {
        "Pulse album art & progress playhead with audio beat"
    };
    draw_toggle_row(cv, v, Rect::new(r.x, y, r.w, 30.0), pulse_lbl, mcfg.art_pulse, ModalAction::ToggleArtPulse, hits);
}

pub fn hit_test_modal(hits: &[ModalHit], x: f32, y: f32) -> Option<ModalAction> {
    hits.iter().rev().find(|h| h.rect.contains(x, y)).map(|h| h.action)
}
