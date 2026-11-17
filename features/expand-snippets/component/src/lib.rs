//! 应用功能 `expand-snippets`：把输入里配置好的缩写（整个词）换成模板文字。
//!
//! 逻辑只有一个纯函数 [`replace`]，就放在组件里。

use std::collections::BTreeMap;
use std::sync::Arc;

use capture_contracts::{CAPTURE_EXPANDERS, Draft, Expander, ExpanderSpec};
use kernel_api::{
    AppError, Component, ConfigSection, Host, Kind, Manifest, async_trait, register_component,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
struct Config {
    /// 缩写 → 模板，比如 `";会" = "开会时提到："`。
    snippets: BTreeMap<String, String>,
}

/// 按空格分词，和某个缩写完全相同的词换成模板。
fn replace(text: &str, snippets: &BTreeMap<String, String>) -> String {
    text.split(' ')
        .map(|w| snippets.get(w).map_or(w, String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

struct Snippets(BTreeMap<String, String>);

#[async_trait]
impl Expander for Snippets {
    async fn spec(&self) -> Result<ExpanderSpec, AppError> {
        Ok(ExpanderSpec {
            id: "snippets".to_owned(),
        })
    }

    async fn expand(&self, mut draft: Draft) -> Result<Draft, AppError> {
        draft.text = replace(&draft.text, &self.0);
        Ok(draft)
    }
}

pub struct ExpandSnippets;

impl Component for ExpandSnippets {
    fn config_defaults(&self) -> Option<ConfigSection> {
        ConfigSection::defaults::<Config>()
    }

    fn manifest(&self) -> Manifest {
        Manifest {
            id: "expand-snippets",
            kind: Kind::Feature,
            summary: "把输入里配置好的缩写换成模板文字",
            provides: vec![],
            requires: vec![CAPTURE_EXPANDERS.id()],
            optional_requires: vec![],
        }
    }

    fn start(&self, config: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        let config: Config = config.parse()?;
        if let Some(bad) = config
            .snippets
            .keys()
            .find(|k| k.is_empty() || k.contains(char::is_whitespace))
        {
            return Err(AppError::config(format!(
                "缩写 `{bad}` 不行：不能是空的，不能带空白"
            )));
        }
        host.contribute(&CAPTURE_EXPANDERS, Arc::new(Snippets(config.snippets)))
    }
}

register_component!(ExpandSnippets);

#[cfg(test)]
mod tests {
    use capture_contracts::testing::{CaptureProbe, expander_contract};
    use serde_json::json;

    use super::*;

    fn snippets() -> BTreeMap<String, String> {
        BTreeMap::from([
            (";会".to_owned(), "开会时提到：".to_owned()),
            (";回".to_owned(), "回复：".to_owned()),
        ])
    }

    #[test]
    fn whole_words_only() {
        assert_eq!(
            replace(";会 预算超了 #t", &snippets()),
            "开会时提到： 预算超了 #t"
        );
        assert_eq!(replace("a;会 ;会b", &snippets()), "a;会 ;会b");
        assert_eq!(replace("", &snippets()), "");
    }

    #[tokio::test]
    async fn passes_the_expander_contract() {
        expander_contract(Arc::new(Snippets(snippets())), "没有缩写")
            .await
            .unwrap();
    }

    async fn boot(config: serde_json::Value) -> Result<CaptureProbe, AppError> {
        let probe = CaptureProbe::default();
        testkit::app()
            .with(ExpandSnippets)
            .config("expand-snippets", config)?
            .with(probe.clone())
            .boot()
            .await?;
        Ok(probe)
    }

    #[tokio::test]
    async fn config_is_checked_at_startup() {
        let probe = boot(json!({ "snippets": { ";会": "开会：" } }))
            .await
            .unwrap();
        assert_eq!(probe.expanders().len(), 1);
        let err = boot(json!({ "snippets": { "a b": "x" } }))
            .await
            .err()
            .unwrap();
        assert_eq!(err.kind, kernel_api::ErrorKind::Config);
    }
}
