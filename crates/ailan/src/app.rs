//! The notch window: owns state, routes OS messages, paces animation and
//! arms the single scheduler timer.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;

use ailan_core::anim::Spring;
use ailan_core::event::Artwork;
use ailan_core::i18n::{t, Key, Lang};
use ailan_core::scheduler::{self, View as SchedView};
use ailan_core::settings::{Settings, Tab};
use ailan_core::state::{BannerIcon, Banner, Collapsed};
use ailan_core::storage::{self, Format};
use ailan_core::todo::{parse_input, MAX_TEXT};
use ailan_core::{AppEvent, AppState};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Direct2D::ID2D1BitmapBrush1;
use windows::Win32::Graphics::Gdi::{
    CreateRectRgn, SetWindowRgn, GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::gfx::Gfx;
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

const HOVER_INTENT_MS: u32 = 160;
const COLLAPSE_DELAY_MS: u32 = 380;
const PBT_POWERSETTINGCHANGE: usize = 0x8013;
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;

#[derive(Default)]
struct Stats {
    frames: u64,
    wakeups: u64,
}

pub struct App {
    hwnd: HWND,
    gfx: Option<Gfx>,
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
    mode: Collapsed,
    tab: Tab,
    input: Option<String>,
    high_surrogate: Option<u16>,
    todo_scroll: usize,
    hits: Vec<Hit>,
    hover: Option<Action>,
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
}

impl App {
    pub fn new(hwnd: HWND, init: Init) -> windows::core::Result<Self> {
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
        let mut app = Self {
            hwnd,
            gfx: None,
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
            w: Spring::new(cw),
            h: Spring::new(ch),
            e: Spring::new(0.0).with(300.0, 30.0),
            mode,
            tab,
            input: None,
            high_surrogate: None,
            todo_scroll: 0,
            hits: Vec::new(),
            hover: None,
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
        let s = self.scale;
        let r = (
            (p.x * s).floor() as i32,
            0,
            (p.right() * s).ceil() as i32,
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

    // ---- state transitions -------------------------------------------------------

    fn sched_view(&self) -> SchedView {
        SchedView { expanded: self.expanded, tab: self.tab, hidden: self.hidden }
    }

    /// Recompute targets and timers after anything changed.
    fn after_change(&mut self) {
        let now = sys::now_ms();
        self.mode = self.state.collapsed(now);
        let (tw, th) = if self.expanded { ui::EXPANDED } else { ui::collapsed_size(self.mode) };
        self.w.set_target(tw);
        self.h.set_target(th);
        self.e.set_target(if self.expanded { 1.0 } else { 0.0 });
        if self.state.settings.reduce_motion {
            self.w.snap();
            self.h.snap();
            self.e.snap();
        }
        self.needs_draw = true;
        self.arm_timer(now);
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
                    // Let Windows batch our wakeup with others when precision doesn't matter.
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
        self.expanded = true;
        self.tab = match self.mode {
            Collapsed::Timer => Tab::Focus,
            Collapsed::Media => Tab::Music,
            _ => self.state.settings.tab,
        };
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
            Action::MediaPrev => self.media.send(MediaCmd::Previous),
            Action::MediaToggle => self.media.send(MediaCmd::TogglePlayPause),
            Action::MediaNext => self.media.send(MediaCmd::Next),
            Action::TimerToggle => self.state.pomodoro.toggle(now),
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
            Some(MenuItem { cmd: MenuCmd::Autostart, label: t(lang, Key::StartWithWindows), checked: Some(autostart::is_enabled()) }),
            Some(MenuItem { cmd: MenuCmd::Minimal, label: t(lang, Key::MinimalNotch), checked: Some(self.state.settings.minimal) }),
            Some(MenuItem { cmd: MenuCmd::Language, label: t(lang, Key::Language), checked: None }),
            None,
            Some(MenuItem { cmd: MenuCmd::About, label: t(lang, Key::About), checked: None }),
            Some(MenuItem { cmd: MenuCmd::Exit, label: t(lang, Key::Exit), checked: None }),
        ];
        let now = sys::now_ms();
        match self.tray.menu(&items) {
            Some(MenuCmd::Autostart) => {
                if let Err(e) = autostart::set_enabled(!autostart::is_enabled()) {
                    log::error!("autostart: {e}");
                }
            }
            Some(MenuCmd::Minimal) => {
                self.state.settings.minimal = !self.state.settings.minimal;
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
        !self.quit && !self.hidden && (self.needs_draw || !self.settled())
    }

    fn settled(&self) -> bool {
        self.w.is_settled() && self.h.is_settled() && self.e.is_settled()
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
        let now_i = Instant::now();
        let dt = now_i.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now_i;
        let animating_before = !self.settled();
        // A fresh animation starts from a small step, not from the idle gap.
        let dt = if animating_before && dt < 0.1 { dt } else { 1.0 / 60.0 };
        self.w.step(dt);
        self.h.step(dt);
        self.e.step(dt);
        let animating = !self.settled();
        self.set_eco(!animating && !self.expanded);
        self.update_region();
        self.sync_artwork();
        self.render();
        self.needs_draw = false;
        if let Some(s) = self.stats.as_mut() {
            s.frames += 1;
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

    // ---- window procedure ------------------------------------------------------------

    fn mouse_pos(&self, lparam: LPARAM) -> (f32, f32) {
        let x = (lparam.0 & 0xFFFF) as i16 as f32 / self.scale;
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32 / self.scale;
        (x, y)
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
                // NOTIFYICON_VERSION_4: event in LOWORD(lParam).
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
                T_STATS => self.log_stats(),
                _ => {}
            },
            WM_MOUSEMOVE => {
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
                }
                if self.expanded {
                    let (x, y) = self.mouse_pos(lparam);
                    let hover = ui::hit_test(&self.hits, x, y);
                    if hover != self.hover {
                        self.hover = hover;
                        self.needs_draw = true;
                    }
                }
            }
            WM_MOUSELEAVE => {
                self.mouse_inside = false;
                unsafe {
                    let _ = KillTimer(Some(hwnd), T_HOVER);
                }
                if self.hover.take().is_some() {
                    self.needs_draw = true;
                }
                if self.expanded && self.input.is_none() {
                    unsafe { SetTimer(Some(hwnd), T_COLLAPSE, COLLAPSE_DELAY_MS, None) };
                }
            }
            WM_LBUTTONDOWN => {
                let (x, y) = self.mouse_pos(lparam);
                match ui::hit_test(&self.hits, x, y) {
                    Some(a) => self.on_action(a),
                    None => self.stop_editing(),
                }
            }
            WM_MOUSEWHEEL => {
                if self.expanded && self.tab == Tab::Todo {
                    let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
                    let max = ui::max_todo_scroll(&self.state);
                    self.todo_scroll = if delta < 0 { (self.todo_scroll + 1).min(max) } else { self.todo_scroll.saturating_sub(1) };
                    self.needs_draw = true;
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
