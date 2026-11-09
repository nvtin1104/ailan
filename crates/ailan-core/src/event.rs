//! Events produced by platform providers and consumed by `AppState::apply`.

use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub app: String,
    pub playing: bool,
    /// Playback position at `sampled_at`.
    pub position_ms: i64,
    pub duration_ms: i64,
    /// Epoch ms when `position_ms` was valid; progress is interpolated from it.
    pub sampled_at: i64,
    /// Identifies the track, so artwork is only re-decoded on change.
    pub track_key: String,
}

impl MediaInfo {
    pub fn position_at(&self, now: i64) -> i64 {
        let p = if self.playing { self.position_ms + (now - self.sampled_at).max(0) } else { self.position_ms };
        if self.duration_ms > 0 { p.min(self.duration_ms) } else { p }
    }
}

/// Decoded artwork: premultiplied BGRA, `width * height * 4` bytes.
#[derive(Debug, PartialEq)]
pub struct Artwork {
    pub track_key: String,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Network {
    pub connected: bool,
    /// `Some((ssid, bars 0..=5))` when on Wi-Fi.
    pub wifi: Option<(String, u8)>,
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    Media(Option<MediaInfo>),
    Artwork(Option<Arc<Artwork>>),
    /// Master volume 0..=1 and mute flag of the default render device.
    Volume { level: f32, muted: bool },
    /// Display brightness 0..=100.
    Brightness(u8),
    /// `None` on machines without a battery.
    Battery(Option<Battery>),
    Network(Network),
    /// Default output device changed; `headphones` from its form factor.
    AudioDevice { name: String, headphones: bool },
}
