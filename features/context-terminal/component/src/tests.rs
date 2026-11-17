//! 假的进程表和版本库，直接测提供者。

use std::collections::HashMap;

use capture_contracts::WindowInfo;
use capture_contracts::testing::{context_contract, snapshot};
use vcs_contract::RepoInfo;

use super::*;

#[derive(Default)]
struct FakeProcs(Vec<ProcessInfo>);

#[async_trait]
impl Processes for FakeProcs {
    async fn info(&self, pid: u32) -> Result<ProcessInfo, AppError> {
        self.0
            .iter()
            .find(|p| p.pid == pid)
            .cloned()
            .ok_or_else(|| AppError::not_found("没有"))
    }

    async fn children(&self, pid: u32) -> Result<Vec<ProcessInfo>, AppError> {
        Ok(self.0.iter().filter(|p| p.ppid == pid).cloned().collect())
    }
}

#[derive(Default)]
struct FakeVcs(HashMap<String, RepoInfo>, bool);

#[async_trait]
impl Vcs for FakeVcs {
    async fn repo_at(&self, path: String) -> Result<Option<RepoInfo>, AppError> {
        if self.1 {
            return Err(AppError::unavailable("git 坏了"));
        }
        Ok(self.0.get(&path).cloned())
    }
}

fn proc(
    pid: u32,
    ppid: u32,
    pgrp: u32,
    fg: u32,
    name: &str,
    argv: &[&str],
    cwd: &str,
) -> ProcessInfo {
    ProcessInfo {
        pid,
        ppid,
        pgrp,
        session: if pid == pgrp && ppid == 100 { pid } else { 200 },
        tty: Some(34816),
        foreground_pgrp: Some(fg),
        started: u64::from(pid),
        name: name.to_owned(),
        cmdline: argv.iter().map(|a| (*a).to_owned()).collect(),
        cwd: Some(cwd.to_owned()),
    }
}

/// ghostty（100）→ fish（200）→ nvim（300），nvim 在 ~/code/stash 里。
fn editing() -> Vec<ProcessInfo> {
    vec![
        proc(200, 100, 200, 300, "fish", &["/usr/bin/fish"], "/home/u"),
        proc(
            300,
            200,
            300,
            300,
            "nvim",
            &["nvim", "src/main.rs"],
            "/home/u/code/stash",
        ),
    ]
}

fn repo() -> RepoInfo {
    RepoInfo {
        root: "/home/u/code/stash".to_owned(),
        branch: Some("main".to_owned()),
        commit: Some("8097587".to_owned()),
        remote_url: Some("git@example.com:u/stash.git".to_owned()),
        dirty: true,
    }
}

fn provider(procs: Vec<ProcessInfo>, vcs: FakeVcs) -> Provider {
    Provider {
        config: Config::default(),
        processes: Arc::new(FakeProcs(procs)),
        vcs: Arc::new(vcs),
        home: Some("/home/u".to_owned()),
    }
}

fn ghostty() -> WindowInfo {
    WindowInfo {
        id: 1,
        app_id: Some("com.mitchellh.ghostty".to_owned()),
        title: Some("nvim".to_owned()),
        workspace: None,
        pid: Some(100),
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
    let vcs = FakeVcs([("/home/u/code/stash".to_owned(), repo())].into(), false);
    context_contract(Arc::new(provider(editing(), vcs)), ghostty())
        .await
        .unwrap();
}

#[tokio::test]
async fn command_directory_and_repository() {
    let vcs = FakeVcs([("/home/u/code/stash".to_owned(), repo())].into(), false);
    let got = provider(editing(), vcs)
        .collect(snapshot(Some(ghostty())))
        .await
        .unwrap();
    assert_eq!(
        pairs(&got),
        [
            ("terminal.command", "nvim src/main.rs"),
            ("terminal.shell", "fish"),
            ("terminal.cwd", "/home/u/code/stash"),
            ("terminal.repo", "/home/u/code/stash"),
            ("terminal.branch", "main"),
            ("terminal.commit", "8097587"),
            ("terminal.remote", "git@example.com:u/stash.git"),
            ("terminal.dirty", "有没提交的改动"),
            ("source", "ghostty · ~/code/stash (main)"),
        ]
    );
    assert_eq!(got[0].kind, ContextKind::Command);
    assert_eq!(got[2].kind, ContextKind::Path);
}

#[tokio::test]
async fn idle_shell_outside_a_repo() {
    let procs = vec![proc(200, 100, 200, 200, "fish", &["fish"], "/tmp")];
    let got = provider(procs, FakeVcs::default())
        .collect(snapshot(Some(ghostty())))
        .await
        .unwrap();
    assert_eq!(
        pairs(&got),
        [
            ("terminal.shell", "fish"),
            ("terminal.cwd", "/tmp"),
            ("source", "ghostty · /tmp"),
        ]
    );
}

#[tokio::test]
async fn a_broken_vcs_is_noted_but_the_rest_is_kept() {
    let got = provider(editing(), FakeVcs(HashMap::new(), true))
        .collect(snapshot(Some(ghostty())))
        .await
        .unwrap();
    assert!(
        got.iter()
            .any(|e| e.key == "terminal.repo" && e.value.contains("git 坏了")),
        "{got:?}"
    );
    assert!(got.iter().any(|e| e.key == "terminal.cwd"));
}

#[tokio::test]
async fn other_apps_and_missing_pids_are_skipped() {
    let p = provider(editing(), FakeVcs::default());
    let zen = WindowInfo {
        app_id: Some("zen".to_owned()),
        ..ghostty()
    };
    assert!(p.collect(snapshot(Some(zen))).await.unwrap().is_empty());
    let no_pid = WindowInfo {
        pid: None,
        ..ghostty()
    };
    assert!(p.collect(snapshot(Some(no_pid))).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_terminal_without_a_shell_is_an_error() {
    let err = provider(vec![], FakeVcs::default())
        .collect(snapshot(Some(ghostty())))
        .await
        .unwrap_err();
    assert_eq!(err.kind, kernel_api::ErrorKind::NotFound);
}
