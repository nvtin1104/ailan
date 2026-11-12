//! The AiLan Personal Hub application window.
//!
//! Two layouts share one controller:
//! - **Compact Mode** (380×600): floating widget, content above a floating glass dock.
//! - **Full Mode** (1000×680): sidebar navigation with an Island status card + mini focus/media dock.

pub mod controller;
pub mod pages;
pub mod widgets;

use ailan_core::i18n::Lang;
use ailan_core::settings::HubPage;
use ailan_core::AppState;

use crate::gfx::{Canvas, Rect};
use widgets::{draw_specular_card, CheckPop};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoFilter {
    All,
    Today,
    Upcoming,
    Completed,
}

impl TodoFilter {
    pub const ALL: [TodoFilter; 4] = [TodoFilter::All, TodoFilter::Today, TodoFilter::Upcoming, TodoFilter::Completed];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|f| *f == self).unwrap_or(0)
    }
}

/// Editable Pomodoro settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PomoField {
    Focus,
    ShortBreak,
    LongBreak,
    /// Focus sessions before a long break.
    Every,
}

impl PomoField {
    /// `(min, max, step)` for the stepper.
    pub fn range(self) -> (u32, u32, i16) {
        match self {
            PomoField::Focus => (5, 180, 5),
            PomoField::ShortBreak => (1, 30, 1),
            PomoField::LongBreak => (5, 60, 5),
            PomoField::Every => (2, 8, 1),
        }
    }

    /// Apply `delta` to `d`, clamped to the field's range. Returns `true` if it changed.
    pub fn apply(self, d: &mut ailan_core::timer::Durations, delta: i16) -> bool {
        let (lo, hi, _) = self.range();
        let v = match self {
            PomoField::Focus => &mut d.focus_min,
            PomoField::ShortBreak => &mut d.short_min,
            PomoField::LongBreak => &mut d.long_min,
            PomoField::Every => &mut d.long_every,
        };
        let new = (*v as i64 + delta as i64).clamp(lo as i64, hi as i64) as u32;
        let changed = new != *v;
        *v = new;
        changed
    }
}

/// Text fields that can own keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// Quick-capture bar (Today / Todos / Reminders).
    Capture,
    /// `Ctrl+K` command bar in the titlebar.
    Command,
    /// Live filter on the Todos page.
    Search,
    /// Natural-language command box on the AI page.
    Copilot,
    /// "New alarm" input on the Reminders → Alarms tab.
    Alarm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubAction {
    // Window controls
    Close,
    Minimize,
    ToggleMode,
    SelectPage(HubPage),

    // Text fields
    FocusField(Field),
    SubmitField(Field),
    ClearSearch,

    // Legacy capture aliases (older pages)
    StartAddTodo,
    FocusTodoInput,
    SubmitAddTodo,
    SubmitTodoInput,
    CancelAddTodo,

    // Todo & Reminder actions
    ToggleTodo(u64),
    TodoToggle(u64),
    DeleteTodo(u64),
    TodoDelete(u64),
    SnoozeTodo(u64, u32),
    ShiftTodoDate(u64, i32),
    SetTodoToday(u64),
    SetTodoFilter(TodoFilter),
    FocusOnTodo(u64),
    ToggleBucket(u8),

    // Pomodoro actions
    PomoToggle,
    FocusToggle,
    PomoReset,
    FocusReset,
    PomoSkip,
    SetPomoWork(u16),
    SetPomoBreak(u16),
    /// Step one Pomodoro setting by a delta (minutes, or sessions for `Every`).
    AdjustPomo(PomoField, i16),

    // Media actions
    MediaPlayPause,
    MediaToggle,
    MediaNext,
    MediaPrev,

    // Clock tools (Reminders page)
    SetClockTab(u8),
    AlarmToggle(u64),
    /// Shift an alarm by ±minutes.
    AlarmShift(u64, i16),
    AlarmDay(u64, u8),
    AlarmDelete(u64),
    CountdownToggle,
    CountdownReset,
    /// Set an idle countdown to this many seconds.
    CountdownSet(u32),
    /// Add (or subtract) seconds.
    CountdownAdd(i32),
    StopwatchToggle,
    StopwatchLap,
    StopwatchReset,
    WorldAdd(u8),
    WorldRemove(u8),

    // AI page
    /// Fill the AI command box with example `n`.
    UseExample(u8),

    // Settings actions
    SetLanguage(Lang),
    ToggleAutostart,
    ToggleReduceMotion,
}

impl HubAction {
    /// Actions that put a text caret in a field (I-beam cursor, keep focus on click).
    pub fn is_text_target(self) -> bool {
        matches!(self, HubAction::FocusField(_) | HubAction::StartAddTodo | HubAction::FocusTodoInput)
    }
}

#[derive(Debug, Clone)]
pub struct HubHit {
    pub rect: Rect,
    pub action: HubAction,
}

/// Everything a page needs to draw one frame, plus its outputs (hits, spotlight surfaces,
/// content height for scrolling).
pub struct PageCtx<'a> {
    pub st: &'a AppState,
    pub now: i64,
    pub hover: Option<HubAction>,
    pub mouse: Option<(f32, f32)>,
    pub focus: Option<Field>,
    pub capture: &'a str,
    pub search: &'a str,
    pub copilot: &'a str,
    pub alarm_input: &'a str,
    /// Selected Reminders tab (0 reminders, 1 alarms, 2 timer, 3 stopwatch, 4 world) and
    /// its spring-animated indicator position.
    pub clock_tab: usize,
    pub clock_tab_pos: f32,
    pub filter: TodoFilter,
    /// Spring-animated fractional index of the active filter segment.
    pub filter_pos: f32,
    pub scroll: f32,
    pub collapsed: [bool; 5],
    pub compact: bool,
    pub pops: &'a [CheckPop],
    pub hits: &'a mut Vec<HubHit>,
    /// Surfaces that react to the pointer (spotlight) — pointer moves over them redraw.
    pub spots: &'a mut Vec<Rect>,
    /// Set by scrollable pages: full content height (unscrolled).
    pub content_h: Option<f32>,
}

impl PageCtx<'_> {
    pub fn hit(&mut self, rect: Rect, action: HubAction) {
        self.hits.push(HubHit { rect, action });
    }

    pub fn hover_t(&self, action: HubAction) -> f32 {
        if self.hover == Some(action) {
            1.0
        } else {
            0.0
        }
    }

    pub fn lang(&self) -> Lang {
        self.st.lang()
    }

    /// Specular card that tracks the pointer spotlight.
    pub fn card(&mut self, cv: &mut Canvas, r: Rect, radius: f32, hover: f32) {
        self.spots.push(r);
        draw_specular_card(cv, r, radius, hover, self.mouse);
    }

    /// `(scale, tick progress)` of a todo's checkbox animation.
    pub fn check_anim(&self, id: u64) -> (f32, f32) {
        self.pops.iter().find(|p| p.id == id).map(|p| (p.scale(), p.tick())).unwrap_or((1.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::PomoField;
    use ailan_core::timer::Durations;

    #[test]
    fn pomodoro_steps_are_clamped() {
        let mut d = Durations::default();
        assert!(PomoField::Focus.apply(&mut d, 5));
        assert_eq!(d.focus_min, 30);
        assert!(PomoField::ShortBreak.apply(&mut d, -10));
        assert_eq!(d.short_min, 1, "never below one minute");
        assert!(!PomoField::ShortBreak.apply(&mut d, -1), "already at the minimum");
        PomoField::Every.apply(&mut d, 100);
        assert_eq!(d.long_every, 8);
        PomoField::LongBreak.apply(&mut d, -5);
        assert_eq!(d.long_min, 10);
    }
}
