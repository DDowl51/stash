//! `context-browser` 的纯逻辑。
//!
//! 浏览器窗口的标题是「网页标题 + 浏览器后缀」。去掉后缀，再到这个浏览器最近的访问历史里
//! 找标题相同的那一条，就知道当时在看哪个网址。

use std::collections::BTreeMap;

use browser_contract::Visit;

/// 配置里 app_id 键末尾的这个字符表示「以前面的文字开头的都算」。
pub const PREFIX_MARK: char = '*';

/// 检查一个 app_id 键：`*` 只能写在末尾，写在别处多半是以为支持通配符。
pub fn check_app_key(key: &str) -> Result<(), String> {
    let head = key.strip_suffix(PREFIX_MARK).unwrap_or(key);
    if head.contains(PREFIX_MARK) {
        return Err(format!(
            "apps 的键 `{key}`：`*` 只能写在末尾，表示前缀（比如 \"qutebrowser-*\"）"
        ));
    }
    Ok(())
}

/// 按 app_id 找配置。精确的键优先；否则取末尾带 `*`、前缀最长的那个。
pub fn find_app<'a, V>(apps: &'a BTreeMap<String, V>, app_id: &str) -> Option<&'a V> {
    apps.get(app_id).or_else(|| {
        apps.iter()
            .filter_map(|(k, v)| Some((k.strip_suffix(PREFIX_MARK)?, v)))
            .filter(|(prefix, _)| app_id.starts_with(prefix))
            .max_by_key(|(prefix, _)| prefix.len())
            .map(|(_, v)| v)
    })
}

/// 去掉第一个匹配上的后缀。都不匹配就原样（只去首尾空白）。
pub fn page_title<'a>(window_title: &'a str, suffixes: &[String]) -> &'a str {
    // 只去掉末尾的空白：后缀可能以空格开头，标题为空时整个窗口标题就是后缀
    let t = window_title.trim_end();
    suffixes
        .iter()
        .filter(|s| !s.is_empty())
        .find_map(|s| t.strip_suffix(s.as_str()))
        .unwrap_or(t)
        .trim()
}

/// 比较用的标题：去掉开头的未读数 `(3) `、首尾空白，空白统一。
fn normalize(title: &str) -> String {
    let t = title.trim();
    let t = match t.strip_prefix('(').and_then(|r| r.split_once(") ")) {
        Some((n, rest)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '+') => {
            rest
        }
        _ => t,
    };
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 在访问记录里找标题和 `title` 一样的，取最近的一条。`visits` 顺序不限。
pub fn best_match<'a>(title: &str, visits: &'a [Visit]) -> Option<&'a Visit> {
    let want = normalize(title);
    if want.is_empty() {
        return None;
    }
    visits
        .iter()
        .filter(|v| v.title.as_deref().is_some_and(|t| normalize(t) == want))
        .max_by_key(|v| v.at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suffixes(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn visit(at: i64, url: &str, title: Option<&str>) -> Visit {
        Visit {
            id: at.to_string(),
            at,
            url: url.to_owned(),
            title: title.map(str::to_owned),
        }
    }

    fn apps(keys: &[&str]) -> BTreeMap<String, String> {
        keys.iter()
            .map(|k| ((*k).to_owned(), (*k).to_owned()))
            .collect()
    }

    #[test]
    fn exact_keys_win_over_prefixes() {
        let a = apps(&[
            "qutebrowser-*",
            "qutebrowser-silly",
            "org.qutebrowser.qutebrowser",
        ]);
        let find = |id| find_app(&a, id).map(String::as_str);
        assert_eq!(find("qutebrowser-silly"), Some("qutebrowser-silly"));
        assert_eq!(find("qutebrowser-new-api"), Some("qutebrowser-*"));
        assert_eq!(
            find("org.qutebrowser.qutebrowser"),
            Some("org.qutebrowser.qutebrowser")
        );
        assert_eq!(find("zen"), None);
    }

    #[test]
    fn the_longest_prefix_wins() {
        let a = apps(&["qutebrowser-*", "qutebrowser-work-*", "*"]);
        let find = |id| find_app(&a, id).map(String::as_str);
        assert_eq!(find("qutebrowser-work-2"), Some("qutebrowser-work-*"));
        assert_eq!(find("qutebrowser-home"), Some("qutebrowser-*"));
        assert_eq!(find("kitty"), Some("*"));
    }

    #[test]
    fn the_text_before_the_star_must_all_be_there() {
        // 前缀是 `*` 前面的全部文字，包括末尾的 `-`
        let a = apps(&["qutebrowser-*"]);
        assert_eq!(find_app(&a, "qutebrowser"), None);
        assert_eq!(
            find_app(&a, "qutebrowser-").map(String::as_str),
            Some("qutebrowser-*")
        );
    }

    #[test]
    fn stars_only_at_the_end() {
        assert!(check_app_key("qutebrowser-*").is_ok());
        assert!(check_app_key("zen").is_ok());
        assert!(check_app_key("*").is_ok());
        let err = check_app_key("qute*-x").unwrap_err();
        assert!(err.contains("qute*-x") && err.contains("末尾"), "{err}");
        assert!(check_app_key("**").is_err());
    }

    #[test]
    fn titles_lose_the_browser_suffix() {
        let zen = suffixes(&[" — Zen Browser", " - Zen Browser"]);
        assert_eq!(
            page_title("PR #12 · niri — Zen Browser", &zen),
            "PR #12 · niri"
        );
        assert_eq!(page_title("PR #12 - Zen Browser ", &zen), "PR #12");
        assert_eq!(page_title("没有后缀", &zen), "没有后缀");
        assert_eq!(page_title("x", &suffixes(&[""])), "x");
    }

    #[test]
    fn the_newest_visit_with_the_same_title() {
        let visits = [
            visit(1, "https://a/old", Some("Inbox")),
            visit(3, "https://a/new", Some("(3) Inbox")),
            visit(2, "https://b", Some("Other")),
            visit(4, "https://c", None),
        ];
        assert_eq!(best_match("Inbox", &visits).unwrap().url, "https://a/new");
        assert_eq!(
            best_match("(12) Inbox", &visits).unwrap().url,
            "https://a/new"
        );
        assert_eq!(best_match("  Other ", &visits).unwrap().url, "https://b");
        assert!(best_match("Nope", &visits).is_none());
        assert!(best_match("", &visits).is_none());
    }

    #[test]
    fn brackets_that_are_not_counts_stay() {
        let visits = [visit(1, "https://x", Some("(draft) plan"))];
        assert!(best_match("(draft) plan", &visits).is_some());
        assert!(best_match("plan", &visits).is_none());
    }
}
