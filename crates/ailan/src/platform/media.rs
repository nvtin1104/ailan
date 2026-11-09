//! "Now playing" from any app via Global System Media Transport Controls (SMTC).
//!
//! A dedicated MTA worker owns the session manager. WinRT change events only
//! nudge the worker through its channel; the worker reads the snapshot, dedupes,
//! decodes artwork once per track and forwards `AppEvent`s to the UI thread.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use ailan_core::event::{Artwork, MediaInfo};
use ailan_core::AppEvent;
use windows::core::Result;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapDitherTypeNone,
    WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand, WICRect,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, IStream, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::WinRT::CreateStreamOverRandomAccessStream;

use super::sys::now_ms;
use super::EventSink;

/// Artwork is decoded to this square size (enough for 2x of an 88 DIP tile).
const ART_PX: u32 = 176;

#[derive(Debug, Clone, Copy)]
pub enum MediaCmd {
    TogglePlayPause,
    Next,
    Previous,
}

enum Msg {
    Cmd(MediaCmd),
    SessionChanged,
    Refresh,
}

#[derive(Clone)]
pub struct MediaControl {
    tx: Sender<Msg>,
}

impl MediaControl {
    pub fn send(&self, cmd: MediaCmd) {
        let _ = self.tx.send(Msg::Cmd(cmd));
    }
}

pub fn spawn(sink: EventSink) -> MediaControl {
    let (tx, rx) = channel();
    let worker_tx = tx.clone();
    let _ = std::thread::Builder::new().name("media".into()).stack_size(512 * 1024).spawn(move || {
        if let Err(e) = run(sink, worker_tx, rx) {
            log::warn!("media provider stopped: {e}");
        }
    });
    MediaControl { tx }
}

struct Bound {
    session: Session,
    tokens: [i64; 3],
}

fn run(sink: EventSink, tx: Sender<Msg>, rx: Receiver<Msg>) -> Result<()> {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let wic: IWICImagingFactory = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? };
    let manager = Manager::RequestAsync()?.join()?;
    {
        let tx = tx.clone();
        manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
            let _ = tx.send(Msg::SessionChanged);
            Ok(())
        }))?;
    }

    let mut bound: Option<Bound> = None;
    let mut last_info: Option<MediaInfo> = None;
    let mut art_key = String::new();
    let mut pending = Some(Msg::SessionChanged);

    loop {
        let msg = match pending.take() {
            Some(m) => m,
            None => match rx.recv() {
                Ok(m) => m,
                Err(_) => return Ok(()),
            },
        };
        // Coalesce bursts (SMTC fires several events per track change).
        let mut session_changed = matches!(msg, Msg::SessionChanged);
        let mut refresh = matches!(msg, Msg::Refresh) || session_changed;
        if let Msg::Cmd(cmd) = msg {
            if let Some(b) = &bound {
                command(&b.session, cmd);
            }
        }
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::SessionChanged => session_changed = true,
                Msg::Refresh => refresh = true,
                Msg::Cmd(cmd) => {
                    if let Some(b) = &bound {
                        command(&b.session, cmd);
                    }
                }
            }
        }

        if session_changed {
            if let Some(old) = bound.take() {
                let _ = old.session.RemoveMediaPropertiesChanged(old.tokens[0]);
                let _ = old.session.RemovePlaybackInfoChanged(old.tokens[1]);
                let _ = old.session.RemoveTimelinePropertiesChanged(old.tokens[2]);
            }
            bound = manager.GetCurrentSession().ok().and_then(|s| bind(s, &tx).ok());
            refresh = true;
        }

        if refresh {
            let info = bound.as_ref().and_then(|b| read(&b.session).ok());
            if info != last_info {
                // Ignore pure position re-samples while paused to avoid redundant redraws.
                sink.send(AppEvent::Media(info.clone()));
                last_info = info.clone();
            }
            match (&bound, &info) {
                (Some(b), Some(i)) if i.track_key != art_key => match artwork(&b.session, &wic, &i.track_key) {
                    Ok(Some(a)) => {
                        art_key = i.track_key.clone();
                        sink.send(AppEvent::Artwork(Some(Arc::new(a))));
                    }
                    Ok(None) => {}
                    Err(e) => log::debug!("artwork: {e}"),
                },
                (_, None) => art_key.clear(),
                _ => {}
            }
        }
    }
}

fn bind(session: Session, tx: &Sender<Msg>) -> Result<Bound> {
    let t = |tx: &Sender<Msg>| {
        let tx = tx.clone();
        move || {
            let _ = tx.send(Msg::Refresh);
        }
    };
    let (a, b, c) = (t(tx), t(tx), t(tx));
    let tokens = [
        session.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
            a();
            Ok(())
        }))?,
        session.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
            b();
            Ok(())
        }))?,
        session.TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
            c();
            Ok(())
        }))?,
    ];
    Ok(Bound { session, tokens })
}

fn command(session: &Session, cmd: MediaCmd) {
    let r = match cmd {
        MediaCmd::TogglePlayPause => session.TryTogglePlayPauseAsync(),
        MediaCmd::Next => session.TrySkipNextAsync(),
        MediaCmd::Previous => session.TrySkipPreviousAsync(),
    };
    // Don't block the worker on the result; the change event will follow.
    if let Err(e) = r {
        log::debug!("media command {cmd:?}: {e}");
    }
}

/// 100 ns ticks between 1601-01-01 and 1970-01-01.
const EPOCH_DIFF_TICKS: i64 = 116_444_736_000_000_000;

fn read(session: &Session) -> Result<MediaInfo> {
    let props = session.TryGetMediaPropertiesAsync()?.join()?;
    let title = props.Title()?.to_string();
    let artist = props.Artist()?.to_string();
    let playing = session.GetPlaybackInfo()?.PlaybackStatus()? == Status::Playing;
    let app = pretty_app(&session.SourceAppUserModelId()?.to_string());

    let (mut position_ms, mut duration_ms, mut sampled_at) = (0, 0, now_ms());
    if let Ok(tl) = session.GetTimelineProperties() {
        let start = tl.StartTime().map(|t| t.Duration).unwrap_or(0);
        let end = tl.EndTime().map(|t| t.Duration).unwrap_or(0);
        duration_ms = (end - start).max(0) / 10_000;
        position_ms = tl.Position().map(|t| t.Duration).unwrap_or(0).max(0) / 10_000;
        let updated = tl.LastUpdatedTime().map(|d| d.UniversalTime).unwrap_or(0);
        if updated > EPOCH_DIFF_TICKS {
            sampled_at = (updated - EPOCH_DIFF_TICKS) / 10_000;
        }
    }
    if title.is_empty() && artist.is_empty() {
        return Err(windows::core::Error::from(windows::Win32::Foundation::E_FAIL));
    }
    let track_key = format!("{app}\u{1}{title}\u{1}{artist}");
    Ok(MediaInfo { title, artist, app, playing, position_ms, duration_ms, sampled_at, track_key })
}

fn pretty_app(aumid: &str) -> String {
    let s = aumid.rsplit('!').next().unwrap_or(aumid);
    let s = s.rsplit(['\\', '/']).next().unwrap_or(s);
    let s = s.strip_suffix(".exe").or_else(|| s.strip_suffix(".EXE")).unwrap_or(s);
    let s = s.rsplit('.').next().filter(|x| !x.is_empty()).unwrap_or(s);
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Decode the session thumbnail to a centred square of premultiplied BGRA.
fn artwork(session: &Session, wic: &IWICImagingFactory, key: &str) -> Result<Option<Artwork>> {
    let props = session.TryGetMediaPropertiesAsync()?.join()?;
    let Ok(thumb) = props.Thumbnail() else { return Ok(None) };
    let stream = thumb.OpenReadAsync()?.join()?;
    unsafe {
        let istream: IStream = CreateStreamOverRandomAccessStream(&stream)?;
        let decoder = wic.CreateDecoderFromStream(&istream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
        let frame = decoder.GetFrame(0)?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h)?;
        if w == 0 || h == 0 {
            return Ok(None);
        }
        // Centre-crop to square (many players pad album art to 16:9).
        let side = w.min(h);
        let clipper = wic.CreateBitmapClipper()?;
        let rect = WICRect { X: ((w - side) / 2) as i32, Y: ((h - side) / 2) as i32, Width: side as i32, Height: side as i32 };
        clipper.Initialize(&frame, &rect)?;
        let scaler = wic.CreateBitmapScaler()?;
        scaler.Initialize(&clipper, ART_PX, ART_PX, WICBitmapInterpolationModeFant)?;
        let conv = wic.CreateFormatConverter()?;
        conv.Initialize(&scaler, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let stride = ART_PX * 4;
        let mut pixels = vec![0u8; (stride * ART_PX) as usize];
        conv.CopyPixels(std::ptr::null(), stride, &mut pixels)?;
        Ok(Some(Artwork { track_key: key.to_string(), width: ART_PX, height: ART_PX, pixels }))
    }
}

#[cfg(test)]
mod tests {
    use super::pretty_app;

    #[test]
    fn app_names() {
        assert_eq!(pretty_app("Spotify.exe"), "Spotify");
        assert_eq!(pretty_app("Chrome"), "Chrome");
        assert_eq!(pretty_app("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"), "ZuneMusic");
        assert_eq!(pretty_app("C:\\Apps\\foobar2000.exe"), "Foobar2000");
    }
}
