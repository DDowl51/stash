//! 日期：今天、`D` 输入的截止日期。

use clock_contract::Moment;
use jiff::civil::Date;
use jiff::tz::{Offset, TimeZone};
use jiff::{Timestamp, Zoned};

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
/// 空的 / `none` / `-` 是清掉；其余的写法和记录时的 `@` 一样（mk-kit 的 `zh-dates`）：
/// `2026-11-20`、`11-20`、`明天`、`周五`、`下周一`、`+3`……
pub fn due_input(input: &str, today: Date) -> Result<Option<String>, String> {
    let s = input.trim();
    if matches!(s, "" | "none" | "-") {
        return Ok(None);
    }
    zh_dates::parse(s, today)
        .map(|d| Some(d.to_string()))
        .ok_or_else(|| format!("看不懂「{s}」：写 2026-11-20、11-20、明天、周五、+3，或者留空清掉"))
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
            ("周五", Some("2026-11-20")),
            ("下周一", Some("2026-11-23")),
        ] {
            assert_eq!(due_input(input, today).unwrap().as_deref(), want, "{input}");
        }
        for bad in ["周八", "2026-02-30", "+0", "+x", "13-01", "1-2-3"] {
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
