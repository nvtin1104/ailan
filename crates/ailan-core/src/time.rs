//! Minimal civil-time helpers (no chrono dependency).

pub const MINUTE: i64 = 60_000;
pub const HOUR: i64 = 60 * MINUTE;
pub const DAY: i64 = 24 * HOUR;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i32,
    pub month: u32, // 1..=12
    pub day: u32,   // 1..=31
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 = Monday … 6 = Sunday
    pub weekday: u32,
}

/// Days since 1970-01-01 → (y, m, d). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

/// Local civil time for `epoch_ms` at UTC offset `offset_min`.
pub fn civil(epoch_ms: i64, offset_min: i32) -> Civil {
    let local = epoch_ms + offset_min as i64 * MINUTE;
    let days = local.div_euclid(DAY);
    let ms_of_day = local.rem_euclid(DAY);
    let (year, month, day) = civil_from_days(days);
    // 1970-01-01 was a Thursday (weekday 3 with Monday = 0).
    let weekday = (days + 3).rem_euclid(7) as u32;
    Civil {
        year,
        month,
        day,
        hour: (ms_of_day / HOUR) as u32,
        minute: (ms_of_day % HOUR / MINUTE) as u32,
        second: (ms_of_day % MINUTE / 1000) as u32,
        weekday,
    }
}

/// Local day number (days since epoch in local time) — used for "today" resets.
pub fn local_day(epoch_ms: i64, offset_min: i32) -> i64 {
    (epoch_ms + offset_min as i64 * MINUTE).div_euclid(DAY)
}

/// Epoch ms of local midnight (00:00:00.000) for the given timestamp.
pub fn local_midnight(epoch_ms: i64, offset_min: i32) -> i64 {
    let off = offset_min as i64 * MINUTE;
    let local = epoch_ms + off;
    (local.div_euclid(DAY) * DAY) - off
}

/// Number of days in a given Gregorian month (1..=12).
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
            if leap { 29 } else { 28 }
        }
        _ => 30,
    }
}

/// Days since 1970-01-01 from civil (year, month, day).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe as i32 - 719_468) as i64
}

/// Convert civil date & time into UTC epoch milliseconds.
pub fn make_epoch(year: i32, month: u32, day: u32, hour: u32, minute: u32, offset_min: i32) -> i64 {
    let days = days_from_civil(year, month, day);
    let ms_of_day = hour as i64 * HOUR + minute as i64 * MINUTE;
    let off = offset_min as i64 * MINUTE;
    (days * DAY + ms_of_day) - off
}

/// Epoch ms of the next local time `hour:minute` strictly after `now`.
pub fn next_local_time(now: i64, offset_min: i32, hour: u32, minute: u32) -> i64 {
    let off = offset_min as i64 * MINUTE;
    let local_now = now + off;
    let day_start = local_now.div_euclid(DAY) * DAY;
    let mut t = day_start + hour as i64 * HOUR + minute as i64 * MINUTE;
    if t <= local_now {
        t += DAY;
    }
    t - off
}

/// Epoch ms of the next whole-minute boundary after `now`.
pub fn next_minute(now: i64) -> i64 {
    (now.div_euclid(MINUTE) + 1) * MINUTE
}

/// Format milliseconds as `m:ss` (or `h:mm:ss`).
pub fn fmt_duration(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_known_date() {
        // 2026-11-10 07:05:09 UTC is a Tuesday.
        let t = 1_794_294_309_000;
        let c = civil(t, 0);
        assert_eq!((c.year, c.month, c.day), (2026, 11, 10));
        assert_eq!((c.hour, c.minute, c.second), (7, 5, 9));
        assert_eq!(c.weekday, 1);
        // UTC+7 → 14:05
        let c7 = civil(t, 420);
        assert_eq!((c7.hour, c7.minute), (14, 5));
    }

    #[test]
    fn epoch_is_thursday() {
        assert_eq!(civil(0, 0).weekday, 3);
        assert_eq!(civil(0, 0).year, 1970);
    }

    #[test]
    fn next_local_time_today_and_tomorrow() {
        let t = 1_794_294_309_000; // 14:05 local at +7
        let later = next_local_time(t, 420, 15, 30);
        let c = civil(later, 420);
        assert_eq!((c.day, c.hour, c.minute), (10, 15, 30));
        let earlier = next_local_time(t, 420, 9, 0);
        let c = civil(earlier, 420);
        assert_eq!((c.day, c.hour, c.minute), (11, 9, 0));
    }

    #[test]
    fn minute_boundary_and_format() {
        assert_eq!(next_minute(61_000), 120_000);
        assert_eq!(next_minute(120_000), 180_000);
        assert_eq!(fmt_duration(1_499_000), "24:59");
        assert_eq!(fmt_duration(3_725_000), "1:02:05");
    }
}
