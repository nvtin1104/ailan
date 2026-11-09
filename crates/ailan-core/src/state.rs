//! Application state and the event reducer.

use std::sync::Arc;

use crate::event::{AppEvent, Artwork, Battery, MediaInfo, Network};
use crate::i18n::{t, Key, Lang};
use crate::settings::{Data, Settings};
use crate::time;
use crate::timer::{Phase, Pomodoro};

pub const OSD_MS: i64 = 1_500;
pub const BANNER_MS: i64 = 4_000;
pub const REMINDER_MS: i64 = 10_000;
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Banner {
    pub icon: BannerIcon,
    pub title: String,
    pub body: String,
    pub until: i64,
}

/// What the collapsed notch is showing, in priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Collapsed {
    Osd,
    Banner,
    Timer,
    Media,
    Clock,
    Minimal,
}

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

    pub pomodoro: Pomodoro,
    pub osd: Option<Osd>,
    pub banner: Option<Banner>,

    save_due: Option<i64>,
    settings_dirty: bool,
    data_dirty: bool,
}

impl AppState {
    pub fn new(settings: Settings, data: Data, offset_min: i32) -> Self {
        let pomodoro = Pomodoro::new(settings.durations);
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
            pomodoro,
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
        self.banner = Some(Banner { icon, title: title.into(), body: body.into(), until });
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
        if let Some(finished) = self.pomodoro.poll(now) {
            let (icon, body) = match finished {
                Phase::Focus => {
                    self.data.add_focus(time::local_day(now, self.offset_min));
                    self.mark_data_dirty(now);
                    (BannerIcon::Focus, t(lang, Key::FocusDone))
                }
                _ => (BannerIcon::Break, t(lang, Key::BreakDone)),
            };
            self.banner(icon, "Ailan", body, now + BANNER_MS);
            out.push(Notification { title: "Ailan".into(), body: body.into() });
        }
        let due = self.data.todos.take_due(now);
        if !due.is_empty() {
            self.mark_data_dirty(now);
            let body = due.join(" · ");
            self.banner(BannerIcon::Reminder, t(lang, Key::Reminder), body.clone(), now + REMINDER_MS);
            out.push(Notification { title: t(lang, Key::Reminder).into(), body });
        }
        out
    }

    pub fn collapsed(&self, now: i64) -> Collapsed {
        if self.osd.as_ref().is_some_and(|o| o.until > now) {
            Collapsed::Osd
        } else if self.banner.as_ref().is_some_and(|b| b.until > now) {
            Collapsed::Banner
        } else if self.pomodoro.is_running() {
            Collapsed::Timer
        } else if self.media.as_ref().is_some_and(|m| m.playing) {
            Collapsed::Media
        } else if self.settings.minimal {
            Collapsed::Minimal
        } else {
            Collapsed::Clock
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
    fn collapsed_priority() {
        let mut s = state();
        assert_eq!(s.collapsed(0), Collapsed::Clock);
        s.settings.minimal = true;
        assert_eq!(s.collapsed(0), Collapsed::Minimal);
        s.apply(AppEvent::Media(Some(media("t", true))), 0);
        assert_eq!(s.collapsed(0), Collapsed::Media);
        s.pomodoro.start(0);
        assert_eq!(s.collapsed(0), Collapsed::Timer);
        s.apply(AppEvent::Battery(Some(Battery { percent: 50, charging: false })), 0);
        s.apply(AppEvent::Battery(Some(Battery { percent: 50, charging: true })), 0);
        assert_eq!(s.collapsed(0), Collapsed::Banner);
    }

    #[test]
    fn artwork_follows_track() {
        let mut s = state();
        s.apply(AppEvent::Media(Some(media("a", true))), 0);
        let art = |k: &str| Arc::new(Artwork { track_key: k.into(), width: 1, height: 1, pixels: vec![0; 4] });
        assert!(!s.apply(AppEvent::Artwork(Some(art("stale"))), 0));
        assert!(s.apply(AppEvent::Artwork(Some(art("a"))), 0));
        assert!(s.art.is_some());
        // Same track, new playback state → keep art.
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
    fn pomodoro_completion_counts_and_notifies() {
        let mut s = state();
        s.pomodoro.start(0);
        assert!(s.tick(MINUTE).is_empty());
        let n = s.tick(25 * MINUTE);
        assert_eq!(n.len(), 1);
        assert_eq!(s.data.focus_count(0), 1);
        assert_eq!(s.take_save(25 * MINUTE, false), (false, false), "debounced");
        assert_eq!(s.take_save(25 * MINUTE + SAVE_DEBOUNCE_MS, false), (false, true));
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
