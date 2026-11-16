//! User settings (settings.toml) and persisted data (data.json).

use serde::{Deserialize, Deserializer, Serialize};

use crate::clock::{Alarms, WorldClock};
use crate::i18n::Lang;
use crate::todo::Todos;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Tab {
    #[default]
    Music,
    Todo,
}

impl Tab {
    pub const ALL: [Tab; 2] = [Tab::Music, Tab::Todo];

    pub fn next(self) -> Self {
        match self {
            Tab::Music => Tab::Todo,
            Tab::Todo => Tab::Music,
        }
    }

    pub fn prev(self) -> Self {
        self.next()
    }
}

fn de_tab<'de, D: Deserializer<'de>>(d: D) -> Result<Tab, D::Error> {
    let s = String::deserialize(d)?;
    Ok(match s.as_str() {
        "todo" => Tab::Todo,
        _ => Tab::Music,
    })
}

/// User settings. The island has one fixed behaviour (no modes, priority or pinning).
/// Keys from older settings files are ignored on load. New fields are optional so a
/// file written before drag-position and hub size still loads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// `None` until first run, then filled from the system locale.
    pub lang: Option<Lang>,
    /// Primary/selected activity shown when the island expands.
    #[serde(deserialize_with = "de_tab")]
    pub tab: Tab,
    /// Disable spring animations (accessibility / very slow machines).
    pub reduce_motion: bool,
    /// Monitor device name (`szDevice`) the island was dropped on. `None` = primary.
    pub monitor: Option<String>,
    /// Pill centre X as a fraction of that monitor's work area. `None` = centred.
    pub anchor_x: Option<f32>,
    /// Pill top Y as a fraction of that monitor's work area. `None` = top edge.
    pub anchor_y: Option<f32>,
    /// Hub client size in DIP. `None` = 1000×680.
    pub hub_w: Option<f32>,
    pub hub_h: Option<f32>,
    /// Whether the Hub window was last maximized.
    pub hub_maximized: bool,
    /// Hub page restored on the next open.
    pub hub_page: HubPage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HubPage {
    #[default]
    Today,
    Todos,
    Reminders,
    Settings,
}

impl<'de> serde::Deserialize<'de> for HubPage {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer).unwrap_or_default();
        Ok(match s.as_str() {
            "todos" => HubPage::Todos,
            "reminders" => HubPage::Reminders,
            "settings" => HubPage::Settings,
            _ => HubPage::Today,
        })
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lang: None,
            tab: Tab::Music,
            reduce_motion: false,
            monitor: None,
            anchor_x: None,
            anchor_y: None,
            hub_w: None,
            hub_h: None,
            hub_maximized: false,
            hub_page: HubPage::Today,
        }
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
