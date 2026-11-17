//! `remind` 和真的 `items`（内存 SQLite）、假通知、假时钟、假命令行装在一起测。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use cli_contract::Format;
use cli_contract::testing::FakeCli;
use clock_contract::testing::FakeClock;
use items::ItemsComponent;
use items_contracts::{ITEMS, ItemPatch, Items, Kind, NewItem, Status};
use log_contract::testing::MemoryLog;
use notify_contract::Urgency;
use notify_contract::testing::RecordingNotify;
use remind::RemindComponent;
use serde_json::{Value, json};
use storage_sqlite::StorageSqlite;
use testkit::TestApp;

/// 2026-11-18 12:00 +08:00
const NOON: i64 = 1_794_974_400_000;
const DAY: i64 = 86_400_000;
const TEXT: Format = Format::Text {
    color: false,
    width: 80,
};

struct Rig {
    _app: TestApp,
    cli: FakeCli,
    items: Arc<dyn Items>,
    notify: RecordingNotify,
    clock: FakeClock,
}

async fn rig_with(notify: RecordingNotify, config: Value) -> Rig {
    let (cli, clock) = (FakeCli::default(), FakeClock::at(NOON, 480));
    let app = testkit::app()
        .with(StorageSqlite)
        .config("storage-sqlite", json!({ "path": ":memory:" }))
        .unwrap()
        .with(clock.clone())
        .with(MemoryLog::default())
        .with(cli.clone())
        .with(notify.clone())
        .with(ItemsComponent)
        .with(RemindComponent)
        .config("remind", config)
        .unwrap()
        .boot()
        .await
        .unwrap();
    let items = app.resolve(&ITEMS).unwrap();
    Rig {
        _app: app,
        cli,
        items,
        notify,
        clock,
    }
}

async fn rig() -> Rig {
    rig_with(RecordingNotify::default(), json!({ "inbox_threshold": 2 })).await
}

impl Rig {
    async fn add(&self, text: &str, kind: Kind, due: Option<&str>) -> String {
        self.items
            .add(NewItem {
                kind: Some(kind),
                due_on: due.map(str::to_owned),
                ..NewItem::text(text)
            })
            .await
            .unwrap()
            .id
    }

    async fn remind(&self, dry: bool) -> Result<String, kernel_api::AppError> {
        let args: &[(&str, &str)] = if dry { &[("dry-run", "true")] } else { &[] };
        self.cli.invoke("remind", args, TEXT).await.map(|o| o.text)
    }
}

#[tokio::test]
async fn nothing_to_say_sends_nothing() {
    let r = rig().await;
    r.add("一条", Kind::Inbox, None).await;
    r.add("以后的", Kind::Todo, Some("2026-12-01")).await;
    assert_eq!(r.remind(false).await.unwrap(), "");
    assert!(r.notify.sent().is_empty());
}

#[tokio::test]
async fn inbox_backlog_and_due_items_once_a_day() {
    let r = rig().await;
    for t in ["甲", "乙", "丙"] {
        r.add(t, Kind::Inbox, None).await;
    }
    r.add("交周报", Kind::Todo, Some("2026-11-17")).await;
    r.add("修 bug", Kind::Todo, Some("2026-11-18")).await;
    let done = r.add("做完的", Kind::Todo, Some("2026-11-18")).await;
    r.items
        .update(
            done,
            ItemPatch {
                status: Some(Status::Done),
                ..ItemPatch::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(r.remind(false).await.unwrap(), "", "没事不输出");
    let sent = r.notify.sent();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].summary, "收件箱有 3 条没整理");
    assert_eq!(sent[1].summary, "2 条到期");
    assert_eq!(sent[1].body, "· 交周报（过期 1 天）\n· 修 bug（今天）");
    assert_eq!(sent[1].urgency, Urgency::Critical);

    // 同一天再跑：不重复
    r.remind(false).await.unwrap();
    assert_eq!(r.notify.sent().len(), 2);

    // 当天新到期的还会提醒，只提醒新的
    r.add("新到期", Kind::Idea, Some("2026-11-18")).await;
    r.remind(false).await.unwrap();
    let sent = r.notify.sent();
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[2].summary, "今天：新到期");

    // 第二天：都再提醒一次
    r.clock.set(NOON + DAY);
    r.remind(false).await.unwrap();
    assert_eq!(r.notify.sent().len(), 5);
}

#[tokio::test]
async fn dry_run_prints_without_sending_or_recording() {
    let r = rig().await;
    r.add("交周报", Kind::Todo, Some("2026-11-18")).await;
    let out = r.remind(true).await.unwrap();
    assert_eq!(out, "通知：今天：交周报\n  截止 2026-11-18\n");
    assert!(r.notify.sent().is_empty());
    let json = r
        .cli
        .invoke("remind", &[("dry-run", "true")], Format::Json)
        .await
        .unwrap()
        .text;
    let v: Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(v["urgency"], "normal");
    assert_eq!(v["keys"].as_array().unwrap().len(), 1);
    // dry-run 没有记，真跑时照样发
    r.remind(false).await.unwrap();
    assert_eq!(r.notify.sent().len(), 1);
}

#[tokio::test]
async fn a_missing_notification_service_fails_and_records_nothing() {
    let r = rig_with(RecordingNotify::failing(), json!({})).await;
    r.add("交周报", Kind::Todo, Some("2026-11-18")).await;
    let err = r.remind(false).await.unwrap_err();
    assert_eq!(err.kind, kernel_api::ErrorKind::Unavailable);
    // 没记下来：dry-run 里还在
    assert!(r.remind(true).await.unwrap().contains("交周报"));
}

#[tokio::test]
async fn days_ahead_from_config() {
    let r = rig_with(RecordingNotify::default(), json!({ "due_days_ahead": 1 })).await;
    r.add("明天的", Kind::Todo, Some("2026-11-19")).await;
    r.remind(false).await.unwrap();
    assert_eq!(r.notify.sent()[0].summary, "明天：明天的");
}
