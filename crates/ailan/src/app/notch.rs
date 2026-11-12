//! Notch window interaction: gaze tracking, expand/collapse, text input, and Win32 handle.

use ailan_core::settings::{HubPage, Tab};
use ailan_core::state::Collapsed;
use ailan_core::todo::{parse_input, MAX_TEXT};
use ailan_core::AppEvent;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::platform::media::MediaCmd;
use crate::platform::{power, sys, WM_APP_APPBAR, WM_APP_AUDIO_DEVICE, WM_APP_EVENT, WM_APP_TRAY};
use crate::platform::appbar::ABN_FULLSCREENAPP;
use crate::ui::{self, Action};

use super::{apply_cursor_icon, visible_index, App, COLLAPSE_DELAY_MS, HOVER_INTENT_MS, PBT_APMRESUMEAUTOMATIC, PBT_POWERSETTINGCHANGE, T_AUDIO, T_COLLAPSE, T_HOVER, T_STATS, T_TICK};

impl App {
    // ---- idle eyes follow the pointer -------------------------------------------------

    /// Raw mouse input (works while the pointer is anywhere on screen) is only registered
    /// while the idle eyes are visible, so a still or elsewhere-busy island costs nothing.
    pub(super) fn sync_raw_mouse(&mut self) {
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
    pub(super) fn update_gaze(&mut self) {
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

    pub(super) fn expand(&mut self) {
        if self.expanded {
            return;
        }
        let now = sys::now_ms();
        self.expanded = true;
        let visible = self.state.hover_activities(now);
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
            let idx = visible_index(&self.state, self.tab, now);
            self.tab_pos.value = idx;
            self.tab_pos.set_target(idx);
        }
        self.state.dismiss_banner();
        self.after_change();
    }

    pub(super) fn collapse(&mut self) {
        if !self.expanded || self.pinned {
            return;
        }
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

    pub(super) fn begin_editing(&mut self) {
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

    pub(super) fn stop_editing(&mut self) {
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
                    let max = ui::max_todo_scroll(&self.state, sys::now_ms());
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
