//! `items` 对外的契约。
//!
//! 「条目」是 stash 记下的一件事：要做的事、一个念头、一个想读的链接。
//! 记录方（`capture`）、整理方（`review`）、提醒方（`remind`）都通过 [`Items`] 读写，
//! 以后的同步通过 [`ITEMS_CHANGED`] 知道哪些条目变了。

use kernel_api::{AppError, EventDef, contract};
use serde::{Deserialize, Serialize};

/// 分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// 还没分类，等整理。
    Inbox,
    Todo,
    Idea,
    ReadLater,
}

impl Kind {
    pub const ALL: [Self; 4] = [Self::Inbox, Self::Todo, Self::Idea, Self::ReadLater];

    /// 存储和命令行里用的名字。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Todo => "todo",
            Self::Idea => "idea",
            Self::ReadLater => "read_later",
        }
    }

    /// 认 `as_str` 的写法，以及命令行里更顺手的 `read`。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "inbox" => Some(Self::Inbox),
            "todo" => Some(Self::Todo),
            "idea" => Some(Self::Idea),
            "read_later" | "read" => Some(Self::ReadLater),
            _ => None,
        }
    }
}

/// 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    Done,
    Archived,
}

impl Status {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Done => "done",
            Self::Archived => "archived",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "open" => Some(Self::Open),
            "done" => Some(Self::Done),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

/// 上下文的值是什么，决定界面上能不能打开它。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextKind {
    Text,
    Url,
    /// 本机的文件或目录。
    Path,
    /// 本机的图片文件（截图）。
    Image,
    /// 一条命令行。
    Command,
}

impl ContextKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Url => "url",
            Self::Path => "path",
            Self::Image => "image",
            Self::Command => "command",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "text" => Some(Self::Text),
            "url" => Some(Self::Url),
            "path" => Some(Self::Path),
            "image" => Some(Self::Image),
            "command" => Some(Self::Command),
            _ => None,
        }
    }
}

/// 记录时的一条上下文。`items` 不解释它的内容，原样存取。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextEntry {
    /// 机器看的键，例如 `terminal.cwd`。
    pub key: String,
    /// 人看的名字，例如「目录」。
    pub label: String,
    pub kind: ContextKind,
    pub value: String,
}

impl ContextEntry {
    pub fn new(key: &str, label: &str, kind: ContextKind, value: impl Into<String>) -> Self {
        Self {
            key: key.to_owned(),
            label: label.to_owned(),
            kind,
            value: value.into(),
        }
    }
}

/// 一个条目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// ULID。
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub kind: Kind,
    pub status: Status,
    /// 同一分类里的手动排序，小的在前。只有相对大小有意义。
    pub rank: i64,
    /// 截止日期 `YYYY-MM-DD`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_on: Option<String>,
    /// 来源的一行摘要，比如「微信 · 张三」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<ContextEntry>,
    /// UTC 毫秒。
    pub created_at: i64,
    pub updated_at: i64,
}

/// 新条目。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NewItem {
    pub text: String,
    #[serde(default)]
    pub url: Option<String>,
    /// 不写进收件箱。
    #[serde(default)]
    pub kind: Option<Kind>,
    #[serde(default)]
    pub due_on: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub context: Vec<ContextEntry>,
}

impl NewItem {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}

/// 截止日期怎么改。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DueChange {
    /// `YYYY-MM-DD`。
    Set(String),
    Clear,
}

/// 改一个条目：写了的字段才改。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ItemPatch {
    #[serde(default)]
    pub text: Option<String>,
    /// 换分类时排到新分类的最后。
    #[serde(default)]
    pub kind: Option<Kind>,
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default)]
    pub due: Option<DueChange>,
}

/// 按截止日期筛。日期以「今天」为准，今天由 `items` 按时钟算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DueFilter {
    /// 已经过了截止日期。
    Overdue,
    /// 今天到期，含过期的。
    Today,
    /// 七天内到期，含过期的。
    Week,
}

/// 列出条目的条件。都不写时是所有未完成的条目。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ItemFilter {
    #[serde(default)]
    pub kind: Option<Kind>,
    /// 不写时只要 `open`。
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default)]
    pub due: Option<DueFilter>,
    /// 最多几条；不写不限。
    #[serde(default)]
    pub limit: Option<u32>,
}

/// 各分类里未完成的条目数，以及到期情况。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Counts {
    pub inbox: u32,
    pub todo: u32,
    pub idea: u32,
    pub read_later: u32,
    /// 未完成且已过期的。
    pub overdue: u32,
    /// 未完成且今天到期的（不含已过期的）。
    pub due_today: u32,
}

contract! {
    /// 条目的读写。
    pub trait Items as ITEMS = service("items", 1) {
        /// 记下一个条目，排在它所在分类的最后。文字为空、日期写错是 `validation`。
        async fn add(&self, new: NewItem) -> Result<Item, AppError>;

        /// 按 id 取一个条目。`id` 可以是完整的 ULID，也可以是唯一的后缀（至少 4 个字符）；
        /// 找不到是 `not_found`，后缀对上不止一个是 `validation`。
        async fn get(&self, id: String) -> Result<Item, AppError>;

        /// 改一个条目，返回改后的样子。`id` 的写法同 [`Items::get`]。
        async fn update(&self, id: String, patch: ItemPatch) -> Result<Item, AppError>;

        /// 在同一分类里挪位置：排到 `before` 前面；`before` 为空时排到最后。
        /// 两个条目不在同一分类是 `validation`。
        async fn reorder(&self, id: String, before: Option<String>) -> Result<(), AppError>;

        /// 删除一个条目（以后的同步要靠墓碑，所以存储里只做标记）。
        async fn delete(&self, id: String) -> Result<(), AppError>;

        /// 按条件列出。顺序：收件箱最新的在前；其余分类按手动排序。
        async fn list(&self, filter: ItemFilter) -> Result<Vec<Item>, AppError>;

        /// 各分类的数量和到期情况。
        async fn counts(&self) -> Result<Counts, AppError>;
    }
}

/// 有条目被新建、修改或删除了。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changed {
    pub ids: Vec<String>,
}

/// 事件：条目变了。写入成功之后发；发不出去不影响写入。
pub static ITEMS_CHANGED: EventDef<Changed> = EventDef::new("items", "changed");

#[cfg(feature = "testing")]
pub mod testing {
    //! 契约测试：`Items` 的任何实现都要通过。

    use std::sync::Arc;

    use kernel_api::{AppError, ErrorKind};

    use super::{
        ContextEntry, ContextKind, DueChange, ItemFilter, ItemPatch, Items, Kind, NewItem, Status,
    };

    fn check(ok: bool, what: &str) -> Result<(), AppError> {
        if ok {
            Ok(())
        } else {
            Err(AppError::validation(format!("契约不满足：{what}")))
        }
    }

    async fn kind_of(items: &Arc<dyn Items>, kind: Kind) -> Result<Vec<String>, AppError> {
        Ok(items
            .list(ItemFilter {
                kind: Some(kind),
                ..ItemFilter::default()
            })
            .await?
            .into_iter()
            .map(|i| i.text)
            .collect())
    }

    /// `today` 是实现眼中的今天（`YYYY-MM-DD`），用来检查到期筛选。
    pub async fn items_contract(items: Arc<dyn Items>, today: &str) -> Result<(), AppError> {
        // 新建：默认进收件箱、未完成；上下文原样存取
        let ctx = vec![
            ContextEntry::new("window.app", "应用", ContextKind::Text, "ghostty"),
            ContextEntry::new("terminal.cwd", "目录", ContextKind::Path, "/tmp"),
        ];
        let a = items
            .add(NewItem {
                context: ctx.clone(),
                source: Some("ghostty · ~".to_owned()),
                ..NewItem::text("回复老王")
            })
            .await?;
        check(a.kind == Kind::Inbox, "不写分类进收件箱")?;
        check(a.status == Status::Open, "新条目未完成")?;
        check(a.id.len() == 26, "id 是 26 位 ULID")?;
        let got = items.get(a.id.clone()).await?;
        check(got.context == ctx, "上下文原样取回，顺序不变")?;
        check(
            got.source.as_deref() == Some("ghostty · ~"),
            "来源摘要原样取回",
        )?;

        // 后缀也能找到
        let suffix = a.id[a.id.len() - 6..].to_owned();
        check(items.get(suffix).await?.id == a.id, "唯一后缀能找到条目")?;
        let missing = items.get("ZZZZZZZZZZZZZZZZZZZZZZZZZZ".to_owned()).await;
        check(
            matches!(missing, Err(ref e) if e.kind == ErrorKind::NotFound),
            "找不到是 not_found",
        )?;
        let short = items.get("AB".to_owned()).await;
        check(
            matches!(short, Err(ref e) if e.kind == ErrorKind::Validation),
            "后缀太短是 validation",
        )?;

        // 校验
        let empty = items.add(NewItem::text("   ")).await;
        check(
            matches!(empty, Err(ref e) if e.kind == ErrorKind::Validation),
            "空文字是 validation",
        )?;
        let bad_date = items
            .add(NewItem {
                due_on: Some("2026-02-30".to_owned()),
                ..NewItem::text("x")
            })
            .await;
        check(
            matches!(bad_date, Err(ref e) if e.kind == ErrorKind::Validation),
            "不存在的日期是 validation",
        )?;

        // 排序：新的排在分类最后；挪到某一条前面；挪到最后
        for text in ["一", "二", "三"] {
            items
                .add(NewItem {
                    kind: Some(Kind::Todo),
                    ..NewItem::text(text)
                })
                .await?;
        }
        check(
            kind_of(&items, Kind::Todo).await? == ["一", "二", "三"],
            "新条目排在最后",
        )?;
        let todos = items
            .list(ItemFilter {
                kind: Some(Kind::Todo),
                ..ItemFilter::default()
            })
            .await?;
        items
            .reorder(todos[2].id.clone(), Some(todos[0].id.clone()))
            .await?;
        check(
            kind_of(&items, Kind::Todo).await? == ["三", "一", "二"],
            "挪到某条前面",
        )?;
        items.reorder(todos[2].id.clone(), None).await?;
        check(
            kind_of(&items, Kind::Todo).await? == ["一", "二", "三"],
            "挪到最后",
        )?;
        // 反复挪到同一个位置前面，间隔用完后要能自己重排
        for _ in 0..40 {
            items
                .reorder(todos[1].id.clone(), Some(todos[2].id.clone()))
                .await?;
            items
                .reorder(todos[0].id.clone(), Some(todos[2].id.clone()))
                .await?;
        }
        check(
            kind_of(&items, Kind::Todo).await? == ["二", "一", "三"],
            "间隔用完时重排，顺序不乱",
        )?;
        let cross = items.reorder(a.id.clone(), Some(todos[0].id.clone())).await;
        check(
            matches!(cross, Err(ref e) if e.kind == ErrorKind::Validation),
            "跨分类挪位置是 validation",
        )?;

        // 修改：换分类排到最后、改文字、完成、截止日期
        let moved = items
            .update(
                a.id.clone(),
                ItemPatch {
                    kind: Some(Kind::Todo),
                    text: Some("回复老王 PR".to_owned()),
                    due: Some(DueChange::Set(today.to_owned())),
                    ..ItemPatch::default()
                },
            )
            .await?;
        check(
            moved.kind == Kind::Todo && moved.text == "回复老王 PR",
            "改分类和文字",
        )?;
        check(
            moved.updated_at >= moved.created_at,
            "updated_at 不早于 created_at",
        )?;
        check(
            kind_of(&items, Kind::Todo)
                .await?
                .last()
                .map(String::as_str)
                == Some("回复老王 PR"),
            "换分类后排在最后",
        )?;
        let due_today = items
            .list(ItemFilter {
                due: Some(super::DueFilter::Today),
                ..ItemFilter::default()
            })
            .await?;
        check(
            due_today.len() == 1 && due_today[0].id == a.id,
            "按今天到期筛",
        )?;
        let counts = items.counts().await?;
        check(counts.todo == 4 && counts.inbox == 0, "按分类计数")?;
        check(counts.due_today == 1 && counts.overdue == 0, "到期计数")?;

        let cleared = items
            .update(
                a.id.clone(),
                ItemPatch {
                    due: Some(DueChange::Clear),
                    status: Some(Status::Done),
                    ..ItemPatch::default()
                },
            )
            .await?;
        check(
            cleared.due_on.is_none() && cleared.status == Status::Done,
            "清掉日期、完成",
        )?;
        check(
            !kind_of(&items, Kind::Todo)
                .await?
                .contains(&"回复老王 PR".to_owned()),
            "默认只列未完成的",
        )?;
        let done = items
            .list(ItemFilter {
                status: Some(Status::Done),
                ..ItemFilter::default()
            })
            .await?;
        check(done.len() == 1, "能列出已完成的")?;

        // 删除
        items.delete(a.id.clone()).await?;
        let gone = items.get(a.id.clone()).await;
        check(
            matches!(gone, Err(ref e) if e.kind == ErrorKind::NotFound),
            "删掉后找不到",
        )?;
        Ok(())
    }
}
