//! 命令行：`stash ls / show / move / done / archive / reopen / rm / due / edit`。
//! （新建条目的 `stash add` 在 `capture` 里：它和弹框记录共用 `#`、`@` 语法。）
//!
//! 只把命令翻译成 `Items` 的请求、把结果排成文本或 JSON；不碰终端、不管退出码（那是 `cli` 服务的事）。
//! 修改类命令成功时什么都不输出。

use std::sync::Arc;

use cli_contract::{ArgKind, ArgSpec, CommandProvider, CommandSpec, Format, Invocation, Output};
use items_contracts::{DueChange, DueFilter, Item, ItemFilter, ItemPatch, Items, Kind, Status};
use kernel_api::{AppError, async_trait};

use crate::Service;

pub(crate) struct Commands(pub Arc<Service>);

/// 列表里显示的短 id：ULID 的最后 6 位（随机部分，不容易撞）。
fn short(id: &str) -> &str {
    &id[id.len().saturating_sub(6)..]
}

fn arg(name: &str, help: &str, kind: ArgKind) -> ArgSpec {
    ArgSpec {
        name: name.to_owned(),
        help: help.to_owned(),
        kind,
    }
}

fn opt(name: &str, help: &str, values: &[&str]) -> ArgSpec {
    arg(
        name,
        help,
        ArgKind::Option {
            values: values.iter().map(|v| (*v).to_owned()).collect(),
            default: None,
        },
    )
}

fn command(name: &str, about: &str, args: Vec<ArgSpec>) -> CommandSpec {
    CommandSpec {
        name: name.to_owned(),
        about: about.to_owned(),
        args,
    }
}

const KINDS: &[&str] = &["inbox", "todo", "idea", "read"];
const ID_HELP: &str = "条目的 id，或者它最后几位（至少 4 位，`stash ls` 里显示的那 6 位就行）";

fn id_arg() -> ArgSpec {
    arg("id", ID_HELP, ArgKind::Positional)
}

fn required<'a>(inv: &'a Invocation, name: &str) -> Result<&'a str, AppError> {
    inv.args
        .get(name)
        .map(String::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| AppError::validation(format!("缺少参数 <{name}>")))
}

fn kind_arg(inv: &Invocation, name: &str) -> Result<Option<Kind>, AppError> {
    inv.args
        .get(name)
        .map(|v| Kind::parse(v).ok_or_else(|| AppError::validation(format!("没有分类 `{v}`"))))
        .transpose()
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Inbox => "收件箱",
        Kind::Todo => "待办",
        Kind::Idea => "想法",
        Kind::ReadLater => "稍后读",
    }
}

fn json_lines(items: &[Item]) -> Result<String, AppError> {
    let mut out = String::new();
    for it in items {
        out.push_str(&serde_json::to_string(it).map_err(|e| AppError::unavailable(e.to_string()))?);
        out.push('\n');
    }
    Ok(out)
}

/// `ABC123  [待办]  @2026-11-20  回复老王  · 微信 · 张三`
fn line(it: &Item, show_kind: bool) -> String {
    let mut s = format!("{}  ", short(&it.id));
    if show_kind {
        s.push_str(&format!("[{}]  ", kind_label(it.kind)));
    }
    if let Some(due) = &it.due_on {
        s.push_str(&format!("@{due}  "));
    }
    s.push_str(&it.text);
    if let Some(url) = &it.url
        && !it.text.contains(url.as_str())
    {
        s.push_str(&format!("  <{url}>"));
    }
    if let Some(source) = &it.source {
        s.push_str(&format!("  · {source}"));
    }
    s
}

fn detail(it: &Item) -> String {
    let mut s = format!(
        "{}\n{}\n分类  {}\n状态  {}\n",
        it.id,
        it.text,
        kind_label(it.kind),
        it.status.as_str()
    );
    if let Some(due) = &it.due_on {
        s.push_str(&format!("截止  {due}\n"));
    }
    if let Some(url) = &it.url {
        s.push_str(&format!("链接  {url}\n"));
    }
    if let Some(source) = &it.source {
        s.push_str(&format!("来源  {source}\n"));
    }
    if !it.context.is_empty() {
        s.push_str("上下文\n");
        for c in &it.context {
            s.push_str(&format!("  {}  {}\n", c.label, c.value));
        }
    }
    s
}

fn render(items: &[Item], format: Format, show_kind: bool) -> Result<String, AppError> {
    match format {
        Format::Json => json_lines(items),
        Format::Text { .. } => Ok(items.iter().map(|it| line(it, show_kind) + "\n").collect()),
    }
}

impl Commands {
    async fn patch(&self, inv: &Invocation, patch: ItemPatch) -> Result<Output, AppError> {
        self.0
            .update(required(inv, "id")?.to_owned(), patch)
            .await?;
        Ok(Output {
            text: String::new(),
        })
    }
}

#[async_trait]
impl CommandProvider for Commands {
    async fn commands(&self) -> Result<Vec<CommandSpec>, AppError> {
        Ok(vec![
            command(
                "ls",
                "列出未完成的条目",
                vec![
                    opt("kind", "只看这个分类", KINDS),
                    opt("status", "看别的状态", &["open", "done", "archived"]),
                    opt(
                        "due",
                        "按截止日期筛（含已过期的）",
                        &["overdue", "today", "week"],
                    ),
                    opt("limit", "最多几条", &[]),
                ],
            ),
            command("show", "一个条目的全部内容和上下文", vec![id_arg()]),
            command(
                "move",
                "换分类（排到新分类的最后）",
                vec![
                    id_arg(),
                    arg("kind", "inbox / todo / idea / read", ArgKind::Positional),
                ],
            ),
            command("done", "标记完成", vec![id_arg()]),
            command("archive", "归档", vec![id_arg()]),
            command("reopen", "恢复成未完成", vec![id_arg()]),
            command("rm", "删除", vec![id_arg()]),
            command(
                "due",
                "设截止日期",
                vec![
                    id_arg(),
                    arg("date", "YYYY-MM-DD，或者 none 清掉", ArgKind::Positional),
                ],
            ),
            command(
                "edit",
                "改文字",
                vec![id_arg(), arg("words", "新的文字", ArgKind::Rest)],
            ),
        ])
    }

    async fn run(&self, inv: Invocation) -> Result<Output, AppError> {
        let text = match inv.command.as_str() {
            "ls" => {
                let kind = kind_arg(&inv, "kind")?;
                let status = inv
                    .args
                    .get("status")
                    .map(|s| {
                        Status::parse(s)
                            .ok_or_else(|| AppError::validation(format!("没有状态 `{s}`")))
                    })
                    .transpose()?;
                let due = match inv.args.get("due").map(String::as_str) {
                    None => None,
                    Some("overdue") => Some(DueFilter::Overdue),
                    Some("today") => Some(DueFilter::Today),
                    Some("week") => Some(DueFilter::Week),
                    Some(other) => {
                        return Err(AppError::validation(format!("--due 不认识 `{other}`")));
                    }
                };
                let limit = inv
                    .args
                    .get("limit")
                    .map(|l| {
                        l.parse::<u32>()
                            .map_err(|_| AppError::validation(format!("--limit `{l}` 不是数字")))
                    })
                    .transpose()?;
                let items = self
                    .0
                    .list(ItemFilter {
                        kind,
                        status,
                        due,
                        limit,
                    })
                    .await?;
                render(&items, inv.format, kind.is_none())?
            }
            "show" => {
                let it = self.0.get(required(&inv, "id")?.to_owned()).await?;
                match inv.format {
                    Format::Json => json_lines(&[it])?,
                    Format::Text { .. } => detail(&it),
                }
            }
            "move" => {
                let kind = Kind::parse(required(&inv, "kind")?)
                    .ok_or_else(|| AppError::validation("分类只能是 inbox / todo / idea / read"))?;
                return self
                    .patch(
                        &inv,
                        ItemPatch {
                            kind: Some(kind),
                            ..ItemPatch::default()
                        },
                    )
                    .await;
            }
            "done" | "archive" | "reopen" => {
                let status = match inv.command.as_str() {
                    "done" => Status::Done,
                    "archive" => Status::Archived,
                    _ => Status::Open,
                };
                return self
                    .patch(
                        &inv,
                        ItemPatch {
                            status: Some(status),
                            ..ItemPatch::default()
                        },
                    )
                    .await;
            }
            "due" => {
                let date = required(&inv, "date")?;
                let due = if date == "none" {
                    DueChange::Clear
                } else {
                    DueChange::Set(date.to_owned())
                };
                return self
                    .patch(
                        &inv,
                        ItemPatch {
                            due: Some(due),
                            ..ItemPatch::default()
                        },
                    )
                    .await;
            }
            "edit" => {
                return self
                    .patch(
                        &inv,
                        ItemPatch {
                            text: Some(required(&inv, "words")?.to_owned()),
                            ..ItemPatch::default()
                        },
                    )
                    .await;
            }
            "rm" => {
                self.0.delete(required(&inv, "id")?.to_owned()).await?;
                String::new()
            }
            other => return Err(AppError::config(format!("items 没有登记命令 `{other}`"))),
        };
        Ok(Output { text })
    }
}
