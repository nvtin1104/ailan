//! Windows App Window configuration (DWM rounded corners, custom non-client framing,
//! dark mode titlebar and taskbar application window).

use windows::core::{w, PCWSTR, Result};
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_BORDER_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWINDOWATTRIBUTE,
};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::*;

// DWMWCP_ROUND = 2
const DWMWCP_ROUND: u32 = 2;
// DWMWA_SYSTEMBACKDROP_TYPE = 38
const DWMWA_SYSTEMBACKDROP_TYPE: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(38);
// DWMSBT_TRANSIENTWINDOW = 3 (Acrylic / Liquid Glass backdrop)
const DWMSBT_TRANSIENTWINDOW: u32 = 3;
// DWMWA_MICA_EFFECT = 1029 (Windows 11 build 22000 fallback)
const DWMWA_MICA_EFFECT: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(1029);

/// Create the first-class AiLan application window with full DWM integration.
pub fn create_app_window(wndproc: WNDPROC) -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = w!("AiLanAppWindow");

        // Load application icon from embedded resource 1
        let icon = LoadImageW(
            Some(instance.into()),
            PCWSTR(1 as _),
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE | LR_SHARED,
        )
        .ok()
        .and_then(|h| if h.is_invalid() { None } else { Some(HICON(h.0)) });

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: wndproc,
            hInstance: instance.into(),
            hIcon: icon.unwrap_or_default(),
            hIconSm: icon.unwrap_or_default(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW,
            class,
            w!("AiLan"),
            WS_POPUP | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1000,
            680,
            None,
            None,
            Some(instance.into()),
            None,
        )?;

        // Apply modern Windows 11 DWM rounded corners and dark theme
        apply_modern_window_styling(hwnd);

        Ok(hwnd)
    }
}

pub fn apply_modern_window_styling(hwnd: HWND) {
    unsafe {
        // 1. Dark mode titlebar & frame
        let dark = 1i32;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const _ as _,
            std::mem::size_of::<i32>() as u32,
        );

        // 2. Windows 11 Rounded Corners (DWMWA_WINDOW_CORNER_PREFERENCE = 33, DWMWCP_ROUND = 2)
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &round as *const _ as _,
            std::mem::size_of::<u32>() as u32,
        );

        // 3. Liquid Glass Acrylic backdrop (Windows 11 22621+) with Mica fallback (22000)
        let backdrop = DWMSBT_TRANSIENTWINDOW;
        let res = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &backdrop as *const _ as _,
            std::mem::size_of::<u32>() as u32,
        );
        if res.is_err() {
            let mica = 1i32;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_MICA_EFFECT,
                &mica as *const _ as _,
                std::mem::size_of::<i32>() as u32,
            );
        }

        // 4. Border color: DWMWA_COLOR_NONE (0xFFFFFFFE) hides the default thick OS border
        // so our custom fluent liquid glass hairline border renders cleanly without duplication.
        let no_border = COLORREF(0xFFFFFFFE);
        let res = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &no_border as *const _ as _,
            std::mem::size_of::<COLORREF>() as u32,
        );
        if res.is_err() {
            let border_color = COLORREF(0x00262A30);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &border_color as *const _ as _,
                std::mem::size_of::<COLORREF>() as u32,
            );
        }

        // 5. Extend frame completely into client area (-1: sheet of glass)
        // This ensures the client area receives true transparency / acrylic diffusion
        // and eliminates non-client background clipping.
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
    }
}
