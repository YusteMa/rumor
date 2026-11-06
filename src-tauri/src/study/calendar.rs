//! "Now" in local time and the first day of the week. The week starts on the
//! day Windows' regional format says (Monday in Spain), so it follows the
//! user's language and region without a setting of its own. Day and week
//! arithmetic lives in `stats.rs`.

use chrono::{DateTime, Local, Weekday};

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
