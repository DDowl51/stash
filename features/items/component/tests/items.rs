//! 把 `items` 和真实的 SQLite（内存库）、假时钟、内存日志、假命令行装在一起测。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use cli_contract::Format;
use cli_contract::testing::FakeCli;
use clock_contract::testing::FakeClock;
use items::ItemsComponent;
use items_contracts::testing::items_contract;
use items_contracts::{Changed, ITEMS, ITEMS_CHANGED, ItemFilter, Items, Kind, NewItem};
use kernel_api::{AppError, Component, ConfigSection, Host, Kind as ComponentKind, Manifest};
use log_contract::testing::MemoryLog;
use serde_json::json;
use storage_sqlite::StorageSqlite;
use testkit::{Builder, TestApp};

/// 2026-11-17T12:00:00+09:00
const NOON_JST: i64 = 1_794_884_400_000;

struct Fixture {
    app: TestApp,
    items: Arc<dyn Items>,
    cli: FakeCli,
}

async fn fixture() -> Fixture {
    fixture_with(|b| b).await
}

async fn fixture_with(extend: impl FnOnce(Builder) -> Builder) -> Fixture {
    let cli = FakeCli::default();
    let base = testkit::app()
        .with(StorageSqlite)
        .config("storage-sqlite", json!({ "path": ":memory:" }))
        .unwrap()
        .with(FakeClock::at(NOON_JST, 540))
        .with(MemoryLog::default())
        .with(cli.clone())
        .with(ItemsComponent);
    let app = extend(base).boot().await.unwrap();
    let items = app.resolve(&ITEMS).unwrap();
    Fixture { app, items, cli }
}

const TEXT: Format = Format::Text {
    color: false,
    width: 80,
};

#[tokio::test]
async fn obeys_the_items_contract_on_sqlite() {
    let f = fixture().await;
    items_contract(f.items.clone(), "2026-11-17").await.unwrap();
}

/// 订阅「条目变了」的假功能。
struct Listener(Arc<Mutex<Vec<Changed>>>);

impl Component for Listener {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: "listener",
            kind: ComponentKind::Feature,
            summary: "测试用",
            provides: vec![],
            requires: vec![ITEMS.id()],
            optional_requires: vec![],
        }
    }

    fn start(&self, _: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        let seen = self.0.clone();
        host.subscribe(&ITEMS_CHANGED, "note", move |c: Changed| {
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(c);
                Ok(())
            }
        })
    }
}

#[tokio::test]
async fn publishes_changes() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let f = fixture_with(|b| b.with(Listener(seen.clone()))).await;
    let it = f.items.add(NewItem::text("x")).await.unwrap();
    f.items.delete(it.id.clone()).await.unwrap();
    f.app.settle().await;
    assert_eq!(
        *seen.lock().unwrap(),
        [
            Changed {
                ids: vec![it.id.clone()]
            },
            Changed { ids: vec![it.id] }
        ]
    );
}

#[tokio::test]
async fn cli_list_show_and_change() {
    let f = fixture().await;
    // 新建在 capture 的 `stash add` 里，这里直接走契约
    let added = f
        .items
        .add(NewItem {
            kind: Some(Kind::Todo),
            due_on: Some("2026-11-20".to_owned()),
            ..NewItem::text("回复老王")
        })
        .await
        .unwrap();
    let id = added.id.clone();
    let short = &id[id.len() - 6..];

    f.items.add(NewItem::text("看看 niri 新版")).await.unwrap();

    // 文本：不限分类时带分类标签；JSON 是一行一个
    let text = f.cli.invoke("ls", &[], TEXT).await.unwrap().text;
    assert!(text.contains("[收件箱]  看看 niri 新版"), "{text}");
    assert!(
        text.contains(&format!("{short}  [待办]  @2026-11-20  回复老王")),
        "{text}"
    );
    let json = f
        .cli
        .invoke("ls", &[("kind", "todo")], Format::Json)
        .await
        .unwrap()
        .text;
    assert_eq!(json.lines().count(), 1);

    // 修改类命令成功时不输出
    assert_eq!(
        f.cli
            .invoke("move", &[("id", short), ("kind", "idea")], TEXT)
            .await
            .unwrap()
            .text,
        ""
    );
    assert_eq!(
        f.cli
            .invoke("due", &[("id", short), ("date", "none")], TEXT)
            .await
            .unwrap()
            .text,
        ""
    );
    assert_eq!(
        f.cli
            .invoke("done", &[("id", short)], TEXT)
            .await
            .unwrap()
            .text,
        ""
    );
    let it = f.items.get(id.clone()).await.unwrap();
    assert_eq!(
        (it.kind, it.due_on, it.status.as_str()),
        (Kind::Idea, None, "done")
    );

    let shown = f
        .cli
        .invoke("show", &[("id", short)], TEXT)
        .await
        .unwrap()
        .text;
    assert!(
        shown.contains("分类  想法") && shown.contains("状态  done"),
        "{shown}"
    );

    f.cli.invoke("rm", &[("id", short)], TEXT).await.unwrap();
    assert!(
        f.items
            .list(ItemFilter::default())
            .await
            .unwrap()
            .iter()
            .all(|i| i.id != id)
    );
}

#[tokio::test]
async fn cli_rejects_bad_input() {
    let f = fixture().await;
    let kind = |r: Result<_, kernel_api::AppError>| r.unwrap_err().kind;
    use kernel_api::ErrorKind::{NotFound, Validation};
    let it = f.items.add(NewItem::text("x")).await.unwrap();
    assert_eq!(
        kind(
            f.cli
                .invoke("due", &[("id", &it.id), ("date", "明天")], TEXT)
                .await
        ),
        Validation
    );
    assert_eq!(
        kind(f.cli.invoke("done", &[("id", "ZZZZZZ")], TEXT).await),
        NotFound
    );
    assert_eq!(
        kind(f.cli.invoke("ls", &[("due", "someday")], TEXT).await),
        Validation
    );
}
