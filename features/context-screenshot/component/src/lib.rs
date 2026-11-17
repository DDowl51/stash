//! 应用功能 `context-screenshot`：记录时截下来源窗口，存成 `<dir>/<条目 id>.png`。
//!
//! 在输入框出现之前截（输入框会挡住屏幕）。记录取消时删掉。

use std::sync::Arc;

use capture_contracts::{
    CAPTURE_CONTEXT, ContextEntry, ContextKind, ContextProvider, ContextSpec, Phase, Snapshot,
};
use kernel_api::{
    AppError, Component, ConfigSection, Host, Kind, Manifest, async_trait, register_component,
};
use screenshot_contract::{SCREENSHOTS, Screenshots};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
struct Config {
    /// 截图放在哪个目录（绝对路径）。启动器会给默认值。
    dir: Option<String>,
    /// 这些应用不截（比如密码管理器）。
    skip_apps: Vec<String>,
}

struct Provider {
    dir: String,
    skip_apps: Vec<String>,
    shots: Arc<dyn Screenshots>,
}

impl Provider {
    fn path(&self, snapshot: &Snapshot) -> String {
        format!(
            "{}/{}.png",
            self.dir.trim_end_matches('/'),
            snapshot.item_id
        )
    }
}

#[async_trait]
impl ContextProvider for Provider {
    async fn spec(&self) -> Result<ContextSpec, AppError> {
        Ok(ContextSpec {
            id: "screenshot".to_owned(),
            phase: Phase::BeforePrompt,
        })
    }

    async fn collect(&self, snapshot: Snapshot) -> Result<Vec<ContextEntry>, AppError> {
        let Some(window) = &snapshot.window else {
            return Ok(Vec::new());
        };
        if window
            .app_id
            .as_ref()
            .is_some_and(|a| self.skip_apps.contains(a))
        {
            return Ok(Vec::new());
        }
        let path = self.path(&snapshot);
        self.shots.window(window.id, path.clone()).await?;
        Ok(vec![ContextEntry::new(
            "screenshot.window",
            "截图",
            ContextKind::Image,
            path,
        )])
    }

    async fn discard(&self, snapshot: Snapshot) -> Result<(), AppError> {
        self.shots.remove(self.path(&snapshot)).await.map(|_| ())
    }
}

pub struct ContextScreenshot;

impl Component for ContextScreenshot {
    fn config_defaults(&self) -> Option<ConfigSection> {
        ConfigSection::defaults::<Config>()
    }

    fn manifest(&self) -> Manifest {
        Manifest {
            id: "context-screenshot",
            kind: Kind::Feature,
            summary: "记录时截下来源窗口",
            provides: vec![],
            requires: vec![CAPTURE_CONTEXT.id(), SCREENSHOTS.id()],
            optional_requires: vec![],
        }
    }

    fn start(&self, config: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        let config: Config = config.parse()?;
        let dir = config.dir.filter(|d| d.starts_with('/')).ok_or_else(|| {
            AppError::config("context-screenshot 的 dir 要是绝对路径（启动器通常会给默认值）")
        })?;
        let provider = Provider {
            dir,
            skip_apps: config.skip_apps,
            shots: host.resolve(&SCREENSHOTS)?,
        };
        host.contribute(&CAPTURE_CONTEXT, Arc::new(provider))
    }
}

register_component!(ContextScreenshot);

#[cfg(test)]
mod tests {
    use capture_contracts::WindowInfo;
    use capture_contracts::testing::{CaptureProbe, context_contract, snapshot};
    use screenshot_contract::testing::FakeScreenshots;

    use super::*;

    fn provider(shots: &FakeScreenshots, skip: &[&str]) -> Provider {
        Provider {
            dir: "/data/shots/".to_owned(),
            skip_apps: skip.iter().map(|s| (*s).to_owned()).collect(),
            shots: Arc::new(shots.clone()),
        }
    }

    fn window(app: &str) -> WindowInfo {
        WindowInfo {
            id: 7,
            app_id: Some(app.to_owned()),
            title: None,
            workspace: None,
            pid: None,
        }
    }

    #[tokio::test]
    async fn passes_the_context_contract() {
        let shots = FakeScreenshots::default();
        context_contract(Arc::new(provider(&shots, &[])), window("zen"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn shoots_the_source_window_named_after_the_item() {
        let shots = FakeScreenshots::default();
        let p = provider(&shots, &[]);
        let snap = snapshot(Some(window("zen")));
        let got = p.collect(snap.clone()).await.unwrap();
        let path = format!("/data/shots/{}.png", snap.item_id);
        assert_eq!(got[0].value, path);
        assert_eq!(got[0].kind, ContextKind::Image);
        assert_eq!(shots.taken().get(&path), Some(&7));

        // 取消：删掉
        p.discard(snap).await.unwrap();
        assert!(shots.taken().is_empty());
    }

    #[tokio::test]
    async fn skipped_apps_and_no_window() {
        let shots = FakeScreenshots::default();
        let p = provider(&shots, &["org.keepassxc.KeePassXC"]);
        let got = p
            .collect(snapshot(Some(window("org.keepassxc.KeePassXC"))))
            .await
            .unwrap();
        assert!(got.is_empty());
        assert!(p.collect(snapshot(None)).await.unwrap().is_empty());
        assert!(shots.taken().is_empty());
    }

    #[tokio::test]
    async fn a_failed_shot_is_an_error() {
        let shots = FakeScreenshots::with_windows(&[1]);
        let err = provider(&shots, &[])
            .collect(snapshot(Some(window("zen"))))
            .await
            .unwrap_err();
        assert_eq!(err.kind, kernel_api::ErrorKind::NotFound);
    }

    async fn boot(config: serde_json::Value) -> Result<(testkit::TestApp, CaptureProbe), AppError> {
        let probe = CaptureProbe::default();
        let app = testkit::app()
            .with(ContextScreenshot)
            .config("context-screenshot", config)?
            .with(FakeScreenshots::default())
            .with(probe.clone())
            .boot()
            .await?;
        Ok((app, probe))
    }

    #[tokio::test]
    async fn registers_itself_and_needs_an_absolute_dir() {
        let (app, probe) = boot(serde_json::json!({ "dir": "/d" })).await.unwrap();
        assert_eq!(probe.providers().len(), 1);
        app.stop().await;
        for config in [serde_json::json!({}), serde_json::json!({ "dir": "shots" })] {
            let err = boot(config).await.err().unwrap();
            assert_eq!(err.kind, kernel_api::ErrorKind::Config, "{err}");
            assert!(err.message.contains("dir"), "{err}");
        }
    }
}
