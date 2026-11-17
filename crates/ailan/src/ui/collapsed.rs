//! Collapsed island: quiet capsule, media, clock tools, to-do progress, OSD and banners.

use ailan_core::clock::{fmt_hms, fmt_stopwatch};
use ailan_core::state::{BannerIcon, Collapsed, OsdKind};
use ailan_core::time::civil;
use ailan_core::AppState;

use crate::gfx::{Align, Canvas, Font, Rect, TextStyle};
use super::{
    draw_audio_bars, hover_amt, Action, CuteMood, Hit, View, BG, G_BELL, G_BOLT, G_BRIGHT, G_CHECKMARK, G_HEADPHONE,
    G_MESSAGE, G_MOON, G_MUSIC, G_MUTE, G_TIMER, G_VOLUME, SURFACE, WHITE,
};

const BLUSH_RGB: u32 = 0xFF7EB6;

fn draw_normal_eye(cv: &mut Canvas, ex: f32, ey: f32, r: f32, gaze: (f32, f32), sparkle: bool) {
    let er = if sparkle { r * 1.08 } else { r };
    cv.fill_circle(ex, ey, er, WHITE, 0.95);
    let px = ex + gaze.0.clamp(-1.0, 1.0) * er * 0.44;
    let py = ey + gaze.1.clamp(-1.0, 1.0) * er * 0.38;
    let pr = er * if sparkle { 0.56 } else { 0.50 };
    cv.fill_circle(px, py, pr, BG, 1.0);
    cv.fill_circle(px - er * 0.16, py - er * 0.16, (er * 0.18).max(0.85), WHITE, 0.95);
    if sparkle {
        cv.fill_circle(px + er * 0.18, py + er * 0.18, (er * 0.10).max(0.6), WHITE, 0.85);
    }
}

fn draw_happy_eye(cv: &mut Canvas, ex: f32, ey: f32, r: f32) {
    cv.stroke_arc(ex, ey + r * 0.35, r * 0.88, -155.0, 130.0, (r * 0.34).clamp(1.6, 2.3), WHITE, 0.96);
}

fn draw_cute_eyes(cv: &mut Canvas, cx: f32, cy: f32, eye_dx: f32, eye_r: f32, gaze: (f32, f32), mood: CuteMood, hot: bool) {
    let mood = if hot && mood == CuteMood::Normal { CuteMood::Happy } else { mood };
    let (lx, rx) = (cx - eye_dx, cx + eye_dx);
    match mood {
        CuteMood::Normal => {
            draw_normal_eye(cv, lx, cy, eye_r, gaze, false);
            draw_normal_eye(cv, rx, cy, eye_r, gaze, false);
        }
        CuteMood::Blink => {
            let w = (eye_r * 0.30).clamp(1.5, 2.0);
            cv.stroke_arc(lx, cy - eye_r * 0.15, eye_r * 0.85, 25.0, 130.0, w, WHITE, 0.92);
            cv.stroke_arc(rx, cy - eye_r * 0.15, eye_r * 0.85, 25.0, 130.0, w, WHITE, 0.92);
        }
        CuteMood::Happy => {
            draw_happy_eye(cv, lx, cy, eye_r);
            draw_happy_eye(cv, rx, cy, eye_r);
            cv.fill_circle(lx - eye_r * 1.15, cy + eye_r * 0.68, (eye_r * 0.34).max(1.5), BLUSH_RGB, 0.42);
            cv.fill_circle(rx + eye_r * 1.15, cy + eye_r * 0.68, (eye_r * 0.34).max(1.5), BLUSH_RGB, 0.42);
        }
        CuteMood::Wink => {
            draw_normal_eye(cv, lx, cy, eye_r, gaze, false);
            draw_happy_eye(cv, rx, cy, eye_r);
            cv.fill_circle(rx + eye_r * 1.15, cy + eye_r * 0.68, (eye_r * 0.34).max(1.5), BLUSH_RGB, 0.42);
        }
        CuteMood::Sparkle => {
            draw_normal_eye(cv, lx, cy, eye_r, gaze, true);
            draw_normal_eye(cv, rx, cy, eye_r, gaze, true);
            cv.fill_circle(lx - eye_r * 1.15, cy + eye_r * 0.68, (eye_r * 0.34).max(1.5), BLUSH_RGB, 0.45);
            cv.fill_circle(rx + eye_r * 1.15, cy + eye_r * 0.68, (eye_r * 0.34).max(1.5), BLUSH_RGB, 0.45);
        }
    }
}

pub(super) fn draw_collapsed(cv: &mut Canvas, st: &AppState, v: &View, hits: &mut Vec<Hit>) {
    let p = v.pill;
    let cy = p.y + p.h * 0.5;
    let c = civil(v.now, st.offset_min);
    let playing = st.media.as_ref().is_some_and(|m| m.playing);

    if v.dock.is_vertical() && !matches!(v.mode, Collapsed::Osd | Collapsed::Banner) {
        draw_collapsed_vertical(cv, st, v, p, &c, playing);
        return;
    }

    match v.mode {
        Collapsed::Minimal => {
            draw_cute_eyes(cv, p.x + p.w * 0.5, cy, 12.0, 6.2, v.gaze, v.cute_mood, v.hot);
        }
        Collapsed::Media => {
            cv.glyph_rotated(G_MUSIC, 12.0, Rect::new(p.x + 10.0, cy - 9.0, 18.0, 18.0), WHITE, if playing { 0.95 } else { 0.45 }, v.art_angle);
            // 4 equalizer bars on the right (4*2 + 3*2 = 14px wide).
            draw_audio_bars(cv, p.right() - 25.0, cy, &v.bars[..4], 2.0, 2.0, 13.0, playing);
            let tr = Rect::new(p.x + 30.0, p.y, (p.w - 61.0).max(10.0), p.h);
            let label = st
                .media
                .as_ref()
                .map(|m| if !m.title.is_empty() { m.title.as_str() } else { m.app.as_str() })
                .filter(|s| !s.is_empty());
            if let Some(title) = label {
                let font = Font::text(12.5, 600, Align::Left);
                let tw = cv.text_width(title, font, TextStyle::default());
                let alpha = if playing { 0.96 } else { 0.65 };
                if tw <= tr.w {
                    cv.text(title, Font::text(12.5, 600, Align::Center), tr, WHITE, alpha);
                } else {
                    cv.push_clip(tr);
                    if playing && !st.settings.reduce_motion {
                        let gap = 36.0;
                        let period = tw + gap;
                        let speed = 28.0;
                        let cycle_ms = ((1.2 + period / speed) * 1000.0).max(1.0) as i64;
                        let t_ms = v.now.rem_euclid(cycle_ms);
                        let scroll_px = if t_ms < 1200 { 0.0 } else { ((t_ms - 1200) as f32 / 1000.0) * speed };
                        cv.text(title, font, Rect::new(tr.x - scroll_px, tr.y, tw + 4.0, tr.h), WHITE, alpha);
                        cv.text(title, font, Rect::new(tr.x - scroll_px + period, tr.y, tw + 4.0, tr.h), WHITE, alpha);
                    } else {
                        cv.text(title, font, tr, WHITE, alpha);
                    }
                    cv.pop_clip();
                }
            } else {
                let time_s = format!("{:02}:{:02}", c.hour, c.minute);
                cv.text(&time_s, Font::text(13.0, 600, Align::Center), p, WHITE, 0.96);
            }
        }
        Collapsed::Countdown => {
            let cd = &st.countdown;
            let (rx, r) = (p.x + 20.0, 5.4);
            cv.stroke_circle(rx, cy, r, 2.2, WHITE, 0.24);
            cv.stroke_arc(rx, cy, r, -90.0, (1.0 - cd.progress(v.now)) * 360.0, 2.2, WHITE, 0.95);
            let alpha = if cd.is_running() { 0.98 } else { 0.62 };
            cv.text(&fmt_hms(cd.remaining(v.now)), Font::text(14.5, 700, Align::Center), p, WHITE, alpha);
            cv.glyph(G_TIMER, 10.0, Rect::new(p.right() - 30.0, cy - 9.0, 18.0, 18.0), WHITE, 0.7);
        }
        Collapsed::Stopwatch => {
            let sw = &st.stopwatch;
            // Pulsing-free "recording" dot while running.
            cv.fill_circle(p.x + 20.0, cy, 3.6, WHITE, if sw.is_running() { 0.95 } else { 0.4 });
            let e = sw.elapsed(v.now);
            // Whole seconds on the island (it only wakes once per second).
            let label = fmt_stopwatch(e - e % 1000);
            let label = label.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or(label);
            let alpha = if sw.is_running() { 0.98 } else { 0.62 };
            cv.text(&label, Font::text(14.5, 700, Align::Center), p, WHITE, alpha);
        }
        Collapsed::Todo => {
            // Today's progress: done / total.
            let today = st.data.todos.today_items(v.now, st.offset_min);
            let total = today.len().max(1);
            let done = today.iter().filter(|t| t.done).count();
            let (rx, r) = (p.x + 20.0, 5.4);
            cv.stroke_circle(rx, cy, r, 2.2, WHITE, 0.24);
            cv.stroke_arc(rx, cy, r, -90.0, done as f32 / total as f32 * 360.0, 2.2, WHITE, 0.95);
            cv.text(&format!("{done}/{}", today.len()), Font::text(13.0, 600, Align::Center), p, WHITE, 0.96);
        }
        Collapsed::Osd => {
            let Some(o) = &st.osd else { return };
            let g = match o.kind {
                OsdKind::Volume if o.muted || o.level <= 0.0 => G_MUTE,
                OsdKind::Volume => G_VOLUME,
                OsdKind::Brightness => G_BRIGHT,
            };
            cv.glyph(g, 13.5, Rect::new(p.x + 12.0, p.y, 24.0, p.h), WHITE, 0.95);
            let bar = Rect::new(p.x + 40.0, cy - 2.0, (p.w - 92.0).max(24.0), 4.0);
            let level = if o.muted { 0.0 } else { o.level.clamp(0.0, 1.0) };
            cv.fill_round(bar, 2.0, SURFACE, 1.0);
            if level > 0.005 {
                let fw = (bar.w * level).max(4.0);
                cv.fill_round(Rect::new(bar.x, bar.y, fw, bar.h), 2.0, WHITE, 1.0);
            }
            let pct = format!("{}", (level * 100.0).round() as i32);
            cv.text(&pct, Font::text(12.0, 650, Align::Right), Rect::new(p.right() - 46.0, p.y, 32.0, p.h), WHITE, 0.95);
        }
        Collapsed::Banner => {
            let Some(b) = &st.banner else { return };
            let (g, sweep) = match b.icon {
                BannerIcon::Charging | BannerIcon::BatteryLow => {
                    let pct = st.battery.flatten().map(|x| x.percent as f32 / 100.0).unwrap_or(0.5);
                    (G_BOLT, pct * 360.0)
                }
                BannerIcon::Headphones => (G_HEADPHONE, 360.0),
                BannerIcon::Reminder => (G_BELL, 360.0),
                BannerIcon::Focus => (G_TIMER, 360.0),
                BannerIcon::Break => (G_MOON, 360.0),
                BannerIcon::Alarm => (G_BELL, 360.0),
                BannerIcon::TimerDone => (G_TIMER, 360.0),
            };
            let (gx, r) = (p.x + 20.0, 8.0);
            cv.stroke_circle(gx, cy, r, 2.0, WHITE, 0.22);
            cv.stroke_arc(gx, cy, r, -90.0, sweep, 2.0, WHITE, 0.95);
            cv.glyph(g, 8.5, Rect::new(gx - 9.0, cy - 9.0, 18.0, 18.0), WHITE, 1.0);

            // One line: reminders show the task itself, other alerts their title.
            let line = if b.icon == BannerIcon::Reminder { b.body.as_str() } else { b.title.as_str() };
            let mut right = p.right() - 10.0;
            if b.todo_id.is_some() || b.alarm_id.is_some() {
                // Compact actions: snooze 5 min and done.
                let done_r = Rect::new(right - 22.0, cy - 9.0, 18.0, 18.0);
                let h_done = hover_amt(v, Action::CompleteBanner);
                cv.fill_round(done_r, 9.0, WHITE, 0.92);
                cv.glyph(G_CHECKMARK, 9.0, done_r, BG, 0.9 + 0.1 * h_done);
                hits.push(Hit { rect: done_r, action: Action::CompleteBanner });
                let snz_r = Rect::new(done_r.x - 36.0, cy - 9.0, 32.0, 18.0);
                let h_snz = hover_amt(v, Action::SnoozeBanner(5));
                cv.fill_round(snz_r, 9.0, SURFACE, 1.0);
                cv.stroke_round(snz_r, 9.0, 1.0, WHITE, 0.16 + 0.2 * h_snz);
                cv.text("+5", Font::text(11.0, 600, Align::Center), snz_r, WHITE, 0.95);
                hits.push(Hit { rect: snz_r, action: Action::SnoozeBanner(5) });
                right = snz_r.x - 6.0;
            } else if matches!(b.icon, BannerIcon::Charging | BannerIcon::BatteryLow) {
                if let Some(bat) = st.battery.flatten() {
                    cv.text(&format!("{}%", bat.percent), Font::text(12.5, 650, Align::Right), Rect::new(right - 44.0, p.y, 44.0, p.h), WHITE, 0.95);
                    right -= 50.0;
                }
            }
            let tx = p.x + 36.0;
            cv.text(line, Font::text(13.0, 600, Align::Left), Rect::new(tx, p.y, (right - tx).max(0.0), p.h), WHITE, 0.96);
            hits.push(Hit { rect: p, action: Action::DismissBanner });
        }
    }
}

fn draw_collapsed_vertical(
    cv: &mut Canvas,
    st: &AppState,
    v: &View,
    p: Rect,
    c: &ailan_core::time::Civil,
    playing: bool,
) {
    let cx = p.x + p.w * 0.5;
    match v.mode {
        Collapsed::Minimal => {
            super::draw_status_ring(cv, st, cx, p.y + 28.0);
            draw_cute_eyes(cv, cx, p.y + 62.0, 7.5, 4.4, v.gaze, v.cute_mood, v.hot);
        }
        Collapsed::Media => {
            cv.glyph(G_MUSIC, 11.5, Rect::new(cx - 8.0, p.y + 12.0, 16.0, 16.0), WHITE, if playing { 0.95 } else { 0.45 });
            cv.text(&format!("{:02}", c.hour), Font::text(14.0, 700, Align::Center), Rect::new(p.x, p.y + 32.0, p.w, 17.0), WHITE, 0.96);
            cv.text(&format!("{:02}", c.minute), Font::text(13.5, 600, Align::Center), Rect::new(p.x, p.y + 49.0, p.w, 17.0), WHITE, 0.85);
            // 5 expressive bars centered below the clock (5*2.5 + 4*2.5 = 22.5px wide).
            draw_audio_bars(cv, cx - 11.25, p.y + 84.0, &v.bars, 2.5, 2.5, 22.0, playing);
        }
        Collapsed::Countdown => {
            let cd = &st.countdown;
            let r = 6.0;
            cv.stroke_circle(cx, p.y + 24.0, r, 2.2, WHITE, 0.24);
            cv.stroke_arc(cx, p.y + 24.0, r, -90.0, (1.0 - cd.progress(v.now)) * 360.0, 2.2, WHITE, 0.95);
            let alpha = if cd.is_running() { 0.98 } else { 0.62 };
            let txt = fmt_hms(cd.remaining(v.now));
            if let Some((a, b)) = txt.rsplit_once(':') {
                cv.text(a, Font::text(12.5, 700, Align::Center), Rect::new(p.x, p.y + 44.0, p.w, 18.0), WHITE, alpha);
                cv.text(b, Font::text(12.5, 600, Align::Center), Rect::new(p.x, p.y + 64.0, p.w, 18.0), WHITE, alpha);
            } else {
                cv.text(&txt, Font::text(11.5, 700, Align::Center), Rect::new(p.x, p.y + 52.0, p.w, 20.0), WHITE, alpha);
            }
        }
        Collapsed::Stopwatch => {
            let sw = &st.stopwatch;
            cv.fill_circle(cx, p.y + 24.0, 3.8, WHITE, if sw.is_running() { 0.95 } else { 0.4 });
            let e = sw.elapsed(v.now);
            let label = fmt_stopwatch(e - e % 1000);
            let label = label.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or(label);
            let alpha = if sw.is_running() { 0.98 } else { 0.62 };
            if let Some((a, b)) = label.rsplit_once(':') {
                cv.text(a, Font::text(12.5, 700, Align::Center), Rect::new(p.x, p.y + 44.0, p.w, 18.0), WHITE, alpha);
                cv.text(b, Font::text(12.5, 600, Align::Center), Rect::new(p.x, p.y + 64.0, p.w, 18.0), WHITE, alpha);
            } else {
                cv.text(&label, Font::text(11.5, 700, Align::Center), Rect::new(p.x, p.y + 52.0, p.w, 20.0), WHITE, alpha);
            }
        }
        Collapsed::Todo => {
            let today = st.data.todos.today_items(v.now, st.offset_min);
            let total = today.len().max(1);
            let done = today.iter().filter(|t| t.done).count();
            let r = 6.0;
            cv.stroke_circle(cx, p.y + 26.0, r, 2.2, WHITE, 0.24);
            cv.stroke_arc(cx, p.y + 26.0, r, -90.0, done as f32 / total as f32 * 360.0, 2.2, WHITE, 0.95);
            cv.text(&format!("{done}/{}", today.len()), Font::text(11.5, 600, Align::Center), Rect::new(p.x, p.y + 52.0, p.w, 22.0), WHITE, 0.96);
        }
        Collapsed::Osd | Collapsed::Banner => {}
    }
}
