//! 应用功能 `context-script`：记录时运行配置里的外部命令，把输出记成上下文。
//!
//! 例子：调 `lifelog` 记下「当时前后在做什么」。命令通过 `exec` 契约运行，
//! 环境变量和输出格式见 `context-script-core`。

use std::sync::Arc;

use capture_contracts::{
    CAPTURE_CONTEXT, ContextEntry, ContextProvider, ContextSpec, Phase, Snapshot,
};
use context_script_core::{applies, check_name, env, failure, parse_output};
use exec_contract::{CommandRequest, EXEC, Exec};
use kernel_api::{
    AppError, Component, ConfigSection, Host, Kind, Manifest, async_trait, register_component,
};
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

fn default_timeout() -> u64 {
    800
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Script {
    /// 出现在上下文的 key 里：`script.<name>…`。
    name: String,
    /// 程序和参数，不经过 shell。
    command: Vec<String>,
    /// 只在来源窗口是这些应用时运行；空 = 总是运行。
    #[serde(default)]
    when_apps: Vec<String>,
    #[serde(default = "default_timeout")]
    timeout_ms: u64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
struct Config {
    scripts: Vec<Script>,
}

struct Provider {
    scripts: Vec<Script>,
    exec: Arc<dyn Exec>,
}

#[async_trait]
impl ContextProvider for Provider {
    async fn spec(&self) -> Result<ContextSpec, AppError> {
        Ok(ContextSpec {
            id: "script".to_owned(),
            phase: Phase::WhilePrompt,
        })
    }

    /// 符合条件的脚本一起跑；一个失败记一条说明，不影响别的。结果按配置的顺序。
    async fn collect(&self, snapshot: Snapshot) -> Result<Vec<ContextEntry>, AppError> {
        let app = snapshot.window.as_ref().and_then(|w| w.app_id.as_deref());
        let vars = env(&snapshot);
        let mut set = JoinSet::new();
        for (i, script) in self.scripts.iter().enumerate() {
            if !applies(&script.when_apps, app) {
                continue;
            }
            let (exec, script, vars) = (self.exec.clone(), script.clone(), vars.clone());
            set.spawn(async move {
                let result = exec
                    .run(CommandRequest {
                        argv: script.command.clone(),
                        stdin: None,
                        env: vars,
                        timeout_ms: script.timeout_ms,
                    })
                    .await;
                let entries = match result {
                    Ok(out) if out.code == Some(0) => parse_output(&script.name, &out.stdout),
                    Ok(out) => vec![failure(
                        &script.name,
                        &format!(
                            "退出码 {}：{}",
                            out.code
                                .map_or_else(|| "（被信号终止）".to_owned(), |c| c.to_string()),
                            out.stderr
                        ),
                    )],
                    Err(e) => vec![failure(&script.name, &e.message)],
                };
                (i, entries)
            });
        }
        let mut done: Vec<(usize, Vec<ContextEntry>)> = set.join_all().await;
        done.sort_by_key(|(i, _)| *i);
        Ok(done.into_iter().flat_map(|(_, e)| e).collect())
    }

    async fn discard(&self, _snapshot: Snapshot) -> Result<(), AppError> {
        Ok(())
    }
}

pub struct ContextScript;

impl Component for ContextScript {
    fn config_defaults(&self) -> Option<ConfigSection> {
        ConfigSection::defaults::<Config>()
    }

    fn manifest(&self) -> Manifest {
        Manifest {
            id: "context-script",
            kind: Kind::Feature,
            summary: "记录时运行配置里的外部命令，把输出记成上下文",
            provides: vec![],
            requires: vec![CAPTURE_CONTEXT.id(), EXEC.id()],
            optional_requires: vec![],
        }
    }

    fn start(&self, config: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        let config: Config = config.parse()?;
        for (i, s) in config.scripts.iter().enumerate() {
            check_name(&s.name)?;
            if s.command.is_empty() {
                return Err(AppError::config(format!(
                    "脚本 `{}` 的 command 是空的",
                    s.name
                )));
            }
            if config.scripts[..i].iter().any(|o| o.name == s.name) {
                return Err(AppError::config(format!("脚本名 `{}` 重复了", s.name)));
            }
        }
        let provider = Provider {
            scripts: config.scripts,
            exec: host.resolve(&EXEC)?,
        };
        host.contribute(&CAPTURE_CONTEXT, Arc::new(provider))
    }
}

register_component!(ContextScript);

#[cfg(test)]
mod tests {
    use capture_contracts::WindowInfo;
    use capture_contracts::testing::{CaptureProbe, context_contract, snapshot};
    use exec_contract::CommandOutput;
    use exec_contract::testing::FakeExec;
    use serde_json::json;

    use super::*;

    fn script(name: &str, when: &[&str]) -> Script {
        Script {
            name: name.to_owned(),
            command: vec![name.to_owned(), "--json".to_owned()],
            when_apps: when.iter().map(|s| (*s).to_owned()).collect(),
            timeout_ms: 800,
        }
    }

    fn zen() -> WindowInfo {
        WindowInfo {
            id: 7,
            app_id: Some("zen".to_owned()),
            title: Some("PR".to_owned()),
            workspace: None,
            pid: None,
        }
    }

    #[tokio::test]
    async fn passes_the_context_contract() {
        let exec = FakeExec::new([FakeExec::ok("hi"), FakeExec::ok(""), FakeExec::ok("")]);
        let p = Provider {
            scripts: vec![script("a", &[])],
            exec: Arc::new(exec),
        };
        context_contract(Arc::new(p), zen()).await.unwrap();
    }

    #[tokio::test]
    async fn runs_matching_scripts_with_the_environment() {
        let exec = FakeExec::new([
            FakeExec::ok("{\"key\":\"doing\",\"value\":\"写代码\"}"),
            Ok(CommandOutput {
                code: Some(2),
                stdout: String::new(),
                stderr: "no db\n".to_owned(),
            }),
        ]);
        let p = Provider {
            scripts: vec![
                script("lifelog", &[]),
                script("only-foot", &["foot"]),
                script("broken", &["zen"]),
            ],
            exec: Arc::new(exec.clone()),
        };
        let got = p.collect(snapshot(Some(zen()))).await.unwrap();
        let keys: Vec<(&str, &str)> = got
            .iter()
            .map(|e| (e.key.as_str(), e.value.as_str()))
            .collect();
        assert_eq!(
            keys,
            [
                ("script.lifelog.doing", "写代码"),
                ("script.broken", "退出码 2：no db")
            ]
        );
        let seen = exec.seen();
        assert_eq!(seen.len(), 2, "only-foot 不该跑");
        assert!(
            seen[0]
                .env
                .iter()
                .any(|(k, v)| k == "STASH_APP_ID" && v == "zen")
        );
        assert_eq!(seen[0].timeout_ms, 800);
    }

    #[tokio::test]
    async fn a_script_that_cannot_start_is_a_note() {
        let exec = FakeExec::new([Err(AppError::temp_fail("800 毫秒没跑完"))]);
        let p = Provider {
            scripts: vec![script("slow", &[])],
            exec: Arc::new(exec),
        };
        let got = p.collect(snapshot(None)).await.unwrap();
        assert_eq!(got[0].value, "800 毫秒没跑完");
    }

    async fn boot(config: serde_json::Value) -> Result<CaptureProbe, AppError> {
        let probe = CaptureProbe::default();
        testkit::app()
            .with(ContextScript)
            .config("context-script", config)?
            .with(FakeExec::default())
            .with(probe.clone())
            .boot()
            .await?;
        Ok(probe)
    }

    #[tokio::test]
    async fn config_is_checked_at_startup() {
        let probe = boot(json!({ "scripts": [{ "name": "a", "command": ["x"] }] }))
            .await
            .unwrap();
        assert_eq!(probe.providers().len(), 1);
        for bad in [
            json!({ "scripts": [{ "name": "A", "command": ["x"] }] }),
            json!({ "scripts": [{ "name": "a", "command": [] }] }),
            json!({ "scripts": [{ "name": "a", "command": ["x"] }, { "name": "a", "command": ["y"] }] }),
        ] {
            let err = boot(bad.clone()).await.err().unwrap();
            assert_eq!(err.kind, kernel_api::ErrorKind::Config, "{bad}");
        }
    }
}
