//! 日期：今天、`D` 输入的截止日期。

use clock_contract::Moment;
use jiff::civil::Date;
use jiff::tz::{Offset, TimeZone};
use jiff::{Span, Timestamp, Zoned};

/// 这一刻的本地时间。
pub fn local_date(at: Moment) -> Option<Zoned> {
    let ts = Timestamp::from_millisecond(at.utc_ms).ok()?;
    let offset = Offset::from_seconds(at.offset_min * 60).ok()?;
    Some(ts.to_zoned(TimeZone::fixed(offset)))
}

/// 本地的今天。
pub fn today(at: Moment) -> Option<Date> {
    local_date(at).map(|z| z.date())
}

/// `D` 里输入的截止日期。外层 `Err` 是给用户看的原因；`Ok(None)` 表示清掉。
///
/// 认：空的 / `none` / `-`（清掉）、`2026-11-20`、`11-20`（今年的，过了就是明年的）、
/// `今天` / `明天` / `后天`、`+3`（3 天后）。完整的写法（`@周五` 之类）在记录时用。
pub fn due_input(input: &str, today: Date) -> Result<Option<String>, String> {
    let s = input.trim();
    let bad = || format!("看不懂「{s}」：写 2026-11-20、11-20、明天、+3，或者留空清掉");
    let plus = |days: i64| -> Result<Option<String>, String> {
        let span = Span::new().try_days(days).map_err(|_| bad())?;
        today
            .checked_add(span)
            .map(|d| Some(d.to_string()))
            .map_err(|_| bad())
    };
    match s {
        "" | "none" | "-" => return Ok(None),
        "今天" | "today" => return plus(0),
        "明天" | "tomorrow" => return plus(1),
        "后天" => return plus(2),
        _ => {}
    }
    if let Some(n) = s.strip_prefix('+') {
        let n: i64 = n.parse().map_err(|_| bad())?;
        return if (1..=3650).contains(&n) {
            plus(n)
        } else {
            Err(bad())
        };
    }
    let parts: Vec<&str> = s.split('-').collect();
    let date = match parts.as_slice() {
        [y, m, d] if y.len() == 4 => Date::new(
            y.parse().map_err(|_| bad())?,
            m.parse().map_err(|_| bad())?,
            d.parse().map_err(|_| bad())?,
        )
        .map_err(|_| bad())?,
        [m, d] => {
            let (m, d) = (m.parse().map_err(|_| bad())?, d.parse().map_err(|_| bad())?);
            let this = Date::new(today.year(), m, d).map_err(|_| bad())?;
            if this >= today {
                this
            } else {
                Date::new(today.year() + 1, m, d).map_err(|_| bad())?
            }
        }
        _ => return Err(bad()),
    };
    Ok(Some(date.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs() {
        let today: Date = "2026-11-18".parse().unwrap();
        for (input, want) in [
            ("", None),
            (" none ", None),
            ("-", None),
            ("2026-12-01", Some("2026-12-01")),
            ("11-20", Some("2026-11-20")),
            ("1-5", Some("2027-01-05")),
            ("今天", Some("2026-11-18")),
            ("明天", Some("2026-11-19")),
            ("+14", Some("2026-12-02")),
        ] {
            assert_eq!(due_input(input, today).unwrap().as_deref(), want, "{input}");
        }
        for bad in ["周五", "2026-02-30", "+0", "+x", "13-01", "1-2-3"] {
            let err = due_input(bad, today).unwrap_err();
            assert!(err.contains("看不懂"), "{bad}: {err}");
        }
    }

    #[test]
    fn today_uses_the_offset() {
        let at = Moment {
            utc_ms: 1_794_933_000_000, // 2026-11-17T16:30Z
            offset_min: 480,
        };
        assert_eq!(today(at).unwrap().to_string(), "2026-11-18");
    }
}
