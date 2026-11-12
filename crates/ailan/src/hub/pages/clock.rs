//! Clock tools on the Reminders page: alarms, countdown timer, stopwatch, world clock.

use ailan_core::clock::{fmt_hms, fmt_stopwatch, Alarm, CITY_PRESETS, EVERY_DAY, WEEKDAYS};
use ailan_core::i18n::{t, weekday, Key, Lang};
use ailan_core::reminder::fmt_due;
use ailan_core::time::{civil, local_day, MINUTE};

use crate::gfx::{Align, Canvas, Font, Rect, TextStyle};
use crate::hub::widgets::*;
use crate::hub::{Field, HubAction, PageCtx};
use crate::ui::{G_ADD, G_BELL, G_PAUSE, G_PLAY, G_RESET};

const G_MINUS: char = '\u{E738}';
const G_FLAG: char = '\u{E7C1}';
const G_GLOBE: char = '\u{E774}';

/// Tab labels; Compact uses a shorter world-clock label.
pub fn tab_labels(lang: Lang, compact: bool) -> [&'static str; 5] {
    [
        t(lang, Key::ClockReminders),
        t(lang, Key::ClockAlarms),
        t(lang, Key::ClockTimer),
        t(lang, Key::ClockStopwatch),
        if compact { t(lang, Key::ClockWorldShort) } else { t(lang, Key::ClockWorld) },
    ]
}

const TAB_ICONS: [char; 5] = ['\u{EA8F}', '\u{E917}', '\u{E823}', '\u{E916}', '\u{E774}'];

/// Reminders page tab bar. Returns the y below it.
pub fn draw_tabs(cv: &mut Canvas, cx: &mut PageCtx, r: Rect, y: f32) -> f32 {
    let labels = tab_labels(cx.lang(), cx.compact);
    let items: Vec<(&str, Option<char>)> = labels.iter().zip(TAB_ICONS).map(|(l, g)| (*l, Some(g))).collect();
    let hover = (0..5).position(|i| cx.hover == Some(HubAction::SetClockTab(i as u8)));
    let tabs = draw_tab_strip(cv, Rect::new(r.x, y, r.w, 40.0), &items, cx.clock_tab_pos, hover, cx.compact);
    for (i, tr) in tabs.iter().enumerate() {
        cx.hit(*tr, HubAction::SetClockTab(i as u8));
    }
    y + 40.0 + 16.0
}

fn repeat_label(lang: Lang, days: u8) -> String {
    match days & EVERY_DAY {
        0 => t(lang, Key::AlarmOnce).to_string(),
        EVERY_DAY => t(lang, Key::AlarmEveryDay).to_string(),
        WEEKDAYS => t(lang, Key::AlarmWeekdays).to_string(),
        m => (0..7u32).filter(|d| m & (1 << d) != 0).map(|d| weekday(lang, d)).collect::<Vec<_>>().join(", "),
    }
}

/// Small `[− label +]` stepper. Returns its width.
fn stepper(cv: &mut Canvas, cx: &mut PageCtx, x: f32, y: f32, label: &str, minus: HubAction, plus: HubAction) -> f32 {
    let (w, h) = (78.0, 26.0);
    let r = Rect::new(x, y, w, h);
    cv.fill_round(r, h * 0.5, WHITE, 0.05);
    cv.hairline_round(r, h * 0.5, WHITE, 0.10);
    let mr = Rect::new(x, y, 24.0, h);
    let pr = Rect::new(x + w - 24.0, y, 24.0, h);
    draw_icon_button(cv, mr, G_MINUS, 9.0, cx.hover_t(minus), false, ACCENT_PRIMARY);
    draw_icon_button(cv, pr, G_ADD, 9.0, cx.hover_t(plus), false, ACCENT_PRIMARY);
    cv.text(label, Font::text(11.0, 600, Align::Center), Rect::new(x + 22.0, y, w - 44.0, h), TEXT_BODY, 1.0);
    cx.hit(mr, minus);
    cx.hit(pr, plus);
    w
}

/// One button in a centred [`button_group`].
struct Btn {
    action: HubAction,
    glyph: Option<char>,
    label: String,
    primary: bool,
}

/// Lay buttons out as one block centred on `center_x`.
fn button_group(cv: &mut Canvas, cx: &mut PageCtx, center_x: f32, y: f32, h: f32, items: &[Btn]) {
    let f = Font::text(13.0, 650, Align::Left);
    let widths: Vec<f32> = items
        .iter()
        .map(|b| {
            let tw = cv.text_width(&b.label, f, TextStyle::TABULAR);
            let icon = if b.glyph.is_some() { 24.0 } else { 0.0 };
            (tw + icon + 36.0).max(if b.primary { 124.0 } else { 56.0 })
        })
        .collect();
    let gap = 10.0;
    let total: f32 = widths.iter().sum::<f32>() + gap * (items.len().saturating_sub(1)) as f32;
    let mut x = center_x - total * 0.5;
    for (b, w) in items.iter().zip(widths) {
        let br = Rect::new(x, y, w, h);
        let kind = if b.primary { ButtonKind::Primary } else { ButtonKind::Ghost };
        draw_button(cv, br, "", kind, cx.hover_t(b.action));
        let fg = if b.primary { WHITE } else { TEXT_TITLE };
        let tw = cv.text_width(&b.label, f, TextStyle::TABULAR);
        let content = tw + if b.glyph.is_some() { 24.0 } else { 0.0 };
        let mut cx0 = br.x + (w - content) * 0.5;
        if let Some(g) = b.glyph {
            cv.glyph(g, 10.5, Rect::new(cx0, br.y, 16.0, br.h), fg, 1.0);
            cx0 += 24.0;
        }
        cv.text_styled(&b.label, f, Rect::new(cx0, br.y, tw + 2.0, br.h), fg, 1.0, TextStyle::TABULAR);
        cx.hit(br, b.action);
        x += w + gap;
    }
}

// ---- Alarms ------------------------------------------------------------------------

pub fn draw_alarms(cv: &mut Canvas, cx: &mut PageCtx, r: Rect, mut y: f32) {
    let lang = cx.lang();
    let st = cx.st;
    alarm_bar(cv, cx, Rect::new(r.x, y, r.w, 44.0));
    y += 44.0 + 18.0;

    let alarms: Vec<&Alarm> = st.data.alarms.sorted();
    if alarms.is_empty() {
        let er = Rect::new(r.x, y, r.w, (r.bottom() - y).max(220.0));
        cv.fill_circle_radial(er.x + er.w * 0.5, er.y + 70.0, 70.0, GLOW_AMBER, 0.5);
        cv.glyph(G_BELL, 26.0, Rect::new(er.x, er.y + 46.0, er.w, 48.0), TEXT_BODY, 0.9);
        cv.text(t(lang, Key::NoAlarms), Font::text(15.0, 650, Align::Center), Rect::new(er.x, er.y + 104.0, er.w, 22.0), TEXT_HERO, 1.0);
        cv.text(t(lang, Key::AlarmHint), Font::text(12.0, 400, Align::Center), Rect::new(er.x, er.y + 128.0, er.w, 18.0), TEXT_BODY, 0.9);
        cx.content_h = Some(y + cx.scroll - r.y + 240.0);
        return;
    }
    let card_h = 116.0;
    for a in alarms {
        if y + card_h >= r.y - 20.0 && y <= r.bottom() + 20.0 {
            alarm_card(cv, cx, Rect::new(r.x, y, r.w, card_h), a);
        }
        y += card_h + 10.0;
    }
    cx.content_h = Some(y + cx.scroll - r.y);
}

fn alarm_bar(cv: &mut Canvas, cx: &mut PageCtx, r: Rect) {
    let lang = cx.lang();
    let focused = cx.focus == Some(Field::Alarm);
    let act = HubAction::FocusField(Field::Alarm);
    draw_text_input(cv, r, cx.alarm_input, t(lang, Key::AlarmHint), focused, cx.hover_t(act), Font::text(13.5, 450, Align::Left), Some(G_BELL), 40.0);
    let submit = HubAction::SubmitField(Field::Alarm);
    let sr = Rect::new(r.right() - r.h + 6.0, r.y + 6.0, r.h - 12.0, r.h - 12.0);
    if !cx.alarm_input.trim().is_empty() {
        cv.fill_round_linear(sr, concentric(RADIUS_INPUT, 6.0), ACCENT_FILL, (sr.x, sr.y), (sr.right(), sr.bottom()), 1.0);
        cv.glyph(G_RETURN, 11.0, sr, WHITE, 1.0);
    } else {
        cv.glyph(G_RETURN, 11.0, sr, TEXT_SECTION, 0.8);
    }
    cx.hit(sr, submit);
    cx.hit(r, act);
}

fn alarm_card(cv: &mut Canvas, cx: &mut PageCtx, r: Rect, a: &Alarm) {
    let lang = cx.lang();
    let st = cx.st;
    let id = a.id;
    let on = a.enabled;
    cx.card(cv, r, RADIUS_ITEM, 0.0);
    let pad = 16.0;
    let fg = if on { TEXT_HERO } else { TEXT_DONE };

    // Time + steppers
    let time = format!("{:02}:{:02}", a.hour, a.minute);
    let tf = Font::text(30.0, 700, Align::Left);
    let tw = cv.text_width(&time, tf, TextStyle::TABULAR);
    cv.text_styled(&time, tf, Rect::new(r.x + pad, r.y + 10.0, tw + 4.0, 40.0), fg, 1.0, TextStyle::TABULAR);
    let sx = r.x + pad + tw + 14.0;
    let hr = t(lang, Key::HourShort);
    let w1 = stepper(cv, cx, sx, r.y + 17.0, hr, HubAction::AlarmShift(id, -60), HubAction::AlarmShift(id, 60));
    stepper(cv, cx, sx + w1 + 6.0, r.y + 17.0, t(lang, Key::MinuteShort), HubAction::AlarmShift(id, -5), HubAction::AlarmShift(id, 5));

    // Switch + delete
    let del = HubAction::AlarmDelete(id);
    let dr = Rect::new(r.right() - 12.0 - 28.0, r.y + 16.0, 28.0, 28.0);
    draw_delete_button(cv, dr, cx.hover_t(del));
    cx.hit(dr, del);
    let tog = HubAction::AlarmToggle(id);
    let swr = Rect::new(dr.x - 52.0, r.y + 10.0, 48.0, 40.0);
    draw_switch(cv, swr, on, cx.hover_t(tog));
    cx.hit(swr, tog);

    // Label · repeat · next ring
    let mut meta = String::new();
    if !a.label.is_empty() {
        meta.push_str(&a.label);
        meta.push_str(" · ");
    }
    meta.push_str(&repeat_label(lang, a.days));
    match a.next_ring(cx.now, st.offset_min) {
        Some(n) if on => {
            meta.push_str(" · ");
            meta.push_str(&fmt_due(n, false, cx.now, st.offset_min, lang));
        }
        _ => {
            meta.push_str(" · ");
            meta.push_str(t(lang, Key::AlarmOff));
        }
    }
    cv.text_styled(&meta, FONT_META, Rect::new(r.x + pad, r.y + 50.0, r.w - pad * 2.0, 18.0), if on { TEXT_BODY } else { TEXT_DONE }, 1.0, TextStyle::TABULAR);

    // Weekday chips (Mon … Sun)
    let (cw, gap) = (32.0, 6.0);
    for d in 0..7u8 {
        let cr = Rect::new(r.x + pad + d as f32 * (cw + gap), r.y + 76.0, cw, 26.0);
        let act = HubAction::AlarmDay(id, d);
        let sel = a.days & (1 << d) != 0;
        let h = cx.hover_t(act);
        if sel {
            cv.fill_round_linear(cr, 13.0, ACCENT_TINT, (cr.x, cr.y), (cr.right(), cr.bottom()), 1.0);
            cv.hairline_round(cr, 13.0, ACCENT_PRIMARY_HI, 0.5);
        } else {
            cv.fill_round(cr, 13.0, WHITE, 0.04 + 0.05 * h);
        }
        let lbl = weekday(lang, d as u32);
        cv.text(lbl, Font::text(10.5, if sel { 650 } else { 500 }, Align::Center), cr, if sel { TEXT_HERO } else { TEXT_BODY }, 1.0);
        cx.hit(cr, act);
    }
}

// ---- Countdown timer -----------------------------------------------------------------

pub fn draw_timer(cv: &mut Canvas, cx: &mut PageCtx, r: Rect, y: f32) {
    let lang = cx.lang();
    let cd = &cx.st.countdown;
    let compact = cx.compact;
    let main_w = if compact { r.w } else { (r.w - 14.0) * 0.55 };
    let avail = (r.bottom() - (y + cx.scroll)).max(0.0);
    let main_h = if compact { 320.0 } else { avail.max(340.0) };
    let main = Rect::new(r.x, y, main_w, main_h);
    cx.card(cv, main, RADIUS_CARD, 0.0);

    // Ring + buttons as one block, centred in the card.
    let ring_r = if compact { 84.0 } else { (main.h * 0.28).clamp(96.0, 132.0) };
    let (gap, bh) = (28.0, 40.0);
    let block_y = main.y + (main.h - (ring_r * 2.0 + gap + bh)) * 0.5;
    let (rcx, rcy) = (main.x + main.w * 0.5, block_y + ring_r);
    let prog = if cd.is_active() { 1.0 - cd.progress(cx.now) } else { 1.0 };
    if cd.is_running() {
        cv.fill_circle_radial(rcx, rcy, ring_r * 1.6, GLOW_AMBER, 0.4);
    }
    draw_progress_ring(cv, rcx, rcy, ring_r, 7.0, prog, ACCENT_AMBER, cd.is_running());
    let rem = fmt_hms(cd.remaining(cx.now));
    let big = if compact { 34.0 } else { (ring_r * 0.42).clamp(40.0, 54.0) };
    cv.text_styled(&rem, Font::text(big, 700, Align::Center), Rect::new(rcx - ring_r, rcy - big * 0.7, ring_r * 2.0, big * 1.4), TEXT_HERO, 1.0, TextStyle::TABULAR);
    let total = fmt_hms(cd.duration);
    cv.text_styled(&total, FONT_META_C, Rect::new(rcx - ring_r, rcy + big * 0.7, ring_r * 2.0, 18.0), TEXT_SECTION, 1.0, TextStyle::TABULAR);

    let running = cd.is_running();
    let items = [
        Btn { action: HubAction::CountdownAdd(-60), glyph: None, label: "−1′".into(), primary: false },
        Btn {
            action: HubAction::CountdownToggle,
            glyph: Some(if running { G_PAUSE } else { G_PLAY }),
            label: if running { t(lang, Key::Pause) } else { t(lang, Key::Start) }.into(),
            primary: true,
        },
        Btn { action: HubAction::CountdownAdd(60), glyph: None, label: "+1′".into(), primary: false },
    ];
    button_group(cv, cx, rcx, rcy + ring_r + gap, bh, &items);
    if cd.is_active() {
        let reset = HubAction::CountdownReset;
        let rr = Rect::new(main.right() - 16.0 - 36.0, main.y + 14.0, 36.0, 36.0);
        draw_icon_button(cv, rr, G_RESET, 12.0, cx.hover_t(reset), false, ACCENT_PRIMARY);
        cx.hit(rr, reset);
    }

    // Presets
    let pr = if compact {
        Rect::new(r.x, main.bottom() + 14.0, r.w, 150.0)
    } else {
        Rect::new(main.right() + 14.0, y, r.w - main_w - 14.0, main.h)
    };
    cx.card(cv, pr, RADIUS_CARD, 0.0);
    draw_section_header(cv, Rect::new(pr.x + 16.0, pr.y + 14.0, pr.w - 32.0, 16.0), t(lang, Key::TimerPresets));
    let presets: [u32; 9] = [1, 3, 5, 10, 15, 20, 30, 45, 60];
    let cols = if compact { 5 } else { 3 };
    let gap = 8.0;
    let cw = (pr.w - 32.0 - gap * (cols as f32 - 1.0)) / cols as f32;
    let rows = presets.len().div_ceil(cols) as f32;
    let ch = if compact { 38.0 } else { ((pr.h - 42.0 - 16.0 - gap * (rows - 1.0)) / rows).clamp(54.0, 96.0) };
    for (i, m) in presets.iter().enumerate() {
        let (row, col) = (i / cols, i % cols);
        let cr = Rect::new(pr.x + 16.0 + col as f32 * (cw + gap), pr.y + 42.0 + row as f32 * (ch + gap), cw, ch);
        if cr.bottom() > pr.bottom() - 8.0 {
            break;
        }
        let act = HubAction::CountdownSet(m * 60);
        let sel = !cd.is_active() && cd.duration == *m as i64 * MINUTE;
        let h = cx.hover_t(act);
        if sel {
            cv.fill_round(cr, RADIUS_BUTTON, ACCENT_AMBER, 0.16);
            cv.hairline_round(cr, RADIUS_BUTTON, ACCENT_AMBER, 0.5);
        } else {
            cv.fill_round(cr, RADIUS_BUTTON, WHITE, 0.04 + 0.05 * h);
            cv.hairline_round(cr, RADIUS_BUTTON, WHITE, 0.07);
        }
        let label = format!("{m} {}", t(lang, Key::MinuteShort));
        cv.text_styled(&label, Font::text(13.0, 600, Align::Center), cr, if sel { mix(ACCENT_AMBER, WHITE, 0.3) } else { TEXT_TITLE }, 1.0, TextStyle::TABULAR);
        cx.hit(cr, act);
    }
    cx.content_h = Some(pr.bottom().max(main.bottom()) + cx.scroll - r.y + 8.0);
}

// ---- Stopwatch -------------------------------------------------------------------------

pub fn draw_stopwatch(cv: &mut Canvas, cx: &mut PageCtx, r: Rect, mut y: f32) {
    let lang = cx.lang();
    let sw = &cx.st.stopwatch;
    // The card fills the page; with laps it takes the upper half and the list follows.
    let avail = (r.bottom() - (y + cx.scroll)).max(0.0);
    let has_laps = !sw.laps.is_empty();
    let card_h = if has_laps { (avail * 0.5).max(230.0) } else { avail.max(260.0) };
    let main = Rect::new(r.x, y, r.w, card_h);
    cx.card(cv, main, RADIUS_CARD, 0.0);

    // Time + buttons as one block, centred in the card.
    let time_h = if cx.compact { 64.0 } else { 84.0 };
    let (gap, bh) = (26.0, 42.0);
    let block_y = main.y + (main.h - (time_h + gap + bh)) * 0.5;
    let (ccx, ccy) = (main.x + main.w * 0.5, block_y + time_h * 0.5);
    if sw.is_running() {
        cv.fill_circle_radial(ccx, ccy, main.w.min(main.h) * 0.55, GLOW_ACCENT, 0.35);
    }
    let el = fmt_stopwatch(sw.elapsed(cx.now));
    cv.text_styled(&el, Font::text(if cx.compact { 48.0 } else { 64.0 }, 700, Align::Center), Rect::new(main.x, block_y, main.w, time_h), TEXT_HERO, 1.0, TextStyle::TABULAR);

    let running = sw.is_running();
    let mut items = Vec::new();
    if running {
        items.push(Btn { action: HubAction::StopwatchLap, glyph: Some(G_FLAG), label: t(lang, Key::Lap).into(), primary: false });
    }
    items.push(Btn {
        action: HubAction::StopwatchToggle,
        glyph: Some(if running { G_PAUSE } else { G_PLAY }),
        label: if running { t(lang, Key::Pause) } else { t(lang, Key::Start) }.into(),
        primary: true,
    });
    if sw.is_active() && !running {
        items.push(Btn { action: HubAction::StopwatchReset, glyph: Some(G_RESET), label: t(lang, Key::Reset).into(), primary: false });
    }
    button_group(cv, cx, ccx, block_y + time_h + gap, bh, &items);
    y = main.bottom() + 20.0;

    // Laps, newest first; best (mint) / worst (rose) once there are 3+.
    let splits = sw.lap_splits();
    if !splits.is_empty() {
        draw_section_header(cv, Rect::new(r.x, y, r.w, 18.0), t(lang, Key::Laps));
        y += 26.0;
        let best = splits.iter().copied().min().unwrap_or(0);
        let worst = splits.iter().copied().max().unwrap_or(0);
        let mark = splits.len() >= 3;
        for (i, (split, total)) in splits.iter().zip(sw.laps.iter()).enumerate().rev() {
            let row = Rect::new(r.x, y, r.w, 38.0);
            if y + 38.0 >= r.y - 10.0 && y <= r.bottom() + 10.0 {
                cv.fill_round(row, RADIUS_ITEM, WHITE, if i % 2 == 0 { 0.035 } else { 0.015 });
                let col = if mark && *split == best { ACCENT_MINT } else if mark && *split == worst { ACCENT_ROSE } else { TEXT_TITLE };
                let f = Font::text(13.0, 600, Align::Left);
                cv.text_styled(&format!("{} {}", t(lang, Key::Lap), i + 1), f, Rect::new(row.x + 16.0, row.y, 120.0, row.h), TEXT_BODY, 1.0, TextStyle::TABULAR);
                cv.text_styled(&fmt_stopwatch(*split), Font::text(13.0, 600, Align::Center), Rect::new(row.x, row.y, row.w, row.h), col, 1.0, TextStyle::TABULAR);
                cv.text_styled(&fmt_stopwatch(*total), Font::text(13.0, 500, Align::Right), Rect::new(row.x, row.y, row.w - 16.0, row.h), TEXT_SECTION, 1.0, TextStyle::TABULAR);
            }
            y += 42.0;
        }
    }
    cx.content_h = Some(y + cx.scroll - r.y + 8.0);
}

// ---- World clock -----------------------------------------------------------------------

fn fmt_offset_delta(mins: i32) -> String {
    let sign = if mins < 0 { "−" } else { "+" };
    let a = mins.abs();
    if a % 60 == 0 {
        format!("{sign}{}h", a / 60)
    } else {
        format!("{sign}{}h{:02}", a / 60, a % 60)
    }
}

pub fn draw_world(cv: &mut Canvas, cx: &mut PageCtx, r: Rect, mut y: f32) {
    let lang = cx.lang();
    let st = cx.st;
    let here_day = local_day(cx.now, st.offset_min);

    // Local time
    let lr = Rect::new(r.x, y, r.w, 76.0);
    cx.card(cv, lr, RADIUS_CARD, 0.0);
    cv.fill_circle_radial(lr.x + 30.0, lr.y + 38.0, 40.0, GLOW_ACCENT, 0.5);
    cv.glyph(G_GLOBE, 16.0, Rect::new(lr.x + 16.0, lr.y, 28.0, lr.h), ACCENT_PRIMARY_HI, 1.0);
    cv.text(t(lang, Key::LocalTime), Font::text(14.0, 600, Align::Left), Rect::new(lr.x + 54.0, lr.y + 16.0, lr.w - 200.0, 20.0), TEXT_TITLE, 1.0);
    let utc = fmt_offset_delta(st.offset_min);
    cv.text_styled(&format!("UTC{utc}"), FONT_META, Rect::new(lr.x + 54.0, lr.y + 38.0, lr.w - 200.0, 18.0), TEXT_SECTION, 1.0, TextStyle::TABULAR);
    let c = civil(cx.now, st.offset_min);
    cv.text_styled(&format!("{:02}:{:02}", c.hour, c.minute), Font::text(30.0, 700, Align::Right), Rect::new(lr.x, lr.y, lr.w - 18.0, lr.h), TEXT_HERO, 1.0, TextStyle::TABULAR);
    y = lr.bottom() + 12.0;

    let clocks = &st.data.world_clocks;
    if clocks.is_empty() {
        cv.text(t(lang, Key::NoCities), Font::text(12.5, 450, Align::Center), Rect::new(r.x, y + 10.0, r.w, 22.0), TEXT_BODY, 0.9);
        y += 44.0;
    }
    for (i, w) in clocks.iter().enumerate() {
        let row = Rect::new(r.x, y, r.w, 68.0);
        let rm = HubAction::WorldRemove(i as u8);
        let hot = cx.hover == Some(rm) || cx.mouse.is_some_and(|(mx, my)| row.contains(mx, my));
        cx.card(cv, row, RADIUS_ITEM, if hot { 0.6 } else { 0.0 });
        cv.text(&w.city, Font::text(15.0, 650, Align::Left), Rect::new(row.x + 18.0, row.y + 12.0, row.w - 220.0, 22.0), TEXT_HERO, 1.0);
        match st.world_offsets.get(i).copied().flatten() {
            Some(off) => {
                let wc = civil(cx.now, off);
                let day = match local_day(cx.now, off) - here_day {
                    d if d < 0 => t(lang, Key::Yesterday),
                    0 => t(lang, Key::BucketToday),
                    _ => t(lang, Key::BucketTomorrow),
                };
                let sub = format!("{day} · {}", fmt_offset_delta(off - st.offset_min));
                cv.text_styled(&sub, FONT_META, Rect::new(row.x + 18.0, row.y + 36.0, row.w - 220.0, 18.0), TEXT_BODY, 1.0, TextStyle::TABULAR);
                let right = if hot { row.right() - 52.0 } else { row.right() - 18.0 };
                cv.text_styled(&format!("{:02}:{:02}", wc.hour, wc.minute), Font::text(26.0, 700, Align::Right), Rect::new(row.x, row.y, right - row.x, row.h), TEXT_HERO, 1.0, TextStyle::TABULAR);
            }
            None => {
                cv.text("—", Font::text(20.0, 600, Align::Right), Rect::new(row.x, row.y, row.w - 52.0, row.h), TEXT_SECTION, 1.0);
            }
        }
        let dr = Rect::new(row.right() - 40.0, row.y + 20.0, 28.0, 28.0);
        if hot {
            draw_delete_button(cv, dr, cx.hover_t(rm));
        }
        cx.hit(dr, rm);
        y += 76.0;
    }

    // Add a city: presets not added yet, as wrapping chips.
    y += 8.0;
    draw_section_header(cv, Rect::new(r.x, y, r.w, 18.0), t(lang, Key::AddCity));
    y += 26.0;
    let mut x = r.x;
    let f = Font::text(12.0, 550, Align::Left);
    for (i, (city, tz)) in CITY_PRESETS.iter().enumerate() {
        if clocks.iter().any(|w| w.tz == *tz) {
            continue;
        }
        let w = cv.text_width(city, f, TextStyle::default()) + 40.0;
        if x + w > r.right() && x > r.x {
            x = r.x;
            y += 40.0;
        }
        let cr = Rect::new(x, y, w, 32.0);
        let act = HubAction::WorldAdd(i as u8);
        let h = cx.hover_t(act);
        cv.fill_round(cr, 16.0, WHITE, 0.04 + 0.06 * h);
        cv.hairline_round(cr, 16.0, if h > 0.0 { ACCENT_PRIMARY_HI } else { WHITE }, if h > 0.0 { 0.5 } else { 0.08 });
        cv.glyph(G_ADD, 9.0, Rect::new(cr.x + 8.0, cr.y, 16.0, cr.h), if h > 0.0 { ACCENT_PRIMARY_HI } else { TEXT_SECTION }, 1.0);
        cv.text(city, f, Rect::new(cr.x + 26.0, cr.y, cr.w - 30.0, cr.h), if h > 0.0 { TEXT_HERO } else { TEXT_BODY }, 1.0);
        cx.hit(cr, act);
        x += w + 8.0;
    }
    cx.content_h = Some(y + 40.0 + cx.scroll - r.y);
}
