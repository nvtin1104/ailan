//! Display brightness changes via WMI `WmiMonitorBrightnessEvent` (laptops).
//! Runs on its own MTA thread that just parks after subscribing.

use ailan_core::AppEvent;
use windows::core::{implement, Result, BSTR, HRESULT};
use windows::Win32::System::Variant::{VariantClear, VARIANT, VT_I2, VT_I4, VT_UI1, VT_UI2, VT_UI4};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, EOAC_NONE,
    RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemObjectSink, IWbemObjectSink_Impl, IWbemServices, WbemLocator,
    WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE, WBEM_INFINITE,
};

use super::EventSink;

#[implement(IWbemObjectSink)]
struct Sink {
    sink: EventSink,
}

impl IWbemObjectSink_Impl for Sink_Impl {
    fn Indicate(&self, count: i32, objs: *const Option<IWbemClassObject>) -> Result<()> {
        if objs.is_null() || count <= 0 {
            return Ok(());
        }
        let objs = unsafe { std::slice::from_raw_parts(objs, count as usize) };
        if let Some(b) = objs.iter().flatten().last().and_then(|o| read_u8(o, "Brightness")) {
            self.sink.send(AppEvent::Brightness(b));
        }
        Ok(())
    }

    fn SetStatus(&self, _: i32, _: HRESULT, _: &BSTR, _: windows::core::Ref<IWbemClassObject>) -> Result<()> {
        Ok(())
    }
}

fn read_u8(obj: &IWbemClassObject, name: &str) -> Option<u8> {
    let mut v = VARIANT::default();
    let name = super::wide(name);
    unsafe {
        obj.Get(windows::core::PCWSTR(name.as_ptr()), 0, &mut v, None, None).ok()?;
        let inner = &v.Anonymous.Anonymous;
        let val = match inner.vt {
            VT_UI1 => Some(inner.Anonymous.bVal as i32),
            VT_I2 | VT_UI2 => Some(inner.Anonymous.iVal as i32),
            VT_I4 | VT_UI4 => Some(inner.Anonymous.lVal),
            _ => None,
        };
        let _ = VariantClear(&mut v);
        val.map(|x| x.clamp(0, 100) as u8)
    }
}

pub fn spawn(sink: EventSink) {
    let _ = std::thread::Builder::new().name("brightness".into()).stack_size(256 * 1024).spawn(move || {
        match subscribe(sink) {
            // Keep the COM objects alive for the life of the process.
            Ok(keep) => {
                let _keep = keep;
                loop {
                    std::thread::park();
                }
            }
            Err(e) => log::info!("brightness events unavailable: {e}"),
        }
    });
}

fn subscribe(sink: EventSink) -> Result<(IWbemServices, IWbemObjectSink)> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
        let services =
            locator.ConnectServer(&BSTR::from("ROOT\\WMI"), &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None)?;
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            None,
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )?;

        // Initial value, so the first real change shows the OSD.
        let wql = BSTR::from("WQL");
        let query = services.ExecQuery(
            &wql,
            &BSTR::from("SELECT CurrentBrightness FROM WmiMonitorBrightness"),
            WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_FORWARD_ONLY.0 | WBEM_FLAG_RETURN_IMMEDIATELY.0),
            None,
        )?;
        let mut objs = [None];
        let mut returned = 0u32;
        let _ = query.Next(WBEM_INFINITE, &mut objs, &mut returned);
        match objs[0].as_ref().and_then(|o| read_u8(o, "CurrentBrightness")) {
            Some(b) => sink.send(AppEvent::Brightness(b)),
            None => return Err(windows::core::Error::from(windows::Win32::Foundation::E_NOTIMPL)),
        }

        let obj_sink: IWbemObjectSink = Sink { sink }.into();
        services.ExecNotificationQueryAsync(
            &wql,
            &BSTR::from("SELECT * FROM WmiMonitorBrightnessEvent"),
            WBEM_GENERIC_FLAG_TYPE(0),
            None,
            &obj_sink,
        )?;
        Ok((services, obj_sink))
    }
}
