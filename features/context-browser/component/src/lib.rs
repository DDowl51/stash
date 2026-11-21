//! 应用功能 `context-browser`：来源窗口是浏览器时，记下当时在看的网页。
//!
//! 不读浏览器的会话文件：窗口标题去掉浏览器后缀，在这个浏览器最近一段时间的访问历史里
//! 找标题相同的那条，拿它的网址。找不到就只记标题，并说明没找到网址。

use std::collections::BTreeMap;
use std::sync::Arc;

use browser_contract::{BROWSER_HISTORY, BrowserHistory, Visit};
use capture_contracts::{
    CAPTURE_CONTEXT, ContextEntry, ContextKind, ContextProvider, ContextSpec, Phase, Snapshot,
};
use context_browser_core::{best_match, check_app_key, find_app, page_title};
use kernel_api::{
    AppError, Component, ConfigSection, Host, Kind, Manifest, async_trait, register_component,
};
use serde::{Deserialize, Serialize};

/// 一种浏览器窗口。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct App {
    /// 历史来源里的浏览器名（`browser-*` 适配报出来的）。
    browser: String,
    /// 窗口标题末尾的浏览器后缀，去掉之后就是网页标题。按顺序试，第一个匹配的生效。
    #[serde(default)]
    title_suffixes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
struct Config {
    /// app_id → 浏览器。键以 `*` 结尾表示前缀，精确的键优先。
    apps: BTreeMap<String, App>,
    /// 往回找多久的历史。
    lookback_minutes: u32,
    /// 每个来源最多看多少条。
    max_visits: u32,
}

impl Default for Config {
    fn default() -> Self {
        let app = |browser: &str, suffixes: &[&str]| App {
            browser: browser.to_owned(),
            title_suffixes: suffixes.iter().map(|s| (*s).to_owned()).collect(),
        };
        Self {
            apps: BTreeMap::from([
                (
                    "zen".to_owned(),
                    app("zen", &[" — Zen Browser", " - Zen Browser"]),
                ),
                (
                    "google-chrome".to_owned(),
                    app("chrome", &[" - Google Chrome"]),
                ),
                ("chromium".to_owned(), app("chromium", &[" - Chromium"])),
                (
                    "org.qutebrowser.qutebrowser".to_owned(),
                    app("qutebrowser", &[" - qutebrowser"]),
                ),
                (
                    "firefox".to_owned(),
                    app("firefox", &[" — Mozilla Firefox", " - Mozilla Firefox"]),
                ),
            ]),
            lookback_minutes: 30,
            max_visits: 300,
        }
    }
}

impl Config {
    /// 启动时检查，写错的键尽早报出来。
    fn check(&self) -> Result<(), AppError> {
        self.apps
            .keys()
            .try_for_each(|k| check_app_key(k))
            .map_err(|e| AppError::config(format!("context-browser 的 {e}")))
    }
}

struct Provider {
    config: Config,
    history: Arc<dyn BrowserHistory>,
}

fn entry(key: &str, label: &str, kind: ContextKind, value: impl Into<String>) -> ContextEntry {
    ContextEntry::new(format!("browser.{key}"), label, kind, value)
}

impl Provider {
    /// 这个浏览器所有来源（profile）里最近的访问。一个来源读不了不影响别的。
    async fn recent(&self, browser: &str, since: i64) -> Result<Vec<Visit>, AppError> {
        let sources = self.history.sources().await?;
        let mut all = Vec::new();
        for source in sources.into_iter().filter(|s| s.browser == browser) {
            if let Ok(visits) = self
                .history
                .recent(source.id, since, self.config.max_visits)
                .await
            {
                all.extend(visits);
            }
        }
        Ok(all)
    }
}

#[async_trait]
impl ContextProvider for Provider {
    async fn spec(&self) -> Result<ContextSpec, AppError> {
        Ok(ContextSpec {
            id: "browser".to_owned(),
            phase: Phase::WhilePrompt,
        })
    }

    async fn collect(&self, snapshot: Snapshot) -> Result<Vec<ContextEntry>, AppError> {
        let Some(window) = snapshot.window else {
            return Ok(Vec::new());
        };
        let Some(app) = window
            .app_id
            .as_deref()
            .and_then(|a| find_app(&self.config.apps, a))
        else {
            return Ok(Vec::new());
        };
        let title = page_title(window.title.as_deref().unwrap_or(""), &app.title_suffixes);
        if title.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = vec![entry("title", "网页", ContextKind::Text, title)];
        let since = snapshot
            .at
            .utc_ms
            .saturating_sub(i64::from(self.config.lookback_minutes) * 60_000);
        let visits = self.recent(&app.browser, since).await?;
        match best_match(title, &visits) {
            Some(v) => out.push(entry("url", "网址", ContextKind::Url, v.url.clone())),
            None => out.push(entry(
                "note",
                "网址",
                ContextKind::Text,
                format!(
                    "最近 {} 分钟的 {} 历史里没找到这个标题，只记了标题",
                    self.config.lookback_minutes, app.browser
                ),
            )),
        }
        Ok(out)
    }

    async fn discard(&self, _snapshot: Snapshot) -> Result<(), AppError> {
        Ok(())
    }
}

pub struct ContextBrowser;

impl Component for ContextBrowser {
    fn config_defaults(&self) -> Option<ConfigSection> {
        ConfigSection::defaults::<Config>()
    }

    fn manifest(&self) -> Manifest {
        Manifest {
            id: "context-browser",
            kind: Kind::Feature,
            summary: "来源是浏览器时：当时在看的网页标题和网址",
            provides: vec![],
            requires: vec![CAPTURE_CONTEXT.id(), BROWSER_HISTORY.id()],
            optional_requires: vec![],
        }
    }

    fn start(&self, config: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        let config: Config = config.parse()?;
        config.check()?;
        let provider = Provider {
            history: host.resolve(&BROWSER_HISTORY)?,
            config,
        };
        host.contribute(&CAPTURE_CONTEXT, Arc::new(provider))
    }
}

register_component!(ContextBrowser);

#[cfg(test)]
mod tests {
    use browser_contract::testing::{MemoryBrowser, visit};
    use browser_contract::{BrowserAdapter, HistorySource, VisitPage};
    use capture_contracts::WindowInfo;
    use capture_contracts::testing::{context_contract, snapshot};

    use super::*;

    /// 用内存浏览器当 `BrowserHistory`（只有一个适配时两者的行为相同）。
    struct Hist(MemoryBrowser);

    #[async_trait]
    impl BrowserHistory for Hist {
        async fn sources(&self) -> Result<Vec<HistorySource>, AppError> {
            self.0.sources().await
        }
        async fn read(&self, s: String, a: Option<String>, l: u32) -> Result<VisitPage, AppError> {
            self.0.read(s, a, l).await
        }
        async fn recent(&self, s: String, since: i64, l: u32) -> Result<Vec<Visit>, AppError> {
            self.0.recent(s, since, l).await
        }
    }

    // testing::snapshot 的时刻
    const NOW: i64 = 1_794_933_000_000;

    fn provider() -> Provider {
        let browser = MemoryBrowser::default();
        browser.push(
            "zen/Default",
            [
                visit(
                    "",
                    NOW - 60 * 60_000,
                    "https://old.example/",
                    "PR #12 · niri",
                ),
                visit(
                    "",
                    NOW - 5 * 60_000,
                    "https://github.com/niri/pull/12",
                    "PR #12 · niri",
                ),
                visit("", NOW - 60_000, "https://other.example/", "别的"),
            ],
        );
        browser.push(
            "zen/Work",
            [visit(
                "",
                NOW - 2 * 60_000,
                "https://work.example/",
                "工作页",
            )],
        );
        browser.push(
            "chrome/Default",
            [visit("", NOW - 60_000, "https://chrome.example/", "工作页")],
        );
        Provider {
            config: Config::default(),
            history: Arc::new(Hist(browser)),
        }
    }

    fn zen(title: &str) -> WindowInfo {
        WindowInfo {
            id: 3,
            app_id: Some("zen".to_owned()),
            title: Some(title.to_owned()),
            workspace: None,
            pid: Some(10),
        }
    }

    fn pairs(entries: &[ContextEntry]) -> Vec<(&str, &str)> {
        entries
            .iter()
            .map(|e| (e.key.as_str(), e.value.as_str()))
            .collect()
    }

    #[tokio::test]
    async fn passes_the_context_contract() {
        context_contract(Arc::new(provider()), zen("PR #12 · niri — Zen Browser"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn finds_the_url_by_title_in_recent_history() {
        let got = provider()
            .collect(snapshot(Some(zen("PR #12 · niri — Zen Browser"))))
            .await
            .unwrap();
        assert_eq!(
            pairs(&got),
            [
                ("browser.title", "PR #12 · niri"),
                ("browser.url", "https://github.com/niri/pull/12"),
            ]
        );
        assert_eq!(got[1].kind, ContextKind::Url);
    }

    #[tokio::test]
    async fn every_profile_of_that_browser_but_not_other_browsers() {
        let got = provider()
            .collect(snapshot(Some(zen("工作页 — Zen Browser"))))
            .await
            .unwrap();
        assert_eq!(
            got[1].value, "https://work.example/",
            "zen 的另一个 profile，不是 chrome 的"
        );
    }

    #[tokio::test]
    async fn not_found_keeps_the_title_and_says_so() {
        let got = provider()
            .collect(snapshot(Some(zen("新标签页 — Zen Browser"))))
            .await
            .unwrap();
        assert_eq!(got[0].value, "新标签页");
        assert_eq!(got[1].key, "browser.note");
        assert!(got[1].value.contains("没找到"), "{got:?}");
    }

    #[tokio::test]
    async fn prefix_keys_cover_every_window_of_that_kind() {
        let mut p = provider();
        p.config.apps.insert(
            "zen-*".to_owned(),
            App {
                browser: "zen".to_owned(),
                title_suffixes: vec![" — Zen Browser".to_owned()],
            },
        );
        let profile = WindowInfo {
            app_id: Some("zen-work".to_owned()),
            ..zen("PR #12 · niri — Zen Browser")
        };
        let got = p.collect(snapshot(Some(profile))).await.unwrap();
        assert_eq!(got[1].value, "https://github.com/niri/pull/12");
    }

    #[test]
    fn a_star_in_the_middle_is_a_config_error() {
        assert!(Config::default().check().is_ok());
        let mut c = Config::default();
        c.apps.insert("qute*-x".to_owned(), c.apps["zen"].clone());
        let err = c.check().unwrap_err();
        assert_eq!(err.kind, kernel_api::ErrorKind::Config);
        assert!(err.message.contains("context-browser"), "{err:?}");
    }

    #[tokio::test]
    async fn other_apps_and_blank_titles_are_skipped() {
        let p = provider();
        let term = WindowInfo {
            app_id: Some("foot".to_owned()),
            ..zen("x")
        };
        assert!(p.collect(snapshot(Some(term))).await.unwrap().is_empty());
        assert!(
            p.collect(snapshot(Some(zen(" — Zen Browser"))))
                .await
                .unwrap()
                .is_empty()
        );
    }
}
