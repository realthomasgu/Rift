//! Times the way a person says them. `rift snapshot` says how long ago the last snapshot was
//! taken and the Search page how long ago the index was written, in the same words. And the wait
//! for the next minute, which the bar's clock and the Date and time page both turn on.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Seconds since 1970, now. 0 on a clock that is before then.
#[must_use]
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| i64::try_from(since.as_secs()).ok())
        .unwrap_or(0)
}

/// The day the clock says it is, in UTC, as `2026-10-06`. The date a hardware report carries.
#[must_use]
pub fn today() -> String {
    crate::search::date(now().saturating_mul(1_000_000_000))
}

/// A span of seconds the way a person says it about the past. A span that has not happened yet,
/// which a clock that was put back gives, is just now.
#[must_use]
pub fn ago(seconds: i64) -> String {
    let (count, unit) = match seconds {
        ..60 => return "just now".to_string(),
        60..3600 => (seconds / 60, "minute"),
        3600..172_800 => (seconds / 3600, "hour"),
        _ => (seconds / 86_400, "day"),
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural} ago")
}

/// How long until the next minute starts. A tick lands a little after the turn, so a clock read
/// then never says the minute that just ended.
#[must_use]
pub fn until_next_minute() -> Duration {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    until(seconds)
}

fn until(seconds: u64) -> Duration {
    Duration::from_secs(60 - seconds % 60) + Duration::from_millis(200)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_span_reads_in_the_largest_unit_that_fits() {
        assert_eq!(ago(0), "just now");
        assert_eq!(ago(59), "just now");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(3599), "59 minutes ago");
        assert_eq!(ago(3600), "1 hour ago");
        assert_eq!(ago(172_799), "47 hours ago");
        assert_eq!(ago(172_800), "2 days ago");
        assert_eq!(ago(86_400 * 30), "30 days ago");
    }

    #[test]
    fn a_clock_that_went_back_says_just_now() {
        assert_eq!(ago(-1), "just now");
        assert_eq!(ago(-86_400), "just now");
    }

    #[test]
    fn a_tick_lands_just_after_the_turn_of_the_minute() {
        assert_eq!(until(0), Duration::from_millis(60_200));
        assert_eq!(until(59), Duration::from_millis(1_200));
        assert_eq!(until(1_789_221_603), Duration::from_millis(57_200));
        // never zero, so the thread cannot spin
        for second in 0..120 {
            assert!(until(second) >= Duration::from_millis(1_200));
            assert!(until(second) <= Duration::from_millis(60_200));
        }
    }

    #[test]
    fn today_is_the_day_in_utc() {
        let today = today();
        assert_eq!(today.len(), 10, "{today}");
        // after the day this was written, and this century
        assert!(today.as_str() > "2026-10-05", "{today}");
        assert!(today.starts_with("20"), "{today}");
    }

    #[test]
    fn the_clock_is_past_the_day_this_was_written() {
        // 2026-09-21, the day the module was written, in seconds since 1970
        assert!(now() > 1_789_000_000);
    }
}
