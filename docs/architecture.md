# 架构

stash 按微内核组织：kernel 只管组件的生命周期、命名和消息传递；存储、窗口、截图、命令行、终端界面这些能力，以及条目、记录、整理、提醒这些业务，全都是组件。组件之间只通过契约里的纯数据通信。

完整的设计和取舍见 [设计文档](specs/2026-11-17-stash-design.md)，实现时和设计不一样的地方记在它的 §11。

## 两个仓库

| 仓库 | 放什么 |
|---|---|
| [mk-kit](../../mk-kit) | kernel、启动库、测试工具，以及和业务无关的契约和系统服务（存储、窗口、截图、进程、git、弹框、通知、命令行、终端界面……）。lifelog 也用它 |
| stash（本仓库） | 应用功能和两个启动器。通过 `Cargo.toml` 里的 path 依赖引用 mk-kit |

系统服务里不能出现 stash 的业务名词。比如通知服务不知道应用叫 stash：应用名由 stash 的启动器通过 `set_default` 告诉它。

## 目录

```
apps/
  stash/          启动器：命令行、快捷键记录、提醒
  stash-tui/      启动器：整理界面
features/<id>/
  contracts/      对外的契约（只有数据和 trait，没有实现）
  core/           纯逻辑：不碰 IO、不碰运行时，单元测试覆盖
  component/      组件：把 core 接到契约上
tests/
  arch/           架构规则（依赖方向）
  e2e/            真实进程 + 假的 niri / vicinae / 剪贴板
  config-doc/     docs/configuration.md 和代码里的默认值一致
dist/             systemd 单元、niri 快捷键例子
scripts/          check.sh（全量检查）、install.sh
```

## 组件

应用功能（本仓库）：

| 组件 | 做什么 | 提供 | 需要 |
|---|---|---|---|
| `items` | 条目的存储、读写和命令行命令 | `items`、事件 `items.changed`；登记 `migrations`、`cli.commands`（后者可选：`stash-tui` 里没有命令行） | `sql`、`clock`、`log` |
| `capture` | 记录的流程：拍窗口快照、并发采集上下文、弹框、展开、解析、保存 | 扩展点 `capture.context`、`capture.expanders`；登记 `cli.commands` | `items`、`prompt`、`clock`、`log`；可选 `window` |
| `context-window` | 来源窗口 | 登记 `capture.context` | — |
| `context-terminal` | 终端里的前台命令、目录、git | 登记 `capture.context` | `process`、`vcs` |
| `context-browser` | 浏览器当前页面的网址 | 登记 `capture.context` | `browser.history` |
| `context-screenshot` | 窗口截图 | 登记 `capture.context` | `screenshot` |
| `context-script` | 外部命令给的上下文 | 登记 `capture.context` | `exec` |
| `expand-calc` | `=算式` | 登记 `capture.expanders` | — |
| `expand-snippets` | 缩写 | 登记 `capture.expanders` | — |
| `expand-script` | 前缀 → 外部命令 | 登记 `capture.expanders` | `exec` |
| `review` | 整理界面的五个页面 | 登记 `tui.pages` | `items`、`clock` |
| `remind` | 积压和到期提醒 | 登记 `cli.commands`、`migrations` | `items`、`notify`、`sql`、`clock` |
| `dida-push` | 把条目推成滴答任务 | 订阅 `capture.saved`；登记 `review.actions`、`cli.commands`、`migrations` | `items`、`dida-tasks`、`sql`、`log`；可选 `capture` |

系统服务（mk-kit）：`storage-sqlite`、`clock-system`、`log-stderr`、`cli-clap`、`tui-ratatui`、`wm-niri`、`screenshot-niri`、`prompt-vicinae`、`process-procfs`、`vcs-git`、`exec-tokio`、`notify-dbus`、`browser-history` 和三个浏览器适配。每个组件的配置见 [配置](configuration.md)。

依赖只从应用功能指向系统服务。`capture` 不认识任何具体的上下文来源或展开器：它只声明两个扩展点，`context-*` 和 `expand-*` 来登记。同样，`capture` 不认识滴答：`#滴` 只是配置里的一个「路由」，存好后发布事件 `capture.saved { item_id, routes }`，`dida-push` 订阅它；整理界面的 `P` 是 `dida-push` 登记到 `review.actions` 扩展点上的。

滴答的组件（`dida-openapi`、`dida-tasks`、`dida-commands`）来自共享仓库 [dida](../../dida)，和 mk-kit 一样用 path 依赖，三个仓库要放在同一个目录下。

## 规则和检查

`scripts/check.sh` 依次跑格式、clippy（警告当错误）、架构规则和全部测试。`lefthook install` 之后，每次提交前都会自动跑一遍。

架构规则（`tests/arch`）根据包所在的目录判断它的角色，再检查依赖方向：

- 一个功能的 `core` 不能依赖 IO 和运行时（tokio 之类），也不能依赖别的功能。
- 一个功能的 `component` 只能依赖 kernel-api、各种契约和自己的 `core`，不能依赖别的功能的实现。
- 契约包只能依赖 kernel-api 和别的契约。
- 系统服务永远不依赖应用功能。

## 测试

| 层 | 在哪 | 测什么 |
|---|---|---|
| 纯逻辑 | `features/*/core` | 语法解析、日期、排序、到期、提醒文案……不需要运行时 |
| 契约测试 | `*/contracts` 的 `testing` 特性 | 任何实现都要通过的同一组测试。比如 `items_contract` 在真实的 SQLite 上跑，`context_contract` 和 `expander_contract` 每个提供者和展开器都跑 |
| 组件 | `features/*/component` | 用 testkit 把组件和假的系统服务装在一起（`FakeWindows`、`FakePrompt`、`FakeExec`、`FakeScreenshots`、`RecordingNotify`……） |
| 端到端 | `tests/e2e` | 跑真正的 `stash` 进程，niri socket、vicinae、`wl-copy` 都是假的 |
| 文档 | `tests/config-doc` | 配置文档里的每个默认值和代码一致 |

## 怎么扩展

**加一种上下文**（比如编辑器里打开的文件）：

1. 新建 `features/context-editor/component`，在 `Cargo.toml` 里写 `[package.metadata.component] id = "context-editor"`。
2. 实现 `capture_contracts::ContextProvider`：`spec` 给出 id 和阶段（`BeforePrompt` 或 `WhilePrompt`），`collect` 根据窗口快照返回上下文条目，不适用时返回空列表。
3. 在 `start` 里 `host.contribute(&CAPTURE_CONTEXT, …)`，`manifest` 的 `requires` 里写上 `CAPTURE_CONTEXT`。
4. 测试里跑 `capture_contracts::testing::context_contract`。
5. 在 `apps/stash/Cargo.toml` 和 `tests/config-doc/Cargo.toml` 的依赖里各加一行，并在 [配置](configuration.md) 里加一节。

不用改 `capture`，也不用改启动器的代码。参考 `features/context-window`，它是最短的一个。

**加一种展开**：同上，实现 `Expander`，登记到 `CAPTURE_EXPANDERS`，参考 `features/expand-snippets`。

**换一个基础设施**（比如用 fuzzel 代替 vicinae）：在 mk-kit 里写一个提供 `prompt` 契约的新服务，跑 `prompt_contract`，再把启动器的依赖从 `prompt-vicinae` 换成它。用到 `prompt` 的组件都不用改。

**改配置项**：同时改组件的 `config_defaults()` 和 [docs/configuration.md](configuration.md)，`tests/config-doc` 会检查两边是否一致。

## 提交

Conventional Commits，scope 写组件 id，比如 `feat(context-terminal): …`、`fix(review): …`。
