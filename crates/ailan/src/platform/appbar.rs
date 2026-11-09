//! Fullscreen detection without polling: register a zero-size app bar so the
//! shell sends ABN_FULLSCREENAPP when a fullscreen app opens or closes.

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, SHQueryUserNotificationState, ABM_NEW, ABM_REMOVE, APPBARDATA, QUNS_BUSY, QUNS_PRESENTATION_MODE,
    QUNS_RUNNING_D3D_FULL_SCREEN,
};

use super::WM_APP_APPBAR;

/// `ABN_FULLSCREENAPP` notification code (wParam of the callback message).
pub const ABN_FULLSCREENAPP: usize = 2;

pub struct AppBar {
    hwnd: HWND,
}

impl AppBar {
    pub fn register(hwnd: HWND) -> Self {
        let mut abd = data(hwnd);
        abd.uCallbackMessage = WM_APP_APPBAR;
        unsafe {
            SHAppBarMessage(ABM_NEW, &mut abd);
        }
        Self { hwnd }
    }
}

impl Drop for AppBar {
    fn drop(&mut self) {
        let mut abd = data(self.hwnd);
        unsafe {
            SHAppBarMessage(ABM_REMOVE, &mut abd);
        }
    }
}

fn data(hwnd: HWND) -> APPBARDATA {
    APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, hWnd: hwnd, lParam: LPARAM(0), ..Default::default() }
}

/// Second opinion used at start-up and when the shell reports a change.
pub fn fullscreen_now() -> bool {
    matches!(
        unsafe { SHQueryUserNotificationState() },
        Ok(s) if s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE
    )
}
