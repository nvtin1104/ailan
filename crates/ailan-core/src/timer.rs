//! Pomodoro focus timer state machine.

use serde::{Deserialize, Serialize};

use crate::time::MINUTE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Focus,
    ShortBreak,
    LongBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Durations {
    pub focus_min: u32,
    pub short_min: u32,
    pub long_min: u32,
    /// Long break after this many focus sessions.
    pub long_every: u32,
}

impl Default for Durations {
    fn default() -> Self {
        Self { focus_min: 25, short_min: 5, long_min: 15, long_every: 4 }
    }
}

impl Durations {
    pub fn of(&self, phase: Phase) -> i64 {
        let m = match phase {
            Phase::Focus => self.focus_min,
            Phase::ShortBreak => self.short_min,
            Phase::LongBreak => self.long_min,
        };
        m.max(1) as i64 * MINUTE
    }
}

#[derive(Debug, Clone)]
pub struct Pomodoro {
    pub phase: Phase,
    pub durations: Durations,
    /// Focus sessions completed in the current cycle.
    pub completed: u32,
    /// Set while running.
    ends_at: Option<i64>,
    /// Remaining time while paused/stopped.
    remaining: i64,
}

impl Pomodoro {
    pub fn new(durations: Durations) -> Self {
        Self { phase: Phase::Focus, durations, completed: 0, ends_at: None, remaining: durations.of(Phase::Focus) }
    }

    pub fn is_running(&self) -> bool {
        self.ends_at.is_some()
    }

    /// True when the timer has been started at least once in this phase (running or paused mid-way).
    pub fn is_active(&self) -> bool {
        self.is_running() || self.remaining != self.durations.of(self.phase)
    }

    pub fn ends_at(&self) -> Option<i64> {
        self.ends_at
    }

    pub fn remaining(&self, now: i64) -> i64 {
        match self.ends_at {
            Some(end) => (end - now).max(0),
            None => self.remaining,
        }
    }

    /// 0..=1 progress through the current phase.
    pub fn progress(&self, now: i64) -> f32 {
        let total = self.durations.of(self.phase) as f32;
        (1.0 - self.remaining(now) as f32 / total).clamp(0.0, 1.0)
    }

    pub fn start(&mut self, now: i64) {
        if self.ends_at.is_none() {
            self.ends_at = Some(now + self.remaining);
        }
    }

    pub fn pause(&mut self, now: i64) {
        if let Some(end) = self.ends_at.take() {
            self.remaining = (end - now).max(0);
        }
    }

    pub fn toggle(&mut self, now: i64) {
        if self.is_running() { self.pause(now) } else { self.start(now) }
    }

    pub fn reset(&mut self) {
        self.ends_at = None;
        self.remaining = self.durations.of(self.phase);
    }

    /// Jump to the next phase (counts a skipped focus session as not completed).
    pub fn skip(&mut self) {
        let next = self.next_phase(false);
        self.enter(next);
    }

    pub fn set_durations(&mut self, d: Durations) {
        self.durations = d;
        if !self.is_active() {
            self.remaining = d.of(self.phase);
        }
    }

    /// Call when time passes. Returns the phase that just finished, if any.
    /// The timer stops after a phase completes; the user starts the next one.
    pub fn poll(&mut self, now: i64) -> Option<Phase> {
        let end = self.ends_at?;
        if now < end {
            return None;
        }
        let finished = self.phase;
        let next = self.next_phase(true);
        self.enter(next);
        Some(finished)
    }

    fn next_phase(&mut self, count: bool) -> Phase {
        match self.phase {
            Phase::Focus => {
                if count {
                    self.completed += 1;
                }
                if count && self.completed % self.durations.long_every.max(1) == 0 {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            }
            Phase::ShortBreak | Phase::LongBreak => Phase::Focus,
        }
    }

    fn enter(&mut self, phase: Phase) {
        self.phase = phase;
        self.ends_at = None;
        self.remaining = self.durations.of(phase);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quick() -> Durations {
        Durations { focus_min: 25, short_min: 5, long_min: 15, long_every: 2 }
    }

    #[test]
    fn run_pause_resume() {
        let mut p = Pomodoro::new(quick());
        p.start(0);
        assert!(p.is_running());
        assert_eq!(p.remaining(60_000), 24 * MINUTE);
        p.pause(60_000);
        assert_eq!(p.remaining(999_999), 24 * MINUTE);
        p.start(100_000);
        assert_eq!(p.ends_at(), Some(100_000 + 24 * MINUTE));
    }

    #[test]
    fn completes_into_breaks_and_long_break() {
        let mut p = Pomodoro::new(quick());
        p.start(0);
        assert_eq!(p.poll(25 * MINUTE - 1), None);
        assert_eq!(p.poll(25 * MINUTE), Some(Phase::Focus));
        assert_eq!(p.phase, Phase::ShortBreak);
        assert!(!p.is_running());

        p.start(0);
        assert_eq!(p.poll(5 * MINUTE), Some(Phase::ShortBreak));
        assert_eq!(p.phase, Phase::Focus);

        p.start(0);
        assert_eq!(p.poll(25 * MINUTE), Some(Phase::Focus));
        assert_eq!(p.phase, Phase::LongBreak, "second focus → long break");
        assert_eq!(p.completed, 2);
    }

    #[test]
    fn skip_and_reset() {
        let mut p = Pomodoro::new(quick());
        p.start(0);
        p.skip();
        assert_eq!(p.phase, Phase::ShortBreak);
        assert_eq!(p.completed, 0);
        assert!(!p.is_running());
        p.start(0);
        p.pause(MINUTE);
        assert!(p.is_active());
        p.reset();
        assert!(!p.is_active());
        assert_eq!(p.remaining(0), 5 * MINUTE);
    }
}
