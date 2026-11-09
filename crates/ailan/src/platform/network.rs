//! Connectivity and Wi-Fi via WinRT `NetworkInformation.NetworkStatusChanged`.

use ailan_core::event::Network;
use ailan_core::AppEvent;
use windows::Networking::Connectivity::{NetworkConnectivityLevel, NetworkInformation, NetworkStatusChangedEventHandler};

use super::EventSink;

pub fn spawn(sink: EventSink) {
    // Initial snapshot off the UI thread (WinRT calls may block briefly).
    let _ = std::thread::Builder::new().name("network".into()).stack_size(256 * 1024).spawn(move || {
        sink.send(AppEvent::Network(snapshot()));
        let s2 = sink.clone();
        let handler = NetworkStatusChangedEventHandler::new(move |_| {
            s2.send(AppEvent::Network(snapshot()));
            Ok(())
        });
        if let Err(e) = NetworkInformation::NetworkStatusChanged(&handler) {
            log::warn!("network events: {e}");
        }
        // Registration is static; the handler lives as long as the process.
    });
}

fn snapshot() -> Network {
    let Ok(profile) = NetworkInformation::GetInternetConnectionProfile() else {
        return Network { connected: false, wifi: None };
    };
    let connected = profile
        .GetNetworkConnectivityLevel()
        .map(|l| l == NetworkConnectivityLevel::InternetAccess || l == NetworkConnectivityLevel::ConstrainedInternetAccess)
        .unwrap_or(false);
    let wifi = if profile.IsWlanConnectionProfile().unwrap_or(false) {
        let ssid = profile
            .WlanConnectionProfileDetails()
            .and_then(|d| d.GetConnectedSsid())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let bars = profile.GetSignalBars().and_then(|r| r.Value()).unwrap_or(0);
        Some((ssid, bars))
    } else {
        None
    };
    Network { connected, wifi }
}
