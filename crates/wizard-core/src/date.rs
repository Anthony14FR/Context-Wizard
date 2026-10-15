//! Calendar dates in UTC, formatted as `YYYY-MM-DD`.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// A proleptic Gregorian calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    year: i64,
    month: u8,
    day: u8,
}

impl Date {
    /// Returns today's date in UTC. A clock set before 1970 yields 1970-01-01.
    #[must_use]
    pub fn today() -> Self {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        Self::from_unix_days(i64::try_from(seconds / 86_400).unwrap_or(i64::MAX))
    }

    /// Converts a number of days since 1970-01-01 into a date.
    #[must_use]
    pub fn from_unix_days(days: i64) -> Self {
        let shifted = days.saturating_add(719_468);
        let era = shifted.div_euclid(146_097);
        let day_of_era = shifted.rem_euclid(146_097);
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_index = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_index + 2) / 5 + 1;
        let month = if month_index < 10 {
            month_index + 3
        } else {
            month_index - 9
        };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        Self {
            year,
            month: u8::try_from(month).unwrap_or(1),
            day: u8::try_from(day).unwrap_or(1),
        }
    }
}

impl fmt::Display for Date {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.year, self.month, self.day
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Date;

    #[test]
    fn converts_known_days() {
        assert_eq!(Date::from_unix_days(0).to_string(), "1970-01-01");
        assert_eq!(Date::from_unix_days(11_016).to_string(), "2000-02-29");
        assert_eq!(Date::from_unix_days(20_741).to_string(), "2026-10-15");
        assert_eq!(Date::from_unix_days(-1).to_string(), "1969-12-31");
    }

    #[test]
    fn today_is_after_release_of_this_code() {
        assert!(Date::today() >= Date::from_unix_days(20_741));
    }
}
