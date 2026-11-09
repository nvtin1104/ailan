//! To-do list with optional reminder time.

use serde::{Deserialize, Serialize};

use crate::time;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todo {
    pub id: u64,
    pub text: String,
    pub done: bool,
    /// Reminder time, epoch ms.
    #[serde(default)]
    pub due: Option<i64>,
    #[serde(default)]
    pub notified: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todos {
    pub items: Vec<Todo>,
    pub next_id: u64,
}

pub const MAX_TEXT: usize = 120;

/// Parse user input. A trailing `@HH:MM` / `@H` / `@HHhMM` sets a reminder for the
/// next occurrence of that local time. Returns `(text, due)`; `None` if empty.
pub fn parse_input(input: &str, now: i64, offset_min: i32) -> Option<(String, Option<i64>)> {
    let input = input.trim();
    let mut text = input;
    let mut due = None;
    if let Some(at) = input.rfind('@') {
        if let Some((h, m)) = parse_hhmm(input[at + 1..].trim()) {
            due = Some(time::next_local_time(now, offset_min, h, m));
            text = input[..at].trim_end();
        }
    }
    if text.is_empty() {
        return None;
    }
    let text: String = text.chars().take(MAX_TEXT).collect();
    Some((text, due))
}

fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let s = s.to_ascii_lowercase();
    let (h, m) = match s.find([':', 'h', '.']) {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s.as_str(), ""),
    };
    let h: u32 = h.parse().ok()?;
    let m: u32 = if m.is_empty() { 0 } else { m.parse().ok()? };
    (h < 24 && m < 60).then_some((h, m))
}

impl Todos {
    pub fn add(&mut self, text: String, due: Option<i64>) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Todo { id, text, done: false, due, notified: false });
        id
    }

    pub fn toggle(&mut self, id: u64) -> bool {
        match self.items.iter_mut().find(|t| t.id == id) {
            Some(t) => {
                t.done = !t.done;
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
        self.items.iter().filter(|t| !t.done && !t.notified).filter_map(|t| t.due).min()
    }

    /// Marks and returns reminders that are due at `now`.
    pub fn take_due(&mut self, now: i64) -> Vec<String> {
        let mut out = Vec::new();
        for t in self.items.iter_mut() {
            if !t.done && !t.notified && t.due.is_some_and(|d| d <= now) {
                t.notified = true;
                out.push(t.text.clone());
            }
        }
        out
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
    fn crud_and_reminders() {
        let mut t = Todos::default();
        let a = t.add("a".into(), Some(100));
        let b = t.add("b".into(), Some(50));
        let _c = t.add("c".into(), None);
        assert_eq!(t.next_due(), Some(50));
        assert!(t.take_due(49).is_empty());
        assert_eq!(t.take_due(60), vec!["b".to_string()]);
        assert_eq!(t.next_due(), Some(100));
        t.toggle(a);
        assert_eq!(t.next_due(), None, "done tasks don't remind");
        assert_eq!(t.sorted().last().unwrap().id, a);
        assert!(t.remove(b));
        assert!(!t.remove(b));
        assert_eq!(t.pending_count(), 1);
    }
}
