//! `dida-push` 的纯逻辑：一条 stash 条目怎么变成一个滴答任务。

use std::collections::BTreeMap;

use dida_tasks_contracts::Due;
use items_contracts::{ContextEntry, ContextKind, Item};

/// 推过去之后追加到条目上的链接。
pub const LINK_KEY: &str = "dida.task";
/// 推送失败时追加到条目上的说明。
pub const FAILED_KEY: &str = "dida.failed";

/// 任务的标题：条目文字。
pub fn title(item: &Item) -> String {
    item.text.clone()
}

/// 截止：条目的截止日期，全天。
pub fn due(item: &Item) -> Option<Due> {
    item.due_on.as_deref().map(Due::date)
}

/// 任务的描述：来源一行，上下文逐行「标签：值」，最后一行是条目 id。
/// 截图之类本地文件只写路径；之前推送的链接、失败说明不带过去。
pub fn description(item: &Item) -> String {
    let mut lines = Vec::new();
    if let Some(source) = &item.source {
        lines.push(format!("来源：{source}"));
    }
    if let Some(url) = &item.url
        && !item.text.contains(url.as_str())
    {
        lines.push(format!("链接：{url}"));
    }
    for c in item
        .context
        .iter()
        .filter(|c| c.key != LINK_KEY && c.key != FAILED_KEY)
    {
        let value = c.value.lines().next().unwrap_or_default();
        if !value.trim().is_empty() {
            lines.push(format!("{}：{value}", c.label));
        }
    }
    lines.push(format!("stash 条目 {}", item.id));
    lines.join("\n")
}

/// 推到哪个清单（名字）：分类有单独配置的用它，否则用默认的；都没有是收集箱（`None`）。
pub fn project_for(
    item: &Item,
    kind_projects: &BTreeMap<String, String>,
    default: &str,
) -> Option<String> {
    kind_projects
        .get(item.kind.as_str())
        .map(String::as_str)
        .or(Some(default))
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
}

pub fn link_entry(url: &str) -> ContextEntry {
    ContextEntry::new(LINK_KEY, "滴答任务", ContextKind::Url, url)
}

pub fn failed_entry(why: &str) -> ContextEntry {
    ContextEntry::new(FAILED_KEY, "推送滴答失败", ContextKind::Text, why)
}

#[cfg(test)]
mod tests {
    use items_contracts::{Kind, Status};

    use super::*;

    fn item() -> Item {
        Item {
            id: "01KAAAAAAAAAAAAAAAAAAAAAAA".into(),
            text: "看 PR https://github.com/a/b/pull/1".into(),
            url: Some("https://github.com/a/b/pull/1".into()),
            kind: Kind::Todo,
            status: Status::Open,
            rank: 0,
            due_on: Some("2026-11-27".into()),
            source: Some("ghostty · ~/code/stash (main)".into()),
            context: vec![
                ContextEntry::new(
                    "terminal.cwd",
                    "目录",
                    ContextKind::Path,
                    "/home/me/code/stash",
                ),
                ContextEntry::new(
                    "screenshot",
                    "截图",
                    ContextKind::Image,
                    "/home/me/.local/share/stash/shots/x.png",
                ),
                ContextEntry::new("script.log", "日志", ContextKind::Text, "第一行\n第二行"),
                ContextEntry::new("empty", "空的", ContextKind::Text, " "),
                failed_entry("断网"),
            ],
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn the_task_looks_like_the_item() {
        let it = item();
        assert_eq!(title(&it), it.text);
        assert_eq!(due(&it), Some(Due::date("2026-11-27")));
        assert_eq!(
            description(&it),
            "来源：ghostty · ~/code/stash (main)\n\
             目录：/home/me/code/stash\n\
             截图：/home/me/.local/share/stash/shots/x.png\n\
             日志：第一行\n\
             stash 条目 01KAAAAAAAAAAAAAAAAAAAAAAA"
        );
    }

    #[test]
    fn a_url_not_in_the_text_is_listed() {
        let mut it = item();
        it.text = "看这个".into();
        assert!(description(&it).contains("链接：https://github.com/a/b/pull/1"));
    }

    #[test]
    fn which_project() {
        let it = item();
        let map = BTreeMap::from([("todo".to_owned(), "工作".to_owned())]);
        assert_eq!(project_for(&it, &map, "生活").as_deref(), Some("工作"));
        assert_eq!(
            project_for(&it, &BTreeMap::new(), "生活").as_deref(),
            Some("生活")
        );
        assert_eq!(project_for(&it, &BTreeMap::new(), " "), None, "空是收集箱");
    }
}
