//! The AiLan Personal Hub application window.
//!
//! Two layouts share one controller:
//! - **Compact Mode** (380×600): floating widget, content above a floating glass dock.
//! - **Full Mode** (1000×680): sidebar navigation with Island sync + mini focus/media dock.

pub mod controller;
pub mod pages;
pub mod widgets;

use ailan_core::i18n::Lang;
use ailan_core::settings::{HubPage, NotchMode, Tab};
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

    // Media actions
    MediaPlayPause,
    MediaToggle,
    MediaNext,
    MediaPrev,

    // Island pin actions
    PinFocus,
    PinMedia,
    PinTodo,
    ToggleMinimalMode,
    SetNotchMode(NotchMode),
    PriorityUp(Tab),
    PriorityDown(Tab),
    ToggleSmartPeek,
    /// Hover target of the Island live preview (shows the expanded state).
    PreviewIsland,
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
    /// Spring-animated rank (0..3) of each activity in the Island priority list,
    /// indexed Focus, Music, Todo.
    pub prio_pos: [f32; 3],
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
