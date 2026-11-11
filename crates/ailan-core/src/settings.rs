//! User settings (settings.toml) and persisted data (data.json).

use serde::{Deserialize, Serialize};

use crate::clock::{Alarms, WorldClock};
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

/// User settings. The Dynamic Island has a single fixed behaviour (no modes, priority or
/// pinning settings); keys from older settings files are ignored on load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// `None` until first run, then filled from the system locale.
    pub lang: Option<Lang>,
    pub durations: Durations,
    /// Primary/selected activity shown when the notch expands.
    pub tab: Tab,
    /// Disable spring animations (accessibility / very slow machines).
    pub reduce_motion: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HubPage {
    #[default]
    Today,
    Todos,
    Reminders,
    Settings,
    Ai,
}

impl Default for Settings {
    fn default() -> Self {
        Self { lang: None, durations: Durations::default(), tab: Tab::Focus, reduce_motion: false }
    }
}

impl Settings {
    pub fn lang(&self) -> Lang {
        self.lang.unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Data {
    pub todos: Todos,
    pub alarms: Alarms,
    pub world_clocks: Vec<WorldClock>,
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
