//! DST-aware UTC offsets for Windows time-zone ids (world clock), using the OS time-zone
//! database — no bundled tz data.

use std::collections::HashMap;

use ailan_core::time::{civil, make_epoch, MINUTE};
use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::Time::{
    EnumDynamicTimeZoneInformation, SystemTimeToTzSpecificLocalTimeEx, DYNAMIC_TIME_ZONE_INFORMATION,
};

#[derive(Default)]
pub struct TzCache {
    zones: HashMap<String, Option<DYNAMIC_TIME_ZONE_INFORMATION>>,
}

fn key_name(z: &DYNAMIC_TIME_ZONE_INFORMATION) -> String {
    let n = z.TimeZoneKeyName.iter().position(|&c| c == 0).unwrap_or(z.TimeZoneKeyName.len());
    String::from_utf16_lossy(&z.TimeZoneKeyName[..n])
}

fn find_zone(id: &str) -> Option<DYNAMIC_TIME_ZONE_INFORMATION> {
    for i in 0..1024u32 {
        let mut z = DYNAMIC_TIME_ZONE_INFORMATION::default();
        if unsafe { EnumDynamicTimeZoneInformation(i, &mut z) } != 0 {
            break;
        }
        if key_name(&z).eq_ignore_ascii_case(id) {
            return Some(z);
        }
    }
    None
}

impl TzCache {
    /// UTC offset in minutes of time zone `id` at `now_ms`, or `None` if unknown.
    pub fn offset_min(&mut self, id: &str, now_ms: i64) -> Option<i32> {
        let zone = self.zones.entry(id.to_string()).or_insert_with(|| find_zone(id)).as_ref()?;
        let c = civil(now_ms, 0);
        let utc = SYSTEMTIME {
            wYear: c.year as u16,
            wMonth: c.month as u16,
            wDayOfWeek: 0,
            wDay: c.day as u16,
            wHour: c.hour as u16,
            wMinute: c.minute as u16,
            wSecond: 0,
            wMilliseconds: 0,
        };
        let mut local = SYSTEMTIME::default();
        unsafe { SystemTimeToTzSpecificLocalTimeEx(Some(zone), &utc, &mut local) }.ok()?;
        let l = make_epoch(local.wYear as i32, local.wMonth as u32, local.wDay as u32, local.wHour as u32, local.wMinute as u32, 0);
        let u = make_epoch(c.year, c.month, c.day, c.hour, c.minute, 0);
        Some(((l - u) / MINUTE) as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::TzCache;
    use ailan_core::time::make_epoch;

    #[test]
    fn known_zones_and_dst() {
        let mut tz = TzCache::default();
        let jan = make_epoch(2026, 1, 15, 12, 0, 0);
        let jul = make_epoch(2026, 7, 15, 12, 0, 0);
        assert_eq!(tz.offset_min("Tokyo Standard Time", jan), Some(540));
        assert_eq!(tz.offset_min("SE Asia Standard Time", jul), Some(420));
        // New York: EST in winter, EDT in summer.
        assert_eq!(tz.offset_min("Eastern Standard Time", jan), Some(-300));
        assert_eq!(tz.offset_min("Eastern Standard Time", jul), Some(-240));
        assert_eq!(tz.offset_min("Not A Zone", jan), None);
    }
}
