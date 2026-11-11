//! Todos page: segmented filter with sliding pill, live search, highlighted quick capture,
//! and tactile rows with hover-revealed actions.

use ailan_core::i18n::{t, Key};
use ailan_core::reminder::{day_bucket, fmt_due, DayBucket, Repeat};
use ailan_core::settings::Tab;
use ailan_core::todo::{due_token, Todo};

use crate::gfx::{Align, Canvas, Font, Rect, TextStyle};
use crate::hub::widgets::*;
use crate::hub::{Field, HubAction, PageCtx, TodoFilter};
use crate::ui::{G_ADD, G_PIN, G_TIMER};

const ROW_H: f32 = 46.0;
const ROW_STEP: f32 = 52.0;

/// Ring colour of a todo's checkbox, by urgency.
pub fn urgency_color(item: &Todo, now: i64, offset_min: i32) -> u32 {
    if item.done {
        return ACCENT_MINT;
    }
    match item.snoozed_until.or(item.due).map(|d| day_bucket(d, now, offset_min)) {
        Some(DayBucket::Overdue) => ACCENT_ROSE,
        Some(DayBucket::Today) => ACCENT_AMBER,
        Some(_) => ACCENT_PRIMARY_HI,
        None => TEXT_SECTION,
    }
}

/// Time pill colours `(fg, bg, bg_alpha)`.
fn badge_colors(item: &Todo, now: i64, offset_min: i32) -> (u32, u32, f32) {
    if item.done {
        return (TEXT_DONE, BG_BADGE, 0.6);
    }
    match item.snoozed_until.or(item.due).map(|d| day_bucket(d, now, offset_min)) {
        Some(DayBucket::Overdue) => (0xFDA4AF, ACCENT_ROSE, 0.16),
        _ => (ACCENT_SKY, BG_BADGE, 0.95),
    }
}

/// One todo row. `framed` draws its own specular card; otherwise it sits inside a parent card.
pub fn todo_row(cv: &mut Canvas, cx: &mut PageCtx, row: Rect, item: &Todo, framed: bool, show_pin: bool) {
    let st = cx.st;
    let id = item.id;
    let hot = matches!(
        cx.hover,
        Some(HubAction::TodoToggle(x) | HubAction::TodoDelete(x) | HubAction::FocusOnTodo(x)) if x == id
    ) || (show_pin && cx.hover == Some(HubAction::PinTodo) && cx.mouse.is_some_and(|(mx, my)| row.contains(mx, my)));
    let hv = if hot { 1.0 } else { 0.0 };
    if framed {
        cx.card(cv, row, RADIUS_ITEM, hv);
    } else if hot {
        cv.fill_round(row, RADIUS_ITEM, WHITE, 0.045);
        if let Some(m) = cx.mouse {
            draw_radial_spotlight(cv, row, RADIUS_ITEM, m, 0.05);
        }
        cx.spots.push(row);
    }

    let cy = row.y + row.h * 0.5;
    let (scale, tick) = cx.check_anim(id);
    draw_checkbox(cv, Rect::new(row.x + 12.0, cy - 9.0, 18.0, 18.0), item.done, urgency_color(item, cx.now, st.offset_min), hv, scale, tick);

    // Hover actions (registered always so moving onto them keeps the row hot).
    let mut right = row.right() - 8.0;
    let mut actions: Vec<(HubAction, char, bool)> = vec![(HubAction::TodoDelete(id), '\u{E74D}', false)];
    if show_pin {
        actions.push((HubAction::PinTodo, G_PIN, st.settings.is_pinned(Tab::Todo)));
    }
    if !item.done {
        actions.push((HubAction::FocusOnTodo(id), G_TIMER, false));
    }
    for (act, glyph, active) in &actions {
        let br = Rect::new(right - 28.0, cy - 14.0, 28.0, 28.0);
        if hot {
            let h = cx.hover_t(*act);
            if matches!(act, HubAction::TodoDelete(_)) {
                draw_delete_button(cv, br, h);
            } else {
                draw_icon_button(cv, br, *glyph, 11.0, h, *active, ACCENT_PRIMARY);
            }
        }
        cx.hit(br, *act);
        right -= 30.0;
    }
    if !hot {
        right = row.right() - 12.0;
    } else {
        right -= 4.0;
    }

    // Time badge + repeat glyph
    if let Some(due) = item.snoozed_until.or(item.due) {
        let label = fmt_due(due, item.all_day, cx.now, st.offset_min, cx.lang());
        let (fg, bg, ba) = badge_colors(item, cx.now, st.offset_min);
        let bw = draw_badge_right(cv, right, cy - 11.0, 22.0, &label, fg, bg, ba);
        right -= bw + 6.0;
        if item.repeat != Repeat::None {
            cv.glyph(G_REPEAT, 10.0, Rect::new(right - 16.0, cy - 8.0, 16.0, 16.0), TEXT_BODY, 0.9);
            right -= 20.0;
        }
    }

    let title_r = Rect::new(row.x + 42.0, row.y, (right - row.x - 50.0).max(0.0), row.h);
    if item.done {
        cv.text_styled(&item.text, FONT_ITEM, title_r, TEXT_DONE, 1.0, TextStyle::STRIKE);
    } else {
        cv.text(&item.text, FONT_ITEM, title_r, TEXT_TITLE, 1.0);
    }
    cx.hit(row, HubAction::TodoToggle(id));
}

/// Quick-capture bar with live `@time` highlighting and a resolved-time preview badge.
pub fn capture_bar(cv: &mut Canvas, cx: &mut PageCtx, r: Rect, hint: &str) {
    let focused = cx.focus == Some(Field::Capture);
    let hv = cx.hover_t(HubAction::FocusField(Field::Capture));
    let mut reserve = 40.0;
    let preview = due_token(cx.capture).and_then(|tok| tok.resolve(cx.now, cx.st.offset_min));
    let preview_label = preview.map(|d| format!("→ {}", fmt_due(d, false, cx.now, cx.st.offset_min, cx.lang())));
    if let Some(lbl) = &preview_label {
        reserve += cv.text_width(lbl, FONT_META_C, TextStyle::TABULAR) + 26.0;
    }
    draw_text_input(cv, r, cx.capture, hint, focused, hv, Font::text(13.5, 450, Align::Left), Some(G_ADD), reserve);
    cx.hit(r, HubAction::FocusField(Field::Capture));

    let submit = HubAction::SubmitField(Field::Capture);
    let sr = Rect::new(r.right() - r.h + 6.0, r.y + 6.0, r.h - 12.0, r.h - 12.0);
    let enabled = !cx.capture.trim().is_empty();
    if enabled {
        let rad = concentric(RADIUS_INPUT, 6.0);
        cv.fill_round_linear(sr, rad, ACCENT_FILL, (sr.x, sr.y), (sr.right(), sr.bottom()), 0.9 + 0.1 * cx.hover_t(submit));
        cv.glyph(G_RETURN, 11.0, sr, WHITE, 1.0);
    } else {
        cv.glyph(G_RETURN, 11.0, sr, TEXT_SECTION, 0.8);
    }
    // Registered after the bar: the bar's hit is first, so give the button its own priority by
    // inserting it ahead of the bar.
    let bar_idx = cx.hits.len() - 1;
    cx.hits.insert(bar_idx, crate::hub::HubHit { rect: sr, action: submit });
    if let Some(lbl) = preview_label {
        draw_badge_right(cv, sr.x - 8.0, r.y + (r.h - 22.0) * 0.5, 22.0, &lbl, ACCENT_AMBER, ACCENT_AMBER, 0.14);
    }
}

fn matches_filter(item: &Todo, filter: TodoFilter, now: i64, offset_min: i32) -> bool {
    let bucket = item.snoozed_until.or(item.due).map(|d| day_bucket(d, now, offset_min));
    match filter {
        TodoFilter::All => true,
        TodoFilter::Today => match bucket {
            Some(b) => matches!(b, DayBucket::Overdue | DayBucket::Today),
            None => !item.done,
        },
        TodoFilter::Upcoming => {
            !item.done && matches!(bucket, Some(DayBucket::Tomorrow | DayBucket::ThisWeek | DayBucket::Later))
        }
        TodoFilter::Completed => item.done,
    }
}

pub fn draw(cv: &mut Canvas, cx: &mut PageCtx, r: Rect) {
    let lang = cx.lang();
    let st = cx.st;
    let mut y = r.y - cx.scroll;

    // 1. Toolbar: segmented filter + search
    let labels = [t(lang, Key::AllTasks), t(lang, Key::TodayTasks), t(lang, Key::UpcomingTasks), t(lang, Key::CompletedTasks)];
    let (seg_r, search_r) = if cx.compact {
        (Rect::new(r.x, y, r.w, 34.0), Rect::new(r.x, y + 44.0, r.w, 34.0))
    } else {
        let sw = 220.0f32.min(r.w * 0.34);
        (Rect::new(r.x, y, r.w - sw - 14.0, 34.0), Rect::new(r.right() - sw, y, sw, 34.0))
    };
    let hover_seg = TodoFilter::ALL.iter().position(|f| cx.hover == Some(HubAction::SetTodoFilter(*f)));
    let segs = draw_segmented(cv, seg_r, &labels, cx.filter_pos, hover_seg);
    for (sr, f) in segs.iter().zip(TodoFilter::ALL) {
        cx.hit(*sr, HubAction::SetTodoFilter(f));
    }
    let s_focused = cx.focus == Some(Field::Search);
    if !cx.search.is_empty() {
        let xr = Rect::new(search_r.right() - 30.0, search_r.y + 3.0, 28.0, 28.0);
        cx.hit(xr, HubAction::ClearSearch);
    }
    let s_hv = cx.hover_t(HubAction::FocusField(Field::Search));
    draw_text_input(cv, search_r, cx.search, t(lang, Key::SearchHint), s_focused, s_hv, Font::text(12.5, 450, Align::Left), Some(G_SEARCH), 22.0);
    if !cx.search.is_empty() {
        let xr = Rect::new(search_r.right() - 30.0, search_r.y + 3.0, 28.0, 28.0);
        cv.glyph(G_CHROME_CLOSE, 9.0, xr, TEXT_BODY, 0.6 + 0.4 * cx.hover_t(HubAction::ClearSearch));
    }
    cx.hit(search_r, HubAction::FocusField(Field::Search));
    y = search_r.bottom().max(seg_r.bottom()) + 14.0;

    // 2. Capture
    capture_bar(cv, cx, Rect::new(r.x, y, r.w, 44.0), t(lang, Key::CaptureHint));
    y += 44.0 + 18.0;

    // 3. List
    let needle = cx.search.trim().to_lowercase();
    let items: Vec<&Todo> = st
        .data
        .todos
        .sorted()
        .into_iter()
        .filter(|it| matches_filter(it, cx.filter, cx.now, st.offset_min))
        .filter(|it| needle.is_empty() || it.text.to_lowercase().contains(&needle))
        .collect();

    let pending = items.iter().filter(|i| !i.done).count();
    draw_section_header(cv, Rect::new(r.x, y, r.w - 60.0, 18.0), labels[cx.filter.index()]);
    let cnt = format!("{pending}/{}", items.len());
    cv.text_styled(&cnt, Font::text(11.5, 500, Align::Right), Rect::new(r.right() - 80.0, y, 80.0, 18.0), TEXT_SECTION, 1.0, TextStyle::TABULAR);
    y += 28.0;

    let list_top = y;
    if items.is_empty() {
        let er = Rect::new(r.x, y, r.w, (r.bottom() - y).max(200.0));
        if !needle.is_empty() {
            cv.text(t(lang, Key::NoSearchResults), Font::text(13.0, 500, Align::Center), Rect::new(r.x, y + 40.0, r.w, 24.0), TEXT_BODY, 0.9);
        } else if cx.filter == TodoFilter::Completed {
            cv.text(t(lang, Key::NoTasks), Font::text(13.0, 500, Align::Center), Rect::new(r.x, y + 40.0, r.w, 24.0), TEXT_BODY, 0.9);
        } else {
            draw_empty_state(cv, er, t(lang, Key::AllClear), t(lang, Key::AllClearSub));
        }
        cx.content_h = Some(list_top + cx.scroll - r.y + 220.0);
        return;
    }

    for (idx, item) in items.iter().enumerate() {
        let ry = list_top + idx as f32 * ROW_STEP;
        if ry + ROW_H < r.y || ry > r.bottom() {
            continue;
        }
        todo_row(cv, cx, Rect::new(r.x, ry, r.w, ROW_H), item, true, true);
    }
    cx.content_h = Some(list_top + cx.scroll - r.y + items.len() as f32 * ROW_STEP + 8.0);
}
