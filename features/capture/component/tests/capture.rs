//! `capture` 的组件测试：假的输入框、窗口、条目存储、提供者和展开器。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use capture::CaptureComponent;
use capture_contracts::{
    CAPTURE_CONTEXT, CAPTURE_EXPANDERS, ContextEntry, ContextKind, ContextProvider, ContextSpec,
    Draft, Expander, ExpanderSpec, Kind, Phase, Snapshot,
};
use cli_contract::Format;
use cli_contract::testing::FakeCli;
use clock_contract::testing::FakeClock;
use items_contracts::testing::RecordingItems;
use kernel_api::{
    AppError, Component, ConfigSection, ErrorKind, Host, Kind as ComponentKind, Manifest,
    async_trait,
};
use log_contract::testing::MemoryLog;
use prompt_contract::testing::{FakePrompt, Scenario};
use prompt_contract::{Answer, PROMPT, Prompt, PromptRequest};
use serde_json::{Value, json};
use testkit::TestApp;
use window_contract::WindowInfo;
use window_contract::testing::FakeWindows;

const TEXT: Format = Format::Text {
    color: false,
    width: 100,
};

/// 2026-11-17T16:30Z，东八区是 11-18（周三）00:30。
const NOW: i64 = 1_794_933_000_000;

fn entry(key: &str, value: &str) -> ContextEntry {
    ContextEntry::new(key, key, ContextKind::Text, value)
}

// ---------- 假的提供者 ----------

#[derive(Clone)]
struct TestProvider {
    component: &'static str,
    id: &'static str,
    phase: Phase,
    delay_ms: u64,
    result: Result<Vec<ContextEntry>, String>,
    seen: Arc<Mutex<Vec<Snapshot>>>,
    discarded: Arc<Mutex<Vec<String>>>,
}

impl TestProvider {
    fn new(component: &'static str, id: &'static str, phase: Phase) -> Self {
        Self {
            component,
            id,
            phase,
            delay_ms: 0,
            result: Ok(vec![entry(&format!("{id}.x"), id)]),
            seen: Arc::default(),
            discarded: Arc::default(),
        }
    }

    fn slow(mut self, ms: u64) -> Self {
        self.delay_ms = ms;
        self
    }

    fn giving(mut self, entries: Vec<ContextEntry>) -> Self {
        self.result = Ok(entries);
        self
    }

    fn failing(mut self, why: &str) -> Self {
        self.result = Err(why.to_owned());
        self
    }

    fn discarded(&self) -> Vec<String> {
        self.discarded.lock().unwrap().clone()
    }
}

#[async_trait]
impl ContextProvider for TestProvider {
    async fn spec(&self) -> Result<ContextSpec, AppError> {
        Ok(ContextSpec {
            id: self.id.to_owned(),
            phase: self.phase,
        })
    }

    async fn collect(&self, snapshot: Snapshot) -> Result<Vec<ContextEntry>, AppError> {
        self.seen.lock().unwrap().push(snapshot);
        tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        self.result.clone().map_err(AppError::unavailable)
    }

    async fn discard(&self, snapshot: Snapshot) -> Result<(), AppError> {
        self.discarded.lock().unwrap().push(snapshot.item_id);
        Ok(())
    }
}

impl Component for TestProvider {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: self.component,
            kind: ComponentKind::Feature,
            summary: "测试用的上下文提供者",
            provides: vec![],
            requires: vec![CAPTURE_CONTEXT.id()],
            optional_requires: vec![],
        }
    }

    fn start(&self, _: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        host.contribute(&CAPTURE_CONTEXT, Arc::new(self.clone()))
    }
}

// ---------- 假的展开器 ----------

#[derive(Clone)]
enum Does {
    /// 把文字里的 from 换成 to，并记一条上下文。
    Replace(&'static str, &'static str),
    Fail,
}

#[derive(Clone)]
struct TestExpander {
    component: &'static str,
    id: &'static str,
    does: Does,
}

#[async_trait]
impl Expander for TestExpander {
    async fn spec(&self) -> Result<ExpanderSpec, AppError> {
        Ok(ExpanderSpec {
            id: self.id.to_owned(),
        })
    }

    async fn expand(&self, mut draft: Draft) -> Result<Draft, AppError> {
        match self.does {
            Does::Replace(from, to) => {
                if draft.text.contains(from) {
                    draft.text = draft.text.replace(from, to);
                    draft
                        .context
                        .push(entry(&format!("{}.from", self.id), from));
                }
                Ok(draft)
            }
            Does::Fail => Err(AppError::unavailable("算不出来")),
        }
    }
}

impl Component for TestExpander {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: self.component,
            kind: ComponentKind::Feature,
            summary: "测试用的展开器",
            provides: vec![],
            requires: vec![CAPTURE_EXPANDERS.id()],
            optional_requires: vec![],
        }
    }

    fn start(&self, _: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        host.contribute(&CAPTURE_EXPANDERS, Arc::new(self.clone()))
    }
}

// ---------- 慢的输入框：用户要打一会儿字 ----------

#[derive(Clone)]
struct SlowPrompt {
    ms: u64,
    text: &'static str,
}

#[async_trait]
impl Prompt for SlowPrompt {
    async fn ask(&self, _: PromptRequest) -> Result<Answer, AppError> {
        tokio::time::sleep(Duration::from_millis(self.ms)).await;
        Ok(Answer::from_raw(self.text))
    }
}

impl Component for SlowPrompt {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: "prompt-slow",
            kind: ComponentKind::Service,
            summary: "测试用：要等一会儿才回答的输入框",
            provides: vec![PROMPT.id()],
            requires: vec![],
            optional_requires: vec![],
        }
    }

    fn start(&self, _: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        host.provide(&PROMPT, Arc::new(self.clone()))
    }
}

// ---------- 装配 ----------

struct Rig {
    app: TestApp,
    cli: FakeCli,
    items: RecordingItems,
    log: MemoryLog,
}

impl Rig {
    async fn capture(&self, args: &[(&str, &str)]) -> Result<String, AppError> {
        self.cli.invoke("capture", args, TEXT).await.map(|o| o.text)
    }
}

fn zen() -> WindowInfo {
    WindowInfo {
        id: 7,
        app_id: Some("zen".to_owned()),
        title: Some("PR #12 — Zen Browser".to_owned()),
        workspace: None,
        pid: Some(4321),
    }
}

/// `with` 往里装测试自己的组件；`config` 是 capture 的配置。
async fn rig(with: impl FnOnce(testkit::Builder) -> testkit::Builder, config: Value) -> Rig {
    let (cli, items, log) = (
        FakeCli::default(),
        RecordingItems::default(),
        MemoryLog::default(),
    );
    let builder = testkit::app()
        .with(CaptureComponent)
        .config("capture", config)
        .unwrap()
        .with(cli.clone())
        .with(items.clone())
        .with(log.clone())
        .with(FakeClock::at(NOW, 480))
        .background_delivery();
    let app = with(builder).boot().await.unwrap();
    Rig {
        app,
        cli,
        items,
        log,
    }
}

/// 有焦点窗口的假窗口管理器。
async fn focused_on(app: &TestApp, windows: &FakeWindows, window: WindowInfo) {
    let _ = app;
    windows
        .focus(
            clock_contract::Moment {
                utc_ms: NOW,
                offset_min: 480,
            },
            Some(window),
        )
        .await
        .unwrap();
}

// ---------- 测试 ----------

#[tokio::test]
async fn saves_what_was_typed_with_the_context_of_that_moment() {
    let windows = FakeWindows::default();
    let window_ctx = TestProvider::new("ctx-window", "window", Phase::WhilePrompt)
        .giving(vec![entry("window.app", "zen")]);
    let shot = TestProvider::new("ctx-shot", "screenshot", Phase::BeforePrompt);
    let prompt = FakePrompt::types("看下 PR  #t @明天");
    let r = rig(
        |b| {
            b.with(windows.clone())
                .with(window_ctx.clone())
                .with(shot.clone())
                .with(prompt.clone())
        },
        json!({}),
    )
    .await;
    focused_on(&r.app, &windows, zen()).await;

    let out = r.capture(&[]).await.unwrap();
    assert!(out.contains("[待办]  @2026-11-19  看下 PR"), "{out}");

    let added = r.items.added();
    assert_eq!(added.len(), 1);
    let it = &added[0];
    assert_eq!(it.text, "看下 PR");
    assert_eq!(it.kind, Some(Kind::Todo));
    assert_eq!(it.due_on.as_deref(), Some("2026-11-19"));
    assert_eq!(it.source.as_deref(), Some("zen · PR #12 — Zen Browser"));
    // 按配置的默认顺序：window 在 screenshot 前面
    assert_eq!(
        it.context,
        [
            entry("window.app", "zen"),
            entry("screenshot.x", "screenshot")
        ]
    );
    // 提供者拿到的快照：窗口、按键时刻、将来的条目 id
    let seen = window_ctx.seen.lock().unwrap()[0].clone();
    assert_eq!(seen.window, Some(zen()));
    assert_eq!(seen.at.utc_ms, NOW);
    assert_eq!(it.id.as_deref(), Some(seen.item_id.as_str()));
    assert_eq!(seen.item_id.len(), 26);
    // 输入框：默认的提示，没有预填
    let asked = prompt.asked();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].placeholder.contains("#t"), "{:?}", asked[0]);
    assert!(window_ctx.discarded().is_empty() && shot.discarded().is_empty());
    r.app.stop().await;
}

#[tokio::test]
async fn escape_saves_nothing_and_providers_clean_up() {
    let shot = TestProvider::new("ctx-shot", "screenshot", Phase::BeforePrompt);
    let other = TestProvider::new("ctx-other", "other", Phase::WhilePrompt);
    for scenario in [
        Scenario::Escapes,
        Scenario::EntersNothing,
        Scenario::Types("#t @明天".into()),
    ] {
        let prompt = FakePrompt::new([scenario.clone()]);
        let r = rig(
            |b| b.with(shot.clone()).with(other.clone()).with(prompt),
            json!({}),
        )
        .await;
        assert_eq!(
            r.capture(&[]).await.unwrap(),
            "",
            "{scenario:?}：没事不输出"
        );
        assert!(r.items.added().is_empty(), "{scenario:?}");
        r.app.stop().await;
    }
    assert_eq!(shot.discarded().len(), 3, "每次取消都让提供者清理");
    assert_eq!(other.discarded().len(), 3);
}

#[tokio::test]
async fn slow_or_failing_providers_never_block_saving() {
    let slow = TestProvider::new("ctx-slow", "slow", Phase::WhilePrompt).slow(10_000);
    let broken =
        TestProvider::new("ctx-broken", "broken", Phase::WhilePrompt).failing("读不到会话文件");
    let slow_shot = TestProvider::new("ctx-shot", "screenshot", Phase::BeforePrompt).slow(10_000);
    let r = rig(
        |b| {
            b.with(slow)
                .with(broken)
                .with(slow_shot)
                .with(FakePrompt::types("记一下"))
        },
        json!({ "context_timeout_ms": 50, "before_prompt_timeout_ms": 50 }),
    )
    .await;
    let started = Instant::now();
    r.capture(&[]).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );

    let it = &r.items.added()[0];
    assert_eq!(it.text, "记一下");
    let notes: Vec<(&str, &str)> = it
        .context
        .iter()
        .map(|e| (e.key.as_str(), e.value.as_str()))
        .collect();
    // screenshot 在默认顺序里，排前面；其余按 id
    assert_eq!(
        notes,
        [
            ("capture.screenshot", "超时，没等到"),
            ("capture.broken", "出错：读不到会话文件"),
            ("capture.slow", "超时，没等到"),
        ]
    );
    let lines = r.log.lines();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("broken") && l.contains("读不到会话文件")),
        "失败也写日志：{lines:?}"
    );
    r.app.stop().await;
}

#[tokio::test]
async fn providers_work_while_the_user_is_typing() {
    // 提供者要 150 ms，用户打字 300 ms；回车后一点都不等（0 ms）也采到了
    let p = TestProvider::new("ctx-p", "p", Phase::WhilePrompt).slow(150);
    let r = rig(
        |b| b.with(p).with(SlowPrompt { ms: 300, text: "x" }),
        json!({ "context_timeout_ms": 0 }),
    )
    .await;
    r.capture(&[]).await.unwrap();
    assert_eq!(r.items.added()[0].context, [entry("p.x", "p")]);
    r.app.stop().await;
}

#[tokio::test]
async fn without_a_window_manager_it_still_saves() {
    let r = rig(|b| b.with(FakePrompt::types("x")), json!({})).await;
    r.capture(&[]).await.unwrap();
    let it = &r.items.added()[0];
    assert_eq!((it.text.as_str(), it.source.as_deref()), ("x", None));
    assert!(it.context.is_empty());
    r.app.stop().await;
}

#[tokio::test]
async fn a_provider_can_override_the_source_line() {
    let windows = FakeWindows::default();
    let term = TestProvider::new("ctx-term", "terminal", Phase::WhilePrompt).giving(vec![
        entry("source", "ghostty · ~/code/stash (main)"),
        entry("terminal.cwd", "~/code/stash"),
    ]);
    let r = rig(
        |b| {
            b.with(windows.clone())
                .with(term)
                .with(FakePrompt::types("x"))
        },
        json!({}),
    )
    .await;
    focused_on(&r.app, &windows, zen()).await;
    r.capture(&[]).await.unwrap();
    let it = &r.items.added()[0];
    assert_eq!(it.source.as_deref(), Some("ghostty · ~/code/stash (main)"));
    assert_eq!(it.context, [entry("terminal.cwd", "~/code/stash")]);
    r.app.stop().await;
}

#[tokio::test]
async fn expanders_run_in_order_then_the_builtin_rules() {
    let first = TestExpander {
        component: "exp-a",
        id: "a",
        does: Does::Replace(";t", "#t"),
    };
    let second = TestExpander {
        component: "exp-b",
        id: "b",
        does: Does::Replace("#t", "#i"),
    };
    let broken = TestExpander {
        component: "exp-broken",
        id: "broken",
        does: Does::Fail,
    };
    // 配置顺序 b 在 a 前：b 看不到 a 产生的 #t，所以结果是待办而不是想法
    let r = rig(
        |b| {
            b.with(first.clone())
                .with(second.clone())
                .with(broken.clone())
                .with(FakePrompt::types("回老王 ;t"))
        },
        json!({ "expander_order": ["broken", "b", "a"] }),
    )
    .await;
    r.capture(&[]).await.unwrap();
    let it = &r.items.added()[0];
    assert_eq!((it.text.as_str(), it.kind), ("回老王", Some(Kind::Todo)));
    let keys: Vec<&str> = it.context.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(keys, ["capture.expand.broken", "a.from"]);
    r.app.stop().await;
}

#[tokio::test]
async fn kind_flag_message_flag_and_tag_priority() {
    let prompt = FakePrompt::default();
    let r = rig(|b| b.with(prompt.clone()), json!({})).await;
    // --message：不弹框
    r.capture(&[("kind", "todo"), ("message", "一")])
        .await
        .unwrap();
    // 文字里的 # 比 --kind 优先
    r.capture(&[("kind", "todo"), ("message", "二 #i")])
        .await
        .unwrap();
    // 只有 URL：归到稍后读
    r.capture(&[("message", "https://example.com/a")])
        .await
        .unwrap();
    assert!(prompt.asked().is_empty());
    let kinds: Vec<Option<Kind>> = r.items.added().iter().map(|i| i.kind).collect();
    assert_eq!(
        kinds,
        [Some(Kind::Todo), Some(Kind::Idea), Some(Kind::ReadLater)]
    );
    assert_eq!(
        r.items.added()[2].url.as_deref(),
        Some("https://example.com/a")
    );
    r.app.stop().await;
}

#[tokio::test]
async fn the_kind_shows_in_the_prompt() {
    let prompt = FakePrompt::default();
    let r = rig(|b| b.with(prompt.clone()), json!({})).await;
    r.capture(&[("kind", "read")]).await.unwrap();
    assert!(prompt.asked()[0].placeholder.starts_with("→ 稍后读"));
    r.app.stop().await;
}

#[tokio::test]
async fn a_broken_prompt_is_an_error_and_nothing_is_kept() {
    #[derive(Clone)]
    struct Broken;
    #[async_trait]
    impl Prompt for Broken {
        async fn ask(&self, _: PromptRequest) -> Result<Answer, AppError> {
            Err(AppError::unavailable("启动 `vicinae` 失败"))
        }
    }
    impl Component for Broken {
        fn manifest(&self) -> Manifest {
            Manifest {
                id: "prompt-broken",
                kind: ComponentKind::Service,
                summary: "测试用：起不来的输入框",
                provides: vec![PROMPT.id()],
                requires: vec![],
                optional_requires: vec![],
            }
        }
        fn start(&self, _: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
            host.provide(&PROMPT, Arc::new(self.clone()))
        }
    }
    let shot = TestProvider::new("ctx-shot", "screenshot", Phase::BeforePrompt);
    let r = rig(|b| b.with(Broken).with(shot.clone()), json!({})).await;
    let err = r.capture(&[]).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Unavailable);
    assert!(r.items.added().is_empty());
    assert_eq!(shot.discarded().len(), 1);
    r.app.stop().await;
}

#[tokio::test]
async fn add_and_parse_use_the_same_syntax_without_context() {
    let ctx = TestProvider::new("ctx-window", "window", Phase::WhilePrompt);
    let prompt = FakePrompt::default();
    let r = rig(|b| b.with(ctx.clone()).with(prompt.clone()), json!({})).await;

    let out = r
        .cli
        .invoke("add", &[("words", "交周报 @周五 #t")], Format::Json)
        .await
        .unwrap()
        .text;
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(
        (v["kind"].as_str(), v["due_on"].as_str()),
        (Some("todo"), Some("2026-11-20"))
    );
    // --due 和文字里的 @：文字优先
    r.cli
        .invoke("add", &[("words", "x @明天"), ("due", "2026-12-31")], TEXT)
        .await
        .unwrap();
    let added = r.items.added();
    assert_eq!(added[1].due_on.as_deref(), Some("2026-11-19"));
    assert!(
        added
            .iter()
            .all(|i| i.context.is_empty() && i.source.is_none() && i.id.is_none())
    );
    assert!(
        ctx.seen.lock().unwrap().is_empty() && prompt.asked().is_empty(),
        "add 不采上下文、不弹框"
    );

    let err = r
        .cli
        .invoke("add", &[("words", "#t @明天")], TEXT)
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Validation);

    let parsed = r
        .cli
        .invoke(
            "parse",
            &[("words", "https://example.com @11-20")],
            Format::Json,
        )
        .await
        .unwrap()
        .text;
    let d: Draft = serde_json::from_str(parsed.trim()).unwrap();
    assert_eq!(d.kind, Some(Kind::ReadLater));
    assert_eq!(d.due_on.as_deref(), Some("2026-11-20"));
    let text = r
        .cli
        .invoke("parse", &[("words", "学 #C++")], TEXT)
        .await
        .unwrap()
        .text;
    assert!(
        text.contains("文字  学 #C++") && text.contains("收件箱"),
        "{text}"
    );
    assert_eq!(r.items.added().len(), 2, "parse 不保存");
    r.app.stop().await;
}

#[tokio::test]
async fn bad_config_fails_at_startup() {
    for config in [
        json!({ "tags": { "todo": ["t"], "idea": ["t"] } }),
        json!({ "url_kind": "someday" }),
        json!({ "colour": "red" }),
    ] {
        let err = testkit::app()
            .with(CaptureComponent)
            .config("capture", config.clone())
            .unwrap()
            .with(FakeCli::default())
            .with(RecordingItems::default())
            .with(MemoryLog::default())
            .with(FakePrompt::default())
            .with(FakeClock::at(NOW, 480))
            .boot()
            .await
            .err()
            .unwrap();
        assert_eq!(err.kind, ErrorKind::Config, "{config}");
    }
}

/// 订阅 `capture.saved`，记下收到的。
#[derive(Clone, Default)]
struct SavedProbe(Arc<Mutex<Vec<capture_contracts::Saved>>>);

impl SavedProbe {
    fn got(&self) -> Vec<capture_contracts::Saved> {
        self.0.lock().unwrap().clone()
    }
}

impl kernel_api::Component for SavedProbe {
    fn manifest(&self) -> kernel_api::Manifest {
        kernel_api::Manifest {
            id: "saved-probe",
            kind: kernel_api::Kind::Feature,
            summary: "测试用：记下 capture.saved",
            provides: vec![],
            requires: vec![capture_contracts::CAPTURE.id()],
            optional_requires: vec![],
        }
    }

    fn start(
        &self,
        _: &kernel_api::ConfigSection,
        host: &mut kernel_api::Host<'_>,
    ) -> Result<(), AppError> {
        let seen = self.0.clone();
        host.subscribe(&capture_contracts::CAPTURE_SAVED, "probe", move |e| {
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(e);
                Ok(())
            }
        })
    }
}

#[tokio::test]
async fn saving_announces_the_item_and_its_routes() {
    let probe = SavedProbe::default();
    let r = rig(
        |b| {
            b.with(probe.clone())
                .with(FakePrompt::types("看 PR #滴 #t"))
        },
        json!({ "routes": { "dida": ["滴"] } }),
    )
    .await;
    r.capture(&[]).await.unwrap();
    r.cli
        .invoke(
            "add",
            &[("words", "买菜"), ("to", "dida, mail")],
            Format::Json,
        )
        .await
        .unwrap();
    r.cli
        .invoke("add", &[("words", "普通的")], Format::Json)
        .await
        .unwrap();
    r.app.settle().await;
    let added = r.items.added();
    assert_eq!(added[0].text, "看 PR", "#滴 从文字里去掉");
    let got = probe.got();
    assert_eq!(got.len(), 3, "每存一条都发");
    assert_eq!(Some(&got[0].item_id), added[0].id.as_ref(), "带着条目 id");
    assert_eq!(got[0].routes, ["dida"]);
    assert_eq!(got[1].routes, ["dida", "mail"], "--to 可以写几个");
    assert!(got[2].routes.is_empty());
}

#[tokio::test]
async fn cancelled_captures_announce_nothing() {
    let probe = SavedProbe::default();
    let r = rig(
        |b| {
            b.with(probe.clone())
                .with(FakePrompt::new([Scenario::Escapes]))
        },
        json!({ "routes": { "dida": ["滴"] } }),
    )
    .await;
    r.capture(&[("to", "dida")]).await.unwrap();
    r.app.settle().await;
    assert!(probe.got().is_empty());
}

#[tokio::test]
async fn a_route_tag_clashing_with_a_kind_tag_fails_at_startup() {
    let err = testkit::app()
        .with(CaptureComponent)
        .config("capture", json!({ "routes": { "dida": ["t"] } }))
        .unwrap()
        .with(FakeCli::default())
        .with(RecordingItems::default())
        .with(MemoryLog::default())
        .with(FakeClock::at(NOW, 480))
        .with(FakePrompt::default())
        .boot()
        .await
        .err()
        .unwrap();
    assert_eq!(err.kind, kernel_api::ErrorKind::Config, "{err:?}");
}
