//! Notch window interaction: gaze tracking, expand/collapse, text input, and Win32 handle.

use ailan_core::settings::{HubPage, Tab};
use ailan_core::state::{BannerIcon, Collapsed};
use ailan_core::todo::{parse_input, MAX_TEXT};
use ailan_core::AppEvent;
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::core::w;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, GetStockObject, ScreenToClient, SetBkColor, SetTextColor, DEFAULT_GUI_FONT, HDC};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetDoubleClickTime, GetFocus, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, VK_ESCAPE, VK_RETURN, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::platform::{notify_mirror, WM_APP_EDIT};

static EDIT_PREV: AtomicIsize = AtomicIsize::new(0);
static NOTCH_HWND: AtomicIsize = AtomicIsize::new(0);
static MOUSE_HOOK: AtomicIsize = AtomicIsize::new(0);

const WM_APP_CLICK_OUTSIDE: u32 = WM_APP + 7;

fn install_outside_hook(hwnd: HWND) {
    NOTCH_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
    if MOUSE_HOOK.load(Ordering::Relaxed) != 0 {
        return;
    }
    unsafe {
        if let Ok(hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(outside_mouse_proc), None, 0) {
            MOUSE_HOOK.store(hook.0 as isize, Ordering::Relaxed);
        }
    }
}

fn uninstall_outside_hook() {
    let raw = MOUSE_HOOK.swap(0, Ordering::Relaxed);
    if raw != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(raw as *mut _));
        }
    }
}

unsafe extern "system" fn outside_mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        if msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN {
            let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let raw = NOTCH_HWND.load(Ordering::Relaxed);
            if raw != 0 {
                let _ = PostMessageW(
                    Some(HWND(raw as *mut _)),
                    WM_APP_CLICK_OUTSIDE,
                    WPARAM(ms.pt.x as isize as usize),
                    LPARAM(ms.pt.y as isize),
                );
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

use crate::platform::media::MediaCmd;
use crate::platform::{power, sys, WM_APP_APPBAR, WM_APP_AUDIO_DEVICE, WM_APP_EVENT, WM_APP_TRAY};
use crate::platform::appbar::ABN_FULLSCREENAPP;
use crate::ui::{self, Action};

use super::{apply_cursor_icon, visible_index, App, Grab, PBT_APMRESUMEAUTOMATIC, PBT_POWERSETTINGCHANGE, T_STATS, T_TICK};

impl App {
    pub(super) fn expand(&mut self) {
        if self.expanded {
            return;
        }
        let now = sys::now_ms();
        self.expanded = true;
        let visible = self.state.hover_activities(now);
        if !visible.is_empty() {
            let preferred = match self.mode {
                Collapsed::Media => Some(Tab::Music),
                Collapsed::Todo => Some(Tab::Todo),
                _ => self.state.primary_activity(now),
            };
            self.tab = preferred
                .filter(|t| visible.contains(t))
                .unwrap_or(visible[0]);
            let idx = visible_index(&self.state, self.tab, now);
            self.tab_pos.value = idx;
            self.tab_pos.set_target(idx);
        }
        if let Some(id) = self.state.dismiss_banner() {
            notify_mirror::dismiss(id);
        }
        if !self.pinned {
            install_outside_hook(self.hwnd);
        }
        self.after_change();
    }

    pub(super) fn collapse(&mut self) {
        if !self.expanded || self.pinned {
            return;
        }
        uninstall_outside_hook();
        self.stop_editing();
        self.expanded = false;
        self.hover = None;
        self.hover_anim.set_target(0.0);
        self.after_change();
    }

    pub(super) fn set_hidden(&mut self, hidden: bool) {
        if hidden == self.hidden {
            return;
        }
        self.hidden = hidden;
        if hidden {
            uninstall_outside_hook();
            unsafe { let _ = ReleaseCapture(); }
            self.grab = None;
            self.expanded = false;
            self.stop_editing();
            self.art = None;
            if let Some(g) = &self.gfx {
                g.trim();
            }
        }
        // Visibility follows island_live() (may peek alert banners over fullscreen).
        self.after_change();
    }

    pub(super) fn select_tab(&mut self, tab: Tab) {
        let now = sys::now_ms();
        let visible = self.state.hover_activities(now);
        if !visible.contains(&tab) {
            return;
        }
        if self.tab != tab {
            self.tab = tab;
            self.state.settings.tab = tab;
            self.state.mark_settings_dirty(now);
            if tab != Tab::Todo {
                self.stop_editing();
            }
            self.after_change();
        }
    }

    pub(super) fn slide_activity(&mut self, delta: isize) {
        let visible = self.state.hover_activities(sys::now_ms());
        if visible.len() <= 1 {
            return;
        }
        let cur = visible.iter().position(|&t| t == self.tab).unwrap_or(0) as isize;
        let len = visible.len() as isize;
        let next = ((cur + delta).rem_euclid(len)) as usize;
        self.select_tab(visible[next]);
    }

    // ---- text input ----------------------------------------------------------------

    fn drop_noactivate(&self) {
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex & !(WS_EX_NOACTIVATE.0 as isize));
            let _ = SetWindowPos(self.hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE);
        }
    }

    fn restore_noactivate(&self) {
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE.0 as isize);
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE,
            );
        }
    }

    fn ensure_edit(&mut self) {
        if !self.edit.is_invalid() {
            return;
        }
        unsafe {
            let edit = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                w!(""),
                WS_CHILD | WS_CLIPSIBLINGS | WINDOW_STYLE(0x0080),
                0,
                0,
                10,
                10,
                Some(self.hwnd),
                None,
                None,
                None,
            );
            if let Ok(edit) = edit {
                let prev = GetWindowLongPtrW(edit, GWLP_WNDPROC);
                EDIT_PREV.store(prev, Ordering::Relaxed);
                SetWindowLongPtrW(edit, GWLP_WNDPROC, todo_edit_proc as *const () as isize);
                let stock = GetStockObject(DEFAULT_GUI_FONT);
                let _ = SendMessageW(edit, WM_SETFONT, Some(WPARAM(stock.0 as usize)), Some(LPARAM(1)));
                self.edit = edit;
            }
        }
    }

    pub(super) fn sync_edit_bounds(&mut self) {
        if self.edit.is_invalid() {
            return;
        }
        let show = self.input.is_some();
        unsafe {
            if !show {
                let _ = ShowWindow(self.edit, SW_HIDE);
                return;
            }
            if let Some(h) = self.hits.iter().find(|h| h.action == Action::TodoInput) {
                let r = h.rect.inset(8.0, 3.0);
                let _ = SetWindowPos(
                    self.edit,
                    None,
                    (r.x * self.scale) as i32,
                    (r.y * self.scale_y) as i32,
                    (r.w * self.scale).max(8.0) as i32,
                    (r.h * self.scale_y).max(8.0) as i32,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let _ = ShowWindow(self.edit, SW_SHOW);
            }
        }
    }

    pub(super) fn begin_editing(&mut self) {
        if self.input.is_none() {
            self.input = Some(String::new());
        }
        self.ensure_edit();
        self.drop_noactivate();
        unsafe {
            if !self.edit.is_invalid() {
                let _ = SetWindowTextW(self.edit, w!(""));
                let _ = SetForegroundWindow(self.hwnd);
                let _ = SetFocus(Some(self.edit));
            }
        }
        self.needs_draw = true;
    }

    pub(super) fn stop_editing(&mut self) {
        if self.input.take().is_some() {
            self.high_surrogate = None;
            if !self.edit.is_invalid() {
                unsafe {
                    let _ = ShowWindow(self.edit, SW_HIDE);
                }
            }
            self.restore_noactivate();
            self.needs_draw = true;
            self.update_notch_cursor();
        }
    }

    fn commit_edit(&mut self, escape: bool) {
        if escape {
            self.stop_editing();
            return;
        }
        if self.edit.is_invalid() {
            self.stop_editing();
            return;
        }
        let mut buf = [0u16; 256];
        let n = unsafe { GetWindowTextW(self.edit, &mut buf) }.max(0) as usize;
        let s = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
        let now = sys::now_ms();
        if let Some((text, due)) = parse_input(&s, now, self.state.offset_min) {
            self.state.data.todos.add_today(text, due, now, self.state.offset_min);
            self.state.mark_data_dirty(now);
        }
        self.stop_editing();
        self.after_change();
    }

    fn drag_threshold(&self) -> (i32, i32) {
        let dpi = (self.scale * 96.0).round().max(96.0) as u32;
        let cx = unsafe { GetSystemMetricsForDpi(SM_CXDRAG, dpi) }.max(1);
        let cy = unsafe { GetSystemMetricsForDpi(SM_CYDRAG, dpi) }.max(1);
        (cx, cy)
    }

    fn chrome_click(&mut self) {
        let (x, y) = self.cursor.unwrap_or((0.0, 0.0));
        match ui::hit_test(&self.hits, x, y) {
            Some(Action::Expand) | None if !self.expanded => {
                self.expand();
                self.arm_eat();
            }
            Some(Action::DismissBanner) => {
                self.on_action(Action::DismissBanner);
                self.arm_eat();
            }
            Some(Action::Expand) | None => {
                self.collapse();
                self.arm_eat();
            }
            Some(a) => self.on_action(a),
        }
    }

    fn arm_eat(&mut self) {
        self.eat_until = sys::now_ms() + unsafe { GetDoubleClickTime() } as i64;
    }

    fn pointer_move_drag(&mut self) {
        let (sx, sy, already) = {
            let Some(grab) = self.grab.as_ref() else { return };
            (grab.sx, grab.sy, grab.dragging)
        };
        let mut pt = POINT::default();
        if unsafe { GetCursorPos(&mut pt) }.is_err() {
            return;
        }
        let dx = pt.x - sx;
        let dy = pt.y - sy;
        let (cx, cy) = self.drag_threshold();
        let dragging = already || dx.abs() >= cx || dy.abs() >= cy;
        if let Some(g) = self.grab.as_mut() {
            g.dragging = dragging;
        }
        if dragging {
            self.snap_drag_to_cursor(false);
        }
    }

    pub(super) fn on_char(&mut self, unit: u16) {
        let Some(input) = self.input.as_mut() else { return };
        match unit {
            0x08 => {
                input.pop();
            }
            0x0D => {
                let now = sys::now_ms();
                if let Some((text, due)) = parse_input(input, now, self.state.offset_min) {
                    self.state.data.todos.add_today(text, due, now, self.state.offset_min);
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

    pub(super) fn on_action(&mut self, action: Action) {
        let now = sys::now_ms();
        match action {
            Action::Expand => self.expand(),
            Action::DismissBanner => {
                let message = self.state.banner.as_ref().is_some_and(|b| b.icon == BannerIcon::Message);
                if let Some(id) = self.state.dismiss_banner() {
                    notify_mirror::dismiss(id);
                }
                if message {
                    self.after_change();
                } else {
                    self.expand();
                }
            }
            Action::Tab(t) => self.select_tab(t),
            Action::SlidePrev => self.slide_activity(-1),
            Action::SlideNext => self.slide_activity(1),
            Action::OpenSettings => self.open_hub(Some(HubPage::Settings)),
            Action::MediaPrev => self.media.send(MediaCmd::Previous),
            Action::MediaToggle => self.media.send(MediaCmd::TogglePlayPause),
            Action::MediaNext => self.media.send(MediaCmd::Next),
            Action::TodoInput => self.begin_editing(),
            Action::TodoToggle(id) => {
                self.state.data.todos.toggle(id, now, self.state.offset_min);
                self.state.mark_data_dirty(now);
                self.todo_scroll = self.todo_scroll.min(ui::max_todo_scroll(&self.state, now));
            }
            Action::TodoDelete(id) => {
                self.state.data.todos.remove(id);
                self.state.mark_data_dirty(now);
                self.todo_scroll = self.todo_scroll.min(ui::max_todo_scroll(&self.state, now));
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

    pub(super) fn cursor_icon_notch(&self, x: f32, y: f32) -> ui::CursorIcon {
        match ui::hit_test(&self.hits, x, y) {
            Some(action) => action.cursor_icon(self.input.is_some()),
            None => ui::CursorIcon::Default,
        }
    }

    pub(super) fn update_notch_cursor(&self) {
        if let Some((x, y)) = self.cursor {
            apply_cursor_icon(self.cursor_icon_notch(x, y));
        } else {
            apply_cursor_icon(ui::CursorIcon::Default);
        }
    }

    pub fn handle(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let hwnd = self.hwnd;
        match msg {
            WM_NCHITTEST => {
                // Explicitly claim client hit-test so hover & mouse events never leak through
                // to windows underneath the notch.
                return Some(LRESULT(HTCLIENT as isize));
            }
            WM_APP_EVENT => {
                let now = sys::now_ms();
                let mut changed = false;
                let mut mirrored = false;
                while let Ok(ev) = self.rx.try_recv() {
                    let is_mirror = matches!(ev, AppEvent::MirroredNotify { .. });
                    if self.state.apply(ev, now) {
                        changed = true;
                        mirrored |= is_mirror;
                    }
                }
                if mirrored {
                    unsafe {
                        let _ = MessageBeep(MB_ICONASTERISK);
                    }
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
                T_STATS => self.log_stats(),
                super::T_GAZE => self.on_gaze_tick(),
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
                self.cursor = Some(pos);
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
                    }
                    self.needs_draw = true;
                }
                self.pointer_move_drag();
                let hover = ui::hit_test(&self.hits, pos.0, pos.1);
                apply_cursor_icon(match hover {
                    Some(act) => act.cursor_icon(self.input.is_some()),
                    None => ui::CursorIcon::Default,
                });
                if hover != self.hover {
                    self.hover = hover;
                    if matches!(hover, Some(a) if !matches!(a, Action::Expand | Action::TodoScroll | Action::DismissBanner)) {
                        self.hover_anim.value = 0.15;
                        self.hover_anim.set_target(1.0);
                    } else if hover.is_some() {
                        self.hover_anim.set_target(0.35);
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
                apply_cursor_icon(ui::CursorIcon::Default);
                self.needs_draw = true;
            }
            WM_LBUTTONDOWN => {
                if sys::now_ms() < self.eat_until {
                    return Some(LRESULT(0));
                }
                let (x, y) = self.mouse_pos(lparam);
                self.cursor = Some((x, y));
                let action = ui::hit_test(&self.hits, x, y);
                let chrome = matches!(action, None | Some(Action::Expand) | Some(Action::DismissBanner));
                if !chrome {
                    if let Some(a) = action {
                        self.on_action(a);
                    }
                } else {
                    let mut pt = POINT::default();
                    unsafe {
                        let _ = GetCursorPos(&mut pt);
                        SetCapture(hwnd);
                    }
                    self.grab = Some(Grab { sx: pt.x, sy: pt.y, dragging: false });
                }
                self.update_notch_cursor();
            }
            WM_LBUTTONUP => {
                let grab = self.grab.take();
                unsafe { let _ = ReleaseCapture(); }
                if let Some(g) = grab {
                    if g.dragging {
                        self.commit_drag();
                    } else {
                        self.chrome_click();
                    }
                }
            }
            WM_CAPTURECHANGED => {
                self.grab = None;
            }
            WM_RBUTTONUP | WM_CONTEXTMENU => {}
            crate::platform::WM_APP_EDIT => match wparam.0 {
                1 => self.commit_edit(false),
                2 => self.commit_edit(true),
                _ => {
                    let focus = unsafe { GetFocus() };
                    if self.input.is_some() && focus != self.edit && focus != hwnd {
                        self.stop_editing();
                    }
                }
            },
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
                if self.expanded && self.tab == Tab::Todo {
                    let max = ui::max_todo_scroll(&self.state, sys::now_ms());
                    self.todo_scroll = if delta < 0 { (self.todo_scroll + 1).min(max) } else { self.todo_scroll.saturating_sub(1) };
                    self.needs_draw = true;
                } else if self.expanded {
                    self.slide_activity(if delta < 0 { 1 } else { -1 });
                }
            }
            WM_MOUSEACTIVATE => {
                let mut pt = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut pt);
                    let _ = ScreenToClient(hwnd, &mut pt);
                }
                let x = pt.x as f32 / self.scale.max(0.5);
                let y = pt.y as f32 / self.scale_y.max(0.5);
                if matches!(ui::hit_test(&self.hits, x, y), Some(Action::TodoInput)) {
                    self.drop_noactivate();
                    return Some(LRESULT(MA_ACTIVATE as isize));
                }
                return Some(LRESULT(MA_NOACTIVATE as isize));
            }
            WM_CHAR => self.on_char(wparam.0 as u16),
            WM_KILLFOCUS => {
                let next = HWND(wparam.0 as *mut _);
                if next != self.edit {
                    self.stop_editing();
                }
            }
            WM_CTLCOLOREDIT => {
                unsafe {
                    SetTextColor(HDC(wparam.0 as _), COLORREF(0x00FCFCFC));
                    SetBkColor(HDC(wparam.0 as _), COLORREF(0x00141414));
                    let brush = CreateSolidBrush(COLORREF(0x00141414));
                    return Some(LRESULT(brush.0 as isize));
                }
            }
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
                let dragging = self.grab.as_ref().is_some_and(|g| g.dragging);
                let result = if dragging { self.refresh_scale_keep_origin() } else { self.place() };
                if let Err(e) = result {
                    log::error!("reposition: {e}");
                }
            }
            WM_APP_CLICK_OUTSIDE => {
                if self.expanded && !self.pinned && self.grab.is_none() {
                    let px = wparam.0 as isize as i32;
                    let py = lparam.0 as i32;
                    let p = ui::pill_rect(self.w.value, self.h.value, self.dock);
                    let ear = ui::ear_for(self.dock);
                    let left = self.origin.0 + ((p.x - ear).max(0.0) * self.scale).floor() as i32;
                    let top = self.origin.1 + ((p.y - ear).max(0.0) * self.scale_y).floor() as i32;
                    let right = self.origin.0 + ((p.right() + ear).min(ui::WIN_W) * self.scale).ceil() as i32;
                    let bottom = self.origin.1 + ((p.bottom() + ear).min(ui::WIN_H) * self.scale_y).ceil() as i32;
                    if px < left || px >= right || py < top || py >= bottom {
                        self.collapse();
                    }
                }
            }
            WM_DESTROY => {
                uninstall_outside_hook();
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

unsafe extern "system" fn todo_edit_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_KEYDOWN {
        let vk = wparam.0 as u16;
        let code = if vk == VK_RETURN.0 { 1 } else if vk == VK_ESCAPE.0 { 2 } else { 0 };
        if code != 0 {
            if let Ok(parent) = GetParent(hwnd) {
                let _ = PostMessageW(Some(parent), WM_APP_EDIT, WPARAM(code), LPARAM(0));
            }
            return LRESULT(0);
        }
    }
    if msg == WM_KILLFOCUS {
        if let Ok(parent) = GetParent(hwnd) {
            let _ = PostMessageW(Some(parent), WM_APP_EDIT, WPARAM(3), LPARAM(0));
        }
    }
    let prev = EDIT_PREV.load(Ordering::Relaxed);
    if prev != 0 {
        let proc: WNDPROC = std::mem::transmute(prev);
        CallWindowProcW(proc, hwnd, msg, wparam, lparam)
    } else {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
