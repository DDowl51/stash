//! 应用功能 `review`：终端里整理条目的五个页面。
//!
//! 页面内容和按键都是纯数据，交给终端界面外壳（`tui` 契约）去画、去收按键；
//! 按键的意思（换分类、完成、挪位置……）在这里翻译成 `items` 的请求。

use std::sync::Arc;

use clock_contract::{CLOCK, Clock, Moment};
use items_contracts::{DueChange, ITEMS, Item, ItemFilter, ItemPatch, Items, Kind, Status};
use kernel_api::{
    AppError, Component, ConfigSection, Host, Kind as ComponentKind, Manifest, NoConfig,
    async_trait, register_component,
};
use review_core::{Op, Page, context_to_open, due_input, neighbour, row, short_id, today};
use tui_contract::{
    ActionRequest, ActionResult, Ask, Block, PageProvider, PageRequest, PageSpec, PageView,
    TUI_PAGES,
};

/// 已完成页最多列多少条。
const DONE_LIMIT: u32 = 200;

struct Pages {
    items: Arc<dyn Items>,
    clock: Arc<dyn Clock>,
}

fn page_of(id: &str) -> Result<Page, AppError> {
    Page::from_id(id).ok_or_else(|| AppError::not_found(format!("review 没有页面 `{id}`")))
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Inbox => "收件箱",
        Kind::Todo => "待办",
        Kind::Idea => "想法",
        Kind::ReadLater => "稍后读",
    }
}

fn said(message: impl Into<String>) -> ActionResult {
    ActionResult {
        message: Some(message.into()),
        ..ActionResult::default()
    }
}

/// 改完了：提示一句、重取这一页。
fn done(message: impl Into<String>) -> ActionResult {
    ActionResult {
        reload: true,
        ..said(message)
    }
}

impl Pages {
    async fn now(&self) -> Result<Moment, AppError> {
        self.clock.now().await
    }

    /// 这一页的条目，按这一页的顺序。
    async fn list(&self, page: Page) -> Result<Vec<Item>, AppError> {
        let mut items = self
            .items
            .list(ItemFilter {
                kind: page.kind(),
                status: Some(page.status()),
                due: None,
                limit: (page == Page::Done).then_some(DONE_LIMIT),
            })
            .await?;
        page.sort(&mut items);
        Ok(items)
    }

    async fn patch(&self, id: &str, patch: ItemPatch) -> Result<Item, AppError> {
        self.items.update(id.to_owned(), patch).await
    }

    async fn run(
        &self,
        page: Page,
        op: Op,
        id: &str,
        input: Option<String>,
    ) -> Result<ActionResult, AppError> {
        let short = short_id(id).to_owned();
        Ok(match op {
            Op::MoveTo(kind) => {
                self.patch(
                    id,
                    ItemPatch {
                        kind: Some(kind),
                        ..ItemPatch::default()
                    },
                )
                .await?;
                done(format!("{short} 移到了{}", kind_label(kind)))
            }
            Op::SetStatus(status) => {
                self.patch(
                    id,
                    ItemPatch {
                        status: Some(status),
                        ..ItemPatch::default()
                    },
                )
                .await?;
                done(match status {
                    Status::Done => format!("{short} 完成了"),
                    Status::Archived => format!("{short} 归档了"),
                    Status::Open => format!("{short} 恢复成未完成"),
                })
            }
            Op::Delete => {
                self.items.delete(id.to_owned()).await?;
                done(format!("{short} 删掉了"))
            }
            Op::EditText => match input {
                None => {
                    let it = self.items.get(id.to_owned()).await?;
                    ActionResult {
                        ask: Some(Ask {
                            prompt: "改成".to_owned(),
                            initial: it.text,
                        }),
                        ..ActionResult::default()
                    }
                }
                Some(text) if text.trim().is_empty() => said("没改：文字不能是空的"),
                Some(text) => {
                    self.patch(
                        id,
                        ItemPatch {
                            text: Some(text),
                            ..ItemPatch::default()
                        },
                    )
                    .await?;
                    ActionResult {
                        select: Some(id.to_owned()),
                        ..done(format!("{short} 改好了"))
                    }
                }
            },
            Op::SetDue => match input {
                None => {
                    let it = self.items.get(id.to_owned()).await?;
                    ActionResult {
                        ask: Some(Ask {
                            prompt: "截止（2026-11-20、11-20、明天、+3，留空清掉）".to_owned(),
                            initial: it.due_on.unwrap_or_default(),
                        }),
                        ..ActionResult::default()
                    }
                }
                Some(text) => {
                    let today = today(self.now().await?)
                        .ok_or_else(|| AppError::unavailable("时钟给的时刻不对"))?;
                    let due = match due_input(&text, today) {
                        Ok(due) => due,
                        Err(why) => return Ok(said(why)),
                    };
                    let change = due.clone().map_or(DueChange::Clear, DueChange::Set);
                    self.patch(
                        id,
                        ItemPatch {
                            due: Some(change),
                            ..ItemPatch::default()
                        },
                    )
                    .await?;
                    ActionResult {
                        select: Some(id.to_owned()),
                        ..done(match due {
                            Some(d) => format!("{short} 截止 {d}"),
                            None => format!("{short} 不设截止日期了"),
                        })
                    }
                }
            },
            Op::Reorder(up) => {
                let ids: Vec<String> = self.list(page).await?.into_iter().map(|i| i.id).collect();
                match neighbour(&ids, id, up) {
                    Some(before) => {
                        self.items.reorder(id.to_owned(), before).await?;
                        ActionResult {
                            select: Some(id.to_owned()),
                            reload: true,
                            ..ActionResult::default()
                        }
                    }
                    None => said(if up {
                        "已经在最上面"
                    } else {
                        "已经在最下面"
                    }),
                }
            }
            Op::OpenLink | Op::OpenImage => {
                let it = self.items.get(id.to_owned()).await?;
                let image = op == Op::OpenImage;
                match context_to_open(&it, image) {
                    Some(target) => ActionResult {
                        open: Some(target),
                        ..ActionResult::default()
                    },
                    None => said(if image {
                        "这条没有截图"
                    } else {
                        "这条没有链接"
                    }),
                }
            }
        })
    }
}

#[async_trait]
impl PageProvider for Pages {
    async fn pages(&self) -> Result<Vec<PageSpec>, AppError> {
        Ok(Page::ALL.into_iter().map(Page::spec).collect())
    }

    async fn render(&self, page: String, _request: PageRequest) -> Result<PageView, AppError> {
        let page = page_of(&page)?;
        let now = self.now().await?;
        let today = today(now).ok_or_else(|| AppError::unavailable("时钟给的时刻不对"))?;
        let items = self.list(page).await?;
        let today_s = today.to_string();
        let overdue = items
            .iter()
            .filter(|i| i.due_on.as_deref().is_some_and(|d| *d < *today_s))
            .count();
        let due_today = items
            .iter()
            .filter(|i| i.due_on.as_deref() == Some(today_s.as_str()))
            .count();
        let mut note = format!("{} 条", items.len());
        if page != Page::Done && (overdue > 0 || due_today > 0) {
            note.push_str(&format!("，过期 {overdue}，今天到期 {due_today}"));
        }
        let blocks = if items.is_empty() {
            vec![Block::Empty {
                message: match page {
                    Page::Inbox => "收件箱是空的".to_owned(),
                    Page::Done => "还没有完成的".to_owned(),
                    _ => format!("没有{}", page.title()),
                },
            }]
        } else {
            vec![Block::List {
                rows: items
                    .iter()
                    .map(|i| row(i, page, today, now.offset_min))
                    .collect(),
            }]
        };
        Ok(PageView {
            heading: page.title().to_owned(),
            note,
            blocks,
        })
    }

    async fn act(&self, page: String, request: ActionRequest) -> Result<ActionResult, AppError> {
        let page = page_of(&page)?;
        let Some(op) = page.op(request.key) else {
            return Err(AppError::validation(format!(
                "页面「{}」没有按键 `{}`",
                page.title(),
                request.key
            )));
        };
        let Some(id) = request.row else {
            return Ok(said("没有选中的条目"));
        };
        self.run(page, op, &id, request.input).await
    }
}

pub struct Review;

impl Component for Review {
    fn config_defaults(&self) -> Option<ConfigSection> {
        ConfigSection::defaults::<NoConfig>()
    }

    fn manifest(&self) -> Manifest {
        Manifest {
            id: "review",
            kind: ComponentKind::Feature,
            summary: "终端里整理条目：收件箱、待办、想法、稍后读、已完成",
            provides: vec![],
            requires: vec![ITEMS.id(), CLOCK.id(), TUI_PAGES.id()],
            optional_requires: vec![],
        }
    }

    fn start(&self, config: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        config.parse::<NoConfig>()?;
        let pages = Pages {
            items: host.resolve(&ITEMS)?,
            clock: host.resolve(&CLOCK)?,
        };
        host.contribute(&TUI_PAGES, Arc::new(pages))
    }
}

register_component!(Review);
