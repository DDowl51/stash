//! 应用功能 `dida-push`：把 stash 的条目推成滴答任务。
//!
//! 三个入口，做的是同一件事：
//! - `stash dida push <id> [--force]`、`stash dida push --retry`；
//! - 整理界面里按 `key`（`review.actions`）；
//! - 记录时带了路由（`#滴`、`--to dida`）：订阅 `capture.saved`。
//!
//! 推过去之后条目原样留着，只追加一条「滴答任务」链接；两边以后各管各的。
//! 推送失败时条目也照样留着，追加一条失败原因，记成待重推。

use std::collections::BTreeMap;
use std::sync::Arc;

use capture_contracts::{CAPTURE, CAPTURE_SAVED};
use cli_contract::{
    ArgKind, ArgSpec, CLI_COMMANDS, CommandProvider, CommandSpec, Invocation, Output,
};
use dida_push_core::{description, due, failed_entry, link_entry, project_for, title};
use dida_tasks_contracts::{NewTask, SyncMode, TASKS, Tasks};
use items_contracts::{ITEMS, Item, ItemPatch, Items};
use kernel_api::{
    AppError, Component, ConfigSection, Host, Kind, Manifest, async_trait, register_component,
};
use log_contract::{LOG, Logger};
use review_contracts::{
    ActionResult, ItemActionRequest, ItemActionSpec, ItemActions, REVIEW_ACTIONS,
};
use serde::{Deserialize, Serialize};
use sql_contract::{MIGRATIONS, SQL, Sql, SqlValue, Statement, StaticMigrations};

const MIGRATION: &[(&str, &str)] = &[(
    "推送记录",
    "
    CREATE TABLE dida_push_record (
        item_id TEXT PRIMARY KEY,
        status TEXT NOT NULL CHECK (status IN ('ok', 'failed')),
        task_id TEXT,
        url TEXT,
        error TEXT
    ) STRICT;
    ",
)];

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
struct Config {
    /// 推到哪个清单（名字，开头几个字也行）。空是收集箱。
    project: String,
    /// 分类 → 清单名，比如 `{ todo = "工作", read_later = "阅读" }`。没写的分类用 `project`。
    kind_projects: BTreeMap<String, String>,
    /// 整理界面里的按键。
    key: char,
    /// 记录时哪个路由表示「推到滴答」（`capture.routes` 里的路由名）。
    route: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            project: String::new(),
            kind_projects: BTreeMap::new(),
            key: 'P',
            route: "dida".to_owned(),
        }
    }
}

/// 一条条目推没推过。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Record {
    Ok { url: String },
    Failed,
}

struct Push {
    config: Config,
    items: Arc<dyn Items>,
    tasks: Arc<dyn Tasks>,
    sql: Arc<dyn Sql>,
    log: Logger,
}

impl Push {
    async fn record(&self, item_id: &str) -> Result<Option<Record>, AppError> {
        let rows = self
            .sql
            .query(
                Statement::new("SELECT status, url FROM dida_push_record WHERE item_id = ?1")
                    .bind(item_id),
            )
            .await?;
        Ok(rows.rows.first().map(|r| {
            match (
                r.first().and_then(SqlValue::as_str),
                r.get(1).and_then(SqlValue::as_str),
            ) {
                (Some("ok"), url) => Record::Ok {
                    url: url.unwrap_or_default().to_owned(),
                },
                _ => Record::Failed,
            }
        }))
    }

    async fn save_record(
        &self,
        item_id: &str,
        ok: Option<(&str, &str)>,
        error: Option<&str>,
    ) -> Result<(), AppError> {
        let (status, task_id, url) = match ok {
            Some((task, url)) => ("ok", Some(task), Some(url)),
            None => ("failed", None, None),
        };
        self.sql
            .execute(vec![
                Statement::new(
                    "INSERT INTO dida_push_record (item_id, status, task_id, url, error)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(item_id) DO UPDATE SET status = excluded.status,
                         task_id = excluded.task_id, url = excluded.url, error = excluded.error",
                )
                .bind(item_id)
                .bind(status)
                .bind(task_id)
                .bind(url)
                .bind(error),
            ])
            .await
            .map(drop)
    }

    async fn append(
        &self,
        item_id: &str,
        entry: items_contracts::ContextEntry,
    ) -> Result<Item, AppError> {
        self.items
            .update(
                item_id.to_owned(),
                ItemPatch {
                    append_context: vec![entry],
                    ..ItemPatch::default()
                },
            )
            .await
    }

    /// 真正去推。失败时条目上记一笔、记成待重推，再把错误传出去。
    async fn send(&self, item: &Item) -> Result<String, AppError> {
        let result = async {
            let project_id =
                match project_for(item, &self.config.kind_projects, &self.config.project) {
                    Some(name) => {
                        // 按名字找清单要用缓存
                        self.tasks.sync(SyncMode::IfStale).await?;
                        Some(self.tasks.project(name).await?.id)
                    }
                    None => None,
                };
            let task = self
                .tasks
                .create(NewTask {
                    project_id,
                    title: title(item),
                    content: description(item),
                    due: due(item),
                    ..NewTask::default()
                })
                .await?;
            let url = self.tasks.web_url(task.id.clone()).await?;
            Ok::<_, AppError>((task.id, url))
        }
        .await;
        match result {
            Ok((task_id, url)) => {
                self.save_record(&item.id, Some((&task_id, &url)), None)
                    .await?;
                self.append(&item.id, link_entry(&url)).await?;
                Ok(url)
            }
            Err(e) => {
                self.save_record(&item.id, None, Some(&e.message)).await?;
                if let Err(w) = self.append(&item.id, failed_entry(&e.message)).await {
                    self.log
                        .warn(format!("推送失败的原因没记到条目上：{w}"))
                        .await;
                }
                Err(e)
            }
        }
    }

    /// 推一条。已经推过的不再推（`force` 除外），返回 `Ok(None)`。
    async fn push(&self, item_id: &str, force: bool) -> Result<Option<String>, AppError> {
        let item = self.items.get(item_id.to_owned()).await?;
        if !force && matches!(self.record(&item.id).await?, Some(Record::Ok { .. })) {
            return Ok(None);
        }
        self.send(&item).await.map(Some)
    }

    /// 重推所有失败过的。返回（成功几条，失败的原因）。
    async fn retry(&self) -> Result<(usize, Vec<String>), AppError> {
        let rows = self
            .sql
            .query(Statement::new(
                "SELECT item_id FROM dida_push_record WHERE status = 'failed' ORDER BY item_id",
            ))
            .await?;
        let ids: Vec<String> = rows
            .rows
            .iter()
            .filter_map(|r| r.first().and_then(SqlValue::as_str).map(str::to_owned))
            .collect();
        let (mut ok, mut failed) = (0, Vec::new());
        for id in ids {
            match self.push(&id, true).await {
                Ok(_) => ok += 1,
                Err(e) => failed.push(format!("{id}：{}", e.message)),
            }
        }
        Ok((ok, failed))
    }
}

struct Commands(Arc<Push>);

#[async_trait]
impl CommandProvider for Commands {
    async fn commands(&self) -> Result<Vec<CommandSpec>, AppError> {
        let flag = |name: &str, help: &str| ArgSpec {
            name: name.to_owned(),
            help: help.to_owned(),
            kind: ArgKind::Flag,
        };
        Ok(vec![CommandSpec {
            name: "dida push".to_owned(),
            about: "把一条条目推成滴答任务；条目上留下链接".to_owned(),
            args: vec![
                ArgSpec {
                    name: "id".to_owned(),
                    help: "条目 id（最后几位就行）".to_owned(),
                    kind: ArgKind::Positional,
                },
                flag("force", "推过的也再推一次"),
                flag("retry", "重推所有失败过的"),
            ],
        }])
    }

    async fn run(&self, inv: Invocation) -> Result<Output, AppError> {
        if inv.command != "dida push" {
            return Err(AppError::config(format!(
                "dida-push 没有登记命令 `{}`",
                inv.command
            )));
        }
        let force = inv.args.contains_key("force");
        match (inv.args.get("id"), inv.args.contains_key("retry")) {
            (Some(_), true) => Err(AppError::validation("<id> 和 --retry 不能一起用")),
            (None, false) => Err(AppError::validation("要么给条目 id，要么 --retry")),
            (None, true) => {
                let (_, failed) = self.0.retry().await?;
                if failed.is_empty() {
                    Ok(Output {
                        text: String::new(),
                    })
                } else {
                    Err(AppError::unavailable(format!(
                        "{} 条还是没推过去：\n{}",
                        failed.len(),
                        failed.join("\n")
                    )))
                }
            }
            (Some(id), false) => match self.0.push(id, force).await? {
                Some(_) => Ok(Output {
                    text: String::new(),
                }),
                None => Err(AppError::validation("这条已经推过了；要再推一次加 --force")),
            },
        }
    }
}

struct Actions(Arc<Push>);

#[async_trait]
impl ItemActions for Actions {
    async fn actions(&self) -> Result<Vec<ItemActionSpec>, AppError> {
        Ok(vec![ItemActionSpec {
            key: self.0.config.key,
            label: "推到滴答".to_owned(),
            confirm: false,
            on_done_page: false,
        }])
    }

    async fn act(&self, request: ItemActionRequest) -> Result<ActionResult, AppError> {
        if request.key != self.0.config.key {
            return Err(AppError::validation(format!(
                "dida-push 没有按键 `{}`",
                request.key
            )));
        }
        let message = match self.0.push(&request.item_id, false).await? {
            Some(_) => "已推到滴答",
            None => "这条已经推过了",
        };
        Ok(ActionResult {
            message: Some(message.to_owned()),
            reload: true,
            ..ActionResult::default()
        })
    }
}

pub struct DidaPush;

impl Component for DidaPush {
    fn config_defaults(&self) -> Option<ConfigSection> {
        ConfigSection::defaults::<Config>()
    }

    fn manifest(&self) -> Manifest {
        Manifest {
            id: "dida-push",
            kind: Kind::Feature,
            summary: "把 stash 条目推成滴答任务",
            provides: vec![],
            requires: vec![ITEMS.id(), TASKS.id(), SQL.id(), MIGRATIONS.id(), LOG.id()],
            // 终端界面里没有 capture 和命令行，命令行里没有 review：有哪个用哪个
            optional_requires: vec![CAPTURE.id(), REVIEW_ACTIONS.id(), CLI_COMMANDS.id()],
        }
    }

    fn start(&self, config: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        let config: Config = config.parse()?;
        if review_contracts::RESERVED.contains(&config.key) {
            return Err(AppError::config(format!(
                "dida-push 的 key `{}` 是界面外壳自己用的，换一个",
                config.key
            )));
        }
        host.contribute(
            &MIGRATIONS,
            Arc::new(StaticMigrations::new("dida-push", MIGRATION)),
        )?;
        let push = Arc::new(Push {
            items: host.resolve(&ITEMS)?,
            tasks: host.resolve(&TASKS)?,
            sql: host.resolve(&SQL)?,
            log: Logger::new(host.resolve(&LOG)?, "dida-push"),
            config,
        });
        let on_saved = push.clone();
        host.subscribe(&CAPTURE_SAVED, "push-routed", move |saved| {
            let push = on_saved.clone();
            async move {
                if saved.routes.iter().any(|r| *r == push.config.route) {
                    // 失败已经记在条目上、记成待重推；这里只让 stderr 看得见
                    if let Err(e) = push.push(&saved.item_id, false).await {
                        push.log
                            .warn(format!(
                                "没推到滴答（之后可以 stash dida push --retry）：{}",
                                e.message
                            ))
                            .await;
                    }
                }
                Ok(())
            }
        })?;
        host.contribute(&REVIEW_ACTIONS, Arc::new(Actions(push.clone())))?;
        host.contribute(&CLI_COMMANDS, Arc::new(Commands(push)))
    }
}

register_component!(DidaPush);

#[cfg(test)]
mod tests;
