//! The notch window: owns state, routes OS messages, paces animation and
//! arms the single scheduler timer. Also manages the unified Activity &
//! Settings Hub window.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;

use ailan_core::anim::Spring;
use ailan_core::event::Artwork;
use ailan_core::i18n::{t, Key, Lang};
use ailan_core::scheduler::{self, View as SchedView};
use ailan_core::settings::{Settings, Tab};
use ailan_core::state::{Banner, BannerIcon, Collapsed};
use ailan_core::storage::{self, Format};
use ailan_core::todo::{parse_input, MAX_TEXT};
use ailan_core::{AppEvent, AppState};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush1;
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, FillRect, GetMonitorInfoW, GetStockObject, MonitorFromPoint,
    RedrawWindow, ScreenToClient, SetWindowRgn, BLACK_BRUSH, HBRUSH, HDC, MONITORINFO,
    MONITOR_DEFAULTTOPRIMARY, RDW_ERASE, RDW_INVALIDATE, RDW_UPDATENOW, RGN_OR,
};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_CONTROL,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::gfx::Gfx;
use crate::hub::controller::HubController;
use crate::hub::controller::KeyOutcome;
use crate::hub::{Field, HubAction};
use ailan_core::settings::HubPage;
use crate::platform::appbar::{self, AppBar, ABN_FULLSCREENAPP};
use crate::platform::audio::Audio;
use crate::platform::media::{self, MediaCmd, MediaControl};
use crate::platform::power::{self, PowerNotify};
use crate::platform::tray::{MenuCmd, MenuItem, Tray};
use crate::platform::{autostart, brightness, network, sys, EventSink};
use crate::platform::{WM_APP_APPBAR, WM_APP_AUDIO_DEVICE, WM_APP_EVENT, WM_APP_TRAY};
use crate::ui::{self, Action, Hit};

const T_TICK: usize = 1;
const T_HOVER: usize = 2;
const T_COLLAPSE: usize = 3;
const T_STATS: usize = 4;
const T_AUDIO: usize = 5;
/// Hub refresh while a live clock tool is on screen (stopwatch, timer, world clock).
const T_HUB: usize = 6;

const HOVER_INTENT_MS: u32 = 95;
const COLLAPSE_DELAY_MS: u32 = 340;
/// Flight tints (Hub accent colours) for things that jump into the island.
const FLY_FOCUS: u32 = crate::hub::widgets::ACCENT_PRIMARY;
const FLY_TODO: u32 = crate::hub::widgets::ACCENT_MINT;
const FLY_REMIND: u32 = crate::hub::widgets::ACCENT_AMBER;
const AUDIO_FRAME_MS: u32 = 30;
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
    audio_bars: [f32; 5],
    audio_peak: f32,
    audio_timer_on: bool,
    mode: Collapsed,
    tab: Tab,
    input: Option<String>,
    high_surrogate: Option<u16>,
    todo_scroll: usize,
    hits: Vec<Hit>,
    hover: Option<Action>,
    cursor: Option<(f32, f32)>,
    mouse_inside: bool,
    /// Idle-eye gaze springs (DIP offsets) and whether raw mouse input is registered.
    eye_x: Spring,
    eye_y: Spring,
    raw_mouse: bool,
    /// Notch window origin on screen (px), for mapping the global cursor to the eyes.
    origin: (i32, i32),
    /// Hub → island flight overlay, and when the last one landed (for the glow).
    flight: Option<crate::platform::flight::Flight>,
    tz: crate::platform::tz::TzCache,
    /// Current `T_HUB` interval in ms (0 = off).
    hub_timer_ms: u32,
    landed_at: Option<Instant>,
    hidden: bool,
    art: Option<(Arc<Artwork>, ID2D1BitmapBrush1)>,
    scale: f32,
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
        let (cw, ch) = ui::collapsed_size(mode);

        let media = media::spawn(sink.clone());
        let audio = Audio::new(sink.clone()).map_err(|e| log::warn!("audio: {e}")).ok();
        brightness::spawn(sink.clone());
        network::spawn(sink.clone());

        let tab = state.settings.tab;
        let init_pos = visible_index(&state, tab);
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
            audio_bars: [0.22; 5],
            audio_peak: 0.0,
            audio_timer_on: false,
            mode,
            tab,
            input: None,
            high_surrogate: None,
            todo_scroll: 0,
            hits: Vec::new(),
            hover: None,
            cursor: None,
            mouse_inside: false,
            eye_x: Spring::new(0.0).with(220.0, 26.0),
            eye_y: Spring::new(0.0).with(220.0, 26.0),
            raw_mouse: false,
            origin: (0, 0),
            flight: crate::platform::flight::Flight::create()
                .map_err(|e| log::warn!("flight overlay unavailable: {e}"))
                .ok(),
            landed_at: None,
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
            app.tab_pos.value = visible_index(&app.state, tab);
        }
        if init.open_hub {
            app.hub_ctrl.is_compact = init.hub_compact;
            app.open_hub(init.hub_page);
        }
        app.after_change();
        Ok(app)
    }

    // ---- geometry ----------------------------------------------------------------

    /// Position at the top-centre of the primary monitor and size the swapchain.
    fn place(&mut self) -> windows::core::Result<()> {
        unsafe {
            let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(mon, &mut mi);
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            self.scale = dx as f32 / 96.0;
            let wpx = (ui::WIN_W * self.scale).ceil() as i32;
            let hpx = (ui::WIN_H * self.scale).ceil() as i32;
            let r = mi.rcMonitor;
            let x = r.left + (r.right - r.left - wpx) / 2;
            SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, r.top, wpx, hpx, SWP_NOACTIVATE)?;
            self.origin = (x, r.top);
            match self.gfx.as_mut() {
                Some(g) => g.resize(wpx as u32, hpx as u32, self.scale)?,
                None => self.gfx = Some(Gfx::new(self.hwnd, wpx as u32, hpx as u32, self.scale)?),
            }
        }
        self.region = (0, 0, 0, 0, 0, 0);
        self.needs_draw = true;
        Ok(())
    }

    /// Updates the exact hit-test window region (`SetWindowRgn`) matching the solid
    /// notch body + top concave ear strip, preventing mouse hover pass-through to
    /// background windows while inside the notch and allowing normal clicks outside.
    fn update_region(&mut self) {
        let p = ui::pill_rect(self.w.value, self.h.value);
        let ear = ui::ear_for(self.e.value, self.mode);
        let s = self.scale;

        let pill_l = (p.x.max(0.0) * s).floor() as i32;
        let pill_r = (p.right().min(ui::WIN_W) * s).ceil() as i32;
        let pill_b = (p.bottom() * s).ceil() as i32;
        let ear_l = ((p.x - ear).max(0.0) * s).floor() as i32;
        let ear_r = ((p.right() + ear).min(ui::WIN_W) * s).ceil() as i32;
        let ear_b = ((ear + 2.0).min(p.bottom()) * s).ceil() as i32;

        let key = (pill_l, pill_r, pill_b, ear_l, ear_r, ear_b);
        if key != self.region {
            self.region = key;
            unsafe {
                let body_rgn = CreateRectRgn(pill_l, 0, pill_r, pill_b);
                let ear_rgn = CreateRectRgn(ear_l, 0, ear_r, ear_b);
                let _ = CombineRgn(Some(body_rgn), Some(body_rgn), Some(ear_rgn), RGN_OR);
                let _ = DeleteObject(ear_rgn.into());
                // The system owns `body_rgn` after SetWindowRgn.
                SetWindowRgn(self.hwnd, Some(body_rgn), false);
            }
        }
    }

    // ---- AiLan Personal Hub window ----------------------------------------------

    pub fn open_hub(&mut self, page: Option<HubPage>) {
        if let Some(p) = page {
            self.hub_ctrl.select_page(p);
        }
        self.hub_open = true;
        let (dip_w, dip_h) = self.hub_ctrl.target_size();
        let wpx = (dip_w as f32 * self.scale).ceil() as i32;
        let hpx = (dip_h as f32 * self.scale).ceil() as i32;

        unsafe {
            let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(mon, &mut mi);
            let r = mi.rcWork;
            let x = r.left + (r.right - r.left - wpx) / 2;
            let y = r.top + ((r.bottom - r.top - hpx) / 2).max(40);
            let _ = SetWindowPos(self.hub_hwnd, None, x, y, wpx, hpx, SWP_SHOWWINDOW | SWP_FRAMECHANGED);

            match self.hub_gfx.as_mut() {
                Some(g) => {
                    let _ = g.resize(wpx as u32, hpx as u32, self.scale);
                }
                None => {
                    self.hub_gfx = Gfx::new(self.hub_hwnd, wpx as u32, hpx as u32, self.scale).ok();
                }
            }
            let _ = ShowWindow(self.hub_hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.hub_hwnd);
            let _ = SetFocus(Some(self.hub_hwnd));
        }
        self.hub_needs_draw = true;
    }

    pub fn resize_hub(&mut self, dip_w: i32, dip_h: i32) {
        let wpx = (dip_w as f32 * self.scale).ceil() as i32;
        let hpx = (dip_h as f32 * self.scale).ceil() as i32;
        unsafe {
            let mut rc = windows::Win32::Foundation::RECT::default();
            let _ = GetWindowRect(self.hub_hwnd, &mut rc);
            let cx = rc.left + (rc.right - rc.left) / 2;
            let cy = rc.top + (rc.bottom - rc.top) / 2;
            let nx = cx - wpx / 2;
            let ny = cy - hpx / 2;
            let _ = SetWindowPos(self.hub_hwnd, None, nx, ny, wpx, hpx, SWP_NOZORDER | SWP_FRAMECHANGED);
            // Clear the newly exposed GDI redirection surface (see WM_ERASEBKGND) so the
            // acrylic backdrop shows through the whole window after growing.
            let _ = RedrawWindow(Some(self.hub_hwnd), None, None, RDW_INVALIDATE | RDW_ERASE | RDW_UPDATENOW);
            if let Some(g) = self.hub_gfx.as_mut() {
                let _ = g.resize(wpx as u32, hpx as u32, self.scale);
            }
        }
        self.hub_needs_draw = true;
    }

    pub fn close_hub(&mut self) {
        if self.hub_open {
            self.hub_open = false;
            self.hub_ctrl.focus = None;
            self.hub_ctrl.on_mouse_leave();
            self.hub_cursor = None;
            unsafe {
                let _ = ShowWindow(self.hub_hwnd, SW_HIDE);
            }
            // Drop the whole D3D/D2D/DComp stack (brushes, gradients, text formats) while
            // hidden; it is rebuilt on the next open.
            self.hub_gfx = None;
            self.sync_hub_timer();
            self.save(true);
        }
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

        let visible = self.state.hover_activities();
        if !visible.is_empty() && !visible.contains(&self.tab) {
            self.tab = visible[0];
        }

        let (tw, th) = if self.expanded {
            ui::EXPANDED
        } else {
            ui::collapsed_size(self.mode)
        };
        self.w.set_target(tw);
        self.h.set_target(th);
        self.e.set_target(if self.expanded { 1.0 } else { 0.0 });
        self.tab_pos.set_target(visible_index(&self.state, self.tab));
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
        self.sync_audio_timer();
        self.sync_raw_mouse();
        self.refresh_world_offsets(now);
        self.sync_hub_timer();
    }

    /// Run `T_HUB` only while the hub shows something that changes by itself.
    fn sync_hub_timer(&mut self) {
        let st = &self.state;
        let on_clock = self.hub_open && self.hub_ctrl.active_page == HubPage::Reminders;
        let want = match (on_clock, self.hub_ctrl.clock_tab) {
            (true, 3) if st.stopwatch.is_running() => 100,
            (true, 2) if st.countdown.is_running() => 250,
            (true, 4) => 1000,
            _ => 0,
        };
        if want == self.hub_timer_ms {
            return;
        }
        self.hub_timer_ms = want;
        unsafe {
            if want == 0 {
                let _ = KillTimer(Some(self.hub_hwnd), T_HUB);
            } else {
                SetTimer(Some(self.hub_hwnd), T_HUB, want, None);
            }
        }
    }

    /// DST-aware offsets for the world clock (cached zone lookups, cheap per call).
    fn refresh_world_offsets(&mut self, now: i64) {
        let offs: Vec<Option<i32>> = self.state.data.world_clocks.iter().map(|w| self.tz.offset_min(&w.tz, now)).collect();
        self.state.world_offsets = offs;
    }

    // ---- idle eyes follow the pointer -------------------------------------------------

    /// Raw mouse input (works while the pointer is anywhere on screen) is only registered
    /// while the idle eyes are visible, so a still or elsewhere-busy island costs nothing.
    fn sync_raw_mouse(&mut self) {
        use windows::Win32::UI::Input::{RegisterRawInputDevices, RAWINPUTDEVICE, RIDEV_INPUTSINK, RIDEV_REMOVE};
        let want = !self.hidden && !self.expanded && self.mode == Collapsed::Minimal && !self.state.settings.reduce_motion;
        if want == self.raw_mouse {
            return;
        }
        let dev = RAWINPUTDEVICE {
            usUsagePage: 0x01, // generic desktop
            usUsage: 0x02,     // mouse
            dwFlags: if want { RIDEV_INPUTSINK } else { RIDEV_REMOVE },
            hwndTarget: if want { self.hwnd } else { HWND::default() },
        };
        let ok = unsafe { RegisterRawInputDevices(&[dev], std::mem::size_of::<RAWINPUTDEVICE>() as u32) }.is_ok();
        if ok {
            self.raw_mouse = want;
        } else {
            log::warn!("raw mouse input (un)registration failed");
        }
        if want {
            self.update_gaze();
        } else {
            self.eye_x.set_target(0.0);
            self.eye_y.set_target(0.0);
        }
    }

    /// Aim the idle eyes at the current cursor position.
    fn update_gaze(&mut self) {
        let mut pt = POINT::default();
        if unsafe { GetCursorPos(&mut pt) }.is_err() {
            return;
        }
        let p = ui::pill_rect(self.w.value, self.h.value);
        let center = (
            self.origin.0 as f32 + (p.x + p.w * 0.5) * self.scale,
            self.origin.1 as f32 + (p.y + p.h * 0.5) * self.scale,
        );
        let (dx, dy) = ui::eye_offset(center, (pt.x as f32, pt.y as f32), self.scale);
        // Quantise so sub-pixel jitter never schedules a frame.
        let q = |v: f32| (v * 4.0).round() / 4.0;
        let (tx, ty) = (q(dx), q(dy));
        if (tx - self.eye_x.target).abs() > 0.01 || (ty - self.eye_y.target).abs() > 0.01 {
            self.eye_x.set_target(tx);
            self.eye_y.set_target(ty);
            self.needs_draw = true;
        }
    }

    fn sync_audio_timer(&mut self) {
        let media_playing = self.state.media.as_ref().is_some_and(|m| m.playing);
        let want_audio_timer = !self.hidden
            && !self.state.settings.reduce_motion
            && (media_playing || self.expanded || self.mode == Collapsed::Media);
        if want_audio_timer != self.audio_timer_on {
            self.audio_timer_on = want_audio_timer;
            unsafe {
                if want_audio_timer {
                    SetTimer(Some(self.hwnd), T_AUDIO, AUDIO_FRAME_MS, None);
                } else {
                    let _ = KillTimer(Some(self.hwnd), T_AUDIO);
                }
            }
        }
    }

    fn step_audio(&mut self, dt: f32, now: i64) {
        let (raw_peak, [raw_l, raw_r]) = self
            .audio
            .as_ref()
            .map(|a| a.sample_peaks())
            .unwrap_or((0.0, [0.0, 0.0]));

        let smtc_playing = self.state.media.as_ref().is_some_and(|m| m.playing);
        let active_sound = smtc_playing || raw_peak > 0.012;
        const GAIN: f32 = 1.35;
        let gain = GAIN;

        let peak = (raw_peak * gain).clamp(0.0, 1.0);
        let left = (raw_l * gain).clamp(0.0, 1.0);
        let right = (raw_r * gain).clamp(0.0, 1.0);

        let targets: [f32; 5] = if !active_sound {
            [0.20; 5]
        } else {
            let t = (now % 60_000) as f32 * 0.001;
            let floor = if peak < 0.04 { 0.32 } else { 0.14 };
            let e_l = left.max(peak * 0.85);
            let e_r = right.max(peak * 0.85);
            let e_m = peak.max((left + right) * 0.5);
            [
                (floor + (e_l * (0.72 + 0.28 * (t * 9.5).sin().abs())) + (1.0 - e_l) * floor * (t * 5.2).sin().abs()).clamp(0.18, 1.0),
                (floor + (e_l * 0.6 + e_m * 0.4) * (0.68 + 0.32 * (t * 12.3 + 1.1).sin().abs())).clamp(0.20, 1.0),
                (floor + e_m * (0.78 + 0.22 * (t * 14.8 + 2.2).sin().abs()) + (1.0 - e_m) * floor * (t * 6.8 + 1.5).sin().abs()).clamp(0.24, 1.0),
                (floor + (e_r * 0.6 + e_m * 0.4) * (0.68 + 0.32 * (t * 11.7 + 3.4).sin().abs())).clamp(0.20, 1.0),
                (floor + (e_r * (0.72 + 0.28 * (t * 8.9 + 4.5).sin().abs())) + (1.0 - e_r) * floor * (t * 5.7 + 2.8).sin().abs()).clamp(0.18, 1.0),
            ]
        };

        let target_peak = if active_sound { peak } else { 0.0 };
        let k_peak = if target_peak > self.audio_peak {
            (dt * 28.0).min(1.0)
        } else {
            (dt * 11.0).min(1.0)
        };
        self.audio_peak += (target_peak - self.audio_peak) * k_peak;

        for (cur, tgt) in self.audio_bars.iter_mut().zip(targets.iter()) {
            let k = if *tgt > *cur {
                (dt * 26.0).min(1.0)
            } else {
                (dt * 13.0).min(1.0)
            };
            *cur += (*tgt - *cur) * k;
        }
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

    fn expand(&mut self) {
        if self.expanded {
            return;
        }
        let now = sys::now_ms();
        self.expanded = true;
        let visible = self.state.hover_activities();
        if !visible.is_empty() {
            let preferred = match self.mode {
                Collapsed::Timer => Some(Tab::Focus),
                Collapsed::Media => Some(Tab::Music),
                Collapsed::Todo => Some(Tab::Todo),
                _ => self.state.primary_activity(now),
            };
            self.tab = preferred
                .filter(|t| visible.contains(t))
                .unwrap_or(visible[0]);
            let idx = visible_index(&self.state, self.tab);
            self.tab_pos.value = idx;
            self.tab_pos.set_target(idx);
        }
        self.state.dismiss_banner();
        self.after_change();
    }

    fn collapse(&mut self) {
        if !self.expanded || self.pinned {
            return;
        }
        self.stop_editing();
        self.expanded = false;
        self.hover = None;
        self.hover_anim.set_target(0.0);
        self.after_change();
    }

    fn set_hidden(&mut self, hidden: bool) {
        if hidden == self.hidden {
            return;
        }
        self.hidden = hidden;
        unsafe {
            let _ = ShowWindow(self.hwnd, if hidden { SW_HIDE } else { SW_SHOWNOACTIVATE });
        }
        if hidden {
            self.expanded = false;
            self.stop_editing();
            self.art = None;
            if let Some(g) = &self.gfx {
                g.trim();
            }
        }
        self.after_change();
    }

    fn select_tab(&mut self, tab: Tab) {
        let visible = self.state.hover_activities();
        if !visible.contains(&tab) {
            return;
        }
        if self.tab != tab {
            self.tab = tab;
            self.state.settings.tab = tab;
            self.state.mark_settings_dirty(sys::now_ms());
            if tab != Tab::Todo {
                self.stop_editing();
            }
            self.after_change();
        }
    }

    fn slide_activity(&mut self, delta: isize) {
        let visible = self.state.hover_activities();
        if visible.len() <= 1 {
            return;
        }
        let cur = visible.iter().position(|&t| t == self.tab).unwrap_or(0) as isize;
        let len = visible.len() as isize;
        let next = ((cur + delta).rem_euclid(len)) as usize;
        self.select_tab(visible[next]);
    }

    // ---- text input ----------------------------------------------------------------

    fn begin_editing(&mut self) {
        if self.input.is_none() {
            self.input = Some(String::new());
        }
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex & !(WS_EX_NOACTIVATE.0 as isize));
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.hwnd));
        }
        self.needs_draw = true;
    }

    fn stop_editing(&mut self) {
        if self.input.take().is_some() {
            self.high_surrogate = None;
            unsafe {
                let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE.0 as isize);
            }
            self.needs_draw = true;
            self.update_notch_cursor();
            if !self.mouse_inside && self.expanded {
                unsafe { SetTimer(Some(self.hwnd), T_COLLAPSE, COLLAPSE_DELAY_MS, None) };
            }
        }
    }

    fn on_char(&mut self, unit: u16) {
        let Some(input) = self.input.as_mut() else { return };
        match unit {
            0x08 => {
                input.pop();
            }
            0x0D => {
                let now = sys::now_ms();
                if let Some((text, due)) = parse_input(input, now, self.state.offset_min) {
                    self.state.data.todos.add(text, due);
                    self.state.mark_data_dirty(now);
                    input.clear();
                    self.after_change();
                }
            }
            0x1B => self.stop_editing(),
            0xD800..=0xDBFF => self.high_surrogate = Some(unit),
            0xDC00..=0xDFFF => {
                if let Some(hi) = self.high_surrogate.take() {
                    if let Some(c) = char::decode_utf16([hi, unit]).next().and_then(|r| r.ok()) {
                        if input.chars().count() < MAX_TEXT + 8 {
                            input.push(c);
                        }
                    }
                }
            }
            u if u >= 0x20 => {
                if let Some(c) = char::from_u32(u as u32) {
                    if input.chars().count() < MAX_TEXT + 8 {
                        input.push(c);
                    }
                }
            }
            _ => {}
        }
        self.needs_draw = true;
    }

    // ---- actions -------------------------------------------------------------------

    fn on_action(&mut self, action: Action) {
        let now = sys::now_ms();
        match action {
            Action::Expand => self.expand(),
            Action::DismissBanner => {
                self.state.dismiss_banner();
                self.expand();
            }
            Action::Tab(t) => self.select_tab(t),
            Action::SlidePrev => self.slide_activity(-1),
            Action::SlideNext => self.slide_activity(1),
            Action::OpenSettings => self.open_hub(Some(HubPage::Settings)),
            Action::MediaPrev => self.media.send(MediaCmd::Previous),
            Action::MediaToggle => self.media.send(MediaCmd::TogglePlayPause),
            Action::MediaNext => self.media.send(MediaCmd::Next),
            Action::TimerToggle => {
                self.state.pomodoro.toggle(now);
            }
            Action::TimerReset => self.state.pomodoro.reset(),
            Action::TimerSkip => self.state.pomodoro.skip(),
            Action::TodoInput => self.begin_editing(),
            Action::TodoToggle(id) => {
                self.state.data.todos.toggle(id, now, self.state.offset_min);
                self.state.mark_data_dirty(now);
            }
            Action::TodoDelete(id) => {
                self.state.data.todos.remove(id);
                self.state.mark_data_dirty(now);
                self.todo_scroll = self.todo_scroll.min(ui::max_todo_scroll(&self.state));
            }
            Action::TodoScroll => {}
            Action::SnoozeBanner(mins) => {
                self.state.snooze_banner(mins, now);
            }
            Action::CompleteBanner => {
                self.state.complete_banner(now);
            }
        }
        if !matches!(action, Action::TodoInput) && self.input.as_deref().is_some_and(str::is_empty) {
            self.stop_editing();
        }
        self.after_change();
    }

    fn on_hub_action(&mut self, action: HubAction) {
        let now = sys::now_ms();
        match action {
            HubAction::Close => {
                self.close_hub();
                return;
            }
            HubAction::Minimize => unsafe {
                let _ = ShowWindow(self.hub_hwnd, SW_MINIMIZE);
                return;
            },
            HubAction::ToggleMode => {
                let (w, h) = self.hub_ctrl.toggle_mode();
                self.resize_hub(w, h);
                return;
            }
            HubAction::SelectPage(page) => {
                self.hub_ctrl.select_page(page);
                self.sync_hub_timer();
                self.hub_needs_draw = true;
                return;
            }
            HubAction::FocusField(f) => {
                self.hub_ctrl.focus = Some(f);
                unsafe {
                    let _ = SetForegroundWindow(self.hub_hwnd);
                    let _ = SetFocus(Some(self.hub_hwnd));
                }
            }
            HubAction::StartAddTodo | HubAction::FocusTodoInput => {
                return self.on_hub_action(HubAction::FocusField(Field::Capture));
            }
            HubAction::CancelAddTodo => {
                self.hub_ctrl.capture.clear();
                self.hub_ctrl.focus = None;
            }
            HubAction::ClearSearch => {
                self.hub_ctrl.search.clear();
                self.hub_ctrl.focus = Some(Field::Search);
            }
            HubAction::SubmitAddTodo | HubAction::SubmitTodoInput => {
                return self.on_hub_action(HubAction::SubmitField(Field::Capture));
            }
            HubAction::SubmitField(Field::Search) => {
                self.hub_ctrl.focus = None;
            }
            HubAction::SubmitField(Field::Copilot) => {
                let plan = ailan_core::command::parse_command(&self.hub_ctrl.copilot);
                if plan.is_actionable() {
                    self.hub_ctrl.copilot.clear();
                    let mut flight = None;
                    if !plan.title.is_empty() {
                        let due = plan.due(now, self.state.offset_min);
                        let text: String = plan.title.chars().take(MAX_TEXT).collect();
                        self.state.data.todos.add_full(text, due, false, plan.repeat, now);
                        self.state.mark_data_dirty(now);
                        flight = Some(if due.is_some() { (ui::G_BELL, FLY_REMIND) } else { (ui::G_CHECKMARK, FLY_TODO) });
                    }
                    if let Some(m) = plan.focus_min {
                        self.state.settings.durations.focus_min = m;
                        let d = self.state.settings.durations;
                        self.state.pomodoro.set_durations(d);
                        self.state.mark_settings_dirty(now);
                    }
                    if plan.start_focus && !self.state.pomodoro.is_running() {
                        self.state.pomodoro.toggle(now);
                        self.tab = Tab::Focus;
                        // Focus wins on the island, so that is what flies in.
                        flight = Some((ui::G_TIMER, FLY_FOCUS));
                    }
                    if let Some((g, tint)) = flight {
                        self.launch_flight(g, tint);
                    }
                }
            }
            HubAction::SetClockTab(tab) => {
                self.hub_ctrl.set_clock_tab(tab as usize);
                self.sync_hub_timer();
                self.hub_needs_draw = true;
                return;
            }
            HubAction::SubmitField(Field::Alarm) => {
                let input = self.hub_ctrl.take_field(Field::Alarm);
                let plan = ailan_core::command::parse_command(&input);
                if let Some((h, m)) = plan.time {
                    use ailan_core::reminder::Repeat;
                    let days = match plan.repeat {
                        Repeat::Daily => ailan_core::clock::EVERY_DAY,
                        Repeat::Weekdays => ailan_core::clock::WEEKDAYS,
                        Repeat::Weekly(mask) => mask,
                        Repeat::None => plan.weekday.map_or(0, |d| 1u8 << d),
                    };
                    let label: String = plan.title.chars().take(MAX_TEXT).collect();
                    self.state.data.alarms.add(h as u8, m as u8, label, days);
                    self.state.mark_data_dirty(now);
                    self.launch_flight(ui::G_BELL, FLY_REMIND);
                } else {
                    // No time recognised: keep the text so it can be fixed.
                    self.hub_ctrl.alarm_input = input;
                    self.hub_ctrl.focus = Some(Field::Alarm);
                }
            }
            HubAction::AlarmToggle(id) => {
                self.state.data.alarms.toggle(id);
                self.state.mark_data_dirty(now);
            }
            HubAction::AlarmShift(id, mins) => {
                self.state.data.alarms.shift(id, mins as i32);
                self.state.mark_data_dirty(now);
            }
            HubAction::AlarmDay(id, d) => {
                self.state.data.alarms.toggle_day(id, d);
                self.state.mark_data_dirty(now);
            }
            HubAction::AlarmDelete(id) => {
                self.state.data.alarms.remove(id);
                self.state.mark_data_dirty(now);
            }
            HubAction::CountdownToggle => {
                let was = self.state.countdown.is_running();
                self.state.countdown.toggle(now);
                if !was && self.state.countdown.is_running() {
                    self.launch_flight(ui::G_TIMER, FLY_REMIND);
                }
            }
            HubAction::CountdownReset => self.state.countdown.reset(),
            HubAction::CountdownSet(sec) => {
                if self.state.countdown.is_active() {
                    self.state.countdown.reset();
                }
                self.state.countdown.set_duration(sec as i64 * 1000);
            }
            HubAction::CountdownAdd(sec) => {
                let ms = sec as i64 * 1000;
                if ms < 0 && !self.state.countdown.is_active() {
                    let d = (self.state.countdown.duration + ms).max(60_000);
                    self.state.countdown.set_duration(d);
                } else if ms < 0 {
                    // Never drop below one second while running/paused.
                    let rem = self.state.countdown.remaining(now);
                    self.state.countdown.add(now, ms.max(1000 - rem));
                } else {
                    self.state.countdown.add(now, ms);
                }
            }
            HubAction::StopwatchToggle => self.state.stopwatch.toggle(now),
            HubAction::StopwatchLap => self.state.stopwatch.lap(now),
            HubAction::StopwatchReset => self.state.stopwatch.reset(),
            HubAction::WorldAdd(i) => {
                if let Some((city, tz)) = ailan_core::clock::CITY_PRESETS.get(i as usize) {
                    if !self.state.data.world_clocks.iter().any(|w| w.tz == *tz) {
                        self.state.data.world_clocks.push(ailan_core::clock::WorldClock { city: (*city).into(), tz: (*tz).into() });
                        self.state.mark_data_dirty(now);
                    }
                }
            }
            HubAction::WorldRemove(i) => {
                if (i as usize) < self.state.data.world_clocks.len() {
                    self.state.data.world_clocks.remove(i as usize);
                    self.state.mark_data_dirty(now);
                }
            }
            HubAction::UseExample(i) => {
                let ex = crate::hub::pages::ai::examples(self.state.lang());
                if let Some(s) = ex.get(i as usize) {
                    self.hub_ctrl.copilot = (*s).to_string();
                }
                self.hub_ctrl.focus = Some(Field::Copilot);
            }
            HubAction::SubmitField(f @ (Field::Capture | Field::Command)) => {
                let input = self.hub_ctrl.take_field(f);
                if f == Field::Command {
                    self.hub_ctrl.focus = None;
                    if let Some(page) = crate::hub::controller::HubController::command_page(&input) {
                        self.hub_ctrl.select_page(page);
                        self.hub_needs_draw = true;
                        return;
                    }
                }
                if let Some((text, due)) = parse_input(&input, now, self.state.offset_min) {
                    self.state.data.todos.add(text, due);
                    self.state.mark_data_dirty(now);
                    if due.is_some() {
                        self.launch_flight(ui::G_BELL, FLY_REMIND);
                    } else {
                        self.launch_flight(ui::G_CHECKMARK, FLY_TODO);
                    }
                }
            }
            HubAction::ToggleTodo(id) | HubAction::TodoToggle(id) => {
                let was_done = self.state.data.todos.items.iter().find(|t| t.id == id).map(|t| t.done);
                self.state.data.todos.toggle(id, now, self.state.offset_min);
                // Repeating items stay pending but advance; still give the tactile pop.
                if was_done == Some(false) {
                    self.hub_ctrl.note_toggled(id, true);
                }
                self.state.mark_data_dirty(now);
            }
            HubAction::FocusOnTodo(_id) => {
                if !self.state.pomodoro.is_running() {
                    self.state.pomodoro.reset();
                    self.state.pomodoro.toggle(now);
                    self.launch_flight(ui::G_TIMER, FLY_FOCUS);
                }
                self.tab = Tab::Focus;
            }
            HubAction::ToggleBucket(i) => {
                if let Some(c) = self.hub_ctrl.collapsed.get_mut(i as usize) {
                    *c = !*c;
                }
                self.hub_needs_draw = true;
                return;
            }
            HubAction::PomoSkip => {
                self.state.pomodoro.skip();
            }
            HubAction::DeleteTodo(id) | HubAction::TodoDelete(id) => {
                self.state.data.todos.remove(id);
                self.state.mark_data_dirty(now);
            }
            HubAction::SnoozeTodo(id, mins) => {
                self.state.data.todos.snooze(id, now, mins);
                self.state.mark_data_dirty(now);
            }
            HubAction::SetTodoFilter(filter) => {
                self.hub_ctrl.set_filter(filter);
                self.hub_needs_draw = true;
                return;
            }
            HubAction::PomoToggle | HubAction::FocusToggle => {
                let was_running = self.state.pomodoro.is_running();
                self.state.pomodoro.toggle(now);
                if self.state.pomodoro.is_running() {
                    self.tab = Tab::Focus;
                    if !was_running {
                        self.launch_flight(ui::G_TIMER, FLY_FOCUS);
                    }
                }
            }
            HubAction::PomoReset | HubAction::FocusReset => {
                self.state.pomodoro.reset();
            }
            HubAction::SetPomoWork(min) => {
                self.state.settings.durations.focus_min = min as u32;
                let d = self.state.settings.durations;
                self.state.pomodoro.set_durations(d);
                self.state.mark_settings_dirty(now);
            }
            HubAction::SetPomoBreak(min) => {
                self.state.settings.durations.short_min = min as u32;
                let d = self.state.settings.durations;
                self.state.pomodoro.set_durations(d);
                self.state.mark_settings_dirty(now);
            }
            HubAction::AdjustPomo(field, delta) => {
                if field.apply(&mut self.state.settings.durations, delta) {
                    let d = self.state.settings.durations;
                    self.state.pomodoro.set_durations(d);
                    self.state.mark_settings_dirty(now);
                }
            }
            HubAction::MediaPlayPause | HubAction::MediaToggle => {
                self.media.send(MediaCmd::TogglePlayPause);
            }
            HubAction::MediaNext => {
                self.media.send(MediaCmd::Next);
            }
            HubAction::MediaPrev => {
                self.media.send(MediaCmd::Previous);
            }
            HubAction::SetLanguage(lang) => {
                self.state.settings.lang = Some(lang);
                self.state.mark_settings_dirty(now);
            }
            HubAction::ToggleAutostart => {
                if let Err(e) = autostart::set_enabled(!autostart::is_enabled()) {
                    log::error!("autostart: {e}");
                }
            }
            HubAction::ToggleReduceMotion => {
                self.state.settings.reduce_motion = !self.state.settings.reduce_motion;
                self.state.mark_settings_dirty(now);
            }
        }
        self.hub_needs_draw = true;
        self.after_change();
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

    fn tray_menu(&mut self) {
        let lang = self.state.lang();
        let items = [
            Some(MenuItem { cmd: MenuCmd::Settings, label: t(lang, Key::OpenSettings), checked: None }),
            None,
            Some(MenuItem { cmd: MenuCmd::Autostart, label: t(lang, Key::StartWithWindows), checked: Some(autostart::is_enabled()) }),
            Some(MenuItem { cmd: MenuCmd::Language, label: t(lang, Key::Language), checked: None }),
            None,
            Some(MenuItem { cmd: MenuCmd::About, label: t(lang, Key::About), checked: None }),
            Some(MenuItem { cmd: MenuCmd::Exit, label: t(lang, Key::Exit), checked: None }),
        ];
        let now = sys::now_ms();
        match self.tray.menu(&items) {
            Some(MenuCmd::Settings) => self.open_hub(None),
            Some(MenuCmd::Autostart) => {
                if let Err(e) = autostart::set_enabled(!autostart::is_enabled()) {
                    log::error!("autostart: {e}");
                }
            }
            Some(MenuCmd::Language) => {
                self.state.settings.lang = Some(if lang == Lang::Vi { Lang::En } else { Lang::Vi });
                self.state.mark_settings_dirty(now);
            }
            Some(MenuCmd::About) => {
                self.state.banner = Some(Banner {
                    icon: BannerIcon::Focus,
                    title: format!("Ailan {}", env!("CARGO_PKG_VERSION")),
                    body: "A living monochrome notch for Windows · native Rust".into(),
                    until: now + 5_000,
                    todo_id: None,
                    alarm_id: None,
                });
            }
            Some(MenuCmd::Exit) => unsafe {
                let _ = DestroyWindow(self.hwnd);
                return;
            },
            None => {}
        }
        self.after_change();
    }

    // ---- frame loop ----------------------------------------------------------------

    pub fn wants_frame(&self) -> bool {
        !self.quit
            && ((!self.hidden && (self.needs_draw || !self.settled()))
                || self.hub_needs_draw
                || self.flight_active()
                || (self.hub_open && self.hub_ctrl.animating()))
    }

    fn settled(&self) -> bool {
        self.w.is_settled()
            && self.h.is_settled()
            && self.e.is_settled()
            && self.hover_anim.is_settled()
            && self.tab_pos.is_settled()
            && self.eye_x.is_settled()
            && self.eye_y.is_settled()
            && self.glow() <= 0.0
    }

    fn flight_active(&self) -> bool {
        self.flight.as_ref().is_some_and(|f| f.is_active())
    }

    /// Landing glow intensity (1 → 0 over 300 ms).
    fn glow(&self) -> f32 {
        self.landed_at.map_or(0.0, |t| (1.0 - t.elapsed().as_secs_f32() / 0.3).max(0.0))
    }

    /// Fly `glyph` from the last pressed Hub control into the island. Without a visible hub,
    /// notch or with reduced motion the state change simply applies immediately.
    fn launch_flight(&mut self, glyph: char, tint: u32) {
        if self.state.settings.reduce_motion || self.hidden || !self.hub_open || self.expanded {
            return;
        }
        let Some((px, py)) = self.hub_ctrl.last_press else { return };
        let mut rc = windows::Win32::Foundation::RECT::default();
        if unsafe { GetWindowRect(self.hub_hwnd, &mut rc) }.is_err() {
            return;
        }
        let start = (rc.left as f32 + px * self.scale, rc.top as f32 + py * self.scale);
        let p = ui::pill_rect(self.w.value, self.h.value);
        let end = (
            self.origin.0 as f32 + (p.x + p.w * 0.5) * self.scale,
            self.origin.1 as f32 + (p.y + p.h * 0.5) * self.scale,
        );
        let scale = self.scale;
        if let Some(f) = self.flight.as_mut() {
            f.launch(start, end, scale, glyph, tint);
        }
    }

    fn on_flight_landed(&mut self) {
        // A small "gulp": brief width/height impulse on the (well-damped) size springs.
        self.w.velocity += 320.0;
        self.h.velocity += 80.0;
        self.landed_at = Some(Instant::now());
        self.after_change();
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
        if !self.hidden && (self.needs_draw || !self.settled()) {
            let now_i = Instant::now();
            let dt = now_i.duration_since(self.last_frame).as_secs_f32();
            self.last_frame = now_i;
            let animating_before = !self.settled();
            let dt = if (animating_before || self.audio_timer_on) && dt < 0.1 {
                dt.max(1.0 / 240.0)
            } else {
                1.0 / 60.0
            };
            self.w.step(dt);
            self.h.step(dt);
            self.e.step(dt);
            self.hover_anim.step(dt);
            self.tab_pos.step(dt);
            self.eye_x.step(dt);
            self.eye_y.step(dt);
            self.step_audio(dt, sys::now_ms());
            let animating = !self.settled();
            if self.glow() <= 0.0 {
                self.landed_at = None;
            }
            self.set_eco(!animating && !self.expanded && !self.audio_timer_on && !self.hub_open);
            self.update_region();
            self.sync_artwork();
            self.render();
            self.needs_draw = false;
            if let Some(s) = self.stats.as_mut() {
                s.frames += 1;
            }
        }
        if self.hub_open && (self.hub_needs_draw || self.hub_ctrl.animating()) {
            self.hub_ctrl.advance();
            self.render_hub();
            self.hub_needs_draw = false;
        }
        let landed = self.flight.as_mut().is_some_and(|f| f.frame());
        if landed {
            self.on_flight_landed();
        }
    }

    fn set_eco(&mut self, on: bool) {
        if self.eco != Some(on) {
            self.eco = Some(on);
            sys::set_eco_qos(on);
        }
    }

    fn sync_artwork(&mut self) {
        let want = self.state.art.clone();
        let same = match (&want, &self.art) {
            (Some(a), Some((b, _))) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.art = None;
        if let (Some(a), Some(g)) = (want, &self.gfx) {
            match g.create_bitmap(a.width, a.height, &a.pixels).and_then(|b| g.create_bitmap_brush(&b)) {
                Ok(brush) => self.art = Some((a, brush)),
                Err(e) => log::warn!("artwork bitmap: {e}"),
            }
        }
    }

    fn render(&mut self) {
        let now = sys::now_ms();
        let view = ui::View {
            now,
            pill: ui::pill_rect(self.w.value, self.h.value),
            expand: self.e.value.clamp(0.0, 1.0),
            mode: self.mode,
            tab: self.tab,
            input: self.input.as_deref(),
            todo_scroll: self.todo_scroll,
            art: self.art.as_ref().map(|(a, b)| (b, (a.width, a.height))),
            hover: self.hover,
            hover_t: self.hover_anim.value.clamp(0.0, 1.0),
            tab_pos: self.tab_pos.value.clamp(0.0, 2.0),
            eye: (self.eye_x.value, self.eye_y.value),
            glow: self.glow(),
            audio_bars: self.audio_bars,
            audio_peak: self.audio_peak,
        };
        let state = &self.state;
        let mut hits = Vec::new();
        let result = match self.gfx.as_mut() {
            Some(g) => g.draw(|cv| hits = ui::draw(cv, state, &view)),
            None => return,
        };
        self.hits = hits;
        if let Err(e) = result {
            log::warn!("render failed ({e}); recreating device");
            self.gfx = None;
            self.art = None;
            if let Err(e) = self.place() {
                log::error!("device recreate: {e}");
            }
        }
    }

    fn render_hub(&mut self) {
        if !self.hub_open {
            return;
        }
        let (dip_w, dip_h) = self.hub_ctrl.target_size();
        let now = sys::now_ms();
        let state = &self.state;
        let ctrl = &mut self.hub_ctrl;
        let result = match self.hub_gfx.as_mut() {
            Some(g) => g.draw(|cv| ctrl.draw(cv, state, now, dip_w as f32, dip_h as f32)),
            None => return,
        };
        if let Err(e) = result {
            log::warn!("hub render failed ({e}); recreating device");
            let wpx = (dip_w as f32 * self.scale).ceil() as u32;
            let hpx = (dip_h as f32 * self.scale).ceil() as u32;
            self.hub_gfx = Gfx::new(self.hub_hwnd, wpx, hpx, self.scale).ok();
            self.hub_needs_draw = true;
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

    fn cursor_icon_notch(&self, x: f32, y: f32) -> ui::CursorIcon {
        match ui::hit_test(&self.hits, x, y) {
            Some(action) => action.cursor_icon(self.input.is_some()),
            None => ui::CursorIcon::Default,
        }
    }

    fn update_notch_cursor(&self) {
        if let Some((x, y)) = self.cursor {
            apply_cursor_icon(self.cursor_icon_notch(x, y));
        } else {
            apply_cursor_icon(ui::CursorIcon::Default);
        }
    }

    fn mouse_pos(&self, lparam: LPARAM) -> (f32, f32) {
        let x = (lparam.0 & 0xFFFF) as i16 as f32 / self.scale;
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32 / self.scale;
        (x, y)
    }

    fn cursor_icon_hub(&self) -> ui::CursorIcon {
        match self.hub_ctrl.hover {
            Some(act) if act.is_text_target() => ui::CursorIcon::Text,
            Some(_) => ui::CursorIcon::Pointer,
            None => ui::CursorIcon::Default,
        }
    }

    pub fn handle_hub(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let hwnd = self.hub_hwnd;
        let (dip_w, dip_h) = self.hub_ctrl.target_size();

        match msg {
            WM_NCACTIVATE => {
                // The hub has no non-client area, but DefWindowProc would still paint the
                // WS_THICKFRAME sizing frame into the GDI surface on (de)activation, which shows
                // through the translucent canvas as a light ring. lParam = -1 skips that repaint
                // while keeping normal activation behaviour.
                return Some(unsafe { DefWindowProcW(hwnd, msg, wparam, LPARAM(-1)) });
            }
            WM_NCPAINT => {
                return Some(LRESULT(0));
            }
            WM_WINDOWPOSCHANGING => {
                // Lock the window to the current layout size (blocks edge drags, Snap, maximize).
                let wp = lparam.0 as *mut WINDOWPOS;
                if !wp.is_null() {
                    unsafe {
                        if (*wp).flags.0 & SWP_NOSIZE.0 == 0 {
                            (*wp).cx = (dip_w as f32 * self.scale).ceil() as i32;
                            (*wp).cy = (dip_h as f32 * self.scale).ceil() as i32;
                        }
                    }
                }
                return Some(LRESULT(0));
            }
            WM_TIMER if wparam.0 == T_HUB => {
                let now = sys::now_ms();
                if self.state.world_offsets.len() != self.state.data.world_clocks.len() || self.hub_ctrl.clock_tab == 4 {
                    self.refresh_world_offsets(now);
                }
                self.hub_needs_draw = true;
                return Some(LRESULT(0));
            }
            WM_ERASEBKGND => {
                // The hub draws through DirectComposition; its GDI redirection surface must be
                // transparent black or DWM treats newly exposed pixels as opaque after a resize.
                unsafe {
                    let mut rc = windows::Win32::Foundation::RECT::default();
                    let _ = GetClientRect(hwnd, &mut rc);
                    FillRect(HDC(wparam.0 as _), &rc, HBRUSH(GetStockObject(BLACK_BRUSH).0));
                }
                return Some(LRESULT(1));
            }
            WM_NCCALCSIZE => {
                // Return 0 for both wParam == 0 and wParam != 0 so client area fills
                // the entire window frame without Windows default non-client borders.
                return Some(LRESULT(0));
            }
            WM_NCHITTEST => {
                let mut pt = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut pt);
                    let _ = ScreenToClient(hwnd, &mut pt);
                }
                let x = pt.x as f32 / self.scale;
                let y = pt.y as f32 / self.scale;

                // No resize borders: the hub has two fixed layouts (Compact / Full) and its
                // swapchain is sized to them; free resizing would expose uncovered bands.
                // Drag on the titlebar band wherever there is no interactive element.
                if self.hub_ctrl.is_caption(x, y) {
                    return Some(LRESULT(HTCAPTION as isize));
                }
                return Some(LRESULT(HTCLIENT as isize));
            }
            WM_SETCURSOR => {
                let hit_test = (lparam.0 & 0xFFFF) as u32;
                if hit_test == HTCLIENT {
                    apply_cursor_icon(self.cursor_icon_hub());
                    return Some(LRESULT(1));
                }
            }
            WM_MOUSEMOVE => {
                let pos = self.mouse_pos(lparam);
                if self.hub_cursor != Some(pos) {
                    self.hub_cursor = Some(pos);
                }
                if !self.hub_mouse_inside {
                    self.hub_mouse_inside = true;
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    unsafe {
                        let _ = TrackMouseEvent(&mut tme);
                    }
                }
                if self.hub_ctrl.on_mouse_move(pos.0, pos.1) {
                    self.hub_needs_draw = true;
                }
                apply_cursor_icon(self.cursor_icon_hub());
            }
            WM_MOUSELEAVE => {
                self.hub_mouse_inside = false;
                self.hub_cursor = None;
                self.hub_ctrl.on_mouse_leave();
                self.hub_needs_draw = true;
                apply_cursor_icon(ui::CursorIcon::Default);
            }
            WM_LBUTTONDOWN => {
                let (x, y) = self.mouse_pos(lparam);
                let had_focus = self.hub_ctrl.focus.is_some();
                match self.hub_ctrl.on_mouse_down(x, y) {
                    Some(act) => self.on_hub_action(act),
                    None if had_focus => self.hub_needs_draw = true,
                    None => {}
                }
                apply_cursor_icon(self.cursor_icon_hub());
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
                if self.hub_ctrl.on_wheel(delta) {
                    self.hub_needs_draw = true;
                }
            }
            WM_KEYDOWN => {
                let vk = wparam.0 as u32;
                let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
                match self.hub_ctrl.on_key_down(vk, ctrl) {
                    KeyOutcome::Action(act) => self.on_hub_action(act),
                    KeyOutcome::Handled => self.hub_needs_draw = true,
                    KeyOutcome::Ignored if vk == 0x1B => {
                        self.close_hub();
                        return Some(LRESULT(0));
                    }
                    KeyOutcome::Ignored => {}
                }
            }
            WM_CHAR => {
                let unit = wparam.0 as u16;
                if let Some(c) = char::from_u32(unit as u32) {
                    if self.hub_ctrl.on_char(c) {
                        self.hub_needs_draw = true;
                    }
                }
            }
            WM_CLOSE => {
                self.close_hub();
            }
            _ => return None,
        }
        Some(LRESULT(0))
    }

    pub fn handle(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let hwnd = self.hwnd;
        match msg {
            WM_INPUT => {
                if self.raw_mouse {
                    self.update_gaze();
                }
                // Let DefWindowProc release the raw-input buffer.
                return None;
            }
            WM_NCHITTEST => {
                // Explicitly claim client hit-test so hover & mouse events never leak through
                // to windows underneath the notch.
                return Some(LRESULT(HTCLIENT as isize));
            }
            WM_APP_EVENT => {
                let now = sys::now_ms();
                let mut changed = false;
                while let Ok(ev) = self.rx.try_recv() {
                    changed |= self.state.apply(ev, now);
                }
                if changed {
                    self.after_change();
                }
            }
            WM_APP_AUDIO_DEVICE => {
                if let Some(a) = self.audio.as_mut() {
                    a.rebind();
                }
            }
            WM_APP_APPBAR => {
                if wparam.0 == ABN_FULLSCREENAPP {
                    self.set_hidden(lparam.0 != 0);
                }
            }
            WM_APP_TRAY => {
                match (lparam.0 & 0xFFFF) as u32 {
                    WM_CONTEXTMENU | WM_RBUTTONUP => self.tray_menu(),
                    WM_LBUTTONUP | 0x400 /* NIN_SELECT */ => {
                        if self.hub_open {
                            self.close_hub();
                        } else {
                            self.open_hub(None);
                        }
                    }
                    _ => {}
                }
            }
            crate::platform::WM_APP_SHOW_HUB => {
                self.open_hub(None);
                return Some(LRESULT(0));
            }
            WM_TIMER => match wparam.0 {
                T_TICK => self.on_tick(),
                T_HOVER => {
                    unsafe {
                        let _ = KillTimer(Some(hwnd), T_HOVER);
                    }
                    if self.mouse_inside {
                        self.expand();
                    }
                }
                T_COLLAPSE => {
                    unsafe {
                        let _ = KillTimer(Some(hwnd), T_COLLAPSE);
                    }
                    if !self.mouse_inside && self.input.is_none() {
                        self.collapse();
                    }
                }
                T_AUDIO => {
                    if !self.hidden {
                        self.needs_draw = true;
                    }
                }
                T_STATS => self.log_stats(),
                _ => {}
            },
            WM_SETCURSOR => {
                let hit_test = (lparam.0 & 0xFFFF) as u32;
                if hit_test == HTCLIENT {
                    let mut pt = POINT::default();
                    unsafe {
                        let _ = GetCursorPos(&mut pt);
                        let _ = ScreenToClient(hwnd, &mut pt);
                    }
                    let x = pt.x as f32 / self.scale;
                    let y = pt.y as f32 / self.scale;
                    apply_cursor_icon(self.cursor_icon_notch(x, y));
                    return Some(LRESULT(1));
                }
            }
            WM_MOUSEMOVE => {
                let pos = self.mouse_pos(lparam);
                if self.cursor != Some(pos) {
                    self.cursor = Some(pos);
                    if self.raw_mouse {
                        self.update_gaze();
                    }
                }
                if !self.mouse_inside {
                    self.mouse_inside = true;
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    unsafe {
                        let _ = TrackMouseEvent(&mut tme);
                        let _ = KillTimer(Some(hwnd), T_COLLAPSE);
                        if !self.expanded {
                            SetTimer(Some(hwnd), T_HOVER, HOVER_INTENT_MS, None);
                        }
                    }
                    self.after_change();
                }
                let (x, y) = pos;
                let hover = ui::hit_test(&self.hits, x, y);
                apply_cursor_icon(match hover {
                    Some(act) => act.cursor_icon(self.input.is_some()),
                    None => ui::CursorIcon::Default,
                });
                if hover != self.hover {
                    self.hover = hover;
                    if matches!(hover, Some(a) if !matches!(a, Action::Expand | Action::TodoScroll)) {
                        self.hover_anim.value = 0.15;
                        self.hover_anim.set_target(1.0);
                    } else if hover.is_some() {
                        self.hover_anim.set_target(0.5);
                    } else {
                        self.hover_anim.set_target(0.0);
                    }
                    if self.state.settings.reduce_motion {
                        self.hover_anim.snap();
                    }
                    self.needs_draw = true;
                }
            }
            WM_MOUSELEAVE => {
                self.mouse_inside = false;
                self.cursor = None;
                self.hover = None;
                self.hover_anim.set_target(0.0);
                unsafe {
                    let _ = KillTimer(Some(hwnd), T_HOVER);
                }
                if self.expanded && self.input.is_none() {
                    unsafe { SetTimer(Some(hwnd), T_COLLAPSE, COLLAPSE_DELAY_MS, None) };
                }
                apply_cursor_icon(ui::CursorIcon::Default);
                self.after_change();
            }
            WM_LBUTTONDOWN => {
                let (x, y) = self.mouse_pos(lparam);
                match ui::hit_test(&self.hits, x, y) {
                    Some(a) => self.on_action(a),
                    None => self.stop_editing(),
                }
                self.update_notch_cursor();
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
                if self.expanded && self.tab == Tab::Todo {
                    let max = ui::max_todo_scroll(&self.state);
                    self.todo_scroll = if delta < 0 { (self.todo_scroll + 1).min(max) } else { self.todo_scroll.saturating_sub(1) };
                    self.needs_draw = true;
                } else if self.expanded {
                    self.slide_activity(if delta < 0 { 1 } else { -1 });
                }
            }
            WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
            WM_CHAR => self.on_char(wparam.0 as u16),
            WM_KILLFOCUS => self.stop_editing(),
            WM_POWERBROADCAST => {
                match wparam.0 {
                    PBT_POWERSETTINGCHANGE => {
                        if self.state.apply(AppEvent::Battery(power::battery()), sys::now_ms()) {
                            self.after_change();
                        }
                    }
                    PBT_APMRESUMEAUTOMATIC => {
                        self.state.offset_min = sys::utc_offset_min();
                        self.armed = None;
                        self.after_change();
                    }
                    _ => {}
                }
                return Some(LRESULT(1));
            }
            WM_TIMECHANGE | WM_SETTINGCHANGE => {
                self.state.offset_min = sys::utc_offset_min();
                self.armed = None;
                self.after_change();
            }
            WM_DPICHANGED | WM_DISPLAYCHANGE => {
                if let Err(e) = self.place() {
                    log::error!("reposition: {e}");
                }
            }
            WM_DESTROY => {
                self.save(true);
                self.quit = true;
                unsafe { PostQuitMessage(0) };
            }
            m if m == self.taskbar_created && m != 0 => self.tray.add(),
            _ => return None,
        }
        Some(LRESULT(0))
    }
}

fn visible_index(st: &AppState, tab: Tab) -> f32 {
    st.hover_activities()
        .iter()
        .position(|&t| t == tab)
        .unwrap_or(0) as f32
}
