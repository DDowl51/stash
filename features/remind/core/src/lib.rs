//! `remind` 的纯逻辑：根据收件箱的积压和到期的条目，算出今天还要发哪些通知。
//!
//! 「一天一次」靠提醒的 key：`inbox`、`due:<条目 id>`。今天发过的 key 不再发；
//! 到期的条目合成一条通知，里面每个条目各占一个 key，所以当天新到期的还会再提醒一次。

use std::collections::HashSet;

use clock_contract::Moment;
use jiff::civil::Date;
use jiff::tz::{Offset, TimeZone};
use jiff::{Span, Timestamp};
use notify_contract::{Notification, Urgency};

/// 提醒规则（来自配置）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rules {
    /// 收件箱超过几条就提醒。
    pub inbox_threshold: u32,
    /// 提前几天提醒到期；0 = 当天。
    pub due_days_ahead: u32,
    /// 一条通知里最多列几个条目。
    pub max_listed: usize,
}

/// 一个有截止日期、还没完成的条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    pub id: String,
    pub text: String,
    /// `YYYY-MM-DD`。
    pub due_on: String,
}

/// 一条要发的通知，和它代表的提醒 key。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub keys: Vec<String>,
    pub notification: Notification,
}

/// 本地的今天。
pub fn today(at: Moment) -> Option<Date> {
    let ts = Timestamp::from_millisecond(at.utc_ms).ok()?;
    let offset = Offset::from_seconds(at.offset_min * 60).ok()?;
    Some(ts.to_zoned(TimeZone::fixed(offset)).date())
}

/// 往前多少天的日期（清理旧记录用）。
pub fn days_before(today: Date, days: u32) -> Option<Date> {
    today
        .checked_sub(Span::new().try_days(i64::from(days)).ok()?)
        .ok()
}

fn when(due: Date, today: Date) -> String {
    let days = (due - today).get_days();
    match days {
        d if d < 0 => format!("过期 {} 天", -d),
        0 => "今天".to_owned(),
        1 => "明天".to_owned(),
        d => format!("还有 {d} 天"),
    }
}

/// 今天还要发的通知。`sent` 是今天已经发过的 key。
pub fn plan(
    rules: Rules,
    inbox: u32,
    due: &[Due],
    today: Date,
    sent: &HashSet<String>,
) -> Vec<Reminder> {
    let mut out = Vec::new();
    if inbox > rules.inbox_threshold && !sent.contains("inbox") {
        out.push(Reminder {
            keys: vec!["inbox".to_owned()],
            notification: Notification {
                summary: format!("收件箱有 {inbox} 条没整理"),
                body: "打开 stash-tui 整理一下".to_owned(),
                urgency: Urgency::Normal,
            },
        });
    }

    let Some(horizon) = today
        .checked_add(Span::new().days(i64::from(rules.due_days_ahead)))
        .ok()
    else {
        return out;
    };
    let mut pending: Vec<(Date, &Due)> = due
        .iter()
        .filter_map(|d| Some((d.due_on.parse::<Date>().ok()?, d)))
        .filter(|(date, d)| *date <= horizon && !sent.contains(&format!("due:{}", d.id)))
        .collect();
    if pending.is_empty() {
        return out;
    }
    pending.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
    let overdue = pending.iter().any(|(date, _)| *date < today);
    let summary = match pending.as_slice() {
        [(date, one)] => format!("{}：{}", when(*date, today), one.text),
        many => format!("{} 条到期", many.len()),
    };
    let mut lines: Vec<String> = if pending.len() == 1 {
        vec![format!("截止 {}", pending[0].1.due_on)]
    } else {
        pending
            .iter()
            .take(rules.max_listed)
            .map(|(date, d)| format!("· {}（{}）", d.text, when(*date, today)))
            .collect()
    };
    if pending.len() > rules.max_listed {
        lines.push(format!("……还有 {} 条", pending.len() - rules.max_listed));
    }
    out.push(Reminder {
        keys: pending
            .iter()
            .map(|(_, d)| format!("due:{}", d.id))
            .collect(),
        notification: Notification {
            summary,
            body: lines.join("\n"),
            urgency: if overdue {
                Urgency::Critical
            } else {
                Urgency::Normal
            },
        },
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: Rules = Rules {
        inbox_threshold: 5,
        due_days_ahead: 0,
        max_listed: 3,
    };

    fn today_() -> Date {
        "2026-11-18".parse().unwrap()
    }

    fn due(id: &str, text: &str, on: &str) -> Due {
        Due {
            id: id.to_owned(),
            text: text.to_owned(),
            due_on: on.to_owned(),
        }
    }

    fn none() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn inbox_over_the_threshold() {
        assert!(
            plan(RULES, 5, &[], today_(), &none()).is_empty(),
            "等于不提醒"
        );
        let got = plan(RULES, 6, &[], today_(), &none());
        assert_eq!(got[0].keys, ["inbox"]);
        assert_eq!(got[0].notification.summary, "收件箱有 6 条没整理");
        let sent = HashSet::from(["inbox".to_owned()]);
        assert!(plan(RULES, 9, &[], today_(), &sent).is_empty(), "一天一次");
    }

    #[test]
    fn one_due_item() {
        let got = plan(
            RULES,
            0,
            &[
                due("A", "交周报", "2026-11-18"),
                due("B", "以后", "2026-11-19"),
            ],
            today_(),
            &none(),
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].keys, ["due:A"]);
        assert_eq!(got[0].notification.summary, "今天：交周报");
        assert_eq!(got[0].notification.body, "截止 2026-11-18");
        assert_eq!(got[0].notification.urgency, Urgency::Normal);
    }

    #[test]
    fn many_due_items_in_one_notification() {
        let items = [
            due("C", "丙", "2026-11-18"),
            due("A", "甲", "2026-11-15"),
            due("B", "乙", "2026-11-17"),
            due("D", "丁", "2026-11-10"),
            due("E", "戊", "2026-11-18"),
        ];
        let got = plan(RULES, 0, &items, today_(), &none());
        let r = &got[0];
        assert_eq!(r.notification.summary, "5 条到期");
        assert_eq!(
            r.notification.body,
            "· 丁（过期 8 天）\n· 甲（过期 3 天）\n· 乙（过期 1 天）\n……还有 2 条"
        );
        assert_eq!(r.notification.urgency, Urgency::Critical, "有过期的");
        assert_eq!(r.keys, ["due:D", "due:A", "due:B", "due:C", "due:E"]);
    }

    #[test]
    fn already_reminded_items_are_left_out() {
        let items = [due("A", "甲", "2026-11-18"), due("B", "乙", "2026-11-18")];
        let sent = HashSet::from(["due:A".to_owned()]);
        let got = plan(RULES, 0, &items, today_(), &sent);
        assert_eq!(got[0].keys, ["due:B"]);
        let sent = HashSet::from(["due:A".to_owned(), "due:B".to_owned()]);
        assert!(plan(RULES, 0, &items, today_(), &sent).is_empty());
    }

    #[test]
    fn days_ahead() {
        let rules = Rules {
            due_days_ahead: 2,
            ..RULES
        };
        let items = [due("A", "甲", "2026-11-20"), due("B", "乙", "2026-11-21")];
        let got = plan(rules, 0, &items, today_(), &none());
        assert_eq!(got[0].keys, ["due:A"]);
        assert_eq!(got[0].notification.summary, "还有 2 天：甲");
        let bad = [due("X", "坏日期", "明天")];
        assert!(plan(RULES, 0, &bad, today_(), &none()).is_empty());
    }

    #[test]
    fn dates() {
        let at = Moment {
            utc_ms: 1_794_933_000_000,
            offset_min: 480,
        };
        assert_eq!(today(at).unwrap(), today_());
        assert_eq!(days_before(today_(), 30).unwrap().to_string(), "2026-10-19");
    }
}
