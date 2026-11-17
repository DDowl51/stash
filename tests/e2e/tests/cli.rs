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

/// 测试一个一个跑：有的测试写假脚本再让 stash 去执行，别的测试同时 fork 出的子进程会带着
/// 写句柄，exec 时报「Text file busy」。
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct World {
    dir: tempfile::TempDir,
    _serial: std::sync::MutexGuard<'static, ()>,
}

impl World {
    fn new() -> Self {
        Self {
            // 别的测试失败（锁被污染）不影响这个
            _serial: SERIAL
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
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
            // 测试可能就跑在 niri 会话里：默认当作不在，要 niri 的测试自己设
            .env_remove("NIRI_SOCKET")
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

    /// 写配置文件。
    fn config(&self, toml: &str) {
        let dir = self.dir.path().join("config/stash");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), toml).unwrap();
    }

    /// 一个假的 vicinae：执行 `body`。返回它的路径。
    /// 一个假程序（shell 脚本）：执行 `body`。返回它的路径。
    fn fake(&self, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = self.dir.path().join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn fake_vicinae(&self, body: &str) -> PathBuf {
        self.fake("vicinae", body)
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
    w.ok(&["add", "看看", "niri", "新版"]);

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

#[test]
fn add_understands_tags_and_dates() {
    let w = World::new();
    let it = &w.json_lines(&["add", "交周报", "@2026-11-20", "#t"])[0];
    assert_eq!(it["text"], "交周报");
    assert_eq!(it["kind"], "todo");
    assert_eq!(it["due_on"], "2026-11-20");
    let it = &w.json_lines(&["add", "https://example.com/post"])[0];
    assert_eq!(it["kind"], "read_later");
    assert_eq!(it["url"], "https://example.com/post");

    let parsed = &w.json_lines(&["parse", "学", "#C++", "#i"])[0];
    assert_eq!(parsed["text"], "学 #C++");
    assert_eq!(parsed["kind"], "idea");
    assert_eq!(w.json_lines(&["ls"]).len(), 2, "parse 不保存");

    let out = w.run(&["add", "#t"]);
    assert_eq!(out.status.code(), Some(65), "只有标记：数据错误");
}

#[test]
fn capture_through_a_fake_vicinae_without_niri() {
    let w = World::new();
    let vicinae = w
        .fake_vicinae(r#"cat > /dev/null; printf '%s\n' "$@" > "$0.args"; echo '  回复老王 #t  '"#);
    w.config(&format!(
        "[components.prompt-vicinae]\ncommand = \"{}\"\n",
        vicinae.display()
    ));
    let it = &w.json_lines(&["capture", "--kind", "idea"])[0];
    assert_eq!(it["text"], "回复老王");
    assert_eq!(it["kind"], "todo", "# 优先于 --kind");
    assert!(it.get("source").is_none(), "没有 niri：没有来源");
    let args = std::fs::read_to_string(w.dir.path().join("vicinae.args")).unwrap();
    assert!(args.starts_with("dmenu\n--placeholder\n→ 想法"), "{args}");

    // Esc：不保存，什么都不输出，退出码 0
    w.fake_vicinae("exit 1");
    assert_eq!(w.ok(&["capture"]), "");
    assert_eq!(w.json_lines(&["ls"]).len(), 1);

    // vicinae 坏了：69
    w.fake_vicinae("echo boom >&2; exit 5");
    let out = w.run(&["capture"]);
    assert_eq!(out.status.code(), Some(69));
    assert!(String::from_utf8_lossy(&out.stderr).contains("boom"));
}

/// 假的 niri：事件流的连接接住不说话；一次性请求按名字回答。
fn fake_niri(socket: &Path) {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixListener;
    let listener = UnixListener::bind(socket).unwrap();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for conn in listener.incoming() {
            let Ok(conn) = conn else { return };
            let mut reader = BufReader::new(conn);
            let mut request = String::new();
            if reader.read_line(&mut request).is_err() {
                continue;
            }
            let mut conn = reader.into_inner();
            let reply = match request.trim() {
                "\"FocusedWindow\"" => {
                    r#"{"Ok":{"FocusedWindow":{"id":9,"title":"PR #12 — Zen Browser","app_id":"zen","pid":4321,"workspace_id":3,"is_focused":true}}}"#
                }
                "\"Workspaces\"" => r#"{"Ok":{"Workspaces":[{"id":3,"idx":1,"name":"web"}]}}"#,
                _ => {
                    held.push(conn);
                    continue;
                }
            };
            let _ = writeln!(conn, "{reply}");
        }
    });
}

#[test]
fn capture_records_the_window_it_came_from() {
    let w = World::new();
    let socket = w.dir.path().join("niri.sock");
    fake_niri(&socket);
    let vicinae = w.fake_vicinae("cat > /dev/null; echo '看下这个 PR'");
    // 截图用的 niri 命令行、剪贴板工具也是假的：剪贴板是空的，截图写一个 PNG
    let niri = w.fake(
        "niri",
        r#"case "$*" in
  "msg -j windows") echo '[{"id":9}]' ;;
  "msg action screenshot-window"*)
    while [ $# -gt 0 ]; do [ "$1" = --path ] && p="$2"; shift; done
    printf '\211PNG\r\n\032\nfake' > "$p" ;;
esac"#,
    );
    let wl_paste = w.fake("wl-paste", "exit 1");
    let wl_copy = w.fake("wl-copy", "cat > /dev/null");
    w.config(&format!(
        "[components.prompt-vicinae]\ncommand = \"{}\"\n\
         [components.screenshot-niri]\nniri = \"{}\"\nwl_paste = \"{}\"\nwl_copy = \"{}\"\n",
        vicinae.display(),
        niri.display(),
        wl_paste.display(),
        wl_copy.display()
    ));
    let out = w
        .cmd(&["capture"])
        .env("NIRI_SOCKET", &socket)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let it: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(it["source"], "zen · PR #12 — Zen Browser");
    let id = it["id"].as_str().unwrap();
    let shot = w.dir.path().join(format!("data/stash/shots/{id}.png"));
    let context: Vec<(String, String)> = it["context"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["key"].as_str().unwrap().to_owned(),
                c["value"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let pairs: Vec<(&str, &str)> = context
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(
        pairs[..6],
        [
            ("window.app", "zen"),
            ("window.title", "PR #12 — Zen Browser"),
            ("window.workspace", "web"),
            ("window.pid", "4321"),
            // 这个环境里没有 zen 的历史：只记标题并说明
            ("browser.title", "PR #12"),
            (
                "browser.note",
                "最近 30 分钟的 zen 历史里没找到这个标题，只记了标题"
            ),
        ]
    );
    assert_eq!(pairs[6], ("screenshot.window", shot.to_str().unwrap()));
    assert!(std::fs::read(&shot).unwrap().starts_with(b"\x89PNG"));
    // show 里看得到
    let shown = w.ok(&["--text", "show", &id[id.len() - 6..]]);
    assert!(
        shown.contains("来源  zen · PR #12") && shown.contains("工作区  web"),
        "{shown}"
    );

    // Esc：刚截的图也删掉
    w.fake_vicinae("cat > /dev/null; exit 1");
    let out = w
        .cmd(&["capture"])
        .env("NIRI_SOCKET", &socket)
        .output()
        .unwrap();
    assert!(out.status.success() && out.stdout.is_empty());
    let shots: Vec<_> = std::fs::read_dir(shot.parent().unwrap()).unwrap().collect();
    assert_eq!(shots.len(), 1, "只剩第一次的截图");
}

#[test]
fn remind_dry_run_and_a_missing_notification_service() {
    let w = World::new();
    w.ok(&["add", "交周报", "@今天", "#t"]);
    let out = w.ok(&["--text", "remind", "--dry-run"]);
    assert!(out.starts_with("通知：今天：交周报\n"), "{out}");

    // 私有总线上没有通知服务：失败，退出码 69，错误在 stderr
    let mut daemon = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut address = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(daemon.stdout.take().unwrap()),
        &mut address,
    )
    .unwrap();
    w.config(&format!(
        "[components.notify-dbus]\nbus = \"{}\"\n",
        address.trim()
    ));
    let out = w.run(&["remind"]);
    let _ = daemon.kill();
    let _ = daemon.wait();
    assert_eq!(
        out.status.code(),
        Some(69),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("发通知失败"));
    // 没发出去就没记：dry-run 里还在
    assert!(w.ok(&["--text", "remind", "--dry-run"]).contains("交周报"));
}
