//! AiLan Personal Hub window lifecycle, action dispatch, rendering, and Win32 handle_hub.

use ailan_core::settings::{HubPage, Tab};
use ailan_core::todo::{parse_input, MAX_TEXT};
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{FillRect, GetMonitorInfoW, GetStockObject, MonitorFromPoint, RedrawWindow, ScreenToClient, BLACK_BRUSH, HBRUSH, HDC, MONITORINFO, MONITOR_DEFAULTTOPRIMARY, RDW_ERASE, RDW_INVALIDATE, RDW_UPDATENOW};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_CONTROL};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::gfx::Gfx;
use crate::hub::controller::KeyOutcome;
use crate::hub::{Field, HubAction};
use crate::platform::media::MediaCmd;
use crate::platform::{autostart, sys};
use crate::ui;

use super::{apply_cursor_icon, App, FLY_FOCUS, FLY_REMIND, FLY_TODO, T_HUB};

impl App {
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

    /// Run `T_HUB` only while the hub shows something that changes by itself.
    pub(super) fn sync_hub_timer(&mut self) {
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
    pub(super) fn refresh_world_offsets(&mut self, now: i64) {
        let offs: Vec<Option<i32>> = self.state.data.world_clocks.iter().map(|w| self.tz.offset_min(&w.tz, now)).collect();
        self.state.world_offsets = offs;
    }

    pub(super) fn on_hub_action(&mut self, action: HubAction) {
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
                        let parsed_due = plan.due(now, self.state.offset_min);
                        let (due, all_day) = match parsed_due {
                            Some(d) => (Some(d), false),
                            None => (Some(ailan_core::time::local_midnight(now, self.state.offset_min)), true),
                        };
                        let text: String = plan.title.chars().take(MAX_TEXT).collect();
                        self.state.data.todos.add_full(text, due, all_day, plan.repeat, now);
                        self.state.mark_data_dirty(now);
                        flight = Some(if parsed_due.is_some() { (ui::G_BELL, FLY_REMIND) } else { (ui::G_CHECKMARK, FLY_TODO) });
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
                    self.state.data.todos.add_today(text, due, now, self.state.offset_min);
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
            HubAction::ShiftTodoDate(id, delta) => {
                if self.state.data.todos.shift_date(id, delta as i64, now, self.state.offset_min) {
                    self.state.mark_data_dirty(now);
                }
            }
            HubAction::SetTodoToday(id) => {
                if self.state.data.todos.set_date_today(id, now, self.state.offset_min) {
                    self.state.mark_data_dirty(now);
                }
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

    pub(super) fn render_hub(&mut self) {
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

    pub(super) fn cursor_icon_hub(&self) -> ui::CursorIcon {
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

}
