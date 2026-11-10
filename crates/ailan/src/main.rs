//! Ailan — a living notch for Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod gfx;
mod modals;
mod platform;
mod ui;

use std::time::Instant;

use ailan_core::i18n::Lang;
use ailan_core::settings::{Data, Settings};
use ailan_core::storage::{self, Format, Loaded};
use windows::core::{w, Result};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;

use app::{App, Init};
use modals::ModalKind;
use platform::sys;

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        if !ptr.is_null() {
            if let Some(r) = (*ptr).handle(msg, wparam, lparam) {
                return r;
            }
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

extern "system" fn modal_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        if !ptr.is_null() {
            if let Some(r) = (*ptr).handle_modal(msg, wparam, lparam) {
                return r;
            }
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

fn main() {
    let t0 = Instant::now();
    sys::init_logging();
    if let Err(e) = run(t0) {
        log::error!("fatal: {e}");
    }
}

fn run(t0: Instant) -> Result<()> {
    let stats = std::env::args().any(|a| a == "--stats");
    let pin_tab = std::env::args().find_map(|a| match a.as_str() {
        "--expand=music" => Some(ailan_core::settings::Tab::Music),
        "--expand=focus" => Some(ailan_core::settings::Tab::Focus),
        "--expand=todo" => Some(ailan_core::settings::Tab::Todo),
        _ => None,
    });
    let initial_modal = std::env::args().find_map(|a| match a.as_str() {
        "--settings" => Some(ModalKind::Settings),
        "--add" => Some(ModalKind::AddActivity),
        _ => None,
    });

    // One notch per session.
    let _mutex = unsafe { CreateMutexW(None, true, w!("Local\\Ailan.Notch.SingleInstance"))? };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        log::info!("already running");
        return Ok(());
    }

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }

    // Settings & data.
    let dir = sys::config_dir();
    let settings_path = dir.join("settings.toml");
    let data_path = dir.join("data.json");
    let (mut settings, how): (Settings, _) = storage::load(&settings_path, Format::Toml);
    if let Loaded::Recovered(p) = &how {
        log::warn!("settings were corrupt; backed up to {}", p.display());
    }
    let (data, how): (Data, _) = storage::load(&data_path, Format::Json);
    if let Loaded::Recovered(p) = &how {
        log::warn!("data was corrupt; backed up to {}", p.display());
    }
    let first_run = settings.lang.is_none();
    if first_run {
        settings.lang = Some(Lang::from_locale(&sys::locale_name()));
    }

    let hwnd = create_window()?;
    let modal_hwnd = create_modal_window()?;
    let mut app = Box::new(App::new(
        hwnd,
        modal_hwnd,
        Init { settings, data, settings_path, data_path, stats, pin_tab, initial_modal },
    )?);
    if first_run {
        app.state.mark_settings_dirty(sys::now_ms());
    }
    let app: *mut App = Box::into_raw(app);
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, app as isize);
        SetWindowLongPtrW(modal_hwnd, GWLP_USERDATA, app as isize);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        (*app).frame();
    }
    log::info!("Ailan {} started; first frame in {} ms", env!("CARGO_PKG_VERSION"), t0.elapsed().as_millis());
    sys::trim_working_set();

    // Event loop: block in GetMessage while idle; only spin while animating.
    let mut msg = MSG::default();
    'outer: loop {
        unsafe {
            if !(*app).wants_frame() {
                if GetMessageW(&mut msg, None, 0, 0).0 <= 0 {
                    break;
                }
                (*app).note_wakeup();
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break 'outer;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if (*app).quit {
                break;
            }
            (*app).frame();
        }
    }

    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        SetWindowLongPtrW(modal_hwnd, GWLP_USERDATA, 0);
        drop(Box::from_raw(app));
    }
    log::info!("bye");
    Ok(())
}

fn create_window() -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = w!("AilanNotch");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
            class,
            w!("Ailan"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
}

fn create_modal_window() -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = w!("AilanModal");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(modal_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP,
            class,
            w!("Ailan Settings"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
}
