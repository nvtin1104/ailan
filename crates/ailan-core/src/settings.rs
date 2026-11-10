//! User settings (settings.toml) and persisted data (data.json).

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;
use crate::timer::Durations;
use crate::todo::Todos;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Tab {
    #[default]
    Music,
    Focus,
    Todo,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Focus, Tab::Music, Tab::Todo];

    pub fn next(self) -> Self {
        match self {
            Tab::Focus => Tab::Music,
            Tab::Music => Tab::Todo,
            Tab::Todo => Tab::Focus,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Tab::Focus => Tab::Todo,
            Tab::Music => Tab::Focus,
            Tab::Todo => Tab::Music,
        }
    }
}

/// Operating mode of the notch when collapsed / idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotchMode {
    /// Dynamic notch only activates when at least 1 activity is running; otherwise shows Minimal UI.
    #[default]
    DynamicWhenActive,
    /// Always show dynamic notch (Clock when idle, activity when running).
    AlwaysDynamic,
    /// Always stay in Minimal UI when collapsed, even if activities are running.
    MinimalOnly,
}

/// Style of the 5-bar sound wave animation when music is playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WaveStyle {
    /// Reacts in real time to system output audio peaks (IAudioMeterInformation).
    #[default]
    AudioReactive,
    /// Smooth flowing wave modulated by audio level.
    SmoothWave,
    /// Symmetric center-out equalizer bounce driven by audio beats.
    BouncePulse,
}

/// Monochrome shade preset for the sound wave bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WaveColor {
    #[default]
    White,
    #[serde(alias = "mint")]
    Silver,
    #[serde(alias = "cyan")]
    SoftGray,
    #[serde(alias = "violet")]
    DimGray,
}

/// Customizable audio-reactive animation settings for Music playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicAnimConfig {
    pub style: WaveStyle,
    pub color: WaveColor,
    /// Audio reactivity sensitivity level: 1 (Low), 2 (Normal), 3 (High), 4 (Boost).
    pub sensitivity: u8,
    /// Subtle bass pulse on the album cover art when music plays.
    pub art_pulse: bool,
}

impl Default for MusicAnimConfig {
    fn default() -> Self {
        Self {
            style: WaveStyle::AudioReactive,
            color: WaveColor::White,
            sensitivity: 2,
            art_pulse: true,
        }
    }
}

impl MusicAnimConfig {
    pub fn gain(&self) -> f32 {
        match self.sensitivity {
            1 => 0.80,
            2 => 1.35,
            3 => 1.95,
            _ => 2.60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PriorityConfig {
    /// Priority order of activities (highest priority first).
    pub order: Vec<Tab>,
    /// When Focus (which updates every minute) runs alongside another activity (e.g. Music),
    /// only peek Focus for `peek_seconds` on each minute change, then show the next activity.
    pub smart_minute_peek: bool,
    pub peek_seconds: u32,
}

impl Default for PriorityConfig {
    fn default() -> Self {
        Self {
            order: vec![Tab::Focus, Tab::Music, Tab::Todo],
            smart_minute_peek: true,
            peek_seconds: 5,
        }
    }
}

impl PriorityConfig {
    pub fn normalized_order(&self) -> [Tab; 3] {
        let mut out = [Tab::Focus, Tab::Music, Tab::Todo];
        let mut idx = 0;
        for &t in &self.order {
            if !out[..idx].contains(&t) && idx < 3 {
                out[idx] = t;
                idx += 1;
            }
        }
        for t in Tab::ALL {
            if !out[..idx].contains(&t) && idx < 3 {
                out[idx] = t;
                idx += 1;
            }
        }
        out
    }

    pub fn move_up(&mut self, tab: Tab) {
        let mut norm = self.normalized_order().to_vec();
        if let Some(pos) = norm.iter().position(|&t| t == tab) {
            if pos > 0 {
                norm.swap(pos, pos - 1);
            }
        }
        self.order = norm;
    }

    pub fn move_down(&mut self, tab: Tab) {
        let mut norm = self.normalized_order().to_vec();
        if let Some(pos) = norm.iter().position(|&t| t == tab) {
            if pos + 1 < norm.len() {
                norm.swap(pos, pos + 1);
            }
        }
        self.order = norm;
    }
}

/// Customizable elements for the Minimal (idle) UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MinimalUiConfig {
    pub hide_when_idle: bool,
    pub show_dot: bool,
    pub show_clock: bool,
    pub show_date: bool,
    pub show_battery: bool,
}

impl Default for MinimalUiConfig {
    fn default() -> Self {
        Self {
            hide_when_idle: false,
            show_dot: true,
            show_clock: false,
            show_date: false,
            show_battery: false,
        }
    }
}

/// Customizable elements for the Collapsed UI when an activity is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CollapsedUiConfig {
    pub focus_show_icon: bool,
    pub focus_show_label: bool,
    pub focus_show_time: bool,
    pub media_show_art: bool,
    pub media_show_title: bool,
    pub media_show_eq: bool,
    pub todo_show_icon: bool,
    pub todo_show_text: bool,
    pub todo_show_count: bool,
}

impl Default for CollapsedUiConfig {
    fn default() -> Self {
        Self {
            focus_show_icon: true,
            focus_show_label: true,
            focus_show_time: true,
            media_show_art: true,
            media_show_title: true,
            media_show_eq: true,
            todo_show_icon: true,
            todo_show_text: true,
            todo_show_count: true,
        }
    }
}

/// Customizable elements for the Hover (Expanded) UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HoverUiConfig {
    pub show_clock: bool,
    pub show_date: bool,
    pub show_wifi: bool,
    pub show_battery: bool,
    pub show_headphones: bool,
    pub show_slide_controls: bool,
}

impl Default for HoverUiConfig {
    fn default() -> Self {
        Self {
            show_clock: true,
            show_date: true,
            show_wifi: true,
            show_battery: true,
            show_headphones: true,
            show_slide_controls: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// `None` until first run, then filled from the system locale.
    pub lang: Option<Lang>,
    /// Legacy minimal flag (synced with `mode`).
    pub minimal: bool,
    pub mode: NotchMode,
    pub durations: Durations,
    /// Primary/selected activity shown when the notch expands.
    pub tab: Tab,
    /// Disable spring animations (accessibility / very slow machines).
    pub reduce_motion: bool,
    /// Activity priority & smart switching rules.
    pub priority: PriorityConfig,
    /// What to show in Minimal mode.
    pub minimal_ui: MinimalUiConfig,
    /// What to show when collapsed with an active activity.
    pub collapsed_ui: CollapsedUiConfig,
    /// What to show when hovering (Expanded).
    pub hover_ui: HoverUiConfig,
    /// Audio-reactive music animation settings.
    pub music_anim: MusicAnimConfig,
    /// Whether Focus activity is pinned to always appear on Hover UI.
    pub pin_focus: bool,
    /// Whether Music activity is pinned to always appear on Hover UI even when idle.
    #[serde(alias = "media_active")]
    pub pin_music: bool,
    /// Whether To-do activity is pinned to appear on Hover UI & dynamic notch.
    #[serde(alias = "todo_active")]
    pub pin_todo: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lang: None,
            minimal: false,
            mode: NotchMode::DynamicWhenActive,
            durations: Durations::default(),
            tab: Tab::Focus,
            reduce_motion: false,
            priority: PriorityConfig::default(),
            minimal_ui: MinimalUiConfig::default(),
            collapsed_ui: CollapsedUiConfig::default(),
            hover_ui: HoverUiConfig::default(),
            music_anim: MusicAnimConfig::default(),
            pin_focus: false,
            pin_music: false,
            pin_todo: false,
        }
    }
}

impl Settings {
    pub fn lang(&self) -> Lang {
        self.lang.unwrap_or_default()
    }

    pub fn is_pinned(&self, tab: Tab) -> bool {
        match tab {
            Tab::Focus => self.pin_focus,
            Tab::Music => self.pin_music,
            Tab::Todo => self.pin_todo,
        }
    }

    pub fn set_pinned(&mut self, tab: Tab, pinned: bool) {
        match tab {
            Tab::Focus => self.pin_focus = pinned,
            Tab::Music => self.pin_music = pinned,
            Tab::Todo => self.pin_todo = pinned,
        }
    }

    pub fn toggle_pinned(&mut self, tab: Tab) {
        let p = !self.is_pinned(tab);
        self.set_pinned(tab, p);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Data {
    pub todos: Todos,
    /// Local day number `focus_day` refers to.
    pub focus_day: i64,
    pub focus_today: u32,
}

impl Data {
    pub fn focus_count(&self, today: i64) -> u32 {
        if self.focus_day == today { self.focus_today } else { 0 }
    }

    pub fn add_focus(&mut self, today: i64) {
        if self.focus_day != today {
            self.focus_day = today;
            self.focus_today = 0;
        }
        self.focus_today += 1;
    }
}
