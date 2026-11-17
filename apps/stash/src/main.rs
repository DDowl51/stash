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
    let running = boot::start(app, |_| {}).await?;
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
            ["cli-clap", "clock-system", "log-stderr"]
        );
    }
}
