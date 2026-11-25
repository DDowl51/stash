//! 记录的流程（设计 §5.2）：
//!
//! 1. 拍窗口快照（输入框一出现焦点就变了，所以最先做）；
//! 2. 跑「弹框之前」的提供者，等它们，有上限；
//! 3. 弹输入框，同时在后台跑「弹框期间」的提供者；
//! 4. 取消：停掉提供者，让它们清理留下的东西，不保存；
//! 5. 回车：展开 → 内置规则 → 最多再等提供者一会儿 → 写入。
//!
//! 任何提供者、展开器失败都不影响保存，失败记成一条上下文。

use std::sync::Arc;
use std::time::Duration;

use capture_contracts::{
    ContextEntry, ContextProvider, Draft, Expander, Kind, Phase, Saved, Snapshot, WindowInfo,
};
use capture_core::{
    Outcome, Rules, expander_failed, local_date, ordered, settle, source_line, split_source,
};
use clock_contract::{Clock, Moment};
use items_contracts::{Item, Items, NewItem};
use kernel_api::{AppError, Collector, Publisher};
use log_contract::Logger;
use prompt_contract::{Answer, Prompt, PromptRequest};
use tokio::task::JoinSet;
use tokio::time::{Instant, timeout, timeout_at};
use window_contract::Windows;

use crate::Config;

pub(crate) struct Capture {
    pub items: Arc<dyn Items>,
    pub prompt: Arc<dyn Prompt>,
    pub clock: Arc<dyn Clock>,
    pub windows: Option<Arc<dyn Windows>>,
    pub providers: Collector<dyn ContextProvider>,
    pub expanders: Collector<dyn Expander>,
    pub log: Logger,
    pub saved: Publisher<Saved>,
    pub rules: Rules,
    pub config: Config,
}

type Provider = Arc<dyn ContextProvider>;

/// 后台跑着的提供者。
struct Running {
    set: JoinSet<(String, Outcome)>,
    pending: Vec<String>,
}

impl Running {
    fn start(jobs: &[(String, Provider)], snapshot: &Snapshot) -> Self {
        let mut set = JoinSet::new();
        for (id, provider) in jobs {
            let (id, provider, snapshot) = (id.clone(), provider.clone(), snapshot.clone());
            set.spawn(async move {
                let outcome = match provider.collect(snapshot).await {
                    Ok(entries) => Outcome::Collected(entries),
                    Err(e) => Outcome::Failed(e.message),
                };
                (id, outcome)
            });
        }
        Self {
            set,
            pending: jobs.iter().map(|(id, _)| id.clone()).collect(),
        }
    }

    /// 等到 `deadline`，没做完的算超时并停掉。
    async fn finish(mut self, deadline: Instant) -> Vec<(String, Outcome)> {
        let mut done = Vec::new();
        while let Ok(Some(joined)) = timeout_at(deadline, self.set.join_next()).await {
            // 提供者 panic 了：它的 id 还在 pending 里，算超时
            if let Ok((id, outcome)) = joined {
                self.pending.retain(|p| *p != id);
                done.push((id, outcome));
            }
        }
        self.set.abort_all();
        done.extend(self.pending.into_iter().map(|id| (id, Outcome::TimedOut)));
        done
    }
}

/// 条目的 id：时间部分是按键那一刻，随机部分来自系统。
fn new_id(at: Moment) -> Result<String, AppError> {
    let mut random = [0u8; 16];
    getrandom::fill(&mut random)
        .map_err(|e| AppError::unavailable(format!("取不到随机数：{e}")))?;
    let ms = u64::try_from(at.utc_ms).map_err(|_| AppError::validation("时钟早于 1970 年"))?;
    Ok(ulid::Ulid::from_parts(ms, u128::from_le_bytes(random)).to_string())
}

impl Capture {
    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    /// 第 1 步：当时的窗口。拿不到不算错，记一条说明。
    async fn window(&self) -> (Option<WindowInfo>, Vec<ContextEntry>) {
        let Some(windows) = &self.windows else {
            return (None, Vec::new());
        };
        let outcome =
            match timeout(Self::ms(self.config.window_timeout_ms), windows.focused()).await {
                Ok(Ok(window)) => return (window, Vec::new()),
                Ok(Err(e)) => Outcome::Failed(e.message),
                Err(_) => Outcome::TimedOut,
            };
        self.warn_about("window-manager", &outcome).await;
        (None, settle("window-manager", outcome))
    }

    /// 所有提供者，按配置的顺序，分成两个阶段。问不出 spec 的记一条说明。
    async fn providers(
        &self,
    ) -> (
        Vec<(String, Provider)>,
        Vec<(String, Provider)>,
        Vec<ContextEntry>,
    ) {
        let mut known = Vec::new();
        let mut notes = Vec::new();
        for provider in self.providers.all() {
            match provider.spec().await {
                Ok(spec) => known.push((spec.id, (spec.phase, provider))),
                Err(e) => {
                    let outcome = Outcome::Failed(e.message);
                    self.warn_about("?", &outcome).await;
                    notes.extend(settle("unknown", outcome));
                }
            }
        }
        let (mut before, mut during) = (Vec::new(), Vec::new());
        for (id, (phase, provider)) in ordered(known, &self.config.context_order) {
            match phase {
                Phase::BeforePrompt => before.push((id, provider)),
                Phase::WhilePrompt => during.push((id, provider)),
            }
        }
        (before, during, notes)
    }

    async fn warn_about(&self, id: &str, outcome: &Outcome) {
        match outcome {
            Outcome::Failed(why) => self.log.warn(format!("上下文 {id} 出错：{why}")).await,
            Outcome::TimedOut => self.log.warn(format!("上下文 {id} 超时，丢掉了")).await,
            Outcome::Collected(_) => {}
        }
    }

    async fn settle_all(
        &self,
        results: Vec<(String, Outcome)>,
    ) -> Vec<(String, Vec<ContextEntry>)> {
        let mut out = Vec::new();
        for (id, outcome) in results {
            self.warn_about(&id, &outcome).await;
            out.push((id.clone(), settle(&id, outcome)));
        }
        out
    }

    /// 取消了：让所有提供者清理。清理失败只记日志。
    async fn discard(&self, providers: &[(String, Provider)], snapshot: &Snapshot) {
        for (id, provider) in providers {
            if let Err(e) = provider.discard(snapshot.clone()).await {
                self.log.warn(format!("上下文 {id} 清理失败：{e}")).await;
            }
        }
    }

    /// 展开 + 内置规则。展开器出错时按展开前的继续，并记一条。
    pub(crate) async fn shape(&self, draft: Draft, at: Moment) -> Result<Draft, AppError> {
        let mut known = Vec::new();
        let mut draft = draft;
        for expander in self.expanders.all() {
            match expander.spec().await {
                Ok(spec) => known.push((spec.id, expander)),
                Err(e) => draft.context.push(expander_failed("?", &e.message)),
            }
        }
        for (id, expander) in ordered(known, &self.config.expander_order) {
            match expander.expand(draft.clone()).await {
                Ok(expanded) => draft = expanded,
                Err(e) => {
                    self.log
                        .warn(format!("展开 {id} 出错，按原文保存：{e}"))
                        .await;
                    draft.context.push(expander_failed(&id, &e.message));
                }
            }
        }
        Ok(self.rules.apply(draft, local_date(at)?))
    }

    /// 只展开和解析，不保存。
    pub(crate) async fn parse(&self, draft: Draft) -> Result<Draft, AppError> {
        let at = self.clock.now().await?;
        self.shape(draft, at).await
    }

    /// 存好了：发 `capture.saved`。发不出去只记日志，条目已经存下了。
    async fn announce(&self, item: &Item, routes: Vec<String>) {
        let event = Saved {
            item_id: item.id.clone(),
            routes,
        };
        if let Err(e) = self.saved.publish(event).await {
            self.log
                .warn(format!("「记好了」的通知没发出去：{e}"))
                .await;
        }
    }

    /// 不弹框、不采上下文，按同样的语法记一条。去掉标记后什么都不剩是 `validation`。
    pub(crate) async fn add(&self, draft: Draft) -> Result<Item, AppError> {
        let draft = self.parse(draft).await?;
        if draft.text.is_empty() {
            return Err(AppError::validation("除了 # 和 @ 标记什么都没写"));
        }
        let item = self
            .items
            .add(NewItem {
                text: draft.text,
                url: draft.url,
                kind: draft.kind,
                due_on: draft.due_on,
                context: draft.context,
                ..NewItem::default()
            })
            .await?;
        self.announce(&item, draft.routes).await;
        Ok(item)
    }

    /// 完整的记录流程。`message` 有值时不弹框。取消了返回 `None`。
    pub(crate) async fn capture(
        &self,
        kind: Option<Kind>,
        message: Option<String>,
        routes: Vec<String>,
    ) -> Result<Option<Item>, AppError> {
        // 1. 按键那一刻
        let at = self.clock.now().await?;
        let (window, mut notes) = self.window().await;
        let snapshot = Snapshot {
            at,
            window: window.clone(),
            item_id: new_id(at)?,
        };
        let (before, during, spec_notes) = self.providers().await;
        notes.extend(spec_notes);

        // 2. 弹框之前
        let before_results = Running::start(&before, &snapshot)
            .finish(Instant::now() + Self::ms(self.config.before_prompt_timeout_ms))
            .await;

        // 3. 弹框，同时采集
        let mut running = Running::start(&during, &snapshot);
        let answer = match message {
            Some(text) => Ok(Answer::from_raw(&text)),
            None => {
                let placeholder = match kind {
                    Some(k) => format!("→ {}  {}", crate::kind_label(k), self.config.placeholder),
                    None => self.config.placeholder.clone(),
                };
                self.prompt
                    .ask(PromptRequest {
                        placeholder,
                        initial: String::new(),
                    })
                    .await
            }
        };
        let all: Vec<(String, Provider)> = before.iter().chain(&during).cloned().collect();
        let text = match answer {
            Ok(Answer::Text(text)) => text,
            // 4. 取消（或者输入框坏了）：什么都不留
            Ok(Answer::Cancelled) => {
                running.set.abort_all();
                self.discard(&all, &snapshot).await;
                return Ok(None);
            }
            Err(e) => {
                running.set.abort_all();
                self.discard(&all, &snapshot).await;
                return Err(e);
            }
        };

        // 5. 展开、规则
        let draft = self
            .shape(
                Draft {
                    text,
                    kind,
                    routes,
                    ..Draft::default()
                },
                at,
            )
            .await?;
        if draft.text.is_empty() {
            running.set.abort_all();
            self.discard(&all, &snapshot).await;
            return Ok(None);
        }
        let during_results = running
            .finish(Instant::now() + Self::ms(self.config.context_timeout_ms))
            .await;

        // 按配置的顺序拼上下文：提供者的、拿不到窗口的说明、展开时追加的
        let mut by_provider = self.settle_all(before_results).await;
        by_provider.extend(self.settle_all(during_results).await);
        let mut context: Vec<ContextEntry> = ordered(by_provider, &self.config.context_order)
            .into_iter()
            .flat_map(|(_, entries)| entries)
            .collect();
        context.extend(notes);
        context.extend(draft.context);
        let (given_source, context) = split_source(context);
        let routes = draft.routes;

        let item = self
            .items
            .add(NewItem {
                id: Some(snapshot.item_id.clone()),
                text: draft.text,
                url: draft.url,
                kind: draft.kind,
                due_on: draft.due_on,
                source: source_line(given_source, window.as_ref()),
                context,
            })
            .await;
        match item {
            Ok(item) => {
                self.announce(&item, routes).await;
                Ok(Some(item))
            }
            Err(e) => {
                // 没写进去：提供者留下的东西（截图）也不该留着
                self.discard(&all, &snapshot).await;
                Err(e)
            }
        }
    }
}

/// `capture` 契约：只是把 `parse` 交出去。
pub(crate) struct Service(pub Arc<Capture>);

#[kernel_api::async_trait]
impl capture_contracts::Capture for Service {
    async fn parse(&self, draft: Draft) -> Result<Draft, AppError> {
        self.0.parse(draft).await
    }
}
