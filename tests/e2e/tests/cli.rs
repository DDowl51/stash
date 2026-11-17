//! 用真实的进程跑 `stash`：输出格式、stdout / stderr 分开、退出码、管道。
//!
//! 每个测试一个临时目录，用 `XDG_DATA_HOME` / `XDG_CONFIG_HOME` / `HOME` 把它和用户的真实数据隔开。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;

use serde_json::Value;

/// 先把 stash 编译出来（只编一次），返回可执行文件的路径。
fn bin() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let status = Command::new(&cargo)
            .args(["build", "--quiet", "-p", "stash"])
            .current_dir(&root)
            .status()
            .expect("运行 cargo build");
        assert!(status.success());
        let out = Command::new(&cargo)
            .args(["metadata", "--format-version", "1", "--no-deps"])
            .current_dir(&root)
            .output()
            .expect("运行 cargo metadata");
        let meta: Value = serde_json::from_slice(&out.stdout).unwrap();
        PathBuf::from(meta["target_directory"].as_str().unwrap()).join("debug/stash")
    })
}

struct World {
    dir: tempfile::TempDir,
}

impl World {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(bin());
        c.env("HOME", self.dir.path())
            .env("XDG_DATA_HOME", self.dir.path().join("data"))
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env_remove("STASH_CONFIG")
            .env_remove("NO_COLOR")
            .args(args);
        c
    }

    /// 输出接在管道上（不是终端），所以默认是 JSON。
    fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().expect("运行 stash")
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "stash {args:?} 失败：{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty(), "成功时 stderr 应该是空的");
        String::from_utf8(out.stdout).unwrap()
    }

    fn json_lines(&self, args: &[&str]) -> Vec<Value> {
        self.ok(args)
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
}

#[test]
fn add_then_list_as_json_lines_in_the_default_data_dir() {
    let w = World::new();
    let added = w.json_lines(&["add", "回复", "老王", "--kind", "todo"]);
    assert_eq!(added.len(), 1);
    assert_eq!(added[0]["text"], "回复 老王");
    w.ok(&["add", "https://example.com/post"]);

    let all = w.json_lines(&["ls"]);
    assert_eq!(all.len(), 2);
    assert_eq!(all[0]["kind"], "inbox", "收件箱排在前面");
    assert_eq!(all[1]["kind"], "todo");

    assert!(
        w.dir.path().join("data/stash/stash.db").is_file(),
        "数据库默认在 $XDG_DATA_HOME/stash/stash.db"
    );
}

#[test]
fn changes_print_nothing_and_ids_can_be_suffixes() {
    let w = World::new();
    let id = w.json_lines(&["add", "x"])[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let short = &id[id.len() - 6..];
    assert_eq!(w.ok(&["move", short, "idea"]), "");
    assert_eq!(
        w.ok(&["done", &short.to_lowercase()]),
        "",
        "id 不区分大小写"
    );
    let done = w.json_lines(&["ls", "--status", "done"]);
    assert_eq!(done[0]["kind"], "idea");
    assert!(w.json_lines(&["ls"]).is_empty());
}

#[test]
fn errors_go_to_stderr_with_sysexits_codes() {
    let w = World::new();
    let out = w.run(&["done", "ZZZZZZ"]);
    assert_eq!(out.status.code(), Some(66), "找不到是 EX_NOINPUT");
    assert!(out.stdout.is_empty(), "出错时 stdout 什么都没有");
    assert!(!out.stderr.is_empty());

    let out = w.run(&["add", "x", "--due", "2026-02-30"]);
    assert_eq!(out.status.code(), Some(65), "数据不对是 EX_DATAERR");

    let out = w.run(&["no-such-command"]);
    assert_eq!(out.status.code(), Some(64), "用法错误是 EX_USAGE");
}

#[test]
fn json_output_feeds_another_process() {
    let w = World::new();
    w.ok(&["add", "一", "--kind", "todo"]);
    w.ok(&["add", "二", "--kind", "todo"]);
    // 第一个进程的输出原样喂给另一个程序（这里用 stash 自己的 JSON 解析代替 jq）
    let listed = w.ok(&["ls", "--kind", "todo"]);
    let mut cat = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    cat.stdin
        .take()
        .unwrap()
        .write_all(listed.as_bytes())
        .unwrap();
    let piped = String::from_utf8(cat.wait_with_output().unwrap().stdout).unwrap();
    let texts: Vec<String> = piped
        .lines()
        .map(|l| {
            serde_json::from_str::<Value>(l).unwrap()["text"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(texts, ["一", "二"]);
}

#[test]
fn text_output_when_asked() {
    let w = World::new();
    w.ok(&["add", "回复老王", "--kind", "todo", "--due", "2026-11-20"]);
    let text = w.ok(&["--text", "ls"]);
    assert!(text.contains("[待办]  @2026-11-20  回复老王"), "{text}");
}
