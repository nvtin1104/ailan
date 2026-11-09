//! Volume / mute and default-device changes through Core Audio callbacks.

use ailan_core::AppEvent;
use windows::core::{implement, Result, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    eConsole, eRender, EDataFlow, ERole, IMMDevice, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator, PKEY_AudioEndpoint_FormFactor, AUDIO_VOLUME_NOTIFICATION_DATA,
    DEVICE_STATE,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, STGM_READ};

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

pub struct Audio {
    sink: EventSink,
    enumerator: IMMDeviceEnumerator,
    device_cb: IMMNotificationClient,
    endpoint: Option<(IAudioEndpointVolume, IAudioEndpointVolumeCallback)>,
}

impl Audio {
    pub fn new(sink: EventSink) -> Result<Self> {
        let enumerator: IMMDeviceEnumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        let device_cb: IMMNotificationClient = DeviceCallback { sink: sink.clone() }.into();
        unsafe { enumerator.RegisterEndpointNotificationCallback(&device_cb)? };
        let mut a = Self { sink, enumerator, device_cb, endpoint: None };
        a.rebind();
        Ok(a)
    }

    /// (Re)attach to the current default output device and publish its state.
    pub fn rebind(&mut self) {
        self.unbind();
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
            self.endpoint = Some((volume, cb));
        }
        Ok(())
    }

    fn unbind(&mut self) {
        if let Some((volume, cb)) = self.endpoint.take() {
            unsafe {
                let _ = volume.UnregisterControlChangeNotify(&cb);
            }
        }
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.unbind();
        unsafe {
            let _ = self.enumerator.UnregisterEndpointNotificationCallback(&self.device_cb);
        }
    }
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
