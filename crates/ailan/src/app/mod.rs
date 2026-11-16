//! The notch window: owns state, routes OS messages, paces animation and
//! arms the single scheduler timer. Also manages the unified Activity &
//! Settings Hub window.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;

use ailan_core::anim::Spring;
use ailan_core::event::Artwork;
use ailan_core::i18n::{t, Key};
use ailan_core::scheduler::{self, View as SchedView};
use ailan_core::settings::{HubPage, Settings, Tab};
use ailan_core::state::{BannerIcon, Collapsed};
use ailan_core::storage::{self, Format};
use ailan_core::{AppEvent, AppState};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, POINT};
use windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush1;
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, EnumDisplayMonitors, GetMonitorInfoW,
    MonitorFromPoint, SetWindowRgn, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
    MONITOR_DEFAULTTOPRIMARY, RGN_OR,
};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{self, *};

use crate::gfx::Gfx;
use crate::hub::controller::HubController;
use crate::platform::appbar::{self, AppBar};
use crate::platform::audio::Audio;
use crate::platform::media::{self, MediaControl};
use crate::platform::power::{self, PowerNotify};
use crate::platform::tray::Tray;
use crate::platform::{brightness, network, sys, EventSink};
use crate::ui::{self, Action, Hit};

mod effects;
mod hub_host;
mod notch;

const T_TICK: usize = 1;
const T_STATS: usize = 4;
/// Hub refresh while a live clock tool is on screen (stopwatch, timer, world clock).
const T_HUB: usize = 6;

/// Flight tints for icons that jump into the island. Not used as pill fills.
const FLY_TODO: u32 = crate::hub::widgets::ACCENT_MINT;
const FLY_REMIND: u32 = crate::hub::widgets::ACCENT_AMBER;
const PBT_POWERSETTINGCHANGE: usize = 0x8013;
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;

fn apply_cursor_icon(icon: ui::CursorIcon) {
    let id = match icon {
        ui::CursorIcon::Default => IDC_ARROW,
        ui::CursorIcon::Pointer => IDC_HAND,
        ui::CursorIcon::Text => IDC_IBEAM,
    };
    unsafe {
        if let Ok(c) = LoadCursorW(None, id) {
            let _ = SetCursor(Some(c));
        }
    }
}

/// Pointer capture that distinguishes a click from a drag.
struct Grab {
    sx: i32,
    sy: i32,
    dragging: bool,
}

#[derive(Default)]
struct Stats {
    frames: u64,
    wakeups: u64,
}

pub struct App {
    hwnd: HWND,
    hub_hwnd: HWND,
    gfx: Option<Gfx>,
    hub_gfx: Option<Gfx>,
    pub state: AppState,
    rx: Receiver<AppEvent>,
    media: MediaControl,
    audio: Option<Audio>,
    tray: Tray,
    _power: PowerNotify,
    _appbar: AppBar,
    settings_path: PathBuf,
    data_path: PathBuf,

    expanded: bool,
    w: Spring,
    h: Spring,
    e: Spring,
    hover_anim: Spring,
    tab_pos: Spring,
    mode: Collapsed,
    tab: Tab,
    input: Option<String>,
    high_surrogate: Option<u16>,
    todo_scroll: usize,
    hits: Vec<Hit>,
    hover: Option<Action>,
    cursor: Option<(f32, f32)>,
    mouse_inside: bool,
    /// Notch window origin on screen (px).
    origin: (i32, i32),
    /// Edge / corner / float silhouette. Forced to `Float` while dragging.
    dock: ui::Dock,
    grab: Option<Grab>,
    /// Ignore clicks until this timestamp so a double-click does not toggle twice.
    eat_until: i64,
    /// Measured banner/OSD width in DIP.
    alert_w: f32,
    alert_key: String,
    edit: HWND,
    scale_y: f32,
    /// Hub → island flight overlay, and when the last one landed (for the glow).
    flight: Option<crate::platform::flight::Flight>,
    tz: crate::platform::tz::TzCache,
    /// Current `T_HUB` interval in ms (0 = off).
    hub_timer_ms: u32,
    landed_at: Option<Instant>,
    audio_bars: [f32; 5],
    hidden: bool,
    art: Option<(Arc<Artwork>, ID2D1BitmapBrush1)>,
    scale: f32,
    /// Hit-region cache: left, top, right, bottom, radius, dock discriminant.
    region: (i32, i32, i32, i32, i32, i32),
    needs_draw: bool,
    last_frame: Instant,
    armed: Option<i64>,
    eco: Option<bool>,
    stats: Option<Stats>,
    taskbar_created: u32,
    pinned: bool,

    // AiLan Personal Hub application window state
    pub hub_ctrl: HubController,
    pub hub_open: bool,
    hub_needs_draw: bool,
    hub_cursor: Option<(f32, f32)>,
    hub_mouse_inside: bool,
    hub_anim: Option<hub_host::HubWindowAnim>,

    pub quit: bool,
}

pub struct Init {
    pub settings: Settings,
    pub data: ailan_core::settings::Data,
    pub settings_path: PathBuf,
    pub data_path: PathBuf,
    pub stats: bool,
    /// Debug aid: start expanded on this tab and stay open (`--expand=focus`).
    pub pin_tab: Option<Tab>,
    /// Start with the Personal Hub window open (`--hub` or `--settings`).
    pub open_hub: bool,
    /// Page to open the hub on (`--hub=island`).
    pub hub_page: Option<HubPage>,
    /// Start the hub in Compact mode (`--compact`).
    pub hub_compact: bool,
}

impl App {
    pub fn new(hwnd: HWND, hub_hwnd: HWND, init: Init) -> windows::core::Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        let sink = EventSink::new(tx, hwnd);
        let state = AppState::new(init.settings, init.data, sys::utc_offset_min());
        let mode = state.collapsed(sys::now_ms());
        let (cw, ch) = ui::collapsed_size(mode, ui::REST_W, ui::Dock::Top);

        let media = media::spawn(sink.clone());
        let audio = Audio::new(sink.clone()).map_err(|e| log::warn!("audio: {e}")).ok();
        brightness::spawn(sink.clone());
        network::spawn(sink.clone());

        let tab = state.settings.tab;
        let init_pos = visible_index(&state, tab, sys::now_ms());
        let mut app = Self {
            hwnd,
            hub_hwnd,
            gfx: None,
            hub_gfx: None,
            state,
            rx,
            media,
            audio,
            tray: Tray::new(hwnd),
            _power: PowerNotify::register(hwnd),
            _appbar: AppBar::register(hwnd),
            settings_path: init.settings_path,
            data_path: init.data_path,
            expanded: false,
            w: Spring::new(cw).with(380.0, 40.0),
            h: Spring::new(ch).with(380.0, 40.0),
            e: Spring::new(0.0).with(340.0, 37.0),
            hover_anim: Spring::new(0.0).with(620.0, 38.0),
            tab_pos: Spring::new(init_pos).with(440.0, 34.0),
            mode,
            tab,
            input: None,
            high_surrogate: None,
            todo_scroll: 0,
            hits: Vec::new(),
            hover: None,
            cursor: None,
            mouse_inside: false,
            origin: (0, 0),
            dock: ui::Dock::Top,
            grab: None,
            eat_until: 0,
            alert_w: ui::REST_W,
            alert_key: String::new(),
            edit: HWND::default(),
            scale_y: 1.0,
            flight: crate::platform::flight::Flight::create()
                .map_err(|e| log::warn!("flight overlay unavailable: {e}"))
                .ok(),
            landed_at: None,
            audio_bars: [0.0; 5],
            tz: Default::default(),
            hub_timer_ms: 0,
            hidden: false,
            art: None,
            scale: 1.0,
            region: (0, 0, 0, 0, 0, 0),
            needs_draw: true,
            last_frame: Instant::now(),
            armed: None,
            eco: None,
            stats: init.stats.then(Stats::default),
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
            pinned: init.pin_tab.is_some(),
            hub_ctrl: HubController::new(),
            hub_open: false,
            hub_needs_draw: false,
            hub_cursor: None,
            hub_mouse_inside: false,
            hub_anim: None,
            quit: false,
        };
        app.place()?;
        let now = sys::now_ms();
        app.state.apply(AppEvent::Battery(power::battery()), now);
        if appbar::fullscreen_now() {
            app.set_hidden(true);
        }
        if app.stats.is_some() {
            unsafe { SetTimer(Some(hwnd), T_STATS, 60_000, None) };
        }
        if let Some(tab) = init.pin_tab {
            app.expanded = true;
            app.tab = tab;
            app.tab_pos.value = visible_index(&app.state, tab, sys::now_ms());
        }
        app.hub_ctrl.select_page(app.state.settings.hub_page);
        app.hub_ctrl.is_maximized = app.state.settings.hub_maximized && !init.hub_compact;
        if let (Some(w), Some(h)) = (app.state.settings.hub_w, app.state.settings.hub_h) {
            app.hub_ctrl.apply_size(w, h);
        }
        if init.open_hub {
            if init.hub_compact {
                app.hub_ctrl.is_maximized = false;
                app.hub_ctrl.apply_size(380.0, 600.0);
            }
            app.open_hub(init.hub_page);
        }
        app.after_change();
        Ok(app)
    }

    // ---- geometry ----------------------------------------------------------------

    /// Place the cap window from the saved anchor, or top-centre of the primary work area.
    fn place(&mut self) -> windows::core::Result<()> {
        let (mon, device, work) = self.resolve_anchor_monitor();
        self.apply_monitor_dpi(mon);
        self.dock = self.dock_from_settings(work);
        if !self.expanded {
            let (cw, ch) = ui::collapsed_size(self.mode, self.alert_w, self.dock);
            self.w.value = cw;
            self.w.set_target(cw);
            self.h.value = ch;
            self.h.set_target(ch);
        }
        let (wpx, hpx) = self.cap_px();
        let (x, y) = self.origin_for(work, wpx, hpx);
        self.commit_origin(x, y, wpx, hpx)?;
        let _ = device;
        Ok(())
    }

    fn dock_from_settings(&self, work: windows::Win32::Foundation::RECT) -> ui::Dock {
        let (Some(ax), Some(ay)) = (self.state.settings.anchor_x, self.state.settings.anchor_y) else {
            return ui::Dock::Top;
        };
        let ww = (work.right - work.left).max(1) as f32;
        let wh = (work.bottom - work.top).max(1) as f32;
        let cx = work.left + (ax.clamp(0.0, 1.0) * ww).round() as i32;
        let cy = work.top + (ay.clamp(0.0, 1.0) * wh).round() as i32;
        let pw = (ui::REST_W * self.scale).ceil() as i32;
        let ph = (ui::REST_H * self.scale_y).ceil() as i32;
        let corner = (ui::SNAP_CORNER_DIP * self.scale).round() as i32;
        ui::Dock::snap_perimeter(cx, cy, pw, ph, (work.left, work.top, work.right, work.bottom), corner).0
    }

    /// DPI changed while dragging: resize the cap, keep following the cursor.
    pub(super) fn refresh_scale_keep_origin(&mut self) -> windows::core::Result<()> {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        let mon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
        self.apply_monitor_dpi(mon);
        let (wpx, hpx) = self.cap_px();
        self.commit_origin(self.origin.0, self.origin.1, wpx, hpx)
    }

    fn cap_px(&self) -> (i32, i32) {
        ((ui::WIN_W * self.scale).ceil() as i32, (ui::WIN_H * self.scale_y).ceil() as i32)
    }

    fn apply_monitor_dpi(&mut self, mon: HMONITOR) {
        let (mut dx, mut dy) = (96u32, 96u32);
        unsafe {
            let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        }
        self.scale = dx as f32 / 96.0;
        self.scale_y = dy as f32 / 96.0;
    }

    fn origin_for(&self, work: windows::Win32::Foundation::RECT, wpx: i32, hpx: i32) -> (i32, i32) {
        let ww = (work.right - work.left).max(1);
        let wh = (work.bottom - work.top).max(1);
        let (ax, ay) = match (self.state.settings.anchor_x, self.state.settings.anchor_y) {
            (Some(x), Some(y)) => (x, y),
            _ => return (work.left + (ww - wpx) / 2, work.top),
        };
        let cx = work.left + (ax.clamp(0.0, 1.0) * ww as f32).round() as i32;
        let cy = work.top + (ay.clamp(0.0, 1.0) * wh as f32).round() as i32;
        let pill = ui::pill_rect(self.w.value, self.h.value, self.dock);
        let pw = (pill.w * self.scale).ceil() as i32;
        let ph = (pill.h * self.scale_y).ceil() as i32;
        let x = match self.dock {
            ui::Dock::Left | ui::Dock::TopLeft | ui::Dock::BottomLeft => work.left,
            ui::Dock::Right | ui::Dock::TopRight | ui::Dock::BottomRight => work.right - wpx,
            ui::Dock::Top | ui::Dock::Bottom => {
                let left = (cx - pw / 2).clamp(work.left, (work.right - pw).max(work.left));
                left - (pill.x * self.scale).round() as i32
            }
        };
        let y = match self.dock {
            ui::Dock::Top | ui::Dock::TopLeft | ui::Dock::TopRight => work.top,
            ui::Dock::Bottom | ui::Dock::BottomLeft | ui::Dock::BottomRight => work.bottom - hpx + 1,
            ui::Dock::Left | ui::Dock::Right => {
                let top = (cy - ph / 2).clamp(work.top, (work.bottom - ph).max(work.top));
                top - (pill.y * self.scale_y).round() as i32
            }
        };
        (x, y)
    }

    fn commit_origin(&mut self, x: i32, y: i32, wpx: i32, hpx: i32) -> windows::core::Result<()> {
        unsafe {
            SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, wpx, hpx, SWP_NOACTIVATE)?;
        }
        self.origin = (x, y);
        match self.gfx.as_mut() {
            Some(g) => g.resize(wpx as u32, hpx as u32, self.scale)?,
            None => self.gfx = Some(Gfx::new(self.hwnd, wpx as u32, hpx as u32, self.scale)?),
        }
        self.region = (0, 0, 0, 0, 0, 0);
        self.needs_draw = true;
        Ok(())
    }

    /// Silhouette used for layout / paint. Always docked to the screen perimeter.
    pub(super) fn visual_dock(&self) -> ui::Dock {
        self.dock
    }

    fn resolve_anchor_monitor(&self) -> (HMONITOR, String, windows::Win32::Foundation::RECT) {
        let want = self.state.settings.monitor.clone();
        let mut found: Option<(HMONITOR, String, windows::Win32::Foundation::RECT)> = None;
        let mut primary: Option<(HMONITOR, String, windows::Win32::Foundation::RECT)> = None;
        let mut mons = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(enum_mon), LPARAM(&mut mons as *mut Vec<HMONITOR> as isize));
            for mon in mons {
                if let Some((name, info)) = monitor_ex(mon) {
                    let work = info.rcWork;
                    let is_primary = info.dwFlags & WindowsAndMessaging::MONITORINFOF_PRIMARY != 0;
                    if primary.is_none() || is_primary {
                        if is_primary || primary.is_none() {
                            primary = Some((mon, name.clone(), work));
                        }
                    }
                    if want.as_deref() == Some(name.as_str()) {
                        found = Some((mon, name, work));
                    }
                }
            }
        }
        found.or(primary).unwrap_or_else(|| {
            let mon = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
            let (name, info) = monitor_ex(mon).unwrap_or_else(|| (String::new(), MONITORINFO::default()));
            (mon, name, info.rcWork)
        })
    }

    /// Project the cursor onto the nearest work-area edge/corner and position the island flush to it.
    pub(super) fn snap_drag_to_cursor(&mut self, save: bool) {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        let mon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
        let Some((name, info)) = monitor_ex(mon) else { return };
        let work = info.rcWork;
        let corner = (ui::SNAP_CORNER_DIP * self.scale).round() as i32;
        let prev_dock = self.dock;

        // Keep corner docks sticky within a hysteresis zone so holding near a diagonal
        // cannot oscillate between horizontal corner and vertical edge sizes.
        let sticky = (ui::SNAP_CORNER_DIP * 2.0 * self.scale).round() as i32;
        let keep_corner = match prev_dock {
            ui::Dock::TopLeft => (pt.x - work.left) <= sticky && (pt.y - work.top) <= sticky,
            ui::Dock::TopRight => (work.right - pt.x) <= sticky && (pt.y - work.top) <= sticky,
            ui::Dock::BottomLeft => (pt.x - work.left) <= sticky && (work.bottom - pt.y) <= sticky,
            ui::Dock::BottomRight => (work.right - pt.x) <= sticky && (work.bottom - pt.y) <= sticky,
            _ => false,
        };

        let (base_w, base_h) = if self.expanded {
            (self.w.target, self.h.target)
        } else {
            ui::collapsed_size(self.mode, self.alert_w, prev_dock)
        };
        let (dock, left, top, pw, ph, target_w, target_h) = if keep_corner {
            let pw = (base_w * self.scale).ceil() as i32;
            let ph = (base_h * self.scale_y).ceil() as i32;
            let left = match prev_dock {
                ui::Dock::TopLeft | ui::Dock::BottomLeft => work.left,
                _ => (work.right - pw).max(work.left),
            };
            let top = match prev_dock {
                ui::Dock::TopLeft | ui::Dock::TopRight => work.top,
                _ => (work.bottom - ph).max(work.top),
            };
            (prev_dock, left, top, pw, ph, base_w, base_h)
        } else {
            let base_pw = (base_w * self.scale).ceil() as i32;
            let base_ph = (base_h * self.scale_y).ceil() as i32;
            let (dock, _, _) = ui::Dock::snap_perimeter(
                pt.x,
                pt.y,
                base_pw,
                base_ph,
                (work.left, work.top, work.right, work.bottom),
                corner,
            );
            let (target_w, target_h) = if self.expanded {
                (self.w.target, self.h.target)
            } else {
                ui::collapsed_size(self.mode, self.alert_w, dock)
            };
            let pw = (target_w * self.scale).ceil() as i32;
            let ph = (target_h * self.scale_y).ceil() as i32;
            let (_, left, top) = ui::Dock::snap_perimeter(
                pt.x,
                pt.y,
                pw,
                ph,
                (work.left, work.top, work.right, work.bottom),
                corner,
            );
            (dock, left, top, pw, ph, target_w, target_h)
        };

        if dock != prev_dock {
            self.dock = dock;
            if !self.expanded {
                self.w.set_target(target_w);
                self.h.set_target(target_h);
                if self.state.settings.reduce_motion {
                    self.w.snap();
                    self.h.snap();
                }
            }
        }

        let (wpx, hpx) = self.cap_px();
        let pill = ui::pill_rect(target_w, target_h, dock);
        let x = match dock {
            ui::Dock::Left | ui::Dock::TopLeft | ui::Dock::BottomLeft => work.left,
            ui::Dock::Right | ui::Dock::TopRight | ui::Dock::BottomRight => work.right - wpx,
            ui::Dock::Top | ui::Dock::Bottom => left - (pill.x * self.scale).round() as i32,
        };
        let y = match dock {
            ui::Dock::Top | ui::Dock::TopLeft | ui::Dock::TopRight => work.top,
            ui::Dock::Bottom | ui::Dock::BottomLeft | ui::Dock::BottomRight => work.bottom - hpx + 1,
            ui::Dock::Left | ui::Dock::Right => top - (pill.y * self.scale_y).round() as i32,
        };
        if (x, y) != self.origin || dock != prev_dock {
            unsafe {
                let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
            self.origin = (x, y);
            self.region = (0, 0, 0, 0, 0, 0);
            self.needs_draw = true;
        }

        if save {
            let ww = (work.right - work.left).max(1) as f32;
            let wh = (work.bottom - work.top).max(1) as f32;
            let ax = match dock {
                ui::Dock::Left | ui::Dock::TopLeft | ui::Dock::BottomLeft => 0.0,
                ui::Dock::Right | ui::Dock::TopRight | ui::Dock::BottomRight => 1.0,
                ui::Dock::Top | ui::Dock::Bottom => ((left + pw / 2 - work.left) as f32 / ww).clamp(0.0, 1.0),
            };
            let ay = match dock {
                ui::Dock::Top | ui::Dock::TopLeft | ui::Dock::TopRight => 0.0,
                ui::Dock::Bottom | ui::Dock::BottomLeft | ui::Dock::BottomRight => 1.0,
                ui::Dock::Left | ui::Dock::Right => ((top + ph / 2 - work.top) as f32 / wh).clamp(0.0, 1.0),
            };
            self.state.settings.monitor = Some(name);
            self.state.settings.anchor_x = Some(ax);
            self.state.settings.anchor_y = Some(ay);
            self.state.mark_settings_dirty(sys::now_ms());
        }
    }

    /// Drop the pill on the nearest perimeter edge/corner and remember the anchor.
    pub(super) fn commit_drag(&mut self) {
        self.snap_drag_to_cursor(true);
    }

    /// Hit region wraps the drawn silhouette with a 2px transparent pad so GDI never clips
    /// anti-aliased Direct2D edges or flush bezel pixels (WS_EX_NOREDIRECTIONBITMAP keeps pad invisible).
    fn update_region(&mut self) {
        let dock = self.visual_dock();
        let p = ui::pill_rect(self.w.value, self.h.value, dock);
        let radii = ui::pill_radii(dock, self.e.value, self.w.value, self.h.value);
        let ear = ui::ear_for(dock);
        let s = self.scale;
        let sy = self.scale_y;
        let (wpx, hpx) = self.cap_px();
        let pad = 2;
        let l = ((p.x * s).floor() as i32 - pad).max(0);
        let t = ((p.y * sy).floor() as i32 - pad).max(0);
        let r = ((p.right() * s).ceil() as i32 + pad).min(wpx);
        let b = ((p.bottom() * sy).ceil() as i32 + pad).min(hpx);
        let rad = (radii.iter().cloned().fold(0.0_f32, f32::max) * s).round().max(1.0) as i32;
        let dock_code = match dock {
            ui::Dock::Top => 1,
            ui::Dock::Bottom => 2,
            ui::Dock::Left => 3,
            ui::Dock::Right => 4,
            ui::Dock::TopLeft => 5,
            ui::Dock::TopRight => 6,
            ui::Dock::BottomLeft => 7,
            ui::Dock::BottomRight => 8,
        };
        let key = (l, t, r, b, rad, dock_code);
        if key == self.region || r <= l || b <= t {
            return;
        }
        self.region = key;
        unsafe {
            let body = CreateRoundRectRgn(l, t, r, b, rad * 2, rad * 2);
            let or_rect = |x0: i32, y0: i32, x1: i32, y1: i32| {
                if x1 > x0 && y1 > y0 {
                    let rgn = CreateRectRgn(x0, y0, x1, y1);
                    let _ = CombineRgn(Some(body), Some(body), Some(rgn), RGN_OR);
                    let _ = DeleteObject(rgn.into());
                }
            };
            // Square off flush bezel corners so they are never clipped by the round-rect.
            let rx = rad.min((r - l).max(0));
            let ry = rad.min((b - t).max(0));
            if radii[0] < 0.5 {
                or_rect(l, t, l + rx, t + ry);
            }
            if radii[1] < 0.5 {
                or_rect(r - rx, t, r, t + ry);
            }
            if radii[2] < 0.5 {
                or_rect(r - rx, b - ry, r, b);
            }
            if radii[3] < 0.5 {
                or_rect(l, b - ry, l + rx, b);
            }

            let ear_x = (ear * s).round() as i32;
            let ear_y = (ear * sy).round() as i32;
            if ear_x > 0 && ear_y > 0 {
                match dock {
                    ui::Dock::Top => {
                        or_rect(l - ear_x, t, l + 2, t + ear_y);
                        or_rect(r - 2, t, r + ear_x, t + ear_y);
                    }
                    ui::Dock::Bottom => {
                        or_rect(l - ear_x, b - ear_y, l + 2, b);
                        or_rect(r - 2, b - ear_y, r + ear_x, b);
                    }
                    ui::Dock::Left => {
                        or_rect(l, t - ear_y, l + ear_x, t + 2);
                        or_rect(l, b - 2, l + ear_x, b + ear_y);
                    }
                    ui::Dock::Right => {
                        or_rect(r - ear_x, t - ear_y, r, t + 2);
                        or_rect(r - ear_x, b - 2, r, b + ear_y);
                    }
                    ui::Dock::TopLeft => {
                        or_rect(r - 2, t, r + ear_x, t + ear_y);
                        or_rect(l, b - 2, l + ear_x, b + ear_y);
                    }
                    ui::Dock::TopRight => {
                        or_rect(l - ear_x, t, l + 2, t + ear_y);
                        or_rect(r - ear_x, b - 2, r, b + ear_y);
                    }
                    ui::Dock::BottomLeft => {
                        or_rect(l, t - ear_y, l + ear_x, t + 2);
                        or_rect(r - 2, b - ear_y, r + ear_x, b);
                    }
                    ui::Dock::BottomRight => {
                        or_rect(l - ear_x, b - ear_y, l + 2, b);
                        or_rect(r - ear_x, t - ear_y, r, t + 2);
                    }
                }
            }
            SetWindowRgn(self.hwnd, Some(body), false);
        }
    }

    pub(super) fn refresh_alert_width(&mut self) {
        let (text, extra) = match self.mode {
            Collapsed::Banner => {
                let Some(b) = &self.state.banner else {
                    self.alert_w = ui::REST_W;
                    return;
                };
                let line = if b.icon == BannerIcon::Reminder { b.body.clone() } else { b.title.clone() };
                let extra = if b.todo_id.is_some() || b.alarm_id.is_some() {
                    70.0
                } else if matches!(b.icon, BannerIcon::Charging | BannerIcon::BatteryLow) {
                    52.0
                } else {
                    8.0
                };
                (line, extra)
            }
            Collapsed::Osd => {
                let Some(o) = &self.state.osd else {
                    self.alert_w = ui::REST_W;
                    return;
                };
                let level = if o.muted { 0.0 } else { o.level };
                (format!("{}", (level * 100.0).round() as i32), 130.0)
            }
            _ => {
                self.alert_w = ui::REST_W;
                self.alert_key.clear();
                return;
            }
        };
        let key = format!("{text}|{extra}|{:.3}", self.scale);
        if key == self.alert_key {
            return;
        }
        self.alert_key = key;
        let measured = if let Some(g) = self.gfx.as_mut() {
            g.measure_line(&text, crate::gfx::Font::text(13.0, 600, crate::gfx::Align::Left))
        } else {
            text.chars().count() as f32 * 7.2
        };
        self.alert_w = ui::alert_width(measured, true, extra);
    }

    // ---- state transitions -------------------------------------------------------

    fn sched_view(&self) -> SchedView {
        SchedView { expanded: self.expanded, tab: self.tab, hidden: self.hidden }
    }

    /// Recompute targets and timers after anything changed.
    fn after_change(&mut self) {
        let now = sys::now_ms();
        // While something flies into the island, keep showing the previous state; the
        // landing re-runs this and the island then switches to the new activity.
        if !self.flight_active() {
            self.mode = self.state.collapsed(now);
        }
        if let Some(a) = &self.audio {
            a.set_spectrum_active(self.audio_visual_active());
        }

        let visible = self.state.hover_activities(now);
        if !visible.is_empty() && !visible.contains(&self.tab) {
            self.tab = visible[0];
        }

        self.refresh_alert_width();
        let (tw, th) = if self.expanded {
            ui::expanded_size(&self.state, self.tab, now)
        } else {
            ui::collapsed_size(self.mode, self.alert_w, self.dock)
        };
        self.w.set_target(tw);
        self.h.set_target(th);
        self.e.set_target(if self.expanded { 1.0 } else { 0.0 });
        self.tab_pos.set_target(visible_index(&self.state, self.tab, now));
        if self.state.settings.reduce_motion {
            self.w.snap();
            self.h.snap();
            self.e.snap();
            self.hover_anim.snap();
            self.tab_pos.snap();
        }
        self.needs_draw = true;
        if self.hub_open {
            self.hub_needs_draw = true;
        }
        self.arm_timer(now);
        self.refresh_world_offsets(now);
        self.sync_hub_timer();
    }

    fn arm_timer(&mut self, now: i64) {
        let deadline = scheduler::next_deadline(&self.state, now, self.sched_view());
        if deadline == self.armed {
            return;
        }
        self.armed = deadline;
        unsafe {
            match deadline {
                Some(d) => {
                    let ms = (d - now).clamp(1, i32::MAX as i64) as u32;
                    let tolerance = if ms >= 5_000 { 500 } else { 0 };
                    SetCoalescableTimer(Some(self.hwnd), T_TICK, ms, None, tolerance);
                }
                None => {
                    let _ = KillTimer(Some(self.hwnd), T_TICK);
                }
            }
        }
    }

    fn on_tick(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), T_TICK);
        }
        self.armed = None;
        let now = sys::now_ms();
        let lang = self.state.lang();
        for n in self.state.tick(now) {
            // Reminders, alarms and finished timers always raise a Windows toast (with sound).
            let urgent = [Key::Reminder, Key::AlarmTitle, Key::ClockTimer].iter().any(|k| n.title == t(lang, *k));
            if urgent || self.hidden {
                self.tray.notify(&n.title, &n.body);
            } else {
                unsafe {
                    let _ = MessageBeep(MB_ICONASTERISK);
                }
            }
        }
        self.save(false);
        self.after_change();
    }

    pub fn save(&mut self, force: bool) {
        let (s, d) = self.state.take_save(sys::now_ms(), force);
        if s {
            if let Err(e) = storage::save(&self.settings_path, Format::Toml, &self.state.settings) {
                log::error!("save settings: {e}");
            }
        }
        if d {
            if let Err(e) = storage::save(&self.data_path, Format::Json, &self.state.data) {
                log::error!("save data: {e}");
            }
        }
    }

    // ---- frame loop ----------------------------------------------------------------

    fn audio_visual_active(&self) -> bool {
        !self.hidden
            && !self.state.settings.reduce_motion
            && self.state.media.as_ref().is_some_and(|m| m.playing)
            && ((!self.expanded && self.mode == Collapsed::Media)
                || (self.expanded && self.tab == Tab::Music))
    }

    fn step_audio_bars(&mut self, dt: f32, now_ms: i64) {
        let active = self.audio_visual_active();
        if let Some(a) = &self.audio {
            a.set_spectrum_active(active);
        }
        let targets: [f32; 5] = if active {
            if let Some(bands) = self.audio.as_ref().and_then(|a| a.spectrum_bands()) {
                bands
            } else {
                let (peak, [ch_l, ch_r]) = self.audio.as_ref().map(|a| a.sample_peaks()).unwrap_or((0.0, [0.0, 0.0]));
                let t = ((now_ms.rem_euclid(600_000)) as f32) * 0.0085;
                let boost = |v: f32| if v > 0.002 { (v * 2.6).sqrt().clamp(0.32, 1.0) } else { 0.25 };
                let l = boost(ch_l.max(peak * 0.8));
                let m = boost(peak);
                let r = boost(ch_r.max(peak * 0.8));
                let base = [l * 0.82, (l + m) * 0.5, m, (r + m) * 0.5, r * 0.82];
                let freqs = [1.0f32, 1.45, 1.85, 1.3, 0.95];
                let phases = [0.0f32, 1.4, 2.7, 4.1, 5.3];
                std::array::from_fn(|i| {
                    let wave = 0.58 + 0.42 * (t * freqs[i] + phases[i]).sin();
                    (base[i] * wave).clamp(0.14, 1.0)
                })
            }
        } else {
            [0.0; 5]
        };

        let attack = 1.0 - (-dt * 30.0).exp();
        let decay = 1.0 - (-dt * 14.0).exp();
        for i in 0..5 {
            let k = if targets[i] > self.audio_bars[i] { attack } else { decay };
            self.audio_bars[i] += (targets[i] - self.audio_bars[i]) * k;
        }
    }

    fn media_marquee_active(&self) -> bool {
        !self.hidden
            && !self.expanded
            && !self.dock.is_vertical()
            && self.mode == Collapsed::Media
            && !self.state.settings.reduce_motion
            && self.state.media.as_ref().is_some_and(|m| m.playing && m.title.chars().count() > 16)
    }

    pub fn wants_frame(&self) -> bool {
        !self.quit
            && ((!self.hidden && (self.needs_draw || !self.settled() || self.media_marquee_active() || self.audio_visual_active()))
                || self.hub_needs_draw
                || self.hub_anim.is_some()
                || self.flight_active()
                || (self.hub_open && self.hub_ctrl.animating()))
    }

    fn settled(&self) -> bool {
        self.w.is_settled()
            && self.h.is_settled()
            && self.e.is_settled()
            && self.hover_anim.is_settled()
            && self.tab_pos.is_settled()
            && self.glow() <= 0.0
    }

    pub fn note_wakeup(&mut self) {
        if let Some(s) = self.stats.as_mut() {
            s.wakeups += 1;
        }
    }

    /// Step animation and draw one frame if needed.
    pub fn frame(&mut self) {
        if !self.wants_frame() {
            return;
        }
        if !self.hidden && (self.needs_draw || !self.settled() || self.media_marquee_active() || self.audio_visual_active()) {
            let now_i = Instant::now();
            let dt = now_i.duration_since(self.last_frame).as_secs_f32();
            self.last_frame = now_i;
            let animating_before = !self.settled() || self.audio_visual_active();
            let dt = if animating_before && dt < 0.1 { dt.max(1.0 / 240.0) } else { 1.0 / 60.0 };
            self.w.step(dt);
            self.h.step(dt);
            self.e.step(dt);
            self.hover_anim.step(dt);
            self.tab_pos.step(dt);
            self.step_audio_bars(dt, sys::now_ms());
            let animating = !self.settled() || self.media_marquee_active() || self.audio_visual_active();
            if self.glow() <= 0.0 {
                self.landed_at = None;
            }
            self.set_eco(!animating && !self.expanded && !self.hub_open);
            self.update_region();
            self.sync_artwork();
            self.render();
            self.needs_draw = false;
            if let Some(s) = self.stats.as_mut() {
                s.frames += 1;
            }
        }
        self.step_hub_anim();
        if self.hub_open && (self.hub_needs_draw || self.hub_ctrl.animating()) {
            self.hub_ctrl.advance();
            self.render_hub();
            self.hub_needs_draw = false;
        }
        match self.flight.as_mut().and_then(|f| f.frame()) {
            Some(crate::platform::flight::FlightDone::ToIsland) => self.on_flight_landed(),
            Some(crate::platform::flight::FlightDone::FromIsland) => self.on_open_flight_landed(),
            None => {}
        }
    }

    fn set_eco(&mut self, on: bool) {
        if self.eco != Some(on) {
            self.eco = Some(on);
            sys::set_eco_qos(on);
        }
    }

    fn render(&mut self) {
        let now = sys::now_ms();
        let dock = self.visual_dock();
        let view = ui::View {
            now,
            pill: ui::pill_rect(self.w.value, self.h.value, dock),
            dock,
            expand: self.e.value.clamp(0.0, 1.0),
            mode: self.mode,
            tab: self.tab,
            input: self.input.as_deref(),
            todo_scroll: self.todo_scroll,
            art: self.art.as_ref().map(|(a, b)| (b, (a.width, a.height))),
            hover: self.hover,
            hover_t: self.hover_anim.value.clamp(0.0, 1.0),
            tab_pos: self.tab_pos.value.clamp(0.0, 2.0),
            hot: self.mouse_inside,
            glow: self.glow(),
            bars: self.audio_bars,
        };
        let state = &self.state;
        let mut hits = Vec::new();
        let result = match self.gfx.as_mut() {
            Some(g) => g.draw(|cv| hits = ui::draw(cv, state, &view)),
            None => return,
        };
        self.hits = hits;
        self.sync_edit_bounds();
        if let Err(e) = result {
            log::warn!("render failed ({e}); recreating device");
            self.gfx = None;
            self.art = None;
            if let Err(e) = self.place() {
                log::error!("device recreate: {e}");
            }
        }
    }

    fn log_stats(&mut self) {
        if let Some(s) = self.stats.as_mut() {
            log::info!(
                "stats: frames={} wakeups={} ws={:.1}MB",
                s.frames,
                s.wakeups,
                sys::working_set() as f64 / 1_048_576.0
            );
            *s = Stats::default();
        }
    }

    // ---- window procedures -----------------------------------------------------------

    fn mouse_pos(&self, lparam: LPARAM) -> (f32, f32) {
        self.dip_from(lparam, self.scale, self.scale_y)
    }

    pub(super) fn dip_from(&self, lparam: LPARAM, sx: f32, sy: f32) -> (f32, f32) {
        let x = (lparam.0 & 0xFFFF) as i16 as f32 / sx.max(0.5);
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32 / sy.max(0.5);
        (x, y)
    }
}

unsafe extern "system" fn enum_mon(mon: HMONITOR, _: HDC, _: *mut windows::Win32::Foundation::RECT, data: LPARAM) -> windows_core::BOOL {
    let list = &mut *(data.0 as *mut Vec<HMONITOR>);
    list.push(mon);
    windows_core::BOOL(1)
}

fn monitor_ex(mon: HMONITOR) -> Option<(String, MONITORINFO)> {
    unsafe {
        let mut ex = MONITORINFOEXW::default();
        ex.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        let ok = GetMonitorInfoW(mon, &mut ex as *mut MONITORINFOEXW as *mut MONITORINFO);
        if !ok.as_bool() {
            return None;
        }
        let name = {
            let raw = ex.szDevice;
            let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
            String::from_utf16_lossy(&raw[..end])
        };
        Some((name, ex.monitorInfo))
    }
}

fn visible_index(st: &AppState, tab: Tab, now: i64) -> f32 {
    st.hover_activities(now)
        .iter()
        .position(|&t| t == tab)
        .unwrap_or(0) as f32
}
