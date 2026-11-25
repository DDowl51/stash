//! `items` 的纯逻辑：校验、排序值、到期区间、id 写法。不碰存储和时钟。

use items_contracts::DueFilter;
use jiff::civil::Date;
use kernel_api::AppError;

/// 新条目之间排序值的间隔。挪位置时取两边的中点，间隔用完再整体重排。
pub const RANK_GAP: i64 = 1024;

/// id 后缀至少几个字符。短了容易对上好几个。
pub const MIN_ID_SUFFIX: usize = 4;

/// 去掉首尾空白；空的不行。
pub fn clean_text(text: &str) -> Result<String, AppError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(AppError::validation("条目的文字不能是空的"));
    }
    Ok(text.to_owned())
}

/// 截止日期必须是存在的 `YYYY-MM-DD`。
pub fn check_date(date: &str) -> Result<String, AppError> {
    let parsed: Date = date
        .parse()
        .map_err(|_| AppError::validation(format!("日期 `{date}` 不对，要写成 YYYY-MM-DD")))?;
    // jiff 也接受别的写法（比如带时间），这里只要规范的形式，存储里才能按字符串比较
    let canonical = parsed.to_string();
    if canonical != date {
        return Err(AppError::validation(format!(
            "日期 `{date}` 不对，要写成 YYYY-MM-DD"
        )));
    }
    Ok(canonical)
}

/// 用户给的 id：完整的 ULID 或者一段后缀。统一成大写（ULID 不区分大小写）。
pub fn id_query(id: &str) -> Result<String, AppError> {
    let id = id.trim().to_ascii_uppercase();
    if id.len() < MIN_ID_SUFFIX {
        return Err(AppError::validation(format!(
            "id `{id}` 太短，至少写最后 {MIN_ID_SUFFIX} 个字符"
        )));
    }
    if !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(AppError::validation(format!("id `{id}` 里有不该有的字符")));
    }
    Ok(id)
}

/// 调用方给的完整 id：必须是 ULID（26 个 Crockford base32 字符）。统一成大写。
pub fn check_id(id: &str) -> Result<String, AppError> {
    let upper = id.trim().to_ascii_uppercase();
    let crockford = |c: char| c.is_ascii_digit() || (c.is_ascii_uppercase() && !"ILOU".contains(c));
    // 第一个字符最大是 7，否则超出 128 位
    if upper.len() != 26 || !upper.chars().all(crockford) || upper.as_bytes()[0] > b'7' {
        return Err(AppError::validation(format!("id `{id}` 不是 ULID")));
    }
    Ok(upper)
}

/// 本地的今天。
pub fn local_date(utc_ms: i64, offset_min: i32) -> Result<Date, AppError> {
    zh_dates::local_date(utc_ms, offset_min)
}

/// 到期筛选对应的截止日期区间 `[from, to]`（含两端，`None` 表示不限）。日期按字符串比较。
pub fn due_range(filter: DueFilter, today: Date) -> Result<(Option<String>, String), AppError> {
    let yesterday = today
        .yesterday()
        .map_err(|e| AppError::validation(e.to_string()))?;
    Ok(match filter {
        DueFilter::Overdue => (None, yesterday.to_string()),
        DueFilter::Today => (None, today.to_string()),
        DueFilter::Week => {
            let end = today
                .checked_add(jiff::Span::new().days(6))
                .map_err(|e| AppError::validation(e.to_string()))?;
            (None, end.to_string())
        }
    })
}

/// 放在 `prev` 和 `next` 之间的排序值。两边都没有时从一个间隔开始。
/// 两边已经挨着、放不下时返回 `None`，调用方应该先重排这一分类再放。
pub fn rank_between(prev: Option<i64>, next: Option<i64>) -> Option<i64> {
    match (prev, next) {
        (None, None) => Some(RANK_GAP),
        (Some(p), None) => p.checked_add(RANK_GAP),
        (None, Some(n)) => n.checked_sub(RANK_GAP),
        (Some(p), Some(n)) => {
            let gap = n.checked_sub(p)?;
            (gap >= 2).then(|| p + gap / 2)
        }
    }
}

/// 重排 `count` 个条目时各自的新排序值，间隔重新拉开。
pub fn spaced_ranks(count: usize) -> Vec<i64> {
    (1..=count)
        .map(|i| i64::try_from(i).unwrap_or(i64::MAX / RANK_GAP) * RANK_GAP)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel_api::ErrorKind;

    #[test]
    fn text_is_trimmed_and_must_not_be_empty() {
        assert_eq!(clean_text("  回复老王 \n").unwrap(), "回复老王");
        assert_eq!(clean_text(" \t ").unwrap_err().kind, ErrorKind::Validation);
    }

    #[test]
    fn dates_must_exist_and_be_canonical() {
        assert_eq!(check_date("2026-11-20").unwrap(), "2026-11-20");
        for bad in ["2026-02-30", "2026-1-2", "明天", "2026-11-20T10:00", ""] {
            assert_eq!(
                check_date(bad).unwrap_err().kind,
                ErrorKind::Validation,
                "{bad}"
            );
        }
    }

    #[test]
    fn id_queries_are_uppercased_and_not_too_short() {
        assert_eq!(id_query(" 7k3q ").unwrap(), "7K3Q");
        assert_eq!(id_query("abc").unwrap_err().kind, ErrorKind::Validation);
        assert_eq!(id_query("ab%d").unwrap_err().kind, ErrorKind::Validation);
    }

    #[test]
    fn given_ids_must_be_ulids() {
        assert_eq!(
            check_id("01jd0000000000000000000000").unwrap(),
            "01JD0000000000000000000000"
        );
        for bad in [
            "",
            "01JD",
            "01JD000000000000000000000U",
            "81JD0000000000000000000000",
        ] {
            assert_eq!(
                check_id(bad).unwrap_err().kind,
                ErrorKind::Validation,
                "{bad}"
            );
        }
    }

    #[test]
    fn local_date_uses_the_offset() {
        // 2026-11-17T16:30Z：东九区已经是 18 号，UTC 还是 17 号
        let ms = 1_794_933_000_000;
        assert_eq!(local_date(ms, 540).unwrap().to_string(), "2026-11-18");
        assert_eq!(local_date(ms, 0).unwrap().to_string(), "2026-11-17");
    }

    #[test]
    fn due_ranges() {
        let today: Date = "2026-03-01".parse().unwrap();
        assert_eq!(
            due_range(DueFilter::Overdue, today).unwrap(),
            (None, "2026-02-28".to_owned())
        );
        assert_eq!(
            due_range(DueFilter::Today, today).unwrap(),
            (None, "2026-03-01".to_owned())
        );
        assert_eq!(
            due_range(DueFilter::Week, today).unwrap(),
            (None, "2026-03-07".to_owned())
        );
    }

    #[test]
    fn ranks_go_between_neighbours_until_they_touch() {
        assert_eq!(rank_between(None, None), Some(RANK_GAP));
        assert_eq!(rank_between(Some(2048), None), Some(3072));
        assert_eq!(rank_between(None, Some(1024)), Some(0));
        assert_eq!(rank_between(Some(1024), Some(2048)), Some(1536));
        assert_eq!(rank_between(Some(5), Some(7)), Some(6));
        assert_eq!(rank_between(Some(5), Some(6)), None);
        assert_eq!(rank_between(Some(i64::MAX), None), None);
    }

    #[test]
    fn respacing() {
        assert_eq!(spaced_ranks(3), [1024, 2048, 3072]);
        assert!(spaced_ranks(0).is_empty());
    }
}
