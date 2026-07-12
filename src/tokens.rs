//! Token substitution for line templates. No Win32, no I/O.

use crate::countdown::Breakdown;

/// Replaces every `{token}` in `template` with its value from `b`.
///
/// An unknown token, or a `{` with no `}`, is copied through unchanged: a typo shows up on
/// the wallpaper instead of silently vanishing.
pub fn render(template: &str, b: &Breakdown) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let name = &after[..close];
        match value(name, b) {
            Some(v) => out.push_str(&v),
            None => {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

fn value(name: &str, b: &Breakdown) -> Option<String> {
    let total_minutes = b.total_hours * 60 + b.minutes;
    let total_seconds = total_minutes * 60 + b.seconds;
    Some(match name {
        "months" => b.months.to_string(),
        "weeks" => b.weeks.to_string(),
        "days" => b.days.to_string(),
        "daysTotal" => (b.total_hours / 24).to_string(),
        "hoursTotal" => b.total_hours.to_string(),
        "minutesTotal" => total_minutes.to_string(),
        "secondsTotal" => total_seconds.to_string(),
        "hours" => (b.total_hours % 24).to_string(),
        "minutes" => b.minutes.to_string(),
        "seconds" => b.seconds.to_string(),
        "hh" => format!("{:02}", b.total_hours),
        "mm" => format!("{:02}", b.minutes),
        "ss" => format!("{:02}", b.seconds),
        _ => return None,
    })
}

/// Every token, with the description the settings window shows next to it.
pub const TOKENS: [(&str, &str); 13] = [
    ("{months}", "whole calendar months left"),
    ("{weeks}", "whole weeks after the months"),
    ("{days}", "whole days after the weeks"),
    ("{daysTotal}", "total whole days left"),
    ("{hoursTotal}", "total whole hours left"),
    ("{minutesTotal}", "total whole minutes left"),
    ("{secondsTotal}", "total seconds left"),
    ("{hours}", "hours within the day (0-23)"),
    ("{minutes}", "minutes within the hour (0-59)"),
    ("{seconds}", "seconds within the minute (0-59)"),
    ("{hh}", "total hours, zero-padded to two digits"),
    ("{mm}", "minutes, zero-padded to two digits"),
    ("{ss}", "seconds, zero-padded to two digits"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::{
        civil::datetime,
        tz::{offset, TimeZone},
        Zoned,
    };

    fn z(y: i16, m: i8, d: i8, h: i8, mi: i8, s: i8) -> Zoned {
        datetime(y, m, d, h, mi, s, 0)
            .to_zoned(TimeZone::fixed(offset(9)))
            .unwrap()
    }

    /// 2026-07-12 09:00 -> 2026-10-24 09:00 = 104 days = 2496 hours.
    fn b() -> Breakdown {
        crate::countdown::breakdown(&z(2026, 7, 12, 9, 0, 0), &z(2026, 10, 24, 9, 0, 0))
    }

    #[test]
    fn main_template_matches_the_old_clock_format() {
        assert_eq!(render("{hh}:{mm}:{ss}", &b()), "2496:00:00");
    }

    #[test]
    fn summary_template_matches_the_old_summary_format() {
        assert_eq!(render("{months}m {weeks}w {days}d", &b()), "3m 1w 5d");
    }

    #[test]
    fn zero_padding_applies_to_minutes_and_seconds() {
        let b = crate::countdown::breakdown(&z(2026, 10, 24, 8, 59, 53), &z(2026, 10, 24, 9, 0, 0));
        assert_eq!(render("{hh}:{mm}:{ss}", &b), "00:00:07");
        assert_eq!(render("{minutes}:{seconds}", &b), "0:7");
    }

    #[test]
    fn totals_are_derived_from_the_breakdown() {
        let b = b();
        assert_eq!(render("{daysTotal}", &b), "104");
        assert_eq!(render("{hoursTotal}", &b), "2496");
        assert_eq!(render("{minutesTotal}", &b), "149760");
        assert_eq!(render("{secondsTotal}", &b), "8985600");
        assert_eq!(render("{hours}", &b), "0");
    }

    #[test]
    fn unknown_tokens_are_left_alone_so_typos_are_visible() {
        assert_eq!(render("{dayz} left", &b()), "{dayz} left");
    }

    #[test]
    fn unmatched_braces_are_left_alone() {
        assert_eq!(render("100% {done", &b()), "100% {done");
        assert_eq!(render("a } b", &b()), "a } b");
    }

    #[test]
    fn plain_text_passes_through_including_non_ascii() {
        assert_eq!(render("수능까지 {daysTotal}일", &b()), "수능까지 104일");
    }

    #[test]
    fn expired_renders_zeroes() {
        let t = z(2026, 10, 24, 9, 0, 0);
        let b = crate::countdown::breakdown(&t, &t);
        assert_eq!(render("{hh}:{mm}:{ss} / {daysTotal}", &b), "00:00:00 / 0");
    }

    #[test]
    fn every_advertised_token_resolves() {
        let b = b();
        for (token, _) in TOKENS {
            assert_ne!(
                render(token, &b),
                token,
                "{token} is advertised but does not resolve"
            );
        }
    }
}
