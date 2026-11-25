//! `review.actions`：别的组件登记的条目按键。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use clock_contract::testing::FakeClock;
use items::ItemsComponent;
use items_contracts::{ITEMS, Items, Kind, NewItem};
use kernel_api::{
    AppError, Component, ConfigSection, ErrorKind, Host, Kind as ComponentKind, Manifest,
    async_trait,
};
use log_contract::testing::MemoryLog;
use review::Review;
use review_contracts::testing::item_actions_contract;
use review_contracts::{
    ActionResult, ItemActionRequest, ItemActionSpec, ItemActions, REVIEW_ACTIONS,
};
use serde_json::json;
use storage_sqlite::StorageSqlite;
use tui_contract::ActionRequest;
use tui_contract::testing::FakeTui;

/// 2026-11-18 12:00 +08:00
const NOW: i64 = 1_794_974_400_000;

/// 登记一个键，记下收到的请求。
#[derive(Clone)]
struct Pusher {
    key: char,
    seen: Arc<Mutex<Vec<ItemActionRequest>>>,
}

#[async_trait]
impl ItemActions for Pusher {
    async fn actions(&self) -> Result<Vec<ItemActionSpec>, AppError> {
        Ok(vec![ItemActionSpec {
            key: self.key,
            label: "推走".to_owned(),
            confirm: false,
            on_done_page: false,
        }])
    }

    async fn act(&self, request: ItemActionRequest) -> Result<ActionResult, AppError> {
        if request.key != self.key {
            return Err(AppError::validation("不是我的键"));
        }
        self.seen.lock().unwrap().push(request);
        Ok(ActionResult {
            message: Some("推走了".to_owned()),
            reload: true,
            ..ActionResult::default()
        })
    }
}

impl Component for Pusher {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: "pusher",
            kind: ComponentKind::Feature,
            summary: "测试用",
            provides: vec![],
            requires: vec![REVIEW_ACTIONS.id()],
            optional_requires: vec![],
        }
    }

    fn start(&self, _: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        host.contribute(&REVIEW_ACTIONS, Arc::new(self.clone()))
    }
}

fn pusher(key: char) -> Pusher {
    Pusher {
        key,
        seen: Arc::default(),
    }
}

async fn rig(p: Pusher) -> (testkit::TestApp, FakeTui, Arc<dyn Items>) {
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
        .with(p)
        .boot()
        .await
        .unwrap();
    let items = app.resolve(&ITEMS).unwrap();
    (app, tui, items)
}

#[tokio::test]
async fn contributed_keys_show_up_and_get_the_selected_item() {
    let p = pusher('P');
    let (_app, tui, items) = rig(p.clone()).await;
    let pages = tui.pages().await.unwrap();
    let todo = pages.iter().find(|s| s.id == "todo").unwrap();
    assert!(
        todo.actions
            .iter()
            .any(|a| a.key == 'P' && a.label == "推走")
    );
    let done = pages.iter().find(|s| s.id == "done").unwrap();
    assert!(
        !done.actions.iter().any(|a| a.key == 'P'),
        "默认不在已完成页"
    );

    let it = items
        .add(NewItem {
            kind: Some(Kind::Todo),
            ..NewItem::text("看 PR")
        })
        .await
        .unwrap();
    let r = tui
        .act(
            "todo",
            ActionRequest {
                key: 'P',
                row: Some(it.id.clone()),
                input: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(r.message.as_deref(), Some("推走了"));
    assert_eq!(p.seen.lock().unwrap()[0].item_id, it.id);

    let none = tui
        .act(
            "todo",
            ActionRequest {
                key: 'P',
                row: None,
                input: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(none.message.as_deref(), Some("没有选中的条目"));
    let on_done = tui
        .act(
            "done",
            ActionRequest {
                key: 'P',
                row: Some(it.id),
                input: None,
            },
        )
        .await;
    assert!(on_done.is_err(), "已完成页上没有这个键");
}

#[tokio::test]
async fn a_clashing_key_is_a_config_error() {
    // x 是「完成」
    let (_app, tui, _) = rig(pusher('x')).await;
    let err = tui.pages().await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Config);
    assert!(err.message.contains("`x`"), "{}", err.message);
    let (_app, tui, _) = rig(pusher('j')).await;
    assert_eq!(
        tui.pages().await.unwrap_err().kind,
        ErrorKind::Config,
        "外壳的键"
    );
}

#[tokio::test]
async fn the_test_pusher_obeys_the_contract() {
    let p = pusher('P');
    item_actions_contract(Arc::new(p), "x").await.unwrap();
}
