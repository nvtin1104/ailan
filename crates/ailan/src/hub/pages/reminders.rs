//! Reminders page: collapsible buckets (Overdue · Today · Tomorrow · This week · Later)
//! with status dots, repeat badges and one-click `+5m` / `+1h` / `Done` actions.

use ailan_core::i18n::{t, Key};
use ailan_core::reminder::{day_bucket, fmt_due, fmt_repeat, DayBucket, Repeat};
use ailan_core::todo::Todo;

use crate::gfx::{Align, Canvas, Font, Rect, TextStyle};
use crate::hub::pages::todos::capture_bar;
use crate::hub::widgets::*;
use crate::hub::{HubAction, PageCtx};

const CARD_H: f32 = 64.0;
const CARD_STEP: f32 = 72.0;
const HEADER_H: f32 = 34.0;

fn bucket_index(b: DayBucket) -> usize {
    match b {
        DayBucket::Overdue => 0,
        DayBucket::Today => 1,
        DayBucket::Tomorrow => 2,
        DayBucket::ThisWeek => 3,
        DayBucket::Later => 4,
    }
}

pub fn draw(cv: &mut Canvas, cx: &mut PageCtx, r: Rect) {
    let lang = cx.lang();
    let st = cx.st;
    let mut y = r.y - cx.scroll;

    capture_bar(cv, cx, Rect::new(r.x, y, r.w, 44.0), t(lang, Key::NewReminderPlaceholder));
    y += 44.0 + 20.0;

    let mut groups: [Vec<&Todo>; 5] = Default::default();
    for item in st.data.todos.reminder_items().into_iter().filter(|t| !t.done) {
        if let Some(due) = item.snoozed_until.or(item.due) {
            groups[bucket_index(day_bucket(due, cx.now, st.offset_min))].push(item);
        }
    }
    let meta: [(&str, u32); 5] = [
        (t(lang, Key::Overdue), ACCENT_ROSE),
        (t(lang, Key::BucketToday), ACCENT_AMBER),
        (t(lang, Key::BucketTomorrow), ACCENT_SKY),
        (t(lang, Key::BucketThisWeek), ACCENT_PRIMARY_HI),
        (t(lang, Key::BucketLater), TEXT_SECTION),
    ];

    if groups.iter().all(Vec::is_empty) {
        let er = Rect::new(r.x, y, r.w, (r.bottom() - y).max(220.0));
        draw_empty_state(cv, er, t(lang, Key::NoReminders), t(lang, Key::AllClearSub));
        cx.content_h = Some(y + cx.scroll - r.y + 240.0);
        return;
    }

    for (gi, items) in groups.iter().enumerate() {
        if items.is_empty() {
            continue;
        }
        let (title, rgb) = meta[gi];
        let collapsed = cx.collapsed[gi];

        // Accordion header
        let hr = Rect::new(r.x, y, r.w, HEADER_H);
        let act = HubAction::ToggleBucket(gi as u8);
        let hv = cx.hover_t(act);
        if hv > 0.0 {
            cv.fill_round(hr, RADIUS_BUTTON, WHITE, 0.04);
        }
        cv.glyph(if collapsed { G_CHEVRON_RIGHT } else { G_CHEVRON_DOWN }, 9.0, Rect::new(hr.x + 4.0, hr.y, 18.0, hr.h), TEXT_BODY, 0.9);
        if gi == 0 {
            cv.fill_circle_radial(hr.x + 32.0, hr.y + hr.h * 0.5, 10.0, GLOW_ROSE, 1.0);
        }
        cv.fill_circle(hr.x + 32.0, hr.y + hr.h * 0.5, 4.0, rgb, 1.0);
        let up = title.to_uppercase();
        cv.text_styled(&up, Font::text(12.0, 650, Align::Left), Rect::new(hr.x + 44.0, hr.y, hr.w - 100.0, hr.h), if gi == 0 { mix(rgb, WHITE, 0.25) } else { TEXT_TITLE }, 1.0, TextStyle::tracked(SECTION_TRACKING));
        draw_count_badge(cv, hr.right() - 6.0, hr.y + hr.h * 0.5, items.len(), rgb);
        cx.hit(hr, act);
        y += HEADER_H + 6.0;

        if collapsed {
            y += 6.0;
            continue;
        }
        for item in items {
            if y + CARD_H >= r.y - 20.0 && y <= r.bottom() + 20.0 {
                reminder_card(cv, cx, Rect::new(r.x, y, r.w, CARD_H), item, rgb);
            }
            y += CARD_STEP;
        }
        y += 10.0;
    }
    cx.content_h = Some(y + cx.scroll - r.y);
}

fn reminder_card(cv: &mut Canvas, cx: &mut PageCtx, r: Rect, item: &Todo, accent: u32) {
    let lang = cx.lang();
    let st = cx.st;
    let id = item.id;
    let hot = matches!(cx.hover, Some(HubAction::ToggleTodo(x) | HubAction::SnoozeTodo(x, _)) if x == id);
    cx.card(cv, r, RADIUS_ITEM, if hot { 1.0 } else { 0.0 });

    // Accent spine, inset 4 so its radius stays concentric with the card.
    cv.fill_round(Rect::new(r.x + 4.0, r.y + 14.0, 3.0, r.h - 28.0), 1.5, accent, 0.9);

    // Actions: [+5m] [+1h] [Done]
    let compact = r.w < 420.0;
    let bh = 30.0;
    let by = r.y + (r.h - bh) * 0.5;
    let done_w = if compact { 50.0 } else { 64.0 };
    let snz_w = if compact { 40.0 } else { 48.0 };
    let done_r = Rect::new(r.right() - 12.0 - done_w, by, done_w, bh);
    let h1_r = Rect::new(done_r.x - 6.0 - snz_w, by, snz_w, bh);
    let m5_r = Rect::new(h1_r.x - 6.0 - snz_w, by, snz_w, bh);

    let done_act = HubAction::ToggleTodo(id);
    let dh = if cx.hover == Some(done_act) && cx.mouse.is_some_and(|(mx, my)| done_r.contains(mx, my)) { 1.0 } else { 0.0 };
    cv.fill_round(done_r, RADIUS_BUTTON, ACCENT_MINT, 0.14 + 0.14 * dh);
    cv.hairline_round(done_r, RADIUS_BUTTON, ACCENT_MINT, 0.35 + 0.3 * dh);
    cv.text(t(lang, Key::DoneAction), Font::text(12.0, 600, Align::Center), done_r, mix(ACCENT_MINT, WHITE, 0.45), 1.0);
    cx.hit(done_r, done_act);

    for (rect, mins, label) in [(m5_r, 5u32, "+5m"), (h1_r, 60u32, "+1h")] {
        let a = HubAction::SnoozeTodo(id, mins);
        let h = cx.hover_t(a);
        cv.fill_round(rect, RADIUS_BUTTON, ACCENT_AMBER, 0.06 + 0.12 * h);
        cv.hairline_round(rect, RADIUS_BUTTON, if h > 0.0 { ACCENT_AMBER } else { WHITE }, if h > 0.0 { 0.45 } else { 0.10 });
        cv.text_styled(label, Font::text(11.5, 600, Align::Center), rect, if h > 0.0 { ACCENT_AMBER } else { TEXT_TITLE }, 1.0, TextStyle::TABULAR);
        cx.hit(rect, a);
    }

    // Checkbox (also completes)
    let (scale, tick) = cx.check_anim(id);
    let cb = Rect::new(r.x + 16.0, r.y + (r.h - 18.0) * 0.5, 18.0, 18.0);
    draw_checkbox(cv, cb, false, accent, if hot { 1.0 } else { 0.0 }, scale, tick);
    cx.hit(Rect::new(r.x, r.y, 46.0, r.h), done_act);

    // Title + meta line
    let tx = r.x + 46.0;
    let tw = (m5_r.x - tx - 10.0).max(0.0);
    cv.text(&item.text, FONT_ITEM, Rect::new(tx, r.y + 11.0, tw, 22.0), TEXT_TITLE, 1.0);
    if let Some(due) = item.snoozed_until.or(item.due) {
        let my = r.y + 35.0;
        let when = fmt_due(due, item.all_day, cx.now, st.offset_min, lang);
        let overdue = day_bucket(due, cx.now, st.offset_min) == DayBucket::Overdue;
        let ww = cv.text_width(&when, FONT_META, TextStyle::TABULAR).min(tw);
        cv.text_styled(&when, FONT_META, Rect::new(tx, my, ww + 2.0, 18.0), if overdue { ACCENT_ROSE } else { TEXT_BODY }, 1.0, TextStyle::TABULAR);
        let bx = tx + ww + 8.0;
        if item.repeat != Repeat::None && bx + 60.0 < tx + tw {
            let lbl = fmt_repeat(item.repeat, lang);
            let rw = cv.text_width(lbl, Font::text(10.5, 550, Align::Center), TextStyle::default()) + 30.0;
            let rr = Rect::new(bx, my, rw.min(tx + tw - bx), 18.0);
            cv.fill_round(rr, 9.0, BG_BADGE, 0.95);
            cv.glyph(G_REPEAT, 8.5, Rect::new(rr.x + 4.0, rr.y, 16.0, rr.h), ACCENT_SKY, 1.0);
            cv.text(lbl, Font::text(10.5, 550, Align::Left), Rect::new(rr.x + 20.0, rr.y, rr.w - 24.0, rr.h), ACCENT_SKY, 1.0);
        }
    }
}
