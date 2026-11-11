//! Windows integrations. Every provider is event-driven: it subscribes to an OS
//! notification and forwards `AppEvent`s through an [`EventSink`].

pub mod appbar;
pub mod appwindow;
pub mod audio;
pub mod autostart;
pub mod brightness;
pub mod media;
pub mod network;
pub mod power;
pub mod sys;
pub mod tray;

use std::sync::mpsc::Sender;

use ailan_core::AppEvent;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Provider events are queued; this message wakes the UI thread to drain them.
pub const WM_APP_EVENT: u32 = WM_APP + 1;
/// Tray icon callback.
pub const WM_APP_TRAY: u32 = WM_APP + 2;
/// Default audio device changed — rebind volume callback on the UI thread.
pub const WM_APP_AUDIO_DEVICE: u32 = WM_APP + 3;
/// Shell app-bar notifications (fullscreen app opened/closed).
pub const WM_APP_APPBAR: u32 = WM_APP + 4;
/// Show / bring to front AiLan Personal Hub window (e.g. from single-instance launch).
pub const WM_APP_SHOW_HUB: u32 = WM_APP + 5;

/// Thread-safe handle providers use to deliver events to the UI thread.
#[derive(Clone)]
pub struct EventSink {
    tx: Sender<AppEvent>,
    hwnd: isize,
}

impl EventSink {
    pub fn new(tx: Sender<AppEvent>, hwnd: HWND) -> Self {
        Self { tx, hwnd: hwnd.0 as isize }
    }

    pub fn send(&self, ev: AppEvent) {
        if self.tx.send(ev).is_ok() {
            self.post(WM_APP_EVENT);
        }
    }

    pub fn post(&self, msg: u32) {
        unsafe {
            let _ = PostMessageW(Some(HWND(self.hwnd as _)), msg, WPARAM(0), LPARAM(0));
        }
    }
}

/// Null-terminated UTF-16.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copy `s` into a fixed UTF-16 buffer (truncating, always terminated).
pub fn copy_wide(dst: &mut [u16], s: &str) {
    let mut i = 0;
    for u in s.encode_utf16() {
        if i + 1 >= dst.len() {
            break;
        }
        dst[i] = u;
        i += 1;
    }
    if i < dst.len() {
        dst[i] = 0;
    }
}
