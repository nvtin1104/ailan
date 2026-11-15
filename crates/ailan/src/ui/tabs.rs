//! Expanded island: music and to-do, one size, coordinates relative to the pill.

use ailan_core::i18n::{short_date, t, Key, Lang};
use ailan_core::settings::Tab;
use ailan_core::time::{civil, fmt_duration};
use ailan_core::AppState;

use crate::gfx::{Align, Canvas, Font, Rect};
use super::{draw_arrow_btn, draw_status_ring, hover_amt, Action, Hit, View, G_ADD, G_DELETE, G_HOME, G_MUSIC, G_NEXT, G_PAUSE, G_PLAY, G_PREV, SURFACE, TEXT_SOFT, TODO_ROWS, WHITE};

pub(super) fn draw_expanded(cv: &mut Canvas, st: &AppState, v: &View, p: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let c = civil(v.now, st.offset_min);

    let hdr_cy = p.y + 20.0;
    draw_status_ring(cv, st, p.x + 22.0, hdr_cy);
    let ds = format!("{} {}", short_date(lang, &c), c.year);
    cv.text(&ds, Font::text(11.0, 400, Align::Left), Rect::new(p.x + 36.0, p.y + 10.0, 120.0, 20.0), TEXT_SOFT, 1.0);
    let time_s = format!("{:02}:{:02}", c.hour, c.minute);
    cv.text(&time_s, Font::text(15.0, 600, Align::Center), Rect::new(p.x + p.w * 0.5 - 32.0, p.y + 8.0, 64.0, 24.0), WHITE, 1.0);

    let home_r = Rect::new(p.right() - 36.0, hdr_cy - 11.0, 22.0, 22.0);
    let h_home = hover_amt(v, Action::OpenSettings);
    if h_home > 0.01 {
        cv.fill_round(home_r, 8.0, SURFACE, 1.0);
    }
    cv.glyph(G_HOME, 12.0, home_r, WHITE, 0.70 + 0.30 * h_home);
    hits.push(Hit { rect: home_r, action: Action::OpenSettings });

    // Pages: Music when media is around, To-do while today still has work.
    let visible = st.hover_activities(v.now);
    if visible.is_empty() {
        return;
    }

    let active_tab = if visible.contains(&v.tab) { v.tab } else { visible[0] };
    let target_idx = visible.iter().position(|&t| t == active_tab).unwrap_or(0) as f32;
    let slide_dx = if visible.len() > 1 {
        (target_idx - v.tab_pos).clamp(-1.0, 1.0) * 14.0
    } else {
        0.0
    };
    let inset = if visible.len() > 1 { 26.0 } else { 0.0 };
    let page_r = Rect::new(p.x + inset + slide_dx, p.y, (p.w - inset * 2.0).max(40.0), p.h);

    if visible.len() > 1 {
        let arrow_y = p.y + 72.0;
        draw_arrow_btn(cv, v, hits, p.x + 16.0, arrow_y, true);
        draw_arrow_btn(cv, v, hits, p.right() - 16.0, arrow_y, false);
    }

    // 4. Main Activity Page Content.
    match active_tab {
        Tab::Music => draw_music(cv, st, v, page_r, hits),
        Tab::Todo => draw_todo(cv, st, v, page_r, hits),
    }

    // 5. Bottom Page Indicator Dots (only when > 1 visible activity, no hover height scaling).
    if visible.len() > 1 {
        let gap = 5.0;
        let widths: Vec<f32> = visible
            .iter()
            .enumerate()
            .map(|(idx, _)| {
                let dist = (idx as f32 - v.tab_pos).abs().clamp(0.0, 1.0);
                4.0 + (15.0 - 4.0) * (1.0 - dist)
            })
            .collect();
        let total_dots_w: f32 = widths.iter().sum::<f32>() + gap * (visible.len().saturating_sub(1) as f32);
        let mut dx = p.x + (p.w - total_dots_w) * 0.5;
        let dy = p.bottom() - 8.0;
        for (idx, &act) in visible.iter().enumerate() {
            let dw = widths[idx];
            let dist = (idx as f32 - v.tab_pos).abs().clamp(0.0, 1.0);
            let h_dot = hover_amt(v, Action::Tab(act));
            let dh = 3.6;
            let dot_r = Rect::new(dx, dy, dw, dh);
            let alpha = 0.35 + 0.65 * (1.0 - dist).max(h_dot * 0.5);
            cv.fill_round(dot_r, dh * 0.5, WHITE, alpha);
            hits.push(Hit {
                rect: Rect::new(dx - 2.0, dy - 7.0, dw + 4.0, 16.0),
                action: Action::Tab(act),
            });
            dx += dw + gap;
        }
    }
}

fn draw_music(cv: &mut Canvas, st: &AppState, v: &View, p: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let art = Rect::new(p.x + 16.0, p.y + 38.0, 44.0, 44.0);

    let text_x = art.right() + 12.0;
    let text_w = (p.right() - 16.0 - text_x).max(40.0);
    let Some(m) = &st.media else {
        cv.fill_round(art, 10.0, SURFACE, 1.0);
        cv.glyph(G_MUSIC, 18.0, art, WHITE, 0.55);
        cv.text(t(lang, Key::NothingPlaying), Font::text(15.0, 600, Align::Left), Rect::new(text_x, p.y + 40.0, text_w, 20.0), WHITE, 1.0);
        cv.text(t(lang, Key::NothingPlayingHint), Font::text(11.0, 400, Align::Left), Rect::new(text_x, p.y + 62.0, text_w, 16.0), TEXT_SOFT, 1.0);
        draw_music_controls(cv, v, p, false, hits);
        return;
    };

    match v.art {
        Some((brush, size)) => cv.image(brush, size, art, 10.0),
        None => {
            cv.fill_round(art, 10.0, SURFACE, 1.0);
            cv.glyph(G_MUSIC, 18.0, art, WHITE, 0.9);
        }
    }

    cv.text(&m.title, Font::text(15.0, 600, Align::Left), Rect::new(text_x, p.y + 40.0, text_w, 20.0), WHITE, 1.0);
    let sub = if m.artist.is_empty() { m.app.clone() } else { format!("{} · {}", m.artist, m.app) };
    cv.text(&sub, Font::text(11.0, 400, Align::Left), Rect::new(text_x, p.y + 62.0, text_w, 16.0), TEXT_SOFT, 1.0);

    let pos = if m.duration_ms > 0 { m.position_at(v.now) } else { 0 };
    let rem = (m.duration_ms - pos).max(0);
    cv.text(&fmt_duration(pos), Font::text(11.0, 500, Align::Left), Rect::new(p.x + 16.0, p.y + 89.0, 48.0, 14.0), TEXT_SOFT, 1.0);
    cv.text(&format!("-{}", fmt_duration(rem)), Font::text(11.0, 500, Align::Right), Rect::new(p.right() - 64.0, p.y + 89.0, 48.0, 14.0), TEXT_SOFT, 1.0);
    let bar = Rect::new(p.x + 16.0, p.y + 106.0, (p.w - 32.0).max(24.0), 3.0);
    cv.fill_round(bar, 1.5, SURFACE, 1.0);
    if m.duration_ms > 0 {
        let f = (pos as f32 / m.duration_ms as f32).clamp(0.0, 1.0);
        if f > 0.002 {
            let fw = (bar.w * f).max(4.0);
            let fill = Rect::new(bar.x, bar.y, fw, bar.h);
            cv.fill_round(fill, 2.0, WHITE, 1.0);
        }
    }
    draw_music_controls(cv, v, p, m.playing, hits);
}

fn draw_music_controls(cv: &mut Canvas, v: &View, p: Rect, playing: bool, hits: &mut Vec<Hit>) {
    let ctl = |cv: &mut Canvas, hits: &mut Vec<Hit>, cx: f32, glyph: char, size: f32, action: Action| {
        let cy = p.bottom() - 22.0;
        let r = Rect::new(cx - 14.0, cy - 12.0, 28.0, 24.0);
        let h = hover_amt(v, action);
        if h > 0.01 {
            cv.fill_circle(cx, cy, 12.0, SURFACE, 1.0);
        }
        cv.glyph(glyph, size, r, WHITE, 0.85 + 0.15 * h);
        hits.push(Hit { rect: r, action });
    };

    let cx = p.x + p.w * 0.5;
    ctl(cv, hits, cx - 44.0, G_PREV, 13.0, Action::MediaPrev);
    ctl(
        cv,
        hits,
        cx,
        if playing { G_PAUSE } else { G_PLAY },
        15.0,
        Action::MediaToggle,
    );
    ctl(cv, hits, cx + 44.0, G_NEXT, 13.0, Action::MediaNext);
}

fn draw_todo(cv: &mut Canvas, st: &AppState, v: &View, p: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let items = st.data.todos.today_pending(v.now, st.offset_min);

    let inner_x = p.x + 16.0;
    let inner_w = (p.w - 32.0).max(40.0);
    let field = Rect::new(inner_x, p.y + 42.0, inner_w, 28.0);
    if let Some(s) = v.input {
        cv.fill_round(field, 13.0, SURFACE, 1.0);
        cv.stroke_round(field.inset(0.5, 0.5), 12.5, 1.0, WHITE, 0.55);
        let txt_r = Rect::new(field.x + 10.0, field.y, field.w - 16.0, field.h);
        if s.is_empty() {
            cv.text(t(lang, Key::AddTask), Font::text(11.0, 400, Align::Left), txt_r, WHITE, 0.55);
        } else {
            cv.text(&format!("{s}▏"), Font::text(11.5, 500, Align::Left), txt_r, WHITE, 1.0);
        }
        hits.push(Hit { rect: field, action: Action::TodoInput });
    } else {
        let h_add = hover_amt(v, Action::TodoInput);
        cv.fill_round(field, 13.0, SURFACE, 1.0);
        cv.stroke_round(field.inset(0.5, 0.5), 12.5, 1.0, WHITE, 0.12 + 0.16 * h_add);
        cv.glyph(G_ADD, 9.5, Rect::new(field.x + 10.0, field.y, 16.0, field.h), WHITE, 0.95);
        cv.text(
            if lang == Lang::Vi { "Thêm việc mới" } else { "Add Task" },
            Font::text(11.0, 600, Align::Left),
            Rect::new(field.x + 28.0, field.y, field.w - 34.0, field.h),
            WHITE,
            0.95,
        );
        hits.push(Hit { rect: field, action: Action::TodoInput });
    }

    // Hide the list UI altogether when there are no pending tasks.
    if items.is_empty() {
        return;
    }

    let list_r = Rect::new(inner_x, p.y + 78.0, inner_w, 52.0);
    hits.push(Hit { rect: list_r, action: Action::TodoScroll });
    let scroll = v.todo_scroll.min(items.len().saturating_sub(TODO_ROWS));
    for (i, item) in items.iter().skip(scroll).take(TODO_ROWS).enumerate() {
        let row = Rect::new(inner_x, p.y + 78.0 + i as f32 * 26.0, inner_w, 22.0);
        let h_toggle = hover_amt(v, Action::TodoToggle(item.id));
        let h_del = hover_amt(v, Action::TodoDelete(item.id));
        let h_any = h_toggle.max(h_del);

        cv.fill_round(row, 8.0, SURFACE, 0.85 + 0.15 * h_any);

        let chk_cx = row.x + 14.0;
        let chk_cy = row.y + row.h * 0.5;
        cv.stroke_circle(chk_cx, chk_cy, 6.8, 1.3, WHITE, 0.50 + 0.35 * h_toggle);

        let del_r = Rect::new(row.right() - 22.0, row.y + 2.0, 20.0, row.h - 4.0);
        if h_del > 0.01 || h_toggle > 0.01 {
            cv.glyph(G_DELETE, 8.5, del_r, WHITE, 0.45 + 0.55 * h_del);
        }
        hits.push(Hit { rect: del_r, action: Action::TodoDelete(item.id) });

        let txt_r = Rect::new(row.x + 26.0, row.y, row.w - 48.0, row.h);
        cv.text(&item.text, Font::text(11.5, 500, Align::Left), txt_r, WHITE, 0.95);
        hits.push(Hit {
            rect: Rect::new(row.x, row.y, row.w - 22.0, row.h),
            action: Action::TodoToggle(item.id),
        });
    }

    // Scrollbar at right of list when > TODO_ROWS pending tasks
    if items.len() > TODO_ROWS {
        let track_h = 52.0;
        let frac = (TODO_ROWS as f32 / items.len() as f32).clamp(0.25, 1.0);
        let max_scroll = items.len().saturating_sub(TODO_ROWS).max(1) as f32;
        let pos = (scroll as f32 / max_scroll).clamp(0.0, 1.0);
        let thumb_h = (track_h * frac).max(12.0);
        let thumb_y = p.y + 78.0 + (track_h - thumb_h) * pos;
        cv.fill_round(Rect::new(inner_x + inner_w + 4.0, thumb_y, 3.0, thumb_h), 1.5, WHITE, 0.40);
    }
}
