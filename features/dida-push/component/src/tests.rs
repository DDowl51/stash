//! 组件测试：真实的 `items`、`dida-tasks` + 内存 SQLite + 内存远端 + 发 `capture.saved` 的假 capture。

use std::sync::{Arc as StdArc, OnceLock};

use capture_contracts::{Capture, Draft, Saved};
use cli_contract::Format;
use cli_contract::testing::FakeCli;
use clock_contract::testing::FakeClock;
use kernel_api::{ErrorKind, Publisher};
use log_contract::testing::MemoryLog;
use review_contracts::testing::item_actions_contract;
use serde_json::{Value, json};
use task_service_contract::testing::MemoryTaskService;
use task_service_contract::{Due, Status};

use super::*;

/// 2026-11-25 10:00 +08:00
const NOW: i64 = 1_795_572_000_000;

/// 假的 capture：只提供契约，好让测试发 `capture.saved`。
#[derive(Clone, Default)]
struct FakeCapture(StdArc<OnceLock<Publisher<Saved>>>);

#[async_trait]
impl Capture for FakeCapture {
    async fn parse(&self, draft: Draft) -> Result<Draft, AppError> {
        Ok(draft)
    }
}

impl Component for FakeCapture {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: "capture-fake",
            kind: Kind::Feature,
            summary: "测试用",
            provides: vec![CAPTURE.id()],
            requires: vec![],
            optional_requires: vec![],
        }
    }

    fn start(&self, _: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        let _ = self.0.set(host.publisher(&CAPTURE_SAVED)?);
        host.provide(&CAPTURE, Arc::new(self.clone()))
    }
}

struct Env {
    remote: MemoryTaskService,
    cli: FakeCli,
    capture: FakeCapture,
    items: Arc<dyn Items>,
    app: testkit::TestApp,
}

impl Env {
    async fn add(&self, text: &str) -> Item {
        self.items
            .add(items_contracts::NewItem {
                due_on: Some("2026-11-27".into()),
                kind: Some(items_contracts::Kind::Todo),
                source: Some("ghostty · ~".into()),
                ..items_contracts::NewItem::text(text)
            })
            .await
            .unwrap()
    }

    async fn push(&self, args: &[(&str, &str)]) -> Result<String, AppError> {
        self.cli
            .invoke("dida push", args, Format::Json)
            .await
            .map(|o| o.text)
    }

    async fn item(&self, id: &str) -> Item {
        self.items.get(id.to_owned()).await.unwrap()
    }

    async fn saved(&self, item_id: &str, routes: &[&str]) {
        let event = Saved {
            item_id: item_id.to_owned(),
            routes: routes.iter().map(|r| (*r).to_owned()).collect(),
        };
        self.capture.0.get().unwrap().publish(event).await.unwrap();
        self.app.settle().await;
    }
}

async fn env(config: Value) -> Env {
    let remote = MemoryTaskService::default()
        .with_project("p1", "工作")
        .with_project("p2", "生活");
    let (cli, capture) = (FakeCli::default(), FakeCapture::default());
    let app = testkit::app()
        .with(DidaPush)
        .config("dida-push", config)
        .unwrap()
        .with(items::ItemsComponent)
        .with(dida_tasks::DidaTasks)
        .with(storage_sqlite::StorageSqlite)
        .config("storage-sqlite", json!({ "path": ":memory:" }))
        .unwrap()
        .with(remote.clone())
        .with(FakeClock::at(NOW, 480))
        .with(MemoryLog::default())
        .with(cli.clone())
        .with(capture.clone())
        .boot()
        .await
        .unwrap();
    let items = app.resolve(&ITEMS).unwrap();
    Env {
        remote,
        cli,
        capture,
        items,
        app,
    }
}

fn link(it: &Item) -> Option<&str> {
    it.context
        .iter()
        .find(|c| c.key == dida_push_core::LINK_KEY)
        .map(|c| c.value.as_str())
}

#[tokio::test(flavor = "multi_thread")]
async fn pushing_creates_a_task_and_leaves_a_link() {
    let e = env(json!({})).await;
    let it = e.add("看 PR").await;
    assert_eq!(
        e.push(&[("id", &it.id[20..])]).await.unwrap(),
        "",
        "没事不输出"
    );
    let t = e.remote.all_tasks().remove(0);
    assert_eq!(t.title, "看 PR");
    assert_eq!(t.project_id, "inbox", "默认进收集箱");
    assert_eq!(t.due, Some(Due::date("2026-11-27")));
    assert!(t.content.starts_with("来源：ghostty · ~"), "{}", t.content);
    assert!(t.content.ends_with(&format!("stash 条目 {}", it.id)));
    let after = e.item(&it.id).await;
    assert_eq!(
        link(&after),
        Some(format!("memory://inbox/{}", t.id).as_str())
    );
    assert_eq!(after.status, items_contracts::Status::Open, "条目原样留着");
    // 推过的不再推
    let again = e.push(&[("id", &it.id)]).await.unwrap_err();
    assert_eq!(again.kind, ErrorKind::Validation);
    assert_eq!(e.remote.all_tasks().len(), 1);
    e.push(&[("id", &it.id), ("force", "true")]).await.unwrap();
    assert_eq!(e.remote.all_tasks().len(), 2, "--force 再推一次");
}

#[tokio::test(flavor = "multi_thread")]
async fn failures_are_recorded_and_can_be_retried() {
    let e = env(json!({})).await;
    let it = e.add("断网时记的").await;
    e.remote.fail_with(Some(ErrorKind::Unavailable));
    let err = e.push(&[("id", &it.id)]).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Unavailable);
    let after = e.item(&it.id).await;
    assert!(
        after
            .context
            .iter()
            .any(|c| c.key == dida_push_core::FAILED_KEY),
        "条目上记着失败原因"
    );
    assert!(link(&after).is_none());
    // 还是不行：retry 报错
    assert_eq!(
        e.push(&[("retry", "true")]).await.unwrap_err().kind,
        ErrorKind::Unavailable
    );
    e.remote.fail_with(None);
    assert_eq!(e.push(&[("retry", "true")]).await.unwrap(), "");
    assert!(link(&e.item(&it.id).await).is_some());
    assert_eq!(e.remote.all_tasks().len(), 1);
    // 已经成功的不会再被 retry
    e.push(&[("retry", "true")]).await.unwrap();
    assert_eq!(e.remote.all_tasks().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn routed_saves_are_pushed() {
    let e = env(json!({})).await;
    let a = e.add("要推的").await;
    let b = e.add("不推的").await;
    e.saved(&a.id, &["dida"]).await;
    e.saved(&b.id, &[]).await;
    e.saved(&b.id, &["mail"]).await;
    let titles: Vec<String> = e.remote.all_tasks().into_iter().map(|t| t.title).collect();
    assert_eq!(titles, ["要推的"]);
    assert!(link(&e.item(&a.id).await).is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_routed_push_keeps_the_item() {
    let e = env(json!({})).await;
    let a = e.add("断网时记的").await;
    e.remote.fail_with(Some(ErrorKind::Unavailable));
    e.saved(&a.id, &["dida"]).await;
    let after = e.item(&a.id).await;
    assert!(
        after
            .context
            .iter()
            .any(|c| c.key == dida_push_core::FAILED_KEY)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn projects_by_kind() {
    let e = env(json!({ "project": "生活", "kind_projects": { "todo": "工" } })).await;
    let it = e.add("写周报").await;
    e.push(&[("id", &it.id)]).await.unwrap();
    assert_eq!(e.remote.all_tasks()[0].project_id, "p1");
    let mut idea = e.add("一个想法").await;
    idea = e
        .items
        .update(
            idea.id.clone(),
            ItemPatch {
                kind: Some(items_contracts::Kind::Idea),
                ..ItemPatch::default()
            },
        )
        .await
        .unwrap();
    e.push(&[("id", &idea.id)]).await.unwrap();
    assert_eq!(
        e.remote.all_tasks()[1].project_id,
        "p2",
        "没单独配的分类用 project"
    );
    assert_eq!(e.remote.all_tasks()[1].status, Status::Open);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_review_key() {
    let e = env(json!({})).await;
    let it = e.add("看 PR").await;
    let actions: Arc<dyn ItemActions> = Arc::new(Actions(Arc::new(Push {
        config: Config::default(),
        items: e.items.clone(),
        tasks: e.app.resolve(&TASKS).unwrap(),
        sql: e.app.resolve(&SQL).unwrap(),
        log: Logger::new(e.app.resolve(&LOG).unwrap(), "t"),
    })));
    item_actions_contract(actions.clone(), &it.id)
        .await
        .unwrap();
    let r = actions
        .act(ItemActionRequest {
            key: 'P',
            item_id: it.id.clone(),
            input: None,
        })
        .await
        .unwrap();
    assert_eq!(r.message.as_deref(), Some("已推到滴答"));
    assert!(r.reload);
    let again = actions
        .act(ItemActionRequest {
            key: 'P',
            item_id: it.id,
            input: None,
        })
        .await
        .unwrap();
    assert_eq!(again.message.as_deref(), Some("这条已经推过了"));
}

#[tokio::test(flavor = "multi_thread")]
async fn argument_mistakes() {
    let e = env(json!({})).await;
    assert_eq!(e.push(&[]).await.unwrap_err().kind, ErrorKind::Validation);
    let both = e.push(&[("id", "x"), ("retry", "true")]).await.unwrap_err();
    assert_eq!(both.kind, ErrorKind::Validation);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reserved_key_fails_at_startup() {
    let err = testkit::app()
        .with(DidaPush)
        .config("dida-push", json!({ "key": "j" }))
        .unwrap()
        .with(items::ItemsComponent)
        .with(dida_tasks::DidaTasks)
        .with(storage_sqlite::StorageSqlite)
        .config("storage-sqlite", json!({ "path": ":memory:" }))
        .unwrap()
        .with(MemoryTaskService::default())
        .with(FakeClock::at(NOW, 480))
        .with(MemoryLog::default())
        .boot()
        .await
        .err()
        .unwrap();
    assert_eq!(err.kind, ErrorKind::Config);
}
