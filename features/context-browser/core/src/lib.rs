//! `context-browser` 的纯逻辑。
//!
//! 浏览器窗口的标题是「网页标题 + 浏览器后缀」。去掉后缀，再到这个浏览器最近的访问历史里
//! 找标题相同的那一条，就知道当时在看哪个网址。

use browser_contract::Visit;

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
