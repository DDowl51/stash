//! 应用功能 `context-window`：把快照里的来源窗口翻译成上下文条目。
//!
//! 窗口信息 `capture` 已经在按键那一刻拍下了（快照），这里不再问窗口管理器，只做翻译。

use std::sync::Arc;

use capture_contracts::{
    CAPTURE_CONTEXT, ContextEntry, ContextKind, ContextProvider, ContextSpec, Phase, Snapshot,
};
use kernel_api::{
    AppError, Component, ConfigSection, Host, Kind, Manifest, NoConfig, async_trait,
    register_component,
};

struct Provider;

#[async_trait]
impl ContextProvider for Provider {
    async fn spec(&self) -> Result<ContextSpec, AppError> {
        Ok(ContextSpec {
            id: "window".to_owned(),
            phase: Phase::WhilePrompt,
        })
    }

    async fn collect(&self, snapshot: Snapshot) -> Result<Vec<ContextEntry>, AppError> {
        let Some(w) = snapshot.window else {
            return Ok(Vec::new());
        };
        let text = |key: &str, label: &str, value: String| {
            ContextEntry::new(format!("window.{key}"), label, ContextKind::Text, value)
        };
        let mut out = Vec::new();
        if let Some(app) = w.app_id.filter(|a| !a.is_empty()) {
            out.push(text("app", "应用", app));
        }
        if let Some(title) = w.title.filter(|t| !t.is_empty()) {
            out.push(text("title", "窗口标题", title));
        }
        if let Some(ws) = w.workspace.filter(|s| !s.is_empty()) {
            out.push(text("workspace", "工作区", ws));
        }
        if let Some(pid) = w.pid {
            out.push(text("pid", "进程", pid.to_string()));
        }
        Ok(out)
    }

    async fn discard(&self, _snapshot: Snapshot) -> Result<(), AppError> {
        Ok(())
    }
}

pub struct ContextWindow;

impl Component for ContextWindow {
    fn config_defaults(&self) -> Option<ConfigSection> {
        ConfigSection::defaults::<NoConfig>()
    }

    fn manifest(&self) -> Manifest {
        Manifest {
            id: "context-window",
            kind: Kind::Feature,
            summary: "记录时的来源窗口：应用、标题、工作区、pid",
            provides: vec![],
            requires: vec![CAPTURE_CONTEXT.id()],
            optional_requires: vec![],
        }
    }

    fn start(&self, config: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        let _: NoConfig = config.parse()?;
        host.contribute(&CAPTURE_CONTEXT, Arc::new(Provider))
    }
}

register_component!(ContextWindow);

#[cfg(test)]
mod tests {
    use capture_contracts::WindowInfo;
    use capture_contracts::testing::{context_contract, snapshot};

    use super::*;

    fn zen() -> WindowInfo {
        WindowInfo {
            id: 7,
            app_id: Some("zen".to_owned()),
            title: Some("PR #12 — Zen Browser".to_owned()),
            workspace: Some("web".to_owned()),
            pid: Some(4321),
        }
    }

    #[tokio::test]
    async fn passes_the_context_contract() {
        context_contract(Arc::new(Provider), zen()).await.unwrap();
    }

    #[tokio::test]
    async fn translates_the_window() {
        let got = Provider.collect(snapshot(Some(zen()))).await.unwrap();
        let pairs: Vec<(&str, &str)> = got
            .iter()
            .map(|e| (e.key.as_str(), e.value.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("window.app", "zen"),
                ("window.title", "PR #12 — Zen Browser"),
                ("window.workspace", "web"),
                ("window.pid", "4321"),
            ]
        );
        let bare = WindowInfo {
            id: 1,
            app_id: None,
            title: Some(String::new()),
            workspace: None,
            pid: None,
        };
        assert!(
            Provider
                .collect(snapshot(Some(bare)))
                .await
                .unwrap()
                .is_empty()
        );
        assert!(Provider.collect(snapshot(None)).await.unwrap().is_empty());
    }
}
