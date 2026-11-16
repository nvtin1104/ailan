//! Volume / mute and default-device changes through Core Audio callbacks,
//! plus an on-demand WASAPI loopback 5-band spectrum analyzer for real-time music bars.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use ailan_core::AppEvent;
use windows::core::{implement, Result, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl, IAudioMeterInformation,
};
use windows::Win32::Media::Audio::{
    eConsole, eRender, EDataFlow, ERole, IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceEnumerator,
    IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator, PKEY_AudioEndpoint_FormFactor,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIO_VOLUME_NOTIFICATION_DATA,
    DEVICE_STATE, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, WAVE_FORMAT_PCM,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};

use super::{EventSink, WM_APP_AUDIO_DEVICE};

#[implement(IAudioEndpointVolumeCallback)]
struct VolumeCallback {
    sink: EventSink,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeCallback_Impl {
    fn OnNotify(&self, data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        if let Some(d) = unsafe { data.as_ref() } {
            self.sink.send(AppEvent::Volume { level: d.fMasterVolume, muted: d.bMuted.as_bool() });
        }
        Ok(())
    }
}

#[implement(IMMNotificationClient)]
struct DeviceCallback {
    sink: EventSink,
}

impl IMMNotificationClient_Impl for DeviceCallback_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> Result<()> {
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _: &PCWSTR) -> Result<()> {
        if flow == eRender && role == eConsole {
            // Never call back into Core Audio from here; rebind on the UI thread.
            self.sink.post(WM_APP_AUDIO_DEVICE);
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> Result<()> {
        Ok(())
    }
}

struct SpectrumShared {
    bands: [AtomicU32; 5],
    has_signal: AtomicBool,
    rebind: AtomicBool,
    state: Mutex<(bool, bool)>, // (active, quit)
    wake: Condvar,
}

impl SpectrumShared {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            bands: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            has_signal: AtomicBool::new(false),
            rebind: AtomicBool::new(false),
            state: Mutex::new((false, false)),
            wake: Condvar::new(),
        })
    }

    fn set_bands(&self, vals: [f32; 5], active_signal: bool) {
        for (slot, v) in self.bands.iter().zip(vals) {
            slot.store(v.to_bits(), Ordering::Relaxed);
        }
        self.has_signal.store(active_signal, Ordering::Relaxed);
    }

    fn read_bands(&self) -> Option<[f32; 5]> {
        if !self.has_signal.load(Ordering::Relaxed) {
            return None;
        }
        let mut out = [0.0f32; 5];
        for (dst, slot) in out.iter_mut().zip(&self.bands) {
            *dst = f32::from_bits(slot.load(Ordering::Relaxed));
        }
        Some(out)
    }
}

pub struct Audio {
    sink: EventSink,
    enumerator: IMMDeviceEnumerator,
    device_cb: IMMNotificationClient,
    endpoint: Option<(IAudioEndpointVolume, IAudioEndpointVolumeCallback)>,
    meter: Option<IAudioMeterInformation>,
    spectrum: Arc<SpectrumShared>,
}

impl Audio {
    pub fn new(sink: EventSink) -> Result<Self> {
        let enumerator: IMMDeviceEnumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        let device_cb: IMMNotificationClient = DeviceCallback { sink: sink.clone() }.into();
        unsafe { enumerator.RegisterEndpointNotificationCallback(&device_cb)? };
        let spectrum = SpectrumShared::new();
        spawn_spectrum_worker(Arc::clone(&spectrum));
        let mut a = Self { sink, enumerator, device_cb, endpoint: None, meter: None, spectrum };
        a.rebind();
        Ok(a)
    }

    /// Turn on/off the background WASAPI loopback capture (sleeps at 0% CPU when off).
    pub fn set_spectrum_active(&self, active: bool) {
        let Ok(mut g) = self.spectrum.state.lock() else { return };
        if g.0 != active {
            g.0 = active;
            if !active {
                self.spectrum.set_bands([0.0; 5], false);
            }
            self.spectrum.wake.notify_one();
        }
    }

    /// Returns real-time 5-band spectrum levels (0..=1) from WASAPI loopback if available.
    pub fn spectrum_bands(&self) -> Option<[f32; 5]> {
        self.spectrum.read_bands()
    }

    /// (Re)attach to the current default output device and publish its state.
    pub fn rebind(&mut self) {
        self.unbind();
        self.spectrum.rebind.store(true, Ordering::Relaxed);
        self.spectrum.wake.notify_one();
        match self.bind() {
            Ok(()) => {}
            Err(e) => log::warn!("audio endpoint: {e}"),
        }
    }

    fn bind(&mut self) -> Result<()> {
        unsafe {
            let device = self.enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let (name, headphones) = describe(&device);
            let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
            let cb: IAudioEndpointVolumeCallback = VolumeCallback { sink: self.sink.clone() }.into();
            volume.RegisterControlChangeNotify(&cb)?;
            let level = volume.GetMasterVolumeLevelScalar()?;
            let muted = volume.GetMute()?.as_bool();
            self.sink.send(AppEvent::AudioDevice { name, headphones });
            self.sink.send(AppEvent::Volume { level, muted });
            self.meter = device.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None).ok();
            self.endpoint = Some((volume, cb));
        }
        Ok(())
    }

    /// Reads real-time output peaks. Kept for a later visual; the island no longer draws audio bars.
    #[allow(dead_code)]
    pub fn sample_peaks(&self) -> (f32, [f32; 2]) {
        let Some(meter) = &self.meter else {
            return (0.0, [0.0, 0.0]);
        };
        unsafe {
            let peak = meter.GetPeakValue().unwrap_or(0.0).clamp(0.0, 1.0);
            let mut ch = [0.0f32; 8];
            let n = meter.GetMeteringChannelCount().unwrap_or(0).min(8) as usize;
            if n >= 2 && meter.GetChannelsPeakValues(&mut ch[..n]).is_ok() {
                (peak, [ch[0].clamp(0.0, 1.0), ch[1].clamp(0.0, 1.0)])
            } else {
                (peak, [peak, peak])
            }
        }
    }

    fn unbind(&mut self) {
        self.meter = None;
        if let Some((volume, cb)) = self.endpoint.take() {
            unsafe {
                let _ = volume.UnregisterControlChangeNotify(&cb);
            }
        }
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        if let Ok(mut g) = self.spectrum.state.lock() {
            g.1 = true;
            self.spectrum.wake.notify_all();
        }
        self.unbind();
        unsafe {
            let _ = self.enumerator.UnregisterEndpointNotificationCallback(&self.device_cb);
        }
    }
}

fn spawn_spectrum_worker(shared: Arc<SpectrumShared>) {
    let _ = std::thread::Builder::new().name("audio-spectrum".into()).stack_size(256 * 1024).spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        loop {
            {
                let Ok(mut g) = shared.state.lock() else { break };
                while !g.0 && !g.1 {
                    g = match shared.wake.wait(g) {
                        Ok(g) => g,
                        Err(_) => return,
                    };
                }
                if g.1 {
                    break;
                }
            }
            shared.rebind.store(false, Ordering::Relaxed);
            if let Err(e) = run_loopback_session(&shared) {
                log::debug!("audio loopback stopped: {e}");
                shared.set_bands([0.0; 5], false);
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        unsafe {
            CoUninitialize();
        }
    });
}

const WAVE_FORMAT_IEEE_FLOAT_TAG: u16 = 0x0003;
const WAVE_FORMAT_EXTENSIBLE_TAG: u16 = 0xFFFE;
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: windows::core::GUID =
    windows::core::GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

unsafe fn is_float_format(fmt: *const WAVEFORMATEX) -> bool {
    let tag = (*fmt).wFormatTag;
    if tag == WAVE_FORMAT_IEEE_FLOAT_TAG {
        return true;
    }
    if tag == WAVE_FORMAT_EXTENSIBLE_TAG {
        let ext = fmt as *const WAVEFORMATEXTENSIBLE;
        let sub = std::ptr::addr_of!((*ext).SubFormat).read_unaligned();
        return sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
    }
    false
}

fn run_loopback_session(shared: &SpectrumShared) -> Result<()> {
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
        let fmt = client.GetMixFormat()?;
        let channels = (*fmt).nChannels.max(1) as usize;
        let sample_rate = (*fmt).nSamplesPerSec.max(8000) as f32;
        let bits = (*fmt).wBitsPerSample;
        let is_float = is_float_format(fmt);
        let is_pcm16 = !is_float && (*fmt).wFormatTag == WAVE_FORMAT_PCM as u16 && bits == 16;

        let init_res = client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            200_000, // 20 ms buffer (in 100ns units)
            0,
            fmt,
            None,
        );
        CoTaskMemFree(Some(fmt as *const _));
        init_res?;

        let capture: IAudioCaptureClient = client.GetService()?;
        client.Start()?;

        const N: usize = 512;
        let mut ring = [0.0f32; N];
        let mut pos = 0usize;
        let mut agc = [0.08f32; 5];

        loop {
            {
                let Ok(g) = shared.state.lock() else { break };
                if !g.0 || g.1 || shared.rebind.load(Ordering::Relaxed) {
                    break;
                }
            }

            let mut got_frames = false;
            loop {
                let pkt_len = capture.GetNextPacketSize()?;
                if pkt_len == 0 {
                    break;
                }
                let mut data_ptr = std::ptr::null_mut();
                let mut num_frames = 0u32;
                let mut flags = 0u32;
                capture.GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None)?;
                let n_frames = num_frames as usize;
                let silent = (flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0;
                if silent || data_ptr.is_null() {
                    for _ in 0..n_frames {
                        ring[pos] = 0.0;
                        pos = (pos + 1) % N;
                    }
                } else if is_float && bits == 32 {
                    let samples = std::slice::from_raw_parts(data_ptr as *const f32, n_frames * channels);
                    for frame in samples.chunks_exact(channels) {
                        let sum: f32 = frame.iter().copied().sum();
                        ring[pos] = sum / channels as f32;
                        pos = (pos + 1) % N;
                    }
                } else if is_pcm16 {
                    let samples = std::slice::from_raw_parts(data_ptr as *const i16, n_frames * channels);
                    for frame in samples.chunks_exact(channels) {
                        let sum: f32 = frame.iter().map(|&s| s as f32 / 32768.0).sum();
                        ring[pos] = sum / channels as f32;
                        pos = (pos + 1) % N;
                    }
                }
                let _ = capture.ReleaseBuffer(num_frames);
                got_frames = true;
            }

            if got_frames {
                let bands = compute_5band_spectrum(&ring, pos, sample_rate, &mut agc);
                shared.set_bands(bands, true);
            }
            std::thread::sleep(Duration::from_millis(14));
        }

        let _ = client.Stop();
        Ok(())
    }
}

fn goertzel_mag(ordered: &[f32], sample_rate: f32, target_hz: f32) -> f32 {
    let n = ordered.len();
    let omega = 2.0 * std::f32::consts::PI * target_hz / sample_rate;
    let coeff = 2.0 * omega.cos();
    let (mut q1, mut q2) = (0.0f32, 0.0f32);
    for (i, &x) in ordered.iter().enumerate() {
        // Hann window reduces spectral leakage across bands.
        let win = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (n as f32 - 1.0)).cos());
        let q0 = coeff * q1 - q2 + x * win;
        q2 = q1;
        q1 = q0;
    }
    let mag_sq = (q1 * q1 + q2 * q2 - q1 * q2 * coeff).max(0.0);
    mag_sq.sqrt() / (n as f32 * 0.25)
}

fn compute_5band_spectrum(ring: &[f32; 512], pos: usize, sample_rate: f32, agc: &mut [f32; 5]) -> [f32; 5] {
    let mut ordered = [0.0f32; 512];
    let tail = 512 - pos;
    ordered[..tail].copy_from_slice(&ring[pos..]);
    ordered[tail..].copy_from_slice(&ring[..pos]);

    // 5 perceptual bands: Bass, Low-Mid, Mid (Vocals), High-Mid, Treble.
    const BAND_FREQS: [[f32; 2]; 5] = [
        [65.0, 120.0],
        [220.0, 380.0],
        [650.0, 1100.0],
        [1900.0, 3200.0],
        [5200.0, 8200.0],
    ];
    let mut out = [0.0f32; 5];
    for i in 0..5 {
        let e = 0.5 * (goertzel_mag(&ordered, sample_rate, BAND_FREQS[i][0])
            + goertzel_mag(&ordered, sample_rate, BAND_FREQS[i][1]));
        // Fast attack, slow decay per-band AGC so each band stays expressive at any volume.
        if e > agc[i] {
            agc[i] = agc[i] * 0.65 + e * 0.35;
        } else {
            agc[i] = (agc[i] * 0.992).max(0.015);
        }
        let norm = (e / agc[i].max(0.015)).clamp(0.0, 1.4);
        out[i] = norm.powf(0.85).clamp(0.08, 1.0);
    }
    out
}

/// Friendly name and whether the endpoint is headphones/headset.
fn describe(device: &IMMDevice) -> (String, bool) {
    unsafe {
        let Ok(store) = device.OpenPropertyStore(STGM_READ) else {
            return (String::new(), false);
        };
        let name = store.GetValue(&PKEY_Device_FriendlyName).map(|v| v.to_string()).unwrap_or_default();
        // EndpointFormFactor: Headphones = 3, Headset = 5.
        let ff = store.GetValue(&PKEY_AudioEndpoint_FormFactor).ok().and_then(|v| u32::try_from(&v).ok()).unwrap_or(0);
        (name, ff == 3 || ff == 5)
    }
}
