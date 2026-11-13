//! Today dashboard — a Bento grid:
//! ┌ Hero (greeting + progress ring) ┬ Focus Hub (glowing countdown arc) ┐
//! ├ Quick Capture (live @time highlighting) ─────────────────────────────┤
//! └ Today's tasks                   ┴ Upcoming reminders timeline ───────┘
//! In Compact mode the same blocks stack vertically.

use ailan_core::i18n::{short_date, t, Key};
use ailan_core::reminder::{day_bucket, fmt_due, DayBucket};
use ailan_core::time::{civil, local_day};
use ailan_core::todo::Todo;

use crate::gfx::{Align, Canvas, Font, Rect, TextStyle};
use crate::hub::pages::todos::{capture_bar, todo_row};
use crate::hub::widgets::*;
use crate::hub::{HubAction, PageCtx};
use crate::ui::G_BELL;

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
    let hero = Rect::new(r.x, r.y, r.w, 152.0);
    draw_hero(cv, cx, hero, false);

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
    let cap = Rect::new(r.x, hero.bottom() + 10.0, r.w, 44.0);
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
        todo_row(cv, cx, row, item, false);
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
