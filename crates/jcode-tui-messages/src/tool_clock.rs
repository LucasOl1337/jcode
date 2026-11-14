//! Wall-clock completion times for tool rows, keyed by tool call id.
//!
//! `DisplayMessage` is built in a few hundred places, so the time a tool
//! finished is kept beside the transcript rather than as a field on every row.
//! History carries the stored session timestamp; live rows are stamped when
//! they reach the transcript. An id recorded as `None` is a restored row whose
//! time is unknown, which stops it from being stamped with the restore time.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn clock() -> &'static Mutex<HashMap<String, Option<i64>>> {
    static CLOCK: OnceLock<Mutex<HashMap<String, Option<i64>>>> = OnceLock::new();
    CLOCK.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with_clock<T>(f: impl FnOnce(&mut HashMap<String, Option<i64>>) -> T) -> T {
    let mut guard = clock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

/// Record the time a restored tool row finished. A known time always wins; an
/// unknown one never erases a time already observed in this process.
pub fn note_tool_history(tool_id: &str, finished_at_ms: Option<i64>) {
    if tool_id.is_empty() {
        return;
    }
    with_clock(|clock| match finished_at_ms {
        Some(ms) => {
            clock.insert(tool_id.to_owned(), Some(ms));
        }
        None => {
            clock.entry(tool_id.to_owned()).or_insert(None);
        }
    });
}

/// Stamp a tool row that just completed, unless its time is already settled.
pub fn note_tool_finished_now(tool_id: &str) {
    if tool_id.is_empty() {
        return;
    }
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok());
    with_clock(|clock| {
        clock.entry(tool_id.to_owned()).or_insert(now_ms);
    });
}

/// Unix milliseconds at which the tool finished, when known.
pub fn tool_finished_at_ms(tool_id: &str) -> Option<i64> {
    with_clock(|clock| clock.get(tool_id).copied().flatten())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_history_time_wins_and_unknown_never_erases() {
        note_tool_history("clock-test-a", None);
        assert_eq!(tool_finished_at_ms("clock-test-a"), None);
        note_tool_finished_now("clock-test-a");
        assert_eq!(tool_finished_at_ms("clock-test-a"), None);
        note_tool_history("clock-test-a", Some(1_000));
        assert_eq!(tool_finished_at_ms("clock-test-a"), Some(1_000));
        note_tool_history("clock-test-a", None);
        assert_eq!(tool_finished_at_ms("clock-test-a"), Some(1_000));
    }

    #[test]
    fn live_rows_are_stamped_once() {
        note_tool_finished_now("clock-test-b");
        let first = tool_finished_at_ms("clock-test-b").expect("stamped");
        note_tool_finished_now("clock-test-b");
        assert_eq!(tool_finished_at_ms("clock-test-b"), Some(first));
        note_tool_finished_now("");
        assert_eq!(tool_finished_at_ms(""), None);
    }
}
