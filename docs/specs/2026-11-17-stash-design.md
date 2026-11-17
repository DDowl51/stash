# stash 设计

- 日期：2026-11-17
- 状态：已确认（§5.2 上下文、§5.3 展开器 2026-11-17 追加；Q1–Q7 全部已定）

## 1. 背景和目标

忙的时候碰到要做的事（别人发消息请帮忙）、闪过的念头、想读的链接，需要**立刻记下、不打断当前工作**，忙完后再统一整理和规划。主要环境：Arch + niri。

1. **快速记录**：一个快捷键弹出输入框，回车保存、窗口消失、焦点回到原窗口，全程 2 秒内。
2. **尽可能多地保存当时的上下文**（用户最看重的一点）：来源窗口、终端里的目录和 git 分支、浏览器当前页面、窗口截图、自定义脚本给的信息……按快捷键那一刻自动采集，整理时能看到「当时在干什么」。见 §5.2。
3. **输入展开**：回车后按规则处理输入（`#分类`、`@日期`、`=计算`、缩写、外部脚本）。见 §5.3。
4. **分类**：收件箱 / 待办 / 想法 / 稍后阅读。记录时可以用 `#` 标记或命令参数直接指定；不指定进收件箱；内容是 URL 时默认进稍后阅读。
5. **整理和规划**：终端界面里把收件箱分流、排序、设截止日期、标记完成、归档、删除。
6. **提醒**：收件箱积压、待办到期时发桌面通知。
7. **以后**：手机同步、输入时实时预览（见 §12）。现在只留边界。

### 不做的事

- 不抓取网页、不做离线阅读。稍后阅读只是一个分类，内容就是 URL（加上记录时写的文字）。
- 不做浏览器扩展。
- 不读剪贴板（用户决定：需要时在输入框里 Ctrl+V）。
- 不做标签、项目、子任务、重复任务。`#` 只用来指定分类。
- 不做图形界面，整理只在终端界面里。
- 不做常驻进程。
- 手机同步、实时预览现在不做（见 §12）。

## 2. 档位和配套

| | 选择 | 理由 |
|---|---|---|
| 档位 | 中等 | 有三个入口（记录、命令行、终端界面）；有要替换的系统服务（输入框 vicinae ↔ fuzzel，窗口管理器 niri ↔ 别的）；会长期用；kernel 和大部分系统服务能从 lifelog 的中等档实现直接复用 |
| 配套 | 数据应用、命令行、常驻进程 / 桌面工具（只用「每个外部程序一个系统服务」那部分） | SQLite；有 CLI；要和 niri、vicinae、通知服务协作 |

预计什么情况下升档：做手机同步时，如果要一个常驻的同步进程对外提供接口，再考虑。

用到的例外：无。

**和配套的差异**：常驻进程配套要求「常驻进程是唯一的写者」。stash 没有常驻进程，记录、命令行、终端界面都是一次性进程，都可能写库，靠 SQLite 的 WAL + `busy_timeout`（`storage-sqlite` 已支持）解决并发写入。写进 §11。

## 3. 架构

### 3.1 技术选型

| 方面 | 选型 |
|---|---|
| 语言 / 运行时 | Rust，tokio（和共享仓库一致） |
| 仓库 | 新仓库 `~/code/personal/stash`；kernel 和通用系统服务来自共享仓库（Q1） |
| 测试 | cargo test、各契约的契约测试、`tests/arch` 架构检查、`tests/e2e` 真实进程测试 |
| 规范 | rustfmt、clippy（`deny(print_stdout, print_stderr)`）、lefthook、Conventional Commits |

### 3.2 仓库结构

共享仓库（暂名 `mk-kit`，见 Q1）从 lifelog 抽出：

```
mk-kit/
  crates/     kernel-api  kernel  boot  boot-build  testkit
  contracts/  browser（加 recent）  cli  clock  files  log  sql  timer  tui  window（加可选的 pid）
  services/   browser-history  browser-firefox  browser-chromium  browser-qutebrowser
              cli-clap  clock-system  files-std  log-stderr  storage-sqlite  timer-tokio  tui-ratatui  wm-niri
  tests/arch/ 架构检查的规则库（各项目的 tests/arch 复用它）
```

stash：

```
Cargo.toml                [workspace]，共享仓库的包用 path 依赖
apps/
  stash/                  命令行启动器：stash capture / stash item … / stash remind
  stash-tui/              终端界面启动器
contracts/
  prompt/  notify/  process/  vcs/  screenshot/
services/
  prompt-vicinae/  notify-dbus/  process-procfs/  vcs-git/  screenshot-niri/
features/
  items/{contracts,core,component}
  capture/{contracts,core,component}      contracts 里是两个扩展点：capture.context、capture.expanders
  context-window/  context-terminal/  context-browser/  context-screenshot/  context-script/
  expand-calc/  expand-snippets/  expand-script/
  review/component
  remind/{core,component}
tests/
  arch/  e2e/
docs/
```

### 3.3 组件清单（按最终完整范围）

系统服务：

| 组件 | 一句话职责 | 提供的契约 | 需要的契约 | 第一个实现 | v1 |
|---|---|---|---|---|---|
| `clock-system` | 现在几点 | `clock` | — | 复用 | 做 |
| `log-stderr` | 日志 | `log` | — | 复用 | 做 |
| `storage-sqlite` | SQL 存储、迁移扩展点 | `sql`、扩展点 `migrations` | `log` | 复用 | 做 |
| `cli-clap` | 命令行外壳、扩展点 `cli.commands` | `cli` | — | 复用 | 做 |
| `tui-ratatui` | 终端界面外壳、扩展点 `tui.pages` | `tui` | — | 复用，**升到 v2**（可选中的行 + 按键操作，见 §6） | 做 |
| `wm-niri` | 当前聚焦的窗口 | `window` | `clock`、`log` | 复用；`WindowInfo` 加可选的 `pid`（niri IPC 有） | 做 |
| `files-std` | 读写文件 | `files` | — | 复用 | 做 |
| `process-procfs` | 按 pid 查进程：命令行、工作目录、子进程 | `process` | — | `/proc` | 做 |
| `vcs-git` | 按目录查仓库：根目录、分支、提交、远端 | `vcs` | — | `git` 命令 | 做 |
| `screenshot-niri` | 截指定窗口，写到给定路径 | `screenshot` | — | `niri msg action screenshot-window --id --path`，并恢复剪贴板（Q5） | 做 |
| `browser-history` + `browser-firefox`（Zen）、`browser-chromium`（Chrome）、`browser-qutebrowser` | 浏览历史，合并多个浏览器 | `browser.history`，扩展点 `browser.adapters` | — | 复用 lifelog，契约加 `recent`（Q6） | 做 |
| `prompt-vicinae` | 弹一个输入框，返回一行文字或「取消」 | `prompt` | — | `vicinae dmenu`（Q2） | 做 |
| `prompt-fuzzel` | 同上 | `prompt` | — | `fuzzel --dmenu` | vicinae 不行时才做 |
| `notify-dbus` | 发桌面通知 | `notify` | — | `org.freedesktop.Notifications`（zbus），不依赖 `notify-send` | 做 |

应用功能：

| 组件 | 一句话职责 | 提供的契约 | 需要的契约 | 可选需要 | v1 |
|---|---|---|---|---|---|
| `items` | 条目的领域模型、存储、变更事件；登记 `stash item …` 命令 | `items`（读写）、事件 `items.changed` | `sql`、`clock`、`cli` | — | 做 |
| `capture` | 拍下当时的窗口 → 并发采集上下文 → 弹输入框 → 展开 → 写入；登记 `stash capture`、`stash parse` | 扩展点 `capture.context`、`capture.expanders` | `items`、`prompt`、`cli`、`clock` | `window`、`notify` | 做 |
| `context-window` | 来源窗口：app、标题、工作区 | 登记 `capture.context` | — | — | 做 |
| `context-terminal` | 来源是终端时：前台命令、工作目录、git 仓库和分支 | 登记 `capture.context` | `process`、`vcs` | — | 做 |
| `context-browser` | 来源是浏览器时：当前页面 URL 和标题 | 登记 `capture.context` | `browser.history` | — | 做 |
| `context-screenshot` | 来源窗口的截图 | 登记 `capture.context` | `screenshot`、`files` | — | 做 |
| `context-script` | 配置里的外部命令给的上下文 | 登记 `capture.context` | — | — | 做 |
| `expand-calc` | `=3*45+12` → 计算结果 | 登记 `capture.expanders` | — | — | 做 |
| `expand-snippets` | 配置里的缩写 → 模板 | 登记 `capture.expanders` | — | — | 做 |
| `expand-script` | 配置里的「前缀 → 外部命令」 | 登记 `capture.expanders` | — | — | 做 |
| `review` | 终端界面里的整理和规划页面 | — | `items`、`tui`、`clock` | — | 做 |
| `remind` | 统计积压和到期，发通知；登记 `stash remind` | — | `items`、`notify`、`cli`、`clock`、`sql`（自己的已提醒记录） | — | 做 |
| `sync` | 手机同步 | — | `items`、订阅 `items.changed` | — | 以后 |

`window` 是 `capture` 的可选依赖：niri 没在运行时照样能记录，只是少了来源信息。不读剪贴板：要贴内容就在输入框里手动 Ctrl+V。

采集上下文和展开输入都是扩展点：`capture` 只管流程（什么时候采、等多久、失败怎么办），不认识任何具体的上下文来源。加一种上下文 = 加一个 `context-*` 组件，不改 `capture`。外部程序（`/proc`、git、浏览器、niri 截图）各自只出现在一个系统服务里，`context-*` 只把它们的结果翻译成上下文条目。

### 3.4 依赖图

```
 capture ──┐   review ──┐   remind ──┐        (sync, 以后)
    │      │      │     │      │     │            │
    ▼      │      ▼     │      ▼     │            ▼
  items ◄──┴──────┴─────┴──────┴─────┘◄──── items.changed
    │
    ▼
 ─────────── 系统服务 ───────────────────────────────
 sql  clock  log  cli  tui  window┄  prompt  notify┄  files  process  vcs  screenshot  browser.history
                                ┄ = capture 的可选依赖

 context-* ──登记──► capture.context      expand-* ──登记──► capture.expanders
 （context-* 依赖 process / vcs / screenshot / files / browser.history；都不依赖 capture 的实现，只依赖 capture/contracts）
```

没有环。`cli`、`tui`、`migrations` 都是扩展点，由功能登记，系统服务不认识任何功能。

### 3.5 通信

| 提供方 | 使用方 | 原语 | 名字 |
|---|---|---|---|
| `items` | `capture`、`review`、`remind` | 请求 | `items` |
| `items` | （以后 `sync`） | 事件 | `items.changed` |
| `items`、`capture`、`remind` | `cli-clap` | 扩展点 | `cli.commands` |
| `review` | `tui-ratatui` | 扩展点 | `tui.pages` |
| `items`、`remind` | `storage-sqlite` | 扩展点 | `migrations` |
| `context-*` | `capture` | 扩展点 | `capture.context` |
| `expand-*` | `capture` | 扩展点 | `capture.expanders` |
| `wm-niri` | `capture` | 请求 | `window.focused()` |
| `prompt-vicinae` | `capture` | 请求 | `prompt.ask()` |
| `notify-dbus` | `remind` | 请求 | `notify.send()` |

### 3.6 契约和可替换性

| 契约 | 现有实现 | 契约测试 | 独立成包 |
|---|---|---|---|
| `prompt` v1 | `prompt-vicinae`（以后 `prompt-fuzzel`） | `prompt_contract`：取消返回 `Cancelled`，空输入算取消，返回的文字去掉首尾空白 | 是 |
| `notify` v1 | `notify-dbus` | 假实现记录发出的通知 | 是 |
| `items` v1 | `items` | 契约测试覆盖增、改、移动、排序、按分类和日期查询、上下文原样存取 | 是（`features/items/contracts`） |
| `process` v1 | `process-procfs` | 不存在的 pid 返回 `not_found`；对测试进程自己查得到 cwd 和命令行 | 是 |
| `vcs` v1 | `vcs-git` | 不是仓库返回 `None`；临时仓库里查得到分支和提交 | 是 |
| `screenshot` v1 | `screenshot-niri` | 写出的是 PNG；窗口不存在返回 `not_found` | 是 |
| `browser.history` v2 | 三个浏览器适配 | 沿用 lifelog 的适配契约测试，加 `recent`：只返回 `since` 之后的、按时间倒序 | 是（mk-kit） |
| 扩展点 `capture.context` v1 | 各 `context-*` | 假的 capture 宿主：给一个窗口快照，断言返回的条目 | 是（`features/capture/contracts`） |
| 扩展点 `capture.expanders` v1 | 各 `expand-*` | 同上，给文字断言展开结果 | 同上 |
| 共享仓库里的契约 | 已有 | 已有 | 是 |

契约草案：

```rust
// prompt：弹一个输入框
struct PromptRequest { placeholder: String, initial: String }
enum Answer { Text(String), Cancelled }
async fn ask(&self, req: PromptRequest) -> Result<Answer, AppError>;

// notify
struct Notification { summary: String, body: String, urgency: Urgency }
async fn send(&self, n: Notification) -> Result<(), AppError>;

// items
enum Kind { Inbox, Todo, Idea, ReadLater }
enum Status { Open, Done, Archived }
struct Item { id, text, url: Option<String>, kind, status, rank: i64,
              due_on: Option<String /* YYYY-MM-DD */>,
              source: Option<String>,          // 一行摘要，列表里显示，比如「微信 · 张三」
              context: Vec<ContextEntry>,      // 完整上下文，详情里显示
              created_at, updated_at }
// 上下文条目：纯数据，items 不解释内容，原样存取
struct ContextEntry { key: String /* "terminal.cwd" */, label: String /* "目录" */,
                      kind: ContextKind /* text | url | path | image | command */, value: String }
async fn add(&self, new: NewItem) -> Result<Item, AppError>;
async fn update(&self, id: String, patch: ItemPatch) -> Result<Item, AppError>;  // 改文字、分类、状态、日期
async fn reorder(&self, id: String, before: Option<String>) -> Result<(), AppError>;
async fn delete(&self, id: String) -> Result<(), AppError>;
async fn list(&self, filter: ItemFilter) -> Result<Vec<Item>, AppError>;          // 按分类、状态、到期范围
async fn counts(&self) -> Result<Counts, AppError>;

// process（系统服务，不认识终端、shell 这些词）
struct ProcessInfo { pid: u32, ppid: u32, exe: Option<String>, cmdline: Vec<String>, cwd: Option<String> }
async fn info(&self, pid: u32) -> Result<ProcessInfo, AppError>;
async fn children(&self, pid: u32) -> Result<Vec<ProcessInfo>, AppError>;

// vcs
struct RepoInfo { root: String, branch: Option<String>, commit: String, remote_url: Option<String>, dirty: bool }
async fn repo_at(&self, path: String) -> Result<Option<RepoInfo>, AppError>;

// screenshot
async fn window(&self, window_id: u64, path: String) -> Result<(), AppError>;

// browser.history（mk-kit，v2 新增的方法；v1 的 sources / read 不变）
async fn recent(&self, source: String, since_ms: i64, limit: u32) -> Result<Vec<Visit>, AppError>;  // 按时间倒序

// 扩展点 capture.context（features/capture/contracts）
struct Snapshot { at: Moment, window: Option<WindowInfo>, item_id: String }
enum Phase { BeforePrompt /* 截图这类：要在输入框盖住屏幕之前做 */, WhilePrompt }
async fn spec(&self) -> Result<ContextSpec { id, phase }, AppError>;
async fn collect(&self, snap: Snapshot) -> Result<Vec<ContextEntry>, AppError>;

// 扩展点 capture.expanders
struct Draft { text: String, kind: Option<Kind>, due_on: Option<String>, url: Option<String>, context: Vec<ContextEntry> }
async fn expand(&self, draft: Draft) -> Result<Draft, AppError>;   // 不处理就原样返回
```

### 3.7 配置

`$XDG_CONFIG_HOME/stash/config.toml`，文件可以不存在，全部有默认值。

```toml
[components.storage-sqlite]
path = "~/.local/share/stash/stash.db"

[components.capture]
enabled = true
context_timeout_ms = 1000           # 回车后最多再等上下文多久，超时的那一项丢掉
placeholder = "记点什么…  #t 待办  #i 想法  #r 稍后读  @明天"
url_kind = "read_later"             # 只有 URL、没写 # 时归到哪类

# 分类标记：# 后面的词 → 分类。策略，可以随意改
[components.capture.tags]
todo = ["todo", "t", "待办"]
idea = ["idea", "i", "想法"]
read_later = ["read", "r", "读"]
inbox = ["inbox", "in"]

[components.context-terminal]
apps = ["com.mitchellh.ghostty", "kitty", "foot", "Alacritty"]   # 哪些 app_id 算终端

[components.context-browser]
# app_id → 历史来源的浏览器名 + 从窗口标题里去掉的后缀（正则）
[components.context-browser.apps]
zen = { browser = "zen", title_suffix = " — Zen Browser$" }
google-chrome = { browser = "chrome", title_suffix = " - Google Chrome$" }
"org.qutebrowser.qutebrowser" = { browser = "qutebrowser", title_suffix = " - qutebrowser$" }
lookback_minutes = 30

[components.context-screenshot]
enabled = true
dir = "~/.local/share/stash/shots"
skip_apps = []                      # 这些应用不截图（比如密码管理器）

[[components.context-script.scripts]]
name = "lifelog"                    # 例子：记下当时 lifelog 里最近在做什么
command = ["lifelog", "timeline", "--last", "10m", "--json"]
when_apps = []                      # 空 = 总是运行
timeout_ms = 800

[components.expand-snippets.snippets]
";会" = "开会时提到："
";回" = "回复："

[[components.expand-script.rules]]
prefix = "!gh "                     # 例子：!gh 123 → issue 标题和链接
command = ["stash-gh-issue"]
timeout_ms = 1500

[components.prompt-vicinae]
command = "vicinae"
width = 700

[components.remind]
enabled = true
inbox_threshold = 5                 # 收件箱超过几条就提醒
due_days_ahead = 0                  # 提前几天提醒到期，0 = 当天
```

### 3.8 kernel 和启动器

kernel、启动库来自共享仓库，不改。

| 启动器 | 装的组件 |
|---|---|
| `stash` | 全部系统服务（除 `tui-ratatui`）+ `items`、`capture`、`remind`、全部 `context-*` 和 `expand-*` |
| `stash-tui` | `clock`、`log`、`storage-sqlite`、`tui-ratatui` + `items`、`review` |

`stash capture` 每次都是新进程，启动要快：组件的 `start` 不做 IO，上下文提供者只在被调用时才碰外部程序。阶段 3 验收时量一次从按键到输入框出现的时间，目标 300 ms 以内。

## 4. 数据设计

按数据应用配套：ULID 主键、UTC 毫秒 `_at`、日期 `_on`、`STRICT`、表名前缀是组件 id。

```sql
-- items
CREATE TABLE items_item (
  id           TEXT PRIMARY KEY,
  text         TEXT NOT NULL,
  url          TEXT,
  kind         TEXT NOT NULL CHECK (kind IN ('inbox','todo','idea','read_later')),
  status       TEXT NOT NULL CHECK (status IN ('open','done','archived')),
  rank         INTEGER NOT NULL,       -- 同一分类里的手动排序，间隔 1024，挤满时重排
  due_on       TEXT,
  source       TEXT,                   -- 一行摘要
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL,       -- 以后同步时按它合并
  deleted_at   INTEGER                 -- 软删除，给同步留墓碑
) STRICT;

CREATE TABLE items_context (
  item_id TEXT NOT NULL REFERENCES items_item(id),
  seq     INTEGER NOT NULL,
  key     TEXT NOT NULL,
  label   TEXT NOT NULL,
  kind    TEXT NOT NULL CHECK (kind IN ('text','url','path','image','command')),
  value   TEXT NOT NULL,
  PRIMARY KEY (item_id, seq)
) STRICT;

-- remind：同一个提醒一天只发一次
CREATE TABLE remind_sent (
  key     TEXT NOT NULL,               -- 'inbox' 或 'due:<item id>'
  sent_on TEXT NOT NULL,
  PRIMARY KEY (key, sent_on)
) STRICT;
```

## 5. 对外接口

### 5.1 记录时的输入语法（`capture` 的纯逻辑，`core` 里测）

| 写法 | 效果 |
|---|---|
| `帮老王看下 PR #t` | `#` + 配置里的标记 → 分类。可以写在任意位置，从文字里去掉 |
| `#t` 不在配置里，比如 `#C++` | 原样保留在文字里 |
| `@明天` `@周五` `@11-20` `@2026-11-20` | 截止日期，从文字里去掉 |
| 整段只有一个 URL | 分类按 `url_kind`，`url` 字段填上 |
| 文字里带 URL | `url` 字段填第一个 URL，分类不变 |
| 什么都没写 / 按 Esc | 不保存，退出码 0，什么都不输出 |

`#` 和 `@` 是 `capture` 自己的规则，在所有展开器之后执行（展开器产生的 `#t` 也能生效）。

### 5.2 上下文采集

用户最看重的部分：**按快捷键那一刻**，尽可能多地留下「当时在干什么」。

流程（`capture` 负责，时间线从按键开始）：

1. 立刻向 `window` 要当前聚焦的窗口，拍成快照。必须在输入框出现之前做，因为 vicinae 一出现焦点就变了。
2. 运行 `phase = BeforePrompt` 的提供者（截图），等它们结束，上限 300 ms。
3. 弹输入框；同时在后台并发运行 `phase = WhilePrompt` 的提供者。你打字的时间就是它们的预算。
4. 回车后最多再等 `context_timeout_ms`，超时的提供者丢掉，并在上下文里记一条「xx 超时」。
5. 按 Esc：取消所有提供者，删掉已经写出的截图。

**任何提供者失败都不影响保存。** 失败记成一条 `kind = text` 的上下文（「浏览器：读不到会话文件」），写 stderr，不弹通知。

v1 的提供者：

| 组件 | 什么时候 | 采集什么 | 怎么拿 |
|---|---|---|---|
| `context-window` | 总是 | app、窗口标题、工作区、时间 | 快照 |
| `context-terminal` | app 在配置的终端列表里 | 前台命令、工作目录、git 仓库 / 分支 / 提交 / 远端 | 窗口 pid → `process.children` 往下找到最深的前台进程 → `cwd` → `vcs.repo_at` |
| `context-browser` | app 在配置的浏览器列表里 | 当前页面 URL、标题 | 窗口标题去掉浏览器后缀 → 在该浏览器最近 30 分钟的历史里找标题相同的最新一条 → URL。找不到就只记标题，并注明「没找到 URL」。各 app_id 和后缀在阶段 3b 真机上核对 |
| `context-screenshot` | 默认开（`skip_apps` 除外） | 来源窗口的截图 | `screenshot.window` → `~/.local/share/stash/shots/<item id>.png` |
| `context-script` | 配置里写的条件 | 外部命令 stdout 的 JSONL（`{"key","label","kind","value"}`）或纯文本 | 环境变量传入 `STASH_APP_ID`、`STASH_TITLE`、`STASH_PID`、`STASH_WORKSPACE`、`STASH_AT` |

`source` 摘要由 `capture` 按「app · 标题」生成，提供者可以给一条 `key = "source"` 的条目来覆盖（比如 `context-terminal` 给「ghostty · ~/code/personal/stash (main)」）。

和 lifelog 的关系：不直接依赖。想要「当时前后 10 分钟在干什么」，用 `context-script` 调 `lifelog` 的命令行（配置例子见 §3.7）。

以后可以加的提供者（不改 `capture`）：编辑器里打开的文件（从标题或编辑器的 socket）、聊天应用的对方名字（从标题解析，可配置正则）、当前播放的音乐（MPRIS）。

### 5.3 输入展开（回车之后）

`capture` 拿到文字后，按配置顺序交给 `capture.expanders` 的每个登记者，每个都可以改文字、分类、日期、URL，追加上下文。最后执行内置的 `#`、`@` 规则。

| 组件 | 触发 | 效果 |
|---|---|---|
| `expand-calc` | `=` 开头的片段，例如 `买 3 个 =3*45` | 换成 `买 3 个 135`，原式记进上下文 |
| `expand-snippets` | 配置的缩写，例如 `;会` | 换成模板文字 |
| `expand-script` | 配置的前缀，例如 `!gh 123` | 把整段文字给外部命令（stdin），stdout 是新文字或 JSON 形式的 `Draft` |

**展开失败时按原文保存**：超时、退出码非 0、输出看不懂，都按原文保存，在上下文里记一条失败原因。记录永远不能丢。

`stash parse --text <文字> --json` 只跑展开和解析、不保存，输出 `Draft`。用来调试，以后的实时预览（§12）也用它。

### 5.4 命令行

```
stash capture [--kind todo|idea|read|inbox] [--text <文字>]   # 弹输入框；--text 时不弹
stash parse --text <文字>                                      # 只展开和解析，不保存
stash item show <id>                                           # 条目和全部上下文
stash item add <文字>                                          # 和 capture 同样的语法，不弹框
stash item list [--kind …] [--status …] [--due today|week|overdue]
stash item move <id> <kind>
stash item done <id> | archive <id> | rm <id>
stash item due <id> <日期|none>
stash remind [--dry-run]                                       # 给 systemd timer 用
```

按命令行配套：stdout 是终端时输出文本，否则输出 JSONL；ID 可以从 stdin 读。

**用命令指定分类**：给每个分类绑一个快捷键，比如：

```kdl
// ~/.config/niri/config.kdl
binds {
    Mod+N       { spawn "stash" "capture"; }
    Mod+Shift+T { spawn "stash" "capture" "--kind" "todo"; }
    Mod+Shift+R { spawn "stash" "capture" "--kind" "read"; }
}
```

`--kind` 和文字里的 `#` 同时出现时，以 `#` 为准（后写的更具体）。

### 5.5 systemd

`stash-remind.timer`：登录后 5 分钟一次，之后每小时一次；运行 `stash remind`。

## 6. 界面（`review`，终端界面）

页面：

| 键 | 页面 | 内容 |
|---|---|---|
| `1` | 收件箱 | 按时间，最新在上；显示 `source` 摘要和时间 |
| `2` | 待办 | 先过期、再今天、再按手动排序 |
| `3` | 想法 | 手动排序 |
| `4` | 稍后阅读 | 手动排序；`o` 用 `xdg-open` 打开 URL |
| `5` | 已完成 | 最近完成的，可以恢复 |

每个页面下半部分是选中条目的**详情**：全部上下文，`url` / `path` / `image` 类可以打开（`o` 打开 URL，`O` 选一条上下文打开，截图用默认看图程序）。

操作：`j/k` 选中，`t/i/r` 移到待办 / 想法 / 稍后阅读，`x` 完成，`a` 归档，`d` 删除（再按一次确认），`J/K` 上下移动排序，`e` 改文字，`D` 设截止日期，`o` 打开链接。

**`tui` 契约要升 v2。** 现在的 v1 只有「页面 → 只读的内容」，没有选中和操作。v2 加：

- `Block::List { rows: Vec<ListRow> }`：`ListRow` 带一个不透明的 `id`，外壳负责选中和高亮。
- `PageSpec.actions: Vec<ActionSpec { key, label, needs_input: Option<String>, confirm: bool }>`：页面声明自己的按键。
- `PageProvider::act(page, action, row_id, input) -> ActionResult { message, reload }`。

外壳只管选中、按键、输入框、确认和重绘，不知道「完成」「分类」是什么。v1 的页面不受影响（`actions` 默认为空）。`o` 打开链接需要「打开 URL」的能力，作为 `ActionResult` 里的一个请求（`open_url: Option<String>`）由外壳执行，外壳不认识 URL 的业务含义。

## 7. 错误处理

用 `principles.md` 的固定错误种类。

| 边界 | 谁映射 | 例子 |
|---|---|---|
| vicinae 没装或启动失败 | `prompt-vicinae` | `unavailable`：「找不到 vicinae，安装或在配置里换 prompt 实现」 |
| niri 没在运行 | `wm-niri` | `capture` 的可选依赖，缺了就不记来源，不报错 |
| 通知服务不在 | `notify-dbus` | `unavailable`，`stash remind` 退出码非 0，systemd 记日志 |
| 输入语法里日期写错 | `capture` | 不报错，`@xxx` 原样留在文字里（记录不能因为格式失败） |
| 上下文提供者失败或超时 | `capture` | 不报错，记一条上下文说明原因，照常保存 |
| 展开器失败或超时 | `capture` | 按原文保存，记一条上下文说明原因 |
| `/proc`、git、浏览器会话文件、截图 | 各自的系统服务 | `not_found` / `unavailable` + 怎么修 |

`capture` 是在弹窗里运行的，没有终端看报错。保存失败时要另外报出来：如果 `notify` 可用就发一条通知（可选依赖），同时写 stderr。

## 8. 检查手段和测试

| 层 | 工具 | 重点 |
|---|---|---|
| 架构 | `tests/arch`（复用共享仓库的规则） | 依赖方向；`core` 不依赖 IO；外部程序名只出现在对应的系统服务 |
| 纯逻辑 | `capture/core`、`remind/core`、`expand-calc` 单元测试 | 输入语法、日期解析（以 `clock` 的今天为基准）、展开顺序、超时和失败时的取舍、提醒规则 |
| 契约 | 每个契约一个契约测试，假实现放在 `testing` feature 后面 | `items`、`prompt`、`notify`、`tui` v2 |
| 组件 | testkit + 假实现 | `capture` 用假的 prompt / window 和会超时、会失败的假提供者走通；`context-terminal` 用假的 process / vcs |
| 端到端 | `tests/e2e`，真实子进程 | `stash item add/list` 的输出格式、退出码、管道；`prompt-vicinae` 用一个假的 `vicinae` 脚本顶替 |
| 真机 | 手动验收清单 | niri 快捷键 → vicinae → 保存 → 焦点回到原窗口；TUI 操作；通知 |

覆盖率：`core` 90%，其余 80%。

## 9. 交付顺序

见 `docs/plans/2026-11-17-stash-plan.md`。

## 10. 待决定

- ~~Q1~~ **已确认**：共享仓库 `~/code/personal/mk-kit`，lifelog 和 stash 都用 path 依赖。抽取后 lifelog 全量测试要通过；保留 git 历史的具体方式执行前再确认。
- ~~Q2：`vicinae dmenu` 能不能返回自由输入的文字？~~ **已确认（2026-11-17 用户实测）**：`printf 'a\nb\nc\n' | vicinae dmenu -p …` 输入 `testinput` 回车，stdout 输出 `testinput`，退出码 0。`prompt-vicinae` 的做法：`vicinae dmenu -p <placeholder> -q <initial> --no-section --no-quick-look`，stdin 给空；退出码 1 且无输出 = 取消（用户实测 Esc：无输出、退出码 1）；退出码 0 但 stdout 去掉空白后为空也当取消；其他非 0 退出码或启动失败 = `unavailable`。
  - 用户实测：列表非空、输入为空时直接回车，输出的是**第一项**（`a`）。所以 `prompt-vicinae` **永远不往 stdin 放条目**，预填内容只走 `-q`；否则空回车或输入恰好匹配某一项时，会存进去列表项而不是用户输入的文字。契约测试里用假的 `vicinae` 脚本断言 stdin 为空。
- ~~Q3~~ **已确认**：`--kind` 和 `#` 冲突时以 `#` 为准。
- ~~Q4~~ **已确认**：先做「回车后展开」（§5.3），实时预览放到以后（§12）。用户最看重的是尽可能多地保存上下文（§5.2）。
- ~~Q5~~ **已定（2026-11-17，用户贴出 `niri msg action screenshot-window --help`）**：niri 有 `--id <窗口 id>` 和 `--path <绝对路径>`，但**总会同时写剪贴板**，没有关掉的选项。`focused-window` 的 `layout.tile_pos_in_workspace_view` 可能是 `null`，拿不到可靠的窗口坐标，`grim -g` 走不通（这台机器也没装 grim）。做法：`screenshot-niri` 截图前用 `wl-paste --list-types` + `wl-paste --type <首选类型>` 存下剪贴板，截完用 `wl-copy --type <类型>` 恢复。这是 niri 的怪癖，只存在于 `screenshot-niri` 内部，契约里不出现剪贴板。已知副作用：剪贴板管理器的历史里会多一张截图；剪贴板里只恢复首选的那一种类型。以后 niri 加了不写剪贴板的选项就去掉这段。
- ~~Q6~~ **已定**：浏览器是 Zen、Chrome、qutebrowser，lifelog 已有这三个的历史适配。不读会话文件，改为「窗口标题 → 在最近历史里按标题找 URL」，三个浏览器一套办法；`browser` 契约在 mk-kit 里加 `recent` 方法（v2）。浏览器适配随阶段 3b 一起搬到 mk-kit。
- ~~Q7~~ **已定**：截图默认开，`skip_apps` 排除。

## 11. 实现中的调整

| 方面 | 设计原文 | 实际 | 原因 |
|---|---|---|---|
| 写者 | 常驻进程配套：常驻进程是唯一写者 | 没有常驻进程，多个一次性进程都写库，靠 WAL + busy_timeout | 记录是低频的单条写入；为它常驻一个进程不值得 |

| 浏览器适配搬家时机 | 阶段 3b 再搬到 mk-kit | 阶段 0 就一起搬了 | 依赖封闭，一次搬完少一轮 lifelog 改动 |
| `boot-build` | 只扫本仓库的 `services/`、`features/` | 也收 `[workspace.dependencies]` 里 path 指向仓库外的组件包（mk-kit `b5d2bff`） | 否则共享仓库里的组件在使用方是「未知组件」，配置文档检查失败 |
| 架构检查 | 复用共享仓库的规则 | mk-kit 和 lifelog 各有一份 `tests/arch`（内容相同），仓库外的包按它所在 workspace 的根判断角色 | 规则库还没拆成可依赖的包；stash 先复制一份，三份对不上时再抽 |

## 12. 以后

不在当前计划里，边界已经留好：

| 功能 | 怎么接进来 | 留好的边界 |
|---|---|---|
| 实时预览（用户 2026-11-17 提出的 B 方案） | 写一个 vicinae 扩展代替 `prompt-vicinae` 的 dmenu，打字时调用 `stash parse --json` 显示分类、日期、计算结果；保存仍走 `stash capture --text` | `stash parse`；解析和展开都在 `capture`，扩展不重复实现 |
| 手机同步 | 新组件 `sync`，订阅 `items.changed`，按 `updated_at` 合并，删除走 `deleted_at` 墓碑；截图等文件另外同步或不同步 | ULID、`updated_at`、`deleted_at`、`items.changed` 事件 |
