# 实现进度（交接用）

新会话从这里接着做，不要从对话历史重建。最后更新：2026-11-18。

## 一句话现状

阶段 0–3b 的代码完成：`stash capture` 记下来源窗口、终端的命令 / 目录 / git、浏览器网址、截图、自定义脚本的输出；输入支持 `=算式`、缩写、`!前缀` 外部改写，以及 `#分类 @日期` URL 语法。`stash-tui` 是整理界面（收件箱 / 待办 / 想法 / 稍后读 / 已完成，j/k 选择、单键操作）。阶段 3、3b 的真机验收要在桌面机上做（见下面清单）。`stash remind` 由 systemd timer 定时运行，收件箱积压、条目到期时发桌面通知（一天一次）。阶段 6 收尾完成：`scripts/install.sh`、用户和开发文档（配置文档由测试核对）、niri 快捷键例子、规范回写。剩下的都是桌面机上的真机验收（见「下一步」）。

档位：中等　配套：数据应用、命令行、常驻进程 / 桌面工具（部分）

## 必读文件

| 文件 | 内容 |
|---|---|
| `docs/specs/2026-11-17-stash-design.md` | 设计（§10 待决定，§11 实现中的调整） |
| `docs/plans/2026-11-17-stash-plan.md` | 实现计划 |
| `~/code/personal/lifelog` | 参考实现，kernel 和通用系统服务从这里抽出 |

## 进度

| 步 | 内容 | 状态 |
|---|---|---|
| 0 | 抽共享仓库 mk-kit | 完成；lifelogd 真机运行待在桌面机上确认 |
| 1 | stash 脚手架 | 完成 |
| 2 | `items` + 命令行 | 完成 |
| 3 | 快速记录（窗口上下文） | 代码完成；真机验收待桌面机 |
| 3b | 更多上下文和展开 | 代码完成；真机验收待桌面机 |
| 4 | `tui` v2 + 整理界面 | 完成（开发机上用 pty 真实运行过） |
| 5 | 提醒 | 代码完成；真机收到通知待桌面机 |
| 6 | 收尾 | 完成；覆盖率门槛脚本未验证（开发机没有 cargo-llvm-cov） |

## 已确认的事

- 2026-11-17：Rust；终端界面；稍后阅读只是分类，只存 URL，不抓取；不做浏览器扩展；手机同步以后再做；现在就加截止日期；复用方式是抽共享仓库；项目名 stash；分类用 `#` 标记或 `--kind` 命令参数，冲突以 `#` 为准；共享仓库 `~/code/personal/mk-kit`；不读剪贴板。
- 2026-11-17：`vicinae dmenu` 实测：自由输入 → 原样输出、退出码 0；列表非空时空回车 → 输出第一项（所以 stdin 永远给空）；Esc → 无输出、退出码 1。

- 2026-11-17：输入框动态逻辑先做「回车后展开」；实时预览和手机同步放到以后（设计 §12）；最看重尽可能多地保存上下文。
- 2026-11-17：抽 mk-kit 用 `git subtree split`；截图用 niri `screenshot-window --id --path` 并恢复剪贴板；浏览器 Zen / Chrome / qutebrowser，用窗口标题匹配最近历史拿 URL；截图默认开。

### 阶段 0 做了什么（2026-11-17）

| 仓库 | 提交 | 内容 | 怎么验证的 |
|---|---|---|---|
| mk-kit | `5dada99` 及之前 27 个 subtree 提交 | 从 lifelog 迁入 kernel、9 个契约、12 个系统服务、`tests/arch` | `scripts/check.sh` 通过 |
| mk-kit | `b5d2bff` | `boot-build` 认得 path 引入的共享组件；arch 检查认得仓库外的包 | 新测试 `knows_components_from_a_shared_repo_through_path_dependencies`；`scripts/check.sh` 通过，155 个测试 |
| lifelog | `4215b1f` | 删掉迁出的目录，改 path 依赖 `../mk-kit` | `scripts/check.sh` 通过，274 个测试；274 + 155 = 迁移前 418 + 复制的 10 个 arch 测试 + 1 个新测试 |

未验证：真机运行 `lifelogd`。开发机（这台）没有桌面环境，lifelogd 的 `idle-wayland` 连不上合成器，和迁移无关（服务原本就是 disabled、从没在这台机器上运行过）；试启动后已停掉。要在桌面机上 `scripts/install.sh` 后确认。
补充提交：lifelog `28c0c4c` 去掉迁移后不再用到的 workspace 依赖；mk-kit、lifelog 各一个提交把 arch 的 serde 挪到 dev-dependencies。

### 阶段 1 做了什么（2026-11-17）

| 提交 | 内容 | 怎么验证的 |
|---|---|---|
| 见 `git log` | workspace、`apps/stash` 启动器（装 cli-clap、clock-system、log-stderr）、`tests/arch`（复制自 mk-kit）、`scripts/check.sh`、lefthook、README | `scripts/check.sh` 通过，0 警告，21 个测试；`stash --help` 退出码 0；临时加一个依赖 tokio 的 `features/probe/core`，arch 报 `[R8] probe-core -> tokio`，删掉后恢复通过 |

### 阶段 2 做了什么（2026-11-17）

| 仓库 | 内容 | 怎么验证的 |
|---|---|---|
| mk-kit | `boot::set_default`、`boot::data_dir`；`storage-sqlite`、`files-std` 不再内置 lifelog 路径 | 新测试 `set_default_only_fills_what_the_file_left_out`、`a_missing_root_is_a_config_error`；`scripts/check.sh` 通过 |
| lifelog | 四个启动器用 `set_default` 给出原来的默认路径；配置文档同步 | `scripts/check.sh` 通过（284）；临时 `XDG_DATA_HOME` 下运行 `lifelog today`，报的路径是 `$XDG_DATA_HOME/lifelog/lifelog.db`，行为不变 |
| mk-kit | cli-clap：参数和全局选项重名时报配置错误，不再崩溃 | 新测试 `an_argument_named_like_a_global_option_is_a_config_error_not_a_crash` |
| stash | `features/items/{contracts,core,component}`：表和迁移、`Items` 契约和契约测试、`items.changed` 事件、10 个命令；启动器装上 `items` 和 `storage-sqlite`；`tests/e2e` | `scripts/check.sh` 通过，0 警告，37 个测试（契约测试在真实 SQLite 上跑；e2e 5 个：默认数据目录、JSONL、修改类命令不输出、后缀 id、错误走 stderr 且退出码 64/65/66）；临时目录里手动跑了 add / ls / done / show |

### 阶段 3 做了什么（2026-11-17）

| 仓库 | 提交 | 内容 | 怎么验证的 |
|---|---|---|---|
| mk-kit | `494e083` | `WindowInfo.pid`；`wm-niri` 事件流没同步时直接问 niri | 新测试 `asks_niri_directly_until_the_event_stream_has_the_full_picture`、`a_failed_direct_question_is_unavailable`、`replies_to_one_off_requests` |
| mk-kit | `7943d99` | `contracts/prompt`（契约测试 `prompt_contract`、`FakePrompt`）、`services/prompt-vicinae` | 假 vicinae 脚本跑契约测试；断言参数和 stdin 为空 |
| lifelog | `67c32e6` | 跟上 `pid` 字段 | `scripts/check.sh` 通过（284） |
| stash | 见 `git log` | `items`：`NewItem.id`、`RecordingItems`、`add` 命令移给 capture；`features/capture/{contracts,core,component}`；`features/context-window`；启动器装上 `wm-niri`、`prompt-vicinae`、`capture`、`context-window` | `scripts/check.sh` 通过，0 警告，56 个测试：core 覆盖 §5.1 语法表和日期写法；组件测试覆盖取消清理、慢 / 失败的提供者不挡保存、弹框期间并发采集、展开顺序和失败、`#` 优先于 `--kind`；e2e 用假 vicinae + 假 niri socket 跑真实进程 |

mk-kit 全量 167 个测试通过。

**桌面机验收清单（阶段 3，未验证）**：
1. 在 niri 配置里绑 `Mod+N { spawn "stash" "capture"; }`，从浏览器窗口按下 → 输入 → 回车；`stash show <id>` 看得到来源窗口（应用、标题、工作区、pid）。
2. 回车后焦点回到原来的窗口。
3. Esc 不保存，`stash ls` 没有新条目。
4. 量从按键到输入框出现的时间（目标 300 ms 以内）。
5. `Mod+Shift+T { spawn "stash" "capture" "--kind" "todo"; }`：提示里显示「→ 待办」。

### 阶段 3b 做了什么（2026-11-17）

| 仓库 | 提交 | 内容 | 怎么验证的 |
|---|---|---|---|
| mk-kit | `c4904ac` | `browser` 契约 v2：`recent`；三个适配按时间列查 | 契约测试加了 `recent` 的用例，三个适配和内存假实现都通过 |
| mk-kit | `727d4ab` | `process` 契约 + `process-procfs`；`vcs` 契约 + `vcs-git` | procfs 用测试进程自己跑契约测试；git 在临时仓库里跑（干净、有改动、子目录、非仓库、detached、空仓库） |
| mk-kit | `3b5b5de` | `screenshot` 契约 + `screenshot-niri`（存下并恢复剪贴板） | 假 niri / wl-paste / wl-copy 脚本：契约测试、剪贴板恢复、原来为空时清掉、关掉恢复、文件不出现、niri 不在 |
| mk-kit | `b678fcf` | `exec` 契约 + `exec-tokio` | 契约测试：stdin、环境变量、stderr、退出码、超时杀掉、起不来、空命令 |
| lifelog | — | 不用改 | `scripts/check.sh` 通过（284） |
| stash | 见 `git log` | `context-terminal`、`context-browser`、`context-screenshot`、`context-script`、`expand-calc`、`expand-snippets`、`expand-script`（有纯逻辑的各带 `core`）；启动器装上全部；`CaptureProbe` | `scripts/check.sh` 通过，0 警告，108 个测试；e2e 用假 niri（socket + 命令）、假 vicinae、假剪贴板跑真实进程：来源窗口、浏览器标题、截图文件都进了条目，Esc 删掉截图 |

mk-kit 全量检查通过。开发机上 `stash ls`、`stash capture --message` 的进程耗时 3–4 ms（release 和 debug 差不多）。
在开发机的真实进程树（herdr → fish → claude）上核对了 `context-terminal` 的判断依据：shell 是有控制终端的会话首进程，前台命令是终端前台进程组的组长。

**桌面机验收清单（阶段 3b，未验证）**：
1. 从 ghostty 里正在跑 nvim 的窗口记一条：`stash show` 有前台命令、目录、仓库、分支，列表里来源是「ghostty · ~/… (分支)」。开多个标签页时看取的是不是当前那个。
2. 从 Zen、Chrome、qutebrowser 各记一条：有网页标题和网址。核对各自的 app_id 和标题后缀（不对就改 `context-browser.apps`）。
3. 截图：`~/.local/share/stash/shots/<id>.png` 是来源窗口；**截图前后剪贴板内容不变**；niri 写剪贴板和我们恢复的先后有没有竞态。
4. Esc 后截图被删掉。
5. 配一个 `context-script`（比如 `lifelog`）和一个 `expand-script`，看输出进了条目。
6. 从按键到输入框出现的时间（截图在弹框之前做，最多等 300 ms）。

### 阶段 4 做了什么（2026-11-18）

| 仓库 | 提交 | 内容 | 怎么验证的 |
|---|---|---|---|
| mk-kit | `04dd8c7` | `tui` 契约 v2（`Block::List`、`PageSpec.actions`、`act`、`ActionResult{message,reload,select,ask,open}`）；`tui-ratatui`：选中和高亮、详情区、确认、两步输入、提示行、`opener` 打开、按键冲突检查、F5 | 单元测试 33 个（状态机、排版、TestBackend 画面、冲突检查、opener 真的运行命令）；`scripts/check.sh` 通过 |
| lifelog | `56bec32` | 页面补 `act`；配置文档加 `opener`（config-doc 测试发现的） | `scripts/check.sh` 通过 |
| stash | 见 `git log` | `features/review/{core,component}`、`apps/stash-tui`；`items` 的命令行依赖改为可选 | `scripts/check.sh` 通过，0 警告，125 个测试；`review` 组件测试用真的 items + 内存 SQLite + 假外壳覆盖每个按键；pty 里真实运行 `stash-tui`：切页、选择、完成、改文字（预填）、删除（二次确认）、退出码 0，退出后命令行核对数据库 |

### 阶段 5 做了什么（2026-11-18）

| 仓库 | 提交 | 内容 | 怎么验证的 |
|---|---|---|---|
| mk-kit | `f5a586e` | `notify` 契约（`RecordingNotify`）+ `notify-dbus`（zbus，不依赖 notify-send） | 起私有 `dbus-daemon`、挂假的 `org.freedesktop.Notifications`：收到的应用名、标题、正文、紧急程度都对；没有通知服务、总线不存在是 `unavailable` |
| stash | 见 `git log` | `features/remind/{core,component}`（表 `remind_sent`）、`stash remind [--dry-run]`、`dist/stash-remind.{service,timer}`；启动器装上 `notify-dbus`、`remind` | `scripts/check.sh` 通过；core 6 个测试（阈值、合并、一天一次、提前天数）；组件 5 个（真 items + 内存 SQLite + 假通知：同一天不重复、当天新到期再提醒、第二天再提醒、dry-run 不发不记、通知失败不记）；e2e：`--dry-run` 的输出，私有总线上没有通知服务时退出码 69、什么都没记；`systemd-analyze verify` 只报 `~/.cargo/bin/stash` 还没装 |

**桌面机验收清单（阶段 5，未验证）**：
1. 装好后 `stash remind --dry-run` 看要发什么，再 `stash remind`，桌面上收到通知（标题、正文、过期时是紧急样式）。
2. `systemctl --user enable --now stash-remind.timer`，`systemctl --user list-timers` 看下次运行时间；`journalctl --user -u stash-remind` 看日志。

### 阶段 6 做了什么（2026-11-18）

| 仓库 | 提交 | 内容 | 怎么验证的 |
|---|---|---|---|
| mk-kit | `0de3a0b` | `crates/config-doc`：配置文档的解析、对照和整份检查 `check()`；arch 认得这个包 | 6 个单元测试（含 `check` 的缺段、未链接、不存在的组件）；`scripts/check.sh` 通过 |
| lifelog | `a554d94` | `tests/config-doc` 改用共享包（包名改为 `config-doc-test`），去掉不再用的 `toml` | `scripts/check.sh` 通过，0 警告；故意把文档里的 `queue_capacity` 改成 1，检查报出「文档写的是 1，代码里是 1024」 |
| stash | 见 `git log` | `tests/config-doc` + `docs/configuration.md`（28 个组件）；`docs/{README,installation,usage,architecture}.md`；README 重写；`scripts/install.sh`；`dist/niri-binds.kdl`；`scripts/coverage.sh` + `coverage_gate.py` | `scripts/check.sh` 通过，0 警告，138 个测试；故意改错一个默认值、加一个不存在的键，两处都报出；文档里的配置例子拼成配置文件真实加载（`stash parse` 用上了例子里的缩写和 `#做` 标记）；所有相对链接都存在；`install.sh` 在临时 `CARGO_INSTALL_ROOT`、`XDG_CONFIG_HOME` 下跑通，单元文件里的路径换成了实际位置，`systemd-analyze --user verify` 无报错，装出来的 `stash add / ls / remind --dry-run` 正常；`coverage_gate.py` 用构造的报告测过（不达标的包报出、`apps/` 和 mk-kit 的文件跳过） |

- **未验证**：`scripts/coverage.sh` 真实跑一次。开发机没有 cargo-llvm-cov，安装它的操作被权限拦下。装好之后（`sudo pacman -S cargo-llvm-cov`、`rustup component add llvm-tools-preview`）跑一次，达标再考虑放进 `check.sh`。
- **补阶段 5 的漏**：阶段 5 的提交里 `tests/e2e/tests/cli.rs` 没有格式化，那次汇报的「`scripts/check.sh` 通过」不成立（格式这一步会失败）。这次一起格式化了。lefthook 没装，所以提交时没拦下；建议 `lefthook install`。
- **规范回写**（`~/.agents/skills/microkernel-architecture`，下面「对规范的反馈」各条）：`stack-rust.md` 加「共享仓库」「扩展点的声明」「测试」三节；原则 8 加「运行外部命令也走 exec 服务」；原则 4 加终端界面的可操作列表；常驻进程 / 桌面工具配套加「没有常驻进程的桌面工具」一节；数据应用配套加「没有常驻进程时的多写者」；SKILL.md 把 stash 列为第二个参考实现。

**这台开发机没有桌面环境**：所有要 niri / vicinae / 通知的真机验收（阶段 3、3b、4、5）都要在桌面机上做，计划里把它们列成「桌面机验收清单」，开发机上只跑自动测试。

## 对规范的反馈

以下各条都已在阶段 6 回写到 `microkernel-architecture`（2026-11-18）。

- 测试里「写脚本再 exec」在并行测试中会偶发 `Text file busy`（别的线程 fork 出的子进程带着写句柄）。这类测试要串行。回写到 `references/stack-rust.md` 的测试部分。
- 应用功能要运行用户配置的外部命令时，通过一个通用的 `exec` 系统服务，而不是自己起进程。回写到原则 8 的例子里。

- 扩展点的收集方要把扩展点写进自己的 `provides`（不写的话 kernel 报「不是自己 provides 里的扩展点」），登记方写进 `requires`。规范里没说清，阶段 6 回写。
- 一次性命令行进程用「靠事件流维护状态」的系统服务时，状态在启动瞬间是空的；这类服务要有「还没同步就直接查询」的路径。回写到常驻进程 / 桌面工具配套。

- 系统服务的默认值不能带应用名：数据目录这类应用相关的默认值由启动器 `set_default`。回写到 `references/stack-rust.md` 和常驻进程 / 桌面工具配套。
- 共享仓库（多个项目共用 kernel 和系统服务）规范里没写：`boot-build` 的组件发现、arch 检查的角色判断都假设组件在本仓库里，要改成认得 path 依赖进来的包。阶段 6 回写到 `references/stack-rust.md`。


## 待用户决定

- 无

## 下一步

都在桌面机上做：

1. `scripts/install.sh`，按 `dist/niri-binds.kdl` 绑快捷键。
2. 阶段 3、3b、5 的验收清单（上面各阶段下面）。
3. 在真终端里看一眼 `stash-tui`（布局、颜色、中文宽度）。
4. lifelog：`scripts/install.sh` 后确认 lifelogd 正常。
5. 可选：装 cargo-llvm-cov，跑 `scripts/coverage.sh`。
