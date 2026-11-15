//! Audio-reactive bars, album artwork sync, Hub → island flight overlay, and tray menu.

use std::sync::Arc;
use std::time::Instant;

use ailan_core::i18n::{t, Key, Lang};
use ailan_core::state::{Banner, BannerIcon};
use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, GetWindowRect};

use crate::platform::tray::{MenuCmd, MenuItem};
use crate::platform::{autostart, sys};
use crate::ui;

use super::App;

impl App {
    pub(super) fn tray_menu(&mut self) {
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
                    icon: BannerIcon::Reminder,
                    title: format!("Ailan {}", env!("CARGO_PKG_VERSION")),
                    body: "A quiet island for Windows".into(),
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

    pub(super) fn flight_active(&self) -> bool {
        self.flight.as_ref().is_some_and(|f| f.is_active())
    }

    /// Landing glow intensity (1 → 0 over 300 ms).
    pub(super) fn glow(&self) -> f32 {
        self.landed_at.map_or(0.0, |t| (1.0 - t.elapsed().as_secs_f32() / 0.3).max(0.0))
    }

    /// Fly `glyph` from the last pressed Hub control into the island. Without a visible hub,
    /// notch or with reduced motion the state change simply applies immediately.
    pub(super) fn launch_flight(&mut self, glyph: char, tint: u32) {
        if self.state.settings.reduce_motion || self.hidden || !self.hub_open || self.expanded {
            return;
        }
        let Some((px, py)) = self.hub_ctrl.last_press else { return };
        let mut rc = windows::Win32::Foundation::RECT::default();
        if unsafe { GetWindowRect(self.hub_hwnd, &mut rc) }.is_err() {
            return;
        }
        let start = (rc.left as f32 + px * self.scale, rc.top as f32 + py * self.scale);
        let p = ui::pill_rect(self.w.value, self.h.value, self.visual_dock());
        let end = (
            self.origin.0 as f32 + (p.x + p.w * 0.5) * self.scale,
            self.origin.1 as f32 + (p.y + p.h * 0.5) * self.scale_y,
        );
        let scale = self.scale;
        if let Some(f) = self.flight.as_mut() {
            f.launch(start, end, scale, glyph, tint);
        }
    }

    pub(super) fn on_flight_landed(&mut self) {
        // A small "gulp": brief width/height impulse on the (well-damped) size springs.
        self.w.velocity += 320.0;
        self.h.velocity += 80.0;
        self.landed_at = Some(Instant::now());
        self.after_change();
    }

    pub(super) fn sync_artwork(&mut self) {
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

}
