//! `review` 和真的 `items`（内存 SQLite）、假的终端外壳、假时钟装在一起测：
//! 渲染出的页面，和每个按键对条目的影响。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use clock_contract::testing::FakeClock;
use items::ItemsComponent;
use items_contracts::{
    ContextEntry, ContextKind, ITEMS, Item, ItemFilter, Items, Kind, NewItem, Status,
};
use log_contract::testing::MemoryLog;
use review::Review;
use serde_json::json;
use storage_sqlite::StorageSqlite;
use testkit::TestApp;
use tui_contract::testing::FakeTui;
use tui_contract::{ActionRequest, ActionResult, Block, PageRequest, PageView};

/// 2026-11-18 12:00 +08:00
const NOW: i64 = 1_794_974_400_000;

struct Rig {
    _app: TestApp,
    tui: FakeTui,
    items: Arc<dyn Items>,
}

async fn rig() -> Rig {
    let tui = FakeTui::default();
    let app = testkit::app()
        .with(StorageSqlite)
        .config("storage-sqlite", json!({ "path": ":memory:" }))
        .unwrap()
        .with(FakeClock::at(NOW, 480))
        .with(MemoryLog::default())
        .with(ItemsComponent)
        .with(tui.clone())
        .with(Review)
        .boot()
        .await
        .unwrap();
    let items = app.resolve(&ITEMS).unwrap();
    Rig {
        _app: app,
        tui,
        items,
    }
}

impl Rig {
    async fn add(&self, text: &str, kind: Kind, due: Option<&str>) -> Item {
        self.items
            .add(NewItem {
                kind: Some(kind),
                due_on: due.map(str::to_owned),
                ..NewItem::text(text)
            })
            .await
            .unwrap()
    }

    async fn view(&self, page: &str) -> PageView {
        self.tui
            .render(
                page,
                PageRequest {
                    offset: 0,
                    width: 80,
                    input: String::new(),
                },
            )
            .await
            .unwrap()
    }

    /// 列表里各行的 id。
    async fn ids(&self, page: &str) -> Vec<String> {
        match &self.view(page).await.blocks[0] {
            Block::List { rows } => rows.iter().map(|r| r.id.clone()).collect(),
            _ => vec![],
        }
    }

    async fn press(&self, page: &str, key: char, row: &Item, input: Option<&str>) -> ActionResult {
        self.tui
            .act(
                page,
                ActionRequest {
                    key,
                    row: Some(row.id.clone()),
                    input: input.map(str::to_owned),
                },
            )
            .await
            .unwrap()
    }

    async fn get(&self, it: &Item) -> Item {
        self.items.get(it.id.clone()).await.unwrap()
    }
}

#[tokio::test]
async fn five_pages_with_their_keys() {
    let r = rig().await;
    let pages = r.tui.pages().await.unwrap();
    let keys: Vec<(char, &str)> = pages.iter().map(|p| (p.key, p.title.as_str())).collect();
    assert_eq!(
        keys,
        [
            ('1', "收件箱"),
            ('2', "待办"),
            ('3', "想法"),
            ('4', "稍后读"),
            ('5', "已完成")
        ]
    );
    assert!(pages[0].actions.iter().any(|a| a.key == 'd' && a.confirm));
}

#[tokio::test]
async fn pages_list_their_items_in_their_order() {
    let r = rig().await;
    let first = r.add("先记的", Kind::Inbox, None).await;
    let second = r.add("后记的", Kind::Inbox, None).await;
    let a = r.add("待办甲", Kind::Todo, Some("2026-11-17")).await;
    let b = r.add("待办乙", Kind::Todo, Some("2026-11-18")).await;

    // 收件箱：最新的在上（同一毫秒记的按 id，后生成的更大）
    let inbox = r.ids("inbox").await;
    assert_eq!(inbox.len(), 2);
    assert!(inbox.contains(&first.id) && inbox.contains(&second.id));

    let todo = r.view("todo").await;
    assert_eq!(todo.heading, "待办");
    assert_eq!(todo.note, "2 条，过期 1，今天到期 1");
    assert_eq!(
        r.ids("todo").await,
        [a.id.clone(), b.id.clone()],
        "按手动排序"
    );

    let empty = r.view("idea").await;
    assert!(matches!(&empty.blocks[0], Block::Empty { message } if message == "没有想法"));
}

#[tokio::test]
async fn triage_keys_change_the_item() {
    let r = rig().await;
    let it = r.add("回复老王", Kind::Inbox, None).await;

    let res = r.press("inbox", 't', &it, None).await;
    assert!(res.reload && res.message.as_deref().unwrap().contains("移到了待办"));
    assert_eq!(r.get(&it).await.kind, Kind::Todo);
    assert!(r.ids("inbox").await.is_empty());

    r.press("todo", 'x', &it, None).await;
    assert_eq!(r.get(&it).await.status, Status::Done);
    assert_eq!(r.ids("done").await, std::slice::from_ref(&it.id));

    r.press("done", 'u', &it, None).await;
    assert_eq!(r.get(&it).await.status, Status::Open);

    r.press("todo", 'a', &it, None).await;
    assert_eq!(r.get(&it).await.status, Status::Archived);

    r.press("inbox", 'd', &it, None).await;
    assert!(r.items.get(it.id.clone()).await.is_err(), "删掉了");
}

#[tokio::test]
async fn editing_asks_first_then_saves() {
    let r = rig().await;
    let it = r.add("回复老王", Kind::Inbox, None).await;

    let ask = r.press("inbox", 'e', &it, None).await;
    let ask = ask.ask.expect("先要输入");
    assert_eq!(ask.initial, "回复老王", "预填原文");
    let res = r.press("inbox", 'e', &it, Some("回复老王：周五前")).await;
    assert_eq!(res.select.as_deref(), Some(it.id.as_str()));
    assert_eq!(r.get(&it).await.text, "回复老王：周五前");

    let res = r.press("inbox", 'e', &it, Some("  ")).await;
    assert!(!res.reload && res.message.unwrap().contains("不能是空的"));
}

#[tokio::test]
async fn due_dates_from_the_input_box() {
    let r = rig().await;
    let it = r.add("交周报", Kind::Todo, Some("2026-11-20")).await;
    assert_eq!(
        r.press("todo", 'D', &it, None).await.ask.unwrap().initial,
        "2026-11-20"
    );

    r.press("todo", 'D', &it, Some("明天")).await;
    assert_eq!(r.get(&it).await.due_on.as_deref(), Some("2026-11-19"));
    r.press("todo", 'D', &it, Some("12-01")).await;
    assert_eq!(r.get(&it).await.due_on.as_deref(), Some("2026-12-01"));
    r.press("todo", 'D', &it, Some("")).await;
    assert_eq!(r.get(&it).await.due_on, None);

    let res = r.press("todo", 'D', &it, Some("下辈子")).await;
    assert!(!res.reload && res.message.unwrap().contains("看不懂"));
}

#[tokio::test]
async fn j_and_k_move_within_the_page() {
    let r = rig().await;
    let a = r.add("甲", Kind::Idea, None).await;
    let b = r.add("乙", Kind::Idea, None).await;
    let c = r.add("丙", Kind::Idea, None).await;

    let res = r.press("idea", 'K', &c, None).await;
    assert_eq!(res.select.as_deref(), Some(c.id.as_str()), "选中跟着它走");
    assert_eq!(
        r.ids("idea").await,
        [a.id.clone(), c.id.clone(), b.id.clone()]
    );
    r.press("idea", 'J', &a, None).await;
    assert_eq!(
        r.ids("idea").await,
        [c.id.clone(), a.id.clone(), b.id.clone()]
    );

    let res = r.press("idea", 'K', &c, None).await;
    assert_eq!(res.message.as_deref(), Some("已经在最上面"));
    let res = r.press("idea", 'J', &b, None).await;
    assert_eq!(res.message.as_deref(), Some("已经在最下面"));
}

#[tokio::test]
async fn opening_links_and_screenshots() {
    let r = rig().await;
    let with = r
        .items
        .add(NewItem {
            context: vec![
                ContextEntry::new(
                    "browser.url",
                    "网址",
                    ContextKind::Url,
                    "https://github.com/x/12",
                ),
                ContextEntry::new(
                    "screenshot.window",
                    "截图",
                    ContextKind::Image,
                    "/shots/1.png",
                ),
            ],
            ..NewItem::text("看 PR")
        })
        .await
        .unwrap();
    let bare = r.add("没东西", Kind::Inbox, None).await;

    assert_eq!(
        r.press("inbox", 'o', &with, None).await.open.as_deref(),
        Some("https://github.com/x/12")
    );
    assert_eq!(
        r.press("inbox", 'O', &with, None).await.open.as_deref(),
        Some("/shots/1.png")
    );
    let res = r.press("inbox", 'o', &bare, None).await;
    assert_eq!(
        (res.open, res.message.as_deref()),
        (None, Some("这条没有链接"))
    );
}

#[tokio::test]
async fn bad_requests() {
    let r = rig().await;
    let none = r
        .tui
        .act(
            "inbox",
            ActionRequest {
                key: 'x',
                row: None,
                input: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(none.message.as_deref(), Some("没有选中的条目"));
    let it = r.add("x", Kind::Todo, None).await;
    let err = r
        .tui
        .act(
            "todo",
            ActionRequest {
                key: 't',
                row: Some(it.id.clone()),
                input: None,
            },
        )
        .await
        .unwrap_err();
    assert!(err.message.contains("没有按键"), "{err}");
    // 别人已经删掉了：items 的错误原样给外壳去显示
    r.items.delete(it.id.clone()).await.unwrap();
    let err = r
        .tui
        .act(
            "todo",
            ActionRequest {
                key: 'x',
                row: Some(it.id.clone()),
                input: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err.kind, kernel_api::ErrorKind::NotFound);
    let _ = r.items.list(ItemFilter::default()).await.unwrap();
}
