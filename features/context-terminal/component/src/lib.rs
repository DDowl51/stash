//! 应用功能 `context-terminal`：来源窗口是终端时，记下前台命令、工作目录、git 仓库和分支。
//!
//! 进程信息来自 `process` 契约，仓库信息来自 `vcs` 契约；这里只负责「终端」这层意思。

use std::sync::Arc;

use capture_contracts::{
    CAPTURE_CONTEXT, ContextEntry, ContextKind, ContextProvider, ContextSpec, Phase, SOURCE_KEY,
    Snapshot,
};
use context_terminal_core::{app_name, command_line, find, source_line, tilde};
use kernel_api::{
    AppError, Component, ConfigSection, Host, Kind, Manifest, async_trait, register_component,
};
use process_contract::{PROCESSES, ProcessInfo, Processes};
use serde::{Deserialize, Serialize};
use vcs_contract::{VCS, Vcs};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
struct Config {
    /// 哪些 app_id 算终端。
    apps: Vec<String>,
    /// 往下找几层子进程。
    max_depth: u32,
    /// 最多看多少个进程。
    max_processes: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            apps: [
                "com.mitchellh.ghostty",
                "kitty",
                "foot",
                "footclient",
                "Alacritty",
                "org.wezfurlong.wezterm",
            ]
            .map(str::to_owned)
            .to_vec(),
            max_depth: 6,
            max_processes: 300,
        }
    }
}

struct Provider {
    config: Config,
    processes: Arc<dyn Processes>,
    vcs: Arc<dyn Vcs>,
    home: Option<String>,
}

fn entry(key: &str, label: &str, kind: ContextKind, value: impl Into<String>) -> ContextEntry {
    ContextEntry::new(format!("terminal.{key}"), label, kind, value)
}

impl Provider {
    /// 终端进程往下的进程树（不含终端自己）。中途退出的进程跳过。
    async fn tree(&self, root: u32) -> Vec<ProcessInfo> {
        let mut out = Vec::new();
        let mut level = vec![root];
        for _ in 0..self.config.max_depth {
            let mut next = Vec::new();
            for pid in level {
                if let Ok(children) = self.processes.children(pid).await {
                    next.extend(children.iter().map(|c| c.pid));
                    out.extend(children);
                }
            }
            if next.is_empty() || out.len() >= self.config.max_processes {
                break;
            }
            level = next;
        }
        out.truncate(self.config.max_processes);
        out
    }
}

#[async_trait]
impl ContextProvider for Provider {
    async fn spec(&self) -> Result<ContextSpec, AppError> {
        Ok(ContextSpec {
            id: "terminal".to_owned(),
            phase: Phase::WhilePrompt,
        })
    }

    async fn collect(&self, snapshot: Snapshot) -> Result<Vec<ContextEntry>, AppError> {
        let Some(window) = snapshot.window else {
            return Ok(Vec::new());
        };
        let (Some(app), Some(pid)) = (window.app_id, window.pid) else {
            return Ok(Vec::new());
        };
        if !self.config.apps.contains(&app) {
            return Ok(Vec::new());
        }
        let tree = self.tree(pid).await;
        let Some(found) = find(pid, &tree) else {
            return Err(AppError::not_found(format!(
                "终端 {app}（pid {pid}）下面没找到 shell"
            )));
        };
        let mut out = Vec::new();
        if let Some(fg) = &found.foreground {
            let line = if fg.cmdline.is_empty() {
                fg.name.clone()
            } else {
                command_line(&fg.cmdline)
            };
            out.push(entry("command", "前台命令", ContextKind::Command, line));
        }
        out.push(entry(
            "shell",
            "shell",
            ContextKind::Text,
            found.shell.name.clone(),
        ));
        let Some(cwd) = found.cwd().map(str::to_owned) else {
            return Ok(out);
        };
        out.push(entry("cwd", "目录", ContextKind::Path, cwd.clone()));
        let shown = tilde(&cwd, self.home.as_deref());
        let repo = match self.vcs.repo_at(cwd.clone()).await {
            Ok(repo) => repo,
            Err(e) => {
                out.push(entry(
                    "repo",
                    "仓库",
                    ContextKind::Text,
                    format!("查不到：{}", e.message),
                ));
                None
            }
        };
        let mut branch = None;
        if let Some(repo) = repo {
            out.push(entry("repo", "仓库", ContextKind::Path, repo.root));
            if let Some(b) = repo.branch {
                out.push(entry("branch", "分支", ContextKind::Text, b.clone()));
                branch = Some(b);
            }
            if let Some(c) = repo.commit {
                out.push(entry("commit", "提交", ContextKind::Text, c));
            }
            if let Some(r) = repo.remote_url {
                out.push(entry("remote", "远端", ContextKind::Url, r));
            }
            if repo.dirty {
                out.push(entry("dirty", "改动", ContextKind::Text, "有没提交的改动"));
            }
        }
        out.push(ContextEntry::new(
            SOURCE_KEY,
            "来源",
            ContextKind::Text,
            source_line(app_name(&app), &shown, branch.as_deref()),
        ));
        Ok(out)
    }

    async fn discard(&self, _snapshot: Snapshot) -> Result<(), AppError> {
        Ok(())
    }
}

pub struct ContextTerminal;

impl Component for ContextTerminal {
    fn config_defaults(&self) -> Option<ConfigSection> {
        ConfigSection::defaults::<Config>()
    }

    fn manifest(&self) -> Manifest {
        Manifest {
            id: "context-terminal",
            kind: Kind::Feature,
            summary: "来源是终端时：前台命令、工作目录、git 仓库和分支",
            provides: vec![],
            requires: vec![CAPTURE_CONTEXT.id(), PROCESSES.id(), VCS.id()],
            optional_requires: vec![],
        }
    }

    fn start(&self, config: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        let config: Config = config.parse()?;
        let provider = Provider {
            processes: host.resolve(&PROCESSES)?,
            vcs: host.resolve(&VCS)?,
            // 只用来把家目录显示成 ~
            home: std::env::var("HOME").ok(),
            config,
        };
        host.contribute(&CAPTURE_CONTEXT, Arc::new(provider))
    }
}

register_component!(ContextTerminal);

#[cfg(test)]
mod tests;
