//! Reminders calculation, repeat scheduling and human formatting.

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;
use crate::time::{civil, local_midnight, DAY};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Repeat {
    #[default]
    None,
    Daily,
    Weekdays,
    /// Bitmask for days of week: bit 0 = Monday (Thứ 2), bit 1 = Tuesday ... bit 6 = Sunday (Chủ nhật).
    Weekly(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayBucket {
    Overdue,
    Today,
    Tomorrow,
    ThisWeek,
    Later,
}

/// Classify a due time into a relative bucket based on current local day.
pub fn day_bucket(due: i64, now: i64, offset_min: i32) -> DayBucket {
    let today_start = local_midnight(now, offset_min);
    let tomorrow_start = today_start + DAY;
    let day_after_tomorrow = tomorrow_start + DAY;
    let week_end = today_start + 7 * DAY;

    if due < today_start {
        DayBucket::Overdue
    } else if due < tomorrow_start {
        DayBucket::Today
    } else if due < day_after_tomorrow {
        DayBucket::Tomorrow
    } else if due < week_end {
        DayBucket::ThisWeek
    } else {
        DayBucket::Later
    }
}

/// Calculate the next occurrence timestamp after `now` for a repeating reminder.
pub fn next_occurrence(due: i64, repeat: Repeat, now: i64, offset_min: i32) -> Option<i64> {
    match repeat {
        Repeat::None => None,
        Repeat::Daily => {
            let mut next = due;
            while next <= now {
                next += DAY;
            }
            Some(next)
        }
        Repeat::Weekdays => {
            let mut next = due;
            while next <= now {
                next += DAY;
                let c = civil(next, offset_min);
                // If Saturday (5) or Sunday (6), keep rolling forward until Monday (0)
                if c.weekday == 5 {
                    next += 2 * DAY;
                } else if c.weekday == 6 {
                    next += DAY;
                }
            }
            // Double check weekday
            let c = civil(next, offset_min);
            if c.weekday == 5 {
                next += 2 * DAY;
            } else if c.weekday == 6 {
                next += DAY;
            }
            Some(next)
        }
        Repeat::Weekly(mask) => {
            if mask == 0 {
                return None;
            }
            let mut next = due;
            let mut loops = 0;
            loop {
                next += DAY;
                loops += 1;
                if loops > 366 {
                    return None;
                }
                let c = civil(next, offset_min);
                let bit = 1u8 << (c.weekday % 7);
                if (mask & bit) != 0 && next > now {
                    return Some(next);
                }
            }
        }
    }
}

/// Friendly format for a due time (e.g. "Hôm nay 15:30", "Ngày mai 09:00", "T3, 10/11 14:00").
pub fn fmt_due(due: i64, all_day: bool, now: i64, offset_min: i32, lang: Lang) -> String {
    let bucket = day_bucket(due, now, offset_min);
    let c = civil(due, offset_min);
    let time_str = if all_day {
        String::new()
    } else {
        format!(" {:02}:{:02}", c.hour, c.minute)
    };

    match lang {
        Lang::Vi => match bucket {
            DayBucket::Overdue => format!("Quá hạn ({:02}/{}{})", c.day, c.month, time_str),
            DayBucket::Today => format!("Hôm nay{}", time_str),
            DayBucket::Tomorrow => format!("Ngày mai{}", time_str),
            DayBucket::ThisWeek | DayBucket::Later => {
                let wd = crate::i18n::weekday(lang, c.weekday);
                format!("{}, {:02}/{}{}", wd, c.day, c.month, time_str)
            }
        },
        Lang::En => match bucket {
            DayBucket::Overdue => format!("Overdue ({:02}/{}{})", c.month, c.day, time_str),
            DayBucket::Today => format!("Today{}", time_str),
            DayBucket::Tomorrow => format!("Tomorrow{}", time_str),
            DayBucket::ThisWeek | DayBucket::Later => {
                let wd = crate::i18n::weekday(lang, c.weekday);
                format!("{}, {:02}/{}{}", wd, c.month, c.day, time_str)
            }
        },
    }
}

pub fn fmt_repeat(repeat: Repeat, lang: Lang) -> &'static str {
    match lang {
        Lang::Vi => match repeat {
            Repeat::None => "Không lặp",
            Repeat::Daily => "Hằng ngày",
            Repeat::Weekdays => "Thứ 2 - Thứ 6",
            Repeat::Weekly(_) => "Hằng tuần",
        },
        Lang::En => match repeat {
            Repeat::None => "Does not repeat",
            Repeat::Daily => "Daily",
            Repeat::Weekdays => "Every weekday",
            Repeat::Weekly(_) => "Weekly",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::HOUR;

    const NOW: i64 = 1_794_294_309_000; // 14:05 local at +7 (Tuesday, 2026-11-10)

    #[test]
    fn test_day_bucket() {
        let today_mid = local_midnight(NOW, 420);
        assert_eq!(day_bucket(today_mid - 1000, NOW, 420), DayBucket::Overdue);
        assert_eq!(day_bucket(today_mid + 12 * HOUR, NOW, 420), DayBucket::Today);
        assert_eq!(day_bucket(today_mid + 25 * HOUR, NOW, 420), DayBucket::Tomorrow);
        assert_eq!(day_bucket(today_mid + 3 * DAY, NOW, 420), DayBucket::ThisWeek);
        assert_eq!(day_bucket(today_mid + 10 * DAY, NOW, 420), DayBucket::Later);
    }

    #[test]
    fn test_next_occurrence_daily() {
        let due = NOW - 1000;
        let next = next_occurrence(due, Repeat::Daily, NOW, 420).unwrap();
        assert!(next > NOW);
        assert_eq!(next, due + DAY);
    }

    #[test]
    fn test_next_occurrence_weekdays() {
        // Friday Nov 13, 2026: 3 days after Tuesday Nov 10
        let friday = NOW + 3 * DAY;
        let c_fri = civil(friday, 420);
        assert_eq!(c_fri.weekday, 4); // Friday = 4 (0=Mon)

        // Advancing from Friday past now should skip Sat (5) and Sun (6) to Monday Nov 16
        let next = next_occurrence(friday, Repeat::Weekdays, friday, 420).unwrap();
        let c_next = civil(next, 420);
        assert_eq!(c_next.weekday, 0); // Monday
        assert_eq!(next, friday + 3 * DAY);
    }
}
