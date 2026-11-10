//! The notch window: owns state, routes OS messages, paces animation and
//! arms the single scheduler timer. Also manages the auxiliary Settings &
//! Quick-Add Activity window.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;

use ailan_core::anim::Spring;
use ailan_core::event::Artwork;
use ailan_core::i18n::{t, Key, Lang};
use ailan_core::scheduler::{self, View as SchedView};
use ailan_core::settings::{NotchMode, Settings, Tab, WaveStyle};
use ailan_core::state::{Banner, BannerIcon, Collapsed};
use ailan_core::storage::{self, Format};
use ailan_core::todo::{parse_input, MAX_TEXT};
use ailan_core::{AppEvent, AppState};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush1;
use windows::Win32::Graphics::Gdi::{
    CreateRectRgn, GetMonitorInfoW, MonitorFromPoint, SetWindowRgn, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::gfx::Gfx;
use crate::modals::{self, ModalAction, ModalHit, ModalKind, ModalView, SettingsPage};
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

const HOVER_INTENT_MS: u32 = 110;
const COLLAPSE_DELAY_MS: u32 = 360;
const AUDIO_FRAME_MS: u32 = 30;
const PBT_POWERSETTINGCHANGE: usize = 0x8013;
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;

#[derive(Default)]
struct Stats {
    frames: u64,
    wakeups: u64,
}

pub struct App {
    hwnd: HWND,
    modal_hwnd: HWND,
    gfx: Option<Gfx>,
    modal_gfx: Option<Gfx>,
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
    hidden: bool,
    art: Option<(Arc<Artwork>, ID2D1BitmapBrush1)>,
    scale: f32,
    region: (i32, i32, i32, i32),
    needs_draw: bool,
    last_frame: Instant,
    armed: Option<i64>,
    eco: Option<bool>,
    stats: Option<Stats>,
    taskbar_created: u32,
    pinned: bool,

    // Auxiliary Modal / Settings window state
    modal_kind: Option<ModalKind>,
    modal_page: SettingsPage,
    modal_hits: Vec<ModalHit>,
    modal_hover: Option<ModalAction>,
    modal_cursor: Option<(f32, f32)>,
    modal_mouse_inside: bool,
    modal_todo_input: Option<String>,
    modal_needs_draw: bool,

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
    /// Start with Settings or Add-Activity window open (`--settings` / `--add`).
    pub initial_modal: Option<ModalKind>,
}

impl App {
    pub fn new(hwnd: HWND, modal_hwnd: HWND, init: Init) -> windows::core::Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        let sink = EventSink::new(tx, hwnd);
        let state = AppState::new(init.settings, init.data, sys::utc_offset_min());
        let mode = state.collapsed(sys::now_ms());
        let (cw, ch) = ui::collapsed_size(&state, mode);

        let media = media::spawn(sink.clone());
        let audio = Audio::new(sink.clone()).map_err(|e| log::warn!("audio: {e}")).ok();
        brightness::spawn(sink.clone());
        network::spawn(sink.clone());

        let tab = state.settings.tab;
        let init_pos = priority_index(&state, tab);
        let mut app = Self {
            hwnd,
            modal_hwnd,
            gfx: None,
            modal_gfx: None,
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
            w: Spring::new(cw).with(380.0, 32.0),
            h: Spring::new(ch).with(380.0, 32.0),
            e: Spring::new(0.0).with(340.0, 30.0),
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
            hidden: false,
            art: None,
            scale: 1.0,
            region: (0, 0, 0, 0),
            needs_draw: true,
            last_frame: Instant::now(),
            armed: None,
            eco: None,
            stats: init.stats.then(Stats::default),
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
            pinned: init.pin_tab.is_some(),
            modal_kind: None,
            modal_page: SettingsPage::default(),
            modal_hits: Vec::new(),
            modal_hover: None,
            modal_cursor: None,
            modal_mouse_inside: false,
            modal_todo_input: None,
            modal_needs_draw: false,
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
            app.tab_pos.value = priority_index(&app.state, tab);
        }
        if let Some(kind) = init.initial_modal {
            app.open_modal(kind);
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
            match self.gfx.as_mut() {
                Some(g) => g.resize(wpx as u32, hpx as u32, self.scale)?,
                None => self.gfx = Some(Gfx::new(self.hwnd, wpx as u32, hpx as u32, self.scale)?),
            }
        }
        self.region = (0, 0, 0, 0);
        self.needs_draw = true;
        Ok(())
    }

    fn update_region(&mut self) {
        let p = ui::pill_rect(self.w.value, self.h.value);
        let ear = ui::ear_for(self.e.value, self.mode);
        let s = self.scale;
        let r = (
            ((p.x - ear).max(0.0) * s).floor() as i32,
            0,
            ((p.right() + ear).min(ui::WIN_W) * s).ceil() as i32,
            (p.bottom() * s).ceil() as i32,
        );
        if r != self.region {
            self.region = r;
            unsafe {
                let rgn = CreateRectRgn(r.0, r.1, r.2, r.3);
                // The system owns the region after this call.
                SetWindowRgn(self.hwnd, Some(rgn), false);
            }
        }
    }

    // ---- auxiliary modal / settings window ---------------------------------------

    pub fn open_modal(&mut self, kind: ModalKind) {
        let same_kind = self.modal_kind == Some(kind);
        self.modal_kind = Some(kind);
        if kind != ModalKind::AddActivity {
            self.modal_todo_input = None;
        }
        let (dip_w, dip_h) = modals::window_size(kind);
        let wpx = (dip_w * self.scale).ceil() as i32;
        let hpx = (dip_h * self.scale).ceil() as i32;

        unsafe {
            let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(mon, &mut mi);
            let r = mi.rcMonitor;
            let x = r.left + (r.right - r.left - wpx) / 2;
            let y = match kind {
                ModalKind::AddActivity => r.top + ((ui::WIN_H + 10.0) * self.scale) as i32,
                ModalKind::Settings => r.top + ((r.bottom - r.top - hpx) / 2).max(((ui::WIN_H + 16.0) * self.scale) as i32),
            };
            let flags = if same_kind { SWP_NOMOVE } else { SWP_SHOWWINDOW };
            let _ = SetWindowPos(self.modal_hwnd, Some(HWND_TOPMOST), x, y, wpx, hpx, flags);
            match self.modal_gfx.as_mut() {
                Some(g) => {
                    let _ = g.resize(wpx as u32, hpx as u32, self.scale);
                }
                None => {
                    self.modal_gfx = Gfx::new(self.modal_hwnd, wpx as u32, hpx as u32, self.scale).ok();
                }
            }
            let _ = ShowWindow(self.modal_hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.modal_hwnd);
            let _ = SetFocus(Some(self.modal_hwnd));
        }
        self.modal_needs_draw = true;
    }

    pub fn close_modal(&mut self) {
        if self.modal_kind.take().is_some() {
            self.modal_todo_input = None;
            self.modal_hover = None;
            self.modal_cursor = None;
            unsafe {
                let _ = ShowWindow(self.modal_hwnd, SW_HIDE);
            }
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
        self.mode = self.state.collapsed(now);
        let (mut tw, mut th) = if self.expanded {
            ui::EXPANDED
        } else {
            ui::collapsed_size(&self.state, self.mode)
        };
        if !self.expanded && self.mouse_inside {
            tw += 8.0;
            th += 2.0;
        }
        self.w.set_target(tw);
        self.h.set_target(th);
        self.e.set_target(if self.expanded { 1.0 } else { 0.0 });
        self.tab_pos.set_target(priority_index(&self.state, self.tab));
        if self.state.settings.reduce_motion {
            self.w.snap();
            self.h.snap();
            self.e.snap();
            self.hover_anim.snap();
            self.tab_pos.snap();
        }
        self.needs_draw = true;
        if self.modal_kind.is_some() {
            self.modal_needs_draw = true;
        }
        self.arm_timer(now);
        self.sync_audio_timer();
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
        let mcfg = &self.state.settings.music_anim;
        let gain = mcfg.gain();

        let peak = (raw_peak * gain).clamp(0.0, 1.0);
        let left = (raw_l * gain).clamp(0.0, 1.0);
        let right = (raw_r * gain).clamp(0.0, 1.0);

        let targets: [f32; 5] = if !active_sound {
            [0.20; 5]
        } else {
            let t = (now % 60_000) as f32 * 0.001;
            match mcfg.style {
                WaveStyle::AudioReactive => {
                    // Combine real hardware stereo/master peaks with 5 harmonic band modulators.
                    // If audio endpoint is momentarily quiet while SMTC is playing, keep a subtle floor wave.
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
                }
                WaveStyle::SmoothWave => {
                    // Flowing sinusoidal wave whose height scales smoothly with system sound energy.
                    let amp = (0.38 + 0.62 * peak).clamp(0.35, 1.0);
                    let mut out = [0.22; 5];
                    for (i, b) in out.iter_mut().enumerate() {
                        let phase = t * 6.4 + i as f32 * 0.95;
                        let w = 0.5 + 0.5 * phase.sin();
                        *b = (0.18 + amp * (0.25 + 0.75 * w)).clamp(0.18, 1.0);
                    }
                    out
                }
                WaveStyle::BouncePulse => {
                    // Symmetric center-out bass pulse driven by peak & stereo punch.
                    let punch = peak.max(0.28 * (t * 7.5).sin().abs());
                    let side = ((left + right) * 0.5).max(punch * 0.7);
                    [
                        (0.16 + 0.58 * side).clamp(0.16, 1.0),
                        (0.20 + 0.78 * ((punch + side) * 0.5)).clamp(0.20, 1.0),
                        (0.24 + 0.76 * punch).clamp(0.24, 1.0),
                        (0.20 + 0.78 * ((punch + side) * 0.5)).clamp(0.20, 1.0),
                        (0.16 + 0.58 * side).clamp(0.16, 1.0),
                    ]
                }
            }
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
        // Show the primary running activity when hovering, or highest-priority configured activity.
        self.tab = match self.mode {
            Collapsed::Timer => Tab::Focus,
            Collapsed::Media => Tab::Music,
            Collapsed::Todo => Tab::Todo,
            _ => self
                .state
                .primary_activity(now)
                .unwrap_or_else(|| self.state.settings.priority.normalized_order()[0]),
        };
        let idx = priority_index(&self.state, self.tab);
        self.tab_pos.value = idx;
        self.tab_pos.set_target(idx);
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
        let order = self.state.settings.priority.normalized_order();
        let cur = order.iter().position(|&t| t == self.tab).unwrap_or(0) as isize;
        let len = order.len() as isize;
        let next = ((cur + delta).rem_euclid(len)) as usize;
        self.select_tab(order[next]);
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
                    self.state.settings.todo_active = true;
                    self.state.mark_data_dirty(now);
                    self.state.mark_settings_dirty(now);
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
            Action::OpenAddModal => self.open_modal(ModalKind::AddActivity),
            Action::OpenSettings => self.open_modal(ModalKind::Settings),
            Action::MediaPrev => self.media.send(MediaCmd::Previous),
            Action::MediaToggle => {
                self.state.settings.media_active = true;
                self.media.send(MediaCmd::TogglePlayPause);
            }
            Action::MediaNext => self.media.send(MediaCmd::Next),
            Action::TimerToggle => {
                self.state.pomodoro.toggle(now);
                if self.state.pomodoro.is_running() {
                    self.state.note_focus_started(now);
                }
            }
            Action::TimerReset => self.state.pomodoro.reset(),
            Action::TimerSkip => self.state.pomodoro.skip(),
            Action::TodoInput => self.begin_editing(),
            Action::TodoToggle(id) => {
                self.state.data.todos.toggle(id);
                self.state.mark_data_dirty(now);
            }
            Action::TodoDelete(id) => {
                self.state.data.todos.remove(id);
                self.state.mark_data_dirty(now);
                self.todo_scroll = self.todo_scroll.min(ui::max_todo_scroll(&self.state));
            }
            Action::TodoScroll => {}
        }
        if !matches!(action, Action::TodoInput) && self.input.as_deref().is_some_and(str::is_empty) {
            self.stop_editing();
        }
        self.after_change();
    }

    fn on_modal_action(&mut self, action: ModalAction) {
        let now = sys::now_ms();
        match action {
            ModalAction::Close => {
                self.close_modal();
                return;
            }
            ModalAction::DragHeader => unsafe {
                let _ = ReleaseCapture();
                SendMessageW(self.modal_hwnd, WM_NCLBUTTONDOWN, Some(WPARAM(HTCAPTION as usize)), Some(LPARAM(0)));
                return;
            },
            ModalAction::SwitchToSettings => {
                self.open_modal(ModalKind::Settings);
                return;
            }
            ModalAction::SwitchToAdd => {
                self.open_modal(ModalKind::AddActivity);
                return;
            }
            ModalAction::SelectPage(page) => {
                self.modal_page = page;
                self.modal_needs_draw = true;
                return;
            }
            ModalAction::QuickFocus(mins) => {
                self.state.settings.durations.focus_min = mins;
                self.state.pomodoro.durations.focus_min = mins;
                self.state.pomodoro.reset();
                self.state.pomodoro.start(now);
                self.state.note_focus_started(now);
                self.tab = Tab::Focus;
                self.state.settings.tab = Tab::Focus;
                self.state.mark_settings_dirty(now);
                self.close_modal();
            }
            ModalAction::QuickMedia => {
                self.state.settings.media_active = true;
                self.tab = Tab::Music;
                self.state.settings.tab = Tab::Music;
                if self.state.media.as_ref().is_some_and(|m| !m.playing) {
                    self.media.send(MediaCmd::TogglePlayPause);
                }
                self.state.mark_settings_dirty(now);
                self.close_modal();
            }
            ModalAction::QuickTodoTogglePin | ModalAction::TogglePinTodo => {
                self.state.settings.todo_active = !self.state.settings.todo_active;
                if self.state.settings.todo_active {
                    self.tab = Tab::Todo;
                }
                self.state.mark_settings_dirty(now);
            }
            ModalAction::FocusTodoInput => {
                if self.modal_todo_input.is_none() {
                    self.modal_todo_input = Some(String::new());
                }
                unsafe {
                    let _ = SetForegroundWindow(self.modal_hwnd);
                    let _ = SetFocus(Some(self.modal_hwnd));
                }
            }
            ModalAction::SubmitTodoInput => {
                if let Some(input) = self.modal_todo_input.as_mut() {
                    if let Some((text, due)) = parse_input(input, now, self.state.offset_min) {
                        self.state.data.todos.add(text, due);
                        self.state.settings.todo_active = true;
                        self.tab = Tab::Todo;
                        self.state.mark_data_dirty(now);
                        self.state.mark_settings_dirty(now);
                        input.clear();
                    }
                }
            }
            ModalAction::SetLang(lang) => {
                self.state.settings.lang = Some(lang);
                self.state.mark_settings_dirty(now);
            }
            ModalAction::SetMode(mode) => {
                self.state.settings.mode = mode;
                self.state.settings.minimal = mode == NotchMode::MinimalOnly;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleAutostart => {
                if let Err(e) = autostart::set_enabled(!autostart::is_enabled()) {
                    log::error!("autostart: {e}");
                }
            }
            ModalAction::ToggleSmartPeek => {
                self.state.settings.priority.smart_minute_peek = !self.state.settings.priority.smart_minute_peek;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::PriorityUp(tab) => {
                self.state.settings.priority.move_up(tab);
                self.state.mark_settings_dirty(now);
            }
            ModalAction::PriorityDown(tab) => {
                self.state.settings.priority.move_down(tab);
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleMinimalHide => {
                self.state.settings.minimal_ui.hide_when_idle = !self.state.settings.minimal_ui.hide_when_idle;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleMinimalDot => {
                self.state.settings.minimal_ui.show_dot = !self.state.settings.minimal_ui.show_dot;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleMinimalClock => {
                self.state.settings.minimal_ui.show_clock = !self.state.settings.minimal_ui.show_clock;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleMinimalDate => {
                self.state.settings.minimal_ui.show_date = !self.state.settings.minimal_ui.show_date;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleMinimalBattery => {
                self.state.settings.minimal_ui.show_battery = !self.state.settings.minimal_ui.show_battery;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleFocusIcon => {
                self.state.settings.collapsed_ui.focus_show_icon = !self.state.settings.collapsed_ui.focus_show_icon;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleFocusLabel => {
                self.state.settings.collapsed_ui.focus_show_label = !self.state.settings.collapsed_ui.focus_show_label;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleFocusTime => {
                self.state.settings.collapsed_ui.focus_show_time = !self.state.settings.collapsed_ui.focus_show_time;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleMediaArt => {
                self.state.settings.collapsed_ui.media_show_art = !self.state.settings.collapsed_ui.media_show_art;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleMediaTitle => {
                self.state.settings.collapsed_ui.media_show_title = !self.state.settings.collapsed_ui.media_show_title;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleMediaEq => {
                self.state.settings.collapsed_ui.media_show_eq = !self.state.settings.collapsed_ui.media_show_eq;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleTodoIcon => {
                self.state.settings.collapsed_ui.todo_show_icon = !self.state.settings.collapsed_ui.todo_show_icon;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleTodoText => {
                self.state.settings.collapsed_ui.todo_show_text = !self.state.settings.collapsed_ui.todo_show_text;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleTodoCount => {
                self.state.settings.collapsed_ui.todo_show_count = !self.state.settings.collapsed_ui.todo_show_count;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleHoverClock => {
                self.state.settings.hover_ui.show_clock = !self.state.settings.hover_ui.show_clock;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleHoverDate => {
                self.state.settings.hover_ui.show_date = !self.state.settings.hover_ui.show_date;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleHoverBattery => {
                self.state.settings.hover_ui.show_battery = !self.state.settings.hover_ui.show_battery;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleHoverWifi => {
                self.state.settings.hover_ui.show_wifi = !self.state.settings.hover_ui.show_wifi;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleHoverHeadphones => {
                self.state.settings.hover_ui.show_headphones = !self.state.settings.hover_ui.show_headphones;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleHoverSlide => {
                self.state.settings.hover_ui.show_slide_controls = !self.state.settings.hover_ui.show_slide_controls;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleHoverAddBtn => {
                self.state.settings.hover_ui.show_add_button = !self.state.settings.hover_ui.show_add_button;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::SetWaveStyle(style) => {
                self.state.settings.music_anim.style = style;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::SetWaveColor(color) => {
                self.state.settings.music_anim.color = color;
                self.state.mark_settings_dirty(now);
            }
            ModalAction::SetWaveSensitivity(lvl) => {
                self.state.settings.music_anim.sensitivity = lvl.clamp(1, 4);
                self.state.mark_settings_dirty(now);
            }
            ModalAction::ToggleArtPulse => {
                self.state.settings.music_anim.art_pulse = !self.state.settings.music_anim.art_pulse;
                self.state.mark_settings_dirty(now);
            }
        }
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
            let is_reminder = n.title == t(lang, Key::Reminder);
            if is_reminder || self.hidden {
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
            Some(MenuItem { cmd: MenuCmd::AddActivity, label: t(lang, Key::AddActivity), checked: None }),
            Some(MenuItem { cmd: MenuCmd::Settings, label: t(lang, Key::OpenSettings), checked: None }),
            None,
            Some(MenuItem { cmd: MenuCmd::Autostart, label: t(lang, Key::StartWithWindows), checked: Some(autostart::is_enabled()) }),
            Some(MenuItem { cmd: MenuCmd::Minimal, label: t(lang, Key::MinimalNotch), checked: Some(self.state.settings.minimal) }),
            Some(MenuItem { cmd: MenuCmd::Language, label: t(lang, Key::Language), checked: None }),
            None,
            Some(MenuItem { cmd: MenuCmd::About, label: t(lang, Key::About), checked: None }),
            Some(MenuItem { cmd: MenuCmd::Exit, label: t(lang, Key::Exit), checked: None }),
        ];
        let now = sys::now_ms();
        match self.tray.menu(&items) {
            Some(MenuCmd::AddActivity) => self.open_modal(ModalKind::AddActivity),
            Some(MenuCmd::Settings) => self.open_modal(ModalKind::Settings),
            Some(MenuCmd::Autostart) => {
                if let Err(e) = autostart::set_enabled(!autostart::is_enabled()) {
                    log::error!("autostart: {e}");
                }
            }
            Some(MenuCmd::Minimal) => {
                self.state.settings.minimal = !self.state.settings.minimal;
                self.state.settings.mode = if self.state.settings.minimal {
                    NotchMode::MinimalOnly
                } else {
                    NotchMode::DynamicWhenActive
                };
                self.state.mark_settings_dirty(now);
            }
            Some(MenuCmd::Language) => {
                self.state.settings.lang = Some(if lang == Lang::Vi { Lang::En } else { Lang::Vi });
                self.state.mark_settings_dirty(now);
            }
            Some(MenuCmd::About) => {
                self.state.banner = Some(Banner {
                    icon: BannerIcon::Focus,
                    title: format!("Ailan {}", env!("CARGO_PKG_VERSION")),
                    body: "A living notch for Windows · native Rust".into(),
                    until: now + 5_000,
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
        !self.quit && (!self.hidden && (self.needs_draw || !self.settled()) || self.modal_needs_draw)
    }

    fn settled(&self) -> bool {
        self.w.is_settled()
            && self.h.is_settled()
            && self.e.is_settled()
            && self.hover_anim.is_settled()
            && self.tab_pos.is_settled()
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
            self.step_audio(dt, sys::now_ms());
            let animating = !self.settled();
            self.set_eco(!animating && !self.expanded && !self.audio_timer_on && self.modal_kind.is_none());
            self.update_region();
            self.sync_artwork();
            self.render();
            self.needs_draw = false;
            if let Some(s) = self.stats.as_mut() {
                s.frames += 1;
            }
        }
        if self.modal_needs_draw {
            self.render_modal();
            self.modal_needs_draw = false;
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
            cursor: self.cursor,
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

    fn render_modal(&mut self) {
        let Some(kind) = self.modal_kind else { return };
        let view = ModalView {
            kind,
            page: self.modal_page,
            cursor: self.modal_cursor,
            hover: self.modal_hover,
            todo_input: self.modal_todo_input.as_deref(),
        };
        let state = &self.state;
        let mut hits = Vec::new();
        if let Some(g) = self.modal_gfx.as_mut() {
            if g.draw(|cv| hits = modals::draw_modal(cv, state, &view)).is_ok() {
                self.modal_hits = hits;
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
        let x = (lparam.0 & 0xFFFF) as i16 as f32 / self.scale;
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32 / self.scale;
        (x, y)
    }

    pub fn handle_modal(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let hwnd = self.modal_hwnd;
        match msg {
            WM_MOUSEMOVE => {
                let pos = self.mouse_pos(lparam);
                if self.modal_cursor != Some(pos) {
                    self.modal_cursor = Some(pos);
                    self.modal_needs_draw = true;
                }
                if !self.modal_mouse_inside {
                    self.modal_mouse_inside = true;
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
                let hover = modals::hit_test_modal(&self.modal_hits, pos.0, pos.1);
                if hover != self.modal_hover {
                    self.modal_hover = hover;
                    self.modal_needs_draw = true;
                }
            }
            WM_MOUSELEAVE => {
                self.modal_mouse_inside = false;
                self.modal_cursor = None;
                self.modal_hover = None;
                self.modal_needs_draw = true;
            }
            WM_LBUTTONDOWN => {
                let (x, y) = self.mouse_pos(lparam);
                if let Some(act) = modals::hit_test_modal(&self.modal_hits, x, y) {
                    self.on_modal_action(act);
                } else {
                    self.modal_todo_input = None;
                    self.modal_needs_draw = true;
                }
            }
            WM_KEYDOWN => {
                if wparam.0 == 0x1B {
                    self.close_modal();
                }
            }
            WM_CHAR => {
                let unit = wparam.0 as u16;
                if unit == 0x1B {
                    self.close_modal();
                    return Some(LRESULT(0));
                }
                if let Some(input) = self.modal_todo_input.as_mut() {
                    match unit {
                        0x08 => {
                            input.pop();
                        }
                        0x0D => {
                            self.on_modal_action(ModalAction::SubmitTodoInput);
                            return Some(LRESULT(0));
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
                    self.modal_needs_draw = true;
                }
            }
            WM_CLOSE => {
                self.close_modal();
            }
            _ => return None,
        }
        Some(LRESULT(0))
    }

    pub fn handle(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let hwnd = self.hwnd;
        match msg {
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
                        if self.hidden {
                            self.set_hidden(false);
                        }
                        if self.expanded {
                            self.collapse();
                        } else {
                            self.expand();
                            unsafe { SetTimer(Some(hwnd), T_COLLAPSE, 4_000, None) };
                        }
                    }
                    _ => {}
                }
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
            WM_MOUSEMOVE => {
                let pos = self.mouse_pos(lparam);
                if self.cursor != Some(pos) {
                    self.cursor = Some(pos);
                    self.needs_draw = true;
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
                self.after_change();
            }
            WM_LBUTTONDOWN => {
                let (x, y) = self.mouse_pos(lparam);
                match ui::hit_test(&self.hits, x, y) {
                    Some(a) => self.on_action(a),
                    None => self.stop_editing(),
                }
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

fn priority_index(st: &AppState, tab: Tab) -> f32 {
    st.settings
        .priority
        .normalized_order()
        .iter()
        .position(|&t| t == tab)
        .unwrap_or(0) as f32
}
