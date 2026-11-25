//! 命令行：`stash capture`、`stash add`、`stash parse`。
//!
//! 只把命令翻译成流程的调用、把结果排成文本或 JSON。

use std::sync::Arc;

use capture_contracts::{Draft, Kind};
use cli_contract::{ArgKind, ArgSpec, CommandProvider, CommandSpec, Format, Invocation, Output};
use items_contracts::Item;
use kernel_api::{AppError, async_trait};
use serde::Serialize;

use crate::flow::Capture;

pub(crate) struct Commands(pub Arc<Capture>);

const KINDS: &[&str] = &["inbox", "todo", "idea", "read"];

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

fn to_opt() -> ArgSpec {
    opt(
        "to",
        "存好后还要送到哪（路由名，逗号分隔，比如 dida）；和文字里的 #路由标记 一样",
        &[],
    )
}

fn to_arg(inv: &Invocation) -> Vec<String> {
    inv.args
        .get("to")
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|r| !r.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn kind_opt() -> ArgSpec {
    opt("kind", "放进哪个分类；文字里的 #标记 优先", KINDS)
}

fn kind_arg(inv: &Invocation) -> Result<Option<Kind>, AppError> {
    inv.args
        .get("kind")
        .map(|v| Kind::parse(v).ok_or_else(|| AppError::validation(format!("没有分类 `{v}`"))))
        .transpose()
}

fn words(inv: &Invocation) -> Result<String, AppError> {
    inv.args
        .get("words")
        .filter(|v| !v.trim().is_empty())
        .cloned()
        .ok_or_else(|| AppError::validation("缺少要记的内容"))
}

fn json<T: Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_string(value)
        .map(|s| s + "\n")
        .map_err(|e| AppError::unavailable(e.to_string()))
}

/// `ABC123  [待办]  @2026-11-20  回复老王`
fn item_line(it: &Item) -> String {
    let mut s = format!(
        "{}  [{}]  ",
        &it.id[it.id.len().saturating_sub(6)..],
        crate::kind_label(it.kind)
    );
    if let Some(due) = &it.due_on {
        s.push_str(&format!("@{due}  "));
    }
    s + &it.text + "\n"
}

fn render_item(it: &Item, format: Format) -> Result<String, AppError> {
    match format {
        Format::Json => json(it),
        Format::Text { .. } => Ok(item_line(it)),
    }
}

fn render_draft(d: &Draft, format: Format) -> Result<String, AppError> {
    if format == Format::Json {
        return json(d);
    }
    let mut s = format!("文字  {}\n", d.text);
    s.push_str(&format!(
        "分类  {}\n",
        d.kind.map_or("收件箱（没写）", crate::kind_label)
    ));
    if let Some(due) = &d.due_on {
        s.push_str(&format!("截止  {due}\n"));
    }
    if let Some(url) = &d.url {
        s.push_str(&format!("链接  {url}\n"));
    }
    for c in &d.context {
        s.push_str(&format!("上下文  {}  {}\n", c.label, c.value));
    }
    if !d.routes.is_empty() {
        s.push_str(&format!("送到  {}\n", d.routes.join("、")));
    }
    Ok(s)
}

#[async_trait]
impl CommandProvider for Commands {
    async fn commands(&self) -> Result<Vec<CommandSpec>, AppError> {
        Ok(vec![
            CommandSpec {
                name: "capture".to_owned(),
                about: "弹输入框记一条，同时记下当时在干什么（给快捷键用）".to_owned(),
                args: vec![
                    kind_opt(),
                    opt("message", "直接用这段文字，不弹输入框（上下文照样采）", &[]),
                    to_opt(),
                ],
            },
            CommandSpec {
                name: "add".to_owned(),
                about: "记一条，不弹框也不采上下文；支持 #分类 @日期".to_owned(),
                args: vec![
                    arg("words", "要记的内容", ArgKind::Rest),
                    kind_opt(),
                    opt("due", "截止日期 YYYY-MM-DD；文字里的 @日期 优先", &[]),
                    to_opt(),
                ],
            },
            CommandSpec {
                name: "parse".to_owned(),
                about: "只展开和解析一段文字，看会记成什么样，不保存".to_owned(),
                args: vec![arg("words", "要解析的文字", ArgKind::Rest), kind_opt()],
            },
        ])
    }

    async fn run(&self, inv: Invocation) -> Result<Output, AppError> {
        let text = match inv.command.as_str() {
            "capture" => {
                let message = inv.args.get("message").cloned();
                match self
                    .0
                    .capture(kind_arg(&inv)?, message, to_arg(&inv))
                    .await?
                {
                    Some(it) => render_item(&it, inv.format)?,
                    // 取消了：没事不输出
                    None => String::new(),
                }
            }
            "add" => {
                let it = self
                    .0
                    .add(Draft {
                        text: words(&inv)?,
                        kind: kind_arg(&inv)?,
                        due_on: inv.args.get("due").cloned(),
                        routes: to_arg(&inv),
                        ..Draft::default()
                    })
                    .await?;
                render_item(&it, inv.format)?
            }
            "parse" => {
                let draft = self
                    .0
                    .parse(Draft {
                        text: words(&inv)?,
                        kind: kind_arg(&inv)?,
                        ..Draft::default()
                    })
                    .await?;
                render_draft(&draft, inv.format)?
            }
            other => return Err(AppError::config(format!("capture 没有登记命令 `{other}`"))),
        };
        Ok(Output { text })
    }
}
