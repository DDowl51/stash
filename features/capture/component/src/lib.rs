//! 应用功能 `capture`：按快捷键那一刻拍下当时的窗口 → 并发采集上下文 → 弹输入框 →
//! 展开 → 写成一个条目。登记 `stash capture`、`stash add`、`stash parse`。
//!
//! 这里只管流程：什么时候采、等多久、失败了怎么办（设计 §5.2）。
//! 具体采什么、怎么展开都在扩展点的登记者里；语法和取舍规则在 `capture-core` 里。

mod commands;
mod flow;

use std::collections::BTreeMap;
use std::sync::Arc;

use capture_contracts::{CAPTURE, CAPTURE_CONTEXT, CAPTURE_EXPANDERS, CAPTURE_SAVED, Kind};
use capture_core::Rules;
use cli_contract::CLI_COMMANDS;
use clock_contract::CLOCK;
use items_contracts::ITEMS;
use kernel_api::{
    AppError, Component, ConfigSection, Host, Kind as ComponentKind, Manifest, register_component,
};
use log_contract::{LOG, Logger};
use prompt_contract::PROMPT;
use serde::{Deserialize, Serialize};
use window_contract::WINDOWS;

use crate::flow::Capture;

/// 给人看的分类名。
pub(crate) const fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Inbox => "收件箱",
        Kind::Todo => "待办",
        Kind::Idea => "想法",
        Kind::ReadLater => "稍后读",
    }
}

/// 分类标记：`#` 后面的词 → 分类。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
struct Tags {
    inbox: Vec<String>,
    todo: Vec<String>,
    idea: Vec<String>,
    read_later: Vec<String>,
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|w| (*w).to_owned()).collect()
}

impl Default for Tags {
    fn default() -> Self {
        Self {
            inbox: words(&["inbox", "in"]),
            todo: words(&["todo", "t", "待办"]),
            idea: words(&["idea", "i", "想法"]),
            read_later: words(&["read", "r", "读"]),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
struct Config {
    /// 输入框里的提示。
    placeholder: String,
    /// 整段只有一个 URL、又没写 `#` 时归到哪类。
    url_kind: Kind,
    /// 拍窗口快照最多等多久。超时就当没有窗口信息。
    window_timeout_ms: u64,
    /// 弹框之前的提供者（截图）最多等多久。
    before_prompt_timeout_ms: u64,
    /// 回车之后最多再等上下文多久，超时的那一项丢掉。
    context_timeout_ms: u64,
    /// 上下文在条目里的顺序（提供者的 id）。没列的按 id 排在后面。
    context_order: Vec<String>,
    /// 展开器的执行顺序（展开器的 id）。没列的按 id 排在后面。
    expander_order: Vec<String>,
    tags: Tags,
    /// 路由名 → 标记：`#标记` 表示这一条存好后还要送去那里（由订阅 `capture.saved` 的组件去送）。
    /// `capture` 不认识任何路由；有哪些路由由启动器或配置给出。
    routes: BTreeMap<String, Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            placeholder: "记点什么…  #t 待办  #i 想法  #r 稍后读  @明天".to_owned(),
            url_kind: Kind::ReadLater,
            window_timeout_ms: 200,
            before_prompt_timeout_ms: 300,
            context_timeout_ms: 1_000,
            context_order: words(&["window", "terminal", "browser", "screenshot", "script"]),
            expander_order: words(&["calc", "snippets", "script"]),
            tags: Tags::default(),
            routes: BTreeMap::new(),
        }
    }
}

impl Config {
    fn rules(&self) -> Result<Rules, AppError> {
        let t = &self.tags;
        Rules::new(
            [
                (Kind::Inbox, t.inbox.clone()),
                (Kind::Todo, t.todo.clone()),
                (Kind::Idea, t.idea.clone()),
                (Kind::ReadLater, t.read_later.clone()),
            ],
            self.url_kind,
        )?
        .with_routes(self.routes.clone())
    }
}

pub struct CaptureComponent;

impl Component for CaptureComponent {
    fn config_defaults(&self) -> Option<ConfigSection> {
        ConfigSection::defaults::<Config>()
    }

    fn manifest(&self) -> Manifest {
        Manifest {
            id: "capture",
            kind: ComponentKind::Feature,
            summary: "快速记录：拍下当时的窗口、采集上下文、弹输入框、写入",
            provides: vec![CAPTURE.id(), CAPTURE_CONTEXT.id(), CAPTURE_EXPANDERS.id()],
            requires: vec![
                ITEMS.id(),
                PROMPT.id(),
                CLOCK.id(),
                LOG.id(),
                CLI_COMMANDS.id(),
            ],
            // 不在 niri 会话里（比如 ssh 上来）照样能记，只是没有来源窗口
            optional_requires: vec![WINDOWS.id()],
        }
    }

    fn start(&self, config: &ConfigSection, host: &mut Host<'_>) -> Result<(), AppError> {
        let config: Config = config.parse()?;
        let rules = config.rules()?;
        let capture = Arc::new(Capture {
            items: host.resolve(&ITEMS)?,
            prompt: host.resolve(&PROMPT)?,
            clock: host.resolve(&CLOCK)?,
            windows: host.try_resolve(&WINDOWS)?,
            providers: host.collector(&CAPTURE_CONTEXT)?,
            expanders: host.collector(&CAPTURE_EXPANDERS)?,
            log: Logger::new(host.resolve(&LOG)?, host.id()),
            saved: host.publisher(&CAPTURE_SAVED)?,
            rules,
            config,
        });
        host.provide(&CAPTURE, Arc::new(flow::Service(capture.clone())))?;
        host.contribute(&CLI_COMMANDS, Arc::new(commands::Commands(capture)))
    }
}

register_component!(CaptureComponent);
