//! `context-terminal` 的纯逻辑。
//!
//! 输入是终端窗口进程往下的一棵进程树（[`ProcessInfo`] 的列表），输出是「用户在看的那个 shell」
//! 和「它前台在跑的命令」。

use process_contract::ProcessInfo;

/// 找到的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub shell: ProcessInfo,
    /// 前台在跑的命令；shell 在等输入时是 `None`。
    pub foreground: Option<ProcessInfo>,
}

impl Found {
    /// 用户「在」的目录：前台命令的，没有就是 shell 的。
    pub fn cwd(&self) -> Option<&str> {
        self.foreground
            .as_ref()
            .and_then(|p| p.cwd.as_deref())
            .or(self.shell.cwd.as_deref())
    }
}

/// `p` 是不是 `root` 的后代（`tree` 里能顺着 ppid 走到 `root`）。
fn descends_from(p: &ProcessInfo, root: u32, tree: &[ProcessInfo]) -> bool {
    let mut cur = p.ppid;
    for _ in 0..tree.len() {
        if cur == root {
            return true;
        }
        match tree.iter().find(|q| q.pid == cur) {
            Some(parent) => cur = parent.ppid,
            None => return false,
        }
    }
    false
}

/// 在 `terminal` 的后代 `tree` 里找 shell 和前台命令。
///
/// - shell：有控制终端的会话首进程（终端模拟器给每个标签页 / 分屏开一个会话）。
///   有好几个时取最晚启动的那个：拿不到「当前是哪个标签页」，最近开的最可能是在用的。
///   （启发式，桌面机上核对。）
/// - 前台命令：shell 所在终端的前台进程组不是 shell 自己的组时，这个组的组长；
///   组长不在了就取组里最早启动的。
pub fn find(terminal: u32, tree: &[ProcessInfo]) -> Option<Found> {
    let shell = tree
        .iter()
        .filter(|p| p.tty.is_some() && p.session == p.pid && descends_from(p, terminal, tree))
        .max_by_key(|p| (p.started, p.pid))?
        .clone();
    let foreground = match shell.foreground_pgrp {
        Some(fg) if fg != shell.pgrp => {
            let group: Vec<&ProcessInfo> = tree
                .iter()
                .filter(|p| p.pgrp == fg && p.session == shell.session)
                .collect();
            group
                .iter()
                .find(|p| p.pid == fg)
                .or_else(|| group.iter().min_by_key(|p| (p.started, p.pid)))
                .map(|p| (*p).clone())
        }
        _ => None,
    };
    Some(Found { shell, foreground })
}

/// 命令行拼成一行给人看：带空白或引号的参数用单引号括起来。
pub fn command_line(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            if a.is_empty() || a.contains(|c: char| c.is_whitespace() || "'\"$`\\".contains(c)) {
                format!("'{}'", a.replace('\'', r"'\''"))
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// 家目录写成 `~`。
pub fn tilde(path: &str, home: Option<&str>) -> String {
    match home.filter(|h| !h.is_empty() && *h != "/") {
        Some(home) if path == home => "~".to_owned(),
        Some(home) => match path.strip_prefix(home) {
            Some(rest) if rest.starts_with('/') => format!("~{rest}"),
            _ => path.to_owned(),
        },
        None => path.to_owned(),
    }
}

/// `com.mitchellh.ghostty` → `ghostty`；`foot` 不变。
pub fn app_name(app_id: &str) -> &str {
    if app_id.matches('.').count() >= 2 {
        app_id.rsplit('.').next().unwrap_or(app_id)
    } else {
        app_id
    }
}

/// 来源摘要：`ghostty · ~/code/stash (main)`。
pub fn source_line(app: &str, cwd: &str, branch: Option<&str>) -> String {
    match branch {
        Some(b) => format!("{app} · {cwd} ({b})"),
        None => format!("{app} · {cwd}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// pid、ppid、会话、进程组、终端上的前台组（`None` = 没有终端）、启动先后、名字、目录。
    #[allow(clippy::too_many_arguments)]
    fn p(
        pid: u32,
        ppid: u32,
        session: u32,
        pgrp: u32,
        fg: Option<u32>,
        started: u64,
        name: &str,
        cwd: &str,
    ) -> ProcessInfo {
        ProcessInfo {
            pid,
            ppid,
            pgrp,
            session,
            tty: fg.map(|_| 34816),
            foreground_pgrp: fg,
            started,
            name: name.to_owned(),
            cmdline: vec![name.to_owned()],
            cwd: Some(cwd.to_owned()),
        }
    }

    const TERM: u32 = 100;

    #[test]
    fn idle_shell() {
        let tree = [p(
            200,
            TERM,
            200,
            200,
            Some(200),
            10,
            "fish",
            "/home/u/code",
        )];
        let got = find(TERM, &tree).unwrap();
        assert_eq!(got.shell.pid, 200);
        assert_eq!(got.foreground, None);
        assert_eq!(got.cwd(), Some("/home/u/code"));
    }

    #[test]
    fn foreground_command_and_its_cwd() {
        // fish 里跑 nvim（自己一个进程组），nvim 又起了语言服务器（同组）
        let tree = [
            p(200, TERM, 200, 200, Some(300), 10, "fish", "/home/u"),
            p(
                300,
                200,
                200,
                300,
                Some(300),
                20,
                "nvim",
                "/home/u/code/stash",
            ),
            p(
                301,
                300,
                200,
                300,
                Some(300),
                21,
                "rust-analyzer",
                "/home/u/code/stash",
            ),
        ];
        let got = find(TERM, &tree).unwrap();
        assert_eq!(got.foreground.as_ref().map(|p| p.pid), Some(300));
        assert_eq!(got.cwd(), Some("/home/u/code/stash"));
    }

    #[test]
    fn pipeline_whose_leader_exited() {
        let tree = [
            p(200, TERM, 200, 200, Some(400), 10, "bash", "/w"),
            p(402, 200, 200, 400, Some(400), 31, "less", "/w"),
            p(401, 200, 200, 400, Some(400), 30, "grep", "/w"),
        ];
        assert_eq!(find(TERM, &tree).unwrap().foreground.unwrap().pid, 401);
    }

    #[test]
    fn the_newest_of_several_tabs_and_nested_shells() {
        // 终端 → 两个标签页；tmux 之类的 server 也在树里但没有终端
        let tree = [
            p(200, TERM, 200, 200, Some(200), 10, "fish", "/old"),
            p(210, TERM, 210, 210, Some(210), 50, "fish", "/new"),
            p(220, TERM, 220, 220, None, 60, "daemon", "/"),
        ];
        assert_eq!(find(TERM, &tree).unwrap().shell.pid, 210);
    }

    #[test]
    fn nothing_that_looks_like_a_shell() {
        assert_eq!(find(TERM, &[]), None);
        // 别的终端下的 shell 不算
        let tree = [p(500, 999, 500, 500, Some(500), 1, "zsh", "/")];
        assert_eq!(find(TERM, &tree), None);
    }

    #[test]
    fn command_lines() {
        let argv = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            command_line(&argv(&["nvim", "src/main.rs"])),
            "nvim src/main.rs"
        );
        assert_eq!(
            command_line(&argv(&["git", "commit", "-m", "it's done"])),
            r"git commit -m 'it'\''s done'"
        );
        assert_eq!(command_line(&argv(&["x", ""])), "x ''");
    }

    #[test]
    fn tildes_and_source_lines() {
        let home = Some("/home/u");
        assert_eq!(tilde("/home/u/code", home), "~/code");
        assert_eq!(tilde("/home/u", home), "~");
        assert_eq!(tilde("/home/user2", home), "/home/user2");
        assert_eq!(tilde("/etc", None), "/etc");
        assert_eq!(
            source_line("ghostty", "~/code", Some("main")),
            "ghostty · ~/code (main)"
        );
        assert_eq!(source_line("foot", "/tmp", None), "foot · /tmp");
    }
}
