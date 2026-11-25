//! 用真实的 `stash` 进程测滴答接入：`#滴` 推送、`stash dida …` 命令、推送失败和重推。
//! 滴答是 `fake-dida`（dida 仓库里的假服务），token 直接放一个假服务认的。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use serde_json::Value;

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
    fake: fake_dida::Running,
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

impl World {
    async fn new() -> Self {
        let fake = fake_dida::start().await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config/stash/config.toml");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        let base = fake.base();
        std::fs::write(
            &config,
            format!(
                "[components.dida-openapi]\noauth_base = \"{base}\"\napi_base = \"{base}\"\nweb_base = \"{base}\"\n\
                 client_id = \"{}\"\nclient_secret = \"{}\"\n",
                fake_dida::CLIENT_ID,
                fake_dida::CLIENT_SECRET
            ),
        )
        .unwrap();
        // 和独立的 dida 共用 token：放在 $XDG_DATA_HOME/dida/token.json
        let token = dir.path().join("data/dida/token.json");
        std::fs::create_dir_all(token.parent().unwrap()).unwrap();
        let access = fake.fake.issue_token();
        std::fs::write(&token, format!("{{\"access_token\": \"{access}\"}}")).unwrap();
        Self { dir, fake }
    }

    async fn run(&self, args: &[&str]) -> Output {
        let mut c = Command::new(bin());
        c.args(args)
            .env("HOME", self.dir.path())
            .env("XDG_DATA_HOME", self.dir.path().join("data"))
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env_remove("STASH_CONFIG")
            .env_remove("NIRI_SOCKET");
        tokio::task::spawn_blocking(move || c.output().unwrap())
            .await
            .unwrap()
    }

    async fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args).await;
        assert!(
            out.status.success(),
            "stash {args:?}（{:?}）：{}",
            out.status.code(),
            text(&out.stderr)
        );
        text(&out.stdout)
    }

    async fn json(&self, args: &[&str]) -> Vec<Value> {
        self.ok(args)
            .await
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn remote_titles(&self) -> Vec<String> {
        self.fake
            .fake
            .tasks()
            .iter()
            .map(|t| t["title"].as_str().unwrap_or_default().to_owned())
            .collect()
    }
}

fn link(item: &Value) -> Option<String> {
    item["context"]
        .as_array()?
        .iter()
        .find(|c| c["key"] == "dida.task")
        .map(|c| c["value"].as_str().unwrap_or_default().to_owned())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_route_tag_saves_in_stash_and_pushes_to_dida() {
    let w = World::new().await;
    let added = w.json(&["add", "看 PR #滴 @11-27 #t"]).await;
    let id = added[0]["id"].as_str().unwrap().to_owned();
    assert_eq!(added[0]["text"], "看 PR", "#滴 从文字里去掉");
    assert_eq!(w.remote_titles(), ["看 PR"], "推到了滴答");
    let shown = w.json(&["show", &id]).await;
    let url = link(&shown[0]).expect("条目上有滴答链接");
    assert!(url.contains("/webapp/#p/"), "{url}");
    assert_eq!(shown[0]["kind"], "todo", "条目原样留在 stash");

    // --to 也一样；不带的不推
    w.ok(&["add", "第二条", "--to", "dida"]).await;
    w.ok(&["add", "只在 stash 里"]).await;
    assert_eq!(w.remote_titles(), ["看 PR", "第二条"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn capture_with_a_message_pushes_too() {
    let w = World::new().await;
    let out = w
        .run(&["capture", "--message", "这个报错之后再查 #滴"])
        .await;
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(w.remote_titles(), ["这个报错之后再查"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn when_dida_is_down_the_item_is_kept_and_can_be_retried() {
    let w = World::new().await;
    w.fake.fake.fail_next(&[503; 10]);
    let out = w.run(&["add", "断网时记的 #滴"]).await;
    assert!(
        out.status.success(),
        "存进 stash 不受影响：{}",
        text(&out.stderr)
    );
    assert!(
        text(&out.stderr).contains("--retry"),
        "stderr 说怎么补：{}",
        text(&out.stderr)
    );
    let id = serde_json::from_str::<Value>(text(&out.stdout).trim()).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let shown = w.json(&["show", &id]).await;
    assert!(
        shown[0]["context"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["key"] == "dida.failed"),
        "条目上记着失败原因"
    );
    assert!(w.remote_titles().is_empty());
    w.fake.fake.recover();
    w.ok(&["dida", "push", "--retry"]).await;
    assert_eq!(w.remote_titles(), ["断网时记的"]);
    assert!(link(&w.json(&["show", &id]).await[0]).is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn dida_commands_live_under_stash_dida() {
    let w = World::new().await;
    let help = w.ok(&["dida", "--help"]).await;
    for sub in ["ls", "add", "login", "push", "sync"] {
        assert!(help.contains(sub), "{help}");
    }
    w.ok(&["dida", "add", "直接建在滴答", "@今天"]).await;
    let today = w.json(&["dida", "ls"]).await;
    assert_eq!(today[0]["title"], "直接建在滴答");
    // 只在滴答里，stash 里没有
    assert!(w.json(&["ls"]).await.is_empty());
    let short = w.json(&["add", "x"]).await[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    w.ok(&["dida", "push", &short]).await;
    let again = w.run(&["dida", "push", &short]).await;
    assert_eq!(
        again.status.code(),
        Some(65),
        "推过的不再推：{}",
        text(&again.stderr)
    );
}
