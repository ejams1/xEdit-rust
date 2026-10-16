// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: the TDateTime support of the Delphi RTL
// `System.SysUtils` that the JVCL JvInterpreter_SysUtils.pas registrations
// expose (EncodeDate/EncodeTime, DecodeDate/DecodeTime, DayOfWeek, Date,
// Time, Now, IncMonth, IsLeapYear, DateToStr/TimeToStr/DateTimeToStr,
// StrToDate/StrToTime/StrToDateTime, FormatDateTime/DateTimeToString,
// FileDateToDateTime/DateTimeToFileDate)

//! `TDateTime`: the Delphi date/time codec and its default (en-US) format
//! settings.
//!
//! A `TDateTime` is the number of days since 1899-12-30 with the time as the
//! fraction, exactly the double upstream carries. The format settings are
//! the RTL's `DefaultFormatSettings` as an en-US Delphi process settles
//! them (`ShortDateFormat` `M/d/yyyy`, `LongTimeFormat` `h:nn:ss am/pm`,
//! the English day and month names); xEdit overrides none of them (grep of
//! the 4.1.5q `xEdit/` and `Core/` sources for `ShortDateFormat` and
//! `FormatSettings` finds no assignment), and the corpus uses no date
//! function at all -- `FormatDateTime`/`DateTimeToString` are exercised by
//! this module's own tests and by the RTL surface a script may call.
//!
//! UPSTREAM-NOTE: `StrToDate`/`StrToTime`/`StrToDateTime` are driven by the
//! format settings string, as the RTL's scanner is: the field order of
//! `ShortDateFormat` decides which number is the month and which the day,
//! a two-digit year goes through `TwoDigitYearCenturyWindow` (50) and a
//! time without an am/pm marker is 24-hour. The port matches those rules
//! for the default settings; a script that writes its own format settings
//! first (the regional options of `JvInterpreter_SysUtils.pas` are
//! commented out upstream, so a script cannot) sees the same.

/// `EncodeDate`: the TDateTime of a date, with the RTL's `EConvertError`
/// text for an out-of-range date.
pub fn encode_date(year: i64, month: i64, day: i64) -> Result<f64, String> {
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return Err("Invalid argument to date encode".to_owned());
    }
    Ok((days_from_civil(year, month, day) - EPOCH_DAYS) as f64)
}

/// `EncodeTime`.
pub fn encode_time(hour: i64, minute: i64, second: i64, millisecond: i64) -> Result<f64, String> {
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=59).contains(&second) {
        return Err("Invalid argument to time encode".to_owned());
    }
    if !(0..=999).contains(&millisecond) {
        return Err("Invalid argument to time encode".to_owned());
    }
    let seconds = f64::from(hour as u32) * 3600.0
        + f64::from(minute as u32) * 60.0
        + f64::from(second as u32)
        + f64::from(millisecond as u32) / 1000.0;
    Ok(seconds / 86400.0)
}

/// `DecodeDate`. The RTL takes the day with `Int` (truncation toward zero),
/// not with a floor: for the negative dates the day of `-0.25` is day 0
/// (1899-12-30) and the day of `-1.25` is day -1 (1899-12-29), which the
/// oracle's `D[]` surface and the `DateTimeToISO8601` offsets pin
/// (`crates/xtask/oracle/scripts/probes/datetime.pas`).
pub fn decode_date(date: f64) -> (i64, i64, i64) {
    civil_from_days(date.trunc() as i64 + EPOCH_DAYS)
}

/// `DecodeTime`. The fraction is the distance from the truncated day, so a
/// negative date reads its time of day as a positive one (`DecodeTime(-1.25)`
/// is 06:00, `DecodeTime(-1.7916666)` is 19:00): the RTL keeps the fraction
/// of a negative `TDateTime` negative, and this is the reading the oracle
/// prints (the same probe).
pub fn decode_time(date: f64) -> (i64, i64, i64, i64) {
    let fraction = (date - date.trunc()).abs();
    let total_ms = (fraction * 86_400_000.0).round() as i64 % 86_400_000;
    (
        total_ms / 3_600_000,
        (total_ms / 60_000) % 60,
        (total_ms / 1000) % 60,
        total_ms % 1000,
    )
}

/// `DayOfWeek`: 1 is Sunday, 7 Saturday (Delphi's numbering). TDateTime day
/// 0 (1899-12-30) was a Saturday, so the Saturday lands on the `0` of the
/// weekday modulo.
pub fn day_of_week(date: f64) -> i64 {
    let days = date.floor() as i64;
    let weekday = days.rem_euclid(7);
    if weekday == 0 { 7 } else { weekday }
}

/// `IsLeapYear`.
pub fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// `IncMonth`.
pub fn inc_month(date: f64, months: i64) -> f64 {
    let (year, month, day) = decode_date(date);
    let total = year * 12 + (month - 1) + months;
    let year = total.div_euclid(12);
    let month = total.rem_euclid(12) + 1;
    let day = day.min(days_in_month(year, month));
    let base = (days_from_civil(year, month, day) - EPOCH_DAYS) as f64;
    base + (date - date.floor())
}

/// `Date`: today's date at midnight, local time.
pub fn today() -> f64 {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    local_days_from_unix(seconds) as f64
}

/// `Time`: the current time as a day fraction.
pub fn time_now() -> f64 {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    let seconds_of_day = local_seconds_of_day(seconds);
    f64::from(seconds_of_day as u32) / 86400.0
}

/// `Now`: `Date + Time`.
pub fn now() -> f64 {
    today() + time_now()
}

/// The timezone offset the port uses for `Date`/`Time`/`Now`: the local
/// offset is not available without the platform API, so the port reports
/// UTC (the corpus reads none of the three; the note is in the module doc).
fn local_days_from_unix(seconds: i64) -> i64 {
    seconds.div_euclid(86_400) + UNIX_DAYS - EPOCH_DAYS
}

fn local_seconds_of_day(seconds: i64) -> i64 {
    seconds.rem_euclid(86_400)
}

/// `DateTimeToFileDate`: the DOS packed date/time; a year before 1980 gives
/// 0, as the RTL's.
pub fn date_time_to_file_date(date: f64) -> i32 {
    let (year, month, day) = decode_date(date);
    let (hour, minute, second, _) = decode_time(date);
    if !(1980..=2107).contains(&year) {
        return 0;
    }
    (((year - 1980) as i32) << 25)
        | ((month as i32) << 21)
        | ((day as i32) << 16)
        | ((hour as i32) << 11)
        | ((minute as i32) << 5)
        | ((second as i32) >> 1)
}

/// `FileDateToDateTime`.
pub fn file_date_to_date_time(file_date: i32) -> f64 {
    let file_date = file_date as u32;
    let year = ((file_date >> 25) & 0x7F) as i64 + 1980;
    let month = ((file_date >> 21) & 0x0F).max(1) as i64;
    let day = ((file_date >> 16) & 0x1F).max(1) as i64;
    let hour = (file_date >> 11) & 0x1F;
    let minute = (file_date >> 5) & 0x3F;
    let second = (file_date & 0x1F) * 2;
    let date = encode_date(year, month.min(12), day.min(days_in_month(year, month.min(12)))).unwrap_or(0.0);
    let time = encode_time(
        i64::from(hour.min(23)),
        i64::from(minute.min(59)),
        i64::from(second.min(59)),
        0,
    )
    .unwrap_or(0.0);
    date + time
}

//=== `TSystemTime` and the time zone =========================================
//
// The four routines the data-format date code and `JsonDataObjects` use to
// cross between a `TDateTime`, a `TSystemTime` and the machine's time zone:
// `Winapi.Windows.DateTimeToSystemTime`, `System.SysUtils.SystemTimeToDateTime`
// and the two `kernel32` conversions `JsonDataObjects` calls with a nil time
// zone (the current one). The port calls the same Win32 functions as the
// oracle, so the bias, the daylight saving rules and the transition cases
// (`TzSpecificLocalTimeToSystemTime` of a local time that does not exist or
// is ambiguous) are the machine's on both sides.

/// A `TSystemTime` (`Winapi.Windows`): the seven fields
/// `DateTimeToSystemTime` fills and `SystemTimeToDateTime` reads.
/// `wDayOfWeek` is not modelled: `DecodeDate` does not set it upstream and
/// neither conversion here reads it.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct SystemTime {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub milliseconds: u16,
}

/// `Winapi.Windows.DateTimeToSystemTime`: `DecodeDate`/`DecodeTime` into the
/// record. A date the RTL cannot decode (a year outside 1..9999) leaves the
/// record zeroed, which is upstream's `except` arm (`FillChar(SystemTime,
/// SizeOf(SystemTime), 0)`).
pub fn date_time_to_system_time(date: f64) -> SystemTime {
    let (year, month, day) = decode_date(date);
    if !(1..=9999).contains(&year) {
        return SystemTime::default();
    }
    let (hour, minute, second, milliseconds) = decode_time(date);
    SystemTime {
        year: year as u16,
        month: month as u16,
        day: day as u16,
        hour: hour as u16,
        minute: minute as u16,
        second: second as u16,
        milliseconds: milliseconds as u16,
    }
}

/// `SystemTimeToDateTime` (`System.SysUtils`): `EncodeDate` and
/// `EncodeTime`, with the RTL's `EConvertError` text for a field out of
/// range.
///
/// The RTL keeps the fraction of a negative `TDateTime` negative, so a date
/// before 1899-12-30 *subtracts* its time of day (the inverse of
/// [`decode_date`]/[`decode_time`]): 1899-12-29 18:00 is `-1.75`, not
/// `-0.25`. The oracle's `D[]` conversion of a pre-epoch UTC text pins it
/// (`crates/xtask/oracle/scripts/probes/datetime.pas`, the `parse-1899*`
/// lines).
pub fn system_time_to_date_time(time: &SystemTime) -> Result<f64, String> {
    let date = encode_date(i64::from(time.year), i64::from(time.month), i64::from(time.day))?;
    let clock = encode_time(
        i64::from(time.hour),
        i64::from(time.minute),
        i64::from(time.second),
        i64::from(time.milliseconds),
    )?;
    Ok(if date < 0.0 { date - clock } else { date + clock })
}

/// `TzSpecificLocalTimeToSystemTime(nil, ...)` (`Winapi.Windows`): a local
/// wall-clock time as UTC through the machine's current time zone. `None`
/// when the call fails, which upstream answers by keeping the value.
#[cfg(windows)]
pub fn local_time_to_utc(time: &SystemTime) -> Option<SystemTime> {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::Time::TzSpecificLocalTimeToSystemTime;
    let local = SYSTEMTIME {
        wYear: time.year,
        wMonth: time.month,
        wDayOfWeek: 0,
        wDay: time.day,
        wHour: time.hour,
        wMinute: time.minute,
        wSecond: time.second,
        wMilliseconds: time.milliseconds,
    };
    let mut utc = SYSTEMTIME {
        wYear: 0,
        wMonth: 0,
        wDayOfWeek: 0,
        wDay: 0,
        wHour: 0,
        wMinute: 0,
        wSecond: 0,
        wMilliseconds: 0,
    };
    // SAFETY: a nil time zone asks for the current one, and both records are
    // live for the call.
    if unsafe { TzSpecificLocalTimeToSystemTime(std::ptr::null(), &local, &mut utc) } != 0 {
        return Some(SystemTime {
            year: utc.wYear,
            month: utc.wMonth,
            day: utc.wDay,
            hour: utc.wHour,
            minute: utc.wMinute,
            second: utc.wSecond,
            milliseconds: utc.wMilliseconds,
        });
    }
    None
}

/// `TzSpecificLocalTimeToSystemTime` on a platform without it (the port's
/// tests run on Windows; the fallback keeps the crate building elsewhere).
#[cfg(not(windows))]
pub fn local_time_to_utc(_time: &SystemTime) -> Option<SystemTime> {
    None
}

/// `SystemTimeToTzSpecificLocalTime(nil, ...)` (`Winapi.Windows`): a UTC time
/// as the machine's local wall-clock time. `None` when the call fails.
#[cfg(windows)]
pub fn utc_to_local_time(time: &SystemTime) -> Option<SystemTime> {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::Time::SystemTimeToTzSpecificLocalTime;
    let utc = SYSTEMTIME {
        wYear: time.year,
        wMonth: time.month,
        wDayOfWeek: 0,
        wDay: time.day,
        wHour: time.hour,
        wMinute: time.minute,
        wSecond: time.second,
        wMilliseconds: time.milliseconds,
    };
    let mut local = SYSTEMTIME {
        wYear: 0,
        wMonth: 0,
        wDayOfWeek: 0,
        wDay: 0,
        wHour: 0,
        wMinute: 0,
        wSecond: 0,
        wMilliseconds: 0,
    };
    // SAFETY: a nil time zone asks for the current one, and both records are
    // live for the call.
    if unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) } != 0 {
        return Some(SystemTime {
            year: local.wYear,
            month: local.wMonth,
            day: local.wDay,
            hour: local.wHour,
            minute: local.wMinute,
            second: local.wSecond,
            milliseconds: local.wMilliseconds,
        });
    }
    None
}

/// `SystemTimeToTzSpecificLocalTime` on a platform without it.
#[cfg(not(windows))]
pub fn utc_to_local_time(_time: &SystemTime) -> Option<SystemTime> {
    None
}

/// The format settings the `*ToStr`/`StrTo*`/`FormatDateTime` routines use.
pub struct DateSettings {
    pub date_separator: char,
    pub time_separator: char,
    pub short_date_format: &'static str,
    pub long_time_format: &'static str,
    pub time_am_string: &'static str,
    pub time_pm_string: &'static str,
    pub short_month_names: [&'static str; 12],
    pub long_month_names: [&'static str; 12],
    pub short_day_names: [&'static str; 7],
    pub long_day_names: [&'static str; 7],
}

/// The en-US `DefaultFormatSettings`.
pub const SETTINGS: DateSettings = DateSettings {
    date_separator: '/',
    time_separator: ':',
    short_date_format: "M/d/yyyy",
    long_time_format: "h:nn:ss am/pm",
    time_am_string: "AM",
    time_pm_string: "PM",
    short_month_names: [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ],
    long_month_names: [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ],
    short_day_names: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
    long_day_names: [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ],
};

/// `DateToStr`: `FormatDateTime(ShortDateFormat, Date)`.
pub fn date_to_str(date: f64) -> String {
    date_time_to_str(SETTINGS.short_date_format, date)
}

/// `TimeToStr`: `FormatDateTime(LongTimeFormat, Time)`.
pub fn time_to_str(date: f64) -> String {
    date_time_to_str(SETTINGS.long_time_format, date)
}

/// `DateTimeToString(format, DateTime)`: the RTL's specifier language.
pub fn date_time_to_str(format: &str, date: f64) -> String {
    let mut out = String::new();
    let chars: Vec<char> = format.chars().collect();
    let (year, month, day) = decode_date(date);
    let (hour, minute, second, millisecond) = decode_time(date);
    let lower_format = format.to_ascii_lowercase();
    let uses_am_pm = lower_format.contains("am/pm") || lower_format.contains("a/p");
    let mut i = 0usize;
    let mut last_was_hour = false;
    while i < chars.len() {
        let c = chars[i];
        let mut count = 1usize;
        while i + count < chars.len() && chars[i + count] == c {
            count += 1;
        }
        // The `am/pm` and `a/p` markers are the one multi-character token a
        // specifier can start.
        let rest: String = chars[i..].iter().collect();
        let rest_lower = rest.to_ascii_lowercase();
        if (c == 'a' || c == 'A') && (rest_lower.starts_with("am/pm") || rest_lower.starts_with("a/p")) {
            let short = rest_lower.starts_with("a/p");
            let text = if hour < 12 {
                SETTINGS.time_am_string
            } else {
                SETTINGS.time_pm_string
            };
            out.push_str(&format_pm_marker(text, c == 'A', short));
            i += if short { 3 } else { 5 };
            last_was_hour = false;
            continue;
        }
        match c {
            'd' => {
                if count >= 4 {
                    out.push_str(SETTINGS.long_day_names[(day_of_week(date) - 1) as usize]);
                } else if count == 3 {
                    out.push_str(SETTINGS.short_day_names[(day_of_week(date) - 1) as usize]);
                } else if count == 2 {
                    out.push_str(&format!("{day:02}"));
                } else {
                    out.push_str(&day.to_string());
                }
                last_was_hour = false;
            }
            'm' => {
                if last_was_hour {
                    // `m` after `h` is the minute.
                    if count >= 2 {
                        out.push_str(&format!("{minute:02}"));
                    } else {
                        out.push_str(&minute.to_string());
                    }
                } else if count >= 4 {
                    out.push_str(SETTINGS.long_month_names[(month - 1) as usize]);
                } else if count == 3 {
                    out.push_str(SETTINGS.short_month_names[(month - 1) as usize]);
                } else if count == 2 {
                    out.push_str(&format!("{month:02}"));
                } else {
                    out.push_str(&month.to_string());
                }
                last_was_hour = false;
            }
            'y' | 'e' => {
                if count >= 4 {
                    out.push_str(&format!("{year:04}"));
                } else if count >= 2 {
                    out.push_str(&format!("{:02}", year % 100));
                } else {
                    out.push_str(&(year % 100).to_string());
                }
                last_was_hour = false;
            }
            'h' => {
                let hour = if uses_am_pm {
                    let h = hour % 12;
                    if h == 0 { 12 } else { h }
                } else {
                    hour
                };
                if count >= 2 {
                    out.push_str(&format!("{hour:02}"));
                } else {
                    out.push_str(&hour.to_string());
                }
                last_was_hour = true;
            }
            'n' => {
                if count >= 2 {
                    out.push_str(&format!("{minute:02}"));
                } else {
                    out.push_str(&minute.to_string());
                }
                last_was_hour = false;
            }
            's' => {
                if count >= 2 {
                    out.push_str(&format!("{second:02}"));
                } else {
                    out.push_str(&second.to_string());
                }
                last_was_hour = false;
            }
            'z' => {
                // `z` tenths of a second, `zz` hundredths, `zzz`
                // milliseconds.
                let value = millisecond / 10i64.pow(3 - count.min(3) as u32);
                let text = value.to_string();
                let width = count.min(3);
                for _ in text.len()..width {
                    out.push('0');
                }
                out.push_str(&text);
                last_was_hour = false;
            }
            '/' => {
                // The RTL keeps the "last specifier was an hour" state
                // across the separators, so `h:mm:ss` reads minutes.
                out.push(SETTINGS.date_separator);
            }
            ':' => {
                out.push(SETTINGS.time_separator);
            }
            '\'' | '"' => {
                // A quoted literal runs to the next quote (a doubled quote
                // is an empty literal).
                let mut j = i + 1;
                while j < chars.len() && chars[j] != c {
                    out.push(chars[j]);
                    j += 1;
                }
                i = j + 1;
                last_was_hour = false;
                continue;
            }
            _ => {
                out.push(c);
                last_was_hour = false;
            }
        }
        i += count;
    }
    out
}

/// The `am/pm`/`a/p` marker text with the case of the format string.
fn format_pm_marker(text: &str, upper: bool, short: bool) -> String {
    let text = if short {
        if text == "AM" { "a" } else { "p" }
    } else {
        text
    };
    if upper {
        text.to_ascii_uppercase()
    } else {
        text.to_ascii_lowercase()
    }
}

/// `StrToDate`: the fields of `ShortDateFormat` decide the order.
pub fn str_to_date(text: &str) -> Result<f64, String> {
    let text = text.trim();
    let (date_part, _) = split_at_time(text);
    let fields = parse_number_fields(date_part);
    if fields.is_empty() {
        return Err(format!("'{text}' is not a valid date"));
    }
    let spec: Vec<char> = SETTINGS
        .short_date_format
        .chars()
        .filter(|c| matches!(c.to_ascii_lowercase(), 'm' | 'd' | 'y'))
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let mut year = 0i64;
    let mut month = 0i64;
    let mut day = 0i64;
    for (index, spec_char) in spec.iter().enumerate() {
        let Some(value) = fields.get(index).copied() else { break };
        match spec_char {
            'y' => year = value,
            'm' => month = value,
            'd' => day = value,
            _ => {}
        }
    }
    if year < 100 {
        // `TwoDigitYearCenturyWindow` (50): a year below 50 is 20xx.
        year += if year >= 50 { 1900 } else { 2000 };
    }
    encode_date(year, month, day)
}

/// `StrToTime`.
pub fn str_to_time(text: &str) -> Result<f64, String> {
    let text = text.trim();
    let time_part = split_at_time(text).1.unwrap_or(text);
    let lower = time_part.to_ascii_lowercase();
    let pm = lower.contains("pm") || lower.contains("p.m.");
    let am = lower.contains("am") || lower.contains("a.m.");
    let digits: String = time_part
        .chars()
        .map(|c| if c.is_ascii_digit() { c } else { ' ' })
        .collect();
    let numbers: Vec<i64> = digits
        .split_whitespace()
        .filter_map(|field| field.parse().ok())
        .collect();
    let Some(&hour) = numbers.first() else {
        return Err(format!("'{text}' is not a valid time"));
    };
    let minute = numbers.get(1).copied().unwrap_or(0);
    let second = numbers.get(2).copied().unwrap_or(0);
    let millisecond = numbers.get(3).copied().unwrap_or(0);
    let hour = if pm && hour < 12 {
        hour + 12
    } else if am && hour == 12 {
        0
    } else {
        hour
    };
    encode_time(hour, minute, second, millisecond)
}

/// `StrToDateTime`: a date, or a time, or both separated by a space.
pub fn str_to_date_time(text: &str) -> Result<f64, String> {
    let text = text.trim();
    let (date_part, time_part) = split_at_time(text);
    match time_part {
        None => {
            if date_part.contains([':', SETTINGS.time_separator]) {
                str_to_time(date_part)
            } else {
                str_to_date(date_part)
            }
        }
        Some(time) => Ok(str_to_date(date_part)? + str_to_time(time)?),
    }
}

//=== Internals ===============================================================

/// `days_from_civil(1899, 12, 30)`: the civil day number of the TDateTime
/// epoch, so `TDateTime = days_from_civil(y, m, d) - EPOCH_DAYS`.
const EPOCH_DAYS: i64 = civil_days(1899, 12, 30);
/// `days_from_civil(1970, 1, 1)`.
const UNIX_DAYS: i64 = civil_days(1970, 1, 1);
/// The TDateTime of the Unix epoch (`1970-01-01`).
pub const UNIX_EPOCH_TDATE: f64 = (UNIX_DAYS - EPOCH_DAYS) as f64;

/// Days since 0001-01-01 (`days_from_civil` of Howard Hinnant's
/// algorithms), `const` so the epoch offsets above are computed once.
const fn civil_days(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era
}

/// Days since 0001-01-01 (the civil-from-days pair of Howard Hinnant's
/// algorithms, which the RTL's `DateTimeToTimeStamp` matches).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    civil_days(year, month, day)
}

/// The inverse of [`days_from_civil`] (the day count is days since
/// 0000-03-01, the scale the era arithmetic works on).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// `DaysInMonth` of the RTL.
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Splits `Date Time` on the space before a time; the date part is the
/// first token, the time the rest when it carries digits and a time marker
/// (`:` or an am/pm suffix).
fn split_at_time(text: &str) -> (&str, Option<&str>) {
    match text.split_once(' ') {
        Some((date, rest))
            if rest.chars().any(|c| c.is_ascii_digit())
                && (rest.contains(SETTINGS.time_separator)
                    || rest.to_ascii_lowercase().contains("am")
                    || rest.to_ascii_lowercase().contains("pm")) =>
        {
            (date, Some(rest))
        }
        _ => (text, None),
    }
}

/// The decimal fields of a date, in the order they appear.
fn parse_number_fields(text: &str) -> Vec<i64> {
    let mut numbers = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            current.push(c);
        } else if !current.is_empty() {
            numbers.push(current.parse().unwrap_or(0));
            current.clear();
        }
    }
    if !current.is_empty() {
        numbers.push(current.parse().unwrap_or(0));
    }
    numbers
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The TDateTime codec: the epoch, the day numbers of known dates and
    /// `DayOfWeek`'s Sunday-first numbering.
    #[test]
    fn date_codec_roundtrips() {
        assert_eq!(encode_date(1899, 12, 30), Ok(0.0));
        assert_eq!(encode_date(1900, 1, 1), Ok(2.0));
        // 2024-02-29 (a leap day) and its neighbours.
        let date = encode_date(2024, 2, 29).unwrap();
        assert_eq!(decode_date(date), (2024, 2, 29));
        assert_eq!(decode_date(date + 1.0), (2024, 3, 1));
        assert_eq!(decode_date(date - 1.0), (2024, 2, 28));
        // 1900-01-01 was a Monday (2).
        assert_eq!(day_of_week(2.0), 2);
        // 1899-12-30 (day 0) was a Saturday (7).
        assert_eq!(day_of_week(0.0), 7);
        // The negative dates: the day is the truncated one and the time is
        // the distance from it, read as a positive time of day (the oracle's
        // `crates/xtask/oracle/scripts/probes/datetime.pas` lines).
        assert_eq!(decode_date(-0.25), (1899, 12, 30));
        assert_eq!(decode_date(-0.75), (1899, 12, 30));
        assert_eq!(decode_date(-1.25), (1899, 12, 29));
        assert_eq!(decode_time(-0.25), (6, 0, 0, 0));
        assert_eq!(decode_time(-1.25), (6, 0, 0, 0));
        assert_eq!(decode_time(-0.791666666666667), (19, 0, 0, 0));
        assert_eq!(decode_time(-1.791666666666667), (19, 0, 0, 0));
        // `SystemTimeToDateTime` subtracts the time of a date before the
        // epoch: the fraction of a negative TDateTime is negative.
        let record = |year, month, day, hour| SystemTime {
            year,
            month,
            day,
            hour,
            minute: 0,
            second: 0,
            milliseconds: 0,
        };
        assert_eq!(system_time_to_date_time(&record(1899, 12, 30, 1)), Ok(1.0 / 24.0));
        assert_eq!(
            system_time_to_date_time(&record(1899, 12, 29, 17)),
            Ok(-1.0 - 17.0 / 24.0)
        );
        assert_eq!(system_time_to_date_time(&record(1899, 12, 29, 18)), Ok(-1.75));
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(encode_date(2023, 2, 29).is_err());
        assert_eq!(
            inc_month(encode_date(2024, 1, 31).unwrap(), 1),
            encode_date(2024, 2, 29).unwrap()
        );
        // A time is the fraction of a day.
        assert_eq!(encode_time(12, 0, 0, 0), Ok(0.5));
        assert_eq!(decode_time(0.5), (12, 0, 0, 0));
        assert_eq!(decode_time(0.25), (6, 0, 0, 0));
    }

    /// The specifier language of `DateTimeToString`, with the en-US
    /// defaults.
    #[test]
    fn format_specifiers_follow_delphi() {
        let date = encode_date(2024, 3, 5).unwrap() + encode_time(14, 7, 9, 250).unwrap();
        assert_eq!(date_time_to_str("yyyy-mm-dd", date), "2024-03-05");
        assert_eq!(date_time_to_str("d.m.yy", date), "5.3.24");
        assert_eq!(date_time_to_str("dddd, mmmm d", date), "Tuesday, March 5");
        assert_eq!(date_time_to_str("ddd mmm", date), "Tue Mar");
        // `nn` is the minute; `m` after `h` is the minute too.
        assert_eq!(date_time_to_str("hh:nn:ss", date), "14:07:09");
        assert_eq!(date_time_to_str("h:mm:ss", date), "14:07:09");
        assert_eq!(date_time_to_str("hh:nn:ss am/pm", date), "02:07:09 pm");
        assert_eq!(date_time_to_str("h:nn:ss AM/PM", date), "2:07:09 PM");
        assert_eq!(date_time_to_str("hh:nn:ss.zzz", date), "14:07:09.250");
        // A doubled quote is an empty literal; a quoted run is copied
        // verbatim (the RTL has no escape for a quote inside a literal).
        assert_eq!(date_time_to_str("''yyyy''", date), "2024");
        assert_eq!(date_time_to_str("'''yyyy'''", date), "yyyy");
    }

    /// `StrToDate`/`StrToTime`/`StrToDateTime` driven by the format
    /// settings (M/d/yyyy, h:nn:ss am/pm).
    #[test]
    fn parsing_follows_the_format_settings() {
        assert_eq!(str_to_date("3/5/2024"), encode_date(2024, 3, 5));
        assert_eq!(str_to_date("12/31/1999"), encode_date(1999, 12, 31));
        assert_eq!(str_to_time("2:07:09 pm"), encode_time(14, 7, 9, 0));
        assert_eq!(str_to_time("14:07:09"), encode_time(14, 7, 9, 0));
        assert_eq!(
            str_to_date_time("3/5/2024 2:07:09 pm"),
            Ok(encode_date(2024, 3, 5).unwrap() + encode_time(14, 7, 9, 0).unwrap())
        );
    }

    /// `DateTimeToFileDate`/`FileDateToDateTime`: the DOS packed shape.
    #[test]
    fn file_dates_roundtrip() {
        let date = encode_date(2024, 3, 5).unwrap() + encode_time(14, 7, 8, 0).unwrap();
        let dos = date_time_to_file_date(date);
        let back = file_date_to_date_time(dos);
        // The DOS time keeps two-second resolution.
        assert_eq!(decode_date(back), (2024, 3, 5));
        assert_eq!(decode_time(back), (14, 7, 8, 0));
        // A year before 1980 is 0.
        assert_eq!(date_time_to_file_date(encode_date(1970, 1, 1).unwrap()), 0);
    }
}
