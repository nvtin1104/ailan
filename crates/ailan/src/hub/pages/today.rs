//! Today dashboard — a Bento grid:
//! ┌ Hero (greeting + progress ring) ┬ Focus Hub (glowing countdown arc) ┐
//! ├ Quick Capture (live @time highlighting) ─────────────────────────────┤
//! └ Today's tasks                   ┴ Upcoming reminders timeline ───────┘
//! In Compact mode the same blocks stack vertically.

use ailan_core::i18n::{short_date, t, Key};
use ailan_core::reminder::{day_bucket, fmt_due, DayBucket};
use ailan_core::settings::Tab;
use ailan_core::time::{civil, fmt_duration, local_day};
use ailan_core::timer::Phase;
use ailan_core::todo::Todo;

use crate::gfx::{Align, Canvas, Font, Rect, TextStyle};
use crate::hub::pages::todos::{capture_bar, todo_row};
use crate::hub::widgets::*;
use crate::hub::{HubAction, PageCtx};
use crate::ui::{G_BELL, G_PAUSE, G_PIN, G_PLAY, G_RESET, G_TIMER};

const GAP: f32 = 14.0;

struct DayStats {
    done: usize,
    total: usize,
}

fn day_stats(cx: &PageCtx) -> DayStats {
    let st = cx.st;
    let today = local_day(cx.now, st.offset_min);
    let done = st.data.todos.items.iter().filter(|t| t.done_at.is_some_and(|d| local_day(d, st.offset_min) == today)).count();
    let pending = st.data.todos.today_items(cx.now, st.offset_min).iter().filter(|t| !t.done).count();
    DayStats { done, total: done + pending }
}

pub fn draw(cv: &mut Canvas, cx: &mut PageCtx, r: Rect) {
    if r.w >= 560.0 {
        draw_bento(cv, cx, r);
    } else {
        draw_stacked(cv, cx, r);
    }
}

fn draw_bento(cv: &mut Canvas, cx: &mut PageCtx, r: Rect) {
    let col = (r.w - GAP) * 0.5;
    let hero = Rect::new(r.x, r.y, col, 172.0);
    let focus = Rect::new(r.x + col + GAP, r.y, col, 172.0);
    draw_hero(cv, cx, hero, false);
    draw_focus_hub(cv, cx, focus, false);

    let cap = Rect::new(r.x, hero.bottom() + GAP, r.w, 48.0);
    capture_bar(cv, cx, cap, t(cx.lang(), Key::CaptureHint));

    let by = cap.bottom() + GAP;
    let bh = (r.bottom() - by).max(120.0);
    let lw = ((r.w - GAP) * 0.56).floor();
    draw_task_list(cv, cx, Rect::new(r.x, by, lw, bh));
    draw_timeline(cv, cx, Rect::new(r.x + lw + GAP, by, r.w - lw - GAP, bh));
}

fn draw_stacked(cv: &mut Canvas, cx: &mut PageCtx, r: Rect) {
    let hero = Rect::new(r.x, r.y, r.w, 96.0);
    draw_hero(cv, cx, hero, true);
    let focus = Rect::new(r.x, hero.bottom() + 10.0, r.w, 92.0);
    draw_focus_hub(cv, cx, focus, true);
    let cap = Rect::new(r.x, focus.bottom() + 10.0, r.w, 44.0);
    capture_bar(cv, cx, cap, t(cx.lang(), Key::CaptureHint));
    let ly = cap.bottom() + 10.0;
    draw_task_list(cv, cx, Rect::new(r.x, ly, r.w, (r.bottom() - ly).max(100.0)));
}

fn draw_hero(cv: &mut Canvas, cx: &mut PageCtx, r: Rect, compact: bool) {
    let lang = cx.lang();
    let st = cx.st;
    cx.card(cv, r, RADIUS_CARD, 0.0);
    let c = civil(cx.now, st.offset_min);
    let greeting = if c.hour < 12 {
        t(lang, Key::GreetingMorning)
    } else if c.hour < 18 {
        t(lang, Key::GreetingAfternoon)
    } else {
        t(lang, Key::GreetingEvening)
    };
    let s = day_stats(cx);
    let progress = if s.total == 0 { 0.0 } else { s.done as f32 / s.total as f32 };
    let ring_rgb = if s.total > 0 && s.done == s.total { ACCENT_MINT } else { ACCENT_PRIMARY_HI };
    let ring_r = if compact { 27.0 } else { 42.0 };
    let (rcx, rcy) = (r.right() - ring_r - if compact { 18.0 } else { 26.0 }, r.y + r.h * 0.5);
    let text_w = rcx - ring_r - r.x - 30.0;

    let pad = if compact { 16.0 } else { 20.0 };
    draw_section_header(cv, Rect::new(r.x + pad, r.y + pad - 2.0, text_w, 16.0), &short_date(lang, &c));
    let gfont = if compact { Font::text(18.0, 700, Align::Left) } else { FONT_HERO };
    cv.text(greeting, gfont, Rect::new(r.x + pad, r.y + pad + 14.0, text_w, 30.0), TEXT_HERO, 1.0);

    let summary = format!("{}/{} {}", s.done, s.total, t(lang, Key::CompletedToday));
    let sy = r.y + pad + if compact { 44.0 } else { 50.0 };
    cv.text_styled(&summary, Font::text(13.0, 500, Align::Left), Rect::new(r.x + pad, sy, text_w, 18.0), TEXT_BODY, 1.0, TextStyle::TABULAR);

    if !compact {
        // Next reminder teaser
        if let Some(next) = st.data.todos.reminder_items().into_iter().find(|t| !t.done) {
            if let Some(due) = next.snoozed_until.or(next.due) {
                let ny = r.bottom() - pad - 30.0;
                let nr = Rect::new(r.x + pad, ny, text_w, 30.0);
                cv.fill_round(nr, RADIUS_BUTTON, WHITE, 0.035);
                cv.glyph(G_BELL, 11.0, Rect::new(nr.x + 6.0, nr.y, 20.0, nr.h), ACCENT_AMBER, 1.0);
                let when = fmt_due(due, next.all_day, cx.now, st.offset_min, lang);
                let ww = cv.text_width(&when, FONT_META, TextStyle::TABULAR);
                cv.text_styled(&when, FONT_META, Rect::new(nr.x + 30.0, nr.y, ww + 2.0, nr.h), ACCENT_AMBER, 1.0, TextStyle::TABULAR);
                cv.text(&next.text, Font::text(12.0, 500, Align::Left), Rect::new(nr.x + 38.0 + ww, nr.y, (nr.w - 46.0 - ww).max(0.0), nr.h), TEXT_TITLE, 1.0);
            }
        }
    }

    draw_progress_ring(cv, rcx, rcy, ring_r, if compact { 5.0 } else { 7.0 }, progress, ring_rgb, true);
    let pct = format!("{}%", (progress * 100.0).round() as u32);
    let pf = Font::text(if compact { 13.0 } else { 17.0 }, 700, Align::Center);
    cv.text_styled(&pct, pf, Rect::new(rcx - ring_r, rcy - 11.0, ring_r * 2.0, 22.0), TEXT_HERO, 1.0, TextStyle::TABULAR);
}

fn draw_focus_hub(cv: &mut Canvas, cx: &mut PageCtx, r: Rect, compact: bool) {
    let lang = cx.lang();
    let st = cx.st;
    let p = &st.pomodoro;
    cx.card(cv, r, RADIUS_CARD, 0.0);
    let (phase_lbl, tint) = match p.phase {
        Phase::Focus => (t(lang, Key::Focus), ACCENT_PRIMARY_HI),
        Phase::ShortBreak => (t(lang, Key::ShortBreak), ACCENT_MINT),
        Phase::LongBreak => (t(lang, Key::LongBreak), ACCENT_MINT),
    };
    let ring_r = if compact { 30.0 } else { 46.0 };
    let pad = if compact { 16.0 } else { 20.0 };
    let (rcx, rcy) = (r.x + pad + ring_r + 4.0, r.y + if compact { r.h * 0.5 } else { r.h * 0.5 + 8.0 });

    if p.is_running() {
        cv.fill_circle_radial(rcx, rcy, ring_r * 1.9, if tint == ACCENT_MINT { GLOW_MINT } else { GLOW_ACCENT }, 0.35);
    }
    let prog = if p.is_active() { p.progress(cx.now) } else { 0.0 };
    draw_progress_ring(cv, rcx, rcy, ring_r, if compact { 4.5 } else { 6.0 }, prog, tint, true);
    let time = fmt_duration(p.remaining(cx.now));
    let tf = Font::text(if compact { 14.0 } else { 20.0 }, 700, Align::Center);
    cv.text_styled(&time, tf, Rect::new(rcx - ring_r, rcy - 13.0, ring_r * 2.0, 26.0), TEXT_HERO, 1.0, TextStyle::TABULAR);

    let x0 = rcx + ring_r + if compact { 18.0 } else { 26.0 };
    let pin_act = HubAction::PinFocus;
    let pinned = st.settings.is_pinned(Tab::Focus);
    let pin_r = Rect::new(r.right() - pad - 30.0 + 6.0, r.y + pad - 6.0, 30.0, 30.0);
    draw_icon_button(cv, pin_r, G_PIN, 12.0, cx.hover_t(pin_act), pinned, ACCENT_PRIMARY);
    cx.hit(pin_r, pin_act);

    if !compact {
        cv.glyph(G_TIMER, 12.0, Rect::new(r.x + pad - 2.0, r.y + pad - 4.0, 18.0, 20.0), tint, 1.0);
        draw_section_header(cv, Rect::new(r.x + pad + 20.0, r.y + pad - 2.0, 160.0, 16.0), phase_lbl);
        // Streak: sessions done today vs. the long-break cycle goal.
        let today = local_day(cx.now, st.offset_min);
        let sessions = st.data.focus_count(today) as usize;
        let goal = (p.durations.long_every.max(1) as usize).max(sessions).min(8);
        let sy = r.y + 56.0;
        cv.text_styled(&format!("{sessions}"), Font::text(20.0, 700, Align::Left), Rect::new(x0, sy - 4.0, 40.0, 26.0), TEXT_HERO, 1.0, TextStyle::TABULAR);
        let nw = cv.text_width(&format!("{sessions}"), Font::text(20.0, 700, Align::Left), TextStyle::TABULAR);
        cv.text(t(lang, Key::SessionsToday), FONT_META, Rect::new(x0 + nw + 6.0, sy, r.right() - x0 - nw - 26.0, 20.0), TEXT_BODY, 1.0);
        for i in 0..goal {
            let dx = x0 + 5.0 + i as f32 * 15.0;
            if i < sessions {
                cv.fill_circle(dx, sy + 30.0, 4.5, ACCENT_MINT, 1.0);
            } else {
                cv.stroke_circle(dx, sy + 30.0, 4.0, 1.2, WHITE, 0.2);
            }
        }
    } else {
        cv.text(phase_lbl, Font::text(12.5, 600, Align::Left), Rect::new(x0, r.y + 18.0, r.right() - x0 - 50.0, 18.0), tint, 1.0);
    }

    // Controls
    let by = if compact { r.bottom() - 16.0 - 30.0 } else { r.bottom() - pad - 32.0 };
    let toggle = HubAction::PomoToggle;
    let label = if p.is_running() { t(lang, Key::Pause) } else { t(lang, Key::Start) };
    let tw = if compact { 84.0 } else { 96.0 };
    let tr = Rect::new(x0, by, tw, 32.0);
    draw_button(cv, tr, "", ButtonKind::Primary, cx.hover_t(toggle));
    cv.glyph(if p.is_running() { G_PAUSE } else { G_PLAY }, 10.0, Rect::new(tr.x + 8.0, tr.y, 22.0, tr.h), WHITE, 1.0);
    cv.text(label, Font::text(12.0, 600, Align::Left), Rect::new(tr.x + 30.0, tr.y, tr.w - 34.0, tr.h), WHITE, 1.0);
    cx.hit(tr, toggle);
    let skip = HubAction::PomoSkip;
    let skr = Rect::new(tr.right() + 8.0, by, 32.0, 32.0);
    draw_button(cv, skr, "", ButtonKind::Ghost, cx.hover_t(skip));
    cv.glyph(G_SKIP, 10.0, skr, TEXT_TITLE, 1.0);
    cx.hit(skr, skip);
    let reset = HubAction::PomoReset;
    let rsr = Rect::new(skr.right() + 6.0, by, 32.0, 32.0);
    draw_button(cv, rsr, "", ButtonKind::Ghost, cx.hover_t(reset));
    cv.glyph(G_RESET, 10.0, rsr, TEXT_TITLE, 1.0);
    cx.hit(rsr, reset);
}

fn draw_task_list(cv: &mut Canvas, cx: &mut PageCtx, r: Rect) {
    let lang = cx.lang();
    let st = cx.st;
    cx.card(cv, r, RADIUS_CARD, 0.0);
    draw_section_header(cv, Rect::new(r.x + 16.0, r.y + 14.0, r.w - 80.0, 16.0), t(lang, Key::TodayTasks));
    let items: Vec<&Todo> = st.data.todos.today_items(cx.now, st.offset_min);
    let pending = items.iter().filter(|t| !t.done).count();
    draw_count_badge(cv, r.right() - 14.0, r.y + 22.0, pending, ACCENT_PRIMARY_HI);

    let top = r.y + 40.0;
    let body = Rect::new(r.x, top, r.w, r.bottom() - top);
    if pending == 0 {
        draw_empty_state(cv, Rect::new(body.x, body.y - 6.0, body.w, body.h), t(lang, Key::AllClear), t(lang, Key::AllClearSub));
        return;
    }
    let row_h = 40.0;
    let step = 42.0;
    let fit = ((body.h - 6.0) / step).floor().max(0.0) as usize;
    // Inset 4 so rows keep R_item = R_card − 4 (concentric).
    for (i, item) in items.iter().take(fit).enumerate() {
        let row = Rect::new(r.x + 4.0, top + i as f32 * step, r.w - 8.0, row_h);
        todo_row(cv, cx, row, item, false, false);
    }
    if items.len() > fit && fit > 0 {
        let more = format!("+{}", items.len() - fit);
        cv.text_styled(&more, Font::text(11.0, 600, Align::Right), Rect::new(r.right() - 70.0, r.y + 14.0, 30.0, 16.0), TEXT_SECTION, 1.0, TextStyle::TABULAR);
    }
}

fn draw_timeline(cv: &mut Canvas, cx: &mut PageCtx, r: Rect) {
    let lang = cx.lang();
    let st = cx.st;
    cx.card(cv, r, RADIUS_CARD, 0.0);
    draw_section_header(cv, Rect::new(r.x + 16.0, r.y + 14.0, r.w - 32.0, 16.0), t(lang, Key::UpcomingTimeline));

    let items: Vec<&Todo> = st.data.todos.reminder_items().into_iter().filter(|t| !t.done).collect();
    let top = r.y + 46.0;
    if items.is_empty() {
        let (ccx, ccy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        cv.fill_circle_radial(ccx, ccy - 8.0, 46.0, GLOW_AMBER, 0.35);
        cv.glyph(G_CALENDAR, 20.0, Rect::new(ccx - 20.0, ccy - 28.0, 40.0, 40.0), TEXT_BODY, 0.9);
        cv.text(t(lang, Key::NoUpcoming), Font::text(12.5, 500, Align::Center), Rect::new(r.x, ccy + 14.0, r.w, 20.0), TEXT_BODY, 0.9);
        return;
    }
    let step = 50.0;
    let fit = ((r.bottom() - top - 6.0) / step).floor().max(1.0) as usize;
    let shown: Vec<&&Todo> = items.iter().take(fit).collect();
    let time_w = 50.0;
    let dot_x = r.x + 16.0 + time_w + 10.0;
    if shown.len() > 1 {
        let y0 = top + 10.0;
        let y1 = top + (shown.len() - 1) as f32 * step + 10.0;
        cv.fill_rect_linear(Rect::new(dot_x - 0.5, y0, 1.0, y1 - y0), DIVIDER_V, (dot_x, y0 - 40.0), (dot_x, y1 + 40.0), 1.0);
        cv.fill_rect(Rect::new(dot_x - 0.5, y0, 1.0, y1 - y0), WHITE, 0.06);
    }
    for (i, item) in shown.iter().enumerate() {
        let Some(due) = item.snoozed_until.or(item.due) else { continue };
        let y = top + i as f32 * step;
        let bucket = day_bucket(due, cx.now, st.offset_min);
        let rgb = match bucket {
            DayBucket::Overdue => ACCENT_ROSE,
            DayBucket::Today => ACCENT_AMBER,
            DayBucket::Tomorrow => ACCENT_SKY,
            _ => ACCENT_PRIMARY_HI,
        };
        let c = civil(due, st.offset_min);
        let hhmm = if item.all_day { "—".to_string() } else { format!("{:02}:{:02}", c.hour, c.minute) };
        cv.text_styled(&hhmm, Font::text(13.0, 600, Align::Right), Rect::new(r.x + 12.0, y, time_w, 20.0), if i == 0 { TEXT_HERO } else { TEXT_TITLE }, 1.0, TextStyle::TABULAR);
        if i == 0 {
            cv.fill_circle_radial(dot_x, y + 10.0, 14.0, if rgb == ACCENT_AMBER { GLOW_AMBER } else { GLOW_ACCENT }, 1.0);
        }
        cv.fill_circle(dot_x, y + 10.0, 4.5, rgb, 1.0);
        cv.stroke_circle(dot_x, y + 10.0, 6.5, 1.0, rgb, 0.35);
        let tx = dot_x + 14.0;
        cv.text(&item.text, Font::text(13.0, 500, Align::Left), Rect::new(tx, y, r.right() - tx - 14.0, 20.0), TEXT_TITLE, 1.0);
        let when = fmt_due(due, item.all_day, cx.now, st.offset_min, lang);
        cv.text_styled(&when, FONT_META, Rect::new(tx, y + 19.0, r.right() - tx - 14.0, 16.0), if bucket == DayBucket::Overdue { ACCENT_ROSE } else { TEXT_SECTION }, 1.0, TextStyle::TABULAR);
        let row = Rect::new(r.x + 4.0, y - 6.0, r.w - 8.0, step - 4.0);
        let act = HubAction::SelectPage(ailan_core::settings::HubPage::Reminders);
        if cx.hover == Some(act) && cx.mouse.is_some_and(|(mx, my)| row.contains(mx, my)) {
            cv.fill_round(row, RADIUS_ITEM, WHITE, 0.035);
        }
        cx.hit(row, act);
    }
}
