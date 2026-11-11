//! Smart brief: a local summary of the day built from to-dos and reminders —
//! what is overdue, what is coming, and the next free window for a focus session.

use crate::reminder::{day_bucket, DayBucket};
use crate::time::{local_midnight, HOUR, MINUTE};
use crate::todo::Todos;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Brief {
    /// Pending items whose reminder is before today, oldest first.
    pub overdue: Vec<u64>,
    /// Pending reminders still ahead today.
    pub due_today: usize,
    /// All pending tasks.
    pub open: usize,
    /// Next upcoming reminder `(id, due)`.
    pub next: Option<(u64, i64)>,
    /// First free `[start, end)` window today long enough for one focus session.
    pub free_slot: Option<(i64, i64)>,
}

/// Reminders block `[due - 5m, due + 10m)` around them; the workday ends at 23:00 local.
pub fn smart_brief(todos: &Todos, now: i64, offset_min: i32, focus_min: u32) -> Brief {
    let mut b = Brief::default();
    let mut overdue: Vec<(i64, u64)> = Vec::new();
    let mut busy: Vec<(i64, i64)> = Vec::new();
    let day_end = local_midnight(now, offset_min) + 23 * HOUR;

    for t in todos.items.iter().filter(|t| !t.done) {
        b.open += 1;
        let Some(due) = t.snoozed_until.or(t.due) else { continue };
        match day_bucket(due, now, offset_min) {
            DayBucket::Overdue => overdue.push((due, t.id)),
            _ if due < now => overdue.push((due, t.id)),
            DayBucket::Today => b.due_today += 1,
            _ => {}
        }
        if due >= now && b.next.is_none_or(|(_, d)| due < d) {
            b.next = Some((t.id, due));
        }
        if due + 10 * MINUTE > now && due - 5 * MINUTE < day_end {
            busy.push((due - 5 * MINUTE, due + 10 * MINUTE));
        }
    }
    overdue.sort();
    b.overdue = overdue.into_iter().map(|(_, id)| id).collect();

    let len = focus_min.max(1) as i64 * MINUTE;
    let step = 5 * MINUTE;
    let mut start = (now + step - 1).div_euclid(step) * step;
    busy.sort();
    for (s, e) in busy {
        if s - start >= len {
            break;
        }
        start = start.max(e);
    }
    if start + len <= day_end {
        b.free_slot = Some((start, start + len));
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::civil;

    const NOW: i64 = 1_794_294_309_000; // 14:05 at +7
    const OFF: i32 = 420;

    #[test]
    fn counts_and_free_slot_skip_reminders() {
        let mut t = Todos::default();
        let mid = local_midnight(NOW, OFF);
        let late = t.add("late".into(), Some(mid - HOUR));
        t.add("soon".into(), Some(mid + 14 * HOUR + 20 * MINUTE)); // 14:20 blocks 14:15–14:30
        t.add("undated".into(), None);
        let b = smart_brief(&t, NOW, OFF, 25);
        assert_eq!(b.overdue, vec![late]);
        assert_eq!(b.due_today, 1);
        assert_eq!(b.open, 3);
        let (s, e) = b.free_slot.unwrap();
        let c = civil(s, OFF);
        assert_eq!((c.hour, c.minute), (14, 30), "14:10 start would collide with the 14:20 reminder");
        assert_eq!(e - s, 25 * MINUTE);
    }

    #[test]
    fn no_slot_late_at_night() {
        let t = Todos::default();
        let late_now = local_midnight(NOW, OFF) + 22 * HOUR + 50 * MINUTE;
        assert_eq!(smart_brief(&t, late_now, OFF, 25).free_slot, None);
    }
}
