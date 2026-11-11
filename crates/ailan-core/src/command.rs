//! Local-first natural-language command parser (Vietnamese + English, no network, no model).
//!
//! Turns a sentence such as
//! `"Lên lịch ôn tập Rust lúc 20h mỗi tối thứ 2, 4, 6 và bật Pomodoro 45p"`
//! into a [`Plan`]: a task title, a (possibly repeating) reminder time and Pomodoro settings.
//! The grammar is deliberately small and predictable; anything it does not recognise stays
//! in the task title, so nothing the user typed is silently lost.

use crate::reminder::Repeat;
use crate::time::{civil, local_midnight, next_local_time, DAY, HOUR, MINUTE};

/// What a command will do when executed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// Task title (empty when the command only configures focus).
    pub title: String,
    /// Local clock time `(hour, minute)`.
    pub time: Option<(u32, u32)>,
    /// Relative day: 0 = today, 1 = tomorrow, 2 = day after.
    pub day_offset: Option<u32>,
    /// One-off weekday (0 = Monday … 6 = Sunday) when a single day is named without "every".
    pub weekday: Option<u32>,
    pub repeat: Repeat,
    /// Pomodoro focus length to set, in minutes.
    pub focus_min: Option<u32>,
    /// Start a focus session right away ("bật Pomodoro", "start focus").
    pub start_focus: bool,
}

impl Plan {
    /// True when executing would change anything.
    pub fn is_actionable(&self) -> bool {
        !self.title.is_empty() || self.focus_min.is_some() || self.start_focus
    }

    /// True when the task carries a reminder.
    pub fn has_schedule(&self) -> bool {
        self.time.is_some() || self.day_offset.is_some() || self.weekday.is_some() || self.repeat != Repeat::None
    }

    /// First reminder time (epoch ms) strictly after `now`, if the plan is scheduled.
    pub fn due(&self, now: i64, offset_min: i32) -> Option<i64> {
        if !self.has_schedule() {
            return None;
        }
        let (h, m) = self.time.unwrap_or((9, 0));
        let at = |day_start: i64| day_start + h as i64 * HOUR + m as i64 * MINUTE;
        let today = local_midnight(now, offset_min);
        let weekday_of = |t: i64| civil(t, offset_min).weekday;
        match self.repeat {
            Repeat::Daily => Some(next_local_time(now, offset_min, h, m)),
            Repeat::Weekdays => {
                let mut t = next_local_time(now, offset_min, h, m);
                while weekday_of(t) >= 5 {
                    t += DAY;
                }
                Some(t)
            }
            Repeat::Weekly(mask) => (0..8)
                .map(|d| at(today + d * DAY))
                .find(|&t| t > now && mask & (1 << weekday_of(t)) != 0),
            Repeat::None => {
                if let Some(d) = self.day_offset {
                    Some(at(today + d as i64 * DAY))
                } else if let Some(wd) = self.weekday {
                    (0..8).map(|d| at(today + d * DAY)).find(|&t| t > now && weekday_of(t) == wd)
                } else {
                    Some(next_local_time(now, offset_min, h, m))
                }
            }
        }
    }
}

struct Tok {
    orig: String,
    /// Lowercase with diacritics — used where folding would be ambiguous ("tối" vs "tôi").
    raw: String,
    /// Lowercase, diacritics folded — tolerant matching for unambiguous keywords.
    low: String,
    used: bool,
}

/// Fold common Vietnamese diacritics so "thu", "moi", "nhac" also match.
fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'à' | 'á' | 'ả' | 'ã' | 'ạ' | 'ă' | 'ằ' | 'ắ' | 'ẳ' | 'ẵ' | 'ặ' | 'â' | 'ầ' | 'ấ' | 'ẩ' | 'ẫ' | 'ậ' => 'a',
            'è' | 'é' | 'ẻ' | 'ẽ' | 'ẹ' | 'ê' | 'ề' | 'ế' | 'ể' | 'ễ' | 'ệ' => 'e',
            'ì' | 'í' | 'ỉ' | 'ĩ' | 'ị' => 'i',
            'ò' | 'ó' | 'ỏ' | 'õ' | 'ọ' | 'ô' | 'ồ' | 'ố' | 'ổ' | 'ỗ' | 'ộ' | 'ơ' | 'ờ' | 'ớ' | 'ở' | 'ỡ' | 'ợ' => 'o',
            'ù' | 'ú' | 'ủ' | 'ũ' | 'ụ' | 'ư' | 'ừ' | 'ứ' | 'ử' | 'ữ' | 'ự' => 'u',
            'ỳ' | 'ý' | 'ỷ' | 'ỹ' | 'ỵ' => 'y',
            'đ' => 'd',
            c => c,
        })
        .collect()
}

fn tokenize(input: &str) -> Vec<Tok> {
    input
        .replace([',', ';'], " ")
        .split_whitespace()
        .map(|w| {
            let w = w.trim_end_matches(['.', '!', '?']);
            let raw = w.to_lowercase();
            Tok { orig: w.to_string(), low: fold(&raw), raw, used: false }
        })
        .filter(|t| !t.orig.is_empty())
        .collect()
}

/// `20h`, `20h30`, `20:30`, `8pm`, `8:30am`, `@20h`. Bare numbers are not clock times.
fn parse_clock(w: &str) -> Option<(u32, u32, Option<bool>)> {
    let w = w.trim_start_matches('@');
    let (body, pm) = if let Some(b) = w.strip_suffix("pm") {
        (b, Some(true))
    } else if let Some(b) = w.strip_suffix("am") {
        (b, Some(false))
    } else {
        (w, None)
    };
    let (h, m) = match body.find([':', 'h', '.']) {
        Some(i) => (&body[..i], &body[i + 1..]),
        None if pm.is_some() => (body, ""),
        None => return None,
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(h) || !(m.is_empty() || digits(m)) {
        return None;
    }
    let h: u32 = h.parse().ok()?;
    let m: u32 = if m.is_empty() { 0 } else { m.parse().ok()? };
    (h < 24 && m < 60).then_some((h, m, pm))
}

/// `45p`, `45'`, `45m`, `45min`, `45phut`. Returns minutes.
fn parse_minutes(w: &str) -> Option<u32> {
    let end = w.find(|c: char| !c.is_ascii_digit()).unwrap_or(w.len());
    if end == 0 {
        return None;
    }
    let n: u32 = w[..end].parse().ok()?;
    let unit = &w[end..];
    matches!(unit, "p" | "'" | "m" | "min" | "mins" | "phut" | "ph").then_some(n).filter(|n| (1..=180).contains(n))
}

fn weekday_word(w: &str) -> Option<u32> {
    Some(match w {
        "t2" | "mon" | "monday" | "mondays" => 0,
        "t3" | "tue" | "tues" | "tuesday" | "tuesdays" => 1,
        "t4" | "wed" | "wednesday" | "wednesdays" => 2,
        "t5" | "thur" | "thurs" | "thursday" | "thursdays" => 3,
        "t6" | "fri" | "friday" | "fridays" => 4,
        "t7" | "sat" | "saturday" | "saturdays" => 5,
        "cn" | "sun" | "sunday" | "sundays" => 6,
        _ => return None,
    })
}

const LEAD_VERBS: [&[&str]; 14] = [
    &["len", "lich"],
    &["nhac", "toi"],
    &["nhac", "nho"],
    &["nhac"],
    &["them"],
    &["tao"],
    &["dat", "lich"],
    &["hay"],
    &["remind", "me", "to"],
    &["remind", "me"],
    &["remind"],
    &["schedule"],
    &["add"],
    &["create"],
];

/// Dangling words trimmed from the title edges (matched with diacritics, plus ASCII spellings).
const CONNECTORS: [&str; 20] = [
    "và", "va", "and", "với", "voi", "rồi", "roi", "then", "lúc", "luc", "at", "on", "vào", "vao", "mỗi", "every", "each", "&",
    "+", "-",
];

/// Parse a natural-language command into a [`Plan`].
pub fn parse_command(input: &str) -> Plan {
    let mut t = tokenize(input);
    let n = t.len();
    let mut plan = Plan::default();
    let mut every = false;
    let mut mask: u8 = 0;
    let mut evening = false;
    let mut morning = false;
    let mut pm_flag: Option<bool> = None;

    // Leading verbs ("lên lịch", "remind me to", …)
    let mut i = 0;
    'verbs: while i < n {
        for v in LEAD_VERBS {
            if i + v.len() <= n && v.iter().enumerate().all(|(k, w)| t[i + k].low == *w) {
                for k in 0..v.len() {
                    t[i + k].used = true;
                }
                i += v.len();
                continue 'verbs;
            }
        }
        break;
    }

    let mut i = 0;
    while i < n {
        if t[i].used {
            i += 1;
            continue;
        }
        let w = t[i].low.clone();
        let raw = t[i].raw.clone();
        let n1 = t.get(i + 1).map(|x| x.low.clone()).unwrap_or_default();

        // Focus / Pomodoro
        let focus_kw = match w.as_str() {
            "pomodoro" | "pomo" => 1,
            "tap" if n1.as_str() == "trung" => 2,
            "focus" if parse_minutes(n1.as_str()).is_some() || (i > 0 && matches!(t[i - 1].low.as_str(), "start" | "bat")) => 1,
            _ => 0,
        };
        if focus_kw > 0 {
            for k in 0..focus_kw {
                t[i + k].used = true;
            }
            if i > 0 && matches!(t[i - 1].low.as_str(), "bat" | "start" | "chay" | "begin") {
                t[i - 1].used = true;
                plan.start_focus = true;
            }
            if i > 1 && t[i - 2].low == "bat" && t[i - 1].low == "dau" {
                t[i - 2].used = true;
                t[i - 1].used = true;
                plan.start_focus = true;
            }
            let j = i + focus_kw;
            if let Some(m) = t.get(j).and_then(|x| parse_minutes(&x.low)) {
                plan.focus_min = Some(m);
                t[j].used = true;
            } else if let (Some(num), Some(unit)) = (t.get(j), t.get(j + 1)) {
                if let (Ok(m), true) = (num.low.parse::<u32>(), matches!(unit.low.as_str(), "phut" | "min" | "minutes" | "p")) {
                    plan.focus_min = Some(m.clamp(1, 180));
                    t[j].used = true;
                    t[j + 1].used = true;
                }
            }
            i = j;
            continue;
        }

        // Repetition
        match raw.as_str() {
            "mỗi" | "every" | "each" | "hằng" | "hàng" => {
                every = true;
                t[i].used = true;
                if matches!(n1.as_str(), "ngay" | "day") {
                    plan.repeat = Repeat::Daily;
                    t[i + 1].used = true;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }
            "daily" | "everyday" => {
                plan.repeat = Repeat::Daily;
                t[i].used = true;
                i += 1;
                continue;
            }
            "weekdays" | "weekday" => {
                plan.repeat = Repeat::Weekdays;
                t[i].used = true;
                i += 1;
                continue;
            }
            "ngay" if n1.as_str() == "thuong" => {
                plan.repeat = Repeat::Weekdays;
                t[i].used = true;
                t[i + 1].used = true;
                i += 2;
                continue;
            }
            _ => {}
        }

        // Weekdays: "thứ 2, 4, 6", "t2 t4", "chủ nhật", "mon wed fri"
        if w == "thu" && n1.as_str().parse::<u32>().is_ok_and(|d| (2..=7).contains(&d)) {
            t[i].used = true;
            let mut j = i + 1;
            while j < n {
                let lw = t[j].low.as_str();
                if let Ok(d) = lw.parse::<u32>() {
                    if (2..=7).contains(&d) {
                        mask |= 1 << (d - 2);
                        t[j].used = true;
                        j += 1;
                        continue;
                    }
                }
                if matches!(lw, "va" | "and") && t.get(j + 1).is_some_and(|x| x.low.parse::<u32>().is_ok_and(|d| (2..=7).contains(&d))) {
                    t[j].used = true;
                    j += 1;
                    continue;
                }
                break;
            }
            i = j;
            continue;
        }
        if w == "chu" && n1.as_str() == "nhat" {
            mask |= 1 << 6;
            t[i].used = true;
            t[i + 1].used = true;
            i += 2;
            continue;
        }
        if let Some(d) = weekday_word(&raw) {
            mask |= 1 << d;
            t[i].used = true;
            i += 1;
            continue;
        }

        // Day words
        let evening_tonight = raw == "tối" && n1 == "nay";
        let day = match (w.as_str(), n1.as_str()) {
            ("ngay", "mai") | ("hom", "nay") | ("ngay", "mot") | ("toi", "nay") if w != "toi" || evening_tonight => {
                if evening_tonight {
                    evening = true;
                }
                let d = match n1.as_str() {
                    "mai" => 1,
                    "mot" => 2,
                    _ => 0,
                };
                t[i + 1].used = true;
                Some(d)
            }
            ("mai", _) | ("tomorrow", _) | ("tmr", _) => Some(1),
            ("today", _) => Some(0),
            ("tonight", _) => {
                evening = true;
                Some(0)
            }
            _ => None,
        };
        if let Some(d) = day {
            plan.day_offset = Some(d);
            t[i].used = true;
            i += 1;
            continue;
        }

        // Time-of-day qualifiers
        match raw.as_str() {
            "tối" | "chiều" | "evening" | "afternoon" | "đêm" | "night" => {
                evening = true;
                t[i].used = true;
                i += 1;
                continue;
            }
            "sáng" | "morning" => {
                morning = true;
                t[i].used = true;
                i += 1;
                continue;
            }
            "pm" | "am" if plan.time.is_some() => {
                pm_flag = Some(w == "pm");
                t[i].used = true;
                i += 1;
                continue;
            }
            "giờ" | "gio" if plan.time.is_some() => {
                t[i].used = true;
                i += 1;
                continue;
            }
            _ => {}
        }

        // Clock time ("lúc 20h", "at 8pm", "@15:30", "lúc 8 giờ")
        if let Some((h, m, pm)) = parse_clock(&w) {
            plan.time = Some((h, m));
            pm_flag = pm.or(pm_flag);
            t[i].used = true;
            if i > 0 && matches!(t[i - 1].low.as_str(), "luc" | "at" | "vao") {
                t[i - 1].used = true;
            }
            i += 1;
            continue;
        }
        if i > 0 && matches!(t[i - 1].low.as_str(), "luc" | "at") {
            if let Ok(h) = w.parse::<u32>() {
                if h < 24 {
                    plan.time = Some((h, 0));
                    t[i].used = true;
                    t[i - 1].used = true;
                    i += 1;
                    continue;
                }
            }
        }
        i += 1;
    }

    // Meridiem
    if let Some((h, m)) = plan.time {
        let h = match pm_flag {
            Some(true) if h < 12 => h + 12,
            Some(false) if h == 12 => 0,
            _ if pm_flag.is_none() && evening && h < 12 => h + 12,
            _ if pm_flag.is_none() && morning && h == 12 => 0,
            _ => h,
        };
        plan.time = Some((h, m));
    }

    // Weekday mask → weekly repeat, or a one-off day
    if mask != 0 {
        if every || mask.count_ones() > 1 {
            plan.repeat = Repeat::Weekly(mask);
        } else {
            plan.weekday = Some(mask.trailing_zeros());
        }
    }

    // Title: unconsumed words, minus dangling connectors at the edges
    let mut words: Vec<&Tok> = t.iter().filter(|x| !x.used).collect();
    while words.first().is_some_and(|x| CONNECTORS.contains(&x.raw.as_str())) {
        words.remove(0);
    }
    while words.last().is_some_and(|x| CONNECTORS.contains(&x.raw.as_str())) {
        words.pop();
    }
    let title = words.iter().map(|x| x.orig.as_str()).collect::<Vec<_>>().join(" ");
    let mut chars = title.chars();
    plan.title = match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_794_294_309_000; // Tue 2026-11-10 14:05 at +7
    const OFF: i32 = 420;

    #[test]
    fn spec_example_vietnamese() {
        let p = parse_command("Lên lịch ôn tập Rust lúc 20h mỗi tối thứ 2, 4, 6 và bật Pomodoro 45p");
        assert_eq!(p.title, "Ôn tập Rust");
        assert_eq!(p.time, Some((20, 0)));
        assert_eq!(p.repeat, Repeat::Weekly(0b10101), "Mon, Wed, Fri");
        assert_eq!(p.focus_min, Some(45));
        assert!(p.start_focus);
        // Next Mon/Wed/Fri 20:00 after Tue 14:05 → Wed 11 Nov 20:00.
        let c = civil(p.due(NOW, OFF).unwrap(), OFF);
        assert_eq!((c.day, c.hour, c.minute, c.weekday), (11, 20, 0, 2));
    }

    #[test]
    fn english_commands() {
        let p = parse_command("Remind me to call mom tomorrow at 9am");
        assert_eq!(p.title, "Call mom");
        assert_eq!(p.day_offset, Some(1));
        let c = civil(p.due(NOW, OFF).unwrap(), OFF);
        assert_eq!((c.day, c.hour), (11, 9));

        let p = parse_command("standup every weekday at 9:30");
        assert_eq!(p.title, "Standup");
        assert_eq!(p.repeat, Repeat::Weekdays);
        assert_eq!(p.time, Some((9, 30)));

        let p = parse_command("gym mon wed fri 6pm");
        assert_eq!(p.repeat, Repeat::Weekly(0b10101));
        assert_eq!(p.time, Some((18, 0)));
    }

    #[test]
    fn vietnamese_variants_and_meridiem() {
        let p = parse_command("nhắc tôi uống thuốc lúc 8 giờ tối nay");
        assert_eq!(p.title, "Uống thuốc");
        assert_eq!(p.time, Some((20, 0)));
        assert_eq!(p.day_offset, Some(0));

        let p = parse_command("tập gym mỗi ngày 6h sáng");
        assert_eq!(p.title, "Tập gym");
        assert_eq!(p.repeat, Repeat::Daily);
        assert_eq!(p.time, Some((6, 0)));

        let p = parse_command("họp nhóm thứ 5 lúc 14h30");
        assert_eq!(p.weekday, Some(3));
        assert_eq!(p.repeat, Repeat::None);
        let c = civil(p.due(NOW, OFF).unwrap(), OFF);
        assert_eq!((c.weekday, c.hour, c.minute), (3, 14, 30));
    }

    #[test]
    fn focus_only_and_plain_text() {
        let p = parse_command("bật pomodoro 50 phút");
        assert!(p.title.is_empty());
        assert_eq!(p.focus_min, Some(50));
        assert!(p.start_focus && p.is_actionable() && !p.has_schedule());

        let p = parse_command("viết báo cáo");
        assert_eq!(p.title, "Viết báo cáo");
        assert!(!p.has_schedule());
        assert_eq!(p.due(NOW, OFF), None);

        assert!(!parse_command("   ").is_actionable());
    }
}
