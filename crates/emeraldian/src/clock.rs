//! Dates and times, in UTC, without a date library.
//!
//! The app asks little of a clock: today's date for a daily note, and for the
//! command line, timestamps that a person and a program can both read and a
//! "how long ago" for each. None of it justifies a date crate; all of it comes
//! down to the civil-calendar conversion at the bottom. UTC throughout, with the
//! `Z` written out wherever a time is stored, since the standard library has no
//! way to ask what the local zone is.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch.
#[must_use]
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A moment, as calendar fields in UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utc {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Utc {
    #[must_use]
    pub fn at(secs: u64) -> Self {
        let (year, month, day) = civil_from_days((secs / 86_400) as i64);
        let rest = secs % 86_400;
        Self {
            year,
            month,
            day,
            hour: (rest / 3600) as u32,
            minute: (rest % 3600 / 60) as u32,
            second: (rest % 60) as u32,
        }
    }

    #[must_use]
    pub fn now() -> Self {
        Self::at(now())
    }

    /// `2026-10-22T13:00:05Z`: what a frontmatter property holds.
    #[must_use]
    pub fn iso(&self) -> String {
        self.format("%Y-%m-%dT%H:%M:%SZ")
    }

    /// Substitutes `%Y`, `%m`, `%d`, `%H`, `%M` and `%S`, which cover every
    /// daily-note format in practice.
    #[must_use]
    pub fn format(&self, format: &str) -> String {
        format
            .replace("%Y", &format!("{:04}", self.year))
            .replace("%m", &format!("{:02}", self.month))
            .replace("%d", &format!("{:02}", self.day))
            .replace("%H", &format!("{:02}", self.hour))
            .replace("%M", &format!("{:02}", self.minute))
            .replace("%S", &format!("{:02}", self.second))
    }
}

/// How long ago `then` was, the way a person says it: `just now`, `5m ago`,
/// `3h ago`, `2d ago`, `6w ago`. Precise enough to tell stale from fresh, which
/// is all it is read for.
#[must_use]
pub fn ago(then: u64, now: u64) -> String {
    let secs = now.saturating_sub(then);
    match secs {
        0..60 => "just now".to_string(),
        60..3600 => format!("{}m ago", secs / 60),
        3600..86_400 => format!("{}h ago", secs / 3600),
        86_400..1_209_600 => format!("{}d ago", secs / 86_400),
        _ => format!("{}w ago", secs / 604_800),
    }
}

/// Days since the Unix epoch to a civil date (Howard Hinnant's algorithm).
#[must_use]
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_match_known_values() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        // A leap day, which is where naive date math usually breaks.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn a_moment_is_written_as_iso_and_by_format() {
        let utc = Utc::at(1_792_674_005);
        assert_eq!(utc.iso(), "2026-10-22T13:00:05Z");
        assert_eq!(utc.format("%Y-%m-%d %H%M"), "2026-10-22 1300");
        assert_eq!(Utc::at(0).iso(), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn ages_are_said_the_way_a_person_says_them() {
        let now = 10_000_000;
        assert_eq!(ago(now, now), "just now");
        assert_eq!(ago(now - 300, now), "5m ago");
        assert_eq!(ago(now - 3 * 3600, now), "3h ago");
        assert_eq!(ago(now - 2 * 86_400, now), "2d ago");
        assert_eq!(ago(now - 30 * 86_400, now), "4w ago");
        assert_eq!(ago(now + 50, now), "just now", "a clock skewed ahead");
    }
}
