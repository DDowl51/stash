//! `capture` 的两个扩展点。
//!
//! - [`CAPTURE_CONTEXT`]：按快捷键那一刻「当时在干什么」。每种来源一个 `context-*` 组件来登记，
//!   `capture` 只管什么时候采、等多久、失败了怎么办，不认识任何具体来源。
//! - [`CAPTURE_EXPANDERS`]：回车之后改写输入（算式、缩写、外部命令）。每种一个 `expand-*` 组件。

use clock_contract::Moment;
use kernel_api::{AppError, contract};
use serde::{Deserialize, Serialize};

pub use items_contracts::{ContextEntry, ContextKind, Kind};
pub use window_contract::WindowInfo;

/// 按下快捷键那一刻。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub at: Moment,
    /// 当时有焦点的窗口。窗口管理器不可用、或者没有窗口有焦点时是 `None`。
    pub window: Option<WindowInfo>,
    /// 这次记录将来的条目 id。提供者用它给自己留下的东西（比如截图文件）起名。
    pub item_id: String,
}

/// 提供者什么时候运行。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// 输入框出现之前，等它做完再弹框（有上限）。给截图这类会被输入框挡住的。
    BeforePrompt,
    /// 和输入框同时进行，用户打字的时间就是它的预算。
    WhilePrompt,
}

/// 提供者的自我介绍。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSpec {
    /// 短名字，比如 `window`。它给的条目的 `key` 都以 `<id>.` 开头（`source` 除外），
    /// 配置里排顺序、出错时的说明也用它。
    pub id: String,
    pub phase: Phase,
}

/// 条目的 `key` 等于它时，`value` 用作条目的一行来源摘要（覆盖默认的「应用 · 标题」）。
pub const SOURCE_KEY: &str = "source";

contract! {
    /// 扩展点：采集上下文。
    pub trait ContextProvider as CAPTURE_CONTEXT = point("capture.context", 1) {
        async fn spec(&self) -> Result<ContextSpec, AppError>;

        /// 采集。不适用（比如来源不是终端）就返回空的。
        /// 出错不影响保存：`capture` 会把错误记成一条上下文。
        async fn collect(&self, snapshot: Snapshot) -> Result<Vec<ContextEntry>, AppError>;

        /// 这次记录取消了：删掉为它留下的东西（比如截图文件）。什么都没留下的提供者直接返回。
        async fn discard(&self, snapshot: Snapshot) -> Result<(), AppError>;
    }
}

/// 正在成形的条目。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Draft {
    pub text: String,
    #[serde(default)]
    pub kind: Option<Kind>,
    /// `YYYY-MM-DD`。
    #[serde(default)]
    pub due_on: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    /// 展开时追加的上下文（比如原来的算式）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<ContextEntry>,
}

/// 展开器的自我介绍。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpanderSpec {
    /// 短名字，比如 `calc`。配置里排顺序、出错时的说明用它。
    pub id: String,
}

contract! {
    /// 扩展点：展开输入。
    pub trait Expander as CAPTURE_EXPANDERS = point("capture.expanders", 1) {
        async fn spec(&self) -> Result<ExpanderSpec, AppError>;

        /// 改写草稿。不处理就原样返回。出错时 `capture` 按展开前的样子继续，并记一条上下文。
        async fn expand(&self, draft: Draft) -> Result<Draft, AppError>;
    }
}

#[cfg(feature = "testing")]
pub mod testing {
    //! 契约测试：任何提供者 / 展开器都要通过。

    use std::sync::{Arc, OnceLock};

    use clock_contract::Moment;
    use kernel_api::{AppError, Collector, Component, ConfigSection, Host, Kind, Manifest};

    use super::{
        CAPTURE_CONTEXT, CAPTURE_EXPANDERS, ContextProvider, Draft, Expander, SOURCE_KEY, Snapshot,
        WindowInfo,
    };

    /// 组件 `capture-probe`：两个扩展点的假所有者。登记方的启动测试里用它代替 `capture`，
    /// 并取出登记上来的实现。克隆出来的是同一个。
    #[derive(Clone, Default)]
    pub struct CaptureProbe {
        context: Arc<OnceLock<Collector<dyn ContextProvider>>>,
        expanders: Arc<OnceLock<Collector<dyn Expander>>>,
    }

    impl CaptureProbe {
        pub fn providers(&self) -> Vec<Arc<dyn ContextProvider>> {
            self.context.get().map(Collector::all).unwrap_or_default()
        }

        pub fn expanders(&self) -> Vec<Arc<dyn Expander>> {
            self.expanders.get().map(Collector::all).unwrap_or_default()
        }
    }

    impl Component for CaptureProbe {
        fn manifest(&self) -> Manifest {
            Manifest {
                id: "capture-probe",
                kind: Kind::Feature,
                summary: "测试用：capture 两个扩展点的假所有者",
                provides: vec![CAPTURE_CONTEXT.id(), CAPTURE_EXPANDERS.id()],
                requires: vec![],
                optional_requires: vec![],
            }
        }

        fn start(&self, _config: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
            let _ = self.context.set(host.collector(&CAPTURE_CONTEXT)?);
            let _ = self.expanders.set(host.collector(&CAPTURE_EXPANDERS)?);
            Ok(())
        }
    }

    fn check(ok: bool, what: &str) -> Result<(), AppError> {
        if ok {
            Ok(())
        } else {
            Err(AppError::validation(format!("契约不满足：{what}")))
        }
    }

    /// 造一个快照。
    pub fn snapshot(window: Option<WindowInfo>) -> Snapshot {
        Snapshot {
            at: Moment {
                utc_ms: 1_794_933_000_000,
                offset_min: 480,
            },
            window,
            item_id: "01JD8X0000000000000000ABCD".to_owned(),
        }
    }

    /// `window` 是一个这个提供者会处理的窗口。
    pub async fn context_contract(
        provider: Arc<dyn ContextProvider>,
        window: WindowInfo,
    ) -> Result<(), AppError> {
        let spec = provider.spec().await?;
        check(
            !spec.id.is_empty() && !spec.id.contains('.'),
            "id 不空、不带点",
        )?;
        let prefix = format!("{}.", spec.id);
        let entries = provider.collect(snapshot(Some(window))).await?;
        for e in &entries {
            check(
                e.key == SOURCE_KEY || e.key.starts_with(&prefix),
                &format!("条目的 key `{}` 要以 `{prefix}` 开头", e.key),
            )?;
            check(!e.label.is_empty(), "条目要有标签")?;
        }
        // 没有窗口时不出错（窗口管理器不可用是常事）
        provider.collect(snapshot(None)).await?;
        provider.discard(snapshot(None)).await?;
        Ok(())
    }

    /// `untouched` 是这个展开器不该处理的文字。
    pub async fn expander_contract(
        expander: Arc<dyn Expander>,
        untouched: &str,
    ) -> Result<(), AppError> {
        let spec = expander.spec().await?;
        check(!spec.id.is_empty(), "id 不空")?;
        let draft = Draft {
            text: untouched.to_owned(),
            ..Draft::default()
        };
        check(
            expander.expand(draft.clone()).await? == draft,
            "不处理的文字原样返回",
        )
    }
}
