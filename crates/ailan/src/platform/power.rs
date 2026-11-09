//! Battery / AC state via power-setting notifications (no polling).

use ailan_core::event::Battery;
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::Power::{
    GetSystemPowerStatus, RegisterPowerSettingNotification, UnregisterPowerSettingNotification, HPOWERNOTIFY,
    SYSTEM_POWER_STATUS,
};
use windows::Win32::System::SystemServices::{GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING};
use windows::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_WINDOW_HANDLE;

pub struct PowerNotify {
    handles: Vec<HPOWERNOTIFY>,
}

impl PowerNotify {
    /// Subscribe `hwnd` to WM_POWERBROADCAST / PBT_POWERSETTINGCHANGE.
    pub fn register(hwnd: HWND) -> Self {
        let mut handles = Vec::new();
        for guid in [&GUID_BATTERY_PERCENTAGE_REMAINING, &GUID_ACDC_POWER_SOURCE] {
            match unsafe { RegisterPowerSettingNotification(HANDLE(hwnd.0), guid, DEVICE_NOTIFY_WINDOW_HANDLE) } {
                Ok(h) => handles.push(h),
                Err(e) => log::warn!("power notification: {e}"),
            }
        }
        Self { handles }
    }
}

impl Drop for PowerNotify {
    fn drop(&mut self) {
        for h in self.handles.drain(..) {
            unsafe {
                let _ = UnregisterPowerSettingNotification(h);
            }
        }
    }
}

/// `None` when the machine has no battery.
pub fn battery() -> Option<Battery> {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s) }.ok()?;
    const NO_BATTERY: u8 = 128;
    const CHARGING: u8 = 8;
    if s.BatteryFlag & NO_BATTERY != 0 || s.BatteryFlag == 255 || s.BatteryLifePercent > 100 {
        return None;
    }
    let ac = s.ACLineStatus == 1;
    Some(Battery { percent: s.BatteryLifePercent, charging: s.BatteryFlag & CHARGING != 0 || (ac && s.BatteryLifePercent < 100) })
}
