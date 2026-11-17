//! UTC calendar date without a date-time dependency.

use std::time::{SystemTime, UNIX_EPOCH};

/// Today as `YYYY-MM-DD` (UTC).
pub fn today_utc() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400);
    civil(days as i64)
}

/// Days since 1970-01-01 → `YYYY-MM-DD` (Howard Hinnant's civil_from_days).
pub fn civil(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn known_dates() {
        assert_eq!(super::civil(0), "1970-01-01");
        assert_eq!(super::civil(19_723), "2024-01-01");
        assert_eq!(super::civil(11_016), "2000-02-29");
        assert_eq!(super::today_utc().len(), 10);
    }
}
