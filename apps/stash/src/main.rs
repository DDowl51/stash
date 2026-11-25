//! 启动器 `stash`：命令行。
//!
//! 很薄：启动 → 把命令行交给命令行服务 → 用它给的退出码退出。
//! 有哪些子命令由装进来的组件决定，这里一个都不认识。

use std::process::ExitCode;

use boot::App;
use cli_contract::{CLI, EXIT_USAGE, exit_code};
use kernel_api::AppError;

include!(concat!(env!("OUT_DIR"), "/components.rs"));

const NAME: &str = "stash";

async fn run(app: &App, argv: Vec<String>) -> Result<u8, AppError> {
    let running = boot::start(app, |settings| {
        let data = boot::data_dir("stash");
        let path =
            |p: std::path::PathBuf| serde_json::Value::String(p.to_string_lossy().into_owned());
        // 系统服务不认识 stash：放在哪由启动器告诉它们（配置文件里写了就以配置为准）
        settings.set_default("storage-sqlite", "path", path(data.join("stash.db")));
        settings.set_default("context-screenshot", "dir", path(data.join("shots")));
        settings.set_default(
            "notify-dbus",
            "app_name",
            serde_json::Value::String("stash".to_owned()),
        );
        // 滴答：命令挂在 `stash dida …` 下；token 和独立的 dida 共用一份，登录一次就行；
        // 缓存放进 stash.db（一个进程只有一个库）。`#滴` 表示存好后推到滴答
        let dida = boot::data_dir("dida");
        let group = serde_json::Value::String("dida".to_owned());
        settings.set_default("dida-openapi", "group", group.clone());
        settings.set_default("dida-commands", "group", group);
        settings.set_default("dida-openapi", "token_path", path(dida.join("token.json")));
        settings.set_default(
            "capture",
            "routes",
            serde_json::json!({ "dida": ["滴", "dida"] }),
        );
        // 不在 niri 会话里（ssh、纯终端）时窗口和截图服务都用不了；stash 照样要能用，
        // 所以没有 NIRI_SOCKET 时默认停用它们，以及依赖截图的上下文
        if std::env::var_os("NIRI_SOCKET").is_none_or(|s| s.is_empty()) {
            for id in ["wm-niri", "screenshot-niri", "context-screenshot"] {
                settings.set_default(id, "enabled", serde_json::Value::Bool(false));
            }
        }
    })
    .await?;
    let code = match running.resolve(&CLI) {
        Ok(cli) => cli.run(argv).await,
        Err(e) => Err(e),
    };
    running.stop().await;
    code
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let flags = match boot::launcher_flags(argv.get(1..).unwrap_or_default()) {
        Ok(flags) => flags,
        Err(e) => {
            boot::report(NAME, &e);
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let app = App {
        name: NAME,
        config_dir: "stash",
        known_components: KNOWN_COMPONENTS,
        config: flags.config,
        trace: flags.trace,
        export: &[],
        // 没有常驻进程：每个 stash 进程自己读写数据库
        connect: false,
    };
    match boot::block_on(run(&app, argv)).and_then(|outcome| outcome) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            boot::report(NAME, &e);
            ExitCode::from(exit_code(e.kind))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 依赖里写了的组件都真的链接进来并登记上了。
    #[test]
    fn every_linked_component_is_registered() {
        let registered: Vec<&str> = kernel_api::registered_components()
            .iter()
            .map(|c| c.manifest().id)
            .collect();
        assert_eq!(registered, LINKED_COMPONENTS);
        assert_eq!(
            LINKED_COMPONENTS,
            [
                "browser-chromium",
                "browser-firefox",
                "browser-history",
                "browser-qutebrowser",
                "capture",
                "cli-clap",
                "clock-system",
                "context-browser",
                "context-screenshot",
                "context-script",
                "context-terminal",
                "context-window",
                "dida-commands",
                "dida-openapi",
                "dida-push",
                "dida-tasks",
                "exec-tokio",
                "expand-calc",
                "expand-script",
                "expand-snippets",
                "items",
                "log-stderr",
                "notify-dbus",
                "process-procfs",
                "prompt-vicinae",
                "remind",
                "screenshot-niri",
                "storage-sqlite",
                "vcs-git",
                "wm-niri"
            ]
        );
    }
}
