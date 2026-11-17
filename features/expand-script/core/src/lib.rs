//! `expand-script` 的纯逻辑。
//!
//! 输入以某个前缀开头时（比如 `!gh 123`），前缀后面的文字交给外部命令（stdin），命令的 stdout：
//! - 是 JSON 对象：按 [`Draft`] 读，`text` 换掉文字，给了的 `kind`、`due_on`、`url` 覆盖，`context` 追加；
//! - 否则整段（去掉首尾空白）是新的文字。

use capture_contracts::{ContextEntry, ContextKind, Draft};

/// 找第一个匹配的前缀，返回（规则序号，前缀后面的文字）。
pub fn matching<'a>(text: &'a str, prefixes: &[String]) -> Option<(usize, &'a str)> {
    prefixes
        .iter()
        .enumerate()
        .find_map(|(i, p)| text.strip_prefix(p.as_str()).map(|rest| (i, rest.trim())))
}

/// 命令输出合进草稿。输出是空的算失败（返回原因）。
pub fn merge(mut draft: Draft, stdout: &str) -> Result<Draft, String> {
    let out = stdout.trim();
    if out.is_empty() {
        return Err("命令没有输出".to_owned());
    }
    let before = draft.text.clone();
    if out.starts_with('{') {
        let given: Draft =
            serde_json::from_str(out).map_err(|e| format!("输出看起来是 JSON，但读不懂：{e}"))?;
        if given.text.trim().is_empty() {
            return Err("输出的 text 是空的".to_owned());
        }
        draft.text = given.text;
        draft.kind = given.kind.or(draft.kind);
        draft.due_on = given.due_on.or(draft.due_on);
        draft.url = given.url.or(draft.url);
        draft.context.extend(given.context);
    } else {
        draft.text = out.to_owned();
    }
    draft.context.push(ContextEntry::new(
        "script.rewrite",
        "改写前",
        ContextKind::Text,
        before,
    ));
    Ok(draft)
}

#[cfg(test)]
mod tests {
    use capture_contracts::Kind;

    use super::*;

    fn draft(text: &str) -> Draft {
        Draft {
            text: text.to_owned(),
            ..Draft::default()
        }
    }

    #[test]
    fn prefixes() {
        let p = vec!["!gh ".to_owned(), "!w ".to_owned()];
        assert_eq!(matching("!gh 123", &p), Some((0, "123")));
        assert_eq!(matching("!w  天气 ", &p), Some((1, "天气")));
        assert_eq!(matching("看 !gh 123", &p), None);
    }

    #[test]
    fn plain_text_output() {
        let got = merge(draft("!gh 123"), "  修 niri 的 bug (#123)\n").unwrap();
        assert_eq!(got.text, "修 niri 的 bug (#123)");
        assert_eq!(got.context[0].value, "!gh 123");
    }

    #[test]
    fn json_output() {
        let mut d = draft("!gh 123");
        d.kind = Some(Kind::Idea);
        let got = merge(
            d,
            r#"{"text":"修 bug #t","url":"https://github.com/x/123","context":[{"key":"gh.state","label":"状态","kind":"text","value":"open"}]}"#,
        )
        .unwrap();
        assert_eq!(got.text, "修 bug #t");
        assert_eq!(got.url.as_deref(), Some("https://github.com/x/123"));
        assert_eq!(got.kind, Some(Kind::Idea), "没给的保留");
        let keys: Vec<&str> = got.context.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["gh.state", "script.rewrite"]);
    }

    #[test]
    fn bad_output() {
        assert!(merge(draft("x"), " \n").is_err());
        assert!(merge(draft("x"), "{not json").is_err());
        assert!(merge(draft("x"), r#"{"text":" "}"#).is_err());
    }
}
