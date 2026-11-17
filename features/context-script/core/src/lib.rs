//! `context-script` 的纯逻辑。
//!
//! 脚本从环境变量拿到当时的情况，stdout 给出上下文：
//! - 每行一个 JSON 对象 `{"key","label","kind","value"}`（`label`、`kind` 可省）；
//! - 否则整段输出算一条文字。

use capture_contracts::{ContextEntry, ContextKind, Snapshot};
use jiff::Timestamp;
use jiff::tz::{Offset, TimeZone};
use kernel_api::AppError;
use serde::Deserialize;

/// 一段文字输出最多留多少字符。
const TEXT_MAX: usize = 4_000;

/// 脚本名只能是小写字母、数字、`-`、`_`：它会出现在上下文的 key 里。
pub fn check_name(name: &str) -> Result<(), AppError> {
    if !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        Ok(())
    } else {
        Err(AppError::config(format!(
            "脚本名 `{name}` 只能用小写字母、数字、- 和 _"
        )))
    }
}

/// `when_apps` 为空表示总是运行。
pub fn applies(when_apps: &[String], app_id: Option<&str>) -> bool {
    when_apps.is_empty() || app_id.is_some_and(|a| when_apps.iter().any(|w| w == a))
}

/// 给脚本的环境变量。没有的值给空字符串，脚本不用判断变量在不在。
pub fn env(snapshot: &Snapshot) -> Vec<(String, String)> {
    let w = snapshot.window.as_ref();
    let at = Timestamp::from_millisecond(snapshot.at.utc_ms)
        .ok()
        .zip(Offset::from_seconds(snapshot.at.offset_min * 60).ok())
        .map(|(ts, off)| {
            ts.to_zoned(TimeZone::fixed(off))
                .strftime("%Y-%m-%dT%H:%M:%S%:z")
                .to_string()
        })
        .unwrap_or_default();
    let s = |v: Option<String>| v.unwrap_or_default();
    [
        ("STASH_ITEM_ID", snapshot.item_id.clone()),
        ("STASH_AT", at),
        ("STASH_AT_MS", snapshot.at.utc_ms.to_string()),
        ("STASH_APP_ID", s(w.and_then(|w| w.app_id.clone()))),
        ("STASH_TITLE", s(w.and_then(|w| w.title.clone()))),
        ("STASH_WORKSPACE", s(w.and_then(|w| w.workspace.clone()))),
        ("STASH_WINDOW_ID", s(w.map(|w| w.id.to_string()))),
        ("STASH_PID", s(w.and_then(|w| w.pid).map(|p| p.to_string()))),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Line {
    key: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    kind: Option<ContextKind>,
    value: String,
}

/// 脚本 `name` 的输出 → 上下文。key 都加上 `script.<name>.` 前缀。
pub fn parse_output(name: &str, stdout: &str) -> Vec<ContextEntry> {
    let text = stdout.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let lines: Option<Vec<Line>> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Line>(l).ok())
        .collect();
    match lines {
        Some(lines) => lines
            .into_iter()
            .map(|l| {
                ContextEntry::new(
                    format!("script.{name}.{}", l.key),
                    l.label.unwrap_or_else(|| l.key.clone()),
                    l.kind.unwrap_or(ContextKind::Text),
                    l.value,
                )
            })
            .collect(),
        None => {
            let value: String = if text.chars().count() > TEXT_MAX {
                format!("{}…", text.chars().take(TEXT_MAX).collect::<String>())
            } else {
                text.to_owned()
            };
            vec![ContextEntry::new(
                format!("script.{name}"),
                name,
                ContextKind::Text,
                value,
            )]
        }
    }
}

/// 脚本失败（起不来、超时、退出码不是 0）→ 一条说明。
pub fn failure(name: &str, why: &str) -> ContextEntry {
    let first = why
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    ContextEntry::new(
        format!("script.{name}"),
        format!("{name}（脚本失败）"),
        ContextKind::Text,
        first,
    )
}

#[cfg(test)]
mod tests {
    use capture_contracts::WindowInfo;
    use capture_contracts::testing::snapshot;

    use super::*;

    #[test]
    fn names() {
        check_name("lifelog").unwrap();
        check_name("a_b-2").unwrap();
        for bad in ["", "Life", "a.b", "a b"] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn when_apps() {
        assert!(applies(&[], None));
        let only = vec!["zen".to_owned()];
        assert!(applies(&only, Some("zen")));
        assert!(!applies(&only, Some("foot")));
        assert!(!applies(&only, None));
    }

    #[test]
    fn environment() {
        let window = WindowInfo {
            id: 7,
            app_id: Some("zen".to_owned()),
            title: Some("PR".to_owned()),
            workspace: None,
            pid: Some(42),
        };
        let vars = env(&snapshot(Some(window)));
        let get = |k: &str| {
            vars.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("STASH_AT"), Some("2026-11-18T00:30:00+08:00"));
        assert_eq!(get("STASH_AT_MS"), Some("1794933000000"));
        assert_eq!(get("STASH_APP_ID"), Some("zen"));
        assert_eq!(get("STASH_PID"), Some("42"));
        assert_eq!(get("STASH_WINDOW_ID"), Some("7"));
        assert_eq!(get("STASH_WORKSPACE"), Some(""), "没有的给空字符串");
        assert_eq!(get("STASH_ITEM_ID"), Some("01JD8X0000000000000000ABCD"));
        let none = env(&snapshot(None));
        assert_eq!(none.iter().find(|(k, _)| k == "STASH_TITLE").unwrap().1, "");
    }

    #[test]
    fn json_lines() {
        let out = "{\"key\":\"activity\",\"label\":\"在做\",\"value\":\"写 stash\"}\n\n{\"key\":\"doc\",\"kind\":\"url\",\"value\":\"https://x\"}\n";
        let got = parse_output("lifelog", out);
        assert_eq!(got.len(), 2);
        assert_eq!(
            (
                got[0].key.as_str(),
                got[0].label.as_str(),
                got[0].value.as_str()
            ),
            ("script.lifelog.activity", "在做", "写 stash")
        );
        assert_eq!(
            (got[1].label.as_str(), got[1].kind),
            ("doc", ContextKind::Url)
        );
    }

    #[test]
    fn plain_text_and_mixed_output() {
        let got = parse_output("now", "  正在听：某首歌\n");
        assert_eq!(got.len(), 1);
        assert_eq!(
            (got[0].key.as_str(), got[0].value.as_str()),
            ("script.now", "正在听：某首歌")
        );
        // 有一行不是 JSON：整段当文字
        let got = parse_output("x", "{\"key\":\"a\",\"value\":\"b\"}\nplain");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].key, "script.x");
        assert!(parse_output("x", " \n ").is_empty());
        let long = "字".repeat(5_000);
        assert_eq!(parse_output("x", &long)[0].value.chars().count(), 4_001);
    }

    #[test]
    fn failures_keep_the_first_line() {
        let got = failure("lifelog", "\nerror: no database\nmore");
        assert_eq!(got.key, "script.lifelog");
        assert_eq!(got.value, "error: no database");
    }
}
