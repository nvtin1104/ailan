//! Computes the single next moment the app must wake up. The shell arms one
//! coalescable timer for it, so an idle notch wakes at most once a minute.

use crate::settings::Tab;
use crate::state::{AppState, Collapsed};
use crate::time;

/// What the shell is currently showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct View {
    pub expanded: bool,
    pub tab: Tab,
    /// Notch hidden (fullscreen app); only non-visual deadlines matter.
    pub hidden: bool,
}

pub fn next_deadline(s: &AppState, now: i64, view: View) -> Option<i64> {
    let mut best: Option<i64> = None;
    let mut add = |t: Option<i64>| {
        if let Some(t) = t {
            best = Some(best.map_or(t, |b: i64| b.min(t)));
        }
    };

    // Always-on, even when hidden: timers, reminders, saves.
    add(s.pomodoro.ends_at());
    add(s.data.todos.next_due());
    add(s.save_due());

    if !view.hidden {
        add(s.osd.as_ref().map(|o| o.until));
        add(s.banner.as_ref().map(|b| b.until));
        add(s.pin_anim.as_ref().map(|p| p.start + crate::state::PIN_PEEK_MS));

        let mode = s.collapsed(now);
        let shows_clock = (view.expanded && s.settings.hover_ui.show_clock)
            || mode == Collapsed::Clock
            || (mode == Collapsed::Minimal
                && !s.settings.minimal_ui.hide_when_idle
                && s.settings.minimal_ui.show_clock);
        if shows_clock {
            add(Some(time::next_minute(now)));
        }

        if s.pomodoro.is_running() {
            let rem = s.pomodoro.remaining(now);
            if view.expanded && view.tab == Tab::Focus {
                // Tick on each displayed second.
                let r = rem % 1000;
                add(Some(now + if r == 0 { 1000 } else { r }));
            } else {
                // Collapsed shows whole minutes remaining.
                let r = rem % time::MINUTE;
                add(Some(now + if r == 0 { time::MINUTE } else { r }));
                // Also wake when the 5s minute-peek window finishes so it can switch back to Music.
                if s.settings.priority.smart_minute_peek {
                    add(s.focus_peek_deadline(now));
                }
            }
        }

        if view.expanded && view.tab == Tab::Music && s.media.as_ref().is_some_and(|m| m.playing) {
            add(Some(now + 1000));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AppEvent, MediaInfo};
    use crate::settings::{Data, NotchMode, Settings};
    use crate::time::MINUTE;

    fn view(expanded: bool, tab: Tab) -> View {
        View { expanded, tab, hidden: false }
    }

    #[test]
    fn idle_wakes_once_per_minute_when_clock_shown() {
        let mut s = AppState::new(Settings::default(), Data::default(), 0);
        s.settings.mode = NotchMode::AlwaysDynamic;
        assert_eq!(next_deadline(&s, 61_000, view(false, Tab::Music)), Some(120_000));
    }

    #[test]
    fn minimal_and_hidden_idle_never_wake() {
        let s = AppState::new(Settings::default(), Data::default(), 0);
        // Default mode is DynamicWhenActive -> idle is Minimal with no clock -> no wakeup!
        assert_eq!(next_deadline(&s, 0, view(false, Tab::Music)), None);
        assert_eq!(next_deadline(&s, 0, View { expanded: false, tab: Tab::Music, hidden: true }), None);
    }

    #[test]
    fn timer_ticks_per_second_only_when_visible_and_wakes_after_peek() {
        let mut s = AppState::new(Settings::default(), Data::default(), 0);
        s.pomodoro.start(0);
        // At t=500ms, Focus is inside the 5_000ms peek window -> wakes at 5_000ms!
        assert_eq!(next_deadline(&s, 500, view(false, Tab::Music)), Some(5_000));
        // After 5_000ms (e.g. t=6_000ms), wakes at the next whole minute (60_000ms).
        assert_eq!(next_deadline(&s, 6_000, view(false, Tab::Music)), Some(MINUTE));
        // Expanded on Focus: next second.
        assert_eq!(next_deadline(&s, 500, view(true, Tab::Focus)), Some(1000));
    }

    #[test]
    fn media_progress_ticks_only_on_music_tab() {
        let mut s = AppState::new(Settings::default(), Data::default(), 0);
        s.apply(
            AppEvent::Media(Some(MediaInfo {
                title: "t".into(),
                artist: "a".into(),
                app: "x".into(),
                playing: true,
                position_ms: 0,
                duration_ms: 10_000,
                sampled_at: 0,
                track_key: "t".into(),
            })),
            0,
        );
        assert_eq!(next_deadline(&s, 10, view(false, Tab::Music)), None);
        assert_eq!(next_deadline(&s, 10, view(true, Tab::Music)), Some(60_000.min(1010)));
    }

    #[test]
    fn reminders_wake_even_when_hidden() {
        let mut s = AppState::new(Settings::default(), Data::default(), 0);
        s.data.todos.add("x".into(), Some(5_000));
        assert_eq!(next_deadline(&s, 0, View { expanded: false, tab: Tab::Music, hidden: true }), Some(5_000));
    }
}
