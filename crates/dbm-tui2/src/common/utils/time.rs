//! Pure time helpers (no IO).

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local};

/// Convert a day count (days since 1970-01-01) to a `(year, month, day)` civil
/// date using the Howard Hinnant "civil_from_days" algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

/// The current UTC timestamp formatted as `[yyyy-mm-dd HH:MM:SS]`, matching the
/// original dbm's timestamp prefix shown before connection-test results.
pub fn utc_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("[{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}]")
}

/// `at` formatted as the local wall-clock label `yyyy-mm-dd HH:MM:SS` (no
/// brackets) — the same shape the instance rows show and what the Results pane
/// title uses to mark when its data was produced. Timezone is the machine's
/// local one, resolved via chrono.
pub fn local_timestamp(at: SystemTime) -> String {
    let dt: DateTime<Local> = at.into();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_matches_expected_shape() {
        let ts = utc_timestamp();
        // `[yyyy-mm-dd HH:MM:SS]`
        assert_eq!(ts.len(), 21, "{ts}");
        assert!(ts.starts_with('['));
        assert!(ts.ends_with(']'));
        assert_eq!(&ts[5..6], "-");
        assert_eq!(&ts[8..9], "-");
        assert_eq!(&ts[11..12], " ");
        assert_eq!(&ts[14..15], ":");
        assert_eq!(&ts[17..18], ":");
    }

    #[test]
    fn local_timestamp_is_digits_and_dashes_without_brackets() {
        let ts = local_timestamp(SystemTime::now());
        // `yyyy-mm-dd HH:MM:SS`, 19 chars, no brackets, all numeric fields.
        assert_eq!(ts.len(), 19, "{ts}");
        assert!(!ts.starts_with('['));
        assert!(!ts.ends_with(']'));
        for i in [0..4, 5..7, 8..10, 11..13, 14..16, 17..19] {
            assert!(ts[i].chars().all(|c| c.is_ascii_digit()), "{ts}");
        }
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], " ");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
    }
}
