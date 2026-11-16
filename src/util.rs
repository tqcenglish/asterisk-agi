//! Time conversion helpers shared by the session and compatibility APIs.

use std::time::Duration;

/// Format a duration as the millisecond string AGI expects.
pub fn to_msec(duration: Duration) -> String {
    format!("{}", (duration.as_secs_f64() * 1000.0) as i64)
}

/// Format a duration as the whole-second string AGI expects.
pub fn to_sec(duration: Duration) -> String {
    format!("{}", duration.as_secs())
}

/// Format a timestamp as seconds since the Unix epoch.
pub fn to_epoch<Tz: chrono::TimeZone>(when: &chrono::DateTime<Tz>) -> String {
    format!("{}", when.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_durations() {
        assert_eq!(to_msec(Duration::from_millis(1500)), "1500");
        assert_eq!(to_sec(Duration::from_secs(90)), "90");
    }
}
