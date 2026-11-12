//! Collapsed notch pill rendering (minimal eyes, media, focus timer, todo, OSD, banners).

use ailan_core::clock::{fmt_hms, fmt_stopwatch};
use ailan_core::state::{BannerIcon, Collapsed, OsdKind};
use ailan_core::time::{civil, fmt_duration};
use ailan_core::AppState;

use crate::gfx::{Align, Canvas, Font, Rect};
use super::{draw_status_ring, draw_wave_bars, hover_amt, Action, Hit, View, BG, G_BELL, G_BOLT, G_BRIGHT, G_CHECKMARK, G_HEADPHONE, G_MOON, G_MUTE, G_TIMER, G_VOLUME, WHITE};

pub(super) fn draw_collapsed(cv: &mut Canvas, st: &AppState, v: &View, hits: &mut Vec<Hit>) {
    let p = v.pill;
    let cy = p.y + p.h * 0.5;
    let c = civil(v.now, st.offset_min);
    let playing = st.media.as_ref().is_some_and(|m| m.playing);

    match v.mode {
        Collapsed::Minimal => {
            // Idle: just the eyes. They blink every ~4s and look toward the pointer.
            let phase_ms = v.now.rem_euclid(4_200);
            let blink = if (3_900..4_080).contains(&phase_ms) {
                let t = (phase_ms - 3_900) as f32 / 90.0;
                (1.0 - t).abs().clamp(0.12, 1.0)
            } else {
                1.0
            };
            let eye_w = 8.2;
            let eye_h = 12.2 * blink;
            let gap = 14.5;
            let cx = p.x + p.w * 0.5 + v.eye.0;
            let ecy = cy + v.eye.1;
            let left_eye = Rect::new(cx - gap * 0.5 - eye_w, ecy - eye_h * 0.5, eye_w, eye_h);
            let right_eye = Rect::new(cx + gap * 0.5, ecy - eye_h * 0.5, eye_w, eye_h);
            cv.fill_round(left_eye, (eye_w * 0.5).min(eye_h * 0.5), WHITE, 0.96);
            cv.fill_round(right_eye, (eye_w * 0.5).min(eye_h * 0.5), WHITE, 0.96);
        }
        Collapsed::Media => {
            draw_status_ring(cv, st, p.x + 20.0, cy);
            let time_s = format!("{:02}:{:02}", c.hour, c.minute);
            cv.text(&time_s, Font::text(14.5, 700, Align::Center), p, WHITE, 0.98);
            draw_wave_bars(cv, v, Rect::new(p.right() - 28.0, cy - 7.0, 20.0, 14.0), playing);
        }
        Collapsed::Timer => {
            // Focus progress ring + remaining time.
            let (rx, r) = (p.x + 20.0, 5.4);
            cv.stroke_circle(rx, cy, r, 2.2, WHITE, 0.24);
            cv.stroke_arc(rx, cy, r, -90.0, st.pomodoro.progress(v.now).clamp(0.0, 1.0) * 360.0, 2.2, WHITE, 0.95);
            let label = fmt_duration(st.pomodoro.remaining(v.now));
            let alpha = if st.pomodoro.is_running() { 0.98 } else { 0.62 };
            cv.text(&label, Font::text(14.5, 700, Align::Center), p, WHITE, alpha);
            draw_wave_bars(cv, v, Rect::new(p.right() - 28.0, cy - 7.0, 20.0, 14.0), playing);
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
            cv.text(&format!("{done}/{}", today.len()), Font::text(14.5, 700, Align::Center), p, WHITE, 0.98);
            draw_wave_bars(cv, v, Rect::new(p.right() - 28.0, cy - 7.0, 20.0, 14.0), playing);
        }
        Collapsed::Osd => {
            let Some(o) = &st.osd else { return };
            let g = match o.kind {
                OsdKind::Volume if o.muted || o.level <= 0.0 => G_MUTE,
                OsdKind::Volume => G_VOLUME,
                OsdKind::Brightness => G_BRIGHT,
            };
            cv.glyph(g, 13.5, Rect::new(p.x + 12.0, p.y, 24.0, p.h), WHITE, 0.95);
            let bar = Rect::new(p.x + 42.0, cy - 2.0, p.w - 92.0, 4.0);
            let level = if o.muted { 0.0 } else { o.level.clamp(0.0, 1.0) };
            cv.fill_round(bar, 2.0, WHITE, 0.22);
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
                let done_r = Rect::new(right - 24.0, cy - 11.0, 24.0, 22.0);
                let h_done = hover_amt(v, Action::CompleteBanner);
                cv.fill_round(done_r, 11.0, WHITE, 0.90 + 0.10 * h_done);
                cv.glyph(G_CHECKMARK, 9.0, done_r, BG, 1.0);
                hits.push(Hit { rect: done_r.inset(-3.0, -5.0), action: Action::CompleteBanner });
                let snz_r = Rect::new(done_r.x - 34.0, cy - 11.0, 30.0, 22.0);
                let h_snz = hover_amt(v, Action::SnoozeBanner(5));
                cv.fill_round(snz_r, 11.0, WHITE, 0.14 + 0.10 * h_snz);
                cv.text("+5", Font::text(10.5, 650, Align::Center), snz_r, WHITE, 0.95);
                hits.push(Hit { rect: snz_r.inset(-2.0, -5.0), action: Action::SnoozeBanner(5) });
                right = snz_r.x - 6.0;
            } else if matches!(b.icon, BannerIcon::Charging | BannerIcon::BatteryLow) {
                if let Some(bat) = st.battery.flatten() {
                    cv.text(&format!("{}%", bat.percent), Font::text(12.5, 650, Align::Right), Rect::new(right - 44.0, p.y, 44.0, p.h), WHITE, 0.95);
                    right -= 50.0;
                }
            }
            let tx = p.x + 36.0;
            cv.text(line, Font::text(12.5, 600, Align::Left), Rect::new(tx, p.y, (right - tx).max(0.0), p.h), WHITE, 0.96);
            hits.push(Hit { rect: p, action: Action::DismissBanner });
        }
    }
}
