//! Expanded notch view and tab pages (Music, Focus, Todo).

use ailan_core::i18n::{short_date, t, Key, Lang};
use ailan_core::settings::Tab;
use ailan_core::time::{civil, fmt_duration};
use ailan_core::AppState;

use crate::gfx::{Align, Canvas, Font, Rect};
use super::{draw_arrow_btn, draw_status_ring, draw_wave_bars, hover_amt, scale_rect, Action, Hit, View, G_ADD, G_DELETE, G_HOME, G_MUSIC, G_NEXT, G_PAUSE, G_PLAY, G_PREV, TEXT_SOFT, TODO_ROWS, WHITE};

pub(super) fn draw_expanded(cv: &mut Canvas, st: &AppState, v: &View, p: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let c = civil(v.now, st.offset_min);
    let playing = st.media.as_ref().is_some_and(|m| m.playing);

    // 1. Shared Top Status Row (y = 18): Battery ring + Date & Time + Home icon + Wave bars.
    let hdr_cy = p.y + 18.0;
    draw_status_ring(cv, st, p.x + 146.0, hdr_cy);
    let mut hx = p.x + 160.0;
    let ds = format!("{} {}", short_date(lang, &c), c.year);
    let dw = (ds.chars().count() as f32 * 6.5).max(88.0);
    cv.text(&ds, Font::text(12.0, 400, Align::Left), Rect::new(hx, p.y + 8.0, dw, 20.0), WHITE, 0.85);
    hx += dw + 4.0;
    let time_s = format!("{:02}:{:02}", c.hour, c.minute);
    cv.text(&time_s, Font::text(12.5, 700, Align::Left), Rect::new(hx, p.y + 8.0, 44.0, 20.0), WHITE, 1.0);

    // Wave bars and Home icon on the top-right
    let wave_w = 20.0;
    let wave_h = 14.0;
    let wave_r = Rect::new(p.right() - 28.0 - wave_w, hdr_cy - wave_h * 0.5, wave_w, wave_h);

    let home_w = 26.0;
    let home_h = 24.0;
    let home_gap = 14.0;
    let home_r = Rect::new(wave_r.x - home_gap - home_w, hdr_cy - home_h * 0.5, home_w, home_h);
    let h_home = hover_amt(v, Action::OpenSettings);
    if h_home > 0.01 {
        cv.fill_round(home_r, 8.0, WHITE, 0.16 * h_home);
        cv.hairline_round(home_r, 8.0, WHITE, 0.22 * h_home);
    }
    cv.glyph(G_HOME, 12.0, home_r, WHITE, 0.70 + 0.30 * h_home);
    hits.push(Hit { rect: home_r.inset(-3.0, -3.0), action: Action::OpenSettings });

    // 5 audio-reactive sound-wave bars
    draw_wave_bars(cv, v, wave_r, playing);

    // 2. Visible pages: Focus while a session is on, Music when media is around, To-do always.
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
    let page_r = Rect::new(p.x + slide_dx, p.y, p.w, p.h);

    // 3. Left / Right Slide Arrow Buttons only when > 1 visible activity.
    if visible.len() > 1 {
        let arrow_y = if p.h < 100.0 { p.y + 46.0 } else { p.y + 70.0 };
        if active_tab != Tab::Music {
            draw_arrow_btn(cv, v, hits, p.x + 22.0, arrow_y, true);
        }
        draw_arrow_btn(cv, v, hits, p.x + 395.0, arrow_y, false);
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
        let dy = p.bottom() - 8.8;
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

// ---- Page 1: Music (`l-music` in 420×140) ---------------------------------------

fn draw_music(cv: &mut Canvas, st: &AppState, v: &View, p: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let base_art = Rect::new(p.x + 22.0, p.y + 12.0, 62.0, 62.0);

    let Some(m) = &st.media else {
        cv.fill_round(base_art, 10.0, WHITE, 0.08);
        cv.glyph(G_MUSIC, 22.0, base_art, WHITE, 0.55);
        cv.text(
            t(lang, Key::NothingPlaying),
            Font::text(15.0, 650, Align::Left),
            Rect::new(p.x + 100.0, p.y + 36.0, 270.0, 20.0),
            WHITE,
            1.0,
        );
        cv.text(
            t(lang, Key::NothingPlayingHint),
            Font::text(11.5, 400, Align::Left),
            Rect::new(p.x + 100.0, p.y + 54.0, 270.0, 18.0),
            WHITE,
            0.50,
        );
        let bar = Rect::new(p.x + 77.0, p.y + 89.0, 260.0, 4.0);
        cv.fill_round(bar, 2.0, WHITE, 0.22);
        cv.text("00:00", Font::text(12.0, 500, Align::Left), Rect::new(p.x + 22.0, p.y + 82.0, 48.0, 18.0), WHITE, 0.55);
        cv.text("-00:00", Font::text(12.0, 500, Align::Right), Rect::new(p.right() - 75.0, p.y + 82.0, 50.0, 18.0), WHITE, 0.55);
        draw_music_controls(cv, v, p, false, hits);
        return;
    };

    // 1. 62×62 Album Art at (22, 12) with subtle audio-reactive bass pulse when playing.
    let pulse = if m.playing {
        1.0 + 0.045 * v.audio_peak
    } else {
        1.0
    };
    let art = scale_rect(base_art, pulse);
    match v.art {
        Some((brush, size)) => {
            cv.image(brush, size, art, 10.0);
        }
        None => {
            cv.fill_round(art, 10.0, WHITE, 0.10 + 0.06 * v.audio_peak);
            cv.glyph(G_MUSIC, 22.0, art, WHITE, 0.90);
        }
    }

    // 2. Title & Artist
    cv.text(
        &m.title,
        Font::text(15.0, 650, Align::Left),
        Rect::new(p.x + 100.0, p.y + 36.0, 276.0, 20.0),
        WHITE,
        1.0,
    );
    let sub = if m.artist.is_empty() { m.app.clone() } else { format!("{} - {}", m.artist, m.app) };
    cv.text(
        &sub,
        Font::text(11.5, 400, Align::Left),
        Rect::new(p.x + 100.0, p.y + 54.0, 276.0, 16.0),
        WHITE,
        0.50,
    );

    // 3. Progress bar row at y = 89 (elapsed | 260px white bar | -remaining).
    let pos = if m.duration_ms > 0 { m.position_at(v.now) } else { 0 };
    let rem = (m.duration_ms - pos).max(0);
    let bar = Rect::new(p.x + 77.0, p.y + 89.0, 260.0, 4.0);
    cv.fill_round(bar, 2.0, WHITE, 0.22);
    if m.duration_ms > 0 {
        let f = (pos as f32 / m.duration_ms as f32).clamp(0.0, 1.0);
        if f > 0.002 {
            let fw = (bar.w * f).max(4.0);
            let fill = Rect::new(bar.x, bar.y, fw, bar.h);
            cv.fill_round(fill, 2.0, WHITE, 1.0);
            if m.playing {
                let head_x = fill.right().clamp(bar.x + 2.0, bar.right() - 2.0);
                let head_y = bar.y + bar.h * 0.5;
                cv.fill_circle(head_x, head_y, 2.8, WHITE, 0.98);
            }
        }
    }
    cv.text(
        &fmt_duration(pos),
        Font::text(12.0, 500, Align::Left),
        Rect::new(p.x + 22.0, p.y + 82.0, 50.0, 18.0),
        WHITE,
        0.95,
    );
    cv.text(
        &format!("-{}", fmt_duration(rem)),
        Font::text(12.0, 500, Align::Right),
        Rect::new(p.right() - 75.0, p.y + 82.0, 50.0, 18.0),
        WHITE,
        0.95,
    );

    // 4. Transport controls row at y = 115 (quick-add button removed, cursor-only hover).
    draw_music_controls(cv, v, p, m.playing, hits);
}

fn draw_music_controls(cv: &mut Canvas, v: &View, p: Rect, playing: bool, hits: &mut Vec<Hit>) {
    // Prev (x = 162.5), Play/Pause (x = 210), Next (x = 257.5) at y = 115.
    let ctl = |cv: &mut Canvas, hits: &mut Vec<Hit>, cx: f32, glyph: char, size: f32, action: Action| {
        let cy = p.y + 115.0;
        let r = Rect::new(cx - 16.0, cy - 13.0, 32.0, 26.0);
        let h = hover_amt(v, action);
        if h > 0.01 {
            cv.fill_circle(cx, cy, 12.0, WHITE, 0.14 * h);
        }
        cv.glyph(glyph, size, r, WHITE, 0.85 + 0.15 * h);
        hits.push(Hit { rect: r, action });
    };

    ctl(cv, hits, p.x + 162.5, G_PREV, 13.5, Action::MediaPrev);
    ctl(
        cv,
        hits,
        p.x + 210.0,
        if playing { G_PAUSE } else { G_PLAY },
        15.0,
        Action::MediaToggle,
    );
    ctl(cv, hits, p.x + 257.5, G_NEXT, 13.5, Action::MediaNext);
}

// ---- Page 2: To-do (`l-todo` in 420×82 when empty, 420×126 with tasks) ----------

fn draw_todo(cv: &mut Canvas, st: &AppState, v: &View, p: Rect, hits: &mut Vec<Hit>) {
    let lang = st.lang();
    let items = st.data.todos.today_pending(v.now, st.offset_min);

    // Top-left monochrome title: "Todo List"
    cv.text(
        "Todo",
        Font::text(15.0, 700, Align::Left),
        Rect::new(p.x + 15.0, p.y + 9.0, 40.0, 20.0),
        WHITE,
        1.0,
    );
    cv.text(
        "List",
        Font::text(15.0, 400, Align::Left),
        Rect::new(p.x + 53.0, p.y + 9.0, 45.0, 20.0),
        TEXT_SOFT,
        0.95,
    );

    // Compact single-column layout: Add Task pill on top, up to TODO_ROWS pending rows below.
    let inner_x = p.x + 42.0;
    let inner_w = p.w - 84.0;
    let field = Rect::new(inner_x, p.y + 33.0, inner_w, 25.0);
    if let Some(s) = v.input {
        cv.fill_round(field, 12.5, WHITE, 0.16);
        cv.stroke_round(field.inset(0.5, 0.5), 12.0, 1.0, WHITE, 0.75);
        let txt_r = Rect::new(field.x + 10.0, field.y, field.w - 16.0, field.h);
        if s.is_empty() {
            cv.text(t(lang, Key::AddTask), Font::text(11.0, 400, Align::Left), txt_r, WHITE, 0.55);
        } else {
            cv.text(&format!("{s}▏"), Font::text(11.5, 500, Align::Left), txt_r, WHITE, 1.0);
        }
        hits.push(Hit { rect: field, action: Action::TodoInput });
    } else {
        let h_add = hover_amt(v, Action::TodoInput);
        cv.fill_round(field, 12.5, WHITE, 0.12 + 0.08 * h_add);
        cv.stroke_round(field.inset(0.5, 0.5), 12.0, 1.0, WHITE, 0.18 + 0.14 * h_add);
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

    let list_r = Rect::new(inner_x, p.y + 63.0, inner_w, 53.0);

    hits.push(Hit { rect: list_r, action: Action::TodoScroll });
    let scroll = v.todo_scroll.min(items.len().saturating_sub(TODO_ROWS));
    for (i, item) in items.iter().skip(scroll).take(TODO_ROWS).enumerate() {
        let row = Rect::new(inner_x, p.y + 63.0 + i as f32 * 28.0, inner_w, 25.0);
        let h_toggle = hover_amt(v, Action::TodoToggle(item.id));
        let h_del = hover_amt(v, Action::TodoDelete(item.id));
        let h_any = h_toggle.max(h_del);

        cv.fill_round(row, 8.5, WHITE, 0.08 + 0.06 * h_any);

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
        let track_h = 48.0;
        let frac = (TODO_ROWS as f32 / items.len() as f32).clamp(0.25, 1.0);
        let max_scroll = items.len().saturating_sub(TODO_ROWS).max(1) as f32;
        let pos = (scroll as f32 / max_scroll).clamp(0.0, 1.0);
        let thumb_h = (track_h * frac).max(12.0);
        let thumb_y = p.y + 65.0 + (track_h - thumb_h) * pos;
        cv.fill_round(Rect::new(inner_x + inner_w + 4.0, thumb_y, 3.0, thumb_h), 1.5, WHITE, 0.40);
    }
}
