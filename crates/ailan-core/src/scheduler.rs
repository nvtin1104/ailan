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
    add(s.data.alarms.next_due(now, s.offset_min));
    add(s.countdown.ends_at());
    add(s.save_due());

    if !view.hidden {
        add(s.osd.as_ref().map(|o| o.until));
        add(s.banner.as_ref().map(|b| b.until));

        // Only the expanded header shows the wall clock.
        if view.expanded {
            add(Some(time::next_minute(now)));
        }
        // "Today's tasks" roll over at local midnight.
        if s.collapsed(now) == Collapsed::Todo {
            add(Some(time::local_midnight(now, s.offset_min) + time::DAY));
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
            }
        }

        // Countdown / stopwatch on the collapsed island show seconds.
        let ticking = match s.collapsed(now) {
            Collapsed::Countdown => s.countdown.is_running(),
            Collapsed::Stopwatch => s.stopwatch.is_running(),
            _ => false,
        };
        if ticking {
            add(Some(now + 1000 - now.rem_euclid(1000)));
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
    use crate::settings::{Data, Settings};
    use crate::time::MINUTE;

    fn view(expanded: bool, tab: Tab) -> View {
        View { expanded, tab, hidden: false }
    }

    #[test]
    fn expanded_header_clock_ticks_each_minute() {
        let s = AppState::new(Settings::default(), Data::default(), 0);
        assert_eq!(next_deadline(&s, 61_000, view(true, Tab::Todo)), Some(120_000));
    }

    #[test]
    fn todo_island_wakes_at_midnight() {
        let mut s = AppState::new(Settings::default(), Data::default(), 0);
        s.data.todos.add_today("x".into(), None, 1_000, 0);
        assert_eq!(next_deadline(&s, 1_000, view(false, Tab::Todo)), Some(time::DAY));
    }

    #[test]
    fn minimal_and_hidden_idle_never_wake() {
        let s = AppState::new(Settings::default(), Data::default(), 0);
        // Idle rests in Minimal (eyes) with no clock -> no wakeup.
        assert_eq!(next_deadline(&s, 0, view(false, Tab::Music)), None);
        assert_eq!(next_deadline(&s, 0, View { expanded: false, tab: Tab::Music, hidden: true }), None);
    }

    #[test]
    fn timer_ticks_per_minute_collapsed_and_per_second_expanded() {
        let mut s = AppState::new(Settings::default(), Data::default(), 0);
        s.pomodoro.start(0);
        // Collapsed shows whole minutes: wake at the next minute boundary of the countdown.
        assert_eq!(next_deadline(&s, 500, view(false, Tab::Music)), Some(MINUTE));
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
        assert_eq!(next_deadline(&s, 10, view(true, Tab::Music)), Some(1010));
    }

    #[test]
    fn alarms_and_timers_wake_even_when_hidden_and_tick_seconds_when_shown() {
        let mut s = AppState::new(Settings::default(), Data::default(), 0);
        s.data.alarms.add(0, 3, String::new(), 0);
        let hidden = View { expanded: false, tab: Tab::Music, hidden: true };
        assert_eq!(next_deadline(&s, 0, hidden), Some(3 * MINUTE));
        s.countdown.toggle(0); // 5 min default
        assert_eq!(next_deadline(&s, 0, hidden), Some(3 * MINUTE));
        assert_eq!(next_deadline(&s, 1_500, view(false, Tab::Music)), Some(2_000), "per-second while shown");
    }

    #[test]
    fn reminders_wake_even_when_hidden() {
        let mut s = AppState::new(Settings::default(), Data::default(), 0);
        s.data.todos.add("x".into(), Some(5_000));
        assert_eq!(next_deadline(&s, 0, View { expanded: false, tab: Tab::Music, hidden: true }), Some(5_000));
    }
}
