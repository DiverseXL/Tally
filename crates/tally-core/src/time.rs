//! RFC 3339 UTC timestamps and run ids, without an extra dependency.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds in one day.
const SECS_PER_DAY: i64 = 86_400;

/// Converts Unix seconds to an RFC 3339 UTC string such as
/// `2026-11-18T09:30:00Z`.
///
/// Negative inputs (times before 1970) are clamped to the epoch.
#[must_use]
pub fn rfc3339_utc_from_unix_secs(secs: i64) -> String {
    let secs = secs.max(0);
    let days = secs.div_euclid(SECS_PER_DAY);
    let time_of_day = secs.rem_euclid(SECS_PER_DAY);

    let (year, month, day) = civil_from_days(days);
    let hour = time_of_day / 3_600;
    let minute = (time_of_day % 3_600) / 60;
    let second = time_of_day % 60;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// The current time as an RFC 3339 UTC string.
#[must_use]
pub fn now_rfc3339() -> String {
    rfc3339_utc_from_unix_secs(unix_now_secs())
}

/// The current Unix time in seconds.
#[must_use]
pub fn unix_now_secs() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(elapsed) => i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

/// The current Unix time in whole milliseconds.
#[must_use]
pub fn unix_now_millis() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(elapsed) => u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
        Err(_) => 0,
    }
}

/// A 32 character hex id for one run.
///
/// It mixes the current time with the per-process random keys of
/// [`RandomState`]; it is unique enough for a report file name, and it is not
/// cryptographic.
#[must_use]
pub fn new_run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);

    let first = hash_of(RandomState::new(), nanos);
    let second = hash_of(RandomState::new(), nanos.rotate_left(37));

    format!("{first:016x}{second:016x}")
}

fn hash_of(state: RandomState, value: u128) -> u64 {
    state.hash_one(value)
}

/// Year, month and day for a day count since 1970-01-01.
///
/// This is Howard Hinnant's `civil_from_days` algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_position = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_position + 2) / 5 + 1;
    let month = if month_position < 10 {
        month_position + 3
    } else {
        month_position - 9
    };

    let year = year + i64::from(month <= 2);
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

#[cfg(test)]
mod tests {
    use super::{new_run_id, rfc3339_utc_from_unix_secs};

    #[test]
    fn known_timestamps() {
        assert_eq!(rfc3339_utc_from_unix_secs(0), "1970-01-01T00:00:00Z");
        assert_eq!(
            rfc3339_utc_from_unix_secs(951_782_400),
            "2000-02-29T00:00:00Z"
        );
        assert_eq!(
            rfc3339_utc_from_unix_secs(1_000_000_000),
            "2001-09-09T01:46:40Z"
        );
        assert_eq!(
            rfc3339_utc_from_unix_secs(1_763_424_000),
            "2025-11-18T00:00:00Z"
        );
    }

    #[test]
    fn negative_times_are_clamped_to_the_epoch() {
        assert_eq!(rfc3339_utc_from_unix_secs(-1), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn run_id_is_hex_and_differs_between_calls() {
        let first = new_run_id();
        let second = new_run_id();

        assert_eq!(first.len(), 32);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }
}
