//! `capture` 的纯逻辑。不碰时钟、输入框、存储。
//!
//! - [`Rules::apply`]：内置的 `#分类`、`@日期`、URL 规则（设计 §5.1）；
//! - [`due_word`]：`@` 后面的日期写法；
//! - [`source_line`]：条目的一行来源摘要；
//! - [`settle`]、[`ordered`]：上下文提供者的结果怎么落到条目上。

mod due;
mod rules;
mod settle;

pub use due::{due_word, local_date};
pub use rules::Rules;
pub use settle::{Outcome, expander_failed, ordered, settle, source_line, split_source};
