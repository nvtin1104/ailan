//! Clock tools on the Reminders page: alarms and countdown timer.

use ailan_core::clock::{fmt_hms, Alarm, EVERY_DAY, WEEKDAYS};
use ailan_core::i18n::{t, weekday, Key, Lang};
use ailan_core::reminder::fmt_due;

use crate::gfx::{Align, Canvas, Font, Rect, TextStyle};
use crate::hub::widgets::*;
use crate::hub::{Field, HubAction, PageCtx};
use crate::ui::{G_ADD, G_BELL, G_CHECKMARK, G_PAUSE, G_PLAY, G_RESET};

const G_MINUS: char = '\u{E738}';
const G_EDIT: char = '\u{E70F}';

/// Tab labels on the Reminders page.
pub fn tab_labels(lang: Lang) -> [&'static str; 3] {
    [
        t(lang, Key::ClockReminders),
        t(lang, Key::ClockAlarms),
        t(lang, Key::ClockTimer),
    ]
}

const TAB_ICONS: [char; 3] = ['\u{EA8F}', '\u{E917}', '\u{E823}'];

/// Reminders page tab bar. Returns the y below it.
pub fn draw_tabs(cv: &mut Canvas, cx: &mut PageCtx, r: Rect, y: f32) -> f32 {
    let labels = tab_labels(cx.lang());
    let items: Vec<(&str, Option<char>)> = labels.iter().zip(TAB_ICONS).map(|(l, g)| (*l, Some(g))).collect();
    let hover = (0..3).position(|i| cx.hover == Some(HubAction::SetClockTab(i as u8)));
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

/// Lay buttons out as one block centred on `center_x`, clamped to `max_w`.
fn button_group(cv: &mut Canvas, cx: &mut PageCtx, center_x: f32, y: f32, h: f32, max_w: f32, items: &[Btn]) {
    let f = Font::text(13.0, 650, Align::Left);
    let mut widths: Vec<f32> = items
        .iter()
        .map(|b| {
            let tw = cv.text_width(&b.label, f, TextStyle::TABULAR);
            let icon = if b.glyph.is_some() { 20.0 } else { 0.0 };
            (tw + icon + 26.0).max(if b.primary { 104.0 } else { 44.0 })
        })
        .collect();
    let gap = 8.0;
    let gaps = gap * (items.len().saturating_sub(1)) as f32;
    let sum_w: f32 = widths.iter().sum();
    let avail = (max_w - gaps).max(80.0);
    if sum_w > avail {
        let scale = avail / sum_w;
        for w in &mut widths {
            *w = (*w * scale).max(36.0);
        }
    }
    let total: f32 = widths.iter().sum::<f32>() + gaps;
    let mut x = center_x - total * 0.5;
    for (b, w) in items.iter().zip(widths) {
        let br = Rect::new(x, y, w, h);
        let kind = if b.primary { ButtonKind::Primary } else { ButtonKind::Ghost };
        draw_button(cv, br, "", kind, cx.hover_t(b.action));
        let fg = if b.primary { BG_DEEP_CANVAS } else { TEXT_TITLE };
        let tw = cv.text_width(&b.label, f, TextStyle::TABULAR);
        let icon_gap = if b.glyph.is_some() { 20.0 } else { 0.0 };
        let content = (tw + icon_gap).min(w - 8.0);
        let mut cx0 = br.x + (w - content) * 0.5;
        if let Some(g) = b.glyph {
            cv.glyph(g, 10.5, Rect::new(cx0, br.y, 16.0, br.h), fg, 1.0);
            cx0 += 20.0;
        }
        cv.text_styled(&b.label, f, Rect::new(cx0, br.y, (tw + 2.0).min(br.right() - cx0), br.h), fg, 1.0, TextStyle::TABULAR);
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
        cv.glyph(G_BELL, 26.0, Rect::new(er.x, er.y + 46.0, er.w, 48.0), TEXT_BODY, 0.9);
        cv.text(t(lang, Key::NoAlarms), Font::text(15.0, 650, Align::Center), Rect::new(er.x, er.y + 104.0, er.w, 22.0), TEXT_HERO, 1.0);
        cv.text(t(lang, Key::AlarmHint), Font::text(12.0, 400, Align::Center), Rect::new(er.x, er.y + 128.0, er.w, 18.0), TEXT_BODY, 0.9);
        cx.content_h = Some(y + cx.scroll - r.y + 240.0);
        return;
    }
    let card_h = 138.0;
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
    let add_w = 88.0;
    let gap = 8.0;
    let ir = Rect::new(r.x, r.y, (r.w - add_w - gap).max(120.0), r.h);
    let focused = cx.focus == Some(Field::Alarm);
    let act = HubAction::FocusField(Field::Alarm);
    draw_text_input(cv, ir, cx.alarm_input, t(lang, Key::AlarmHint), focused, cx.hover_t(act), Font::text(13.5, 450, Align::Left), Some(G_BELL), 12.0);
    cx.hit(ir, act);

    let br = Rect::new(ir.right() + gap, r.y + 3.0, add_w, r.h - 6.0);
    let add_act = HubAction::AlarmNew;
    draw_button(cv, br, "", ButtonKind::Primary, cx.hover_t(add_act));
    let f = Font::text(12.5, 650, Align::Center);
    let label = format!("+ {}", t(lang, Key::AddAlarm));
    cv.text(&label, f, br, BG_DEEP_CANVAS, 1.0);
    cx.hit(br, add_act);
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

    // Inline editable label (nội dung)
    let lr = Rect::new(r.x + pad, r.y + 52.0, r.w - pad * 2.0, 26.0);
    let edit_field = Field::AlarmLabel(id);
    if cx.focus == Some(edit_field) {
        draw_text_input(
            cv,
            lr,
            cx.alarm_label_input,
            t(lang, Key::AlarmLabelHint),
            true,
            0.0,
            Font::text(12.5, 500, Align::Left),
            Some(G_EDIT),
            34.0,
        );
        let save = HubAction::SubmitField(edit_field);
        let sr = Rect::new(lr.right() - 28.0, lr.y + 2.0, 24.0, 22.0);
        draw_icon_button(cv, sr, G_CHECKMARK, 10.0, cx.hover_t(save), true, ACCENT_PRIMARY);
        cx.hit(sr, save);
        cx.hit(lr, HubAction::FocusField(edit_field));
    } else {
        let foc = HubAction::FocusField(edit_field);
        let h = cx.hover_t(foc);
        cv.fill_round(lr, 8.0, WHITE, 0.025 + 0.04 * h);
        cv.glyph(G_EDIT, 10.0, Rect::new(lr.x + 8.0, lr.y, 16.0, lr.h), if h > 0.0 { ACCENT_PRIMARY_HI } else { TEXT_SECTION }, 0.9);
        let (lbl_txt, lbl_col) = if a.label.is_empty() {
            (t(lang, Key::AlarmLabelHint), TEXT_SECTION)
        } else {
            (a.label.as_str(), if on { TEXT_TITLE } else { TEXT_DONE })
        };
        cv.text(lbl_txt, Font::text(12.5, 500, Align::Left), Rect::new(lr.x + 28.0, lr.y, lr.w - 36.0, lr.h), lbl_col, 1.0);
        cx.hit(lr, foc);
    }

    // Repeat · next ring
    let mut meta = repeat_label(lang, a.days);
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
    cv.text_styled(&meta, FONT_META, Rect::new(r.x + pad, r.y + 82.0, r.w - pad * 2.0, 16.0), if on { TEXT_BODY } else { TEXT_DONE }, 1.0, TextStyle::TABULAR);

    // Weekday chips (Mon … Sun)
    let (cw, gap) = (32.0, 6.0);
    for d in 0..7u8 {
        let cr = Rect::new(r.x + pad + d as f32 * (cw + gap), r.y + 102.0, cw, 26.0);
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

// ---- Countdown timer (monochrome draggable analog clock + Start + Save) ----------------

fn draw_analog_clock(
    cv: &mut Canvas,
    cx: &mut PageCtx,
    cx_pos: f32,
    cy_pos: f32,
    radius: f32,
    rem_ms: i64,
    prog: f32,
    running: bool,
) {
    use std::f32::consts::{FRAC_PI_2, TAU};

    // Subtle outer dial ring
    cv.stroke_circle(cx_pos, cy_pos, radius, 1.2, WHITE, 0.14);

    // 12 clean hour ticks (4 cardinals slightly longer)
    for i in 0..12 {
        let is_cardinal = i % 3 == 0;
        let ang = (i as f32 / 12.0) * TAU - FRAC_PI_2;
        let (ca, sa) = (ang.cos(), ang.sin());
        let r_out = radius - 6.0;
        let r_in = r_out - if is_cardinal { 10.0 } else { 5.5 };
        cv.stroke_polyline(
            &[(cx_pos + ca * r_in, cy_pos + sa * r_in), (cx_pos + ca * r_out, cy_pos + sa * r_out)],
            1.0,
            if is_cardinal { 1.8 } else { 1.2 },
            WHITE,
            if is_cardinal { 0.60 } else { 0.28 },
        );
    }

    // Monochrome progress arc around the dial
    let sweep = 360.0 * prog.clamp(0.0, 1.0);
    if sweep > 0.5 {
        cv.stroke_arc(cx_pos, cy_pos, radius, -90.0, sweep, 2.6, WHITE, if running { 0.92 } else { 0.55 });
    }

    // Hands driven by remaining time: hour, minute, and second
    let total_s = (rem_ms.max(0) as f32) / 1000.0;
    let hr_ang = (((total_s / 3600.0) % 12.0) / 12.0) * TAU - FRAC_PI_2;
    let min_ang = (((total_s / 60.0) % 60.0) / 60.0) * TAU - FRAC_PI_2;
    let sec_ang = ((total_s % 60.0) / 60.0) * TAU - FRAC_PI_2;

    // Hour hand
    cv.stroke_polyline(
        &[
            (cx_pos, cy_pos),
            (cx_pos + hr_ang.cos() * radius * 0.46, cy_pos + hr_ang.sin() * radius * 0.46),
        ],
        1.0,
        3.6,
        TEXT_BODY,
        0.90,
    );
    // Minute hand (no dot at tip)
    cv.stroke_polyline(
        &[
            (cx_pos, cy_pos),
            (cx_pos + min_ang.cos() * radius * 0.70, cy_pos + min_ang.sin() * radius * 0.70),
        ],
        1.0,
        2.6,
        WHITE,
        0.95,
    );
    // Second hand with counterweight tail
    cv.stroke_polyline(
        &[
            (cx_pos - sec_ang.cos() * radius * 0.14, cy_pos - sec_ang.sin() * radius * 0.14),
            (cx_pos + sec_ang.cos() * radius * 0.80, cy_pos + sec_ang.sin() * radius * 0.80),
        ],
        1.0,
        1.4,
        WHITE,
        0.65,
    );

    // Center pivot
    cv.fill_circle(cx_pos, cy_pos, 4.0, WHITE, 1.0);
    cv.fill_circle(cx_pos, cy_pos, 1.6, BG_DEEP_CANVAS, 1.0);

    // Direct-edit digital readout tucked below center pivot (hit registered BEFORE TimerDrag)
    let dr = Rect::new(cx_pos - 46.0, cy_pos + radius * 0.24, 92.0, 28.0);
    let tf = Font::text(15.0, 700, Align::Center);
    if cx.focus == Some(Field::TimerInput) {
        cv.fill_round(dr, 7.0, WHITE, 0.08);
        cv.hairline_round(dr, 7.0, WHITE, 0.35);
        let disp = if cx.timer_input.is_empty() { "00:00" } else { cx.timer_input };
        let col = if cx.timer_input.is_empty() { TEXT_SECTION } else { TEXT_HERO };
        cv.text_styled(disp, tf, dr, col, 1.0, TextStyle::TABULAR);
        let tw = if cx.timer_input.is_empty() {
            0.0
        } else {
            cv.text_width(cx.timer_input, tf, TextStyle::TABULAR)
        };
        let caret_x = (cx_pos + tw * 0.5 + 2.0).min(dr.right() - 6.0);
        cv.fill_round(Rect::new(caret_x, dr.y + 6.0, 1.5, dr.h - 12.0), 0.75, WHITE, 0.95);
        cx.hit(dr, HubAction::FocusField(Field::TimerInput));
    } else {
        let rem = fmt_hms(rem_ms);
        cv.text_styled(&rem, tf, dr, TEXT_HERO, 0.95, TextStyle::TABULAR);
        cx.hit(dr, HubAction::FocusField(Field::TimerInput));
    }

    // Dial drag hit target (registered after digital readout so clicking the text edits it)
    *cx.clock_dial = Some((cx_pos, cy_pos, radius));
    cx.hit(Rect::new(cx_pos - radius - 10.0, cy_pos - radius - 10.0, (radius + 10.0) * 2.0, (radius + 10.0) * 2.0), HubAction::TimerDrag);
}

fn fmt_preset_dur(lang: Lang, secs: u32) -> String {
    if secs % 3600 == 0 && secs >= 3600 {
        format!("{}{}", secs / 3600, t(lang, Key::HourShort))
    } else if secs % 60 == 0 {
        format!("{}{}", secs / 60, t(lang, Key::MinuteShort))
    } else {
        fmt_hms(secs as i64 * 1000)
    }
}

pub fn draw_timer(cv: &mut Canvas, cx: &mut PageCtx, r: Rect, mut y: f32) {
    let lang = cx.lang();
    let cd = &cx.st.countdown;
    let compact = cx.compact;
    let running = cd.is_running();

    // 1. Monochrome draggable analog clock (no card)
    let clock_r = if compact { 86.0 } else { 102.0 };
    let rcx = r.x + r.w * 0.5;
    let rcy = y + 8.0 + clock_r;
    let prog = if cd.is_active() { 1.0 - cd.progress(cx.now) } else { 1.0 };
    draw_analog_clock(cv, cx, rcx, rcy, clock_r, cd.remaining(cx.now), prog, running);

    if cd.is_active() {
        let reset = HubAction::CountdownReset;
        let rr = Rect::new(rcx + clock_r + 12.0, rcy - clock_r, 34.0, 34.0);
        if rr.right() <= r.right() {
            draw_icon_button(cv, rr, G_RESET, 12.0, cx.hover_t(reset), false, WHITE);
            cx.hit(rr, reset);
        }
    }

    y = rcy + clock_r + 14.0;

    if !cd.label.is_empty() {
        cv.text(&cd.label, Font::text(12.5, 600, Align::Center), Rect::new(r.x, y, r.w, 18.0), TEXT_BODY, 0.9);
        y += 24.0;
    } else {
        y += 8.0;
    }

    // 2. Start + Save buttons only (no -1' / +1')
    let items = [
        Btn {
            action: HubAction::CountdownToggle,
            glyph: Some(if running { G_PAUSE } else { G_PLAY }),
            label: if running { t(lang, Key::Pause) } else { t(lang, Key::Start) }.into(),
            primary: true,
        },
        Btn {
            action: HubAction::TimerSaveToggle,
            glyph: None,
            label: t(lang, Key::SavePreset).into(),
            primary: false,
        },
    ];
    button_group(cv, cx, rcx, y, 38.0, r.w.min(320.0), &items);
    y += 38.0 + 12.0;

    // 3. Animated Save-label input (slides/fades open when clicking Save)
    let form_w = r.w.min(420.0);
    let form_x = r.x + (r.w - form_w) * 0.5;
    let t_anim = (cx.timer_save_anim / 100.0).clamp(0.0, 1.0);
    if t_anim > 0.01 {
        let ease = 1.0 - (1.0 - t_anim) * (1.0 - t_anim);
        let box_h = 40.0 * ease;
        let prev_a = cv.alpha;
        cv.alpha = prev_a * ease;
        let ir = Rect::new(form_x, y, form_w, box_h.max(24.0));
        draw_text_input(
            cv,
            ir,
            cx.timer_label_input,
            t(lang, Key::TimerLabelHint),
            cx.focus == Some(Field::TimerPresetLabel),
            cx.hover_t(HubAction::FocusField(Field::TimerPresetLabel)),
            Font::text(13.0, 450, Align::Left),
            None,
            38.0,
        );
        let save = HubAction::SubmitField(Field::TimerPresetLabel);
        let sr = Rect::new(ir.right() - 32.0, ir.y + (ir.h - 24.0) * 0.5, 26.0, 24.0);
        draw_icon_button(cv, sr, G_CHECKMARK, 10.0, cx.hover_t(save), true, WHITE);
        cx.hit(sr, save);
        cx.hit(ir, HubAction::FocusField(Field::TimerPresetLabel));
        cv.alpha = prev_a;
        y += box_h + 14.0 * ease;
    } else {
        y += 6.0;
    }

    // 4. Monochrome saved presets with labels (no card)
    let presets = &cx.st.data.timer_presets;
    if !presets.is_empty() {
        let mut px = form_x;
        let max_x = form_x + form_w;
        let f = Font::text(12.0, 550, Align::Left);
        for (i, p) in presets.iter().enumerate() {
            let dur = fmt_preset_dur(lang, p.seconds);
            let txt = format!("{dur} · {}", p.label);
            let rm_space = if presets.len() > 1 { 20.0 } else { 0.0 };
            let cw = (cv.text_width(&txt, f, TextStyle::TABULAR) + 26.0 + rm_space).min(form_w);
            if px + cw > max_x && px > form_x {
                px = form_x;
                y += 40.0;
            }
            let cr = Rect::new(px, y, cw, 32.0);
            let set_act = HubAction::CountdownSetPreset(i as u8);
            let rm_act = HubAction::TimerPresetRemove(i as u8);
            let sel = !cd.is_active() && cd.duration == p.seconds as i64 * 1000 && (cd.label.is_empty() || cd.label == p.label);
            let h = cx.hover_t(set_act);
            let h_rm = cx.hover_t(rm_act);
            if sel {
                cv.fill_round(cr, 16.0, WHITE, 0.14);
                cv.hairline_round(cr, 16.0, WHITE, 0.55);
            } else {
                cv.fill_round(cr, 16.0, WHITE, 0.04 + 0.05 * (h.max(h_rm)));
                cv.hairline_round(cr, 16.0, WHITE, 0.10);
            }
            cv.text_styled(
                &txt,
                f,
                Rect::new(cr.x + 12.0, cr.y, cr.w - 22.0 - rm_space, cr.h),
                if sel { TEXT_HERO } else { TEXT_TITLE },
                1.0,
                TextStyle::TABULAR,
            );
            if presets.len() > 1 {
                let rr = Rect::new(cr.right() - 22.0, cr.y + 6.0, 18.0, 20.0);
                cv.glyph(G_CHROME_CLOSE, 8.0, rr, if h_rm > 0.0 { TEXT_HERO } else { TEXT_SECTION }, 0.7 + 0.3 * h_rm);
                cx.hit(rr, rm_act);
            }
            cx.hit(cr, set_act);
            px += cw + 8.0;
        }
        y += 40.0;
    }

    cx.content_h = Some(y + cx.scroll - r.y + 8.0);
}
