//! Platform-agnostic core of Ailan: events, state reducer, timers, to-dos,
//! scheduling, animation, settings and persistence. Everything here is pure
//! and unit-tested; the Windows binary only feeds events in and draws state out.
//!
//! Time is always passed in explicitly as epoch milliseconds (`now`) plus a
//! local UTC offset in minutes, so logic is deterministic under test.

pub mod anim;
pub mod event;
pub mod i18n;
pub mod scheduler;
pub mod settings;
pub mod state;
pub mod storage;
pub mod time;
pub mod timer;
pub mod todo;

pub use event::AppEvent;
pub use state::AppState;
