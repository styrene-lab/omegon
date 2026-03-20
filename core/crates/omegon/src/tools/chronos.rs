//! chronos — Authoritative date/time from the system clock.
//!
//! Pure computation, no I/O. The agent calls this instead of guessing dates.
//! Ported from extensions/chronos/ (668 LoC TS → ~200 LoC Rust).

use chrono::{Datelike, Duration, Local, NaiveDate, Weekday};

/// Execute the chronos subcommand and return formatted output.
pub fn execute(subcommand: &str, expression: Option<&str>, from: Option<&str>, to: Option<&str>) -> Result<String, String> {
    let now = Local::now();
    let today = now.date_naive();

    match subcommand {
        "week" => Ok(week(today)),
        "month" => Ok(month(today)),
        "quarter" => Ok(quarter(today)),
        "relative" => {
            let expr = expression.ok_or("'relative' requires an 'expression' parameter")?;
            relative(expr, today)
        }
        "iso" => Ok(iso(today)),
        "epoch" => Ok(epoch()),
        "tz" => Ok(tz(now)),
        "range" => {
            let f = from.ok_or("'range' requires 'from_date'")?;
            let t = to.ok_or("'range' requires 'to_date'")?;
            range(f, t)
        }
        "all" => Ok(all(today, now)),
        _ => Err(format!("Unknown subcommand: {subcommand}")),
    }
}

fn ymd(d: NaiveDate) -> String { d.format("%Y-%m-%d").to_string() }
fn dow(d: NaiveDate) -> &'static str {
    match d.weekday() {
        Weekday::Mon => "Monday", Weekday::Tue => "Tuesday", Weekday::Wed => "Wednesday",
        Weekday::Thu => "Thursday", Weekday::Fri => "Friday", Weekday::Sat => "Saturday",
        Weekday::Sun => "Sunday",
    }
}
fn short(d: NaiveDate) -> String { d.format("%b %-d").to_string() }

fn week(today: NaiveDate) -> String {
    let days_since_mon = today.weekday().num_days_from_monday() as i64;
    let mon = today - Duration::days(days_since_mon);
    let fri = mon + Duration::days(4);
    let prev_mon = mon - Duration::days(7);
    let prev_fri = prev_mon + Duration::days(4);
    let range = |m: NaiveDate, f: NaiveDate| {
        if m.year() == f.year() {
            format!("{} - {}, {}", short(m), short(f), f.year())
        } else {
            format!("{}, {} - {}, {}", short(m), m.year(), short(f), f.year())
        }
    };
    format!(
        "DATE_CONTEXT:\n  TODAY: {} ({})\n  CURR_WEEK_START: {} (Monday)\n  CURR_WEEK_END: {} (Friday)\n  CURR_WEEK_RANGE: {}\n  PREV_WEEK_START: {} (Monday)\n  PREV_WEEK_END: {} (Friday)\n  PREV_WEEK_RANGE: {}",
        ymd(today), dow(today), ymd(mon), ymd(fri), range(mon, fri),
        ymd(prev_mon), ymd(prev_fri), range(prev_mon, prev_fri),
    )
}

fn month(today: NaiveDate) -> String {
    let curr_start = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
    let curr_end = if today.month() == 12 {
        NaiveDate::from_ymd_opt(today.year() + 1, 1, 1).unwrap() - Duration::days(1)
    } else {
        NaiveDate::from_ymd_opt(today.year(), today.month() + 1, 1).unwrap() - Duration::days(1)
    };
    let prev = curr_start - Duration::days(1);
    let prev_start = NaiveDate::from_ymd_opt(prev.year(), prev.month(), 1).unwrap();
    format!(
        "MONTH_CONTEXT:\n  TODAY: {} ({})\n  CURR_MONTH_START: {}\n  CURR_MONTH_END: {}\n  CURR_MONTH_RANGE: {} - {}, {}\n  PREV_MONTH_START: {}\n  PREV_MONTH_END: {}\n  PREV_MONTH_RANGE: {}, {} - {}, {}",
        ymd(today), dow(today), ymd(curr_start), ymd(curr_end),
        short(curr_start), short(curr_end), today.year(),
        ymd(prev_start), ymd(prev),
        short(prev_start), prev_start.year(), short(prev), prev.year(),
    )
}

fn quarter(today: NaiveDate) -> String {
    let m = today.month();
    let q = ((m - 1) / 3) + 1;
    let q_start_m = (q - 1) * 3 + 1;
    let q_start = NaiveDate::from_ymd_opt(today.year(), q_start_m, 1).unwrap();
    let q_end = if q_start_m + 2 == 12 {
        NaiveDate::from_ymd_opt(today.year() + 1, 1, 1).unwrap() - Duration::days(1)
    } else {
        NaiveDate::from_ymd_opt(today.year(), q_start_m + 3, 1).unwrap() - Duration::days(1)
    };
    let (fy, fy_start, fy_end) = if m >= 10 {
        (today.year() + 1, format!("{}-10-01", today.year()), format!("{}-09-30", today.year() + 1))
    } else {
        (today.year(), format!("{}-10-01", today.year() - 1), format!("{}-09-30", today.year()))
    };
    let fy_month = if m >= 10 { m - 9 } else { m + 3 };
    let fq = ((fy_month - 1) / 3) + 1;
    format!(
        "QUARTER_CONTEXT:\n  TODAY: {} ({})\n  CALENDAR_QUARTER: Q{} {}\n  QUARTER_START: {}\n  QUARTER_END: {}\n  FISCAL_YEAR: FY{} (Oct-Sep)\n  FISCAL_QUARTER: FQ{}\n  FY_START: {}\n  FY_END: {}",
        ymd(today), dow(today), q, today.year(), ymd(q_start), ymd(q_end), fy, fq, fy_start, fy_end,
    )
}

fn resolve_relative(expr: &str, today: NaiveDate) -> Result<NaiveDate, String> {
    let e = expr.trim().to_lowercase();
    if e == "yesterday" { return Ok(today - Duration::days(1)); }
    if e == "tomorrow" { return Ok(today + Duration::days(1)); }
    if e == "today" { return Ok(today); }

    // N days/weeks/months ago
    let re_ago = regex_lite::Regex::new(r"^(\d+)\s+(days?|weeks?|months?)\s+ago$").unwrap();
    if let Some(caps) = re_ago.captures(&e) {
        let n: i64 = caps[1].parse().map_err(|_| "bad number")?;
        return match &caps[2] {
            s if s.starts_with("day") => Ok(today - Duration::days(n)),
            s if s.starts_with("week") => Ok(today - Duration::weeks(n)),
            s if s.starts_with("month") => Ok(shift_months(today, -(n as i32))),
            _ => Err(format!("unknown unit: {}", &caps[2])),
        };
    }

    // N days/weeks from now
    let re_ahead = regex_lite::Regex::new(r"^(\d+)\s+(days?|weeks?)\s+(?:from now|ahead|from today)$").unwrap();
    if let Some(caps) = re_ahead.captures(&e) {
        let n: i64 = caps[1].parse().map_err(|_| "bad number")?;
        return match &caps[2] {
            s if s.starts_with("day") => Ok(today + Duration::days(n)),
            s if s.starts_with("week") => Ok(today + Duration::weeks(n)),
            _ => Err(format!("unknown unit: {}", &caps[2])),
        };
    }

    // next/last {weekday}
    let re_day = regex_lite::Regex::new(r"^(next|last)\s+(monday|tuesday|wednesday|thursday|friday|saturday|sunday)$").unwrap();
    if let Some(caps) = re_day.captures(&e) {
        let target: u32 = match &caps[2] {
            "monday" => 0, "tuesday" => 1, "wednesday" => 2, "thursday" => 3,
            "friday" => 4, "saturday" => 5, "sunday" => 6,
            _ => return Err("bad weekday".into()),
        };
        let current = today.weekday().num_days_from_monday();
        if &caps[1] == "next" {
            let mut diff = target as i64 - current as i64;
            if diff <= 0 { diff += 7; }
            return Ok(today + Duration::days(diff));
        } else {
            let mut diff = current as i64 - target as i64;
            if diff <= 0 { diff += 7; }
            return Ok(today - Duration::days(diff));
        }
    }

    Err(format!("Cannot parse: '{expr}'. Supported: N days/weeks/months ago, N days/weeks from now, yesterday, tomorrow, next/last {{weekday}}."))
}

fn shift_months(d: NaiveDate, months: i32) -> NaiveDate {
    let total = d.year() * 12 + d.month() as i32 - 1 + months;
    let y = total.div_euclid(12);
    let m = (total.rem_euclid(12) + 1) as u32;
    NaiveDate::from_ymd_opt(y, m, d.day().min(days_in_month(y, m))).unwrap()
}

fn days_in_month(year: i32, month: u32) -> u32 {
    if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap().pred_opt().unwrap().day()
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap().pred_opt().unwrap().day()
    }
}

fn relative(expr: &str, today: NaiveDate) -> Result<String, String> {
    let resolved = resolve_relative(expr, today)?;
    Ok(format!(
        "RELATIVE_DATE:\n  EXPRESSION: {}\n  RESOLVED: {} ({})\n  TODAY: {} ({})",
        expr, ymd(resolved), dow(resolved), ymd(today), dow(today),
    ))
}

fn iso(today: NaiveDate) -> String {
    let iso_week = today.iso_week();
    let doy = today.ordinal();
    format!(
        "ISO_CONTEXT:\n  TODAY: {} ({})\n  ISO_WEEK: W{:02}\n  ISO_YEAR: {}\n  ISO_WEEKDATE: {}-W{:02}-{}\n  DAY_OF_YEAR: {:03}",
        ymd(today), dow(today), iso_week.week(), iso_week.year(),
        iso_week.year(), iso_week.week(), today.weekday().number_from_monday(), doy,
    )
}

fn epoch() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap();
    let secs = now.as_secs();
    let millis = now.as_millis();
    let today = Local::now().date_naive();
    format!(
        "EPOCH_CONTEXT:\n  TODAY: {} ({})\n  UNIX_SECONDS: {}\n  UNIX_MILLIS: {}",
        ymd(today), dow(today), secs, millis,
    )
}

fn tz(now: chrono::DateTime<Local>) -> String {
    let today = now.date_naive();
    let offset = now.offset();
    let tz_abbrev = now.format("%Z").to_string();
    let utc_offset = offset.to_string();
    format!(
        "TIMEZONE_CONTEXT:\n  TODAY: {} ({})\n  TIMEZONE: {}\n  UTC_OFFSET: {}",
        ymd(today), dow(today), tz_abbrev, utc_offset,
    )
}

fn range(from: &str, to: &str) -> Result<String, String> {
    let d1 = NaiveDate::parse_from_str(from, "%Y-%m-%d")
        .map_err(|_| format!("Invalid date: '{from}'. Use YYYY-MM-DD."))?;
    let d2 = NaiveDate::parse_from_str(to, "%Y-%m-%d")
        .map_err(|_| format!("Invalid date: '{to}'. Use YYYY-MM-DD."))?;
    let cal_days = (d2 - d1).num_days().unsigned_abs();
    let mut biz = 0u64;
    let step = if d2 >= d1 { 1 } else { -1 };
    let mut cursor = d1;
    for _ in 0..cal_days {
        if matches!(cursor.weekday(), Weekday::Mon | Weekday::Tue | Weekday::Wed | Weekday::Thu | Weekday::Fri) {
            biz += 1;
        }
        cursor += Duration::days(step);
    }
    Ok(format!(
        "RANGE_CONTEXT:\n  FROM: {}\n  TO: {}\n  CALENDAR_DAYS: {}\n  BUSINESS_DAYS: {}",
        from, to, cal_days, biz,
    ))
}

fn all(today: NaiveDate, now: chrono::DateTime<Local>) -> String {
    [week(today), String::new(), month(today), String::new(), quarter(today),
     String::new(), iso(today), String::new(), epoch(), String::new(), tz(now)]
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate { NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap() }

    #[test]
    fn week_output() {
        let out = week(d("2026-03-18")); // Wednesday
        assert!(out.contains("2026-03-18"));
        assert!(out.contains("Wednesday"));
        assert!(out.contains("2026-03-16")); // Monday
        assert!(out.contains("2026-03-20")); // Friday
    }

    #[test]
    fn month_boundaries() {
        let out = month(d("2026-01-15"));
        assert!(out.contains("2026-01-01"));
        assert!(out.contains("2026-01-31"));
        assert!(out.contains("2025-12-01"));
        assert!(out.contains("2025-12-31"));
    }

    #[test]
    fn quarter_q1() {
        let out = quarter(d("2026-02-15"));
        assert!(out.contains("Q1 2026"));
        assert!(out.contains("2026-01-01"));
        assert!(out.contains("2026-03-31"));
    }

    #[test]
    fn quarter_fiscal() {
        let out = quarter(d("2025-11-15"));
        assert!(out.contains("FY2026"));
        assert!(out.contains("FQ1"));
    }

    #[test]
    fn relative_days_ago() {
        let r = resolve_relative("3 days ago", d("2026-03-18")).unwrap();
        assert_eq!(r, d("2026-03-15"));
    }

    #[test]
    fn relative_weeks_ago() {
        let r = resolve_relative("2 weeks ago", d("2026-03-18")).unwrap();
        assert_eq!(r, d("2026-03-04"));
    }

    #[test]
    fn relative_next_monday() {
        let r = resolve_relative("next monday", d("2026-03-18")).unwrap(); // Wed
        assert_eq!(r, d("2026-03-23"));
    }

    #[test]
    fn relative_last_friday() {
        let r = resolve_relative("last friday", d("2026-03-18")).unwrap(); // Wed
        assert_eq!(r, d("2026-03-13"));
    }

    #[test]
    fn relative_yesterday() {
        let r = resolve_relative("yesterday", d("2026-03-18")).unwrap();
        assert_eq!(r, d("2026-03-17"));
    }

    #[test]
    fn relative_bad_expr() {
        assert!(resolve_relative("the day after never", d("2026-03-18")).is_err());
    }

    #[test]
    fn iso_week() {
        let out = iso(d("2026-03-18"));
        assert!(out.contains("W12")); // ISO week 12
    }

    #[test]
    fn range_basic() {
        let out = range("2026-03-16", "2026-03-20").unwrap();
        assert!(out.contains("CALENDAR_DAYS: 4"));
        assert!(out.contains("BUSINESS_DAYS: 4")); // Mon-Thu
    }

    #[test]
    fn range_bad_date() {
        assert!(range("not-a-date", "2026-03-20").is_err());
    }

    #[test]
    fn execute_dispatch() {
        assert!(execute("week", None, None, None).is_ok());
        assert!(execute("month", None, None, None).is_ok());
        assert!(execute("quarter", None, None, None).is_ok());
        assert!(execute("iso", None, None, None).is_ok());
        assert!(execute("epoch", None, None, None).is_ok());
        assert!(execute("tz", None, None, None).is_ok());
        assert!(execute("all", None, None, None).is_ok());
        assert!(execute("relative", Some("yesterday"), None, None).is_ok());
        assert!(execute("range", None, Some("2026-01-01"), Some("2026-03-01")).is_ok());
        assert!(execute("bogus", None, None, None).is_err());
    }
}
