//! 一个条目在列表里的一行，和选中时的详情。

use clock_contract::Moment;
use items_contracts::{ContextKind, Item, Kind, Status};
use jiff::civil::Date;
use tui_contract::{Line, ListRow, Span, Style};

use crate::due::local_date;
use crate::page::Page;

/// 列表里显示的短 id：和命令行一样是最后 6 位。
pub fn short_id(id: &str) -> &str {
    &id[id.len().saturating_sub(6)..]
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Inbox => "收件箱",
        Kind::Todo => "待办",
        Kind::Idea => "想法",
        Kind::ReadLater => "稍后读",
    }
}

fn status_label(status: Status) -> &'static str {
    match status {
        Status::Open => "未完成",
        Status::Done => "已完成",
        Status::Archived => "已归档",
    }
}

/// `11-18 09:30`；年份不同时带上年份。
fn when(ms: i64, offset_min: i32, today: Date) -> String {
    match local_date(Moment {
        utc_ms: ms,
        offset_min,
    }) {
        Some(z) if z.year() == today.year() => z.strftime("%m-%d %H:%M").to_string(),
        Some(z) => z.strftime("%Y-%m-%d %H:%M").to_string(),
        None => String::new(),
    }
}

/// 截止日期的样式：过期加粗、今天突出、以后的弱一点。
fn due_span(due: &str, today: &str) -> Span {
    let style = match due.cmp(today) {
        std::cmp::Ordering::Less => Style::Strong,
        std::cmp::Ordering::Equal => Style::Accent,
        std::cmp::Ordering::Greater => Style::Dim,
    };
    let text = match due.cmp(today) {
        std::cmp::Ordering::Less => format!("@{due} 过期  "),
        std::cmp::Ordering::Equal => "@今天  ".to_owned(),
        std::cmp::Ordering::Greater => format!("@{due}  "),
    };
    Span::new(text, style)
}

fn text_line(spans: Vec<Span>) -> Line {
    Line { spans }
}

/// 一个条目的行和详情。`offset_min` 是本地时区，用来显示时间。
pub fn row(item: &Item, page: Page, today: Date, offset_min: i32) -> ListRow {
    let today_s = today.to_string();
    let mut spans = vec![Span::new(format!("{}  ", short_id(&item.id)), Style::Dim)];
    if page == Page::Inbox {
        spans.push(Span::new(
            format!("{}  ", when(item.created_at, offset_min, today)),
            Style::Dim,
        ));
    }
    if page == Page::Done {
        spans.push(Span::new(
            format!("[{}]  ", kind_label(item.kind)),
            Style::Dim,
        ));
    }
    if let Some(due) = &item.due_on
        && page != Page::Done
    {
        spans.push(due_span(due, &today_s));
    }
    spans.push(Span::new(item.text.clone(), Style::Normal));
    if let Some(source) = &item.source {
        spans.push(Span::new(format!("  · {source}"), Style::Dim));
    }

    let field = |label: &str, value: String| {
        text_line(vec![
            Span::new(format!("  {label}  "), Style::Dim),
            Span::new(value, Style::Normal),
        ])
    };
    let mut detail = vec![
        text_line(vec![Span::new(format!("  {}", item.text), Style::Strong)]),
        text_line(vec![Span::new(
            format!(
                "  {} · {} · 记于 {} · {}",
                kind_label(item.kind),
                status_label(item.status),
                when(item.created_at, offset_min, today),
                item.id
            ),
            Style::Dim,
        )]),
    ];
    if let Some(due) = &item.due_on {
        detail.push(field("截止", due.clone()));
    }
    if let Some(url) = &item.url {
        detail.push(field("链接", url.clone()));
    }
    if let Some(source) = &item.source {
        detail.push(field("来源", source.clone()));
    }
    for c in &item.context {
        detail.push(field(&c.label, c.value.clone()));
    }
    ListRow {
        id: item.id.clone(),
        line: text_line(spans),
        detail,
    }
}

/// `o` / `O` 打开什么：链接（条目的 url，没有就是上下文里第一个网址）或截图。
pub fn context_to_open(item: &Item, image: bool) -> Option<String> {
    if image {
        return item
            .context
            .iter()
            .find(|c| c.kind == ContextKind::Image)
            .map(|c| c.value.clone());
    }
    item.url.clone().or_else(|| {
        item.context
            .iter()
            .find(|c| c.kind == ContextKind::Url)
            .map(|c| c.value.clone())
    })
}

#[cfg(test)]
mod tests {
    use items_contracts::ContextEntry;

    use super::*;

    // 2026-11-18 01:30 +08:00
    const CREATED: i64 = 1_794_936_600_000;

    fn item() -> Item {
        Item {
            id: "01JD8X0000000000000000ABCD".to_owned(),
            text: "看下 PR".to_owned(),
            url: None,
            kind: Kind::Todo,
            status: Status::Open,
            rank: 1024,
            due_on: Some("2026-11-17".to_owned()),
            source: Some("zen · PR #12".to_owned()),
            context: vec![
                ContextEntry::new(
                    "browser.url",
                    "网址",
                    ContextKind::Url,
                    "https://github.com/x/12",
                ),
                ContextEntry::new("screenshot.window", "截图", ContextKind::Image, "/s/1.png"),
            ],
            created_at: CREATED,
            updated_at: CREATED,
        }
    }

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.text.as_str()).collect()
    }

    fn today() -> Date {
        "2026-11-18".parse().unwrap()
    }

    #[test]
    fn rows_per_page() {
        let r = row(&item(), Page::Todo, today(), 480);
        assert_eq!(r.id, item().id);
        assert_eq!(
            text(&r.line),
            "00ABCD  @2026-11-17 过期  看下 PR  · zen · PR #12"
        );
        assert_eq!(r.line.spans[1].style, Style::Strong);

        let mut it = item();
        it.kind = Kind::Inbox;
        it.due_on = Some("2026-11-18".to_owned());
        let r = row(&it, Page::Inbox, today(), 480);
        assert_eq!(
            text(&r.line),
            "00ABCD  11-18 01:30  @今天  看下 PR  · zen · PR #12"
        );

        let mut it = item();
        it.status = Status::Done;
        assert_eq!(
            text(&row(&it, Page::Done, today(), 480).line),
            "00ABCD  [待办]  看下 PR  · zen · PR #12",
            "已完成页不显示截止日期，显示分类"
        );
    }

    #[test]
    fn detail_has_everything() {
        let lines: Vec<String> = row(&item(), Page::Todo, today(), 480)
            .detail
            .iter()
            .map(text)
            .collect();
        assert_eq!(lines[0], "  看下 PR");
        assert!(
            lines[1].contains("待办 · 未完成 · 记于 11-18 01:30"),
            "{lines:?}"
        );
        assert!(lines.contains(&"  截止  2026-11-17".to_owned()));
        assert!(lines.contains(&"  来源  zen · PR #12".to_owned()));
        assert!(lines.contains(&"  网址  https://github.com/x/12".to_owned()));
        assert!(lines.contains(&"  截图  /s/1.png".to_owned()));
    }

    #[test]
    fn what_to_open() {
        assert_eq!(
            context_to_open(&item(), false).as_deref(),
            Some("https://github.com/x/12")
        );
        let mut it = item();
        it.url = Some("https://own".to_owned());
        assert_eq!(
            context_to_open(&it, false).as_deref(),
            Some("https://own"),
            "条目自己的优先"
        );
        assert_eq!(context_to_open(&it, true).as_deref(), Some("/s/1.png"));
        it.context.clear();
        assert_eq!(context_to_open(&it, true), None);
    }
}
