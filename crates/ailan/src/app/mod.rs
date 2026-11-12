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
use ailan_core::state::Collapsed;
use ailan_core::storage::{self, Format};
use ailan_core::{AppEvent, AppState};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, POINT};
use windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush1;
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, GetMonitorInfoW, MonitorFromPoint,
    SetWindowRgn, MONITORINFO, MONITOR_DEFAULTTOPRIMARY, RGN_OR,
};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::*;

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

}

fn visible_index(st: &AppState, tab: Tab) -> f32 {
    st.hover_activities()
        .iter()
        .position(|&t| t == tab)
        .unwrap_or(0) as f32
}
