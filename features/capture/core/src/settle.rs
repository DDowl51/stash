//! 提供者和展开器的结果怎么落到条目上。

use capture_contracts::{ContextEntry, ContextKind, SOURCE_KEY, WindowInfo};

/// 一个提供者这次的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Collected(Vec<ContextEntry>),
    Failed(String),
    /// 到时间还没给结果，丢掉了。
    TimedOut,
}

/// 结果 → 条目上的上下文。失败和超时不丢：记成一条文字，以后看得出「当时这项没采到」。
pub fn settle(id: &str, outcome: Outcome) -> Vec<ContextEntry> {
    let note = |value: String| {
        vec![ContextEntry::new(
            format!("capture.{id}"),
            format!("{id}（没采到）"),
            ContextKind::Text,
            value,
        )]
    };
    match outcome {
        Outcome::Collected(entries) => entries,
        Outcome::Failed(why) => note(format!("出错：{why}")),
        Outcome::TimedOut => note("超时，没等到".to_owned()),
    }
}

/// 展开器出错：按展开前的文字继续，记一条。
pub fn expander_failed(id: &str, why: &str) -> ContextEntry {
    ContextEntry::new(
        format!("capture.expand.{id}"),
        format!("展开 {id}（失败，按原文保存）"),
        ContextKind::Text,
        why,
    )
}

/// 按配置的顺序排：配置里列了的按列的顺序在前，没列的按 id 排在后面。
pub fn ordered<T>(mut items: Vec<(String, T)>, preferred: &[String]) -> Vec<(String, T)> {
    items.sort_by(|(a, _), (b, _)| {
        let rank = |id: &String| preferred.iter().position(|p| p == id).unwrap_or(usize::MAX);
        rank(a).cmp(&rank(b)).then_with(|| a.cmp(b))
    });
    items
}

/// 把 `key = source` 的条目拿出来（多条时以最后一条为准），其余原样。
pub fn split_source(entries: Vec<ContextEntry>) -> (Option<String>, Vec<ContextEntry>) {
    let (sources, rest): (Vec<_>, Vec<_>) = entries.into_iter().partition(|e| e.key == SOURCE_KEY);
    (sources.into_iter().last().map(|e| e.value), rest)
}

/// 标题太长时截断到多少个字符。
const TITLE_MAX: usize = 60;

/// `com.mitchellh.ghostty` → `ghostty`；`google-chrome` 不变。
fn app_name(app_id: &str) -> &str {
    if app_id.matches('.').count() >= 2 {
        app_id.rsplit('.').next().unwrap_or(app_id)
    } else {
        app_id
    }
}

/// 一行来源摘要。提供者给了 `source` 就用它，否则是「应用 · 标题」。
pub fn source_line(given: Option<String>, window: Option<&WindowInfo>) -> Option<String> {
    if let Some(given) = given.filter(|s| !s.trim().is_empty()) {
        return Some(given.trim().to_owned());
    }
    let window = window?;
    let app = window
        .app_id
        .as_deref()
        .map(app_name)
        .filter(|a| !a.is_empty());
    let title = window
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| {
            if t.chars().count() > TITLE_MAX {
                format!("{}…", t.chars().take(TITLE_MAX).collect::<String>())
            } else {
                t.to_owned()
            }
        });
    match (app, title) {
        (Some(app), Some(title)) => Some(format!("{app} · {title}")),
        (Some(app), None) => Some(app.to_owned()),
        (None, Some(title)) => Some(title),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, value: &str) -> ContextEntry {
        ContextEntry::new(key, key, ContextKind::Text, value)
    }

    fn window(app: Option<&str>, title: Option<&str>) -> WindowInfo {
        WindowInfo {
            id: 1,
            app_id: app.map(str::to_owned),
            title: title.map(str::to_owned),
            workspace: None,
            pid: None,
        }
    }

    #[test]
    fn failures_and_timeouts_are_kept_as_notes() {
        let got = settle("browser", Outcome::Failed("读不到会话文件".into()));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].key, "capture.browser");
        assert!(got[0].label.contains("browser") && got[0].value.contains("读不到会话文件"));
        let got = settle("terminal", Outcome::TimedOut);
        assert_eq!(got[0].key, "capture.terminal");
        assert!(got[0].value.contains("超时"));
        let ok = vec![entry("window.app", "zen")];
        assert_eq!(settle("window", Outcome::Collected(ok.clone())), ok);
        assert!(
            expander_failed("calc", "除以 0")
                .label
                .contains("按原文保存")
        );
    }

    #[test]
    fn configured_order_then_by_id() {
        let items = ["script", "browser", "window", "zeta", "alpha"]
            .map(|id| (id.to_owned(), ()))
            .to_vec();
        let got: Vec<String> = ordered(items, &["window".into(), "script".into()])
            .into_iter()
            .map(|(id, ())| id)
            .collect();
        assert_eq!(got, ["window", "script", "alpha", "browser", "zeta"]);
    }

    #[test]
    fn source_entries_are_split_out() {
        let (source, rest) = split_source(vec![
            entry("source", "一"),
            entry("window.app", "zen"),
            entry("source", "二"),
        ]);
        assert_eq!(source.as_deref(), Some("二"));
        assert_eq!(rest, [entry("window.app", "zen")]);
    }

    #[test]
    fn source_lines() {
        let w = window(Some("com.mitchellh.ghostty"), Some("  nvim  "));
        assert_eq!(
            source_line(None, Some(&w)).as_deref(),
            Some("ghostty · nvim")
        );
        assert_eq!(
            source_line(Some("ghostty · ~/code (main)".into()), Some(&w)).as_deref(),
            Some("ghostty · ~/code (main)"),
            "提供者给的优先"
        );
        assert_eq!(
            source_line(Some(" ".into()), Some(&w)).as_deref(),
            Some("ghostty · nvim"),
            "空的不算"
        );
        assert_eq!(
            source_line(None, Some(&window(Some("google-chrome"), None))).as_deref(),
            Some("google-chrome")
        );
        assert_eq!(
            source_line(None, Some(&window(None, Some("标题")))).as_deref(),
            Some("标题")
        );
        assert_eq!(source_line(None, Some(&window(None, None))), None);
        assert_eq!(source_line(None, None), None);
        let long = "长".repeat(80);
        let got = source_line(None, Some(&window(Some("zen"), Some(&long)))).unwrap();
        assert_eq!(got.chars().count(), "zen · ".chars().count() + 60 + 1);
        assert!(got.ends_with('…'));
    }
}
