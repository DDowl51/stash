//! 启动器 `stash-tui`：终端界面，整理和规划记下的条目。
//!
//! 很薄：启动 → 把终端交给界面服务 → 退出。有哪些页面由装进来的组件决定。

use std::process::ExitCode;

use boot::App;
use cli_contract::{EXIT_USAGE, exit_code};
use kernel_api::AppError;
use tui_contract::TUI;

include!(concat!(env!("OUT_DIR"), "/components.rs"));

const NAME: &str = "stash-tui";

const USAGE: &str = "\
用法：stash-tui [--config <文件>]

在终端里整理记下的条目：1 收件箱、2 待办、3 想法、4 稍后读、5 已完成。
j/k 选择，最下面一行是当前页的按键，q 退出。

  --config <文件>  用另一个配置文件（默认 ~/.config/stash/config.toml）
  --trace          把组件之间的每次请求和事件按行写到 stderr（会弄乱界面，配合 2>文件 使用）
";

async fn run(app: &App) -> Result<u8, AppError> {
    let running = boot::start(app, |settings| {
        // 和 stash 命令行用同一个库
        settings.set_default(
            "storage-sqlite",
            "path",
            serde_json::Value::String(
                boot::data_dir("stash")
                    .join("stash.db")
                    .to_string_lossy()
                    .into_owned(),
            ),
        );
    })
    .await?;
    let code = match running.resolve(&TUI) {
        Ok(tui) => tui.run().await,
        Err(e) => Err(e),
    };
    running.stop().await;
    code
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flags = match boot::launcher_flags(&args) {
        Ok(flags) => flags,
        Err(e) => {
            boot::report(NAME, &e);
            return ExitCode::from(EXIT_USAGE);
        }
    };
    match flags.rest.as_slice() {
        [] => {}
        [help] if help == "--help" || help == "-h" => {
            boot::say(USAGE);
            return ExitCode::SUCCESS;
        }
        [other, ..] => {
            let e = AppError::validation(format!("不认识的参数 {other:?}。用 --help 看用法"));
            boot::report(NAME, &e);
            return ExitCode::from(EXIT_USAGE);
        }
    }

    let app = App {
        name: NAME,
        config_dir: "stash",
        known_components: KNOWN_COMPONENTS,
        config: flags.config,
        trace: flags.trace,
        export: &[],
        // 没有常驻进程：自己读写数据库
        connect: false,
    };
    match boot::block_on(run(&app)) {
        Ok(Ok(code)) => ExitCode::from(code),
        Ok(Err(e)) | Err(e) => {
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
                "clock-system",
                "items",
                "log-stderr",
                "review",
                "storage-sqlite",
                "tui-ratatui"
            ]
        );
    }
}
