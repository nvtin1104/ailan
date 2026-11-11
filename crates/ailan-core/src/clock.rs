//! Clock tools: alarms (persisted), countdown timer, stopwatch and world clocks.
//! Pure logic; the shell feeds in `now` (epoch ms) and the local UTC offset.

use serde::{Deserialize, Serialize};

use crate::time::{civil, local_midnight, DAY, HOUR, MINUTE};

/// Weekday mask helpers: bit 0 = Monday … bit 6 = Sunday. `0` = rings once.
pub const EVERY_DAY: u8 = 0x7F;
pub const WEEKDAYS: u8 = 0x1F;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alarm {
    pub id: u64,
    pub hour: u8,
    pub minute: u8,
    #[serde(default)]
    pub label: String,
    /// Repeat days (bit 0 = Monday). `0` rings once, then switches itself off.
    #[serde(default)]
    pub days: u8,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub snoozed_until: Option<i64>,
    /// Last time this alarm rang (prevents ringing twice for the same slot).
    #[serde(default)]
    pub last_fired: Option<i64>,
}

fn yes() -> bool {
    true
}

impl Alarm {
    /// Next time this alarm rings strictly after `after`, or `None` when switched off.
    pub fn next_ring(&self, after: i64, offset_min: i32) -> Option<i64> {
        if !self.enabled {
            return None;
        }
        let at = self.hour as i64 * HOUR + self.minute as i64 * MINUTE;
        let today = local_midnight(after, offset_min);
        let slot = (0..8).map(|d| today + d * DAY + at).find(|&t| {
            t > after
                && (self.days == 0 || self.days & (1 << civil(t, offset_min).weekday) != 0)
                && self.last_fired.is_none_or(|f| t > f)
        });
        match (self.snoozed_until, slot) {
            (Some(s), Some(t)) if s > after => Some(s.min(t)),
            (Some(s), None) if s > after => Some(s),
            (_, t) => t,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Alarms {
    pub items: Vec<Alarm>,
    pub next_id: u64,
}

impl Alarms {
    pub fn add(&mut self, hour: u8, minute: u8, label: String, days: u8) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Alarm {
            id,
            hour: hour.min(23),
            minute: minute.min(59),
            label,
            days: days & EVERY_DAY,
            enabled: true,
            snoozed_until: None,
            last_fired: None,
        });
        id
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Alarm> {
        self.items.iter_mut().find(|a| a.id == id)
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let n = self.items.len();
        self.items.retain(|a| a.id != id);
        self.items.len() != n
    }

    pub fn toggle(&mut self, id: u64) {
        if let Some(a) = self.get_mut(id) {
            a.enabled = !a.enabled;
            a.snoozed_until = None;
        }
    }

    /// Shift the alarm time by `minutes` (wraps around the day).
    pub fn shift(&mut self, id: u64, minutes: i32) {
        if let Some(a) = self.get_mut(id) {
            let total = (a.hour as i32 * 60 + a.minute as i32 + minutes).rem_euclid(24 * 60);
            a.hour = (total / 60) as u8;
            a.minute = (total % 60) as u8;
            a.enabled = true;
            a.snoozed_until = None;
        }
    }

    pub fn toggle_day(&mut self, id: u64, weekday: u8) {
        if let Some(a) = self.get_mut(id) {
            a.days ^= 1 << (weekday % 7);
            a.enabled = true;
        }
    }

    pub fn snooze(&mut self, id: u64, now: i64, minutes: u32) {
        if let Some(a) = self.get_mut(id) {
            a.snoozed_until = Some(now + minutes as i64 * MINUTE);
            a.enabled = true;
        }
    }

    /// Earliest upcoming ring of any alarm.
    pub fn next_due(&self, now: i64, offset_min: i32) -> Option<i64> {
        self.items.iter().filter_map(|a| a.next_ring(now - 1, offset_min)).min()
    }

    /// Alarms ringing at `now`: returns `(id, label)` and marks them fired. One-shot alarms
    /// switch themselves off afterwards.
    pub fn take_due(&mut self, now: i64, offset_min: i32) -> Vec<(u64, String)> {
        let mut out = Vec::new();
        for a in &mut self.items {
            // Look back a minute so a slightly late wake-up still rings.
            let Some(t) = a.next_ring(now - MINUTE, offset_min) else { continue };
            if t <= now {
                a.last_fired = Some(t);
                a.snoozed_until = None;
                if a.days == 0 {
                    a.enabled = false;
                }
                out.push((a.id, a.label.clone()));
            }
        }
        out
    }

    /// Alarms ordered by time of day.
    pub fn sorted(&self) -> Vec<&Alarm> {
        let mut v: Vec<&Alarm> = self.items.iter().collect();
        v.sort_by_key(|a| (a.hour, a.minute, a.id));
        v
    }
}

/// Countdown timer (not persisted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Countdown {
    /// Configured length, ms.
    pub duration: i64,
    ends_at: Option<i64>,
    remaining: i64,
}

impl Default for Countdown {
    fn default() -> Self {
        Self::new(5 * MINUTE)
    }
}

impl Countdown {
    pub const MAX: i64 = 24 * HOUR - 1000;

    pub fn new(duration: i64) -> Self {
        let d = duration.clamp(1000, Self::MAX);
        Self { duration: d, ends_at: None, remaining: d }
    }

    pub fn is_running(&self) -> bool {
        self.ends_at.is_some()
    }

    /// Started and not yet reset (running or paused mid-way).
    pub fn is_active(&self) -> bool {
        self.is_running() || self.remaining != self.duration
    }

    pub fn ends_at(&self) -> Option<i64> {
        self.ends_at
    }

    pub fn remaining(&self, now: i64) -> i64 {
        self.ends_at.map_or(self.remaining, |e| (e - now).max(0))
    }

    pub fn progress(&self, now: i64) -> f32 {
        (1.0 - self.remaining(now) as f32 / self.duration.max(1) as f32).clamp(0.0, 1.0)
    }

    /// Set a new length (only while idle).
    pub fn set_duration(&mut self, duration: i64) {
        if !self.is_active() {
            *self = Self::new(duration);
        }
    }

    pub fn toggle(&mut self, now: i64) {
        match self.ends_at.take() {
            Some(e) => self.remaining = (e - now).max(0),
            None if self.remaining > 0 => self.ends_at = Some(now + self.remaining),
            None => {}
        }
    }

    pub fn reset(&mut self) {
        self.ends_at = None;
        self.remaining = self.duration;
    }

    /// Add time to a running/paused timer (or lengthen an idle one).
    pub fn add(&mut self, now: i64, ms: i64) {
        if !self.is_active() {
            self.set_duration(self.duration + ms);
            return;
        }
        self.duration = (self.duration + ms).min(Self::MAX);
        match &mut self.ends_at {
            Some(e) => *e = (*e + ms).min(now + Self::MAX),
            None => self.remaining = (self.remaining + ms).min(Self::MAX),
        }
    }

    /// Returns `true` once when the countdown reaches zero (it then resets).
    pub fn poll(&mut self, now: i64) -> bool {
        match self.ends_at {
            Some(e) if e <= now => {
                self.reset();
                true
            }
            _ => false,
        }
    }
}

/// Stopwatch with laps (not persisted).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stopwatch {
    started_at: Option<i64>,
    accumulated: i64,
    /// Total elapsed at each lap mark, ms.
    pub laps: Vec<i64>,
}

impl Stopwatch {
    pub fn is_running(&self) -> bool {
        self.started_at.is_some()
    }

    pub fn is_active(&self) -> bool {
        self.is_running() || self.accumulated > 0
    }

    pub fn elapsed(&self, now: i64) -> i64 {
        self.accumulated + self.started_at.map_or(0, |s| (now - s).max(0))
    }

    pub fn toggle(&mut self, now: i64) {
        match self.started_at.take() {
            Some(s) => self.accumulated += (now - s).max(0),
            None => self.started_at = Some(now),
        }
    }

    pub fn lap(&mut self, now: i64) {
        if self.is_running() {
            self.laps.push(self.elapsed(now));
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Individual lap durations (each lap minus the previous mark).
    pub fn lap_splits(&self) -> Vec<i64> {
        let mut prev = 0;
        self.laps
            .iter()
            .map(|&t| {
                let d = t - prev;
                prev = t;
                d
            })
            .collect()
    }
}

/// A city shown in the world clock; `tz` is a Windows time-zone id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldClock {
    pub city: String,
    pub tz: String,
}

/// Cities offered in the world-clock picker: `(city, Windows time-zone id)`.
pub const CITY_PRESETS: [(&str, &str); 16] = [
    ("Hà Nội", "SE Asia Standard Time"),
    ("Tokyo", "Tokyo Standard Time"),
    ("Seoul", "Korea Standard Time"),
    ("Singapore", "Singapore Standard Time"),
    ("Bắc Kinh", "China Standard Time"),
    ("Sydney", "AUS Eastern Standard Time"),
    ("Dubai", "Arabian Standard Time"),
    ("Mumbai", "India Standard Time"),
    ("Moscow", "Russian Standard Time"),
    ("Berlin", "W. Europe Standard Time"),
    ("Paris", "Romance Standard Time"),
    ("London", "GMT Standard Time"),
    ("New York", "Eastern Standard Time"),
    ("Chicago", "Central Standard Time"),
    ("Los Angeles", "Pacific Standard Time"),
    ("São Paulo", "E. South America Standard Time"),
];

/// `mm:ss`, or `h:mm:ss` from one hour.
pub fn fmt_hms(ms: i64) -> String {
    let s = (ms.max(0) + 999) / 1000;
    let (h, m, sec) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m:02}:{sec:02}")
    }
}

/// Stopwatch format `mm:ss.d` (or `h:mm:ss.d`), truncating.
pub fn fmt_stopwatch(ms: i64) -> String {
    let ms = ms.max(0);
    let (h, m, s, d) = (ms / 3_600_000, (ms / 60_000) % 60, (ms / 1000) % 60, (ms / 100) % 10);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{d}")
    } else {
        format!("{m:02}:{s:02}.{d}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_794_294_309_000; // Tue 2026-11-10 14:05 at +7
    const OFF: i32 = 420;

    #[test]
    fn one_shot_alarm_rings_once_then_turns_off() {
        let mut a = Alarms::default();
        let id = a.add(14, 30, "tea".into(), 0);
        let ring = a.next_due(NOW, OFF).unwrap();
        let c = civil(ring, OFF);
        assert_eq!((c.day, c.hour, c.minute), (10, 14, 30));
        assert!(a.take_due(ring - 1000, OFF).is_empty());
        assert_eq!(a.take_due(ring, OFF), vec![(id, "tea".to_string())]);
        assert!(!a.items[0].enabled, "one-shot switches off");
        assert_eq!(a.next_due(ring, OFF), None);
    }

    #[test]
    fn repeating_alarm_skips_unselected_days() {
        let mut a = Alarms::default();
        // Wednesday + Friday 07:00 → from Tue 14:05 the next is Wed 11th.
        a.add(7, 0, String::new(), (1 << 2) | (1 << 4));
        let ring = a.next_due(NOW, OFF).unwrap();
        let c = civil(ring, OFF);
        assert_eq!((c.day, c.weekday, c.hour), (11, 2, 7));
        assert_eq!(a.take_due(ring, OFF).len(), 1);
        assert!(a.items[0].enabled, "repeating stays on");
        let c = civil(a.next_due(ring, OFF).unwrap(), OFF);
        assert_eq!((c.day, c.weekday), (13, 4), "then Friday");
    }

    #[test]
    fn snooze_shift_and_days() {
        let mut a = Alarms::default();
        let id = a.add(23, 50, String::new(), 0);
        a.shift(id, 20);
        assert_eq!((a.items[0].hour, a.items[0].minute), (0, 10), "wraps past midnight");
        a.shift(id, -15);
        assert_eq!((a.items[0].hour, a.items[0].minute), (23, 55));
        a.toggle_day(id, 0);
        assert_eq!(a.items[0].days, 1);
        a.snooze(id, NOW, 5);
        assert_eq!(a.next_due(NOW, OFF), Some(NOW + 5 * MINUTE));
        a.toggle(id);
        assert_eq!(a.next_due(NOW, OFF), None);
    }

    #[test]
    fn countdown_runs_pauses_and_finishes_once() {
        let mut c = Countdown::new(3 * MINUTE);
        assert!(!c.is_active());
        c.toggle(0);
        assert_eq!(c.remaining(60_000), 2 * MINUTE);
        c.toggle(60_000);
        assert!(c.is_active() && !c.is_running());
        assert_eq!(c.remaining(999_999), 2 * MINUTE, "paused holds");
        c.toggle(100_000);
        c.add(100_000, MINUTE);
        assert_eq!(c.ends_at(), Some(100_000 + 3 * MINUTE));
        assert!(!c.poll(100_000 + 3 * MINUTE - 1));
        assert!(c.poll(100_000 + 3 * MINUTE));
        assert!(!c.poll(100_000 + 4 * MINUTE), "fires once");
        assert!(!c.is_active());
    }

    #[test]
    fn stopwatch_laps() {
        let mut s = Stopwatch::default();
        s.toggle(1_000);
        s.lap(4_000);
        s.lap(9_000);
        s.toggle(10_000);
        assert_eq!(s.elapsed(50_000), 9_000);
        assert_eq!(s.lap_splits(), vec![3_000, 5_000]);
        s.reset();
        assert!(!s.is_active());
    }

    #[test]
    fn formats() {
        assert_eq!(fmt_hms(65_000), "01:05");
        assert_eq!(fmt_hms(3_725_000), "1:02:05");
        assert_eq!(fmt_hms(500), "00:01", "rounds up so 0 shows only at the end");
        assert_eq!(fmt_stopwatch(61_234), "01:01.2");
    }
}
