//! Application state and the event reducer.

use std::sync::Arc;

use crate::clock::{Countdown, Stopwatch};
use crate::event::{AppEvent, Artwork, Battery, MediaInfo, Network};
use crate::i18n::{t, Key, Lang};
use crate::settings::{Data, Settings, Tab};

pub const OSD_MS: i64 = 1_500;
pub const BANNER_MS: i64 = 4_000;
pub const REMINDER_BANNER_MS: i64 = 25_000;
/// Alarms and finished timers keep ringing on the island for a minute.
pub const ALARM_BANNER_MS: i64 = 60_000;
pub const SAVE_DEBOUNCE_MS: i64 = 1_000;
pub const LOW_BATTERY: u8 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsdKind {
    Volume,
    Brightness,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Osd {
    pub kind: OsdKind,
    pub level: f32,
    pub muted: bool,
    pub until: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerIcon {
    Charging,
    BatteryLow,
    Headphones,
    Reminder,
    Focus,
    Break,
    Alarm,
    TimerDone,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Banner {
    pub icon: BannerIcon,
    pub title: String,
    pub body: String,
    pub until: i64,
    pub todo_id: Option<u64>,
    /// Set when the banner is a ringing alarm (snooze / dismiss act on it).
    pub alarm_id: Option<u64>,
}

/// What the collapsed notch is showing, in priority order. With nothing going on the
/// island rests in `Minimal` (the eyes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Collapsed {
    Osd,
    Banner,
    Countdown,
    Stopwatch,
    Media,
    Todo,
    Minimal,
}

/// Fixed activity priority: Music, then To-do.
pub const ACTIVITY_ORDER: [Tab; 2] = [Tab::Music, Tab::Todo];

/// A user-facing notification the shell should also surface outside the notch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

#[derive(Debug)]
pub struct AppState {
    pub settings: Settings,
    pub data: Data,
    /// Local UTC offset in minutes (refreshed by the shell on time-zone change).
    pub offset_min: i32,

    pub media: Option<MediaInfo>,
    pub art: Option<Arc<Artwork>>,
    pub volume: Option<(f32, bool)>,
    pub brightness: Option<u8>,
    /// Outer `None`: unknown yet. Inner `None`: no battery.
    pub battery: Option<Option<Battery>>,
    pub network: Network,
    pub audio_device: Option<(String, bool)>,

    pub countdown: Countdown,
    pub stopwatch: Stopwatch,
    /// UTC offsets (minutes) of `data.world_clocks`, refreshed by the shell (DST-aware).
    pub world_offsets: Vec<Option<i32>>,
    pub osd: Option<Osd>,
    pub banner: Option<Banner>,

    save_due: Option<i64>,
    settings_dirty: bool,
    data_dirty: bool,
}

impl AppState {
    pub fn new(settings: Settings, data: Data, offset_min: i32) -> Self {
        Self {
            settings,
            data,
            offset_min,
            media: None,
            art: None,
            volume: None,
            brightness: None,
            battery: None,
            network: Network::default(),
            audio_device: None,
            countdown: Countdown::default(),
            stopwatch: Stopwatch::default(),
            world_offsets: Vec::new(),
            osd: None,
            banner: None,
            save_due: None,
            settings_dirty: false,
            data_dirty: false,
        }
    }

    pub fn lang(&self) -> Lang {
        self.settings.lang()
    }

    fn banner(&mut self, icon: BannerIcon, title: impl Into<String>, body: impl Into<String>, until: i64) {
        self.banner_with_id(icon, title, body, until, None);
    }

    fn banner_with_id(
        &mut self,
        icon: BannerIcon,
        title: impl Into<String>,
        body: impl Into<String>,
        until: i64,
        todo_id: Option<u64>,
    ) {
        self.banner = Some(Banner {
            icon,
            title: title.into(),
            body: body.into(),
            until,
            todo_id,
            alarm_id: None,
        });
    }

    pub fn snooze_banner(&mut self, minutes: u32, now: i64) {
        let ids = self.banner.as_ref().map(|b| (b.todo_id, b.alarm_id));
        if let Some((todo, alarm)) = ids {
            if let Some(id) = todo {
                self.data.todos.snooze(id, now, minutes);
                self.mark_data_dirty(now);
            }
            if let Some(id) = alarm {
                self.data.alarms.snooze(id, now, minutes);
                self.mark_data_dirty(now);
            }
        }
        self.banner = None;
    }

    /// "Done" on a banner: completes the reminder's task; for an alarm it just stops ringing.
    pub fn complete_banner(&mut self, now: i64) {
        let todo = self.banner.as_ref().and_then(|b| b.todo_id);
        if let Some(id) = todo {
            self.data.todos.toggle(id, now, self.offset_min);
            self.mark_data_dirty(now);
        }
        self.banner = None;
    }

    /// Whether an activity is live on the collapsed island right now.
    /// - Focus: a session is on (running or paused).
    /// - Music: media is playing.
    /// - To-do: unfinished tasks remain for today.
    pub fn is_activity_running(&self, tab: Tab, now: i64) -> bool {
        match tab {
            Tab::Music => self.media.as_ref().is_some_and(|m| m.playing),
            Tab::Todo => self.data.todos.today_items(now, self.offset_min).iter().any(|t| !t.done),
        }
    }

    /// Whether an activity appears as a page in the Hover (Expanded) UI.
    /// To-do is only shown while there are unfinished tasks for today.
    pub fn is_activity_visible_on_hover(&self, tab: Tab, now: i64) -> bool {
        match tab {
            Tab::Music => self.media.as_ref().is_some_and(|m| m.playing || !m.title.is_empty()),
            Tab::Todo => !self.data.todos.today_pending(now, self.offset_min).is_empty(),
        }
    }

    /// Activities visible in the Hover UI, in fixed priority order.
    pub fn hover_activities(&self, now: i64) -> Vec<Tab> {
        ACTIVITY_ORDER.into_iter().filter(|&t| self.is_activity_visible_on_hover(t, now)).collect()
    }

    /// Live activities in fixed priority order.
    pub fn running_activities(&self, now: i64) -> Vec<Tab> {
        ACTIVITY_ORDER.into_iter().filter(|&t| self.is_activity_running(t, now)).collect()
    }

    /// True when at least one activity is live.
    pub fn has_running_activity(&self, now: i64) -> bool {
        ACTIVITY_ORDER.iter().any(|&t| self.is_activity_running(t, now))
    }

    /// Apply a provider event. Returns `true` if anything visible changed.
    pub fn apply(&mut self, ev: AppEvent, now: i64) -> bool {
        let lang = self.lang();
        match ev {
            AppEvent::Media(m) => {
                let key_changed = self.media.as_ref().map(|x| &x.track_key) != m.as_ref().map(|x| &x.track_key);
                if key_changed && self.art.as_ref().map(|a| &a.track_key) != m.as_ref().map(|x| &x.track_key) {
                    self.art = None;
                }
                let changed = self.media != m;
                self.media = m;
                changed
            }
            AppEvent::Artwork(a) => {
                // Ignore late artwork for a track that is no longer current.
                if let (Some(a), Some(m)) = (&a, &self.media) {
                    if a.track_key != m.track_key {
                        return false;
                    }
                }
                self.art = a;
                true
            }
            AppEvent::Volume { level, muted } => {
                let level = level.clamp(0.0, 1.0);
                let prev = self.volume.replace((level, muted));
                if let Some((pl, pm)) = prev {
                    if (pl - level).abs() > 0.001 || pm != muted {
                        self.osd = Some(Osd { kind: OsdKind::Volume, level, muted, until: now + OSD_MS });
                        return true;
                    }
                    return false;
                }
                true
            }
            AppEvent::Brightness(b) => {
                let b = b.min(100);
                let prev = self.brightness.replace(b);
                if prev.is_some_and(|p| p != b) {
                    self.osd =
                        Some(Osd { kind: OsdKind::Brightness, level: b as f32 / 100.0, muted: false, until: now + OSD_MS });
                }
                prev != Some(b)
            }
            AppEvent::Battery(b) => {
                let prev = self.battery.replace(b);
                if let (Some(Some(p)), Some(n)) = (prev, b) {
                    if n.charging && !p.charging {
                        self.banner(BannerIcon::Charging, t(lang, Key::Charging), format!("{}%", n.percent), now + BANNER_MS);
                    } else if !n.charging && n.percent <= LOW_BATTERY && (p.percent > LOW_BATTERY || p.charging) {
                        self.banner(BannerIcon::BatteryLow, t(lang, Key::BatteryLow), format!("{}%", n.percent), now + BANNER_MS);
                    }
                }
                prev != Some(b)
            }
            AppEvent::Network(n) => {
                let changed = self.network != n;
                self.network = n;
                changed
            }
            AppEvent::AudioDevice { name, headphones } => {
                let prev = self.audio_device.replace((name.clone(), headphones));
                if let Some((pname, _)) = prev {
                    if headphones && pname != name {
                        self.banner(BannerIcon::Headphones, name, t(lang, Key::HeadphonesConnected), now + BANNER_MS);
                    }
                }
                true
            }
        }
    }

    /// Advance time-based state. Returns notifications to surface outside the notch.
    pub fn tick(&mut self, now: i64) -> Vec<Notification> {
        let lang = self.lang();
        let mut out = Vec::new();
        if self.osd.as_ref().is_some_and(|o| o.until <= now) {
            self.osd = None;
        }
        if self.banner.as_ref().is_some_and(|b| b.until <= now) {
            self.banner = None;
        }
        let rang = self.data.alarms.take_due(now, self.offset_min);
        if let Some((id, label)) = rang.first() {
            self.mark_data_dirty(now);
            let title = if label.is_empty() { t(lang, Key::AlarmTitle).to_string() } else { label.clone() };
            self.banner(BannerIcon::Alarm, title.clone(), t(lang, Key::AlarmTitle), now + ALARM_BANNER_MS);
            if let Some(b) = &mut self.banner {
                b.alarm_id = Some(*id);
            }
            out.push(Notification { title: t(lang, Key::AlarmTitle).into(), body: title });
        }
        if self.countdown.poll(now) {
            let body = t(lang, Key::TimerDone);
            self.banner(BannerIcon::TimerDone, body, body, now + ALARM_BANNER_MS);
            out.push(Notification { title: t(lang, Key::ClockTimer).into(), body: body.into() });
        }
        let due = self.data.todos.take_due(now);
        if !due.is_empty() {
            self.mark_data_dirty(now);
            let first_id = due[0].0;
            let body = due.iter().map(|(_, text)| text.as_str()).collect::<Vec<_>>().join(" · ");
            self.banner_with_id(
                BannerIcon::Reminder,
                t(lang, Key::Reminder),
                body.clone(),
                now + REMINDER_BANNER_MS,
                Some(first_id),
            );
            out.push(Notification { title: t(lang, Key::Reminder).into(), body });
        }
        out
    }

    /// Activity the collapsed island shows: the highest-priority live one (Focus wins).
    pub fn primary_activity(&self, now: i64) -> Option<Tab> {
        self.running_activities(now).first().copied()
    }

    pub fn collapsed(&self, now: i64) -> Collapsed {
        if self.osd.as_ref().is_some_and(|o| o.until > now) {
            return Collapsed::Osd;
        }
        if self.banner.as_ref().is_some_and(|b| b.until > now) {
            return Collapsed::Banner;
        }
        if self.countdown.is_active() {
            return Collapsed::Countdown;
        }
        if self.stopwatch.is_active() {
            return Collapsed::Stopwatch;
        }
        match self.primary_activity(now) {
            Some(Tab::Music) => Collapsed::Media,
            Some(Tab::Todo) => Collapsed::Todo,
            None => Collapsed::Minimal,
        }
    }

    pub fn dismiss_banner(&mut self) {
        self.banner = None;
    }

    // ---- persistence bookkeeping -------------------------------------------------

    pub fn mark_settings_dirty(&mut self, now: i64) {
        self.settings_dirty = true;
        self.save_due = Some(now + SAVE_DEBOUNCE_MS);
    }

    pub fn mark_data_dirty(&mut self, now: i64) {
        self.data_dirty = true;
        self.save_due = Some(now + SAVE_DEBOUNCE_MS);
    }

    pub fn save_due(&self) -> Option<i64> {
        self.save_due
    }

    /// If the debounce elapsed (or `force`), returns which files to write: (settings, data).
    pub fn take_save(&mut self, now: i64, force: bool) -> (bool, bool) {
        match self.save_due {
            Some(due) if force || due <= now => {
                self.save_due = None;
                (std::mem::take(&mut self.settings_dirty), std::mem::take(&mut self.data_dirty))
            }
            _ => (false, false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::MINUTE;

    fn state() -> AppState {
        AppState::new(Settings::default(), Data::default(), 0)
    }

    fn media(key: &str, playing: bool) -> MediaInfo {
        MediaInfo {
            title: key.into(),
            artist: "a".into(),
            app: "app".into(),
            playing,
            position_ms: 0,
            duration_ms: 1000,
            sampled_at: 0,
            track_key: key.into(),
        }
    }

    #[test]
    fn first_volume_event_is_silent_then_osd() {
        let mut s = state();
        assert!(s.apply(AppEvent::Volume { level: 0.5, muted: false }, 0));
        assert!(s.osd.is_none());
        assert!(!s.apply(AppEvent::Volume { level: 0.5, muted: false }, 10));
        assert!(s.apply(AppEvent::Volume { level: 0.6, muted: false }, 20));
        assert_eq!(s.collapsed(20), Collapsed::Osd);
        s.tick(20 + OSD_MS);
        assert!(s.osd.is_none());
    }

    #[test]
    fn idle_rests_in_minimal() {
        let s = state();
        assert_eq!(s.collapsed(0), Collapsed::Minimal);
        assert_eq!(s.hover_activities(0), Vec::<Tab>::new(), "to-do is hidden when empty");
        assert!(!s.has_running_activity(0));
    }

    #[test]
    fn music_ranks_above_todo() {
        let mut s = state();
        // A task for today makes To-do live.
        s.data.todos.add_today("write".into(), None, 0, 0);
        assert_eq!(s.collapsed(0), Collapsed::Todo);

        s.apply(AppEvent::Media(Some(media("t", true))), 0);
        assert_eq!(s.collapsed(0), Collapsed::Media);
        assert_eq!(s.hover_activities(0), vec![Tab::Music, Tab::Todo]);
    }

    #[test]
    fn clock_tools_rank_above_music_and_ring_as_banners() {
        let mut s = state();
        s.apply(AppEvent::Media(Some(media("t", true))), 0);
        s.stopwatch.toggle(0);
        assert_eq!(s.collapsed(0), Collapsed::Stopwatch, "above music");
        s.countdown = Countdown::new(MINUTE);
        s.countdown.toggle(0);
        assert_eq!(s.collapsed(0), Collapsed::Countdown, "timer above stopwatch");

        let notes = s.tick(MINUTE);
        assert_eq!(s.banner.as_ref().map(|b| b.icon), Some(BannerIcon::TimerDone));
        assert_eq!(notes.len(), 1);
        assert_eq!(s.collapsed(MINUTE), Collapsed::Banner);

        // Alarm at 00:02 local (offset 0) rings and can be snoozed from the banner.
        let id = s.data.alarms.add(0, 2, "wake".into(), 0);
        s.banner = None;
        s.tick(2 * MINUTE);
        let b = s.banner.clone().unwrap();
        assert_eq!((b.icon, b.alarm_id), (BannerIcon::Alarm, Some(id)));
        s.snooze_banner(5, 2 * MINUTE);
        assert_eq!(s.data.alarms.next_due(2 * MINUTE, 0), Some(7 * MINUTE));
    }

    #[test]
    fn todo_only_live_while_today_has_unfinished_work() {
        let mut s = state();
        let id = s.data.todos.add_today("a".into(), None, 0, 0);
        assert!(s.is_activity_running(Tab::Todo, 0));
        s.data.todos.toggle(id, 0, 0);
        assert!(!s.is_activity_running(Tab::Todo, 0));
        // Due next week → not today.
        s.data.todos.add("later".into(), Some(8 * crate::time::DAY));
        assert!(!s.is_activity_running(Tab::Todo, 0));
        assert_eq!(s.collapsed(0), Collapsed::Minimal);
    }

    #[test]
    fn artwork_follows_track() {
        let mut s = state();
        s.apply(AppEvent::Media(Some(media("a", true))), 0);
        let art = |k: &str| Arc::new(Artwork { track_key: k.into(), width: 1, height: 1, pixels: vec![0; 4] });
        assert!(!s.apply(AppEvent::Artwork(Some(art("stale"))), 0));
        assert!(s.apply(AppEvent::Artwork(Some(art("a"))), 0));
        assert!(s.art.is_some());
        s.apply(AppEvent::Media(Some(media("a", false))), 0);
        assert!(s.art.is_some());
        s.apply(AppEvent::Media(Some(media("b", true))), 0);
        assert!(s.art.is_none());
    }

    #[test]
    fn low_battery_and_headphones_banners() {
        let mut s = state();
        s.apply(AppEvent::Battery(Some(Battery { percent: 21, charging: false })), 0);
        assert!(s.banner.is_none());
        s.apply(AppEvent::Battery(Some(Battery { percent: 20, charging: false })), 0);
        assert_eq!(s.banner.as_ref().unwrap().icon, BannerIcon::BatteryLow);

        s.dismiss_banner();
        s.apply(AppEvent::AudioDevice { name: "Speakers".into(), headphones: false }, 0);
        assert!(s.banner.is_none());
        s.apply(AppEvent::AudioDevice { name: "Buds".into(), headphones: true }, 0);
        assert_eq!(s.banner.as_ref().unwrap().icon, BannerIcon::Headphones);
    }

    #[test]
    fn todo_reminder_fires_once() {
        let mut s = state();
        s.data.todos.add("call".into(), Some(5_000));
        assert!(s.tick(4_999).is_empty());
        let n = s.tick(5_000);
        assert_eq!(n[0].body, "call");
        assert!(s.tick(6_000).is_empty());
        assert_eq!(s.banner.as_ref().unwrap().icon, BannerIcon::Reminder);
    }
}
