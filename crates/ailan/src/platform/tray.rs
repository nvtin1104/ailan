//! Tray icon, context menu and balloon notifications.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, GetDC, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_RESPECT_QUIET_TIME, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, SetForegroundWindow,
    TrackPopupMenu, HICON, ICONINFO, MF_CHECKED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, TPM_BOTTOMALIGN,
    TPM_RETURNCMD, TPM_RIGHTBUTTON,
};

use super::{copy_wide, wide, WM_APP_TRAY};

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCmd {
    Settings = 1,
    Autostart,
    Language,
    About,
    Exit,
}

pub struct MenuItem<'a> {
    pub cmd: MenuCmd,
    pub label: &'a str,
    pub checked: Option<bool>,
}

impl Tray {
    pub fn new(hwnd: HWND) -> Self {
        let icon = make_icon(32).unwrap_or_default();
        let tray = Self { hwnd, icon };
        tray.add();
        tray
    }

    fn base(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            ..Default::default()
        }
    }

    /// (Re-)add the icon — also called after Explorer restarts (TaskbarCreated).
    pub fn add(&self) {
        let mut nid = self.base();
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_APP_TRAY;
        nid.hIcon = self.icon;
        copy_wide(&mut nid.szTip, "Ailan");
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
            nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
        }
    }

    /// Native Windows notification attributed to the tray icon.
    pub fn notify(&self, title: &str, body: &str) {
        let mut nid = self.base();
        nid.uFlags = NIF_INFO;
        nid.dwInfoFlags = NIIF_INFO | NIIF_RESPECT_QUIET_TIME;
        copy_wide(&mut nid.szInfoTitle, title);
        copy_wide(&mut nid.szInfo, body);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    /// Show the context menu at the cursor; returns the chosen command.
    pub fn menu(&self, items: &[Option<MenuItem>]) -> Option<MenuCmd> {
        unsafe {
            let menu = CreatePopupMenu().ok()?;
            for item in items {
                match item {
                    None => {
                        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                    }
                    Some(it) => {
                        let flags = match it.checked {
                            Some(true) => MF_STRING | MF_CHECKED,
                            Some(false) => MF_STRING | MF_UNCHECKED,
                            None => MF_STRING,
                        };
                        let label = wide(it.label);
                        let _ = AppendMenuW(menu, flags, it.cmd as usize, PCWSTR(label.as_ptr()));
                    }
                }
            }
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            // Required so the menu closes when clicking elsewhere.
            let _ = SetForegroundWindow(self.hwnd);
            let id = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, None, self.hwnd, None);
            let _ = DestroyMenu(menu);
            [
                MenuCmd::Settings,
                MenuCmd::Autostart,
                MenuCmd::Language,
                MenuCmd::About,
                MenuCmd::Exit,
            ]
            .into_iter()
            .find(|c| *c as i32 == id.0)
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let nid = self.base();
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            if !self.icon.is_invalid() {
                let _ = DestroyIcon(self.icon);
            }
        }
    }
}

/// Draw or load the AiLan mark into an HICON.
fn make_icon(size: i32) -> windows::core::Result<HICON> {
    unsafe {
        if let Ok(inst) = windows::Win32::System::LibraryLoader::GetModuleHandleW(None) {
            if let Ok(h) = windows::Win32::UI::WindowsAndMessaging::LoadImageW(
                Some(inst.into()),
                PCWSTR(1 as _),
                windows::Win32::UI::WindowsAndMessaging::IMAGE_ICON,
                size,
                size,
                windows::Win32::UI::WindowsAndMessaging::LR_DEFAULTCOLOR,
            ) {
                if !h.is_invalid() {
                    return Ok(HICON(h.0));
                }
            }
        }
    }

    let px = crate::platform::icon_art::render_icon(size);
    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let hdc = GetDC(None);
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(Some(hdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        ReleaseDC(None, hdc);
        let color = color?;
        std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u8, px.len());
        let mask_bits = vec![0u8; ((size + 15) / 16 * 2 * size) as usize];
        let mask = CreateBitmap(size, size, 1, 1, Some(mask_bits.as_ptr() as _));
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info);
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}
