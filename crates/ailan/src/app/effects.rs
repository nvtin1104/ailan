//! Audio-reactive bars, album artwork sync, Hub → island flight overlay, and tray menu.

use std::sync::Arc;
use std::time::Instant;

use ailan_core::i18n::{t, Key, Lang};
use ailan_core::state::{Banner, BannerIcon, Collapsed};
use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, GetWindowRect, KillTimer, SetTimer};

use crate::platform::tray::{MenuCmd, MenuItem};
use crate::platform::{autostart, sys};
use crate::ui;

use super::{App, AUDIO_FRAME_MS, T_AUDIO};

impl App {
    pub(super) fn sync_audio_timer(&mut self) {
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

    pub(super) fn step_audio(&mut self, dt: f32, now: i64) {
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
