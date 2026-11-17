# stash 实现计划

- 日期：2026-11-17
- 设计：`docs/specs/2026-11-17-stash-design.md`

每个阶段结束都跑 `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`，全部通过才进入下一阶段。阶段 0 还要在 lifelog 里跑同样的命令。

## 阶段 0：抽出共享仓库

- 方式已定：`git subtree split`。
- 新建 `~/code/personal/mk-kit`，把 lifelog 的 `crates/*`、`contracts/{cli,clock,log,sql,timer,tui,window}`、对应的 `services/*`、`tests/arch` 的规则库移过去，保留 git 历史（`git subtree split`）。
- lifelog 的 `Cargo.toml` 改成 path 依赖 mk-kit。
- 验收：mk-kit 和 lifelog 各自全量 `cargo test` 通过；lifelog 的 `tests/arch` 和 `tests/e2e` 通过；`lifelogd` 真机跑一次。

## 阶段 1：stash 脚手架

- workspace、`apps/stash`（只有启动器）、`tests/arch`、lefthook、`scripts/check.sh`。
- 验收：`stash --help` 能运行；故意让一个 `core` 依赖 tokio，`tests/arch` 报错。

## 阶段 2：`items` + 命令行

- `features/items/{contracts,core,component}`：表、迁移、`Items` 契约和契约测试、`items.changed` 事件、`stash item add/list/move/done/archive/rm/due`。
- `stash item add` 先用最简单的解析（原文），语法解析放到阶段 3。
- 验收：e2e 测试 `stash item add 'x' && stash item list | jq` 能拿到 JSON；退出码有测试。

## 阶段 3：快速记录（只带窗口上下文）

- mk-kit：`window` 契约的 `WindowInfo` 加可选的 `pid`，`wm-niri` 填上；lifelog 测试仍通过。
- `contracts/prompt` + `services/prompt-vicinae`（契约测试用假的 `vicinae` 脚本，断言 stdin 为空）。
- `features/capture/{contracts,core,component}`：两个扩展点、采集流程（快照 → BeforePrompt → 弹框 + WhilePrompt → 超时）、内置 `#`、`@`、URL 规则；`stash capture`、`stash parse`、`stash item show`。
- `features/context-window`。
- 验收：`core` 单元测试覆盖 §5.1 的语法表和超时 / 失败的取舍；组件测试用假 prompt / window / 假提供者走通；**真机**：niri 绑 `Mod+N`，从别的窗口呼出 → 输入 → 回车，`stash item show` 能看到来源窗口，焦点回到原窗口；Esc 不保存；量按键到输入框出现的时间（目标 300 ms 内）。

## 阶段 3b：更多上下文和展开

- `process-procfs` + `vcs-git` + `context-terminal`。
- `screenshot-niri`（`--id --path` + 恢复剪贴板）+ `context-screenshot`。
- mk-kit：从 lifelog 搬 `browser` 契约和 `browser-history`、`browser-firefox`、`browser-chromium`、`browser-qutebrowser`，契约加 `recent`（v2）；lifelog 测试仍通过。然后做 `context-browser`。
- `context-script`。
- `expand-calc`、`expand-snippets`、`expand-script`。
- 验收：各契约测试；真机在终端（git 仓库里）、浏览器、聊天窗口各记一条，`stash item show` 看到对应上下文；故意配置一个会超时的脚本，条目照常保存并记下超时。

## 阶段 4：`tui` v2 + 整理界面

- 在 mk-kit 里给 `tui` 契约加 v2（可选中的列表、页面声明的按键、`act`），`tui-ratatui` 实现，契约测试；lifelog 的页面不改也能通过测试。
- `features/review` + `apps/stash-tui`。
- 验收：契约测试；`review` 用假的 tui 外壳测各操作；真机用独立 socket 的 tmux（`tmux -L stash-test`）运行并截屏。

## 阶段 5：提醒

- `contracts/notify` + `services/notify-dbus`；`features/remind/{core,component}`；`stash remind [--dry-run]`；`dist/systemd/stash-remind.{service,timer}`。
- 验收：`remind/core` 测试提醒规则和「一天一次」；`stash remind --dry-run` 打印将要发的通知；真机收到一条通知。

## 阶段 6：收尾

- README、`docs/guide`（安装、niri 配置示例、配置说明）、`docs/dev`、`scripts/install.sh`。
- 覆盖率门槛。
- 把「简单档以外」用到的未验证做法（无常驻进程的多写者、`tui` v2）回写到 microkernel-architecture 规范。
