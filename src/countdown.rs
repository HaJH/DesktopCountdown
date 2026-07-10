//! Pure countdown arithmetic. No Win32, no I/O.

use jiff::{Unit, Zoned};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Breakdown {
    /// Whole calendar months remaining.
    pub months: i64,
    /// Whole weeks in the remainder after `months`.
    pub weeks: i64,
    /// Whole days in the remainder after `weeks`.
    pub days: i64,
    /// Total hours remaining. Unbounded; not reduced by `months`/`weeks`/`days`.
    pub total_hours: i64,
    pub minutes: i64,
    pub seconds: i64,
    pub expired: bool,
}

const EXPIRED: Breakdown = Breakdown {
    months: 0,
    weeks: 0,
    days: 0,
    total_hours: 0,
    minutes: 0,
    seconds: 0,
    expired: true,
};

pub fn breakdown(now: &Zoned, target: &Zoned) -> Breakdown {
    let secs = target.timestamp().as_second() - now.timestamp().as_second();
    if secs <= 0 {
        return EXPIRED;
    }

    // Calendar units for the summary line. `until` clamps month-end overflow
    // (Jan 31 + 1 month => Feb 28/29), which is what the spec asks for.
    let span = now
        .until((Unit::Month, target))
        .expect("calendar difference between two zoned datetimes");
    let rem_days = span.get_days();

    Breakdown {
        months: span.get_months() as i64,
        weeks: (rem_days / 7) as i64,
        days: (rem_days % 7) as i64,
        total_hours: secs / 3600,
        minutes: (secs / 60) % 60,
        seconds: secs % 60,
        expired: false,
    }
}

/// `"2544:18:07"` — hours are zero-padded to at least two digits and grow freely.
pub fn format_main(b: &Breakdown) -> String {
    format!("{:02}:{:02}:{:02}", b.total_hours, b.minutes, b.seconds)
}

/// `"3m 2w 0d"` — auxiliary summary. Months are unbounded; years are never used.
pub fn format_summary(b: &Breakdown) -> String {
    format!("{}m {}w {}d", b.months, b.weeks, b.days)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::{civil::datetime, tz::{offset, TimeZone}, Zoned};

    fn z(y: i16, m: i8, d: i8, h: i8, mi: i8, s: i8) -> Zoned {
        datetime(y, m, d, h, mi, s, 0)
            .to_zoned(TimeZone::fixed(offset(9)))
            .unwrap()
    }

    #[test]
    fn one_second_before_target() {
        let b = breakdown(&z(2026, 10, 24, 8, 59, 59), &z(2026, 10, 24, 9, 0, 0));
        assert!(!b.expired);
        assert_eq!(format_main(&b), "00:00:01");
        assert_eq!(format_summary(&b), "0m 0w 0d");
    }

    #[test]
    fn exactly_at_target_is_expired() {
        let t = z(2026, 10, 24, 9, 0, 0);
        let b = breakdown(&t, &t);
        assert!(b.expired);
        assert_eq!(format_main(&b), "00:00:00");
        assert_eq!(format_summary(&b), "0m 0w 0d");
    }

    #[test]
    fn past_target_stays_at_zero() {
        let b = breakdown(&z(2026, 10, 25, 0, 0, 0), &z(2026, 10, 24, 9, 0, 0));
        assert!(b.expired);
        assert_eq!(format_main(&b), "00:00:00");
    }

    #[test]
    fn hour_digits_grow_past_two() {
        // 4 days 4 hours = 100 hours exactly.
        let b = breakdown(&z(2026, 10, 20, 5, 0, 0), &z(2026, 10, 24, 9, 0, 0));
        assert_eq!(format_main(&b), "100:00:00");
    }

    #[test]
    fn hour_digits_shrink_to_two() {
        let b = breakdown(&z(2026, 10, 20, 5, 0, 1), &z(2026, 10, 24, 9, 0, 0));
        assert_eq!(format_main(&b), "99:59:59");
    }

    #[test]
    fn summary_splits_months_weeks_days() {
        // 2026-07-10 09:00 -> 2026-10-24 09:00 is 106 days = 3 months + 14 days.
        let b = breakdown(&z(2026, 7, 10, 9, 0, 0), &z(2026, 10, 24, 9, 0, 0));
        assert_eq!(format_summary(&b), "3m 2w 0d");
        assert_eq!(format_main(&b), "2544:00:00");
    }

    #[test]
    fn month_end_clamps_to_shorter_month() {
        // Jan 31 + 1 month clamps to Feb 28, leaving 1 day to Mar 1.
        let b = breakdown(&z(2026, 1, 31, 0, 0, 0), &z(2026, 3, 1, 0, 0, 0));
        assert_eq!(format_summary(&b), "1m 0w 1d");
    }

    #[test]
    fn leap_day_is_handled() {
        // 2028 is a leap year: Jan 31 + 1 month clamps to Feb 29, leaving 1 day.
        let b = breakdown(&z(2028, 1, 31, 0, 0, 0), &z(2028, 3, 1, 0, 0, 0));
        assert_eq!(format_summary(&b), "1m 0w 1d");
    }

    #[test]
    fn months_have_no_upper_bound() {
        let b = breakdown(&z(2026, 1, 1, 0, 0, 0), &z(2027, 7, 1, 0, 0, 0));
        assert_eq!(format_summary(&b), "18m 0w 0d");
    }
}
