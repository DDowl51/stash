//! 应用功能 `expand-script`：输入以配置的前缀开头时（比如 `!gh 123`），交给外部命令改写。
//!
//! 命令通过 `exec` 契约运行；失败时 `capture` 按原文保存并记一条说明。

use std::sync::Arc;

use capture_contracts::{CAPTURE_EXPANDERS, Draft, Expander, ExpanderSpec};
use exec_contract::{CommandRequest, EXEC, Exec};
use expand_script_core::{matching, merge};
use kernel_api::{
    AppError, Component, ConfigSection, Host, Kind, Manifest, async_trait, register_component,
};
use serde::{Deserialize, Serialize};

fn default_timeout() -> u64 {
    1_500
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Rule {
    /// 输入以它开头时触发，比如 `"!gh "`。
    prefix: String,
    /// 程序和参数，不经过 shell。前缀后面的文字从 stdin 给。
    command: Vec<String>,
    #[serde(default = "default_timeout")]
    timeout_ms: u64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
struct Config {
    rules: Vec<Rule>,
}

struct Script {
    rules: Vec<Rule>,
    prefixes: Vec<String>,
    exec: Arc<dyn Exec>,
}

#[async_trait]
impl Expander for Script {
    async fn spec(&self) -> Result<ExpanderSpec, AppError> {
        Ok(ExpanderSpec {
            id: "script".to_owned(),
        })
    }

    async fn expand(&self, draft: Draft) -> Result<Draft, AppError> {
        let Some((i, input)) = matching(&draft.text, &self.prefixes) else {
            return Ok(draft);
        };
        let rule = &self.rules[i];
        let out = self
            .exec
            .run(CommandRequest {
                argv: rule.command.clone(),
                stdin: Some(input.to_owned()),
                env: vec![],
                timeout_ms: rule.timeout_ms,
            })
            .await?;
        if out.code != Some(0) {
            return Err(AppError::unavailable(format!(
                "`{}` 退出码 {}：{}",
                rule.command[0],
                out.code
                    .map_or_else(|| "（被信号终止）".to_owned(), |c| c.to_string()),
                out.stderr.trim()
            )));
        }
        merge(draft, &out.stdout).map_err(AppError::unavailable)
    }
}

pub struct ExpandScript;

impl Component for ExpandScript {
    fn config_defaults(&self) -> Option<ConfigSection> {
        ConfigSection::defaults::<Config>()
    }

    fn manifest(&self) -> Manifest {
        Manifest {
            id: "expand-script",
            kind: Kind::Feature,
            summary: "输入以配置的前缀开头时，交给外部命令改写",
            provides: vec![],
            requires: vec![CAPTURE_EXPANDERS.id(), EXEC.id()],
            optional_requires: vec![],
        }
    }

    fn start(&self, config: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        let config: Config = config.parse()?;
        for r in &config.rules {
            if r.prefix.trim().is_empty() || r.command.is_empty() {
                return Err(AppError::config(
                    "expand-script 的每条规则都要有 prefix 和 command",
                ));
            }
        }
        let script = Script {
            prefixes: config.rules.iter().map(|r| r.prefix.clone()).collect(),
            rules: config.rules,
            exec: host.resolve(&EXEC)?,
        };
        host.contribute(&CAPTURE_EXPANDERS, Arc::new(script))
    }
}

register_component!(ExpandScript);

#[cfg(test)]
mod tests {
    use capture_contracts::testing::{CaptureProbe, expander_contract};
    use exec_contract::CommandOutput;
    use exec_contract::testing::FakeExec;
    use serde_json::json;

    use super::*;

    fn script(exec: &FakeExec) -> Script {
        let rules = vec![Rule {
            prefix: "!gh ".to_owned(),
            command: vec!["stash-gh-issue".to_owned()],
            timeout_ms: 1_500,
        }];
        Script {
            prefixes: rules.iter().map(|r| r.prefix.clone()).collect(),
            rules,
            exec: Arc::new(exec.clone()),
        }
    }

    fn draft(text: &str) -> Draft {
        Draft {
            text: text.to_owned(),
            ..Draft::default()
        }
    }

    #[tokio::test]
    async fn passes_the_expander_contract() {
        let exec = FakeExec::default();
        expander_contract(Arc::new(script(&exec)), "没有前缀")
            .await
            .unwrap();
        assert!(exec.seen().is_empty(), "不匹配的不运行命令");
    }

    #[tokio::test]
    async fn rewrites_through_the_command() {
        let exec = FakeExec::new([FakeExec::ok("修 niri 的 bug (#123)\n")]);
        let got = script(&exec).expand(draft("!gh 123")).await.unwrap();
        assert_eq!(got.text, "修 niri 的 bug (#123)");
        let seen = exec.seen();
        assert_eq!(seen[0].stdin.as_deref(), Some("123"));
        assert_eq!(seen[0].argv, ["stash-gh-issue"]);
    }

    #[tokio::test]
    async fn failures_are_errors() {
        let exec = FakeExec::new([
            Ok(CommandOutput {
                code: Some(1),
                stdout: String::new(),
                stderr: "rate limited\n".to_owned(),
            }),
            FakeExec::ok(""),
            Err(AppError::temp_fail("超时")),
        ]);
        let s = script(&exec);
        let err = s.expand(draft("!gh 1")).await.unwrap_err();
        assert!(err.message.contains("rate limited"), "{err}");
        assert!(
            s.expand(draft("!gh 1"))
                .await
                .unwrap_err()
                .message
                .contains("没有输出")
        );
        assert_eq!(
            s.expand(draft("!gh 1")).await.unwrap_err().kind,
            kernel_api::ErrorKind::TempFail
        );
    }

    async fn boot(config: serde_json::Value) -> Result<CaptureProbe, AppError> {
        let probe = CaptureProbe::default();
        testkit::app()
            .with(ExpandScript)
            .config("expand-script", config)?
            .with(FakeExec::default())
            .with(probe.clone())
            .boot()
            .await?;
        Ok(probe)
    }

    #[tokio::test]
    async fn config_is_checked_at_startup() {
        let probe = boot(json!({ "rules": [{ "prefix": "!gh ", "command": ["x"] }] }))
            .await
            .unwrap();
        assert_eq!(probe.expanders().len(), 1);
        for bad in [
            json!({ "rules": [{ "prefix": " ", "command": ["x"] }] }),
            json!({ "rules": [{ "prefix": "!x", "command": [] }] }),
        ] {
            let err = boot(bad.clone()).await.err().unwrap();
            assert_eq!(err.kind, kernel_api::ErrorKind::Config, "{bad}");
        }
    }
}
