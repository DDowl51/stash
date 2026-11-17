//! 应用功能 `items`：条目的存储、读写契约和 `stash add / ls / show / …` 命令。
//!
//! 这个包是适配层和组件定义：用 `Sql` 契约实现存储、用 `Clock` 契约取时间。
//! 规则（校验、排序值、到期区间）在 `items-core` 里。

mod commands;
mod repo;

use std::sync::Arc;

use cli_contract::CLI_COMMANDS;
use clock_contract::{CLOCK, Clock};
use items_contracts::{
    Changed, Counts, DueChange, ITEMS, ITEMS_CHANGED, Item, ItemFilter, ItemPatch, Items, Kind,
    NewItem, Status,
};
use items_core::{
    check_date, clean_text, due_range, id_query, local_date, rank_between, spaced_ranks,
};
use kernel_api::{
    AppError, Component, ConfigSection, Host, Kind as ComponentKind, Manifest, NoConfig, Publisher,
    async_trait, register_component,
};
use log_contract::{LOG, Logger};
use sql_contract::{MIGRATIONS, SQL, StaticMigrations};

use crate::repo::{Repo, Where};

struct Service {
    repo: Repo,
    clock: Arc<dyn Clock>,
    changed: Publisher<Changed>,
    log: Logger,
}

/// 新的 ULID：时间部分来自时钟（测试里可控），随机部分来自系统。
fn new_id(utc_ms: i64) -> Result<String, AppError> {
    let mut random = [0u8; 16];
    getrandom::fill(&mut random)
        .map_err(|e| AppError::unavailable(format!("取不到随机数：{e}")))?;
    let ms = u64::try_from(utc_ms).map_err(|_| AppError::validation("时钟早于 1970 年"))?;
    Ok(ulid::Ulid::from_parts(ms, u128::from_le_bytes(random)).to_string())
}

impl Service {
    async fn now(&self) -> Result<(i64, String), AppError> {
        let now = self.clock.now().await?;
        let today = local_date(now.utc_ms, now.offset_min)?;
        Ok((now.utc_ms, today.to_string()))
    }

    async fn notify(&self, ids: Vec<String>) {
        if let Err(e) = self.changed.publish(Changed { ids }).await {
            // 已经写进去了，通知发不出去不算失败
            self.log.warn(format!("条目变更的通知没发出去：{e}")).await;
        }
    }

    /// 排到 `kind` 的最后。
    async fn last_rank(&self, kind: Kind) -> Result<i64, AppError> {
        let max = self.repo.max_rank(kind).await?;
        rank_between(max, None).ok_or_else(|| AppError::unavailable("排序值用完了"))
    }
}

#[async_trait]
impl Items for Service {
    async fn add(&self, new: NewItem) -> Result<Item, AppError> {
        let text = clean_text(&new.text)?;
        let due_on = new.due_on.as_deref().map(check_date).transpose()?;
        let kind = new.kind.unwrap_or(Kind::Inbox);
        let (now, _) = self.now().await?;
        let it = Item {
            id: new_id(now)?,
            text,
            url: new.url,
            kind,
            status: Status::Open,
            rank: self.last_rank(kind).await?,
            due_on,
            source: new.source,
            context: new.context,
            created_at: now,
            updated_at: now,
        };
        self.repo.insert(&it).await?;
        self.notify(vec![it.id.clone()]).await;
        Ok(it)
    }

    async fn get(&self, id: String) -> Result<Item, AppError> {
        let id = self.repo.resolve(&id_query(&id)?).await?;
        self.repo.get(&id).await
    }

    async fn update(&self, id: String, patch: ItemPatch) -> Result<Item, AppError> {
        let mut it = self.get(id).await?;
        if let Some(text) = patch.text {
            it.text = clean_text(&text)?;
        }
        match patch.due {
            Some(DueChange::Set(date)) => it.due_on = Some(check_date(&date)?),
            Some(DueChange::Clear) => it.due_on = None,
            None => {}
        }
        if let Some(kind) = patch.kind
            && kind != it.kind
        {
            it.rank = self.last_rank(kind).await?;
            it.kind = kind;
        }
        if let Some(status) = patch.status {
            it.status = status;
        }
        it.updated_at = self.now().await?.0;
        self.repo.save(&it).await?;
        self.notify(vec![it.id.clone()]).await;
        Ok(it)
    }

    async fn reorder(&self, id: String, before: Option<String>) -> Result<(), AppError> {
        let mut it = self.get(id).await?;
        let target = match before {
            Some(b) => {
                let b = self.get(b).await?;
                if b.kind != it.kind {
                    return Err(AppError::validation("只能在同一个分类里挪位置"));
                }
                Some(b.id)
            }
            None => None,
        };
        let now = self.now().await?.0;
        let mut others: Vec<(String, i64)> = self
            .repo
            .ranks(it.kind)
            .await?
            .into_iter()
            .filter(|(i, _)| *i != it.id)
            .collect();
        let slot = target.as_ref().map_or(others.len(), |t| {
            others
                .iter()
                .position(|(i, _)| i == t)
                .unwrap_or(others.len())
        });
        let prev = slot
            .checked_sub(1)
            .and_then(|i| others.get(i))
            .map(|(_, r)| *r);
        let next = others.get(slot).map(|(_, r)| *r);
        let mut changed = vec![it.id.clone()];
        it.rank = match rank_between(prev, next) {
            Some(rank) => rank,
            None => {
                // 两边挨着了：整个分类重新拉开间隔，再放进去
                others.insert(slot, (it.id.clone(), 0));
                let spaced: Vec<(String, i64)> = others
                    .iter()
                    .zip(spaced_ranks(others.len()))
                    .map(|((i, _), r)| (i.clone(), r))
                    .collect();
                self.repo.set_ranks(&spaced, now).await?;
                changed = spaced.iter().map(|(i, _)| i.clone()).collect();
                self.notify(changed).await;
                return Ok(());
            }
        };
        it.updated_at = now;
        self.repo.save(&it).await?;
        self.notify(changed).await;
        Ok(())
    }

    async fn delete(&self, id: String) -> Result<(), AppError> {
        let it = self.get(id).await?;
        self.repo.mark_deleted(&it.id, self.now().await?.0).await?;
        self.notify(vec![it.id]).await;
        Ok(())
    }

    async fn list(&self, filter: ItemFilter) -> Result<Vec<Item>, AppError> {
        let due_until = match filter.due {
            Some(due) => {
                let now = self.clock.now().await?;
                Some(due_range(due, local_date(now.utc_ms, now.offset_min)?)?.1)
            }
            None => None,
        };
        self.repo
            .list(&Where {
                kind: filter.kind,
                status: filter.status.unwrap_or(Status::Open),
                due_until,
                limit: filter.limit,
            })
            .await
    }

    async fn counts(&self) -> Result<Counts, AppError> {
        let (_, today) = self.now().await?;
        let (by_kind, overdue, due_today) = self.repo.counts(&today).await?;
        let n = |k: Kind| by_kind.get(k.as_str()).copied().unwrap_or(0);
        Ok(Counts {
            inbox: n(Kind::Inbox),
            todo: n(Kind::Todo),
            idea: n(Kind::Idea),
            read_later: n(Kind::ReadLater),
            overdue,
            due_today,
        })
    }
}

pub struct ItemsComponent;

impl Component for ItemsComponent {
    fn config_defaults(&self) -> Option<ConfigSection> {
        ConfigSection::defaults::<NoConfig>()
    }

    fn manifest(&self) -> Manifest {
        Manifest {
            id: "items",
            kind: ComponentKind::Feature,
            summary: "条目的存储和读写",
            provides: vec![ITEMS.id()],
            requires: vec![
                SQL.id(),
                MIGRATIONS.id(),
                CLOCK.id(),
                LOG.id(),
                CLI_COMMANDS.id(),
            ],
            optional_requires: vec![],
        }
    }

    fn start(&self, config: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        let _: NoConfig = config.parse()?;
        host.contribute(
            &MIGRATIONS,
            Arc::new(StaticMigrations::new("items", repo::MIGRATIONS)),
        )?;
        let service = Arc::new(Service {
            repo: Repo::new(host.resolve(&SQL)?),
            clock: host.resolve(&CLOCK)?,
            changed: host.publisher(&ITEMS_CHANGED)?,
            log: Logger::new(host.resolve(&LOG)?, host.id()),
        });
        host.contribute(&CLI_COMMANDS, Arc::new(commands::Commands(service.clone())))?;
        host.provide(&ITEMS, service)
    }
}

register_component!(ItemsComponent);
