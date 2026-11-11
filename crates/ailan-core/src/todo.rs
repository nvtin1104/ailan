//! To-do list with optional reminder time, repeat schedules and snooze.

use serde::{Deserialize, Serialize};

use crate::reminder::{day_bucket, next_occurrence, DayBucket, Repeat};
use crate::time;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todo {
    pub id: u64,
    pub text: String,
    pub done: bool,
    /// Reminder time, epoch ms.
    #[serde(default)]
    pub due: Option<i64>,
    /// All-day item (has a date but no explicit alert time).
    #[serde(default)]
    pub all_day: bool,
    /// Repetition schedule.
    #[serde(default)]
    pub repeat: Repeat,
    /// Postponed / snoozed alert time, epoch ms.
    #[serde(default)]
    pub snoozed_until: Option<i64>,
    #[serde(default)]
    pub notified: bool,
    /// Creation timestamp, epoch ms.
    #[serde(default)]
    pub created: i64,
    /// Completion timestamp, epoch ms.
    #[serde(default)]
    pub done_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todos {
    pub items: Vec<Todo>,
    pub next_id: u64,
}

pub const MAX_TEXT: usize = 120;

/// Byte range inside the raw input string.
pub type Span = std::ops::Range<usize>;

/// A trailing `@…` due-date token recognised in the input, with the byte ranges of its
/// parts so the UI can syntax-highlight exactly what the parser will consume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueToken {
    /// Whole token, from `@` to the end of the (right-trimmed) input.
    pub span: Span,
    /// Relative day word (`mai`, `tomorrow`, `nay`, `today`…) and its day offset.
    pub day: Option<(Span, u32)>,
    /// Clock time and its parsed `(hour, minute)`.
    pub time: Option<(Span, (u32, u32))>,
}

impl DueToken {
    /// Resolve the token to an epoch-ms due time.
    pub fn resolve(&self, now: i64, offset_min: i32) -> Option<i64> {
        match (&self.day, &self.time) {
            (None, Some((_, (h, m)))) => Some(time::next_local_time(now, offset_min, *h, *m)),
            (Some((_, days)), Some((_, (h, m)))) => {
                let base = time::local_midnight(now, offset_min) + *days as i64 * time::DAY;
                Some(base + *h as i64 * time::HOUR + *m as i64 * time::MINUTE)
            }
            // A bare future day ("@mai") defaults to a 09:00 reminder.
            (Some((_, days)), None) if *days > 0 => {
                let base = time::local_midnight(now, offset_min) + *days as i64 * time::DAY;
                Some(base + 9 * time::HOUR)
            }
            _ => None,
        }
    }
}

fn day_word(w: &str) -> Option<u32> {
    match w.to_lowercase().as_str() {
        "nay" | "today" => Some(0),
        "mai" | "tomorrow" | "tmr" => Some(1),
        "mốt" | "mot" => Some(2),
        _ => None,
    }
}

/// Find a trailing `@[day] [time]` token, e.g. `@15:30`, `@9pm`, `@mai 9h`, `@tomorrow 9:30am`.
pub fn due_token(input: &str) -> Option<DueToken> {
    let trimmed = input.trim_end();
    let at = trimmed.rfind('@')?;
    let rest = &trimmed[at + 1..];
    // Words with their absolute byte ranges.
    let mut words: Vec<(Span, &str)> = Vec::new();
    let mut i = 0;
    for part in rest.split(char::is_whitespace) {
        if !part.is_empty() {
            let s = at + 1 + i;
            words.push((s..s + part.len(), part));
        }
        i += part.len() + 1;
    }
    let mut day = None;
    let mut tm = None;
    match words.as_slice() {
        [(sp, w)] => {
            if let Some(d) = day_word(w) {
                day = Some((sp.clone(), d));
            } else {
                tm = Some((sp.clone(), parse_hhmm(w)?));
            }
        }
        [(sp1, w1), (sp2, w2)] => {
            day = Some((sp1.clone(), day_word(w1)?));
            tm = Some((sp2.clone(), parse_hhmm(w2)?));
        }
        _ => return None,
    }
    // A token must sit directly on the `@` (no "@ 15:00").
    let first = day.as_ref().map(|d| d.0.start).or(tm.as_ref().map(|t| t.0.start))?;
    if first != at + 1 {
        return None;
    }
    let tok = DueToken { span: at..trimmed.len(), day, time: tm };
    // "@nay" without a time carries no information.
    if tok.time.is_none() && tok.day.as_ref().is_some_and(|d| d.1 == 0) {
        return None;
    }
    Some(tok)
}

/// Parse user input. A trailing `@HH:MM` / `@H` / `@HHhMM` / `@9pm` sets a reminder for the
/// next occurrence of that local time; `@mai 9h` / `@tomorrow 9am` pins it to a day.
/// Returns `(text, due)`; `None` if empty.
pub fn parse_input(input: &str, now: i64, offset_min: i32) -> Option<(String, Option<i64>)> {
    let input = input.trim();
    let mut text = input;
    let mut due = None;
    if let Some(tok) = due_token(input) {
        if let Some(d) = tok.resolve(now, offset_min) {
            due = Some(d);
            text = input[..tok.span.start].trim_end();
        }
    }
    if text.is_empty() {
        return None;
    }
    let text: String = text.chars().take(MAX_TEXT).collect();
    Some((text, due))
}

fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let mut s = s.to_ascii_lowercase();
    let mut pm = None;
    if let Some(x) = s.strip_suffix("am") {
        pm = Some(false);
        s = x.to_string();
    } else if let Some(x) = s.strip_suffix("pm") {
        pm = Some(true);
        s = x.to_string();
    }
    let (h, m) = match s.find([':', 'h', '.']) {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s.as_str(), ""),
    };
    if h.is_empty() || !h.bytes().all(|b| b.is_ascii_digit()) || !m.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut h: u32 = h.parse().ok()?;
    let m: u32 = if m.is_empty() { 0 } else { m.parse().ok()? };
    if let Some(pm) = pm {
        if !(1..=12).contains(&h) {
            return None;
        }
        h = match (pm, h) {
            (false, 12) => 0,
            (true, 12) => 12,
            (true, h) => h + 12,
            (false, h) => h,
        };
    }
    (h < 24 && m < 60).then_some((h, m))
}

impl Todos {
    pub fn add(&mut self, text: String, due: Option<i64>) -> u64 {
        self.add_full(text, due, false, Repeat::None, 0)
    }

    pub fn add_full(
        &mut self,
        text: String,
        due: Option<i64>,
        all_day: bool,
        repeat: Repeat,
        created: i64,
    ) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Todo {
            id,
            text,
            done: false,
            due,
            all_day,
            repeat,
            snoozed_until: None,
            notified: false,
            created,
            done_at: None,
        });
        id
    }

    pub fn toggle(&mut self, id: u64, now: i64, offset_min: i32) -> bool {
        match self.items.iter_mut().find(|t| t.id == id) {
            Some(t) => {
                if !t.done && t.repeat != Repeat::None {
                    // Repeating task completed: advance to next occurrence
                    if let Some(base) = t.due {
                        if let Some(next) = next_occurrence(base, t.repeat, now, offset_min) {
                            t.due = Some(next);
                            t.notified = false;
                            t.snoozed_until = None;
                            t.done_at = Some(now);
                            return true;
                        }
                    }
                }
                t.done = !t.done;
                t.done_at = if t.done { Some(now) } else { None };
                if !t.done {
                    t.notified = false;
                }
                true
            }
            None => false,
        }
    }

    pub fn snooze(&mut self, id: u64, now: i64, minutes: u32) -> bool {
        match self.items.iter_mut().find(|t| t.id == id) {
            Some(t) => {
                t.snoozed_until = Some(now + (minutes as i64) * 60_000);
                t.notified = false;
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|t| t.id != id);
        self.items.len() != before
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Todo> {
        self.items.iter_mut().find(|t| t.id == id)
    }

    /// Pending (not done) first, keeping insertion order inside each group.
    pub fn sorted(&self) -> Vec<&Todo> {
        let mut v: Vec<&Todo> = self.items.iter().collect();
        v.sort_by_key(|t| t.done);
        v
    }

    pub fn pending_count(&self) -> usize {
        self.items.iter().filter(|t| !t.done).count()
    }

    /// Earliest un-notified reminder of a pending task.
    pub fn next_due(&self) -> Option<i64> {
        self.items
            .iter()
            .filter(|t| !t.done && !t.notified && !t.all_day)
            .filter_map(|t| t.snoozed_until.or(t.due))
            .min()
    }

    /// Marks and returns reminders that are due at `now`: list of `(todo_id, task_text)`.
    pub fn take_due(&mut self, now: i64) -> Vec<(u64, String)> {
        let mut out = Vec::new();
        for t in self.items.iter_mut() {
            let eff = t.snoozed_until.or(t.due);
            if !t.done && !t.notified && !t.all_day && eff.is_some_and(|d| d <= now) {
                t.notified = true;
                out.push((t.id, t.text.clone()));
            }
        }
        out
    }

    /// Filter items for Today view (overdue or due today or without due date, pending first).
    pub fn today_items(&self, now: i64, offset_min: i32) -> Vec<&Todo> {
        let mut v: Vec<&Todo> = self
            .items
            .iter()
            .filter(|t| {
                if let Some(d) = t.snoozed_until.or(t.due) {
                    let b = day_bucket(d, now, offset_min);
                    matches!(b, DayBucket::Overdue | DayBucket::Today)
                } else {
                    // Task without date appears in today if pending
                    !t.done
                }
            })
            .collect();
        v.sort_by_key(|t| (t.done, t.snoozed_until.or(t.due).unwrap_or(i64::MAX)));
        v
    }

    /// Filter items with reminders (due != None), sorted chronologically.
    pub fn reminder_items(&self) -> Vec<&Todo> {
        let mut v: Vec<&Todo> = self.items.iter().filter(|t| t.due.is_some() || t.snoozed_until.is_some()).collect();
        v.sort_by_key(|t| (t.done, t.snoozed_until.or(t.due).unwrap_or(i64::MAX)));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::civil;

    const NOW: i64 = 1_794_294_309_000; // 14:05 at UTC+7

    #[test]
    fn parse_plain_and_reminder() {
        assert_eq!(parse_input("  buy milk ", NOW, 420), Some(("buy milk".into(), None)));
        let (text, due) = parse_input("call mom @15:30", NOW, 420).unwrap();
        assert_eq!(text, "call mom");
        let c = civil(due.unwrap(), 420);
        assert_eq!((c.day, c.hour, c.minute), (10, 15, 30));
        let (_, due) = parse_input("standup @9", NOW, 420).unwrap();
        let c = civil(due.unwrap(), 420);
        assert_eq!((c.day, c.hour, c.minute), (11, 9, 0), "past time rolls to tomorrow");
        let (_, due) = parse_input("x @16h45", NOW, 420).unwrap();
        assert_eq!(civil(due.unwrap(), 420).minute, 45);
    }

    #[test]
    fn parse_edge_cases() {
        assert_eq!(parse_input("   ", NOW, 0), None);
        assert_eq!(parse_input("@15:00", NOW, 0), None);
        // Not a time → kept verbatim.
        assert_eq!(parse_input("mail a@b.com", NOW, 0), Some(("mail a@b.com".into(), None)));
        assert_eq!(parse_input("x @25:00", NOW, 0), Some(("x @25:00".into(), None)));
        let long = "a".repeat(500);
        assert_eq!(parse_input(&long, NOW, 0).unwrap().0.len(), MAX_TEXT);
    }

    #[test]
    fn parse_day_words_and_meridiem() {
        // NOW is Tuesday 2026-11-10 14:05 at +7.
        let (text, due) = parse_input("Gọi mẹ @mai 9h", NOW, 420).unwrap();
        assert_eq!(text, "Gọi mẹ");
        let c = civil(due.unwrap(), 420);
        assert_eq!((c.day, c.hour, c.minute), (11, 9, 0));

        let (_, due) = parse_input("review @tomorrow 9:30am", NOW, 420).unwrap();
        let c = civil(due.unwrap(), 420);
        assert_eq!((c.day, c.hour, c.minute), (11, 9, 30));

        let (_, due) = parse_input("gym @9pm", NOW, 420).unwrap();
        let c = civil(due.unwrap(), 420);
        assert_eq!((c.day, c.hour), (10, 21));

        let (_, due) = parse_input("plan @mai", NOW, 420).unwrap();
        let c = civil(due.unwrap(), 420);
        assert_eq!((c.day, c.hour), (11, 9), "bare day defaults to 09:00");

        let (_, due) = parse_input("ship @nay 16:00", NOW, 420).unwrap();
        let c = civil(due.unwrap(), 420);
        assert_eq!((c.day, c.hour), (10, 16));

        // Not tokens → text kept verbatim.
        assert_eq!(parse_input("x @nay", NOW, 0), Some(("x @nay".into(), None)));
        assert_eq!(parse_input("x @13pm", NOW, 0), Some(("x @13pm".into(), None)));
        assert_eq!(parse_input("x @mai mai", NOW, 0), Some(("x @mai mai".into(), None)));
    }

    #[test]
    fn due_token_spans() {
        let s = "Gọi mẹ @mai 9h";
        let tok = due_token(s).unwrap();
        assert_eq!(&s[tok.span.clone()], "@mai 9h");
        assert_eq!(&s[tok.day.clone().unwrap().0], "mai");
        assert_eq!(&s[tok.time.clone().unwrap().0], "9h");
        assert!(due_token("hello @").is_none());
        assert!(due_token("hello @ 9h").is_none());
        let tok = due_token("x @15:30  ").unwrap();
        assert_eq!(tok.span, 2..8);
    }

    #[test]
    fn crud_and_reminders() {
        let mut t = Todos::default();
        let a = t.add("a".into(), Some(100));
        let b = t.add("b".into(), Some(50));
        let _c = t.add("c".into(), None);
        assert_eq!(t.next_due(), Some(50));
        assert!(t.take_due(49).is_empty());
        assert_eq!(t.take_due(60), vec![(b, "b".to_string())]);
        assert_eq!(t.next_due(), Some(100));
        t.toggle(a, NOW, 420);
        assert_eq!(t.next_due(), None, "done tasks don't remind");
        assert_eq!(t.sorted().last().unwrap().id, a);
        assert!(t.remove(b));
        assert!(!t.remove(b));
        assert_eq!(t.pending_count(), 1);
    }

    #[test]
    fn test_snooze_and_repeat() {
        let mut t = Todos::default();
        let id = t.add_full("drink water".into(), Some(1000), false, Repeat::Daily, 0);
        assert_eq!(t.next_due(), Some(1000));
        assert_eq!(t.take_due(1000), vec![(id, "drink water".to_string())]);
        assert_eq!(t.next_due(), None, "notified is true");

        // Snooze 5 minutes (300_000 ms)
        t.snooze(id, 1000, 5);
        assert_eq!(t.next_due(), Some(1000 + 300_000));

        // Toggle repeat item
        t.toggle(id, 1000 + 300_000, 420);
        let item = t.items.iter().find(|x| x.id == id).unwrap();
        assert!(!item.done, "repeating item stays active");
        assert!(item.due.unwrap() > 1000 + 300_000);
    }
}
