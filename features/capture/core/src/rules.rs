//! 内置的输入规则：`#分类`、`@日期`、URL。在所有展开器之后执行。

use capture_contracts::{Draft, Kind};
use jiff::civil::Date;
use kernel_api::AppError;

/// 分类标记和「只有 URL 时」归到哪类。来自配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    /// 标记（小写，不带 `#`）→ 分类。
    tags: Vec<(String, Kind)>,
    /// 标记（小写，不带 `#`）→ 路由名。
    routes: Vec<(String, String)>,
    url_kind: Kind,
}

/// 标记不能是空的、不能带空白或 `#`。
fn check_tag(word: &str, what: &str) -> Result<String, AppError> {
    let tag = word.to_lowercase();
    if tag.is_empty() || tag.contains(char::is_whitespace) || tag.contains('#') {
        return Err(AppError::config(format!(
            "{what} `{word}` 不行：不能是空的，不能带空白或 #"
        )));
    }
    Ok(tag)
}

/// URL 末尾常跟着的标点，不算 URL 的一部分。
const TRAILING: &[char] = &[',', '.', ';', ':', '!', '?', ')', '\'', '"'];

/// 以 `http(s)://` 开头的词里的 URL。中文里 URL 后面常常不空格直接接着字
/// （`https://a.com/x。还有`），所以到第一个非 ASCII 字符为止，再去掉末尾的标点。
fn url_in(token: &str) -> Option<&str> {
    let end = token.find(|c: char| !c.is_ascii()).unwrap_or(token.len());
    let url = token[..end].trim_end_matches(TRAILING);
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    (!rest.is_empty()).then_some(url)
}

impl Rules {
    /// 标记不能是空的、不能带空白或 `#`，同一个标记不能指向两个分类。
    pub fn new(
        tags: impl IntoIterator<Item = (Kind, Vec<String>)>,
        url_kind: Kind,
    ) -> Result<Self, AppError> {
        let mut all: Vec<(String, Kind)> = Vec::new();
        for (kind, words) in tags {
            for word in words {
                let tag = check_tag(&word, "分类标记")?;
                if let Some((_, other)) = all.iter().find(|(t, _)| *t == tag)
                    && *other != kind
                {
                    return Err(AppError::config(format!(
                        "分类标记 `{word}` 同时指向 {} 和 {}",
                        other.as_str(),
                        kind.as_str()
                    )));
                }
                all.push((tag, kind));
            }
        }
        Ok(Self {
            tags: all,
            routes: Vec::new(),
            url_kind,
        })
    }

    /// 路由标记：路由名 → 标记。`#标记` 从文字里去掉，路由名记进草稿的 `routes`。
    /// 路由名不能有空白；标记不能和分类标记、别的路由的标记重复。
    pub fn with_routes(
        mut self,
        routes: impl IntoIterator<Item = (String, Vec<String>)>,
    ) -> Result<Self, AppError> {
        for (route, words) in routes {
            if route.trim().is_empty() || route.contains(char::is_whitespace) {
                return Err(AppError::config(format!(
                    "路由名 `{route}` 不能是空的、不能带空白"
                )));
            }
            for word in words {
                let tag = check_tag(&word, "路由标记")?;
                if self.tags.iter().any(|(t, _)| *t == tag) {
                    return Err(AppError::config(format!(
                        "路由标记 `{word}` 和分类标记重复了"
                    )));
                }
                if let Some((_, other)) = self.routes.iter().find(|(t, _)| *t == tag)
                    && *other != route
                {
                    return Err(AppError::config(format!(
                        "路由标记 `{word}` 同时指向 {other} 和 {route}"
                    )));
                }
                self.routes.push((tag, route.clone()));
            }
        }
        Ok(self)
    }

    fn route(&self, word: &str) -> Option<&str> {
        let word = word.to_lowercase();
        self.routes
            .iter()
            .find(|(t, _)| *t == word)
            .map(|(_, r)| r.as_str())
    }

    fn tag(&self, word: &str) -> Option<Kind> {
        let word = word.to_lowercase();
        self.tags.iter().find(|(t, _)| *t == word).map(|(_, k)| *k)
    }

    /// 应用规则。
    ///
    /// - `#标记`（配置里有的）→ 分类，从文字里去掉；没配置的 `#词` 原样保留；
    /// - `@日期`（认得出的）→ 截止日期，从文字里去掉；
    /// - 第一个 URL 填进 `url`（草稿里已经有就不动）；
    /// - 整段只剩一个 URL 时，分类按 `url_kind`。
    ///
    /// 分类的优先级：文字里的 `#` > 草稿里已有的（命令行 `--kind`、展开器给的）> 只有 URL。
    /// 日期同理：文字里的 `@` > 草稿里已有的。写了好几个时以最后一个为准。
    /// 空白统一成一个空格。去掉标记后可能是空的，由调用方决定怎么办。
    pub fn apply(&self, draft: Draft, today: Date) -> Draft {
        let mut kind_tag = None;
        let mut due = None;
        let mut routes = draft.routes;
        let mut kept: Vec<&str> = Vec::new();
        for token in draft.text.split_whitespace() {
            if let Some(route) = token.strip_prefix('#').and_then(|w| self.route(w)) {
                if !routes.iter().any(|r| r == route) {
                    routes.push(route.to_owned());
                }
            } else if let Some(kind) = token.strip_prefix('#').and_then(|w| self.tag(w)) {
                kind_tag = Some(kind);
            } else if let Some(date) = token
                .strip_prefix('@')
                .and_then(|w| zh_dates::parse(w, today))
            {
                due = Some(date);
            } else {
                kept.push(token);
            }
        }
        let first_url = kept.iter().find_map(|t| url_in(t));
        let only_url = matches!(kept.as_slice(), [one] if url_in(one) == Some(*one));
        Draft {
            kind: kind_tag
                .or(draft.kind)
                .or_else(|| only_url.then_some(self.url_kind)),
            due_on: due.map(|d| d.to_string()).or(draft.due_on),
            url: draft.url.or_else(|| first_url.map(str::to_owned)),
            text: kept.join(" "),
            context: draft.context,
            routes,
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel_api::ErrorKind;

    use super::*;

    fn rules() -> Rules {
        Rules::new(
            [
                (Kind::Todo, vec!["todo".into(), "t".into(), "待办".into()]),
                (Kind::Idea, vec!["idea".into(), "i".into(), "想法".into()]),
                (
                    Kind::ReadLater,
                    vec!["read".into(), "r".into(), "读".into()],
                ),
                (Kind::Inbox, vec!["inbox".into(), "in".into()]),
            ],
            Kind::ReadLater,
        )
        .unwrap()
    }

    fn today() -> Date {
        "2026-11-18".parse().unwrap() // 周三
    }

    fn apply(text: &str) -> Draft {
        apply_to(Draft {
            text: text.to_owned(),
            ..Draft::default()
        })
    }

    fn apply_to(draft: Draft) -> Draft {
        rules().apply(draft, today())
    }

    fn want(text: &str, kind: Option<Kind>, due: Option<&str>, url: Option<&str>) -> Draft {
        Draft {
            text: text.to_owned(),
            kind,
            due_on: due.map(str::to_owned),
            url: url.map(str::to_owned),
            context: vec![],
            routes: vec![],
        }
    }

    #[test]
    fn the_syntax_table_from_the_design() {
        for (input, expected) in [
            // `#` + 配置里的标记 → 分类，任意位置，从文字里去掉；大小写不论
            (
                "帮老王看下 PR #t",
                want("帮老王看下 PR", Some(Kind::Todo), None, None),
            ),
            (
                "#待办 帮老王看下 PR",
                want("帮老王看下 PR", Some(Kind::Todo), None, None),
            ),
            (
                "帮老王 #I 看下",
                want("帮老王 看下", Some(Kind::Idea), None, None),
            ),
            // 不在配置里的 `#词` 原样保留
            ("学 #C++ 模板", want("学 #C++ 模板", None, None, None)),
            ("issue #12 修掉", want("issue #12 修掉", None, None, None)),
            // `@日期` → 截止日期
            (
                "交周报 @周五",
                want("交周报", None, Some("2026-11-20"), None),
            ),
            (
                "交周报 @明天 #t",
                want("交周报", Some(Kind::Todo), Some("2026-11-19"), None),
            ),
            (
                "@11-20 交周报",
                want("交周报", None, Some("2026-11-20"), None),
            ),
            (
                "@2026-12-01 交周报",
                want("交周报", None, Some("2026-12-01"), None),
            ),
            // 认不出的 `@词` 原样保留
            ("问 @老王 要文档", want("问 @老王 要文档", None, None, None)),
            // 整段只有一个 URL → url_kind
            (
                "https://example.com/post",
                want(
                    "https://example.com/post",
                    Some(Kind::ReadLater),
                    None,
                    Some("https://example.com/post"),
                ),
            ),
            // 去掉标记后只有 URL 也算
            (
                "@明天 https://example.com/post",
                want(
                    "https://example.com/post",
                    Some(Kind::ReadLater),
                    Some("2026-11-19"),
                    Some("https://example.com/post"),
                ),
            ),
            // 只有 URL 但写了 `#`：以 `#` 为准
            (
                "https://example.com/post #i",
                want(
                    "https://example.com/post",
                    Some(Kind::Idea),
                    None,
                    Some("https://example.com/post"),
                ),
            ),
            // 文字里带 URL：填第一个，分类不变；末尾的标点不算
            (
                "看 https://a.com/x。还有 https://b.com",
                want(
                    "看 https://a.com/x。还有 https://b.com",
                    None,
                    None,
                    Some("https://a.com/x"),
                ),
            ),
            // 写了好几个：以最后一个为准
            (
                "x #t #i @明天 @后天",
                want("x", Some(Kind::Idea), Some("2026-11-20"), None),
            ),
            // 空白统一
            ("  a \t  b  ", want("a b", None, None, None)),
            // 只剩标记：文字为空，交给调用方
            (
                "#t @明天",
                want("", Some(Kind::Todo), Some("2026-11-19"), None),
            ),
            // 光一个协议头不算 URL
            ("https://", want("https://", None, None, None)),
            // 英文标点、括号
            (
                "见 (https://a.com/x).",
                want("见 (https://a.com/x).", None, None, None),
            ),
            (
                "见 https://a.com/x).",
                want("见 https://a.com/x).", None, None, Some("https://a.com/x")),
            ),
        ] {
            assert_eq!(apply(input), expected, "{input}");
        }
    }

    #[test]
    fn text_wins_over_what_the_draft_already_has() {
        // 命令行 --kind todo，文字里 #i：以 # 为准
        let got = apply_to(Draft {
            text: "x #i @明天".into(),
            kind: Some(Kind::Todo),
            due_on: Some("2026-12-31".into()),
            ..Draft::default()
        });
        assert_eq!(
            (got.kind, got.due_on.as_deref()),
            (Some(Kind::Idea), Some("2026-11-19"))
        );

        // 没写 # 时保留草稿里的；只有 URL 也不覆盖明确给的分类
        let got = apply_to(Draft {
            text: "https://example.com".into(),
            kind: Some(Kind::Todo),
            url: Some("https://given.example".into()),
            ..Draft::default()
        });
        assert_eq!(got.kind, Some(Kind::Todo));
        assert_eq!(got.url.as_deref(), Some("https://given.example"));
    }

    #[test]
    fn bad_tag_config_is_a_config_error() {
        for tags in [
            vec![(Kind::Todo, vec![String::new()])],
            vec![(Kind::Todo, vec!["a b".into()])],
            vec![(Kind::Todo, vec!["#t".into()])],
            vec![
                (Kind::Todo, vec!["t".into()]),
                (Kind::Idea, vec!["T".into()]),
            ],
        ] {
            assert_eq!(
                Rules::new(tags, Kind::Inbox).unwrap_err().kind,
                ErrorKind::Config
            );
        }
        // 同一个分类重复写不算错
        Rules::new([(Kind::Todo, vec!["t".into(), "T".into()])], Kind::Inbox).unwrap();
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;

    fn rules() -> Rules {
        Rules::new([(Kind::Todo, vec!["t".into()])], Kind::ReadLater)
            .unwrap()
            .with_routes([("dida".to_owned(), vec!["滴".into(), "Dida".into()])])
            .unwrap()
    }

    fn apply(draft: Draft) -> Draft {
        rules().apply(draft, "2026-11-18".parse().unwrap())
    }

    #[test]
    fn route_tags_leave_the_text_and_land_in_routes() {
        let d = apply(Draft {
            text: "看 PR #滴 #t #DIDA".into(),
            ..Draft::default()
        });
        assert_eq!(d.text, "看 PR");
        assert_eq!(d.kind, Some(Kind::Todo));
        assert_eq!(d.routes, ["dida"], "同一个路由只记一次，标记不分大小写");
    }

    #[test]
    fn routes_from_the_command_line_are_kept() {
        let d = apply(Draft {
            text: "看 PR #滴".into(),
            routes: vec!["mail".into()],
            ..Draft::default()
        });
        assert_eq!(d.routes, ["mail", "dida"]);
        let plain = apply(Draft {
            text: "#滴滴 打车".into(),
            ..Draft::default()
        });
        assert!(plain.routes.is_empty(), "不是配置里的标记原样保留");
        assert_eq!(plain.text, "#滴滴 打车");
    }

    #[test]
    fn route_config_errors() {
        let base = || Rules::new([(Kind::Todo, vec!["t".into()])], Kind::ReadLater).unwrap();
        for bad in [
            vec![("dida".to_owned(), vec!["t".to_owned()])],
            vec![("di da".to_owned(), vec!["x".to_owned()])],
            vec![("dida".to_owned(), vec!["#x".to_owned()])],
            vec![
                ("a".to_owned(), vec!["x".to_owned()]),
                ("b".to_owned(), vec!["x".to_owned()]),
            ],
        ] {
            let err = base().with_routes(bad.clone()).unwrap_err();
            assert_eq!(err.kind, kernel_api::ErrorKind::Config, "{bad:?}");
        }
    }
}
