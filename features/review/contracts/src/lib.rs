//! 契约：整理界面（`review`）的扩展点 `review.actions`。
//!
//! 别的组件给条目加按键（比如「推到滴答」）：声明键和说明，按下时收到选中条目的 id。
//! `review` 只负责把键并进每一页的帮助行、把按键转过去；键的意思只有登记的组件知道。
//! 返回的是外壳的 [`ActionResult`]（一句话、重画、要输入、打开链接），`review` 原样交给外壳。

use kernel_api::{AppError, contract};
use serde::{Deserialize, Serialize};
pub use tui_contract::{ActionResult, Ask};

/// 外壳自己用的键，登记了也收不到。
pub const RESERVED: [char; 8] = ['q', '/', 'j', 'k', 'h', 'l', '.', 'r'];

/// 一个按键。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemActionSpec {
    pub key: char,
    /// 写在帮助行里，比如「推到滴答」。
    pub label: String,
    /// 要再按一次才执行。
    #[serde(default)]
    pub confirm: bool,
    /// 已完成页也显示。默认只在收件箱、待办、想法、稍后读四页。
    #[serde(default)]
    pub on_done_page: bool,
}

/// 按下了。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemActionRequest {
    pub key: char,
    /// 选中的条目。
    pub item_id: String,
    /// 上一次返回 `ask` 时，用户提交的文字。
    #[serde(default)]
    pub input: Option<String>,
}

contract! {
    /// 扩展点：给整理界面里的条目加按键。
    pub trait ItemActions as REVIEW_ACTIONS = point("review.actions", 1) {
        async fn actions(&self) -> Result<Vec<ItemActionSpec>, AppError>;

        /// 不是自己声明的键是 `validation`。
        async fn act(&self, request: ItemActionRequest) -> Result<ActionResult, AppError>;
    }
}

#[cfg(feature = "testing")]
pub mod testing {
    //! 契约测试：任何登记 `review.actions` 的组件都要通过。

    use std::sync::Arc;

    use kernel_api::{AppError, ErrorKind};

    use super::{ItemActionRequest, ItemActions, RESERVED};

    fn fail(msg: impl Into<String>) -> AppError {
        AppError::validation(format!("违反 review.actions 契约：{}", msg.into()))
    }

    /// `item_id` 是一个存在的条目。
    pub async fn item_actions_contract(
        actions: Arc<dyn ItemActions>,
        item_id: &str,
    ) -> Result<(), AppError> {
        let specs = actions.actions().await?;
        if specs.is_empty() {
            return Err(fail("至少声明一个键"));
        }
        let mut keys: Vec<char> = specs.iter().map(|s| s.key).collect();
        keys.sort_unstable();
        keys.dedup();
        if keys.len() != specs.len() {
            return Err(fail("键不能重复"));
        }
        if let Some(s) = specs.iter().find(|s| RESERVED.contains(&s.key)) {
            return Err(fail(format!("键 `{}` 是外壳自己用的", s.key)));
        }
        if specs.iter().any(|s| s.label.trim().is_empty()) {
            return Err(fail("每个键要有说明"));
        }
        let unknown = ('A'..='Z')
            .chain('a'..='z')
            .find(|c| !keys.contains(c))
            .unwrap_or('~');
        let r = actions
            .act(ItemActionRequest {
                key: unknown,
                item_id: item_id.to_owned(),
                input: None,
            })
            .await;
        match r {
            Err(e) if e.kind == ErrorKind::Validation => Ok(()),
            other => Err(fail(format!(
                "没声明的键应该是 validation，实际是 {other:?}"
            ))),
        }
    }
}
