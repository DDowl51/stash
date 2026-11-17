//! 应用功能 `remind`：收件箱积压、条目到期时发桌面通知，同一个提醒一天只发一次。
//!
//! 登记 `stash remind [--dry-run]`，由 systemd timer 定时运行（`dist/systemd/`）。
//! 发过的提醒记在自己的表 `remind_sent` 里。规则在 `remind-core`。

use std::collections::HashSet;
use std::sync::Arc;

use cli_contract::{
    ArgKind, ArgSpec, CLI_COMMANDS, CommandProvider, CommandSpec, Format, Invocation, Output,
};
use clock_contract::{CLOCK, Clock};
use items_contracts::{ITEMS, ItemFilter, Items, Status};
use kernel_api::{
    AppError, Component, ConfigSection, Host, Kind, Manifest, async_trait, register_component,
};
use notify_contract::{NOTIFY, Notify, Urgency};
use remind_core::{Due, Reminder, Rules, days_before, plan, today};
use serde::{Deserialize, Serialize};
use sql_contract::{MIGRATIONS, SQL, Sql, Statement, StaticMigrations};

const MIGRATION: &[(&str, &str)] = &[(
    "已发的提醒",
    "
    CREATE TABLE remind_sent (
        key TEXT NOT NULL,
        sent_on TEXT NOT NULL,
        PRIMARY KEY (key, sent_on)
    ) STRICT;
    ",
)];

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
struct Config {
    /// 收件箱超过几条就提醒。
    inbox_threshold: u32,
    /// 提前几天提醒到期；0 = 当天。
    due_days_ahead: u32,
    /// 一条通知里最多列几个条目。
    max_listed: usize,
    /// 发过的提醒记录留多少天。
    keep_days: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            inbox_threshold: 5,
            due_days_ahead: 0,
            max_listed: 5,
            keep_days: 30,
        }
    }
}

struct Remind {
    config: Config,
    items: Arc<dyn Items>,
    notify: Arc<dyn Notify>,
    clock: Arc<dyn Clock>,
    sql: Arc<dyn Sql>,
}

fn urgency(u: Urgency) -> &'static str {
    match u {
        Urgency::Low => "low",
        Urgency::Normal => "normal",
        Urgency::Critical => "critical",
    }
}

impl Remind {
    async fn sent_on(&self, day: &str) -> Result<HashSet<String>, AppError> {
        let rows = self
            .sql
            .query(Statement::new("SELECT key FROM remind_sent WHERE sent_on = ?1").bind(day))
            .await?;
        Ok(rows
            .rows
            .iter()
            .filter_map(|r| r.first().and_then(|v| v.as_str()).map(str::to_owned))
            .collect())
    }

    /// 今天还要发的。
    async fn due_today(&self) -> Result<(String, Vec<Reminder>), AppError> {
        let day = today(self.clock.now().await?)
            .ok_or_else(|| AppError::unavailable("时钟给的时刻不对"))?;
        let counts = self.items.counts().await?;
        let due: Vec<Due> = self
            .items
            .list(ItemFilter {
                status: Some(Status::Open),
                ..ItemFilter::default()
            })
            .await?
            .into_iter()
            .filter_map(|it| {
                Some(Due {
                    due_on: it.due_on?,
                    id: it.id,
                    text: it.text,
                })
            })
            .collect();
        let rules = Rules {
            inbox_threshold: self.config.inbox_threshold,
            due_days_ahead: self.config.due_days_ahead,
            max_listed: self.config.max_listed,
        };
        let day_s = day.to_string();
        let sent = self.sent_on(&day_s).await?;
        let keep = days_before(day, self.config.keep_days)
            .unwrap_or(day)
            .to_string();
        // 顺手清掉太旧的记录
        self.sql
            .execute(vec![
                Statement::new("DELETE FROM remind_sent WHERE sent_on < ?1").bind(keep),
            ])
            .await?;
        Ok((day_s, plan(rules, counts.inbox, &due, day, &sent)))
    }

    /// 发出去，发成功的记下来。一条发不出去就停：通知服务不在，后面的也发不出去。
    async fn send(&self, day: &str, reminders: Vec<Reminder>) -> Result<(), AppError> {
        for r in reminders {
            self.notify.send(r.notification).await?;
            let stmts = r
                .keys
                .iter()
                .map(|k| {
                    Statement::new(
                        "INSERT OR IGNORE INTO remind_sent (key, sent_on) VALUES (?1, ?2)",
                    )
                    .bind(k.as_str())
                    .bind(day)
                })
                .collect();
            self.sql.execute(stmts).await?;
        }
        Ok(())
    }
}

fn render(reminders: &[Reminder], format: Format) -> Result<String, AppError> {
    let mut out = String::new();
    for r in reminders {
        match format {
            Format::Json => {
                let v = serde_json::json!({
                    "summary": r.notification.summary,
                    "body": r.notification.body,
                    "urgency": urgency(r.notification.urgency),
                    "keys": r.keys,
                });
                out.push_str(&v.to_string());
                out.push('\n');
            }
            Format::Text { .. } => {
                out.push_str(&format!("通知：{}\n", r.notification.summary));
                for line in r.notification.body.lines() {
                    out.push_str(&format!("  {line}\n"));
                }
            }
        }
    }
    Ok(out)
}

struct Commands(Arc<Remind>);

#[async_trait]
impl CommandProvider for Commands {
    async fn commands(&self) -> Result<Vec<CommandSpec>, AppError> {
        Ok(vec![CommandSpec {
            name: "remind".to_owned(),
            about: "收件箱积压、条目到期时发通知，同一个提醒一天一次（给 systemd timer 用）"
                .to_owned(),
            args: vec![ArgSpec {
                name: "dry-run".to_owned(),
                help: "只打印要发的通知，不发、不记".to_owned(),
                kind: ArgKind::Flag,
            }],
        }])
    }

    async fn run(&self, inv: Invocation) -> Result<Output, AppError> {
        if inv.command != "remind" {
            return Err(AppError::config(format!(
                "remind 没有登记命令 `{}`",
                inv.command
            )));
        }
        let (day, reminders) = self.0.due_today().await?;
        if inv.args.contains_key("dry-run") {
            return Ok(Output {
                text: render(&reminders, inv.format)?,
            });
        }
        self.0.send(&day, reminders).await?;
        // 没事不输出：发了什么看通知
        Ok(Output {
            text: String::new(),
        })
    }
}

pub struct RemindComponent;

impl Component for RemindComponent {
    fn config_defaults(&self) -> Option<ConfigSection> {
        ConfigSection::defaults::<Config>()
    }

    fn manifest(&self) -> Manifest {
        Manifest {
            id: "remind",
            kind: Kind::Feature,
            summary: "收件箱积压、条目到期时发通知",
            provides: vec![],
            requires: vec![
                ITEMS.id(),
                NOTIFY.id(),
                CLOCK.id(),
                SQL.id(),
                MIGRATIONS.id(),
                CLI_COMMANDS.id(),
            ],
            optional_requires: vec![],
        }
    }

    fn start(&self, config: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        let config: Config = config.parse()?;
        if config.max_listed == 0 {
            return Err(AppError::config("remind 的 max_listed 至少是 1"));
        }
        host.contribute(
            &MIGRATIONS,
            Arc::new(StaticMigrations::new("remind", MIGRATION)),
        )?;
        let remind = Arc::new(Remind {
            config,
            items: host.resolve(&ITEMS)?,
            notify: host.resolve(&NOTIFY)?,
            clock: host.resolve(&CLOCK)?,
            sql: host.resolve(&SQL)?,
        });
        host.contribute(&CLI_COMMANDS, Arc::new(Commands(remind)))
    }
}

register_component!(RemindComponent);
