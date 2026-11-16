//! AiLan Personal Hub window lifecycle, action dispatch, rendering, and Win32 handle_hub.

use std::time::Instant;

use ailan_core::settings::HubPage;
use ailan_core::todo::{parse_input, MAX_TEXT};
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{FillRect, GetMonitorInfoW, GetStockObject, MonitorFromPoint, MonitorFromWindow, RedrawWindow, ScreenToClient, BLACK_BRUSH, HBRUSH, HDC, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, RDW_ERASE, RDW_INVALIDATE, RDW_UPDATENOW};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_CONTROL};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::gfx::Gfx;
use crate::hub::controller::KeyOutcome;
use crate::hub::{Field, HubAction};
use crate::platform::media::MediaCmd;
use crate::platform::{autostart, sys};
use crate::ui;

use super::{apply_cursor_icon, App, FLY_REMIND, FLY_TODO, T_HUB};

pub(super) enum HubAnimEnd {
    Normal,
    Maximize { restore_rect: RECT },
    CloseToIsland,
}

pub(super) struct HubWindowAnim {
    pub start: Instant,
    pub duration: f32,
    pub from: RECT,
    pub to: RECT,
    pub fade_in: bool,
    pub end: HubAnimEnd,
}

fn lerp_i32(a: i32, b: i32, t: f32) -> i32 {
    (a as f32 + (b - a) as f32 * t).round() as i32
}

impl App {
    // ---- AiLan Personal Hub window ----------------------------------------------

    fn island_anim_rect(&self) -> RECT {
        let p = ui::pill_rect(self.w.value, self.h.value, self.visual_dock());
        let ix = (self.origin.0 as f32 + (p.x + p.w * 0.5) * self.scale).round() as i32;
        let iy = (self.origin.1 as f32 + (p.y + p.h * 0.5) * self.scale_y).round() as i32;
        let tw = (64.0 * self.scale).round() as i32;
        let th = (28.0 * self.scale_y).round() as i32;
        RECT {
            left: ix - tw / 2,
            top: iy - th / 2,
            right: ix + tw / 2,
            bottom: iy + th / 2,
        }
    }

    pub fn open_hub(&mut self, page: Option<HubPage>) {
        self.hub_anim = None;
        if let Some(p) = page {
            self.hub_ctrl.select_page(p);
        }
        let was_open = self.hub_open;
        let hs = self.hub_scale();
        let (work_w, work_h) = self.hub_work_dip(hs);
        let want_max = self.hub_ctrl.is_maximized;
        let base_w = if want_max {
            self.state.settings.hub_w.unwrap_or(crate::hub::controller::FULL_WIDTH as f32)
        } else {
            self.hub_ctrl.width
        };
        let base_h = if want_max {
            self.state.settings.hub_h.unwrap_or(crate::hub::controller::FULL_HEIGHT as f32)
        } else {
            self.hub_ctrl.height
        };
        let (cw, ch) = crate::hub::controller::HubController::clamp_dip(base_w, base_h, work_w, work_h);
        self.hub_ctrl.apply_size(cw, ch);
        let (dip_w, dip_h) = self.hub_ctrl.target_size();
        let wpx = (dip_w as f32 * hs).ceil() as i32;
        let hpx = (dip_h as f32 * hs).ceil() as i32;

        unsafe {
            let mon = if self.hidden {
                MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY)
            } else {
                MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST)
            };
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(mon, &mut mi);
            let r = mi.rcWork;
            let x = r.left + (r.right - r.left - wpx) / 2;
            let y = r.top + ((r.bottom - r.top - hpx) / 2).max(40);

            if !was_open && !self.state.settings.reduce_motion && !self.hidden {
                let p = ui::pill_rect(self.w.value, self.h.value, self.visual_dock());
                let start = (
                    self.origin.0 as f32 + (p.x + p.w * 0.5) * self.scale,
                    self.origin.1 as f32 + (p.y + p.h * 0.5) * self.scale_y,
                );
                let (end, target_dip) = if want_max {
                    (
                        ((r.left + r.right) as f32 * 0.5, (r.top + r.bottom) as f32 * 0.5),
                        ((r.right - r.left) as f32 / hs, (r.bottom - r.top) as f32 / hs),
                    )
                } else {
                    (
                        (x as f32 + wpx as f32 * 0.5, y as f32 + hpx as f32 * 0.5),
                        (dip_w as f32, dip_h as f32),
                    )
                };
                let launched = self.flight.as_mut().is_some_and(|f| {
                    f.launch_open(
                        start,
                        end,
                        self.scale,
                        ui::G_HOME,
                        crate::hub::widgets::ACCENT_SKY,
                        target_dip,
                    )
                });
                if launched {
                    let _ = SetWindowPos(self.hub_hwnd, None, x, y, wpx, hpx, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
                    self.collapse();
                    if !self.expanded {
                        self.w.velocity += 280.0;
                        self.h.velocity += 70.0;
                        self.needs_draw = true;
                    }
                    return;
                }
            }
        }

        self.collapse();
        self.reveal_hub_now();
    }

    pub(super) fn on_open_flight_landed(&mut self) {
        self.reveal_hub_now();
    }

    fn reveal_hub_now(&mut self) {
        self.hub_ctrl.close_alpha = 1.0;
        self.hub_open = true;
        let hs = self.hub_scale();
        let (work_w, work_h) = self.hub_work_dip(hs);
        let want_max = self.hub_ctrl.is_maximized;
        let base_w = if want_max {
            self.state.settings.hub_w.unwrap_or(crate::hub::controller::FULL_WIDTH as f32)
        } else {
            self.hub_ctrl.width
        };
        let base_h = if want_max {
            self.state.settings.hub_h.unwrap_or(crate::hub::controller::FULL_HEIGHT as f32)
        } else {
            self.hub_ctrl.height
        };
        let (cw, ch) = crate::hub::controller::HubController::clamp_dip(base_w, base_h, work_w, work_h);
        self.hub_ctrl.apply_size(cw, ch);
        let (dip_w, dip_h) = self.hub_ctrl.target_size();
        let wpx = (dip_w as f32 * hs).ceil() as i32;
        let hpx = (dip_h as f32 * hs).ceil() as i32;

        unsafe {
            let mon = if self.hidden {
                MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY)
            } else {
                MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST)
            };
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(mon, &mut mi);
            let r = mi.rcWork;
            let x = r.left + (r.right - r.left - wpx) / 2;
            let y = r.top + ((r.bottom - r.top - hpx) / 2).max(40);
            let _ = SetWindowPos(self.hub_hwnd, None, x, y, wpx, hpx, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
            if want_max {
                crate::platform::appwindow::set_window_rounded(self.hub_hwnd, false);
                let _ = ShowWindow(self.hub_hwnd, SW_MAXIMIZE);
            } else {
                crate::platform::appwindow::set_window_rounded(self.hub_hwnd, true);
                let _ = ShowWindow(self.hub_hwnd, SW_SHOW);
            }

            let mut rc = RECT::default();
            let _ = GetClientRect(self.hub_hwnd, &mut rc);
            let cur_w = (rc.right - rc.left).max(1) as u32;
            let cur_h = (rc.bottom - rc.top).max(1) as u32;
            let hs_now = self.hub_scale();
            self.hub_ctrl.apply_size(cur_w as f32 / hs_now, cur_h as f32 / hs_now);
            match self.hub_gfx.as_mut() {
                Some(g) => {
                    let _ = g.resize(cur_w, cur_h, hs_now);
                }
                None => {
                    self.hub_gfx = Gfx::new(self.hub_hwnd, cur_w, cur_h, hs_now).ok();
                }
            }
            let _ = SetForegroundWindow(self.hub_hwnd);
            let _ = SetFocus(Some(self.hub_hwnd));
        }
        if !self.state.settings.reduce_motion {
            self.hub_ctrl.restart_fade();
        }
        self.hub_needs_draw = true;
    }

    fn hub_scale(&self) -> f32 {
        let dpi = unsafe { GetDpiForWindow(self.hub_hwnd) };
        if dpi == 0 { self.scale } else { dpi as f32 / 96.0 }
    }

    fn hub_work_dip(&self, scale: f32) -> (f32, f32) {
        unsafe {
            let mon = MonitorFromWindow(self.hub_hwnd, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(mon, &mut mi);
            let s = scale.max(0.5);
            ((mi.rcWork.right - mi.rcWork.left) as f32 / s, (mi.rcWork.bottom - mi.rcWork.top) as f32 / s)
        }
    }

    fn persist_hub_geometry(&mut self) {
        self.state.settings.hub_maximized = self.hub_ctrl.is_maximized;
        if !self.hub_ctrl.is_maximized {
            self.state.settings.hub_w = Some(self.hub_ctrl.width);
            self.state.settings.hub_h = Some(self.hub_ctrl.height);
        }
        self.state.settings.hub_page = self.hub_ctrl.active_page;
        self.state.mark_settings_dirty(sys::now_ms());
    }

    pub fn resize_hub(&mut self, dip_w: i32, dip_h: i32) {
        let hs = self.hub_scale();
        let wpx = (dip_w as f32 * hs).ceil() as i32;
        let hpx = (dip_h as f32 * hs).ceil() as i32;
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
                let _ = g.resize(wpx as u32, hpx as u32, hs);
            }
        }
        self.hub_needs_draw = true;
    }

    fn hub_monitor_work_rect(&self) -> RECT {
        unsafe {
            let mon = MonitorFromWindow(self.hub_hwnd, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(mon, &mut mi);
            mi.rcWork
        }
    }

    fn hub_centered_rect(&self, from: RECT, dip_w: i32, dip_h: i32) -> RECT {
        let hs = self.hub_scale();
        let wpx = (dip_w as f32 * hs).ceil() as i32;
        let hpx = (dip_h as f32 * hs).ceil() as i32;
        let work = self.hub_monitor_work_rect();
        let w = wpx.min(work.right - work.left).max(1);
        let h = hpx.min(work.bottom - work.top).max(1);
        let cx = from.left + (from.right - from.left) / 2;
        let cy = from.top + (from.bottom - from.top) / 2;
        let x = (cx - w / 2).clamp(work.left, (work.right - w).max(work.left));
        let y = (cy - h / 2).clamp(work.top, (work.bottom - h).max(work.top));
        RECT { left: x, top: y, right: x + w, bottom: y + h }
    }

    fn finish_close_hub(&mut self) {
        self.hub_anim = None;
        self.hub_open = false;
        self.hub_ctrl.close_alpha = 1.0;
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

    pub fn close_hub(&mut self) {
        if !self.hub_open {
            return;
        }
        if self.state.settings.reduce_motion || self.hidden {
            self.finish_close_hub();
            return;
        }
        if matches!(self.hub_anim.as_ref().map(|a| &a.end), Some(HubAnimEnd::CloseToIsland)) {
            return;
        }
        self.persist_hub_geometry();
        let mut from = RECT::default();
        unsafe {
            let _ = GetWindowRect(self.hub_hwnd, &mut from);
            if IsZoomed(self.hub_hwnd).as_bool() {
                let _ = ShowWindow(self.hub_hwnd, SW_RESTORE);
                let _ = SetWindowPos(
                    self.hub_hwnd,
                    None,
                    from.left,
                    from.top,
                    from.right - from.left,
                    from.bottom - from.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                self.hub_ctrl.is_maximized = false;
                crate::platform::appwindow::set_window_rounded(self.hub_hwnd, true);
            }
        }
        let to = self.island_anim_rect();
        self.hub_ctrl.focus = None;
        self.hub_ctrl.on_mouse_leave();
        self.hub_cursor = None;
        self.hub_anim = Some(HubWindowAnim {
            start: Instant::now(),
            duration: 0.20,
            from,
            to,
            fade_in: false,
            end: HubAnimEnd::CloseToIsland,
        });
    }

    pub(super) fn step_hub_anim(&mut self) {
        let Some(anim) = self.hub_anim.as_ref() else { return };
        let t = (anim.start.elapsed().as_secs_f32() / anim.duration).clamp(0.0, 1.0);
        let is_close = matches!(anim.end, HubAnimEnd::CloseToIsland);
        let is_max = matches!(anim.end, HubAnimEnd::Maximize { .. });
        let fade_in = anim.fade_in;
        let ease = if is_close {
            t * t * (3.0 - 2.0 * t)
        } else if fade_in {
            // Spring-pop out of the Dynamic Island (~4.4% overshoot, then settle).
            let u = t - 1.0;
            let c1 = 1.1;
            let c3 = c1 + 1.0;
            let back = 1.0 + c3 * u * u * u + c1 * u * u;
            if is_max { back.min(1.0) } else { back }
        } else {
            1.0 - (1.0 - t).powi(3)
        };
        let x = lerp_i32(anim.from.left, anim.to.left, ease);
        let y = lerp_i32(anim.from.top, anim.to.top, ease);
        let w = lerp_i32(anim.from.right - anim.from.left, anim.to.right - anim.to.left, ease).max(1);
        let h = lerp_i32(anim.from.bottom - anim.from.top, anim.to.bottom - anim.to.top, ease).max(1);

        if is_close {
            self.hub_ctrl.close_alpha = (1.0 - t).powf(1.4);
            self.hub_needs_draw = true;
        } else if fade_in {
            self.hub_ctrl.close_alpha = (t / 0.65).clamp(0.0, 1.0).powf(0.8);
            self.hub_needs_draw = true;
        }

        unsafe {
            let _ = SetWindowPos(self.hub_hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        }

        if t < 1.0 {
            return;
        }

        self.hub_ctrl.close_alpha = 1.0;
        self.hub_needs_draw = true;

        let finished = self.hub_anim.take().unwrap();
        match finished.end {
            HubAnimEnd::Normal => {
                self.persist_hub_geometry();
            }
            HubAnimEnd::Maximize { restore_rect } => {
                unsafe {
                    let mut wp = WINDOWPLACEMENT {
                        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                        ..Default::default()
                    };
                    if GetWindowPlacement(self.hub_hwnd, &mut wp).is_ok() {
                        wp.rcNormalPosition = restore_rect;
                        let _ = SetWindowPlacement(self.hub_hwnd, &wp);
                    }
                    let _ = ShowWindow(self.hub_hwnd, SW_MAXIMIZE);
                }
                self.persist_hub_geometry();
            }
            HubAnimEnd::CloseToIsland => {
                self.finish_close_hub();
                if !self.expanded && !self.hidden {
                    self.w.velocity += 280.0;
                    self.h.velocity += 60.0;
                    self.landed_at = Some(Instant::now());
                    self.needs_draw = true;
                }
            }
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
            HubAction::ToggleMaximize => {
                if self.state.settings.reduce_motion {
                    unsafe {
                        let cmd = if IsZoomed(self.hub_hwnd).as_bool() { SW_RESTORE } else { SW_MAXIMIZE };
                        let _ = ShowWindow(self.hub_hwnd, cmd);
                    }
                    self.persist_hub_geometry();
                    return;
                }
                let mut from = RECT::default();
                unsafe {
                    let _ = GetWindowRect(self.hub_hwnd, &mut from);
                }
                if unsafe { IsZoomed(self.hub_hwnd).as_bool() } {
                    let mut wp = WINDOWPLACEMENT {
                        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                        ..Default::default()
                    };
                    let restore_rect = unsafe {
                        if GetWindowPlacement(self.hub_hwnd, &mut wp).is_ok()
                            && wp.rcNormalPosition.right > wp.rcNormalPosition.left
                            && wp.rcNormalPosition.bottom > wp.rcNormalPosition.top
                        {
                            wp.rcNormalPosition
                        } else {
                            let dw = self.state.settings.hub_w.unwrap_or(crate::hub::controller::FULL_WIDTH as f32).round() as i32;
                            let dh = self.state.settings.hub_h.unwrap_or(crate::hub::controller::FULL_HEIGHT as f32).round() as i32;
                            self.hub_centered_rect(from, dw, dh)
                        }
                    };
                    unsafe {
                        let _ = ShowWindow(self.hub_hwnd, SW_RESTORE);
                        let _ = SetWindowPos(
                            self.hub_hwnd,
                            None,
                            from.left,
                            from.top,
                            from.right - from.left,
                            from.bottom - from.top,
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                    }
                    self.hub_ctrl.is_maximized = false;
                    crate::platform::appwindow::set_window_rounded(self.hub_hwnd, true);
                    self.hub_ctrl.restart_fade();
                    self.hub_anim = Some(HubWindowAnim {
                        start: Instant::now(),
                        duration: 0.22,
                        from,
                        to: restore_rect,
                        fade_in: false,
                        end: HubAnimEnd::Normal,
                    });
                } else {
                    let to = self.hub_monitor_work_rect();
                    self.hub_ctrl.is_maximized = true;
                    crate::platform::appwindow::set_window_rounded(self.hub_hwnd, false);
                    self.hub_ctrl.restart_fade();
                    self.hub_anim = Some(HubWindowAnim {
                        start: Instant::now(),
                        duration: 0.22,
                        from,
                        to,
                        fade_in: false,
                        end: HubAnimEnd::Maximize { restore_rect: from },
                    });
                }
                return;
            }
            HubAction::ToggleMode => {
                if self.state.settings.reduce_motion {
                    if unsafe { IsZoomed(self.hub_hwnd).as_bool() } {
                        unsafe {
                            let _ = ShowWindow(self.hub_hwnd, SW_RESTORE);
                        }
                        self.hub_ctrl.is_maximized = false;
                        crate::platform::appwindow::set_window_rounded(self.hub_hwnd, true);
                    }
                    let (w, h) = self.hub_ctrl.toggle_mode();
                    self.resize_hub(w, h);
                    self.persist_hub_geometry();
                    return;
                }
                let mut from = RECT::default();
                unsafe {
                    let _ = GetWindowRect(self.hub_hwnd, &mut from);
                    if IsZoomed(self.hub_hwnd).as_bool() {
                        let _ = ShowWindow(self.hub_hwnd, SW_RESTORE);
                        let _ = SetWindowPos(
                            self.hub_hwnd,
                            None,
                            from.left,
                            from.top,
                            from.right - from.left,
                            from.bottom - from.top,
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                        self.hub_ctrl.is_maximized = false;
                        crate::platform::appwindow::set_window_rounded(self.hub_hwnd, true);
                    }
                }
                let (dip_w, dip_h) = self.hub_ctrl.toggle_mode();
                let to = self.hub_centered_rect(from, dip_w, dip_h);
                self.hub_anim = Some(HubWindowAnim {
                    start: Instant::now(),
                    duration: 0.22,
                    from,
                    to,
                    fade_in: false,
                    end: HubAnimEnd::Normal,
                });
                return;
            }
            HubAction::SelectPage(page) => {
                self.hub_ctrl.select_page(page);
                self.persist_hub_geometry();
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
            HubAction::ToggleBucket(i) => {
                if let Some(c) = self.hub_ctrl.collapsed.get_mut(i as usize) {
                    *c = !*c;
                }
                self.hub_needs_draw = true;
                return;
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
        let hs = self.hub_scale();
        let now = sys::now_ms();
        let state = &self.state;
        let ctrl = &mut self.hub_ctrl;
        let result = match self.hub_gfx.as_mut() {
            Some(g) => g.draw(|cv| ctrl.draw(cv, state, now, dip_w as f32, dip_h as f32)),
            None => return,
        };
        if let Err(e) = result {
            log::warn!("hub render failed ({e}); recreating device");
            let wpx = (dip_w as f32 * hs).ceil() as u32;
            let hpx = (dip_h as f32 * hs).ceil() as u32;
            self.hub_gfx = Gfx::new(self.hub_hwnd, wpx, hpx, hs).ok();
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
        let hs = self.hub_scale();

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
            WM_GETMINMAXINFO => {
                let mmi = lparam.0 as *mut MINMAXINFO;
                if !mmi.is_null() {
                    unsafe {
                        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                        let mut mi = MONITORINFO {
                            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                            ..Default::default()
                        };
                        if GetMonitorInfoW(mon, &mut mi).as_bool() {
                            let rc_work = mi.rcWork;
                            let rc_mon = mi.rcMonitor;
                            (*mmi).ptMaxPosition.x = rc_work.left - rc_mon.left;
                            (*mmi).ptMaxPosition.y = rc_work.top - rc_mon.top;
                            (*mmi).ptMaxSize.x = rc_work.right - rc_work.left;
                            (*mmi).ptMaxSize.y = rc_work.bottom - rc_work.top;
                        }
                        if matches!(self.hub_anim.as_ref().map(|a| &a.end), Some(HubAnimEnd::CloseToIsland)) {
                            (*mmi).ptMinTrackSize.x = 1;
                            (*mmi).ptMinTrackSize.y = 1;
                        } else {
                            (*mmi).ptMinTrackSize.x = (crate::hub::controller::HUB_MIN_W * hs).round() as i32;
                            (*mmi).ptMinTrackSize.y = (crate::hub::controller::HUB_MIN_H * hs).round() as i32;
                        }
                    }
                }
                return Some(LRESULT(0));
            }
            WM_SIZING => {
                if self.hub_ctrl.is_maximized {
                    return Some(LRESULT(0));
                }
                let rect = lparam.0 as *mut windows::Win32::Foundation::RECT;
                if !rect.is_null() {
                    unsafe {
                        let (work_w, work_h) = self.hub_work_dip(hs);
                        let w = ((*rect).right - (*rect).left) as f32 / hs;
                        let h = ((*rect).bottom - (*rect).top) as f32 / hs;
                        let (cw, ch) = crate::hub::controller::HubController::clamp_dip(w, h, work_w, work_h);
                        let wpx = (cw * hs).round() as i32;
                        let hpx = (ch * hs).round() as i32;
                        let edge = wparam.0 as u32;
                        if matches!(edge, WMSZ_LEFT | WMSZ_TOPLEFT | WMSZ_BOTTOMLEFT) {
                            (*rect).left = (*rect).right - wpx;
                        } else {
                            (*rect).right = (*rect).left + wpx;
                        }
                        if matches!(edge, WMSZ_TOP | WMSZ_TOPLEFT | WMSZ_TOPRIGHT) {
                            (*rect).top = (*rect).bottom - hpx;
                        } else {
                            (*rect).bottom = (*rect).top + hpx;
                        }
                    }
                }
                return Some(LRESULT(1));
            }
            WM_SIZE => {
                if wparam.0 != SIZE_MINIMIZED as usize {
                    let zoomed = wparam.0 == SIZE_MAXIMIZED as usize;
                    if self.hub_ctrl.is_maximized != zoomed {
                        self.hub_ctrl.is_maximized = zoomed;
                        crate::platform::appwindow::set_window_rounded(hwnd, !zoomed);
                    }
                    let w = (lparam.0 & 0xFFFF) as u32 as f32 / hs;
                    let h = ((lparam.0 >> 16) & 0xFFFF) as u32 as f32 / hs;
                    if w > 8.0 && h > 8.0 {
                        self.hub_ctrl.apply_size(w, h);
                        let px_w = (lparam.0 & 0xFFFF) as u32;
                        let px_h = ((lparam.0 >> 16) & 0xFFFF) as u32;
                        if let Some(g) = self.hub_gfx.as_mut() {
                            let _ = g.resize(px_w, px_h, hs);
                        }
                        self.hub_needs_draw = true;
                    }
                }
            }
            WM_EXITSIZEMOVE => self.persist_hub_geometry(),
            WM_NCHITTEST => {
                let mut pt = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut pt);
                    let _ = ScreenToClient(hwnd, &mut pt);
                }
                let x = pt.x as f32 / hs;
                let y = pt.y as f32 / hs;
                if self.hub_ctrl.is_maximized {
                    let hit = if self.hub_ctrl.is_caption(x, y) { HTCAPTION } else { HTCLIENT };
                    return Some(LRESULT(hit as isize));
                }
                let b = 6.0;
                let w = self.hub_ctrl.width;
                let h = self.hub_ctrl.height;
                let left = x < b;
                let right = x > w - b;
                let top = y < b;
                let bottom = y > h - b;
                let hit = match (left, right, top, bottom) {
                    (true, _, true, _) => HTTOPLEFT,
                    (_, true, true, _) => HTTOPRIGHT,
                    (true, _, _, true) => HTBOTTOMLEFT,
                    (_, true, _, true) => HTBOTTOMRIGHT,
                    (true, _, _, _) => HTLEFT,
                    (_, true, _, _) => HTRIGHT,
                    (_, _, true, _) => HTTOP,
                    (_, _, _, true) => HTBOTTOM,
                    _ if self.hub_ctrl.is_caption(x, y) => HTCAPTION,
                    _ => HTCLIENT,
                };
                return Some(LRESULT(hit as isize));
            }
            WM_SETCURSOR => {
                let hit_test = (lparam.0 & 0xFFFF) as u32;
                if hit_test == HTCLIENT {
                    apply_cursor_icon(self.cursor_icon_hub());
                    return Some(LRESULT(1));
                }
            }
            WM_MOUSEMOVE => {
                let pos = self.dip_from(lparam, hs, hs);
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
                let (x, y) = self.dip_from(lparam, hs, hs);
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
