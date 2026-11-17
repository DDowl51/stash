//! 用 `Sql` 契约存条目。表和 SQL 都归这个组件，表名以 `items_` 开头。

use std::collections::HashMap;
use std::sync::Arc;

use items_contracts::{ContextEntry, ContextKind, Item, Kind, Status};
use kernel_api::AppError;
use sql_contract::{Rows, Sql, SqlValue, Statement};

/// 已经执行过的迁移不能改。改表结构在末尾追加。
pub(crate) const MIGRATIONS: &[(&str, &str)] = &[(
    "条目和上下文",
    "
    CREATE TABLE items_item (
        id TEXT PRIMARY KEY,
        text TEXT NOT NULL,
        url TEXT,
        kind TEXT NOT NULL CHECK (kind IN ('inbox', 'todo', 'idea', 'read_later')),
        status TEXT NOT NULL CHECK (status IN ('open', 'done', 'archived')),
        rank INTEGER NOT NULL,
        due_on TEXT,
        source TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        deleted_at INTEGER
    ) STRICT;
    CREATE INDEX items_item_kind_rank ON items_item(kind, status, rank);
    CREATE INDEX items_item_due ON items_item(due_on) WHERE due_on IS NOT NULL;
    CREATE TABLE items_context (
        item_id TEXT NOT NULL REFERENCES items_item(id),
        seq INTEGER NOT NULL,
        key TEXT NOT NULL,
        label TEXT NOT NULL,
        kind TEXT NOT NULL CHECK (kind IN ('text', 'url', 'path', 'image', 'command')),
        value TEXT NOT NULL,
        PRIMARY KEY (item_id, seq)
    ) STRICT;
    ",
)];

const COLUMNS: &str = "id, text, url, kind, status, rank, due_on, source, created_at, updated_at";

/// 没删掉的条目。
const LIVE: &str = "deleted_at IS NULL";

fn corrupt(what: &str) -> AppError {
    AppError::unavailable(format!("条目表里的数据不对：{what}"))
}

fn text(value: &SqlValue, column: &str) -> Result<String, AppError> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| corrupt(column))
}

fn opt_text(value: &SqlValue, column: &str) -> Result<Option<String>, AppError> {
    if value.is_null() {
        Ok(None)
    } else {
        text(value, column).map(Some)
    }
}

fn int(value: &SqlValue, column: &str) -> Result<i64, AppError> {
    value.as_i64().ok_or_else(|| corrupt(column))
}

fn item(row: &[SqlValue]) -> Result<Item, AppError> {
    let [
        id,
        body,
        url,
        kind,
        status,
        rank,
        due_on,
        source,
        created_at,
        updated_at,
    ] = row
    else {
        return Err(corrupt("列数"));
    };
    Ok(Item {
        id: text(id, "id")?,
        text: text(body, "text")?,
        url: opt_text(url, "url")?,
        kind: Kind::parse(&text(kind, "kind")?).ok_or_else(|| corrupt("kind"))?,
        status: Status::parse(&text(status, "status")?).ok_or_else(|| corrupt("status"))?,
        rank: int(rank, "rank")?,
        due_on: opt_text(due_on, "due_on")?,
        source: opt_text(source, "source")?,
        context: Vec::new(),
        created_at: int(created_at, "created_at")?,
        updated_at: int(updated_at, "updated_at")?,
    })
}

/// 列出条目的 SQL 条件，已经翻译成存储的说法。
#[derive(Debug)]
pub(crate) struct Where {
    pub kind: Option<Kind>,
    pub status: Status,
    /// 截止日期不晚于这一天（含）。
    pub due_until: Option<String>,
    pub limit: Option<u32>,
}

pub(crate) struct Repo {
    sql: Arc<dyn Sql>,
}

impl Repo {
    pub fn new(sql: Arc<dyn Sql>) -> Self {
        Self { sql }
    }

    /// 完整 id 或唯一后缀 → 完整 id。`query` 已经是大写。
    pub async fn resolve(&self, query: &str) -> Result<String, AppError> {
        let rows = self
            .sql
            .query(
                Statement::new(format!(
                    "SELECT id FROM items_item WHERE {LIVE} AND (id = ?1 OR id LIKE '%' || ?1) \
                     ORDER BY id LIMIT 2"
                ))
                .bind(query),
            )
            .await?;
        match rows.rows.as_slice() {
            [] => Err(AppError::not_found(format!("没有 id 是 `{query}` 的条目"))),
            [one] => text(&one[0], "id"),
            _ => Err(AppError::validation(format!(
                "`{query}` 对上了不止一个条目，多写几个字符"
            ))),
        }
    }

    /// 这个 id 用过没有（包括删掉的）。
    pub async fn exists(&self, id: &str) -> Result<bool, AppError> {
        let rows = self
            .sql
            .query(Statement::new("SELECT 1 FROM items_item WHERE id = ?1").bind(id))
            .await?;
        Ok(!rows.rows.is_empty())
    }

    pub async fn get(&self, id: &str) -> Result<Item, AppError> {
        let rows = self
            .sql
            .query(
                Statement::new(format!(
                    "SELECT {COLUMNS} FROM items_item WHERE {LIVE} AND id = ?1"
                ))
                .bind(id),
            )
            .await?;
        let mut items = self.with_context(rows).await?;
        items
            .pop()
            .ok_or_else(|| AppError::not_found(format!("没有 id 是 `{id}` 的条目")))
    }

    pub async fn list(&self, w: &Where) -> Result<Vec<Item>, AppError> {
        // 收件箱最新的在前，其余按手动排序；不限分类时按分类分组
        let sql = format!(
            "SELECT {COLUMNS} FROM items_item
             WHERE {LIVE} AND status = ?1
               AND (?2 IS NULL OR kind = ?2)
               AND (?3 IS NULL OR (due_on IS NOT NULL AND due_on <= ?3))
             ORDER BY CASE kind WHEN 'inbox' THEN 0 WHEN 'todo' THEN 1 WHEN 'idea' THEN 2 ELSE 3 END,
                      CASE WHEN kind = 'inbox' THEN -created_at ELSE rank END,
                      id
             LIMIT ?4"
        );
        let limit = w.limit.map_or(-1, i64::from);
        let rows = self
            .sql
            .query(
                Statement::new(sql)
                    .bind(w.status.as_str())
                    .bind(w.kind.map(Kind::as_str))
                    .bind(w.due_until.clone())
                    .bind(limit),
            )
            .await?;
        self.with_context(rows).await
    }

    /// 把条目行变成条目，并补上各自的上下文。
    async fn with_context(&self, rows: Rows) -> Result<Vec<Item>, AppError> {
        let mut items: Vec<Item> = rows
            .rows
            .iter()
            .map(|r| item(r))
            .collect::<Result<_, _>>()?;
        if items.is_empty() {
            return Ok(items);
        }
        let marks = vec!["?"; items.len()].join(", ");
        let mut stmt = Statement::new(format!(
            "SELECT item_id, key, label, kind, value FROM items_context
             WHERE item_id IN ({marks}) ORDER BY item_id, seq"
        ));
        for it in &items {
            stmt = stmt.bind(it.id.as_str());
        }
        let mut by_item: HashMap<String, Vec<ContextEntry>> = HashMap::new();
        for row in self.sql.query(stmt).await?.rows {
            let [item_id, key, label, kind, value] = row.as_slice() else {
                return Err(corrupt("上下文的列数"));
            };
            by_item
                .entry(text(item_id, "item_id")?)
                .or_default()
                .push(ContextEntry {
                    key: text(key, "key")?,
                    label: text(label, "label")?,
                    kind: ContextKind::parse(&text(kind, "context.kind")?)
                        .ok_or_else(|| corrupt("context.kind"))?,
                    value: text(value, "value")?,
                });
        }
        for it in &mut items {
            it.context = by_item.remove(&it.id).unwrap_or_default();
        }
        Ok(items)
    }

    /// 一个分类里未完成条目的排序值，从小到大，带 id。
    pub async fn ranks(&self, kind: Kind) -> Result<Vec<(String, i64)>, AppError> {
        let rows = self
            .sql
            .query(
                Statement::new(format!(
                    "SELECT id, rank FROM items_item
                     WHERE {LIVE} AND status = 'open' AND kind = ?1 ORDER BY rank, id"
                ))
                .bind(kind.as_str()),
            )
            .await?;
        rows.rows
            .iter()
            .map(|r| match r.as_slice() {
                [id, rank] => Ok((text(id, "id")?, int(rank, "rank")?)),
                _ => Err(corrupt("rank 的列数")),
            })
            .collect()
    }

    /// 一个分类里最大的排序值（含已完成的，免得恢复时撞上）。
    pub async fn max_rank(&self, kind: Kind) -> Result<Option<i64>, AppError> {
        let rows = self
            .sql
            .query(
                Statement::new(format!(
                    "SELECT max(rank) FROM items_item WHERE {LIVE} AND kind = ?1"
                ))
                .bind(kind.as_str()),
            )
            .await?;
        match rows.rows.first().and_then(|r| r.first()) {
            None | Some(SqlValue::Null) => Ok(None),
            Some(v) => int(v, "max(rank)").map(Some),
        }
    }

    pub async fn insert(&self, it: &Item) -> Result<(), AppError> {
        let mut stmts = vec![
            Statement::new(format!(
                "INSERT INTO items_item ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
            ))
            .bind(it.id.as_str())
            .bind(it.text.as_str())
            .bind(it.url.clone())
            .bind(it.kind.as_str())
            .bind(it.status.as_str())
            .bind(it.rank)
            .bind(it.due_on.clone())
            .bind(it.source.clone())
            .bind(it.created_at)
            .bind(it.updated_at),
        ];
        for (seq, c) in (0i64..).zip(&it.context) {
            stmts.push(
                Statement::new(
                    "INSERT INTO items_context (item_id, seq, key, label, kind, value)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )
                .bind(it.id.as_str())
                .bind(seq)
                .bind(c.key.as_str())
                .bind(c.label.as_str())
                .bind(c.kind.as_str())
                .bind(c.value.as_str()),
            );
        }
        self.sql.execute(stmts).await.map(drop)
    }

    /// 写回可改的字段。
    pub async fn save(&self, it: &Item) -> Result<(), AppError> {
        self.sql
            .execute(vec![
                Statement::new(
                    "UPDATE items_item SET text = ?2, kind = ?3, status = ?4, rank = ?5,
                         due_on = ?6, updated_at = ?7
                     WHERE id = ?1",
                )
                .bind(it.id.as_str())
                .bind(it.text.as_str())
                .bind(it.kind.as_str())
                .bind(it.status.as_str())
                .bind(it.rank)
                .bind(it.due_on.clone())
                .bind(it.updated_at),
            ])
            .await
            .map(drop)
    }

    /// 一次改好几个条目的排序值（重排一个分类时用），在一个事务里。
    pub async fn set_ranks(&self, ranks: &[(String, i64)], at: i64) -> Result<(), AppError> {
        let stmts = ranks
            .iter()
            .map(|(id, rank)| {
                Statement::new("UPDATE items_item SET rank = ?2, updated_at = ?3 WHERE id = ?1")
                    .bind(id.as_str())
                    .bind(*rank)
                    .bind(at)
            })
            .collect();
        self.sql.execute(stmts).await.map(drop)
    }

    pub async fn mark_deleted(&self, id: &str, at: i64) -> Result<(), AppError> {
        self.sql
            .execute(vec![
                Statement::new(
                    "UPDATE items_item SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1",
                )
                .bind(id)
                .bind(at),
            ])
            .await
            .map(drop)
    }

    /// 未完成条目按分类的数量，以及已过期、今天到期的数量。
    pub async fn counts(&self, today: &str) -> Result<(HashMap<String, u32>, u32, u32), AppError> {
        let rows = self
            .sql
            .query(Statement::new(format!(
                "SELECT kind, count(*) FROM items_item WHERE {LIVE} AND status = 'open' GROUP BY kind"
            )))
            .await?;
        let mut by_kind = HashMap::new();
        for row in rows.rows {
            let [kind, n] = row.as_slice() else {
                return Err(corrupt("计数的列数"));
            };
            by_kind.insert(
                text(kind, "kind")?,
                u32::try_from(int(n, "count")?).unwrap_or(u32::MAX),
            );
        }
        let due = self
            .sql
            .query(
                Statement::new(format!(
                    "SELECT coalesce(sum(due_on < ?1), 0), coalesce(sum(due_on = ?1), 0)
                     FROM items_item WHERE {LIVE} AND status = 'open' AND due_on IS NOT NULL"
                ))
                .bind(today),
            )
            .await?;
        let (overdue, today_n) = match due.rows.first().map(Vec::as_slice) {
            Some([a, b]) => (int(a, "overdue")?, int(b, "due_today")?),
            _ => return Err(corrupt("到期计数")),
        };
        Ok((
            by_kind,
            u32::try_from(overdue).unwrap_or(u32::MAX),
            u32::try_from(today_n).unwrap_or(u32::MAX),
        ))
    }
}
