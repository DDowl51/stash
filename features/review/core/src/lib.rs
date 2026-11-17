//! `review` 的纯逻辑。不碰条目存储、时钟、终端。
//!
//! - [`Page`]：五个页面各列哪些条目、怎么排、有哪些按键；
//! - [`row`]：一个条目在列表里的一行和详情；
//! - [`neighbour`]：`J` / `K` 挪位置时排到谁前面；
//! - [`due_input`]：`D` 输入的截止日期。

mod due;
mod page;
mod row;

pub use due::{due_input, local_date, today};
pub use page::{Op, Page, neighbour};
pub use row::{context_to_open, row, short_id};
