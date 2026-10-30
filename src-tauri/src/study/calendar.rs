//! "Today" and "this week" in local time, for the totals. The week starts on
//! the day Windows' regional format says (Monday in Spain), so it follows the
//! user's language and region without a setting of its own.

use std::time::SystemTime;

use chrono::{DateTime, Datelike, Days, Local, NaiveDate, TimeZone, Weekday};

/// Start of the day and of the week containing `now`, as instants.
pub fn bounds<Tz: TimeZone>(now: &DateTime<Tz>, week_starts: Weekday) -> (SystemTime, SystemTime) {
    let tz = now.timezone();
    let today = now.date_naive();
    let back = u64::from(today.weekday().days_since(week_starts));
    let week = today.checked_sub_days(Days::new(back)).unwrap_or(today);
    (midnight(&tz, today), midnight(&tz, week))
}

/// Local midnight; on a day whose midnight doesn't exist (a DST jump at
/// 00:00) the first instant of the day that does.
fn midnight<Tz: TimeZone>(tz: &Tz, day: NaiveDate) -> SystemTime {
    let naive = day.and_hms_opt(0, 0, 0).unwrap_or_default();
    let at = tz.from_local_datetime(&naive).earliest().or_else(|| {
        (1..=3)
            .filter_map(|h| {
                tz.from_local_datetime(&(naive + chrono::Duration::hours(h)))
                    .earliest()
            })
            .next()
    });
    at.map_or(SystemTime::UNIX_EPOCH, SystemTime::from)
}

pub fn now_local() -> DateTime<Local> {
    Local::now()
}

/// First day of the week from Windows' regional settings.
#[cfg(windows)]
pub fn week_start() -> Weekday {
    use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_IFIRSTDAYOFWEEK};

    let mut buf = [0u16; 4];
    // SAFETY: a null locale name is the user's default; `buf` is writable.
    let n = unsafe { GetLocaleInfoEx(None, LOCALE_IFIRSTDAYOFWEEK, Some(&mut buf)) };
    // "0" is Monday … "6" is Sunday.
    match (n > 0).then(|| buf[0].wrapping_sub(u16::from(b'0'))) {
        Some(d @ 0..=6) => Weekday::try_from(d as u8).unwrap_or(Weekday::Mon),
        _ => Weekday::Mon,
    }
}

#[cfg(not(windows))]
pub fn week_start() -> Weekday {
    Weekday::Mon
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn madrid() -> FixedOffset {
        FixedOffset::east_opt(3600).unwrap()
    }

    #[test]
    fn today_and_week_in_local_time() {
        let tz = madrid();
        // Thursday 2026-10-29, 00:30 local = Wednesday 23:30 UTC.
        let now = tz.with_ymd_and_hms(2026, 10, 29, 0, 30, 0).unwrap();
        let (today, week) = bounds(&now, Weekday::Mon);
        let expect_today = tz.with_ymd_and_hms(2026, 10, 29, 0, 0, 0).unwrap();
        let expect_week = tz.with_ymd_and_hms(2026, 10, 26, 0, 0, 0).unwrap();
        assert_eq!(today, SystemTime::from(expect_today));
        assert_eq!(week, SystemTime::from(expect_week));
    }

    #[test]
    fn sunday_first_weeks() {
        let tz = madrid();
        let now = tz.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap();
        let (today, week) = bounds(&now, Weekday::Sun);
        assert_eq!(today, week, "Sunday is the week's first day");
        let (_, monday_week) = bounds(&now, Weekday::Mon);
        let expect = tz.with_ymd_and_hms(2026, 10, 26, 0, 0, 0).unwrap();
        assert_eq!(monday_week, SystemTime::from(expect));
    }
}
