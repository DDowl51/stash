//! `capture` 的纯逻辑。不碰时钟、输入框、存储。
//!
//! - [`Rules::apply`]：内置的 `#分类`、`@日期`、URL 规则（设计 §5.1）；
//! - `@` 后面的日期写法在 mk-kit 的 `zh-dates`；
//! - [`source_line`]：条目的一行来源摘要；
//! - [`settle`]、[`ordered`]：上下文提供者的结果怎么落到条目上。

mod rules;
mod settle;

/// 这一刻在本地是哪天。
pub fn local_date(at: clock_contract::Moment) -> Result<jiff::civil::Date, kernel_api::AppError> {
    zh_dates::local_date(at.utc_ms, at.offset_min)
}
pub use rules::Rules;
pub use settle::{Outcome, expander_failed, ordered, settle, source_line, split_source};
