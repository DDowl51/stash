# 实现进度（交接用）

新会话从这里接着做，不要从对话历史重建。最后更新：2026-11-17。

## 一句话现状

阶段 0–2 完成：`stash add / ls / show / move / done / …` 能用（数据在 `~/.local/share/stash/stash.db`）。下一步阶段 3：快速记录（vicinae 输入框 + 窗口上下文）。上下文采集（§5.2）和输入展开（§5.3）已写进设计；待决定项全部已定。

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
| 3 | 快速记录（窗口上下文） | 未开始 |
| 3b | 更多上下文和展开 | 未开始 |
| 4 | `tui` v2 + 整理界面 | 未开始 |
| 5 | 提醒 | 未开始 |
| 6 | 收尾 | 未开始 |

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

**这台开发机没有桌面环境**：所有要 niri / vicinae / 通知的真机验收（阶段 3、3b、4、5）都要在桌面机上做，计划里把它们列成「桌面机验收清单」，开发机上只跑自动测试。

## 对规范的反馈

- 系统服务的默认值不能带应用名：数据目录这类应用相关的默认值由启动器 `set_default`。回写到 `references/stack-rust.md` 和常驻进程 / 桌面工具配套。
- 共享仓库（多个项目共用 kernel 和系统服务）规范里没写：`boot-build` 的组件发现、arch 检查的角色判断都假设组件在本仓库里，要改成认得 path 依赖进来的包。阶段 6 回写到 `references/stack-rust.md`。


## 待用户决定

- 无

## 下一步

1. 阶段 3：`prompt` 契约 + `prompt-vicinae`、`capture`（两个扩展点、输入语法）、`context-window`、mk-kit 的 `window` 加 `pid`；`stash add` 改走 capture 的语法。`items` 契约包补一个内存假实现给 capture 的测试用。
2. 桌面机上：lifelog `scripts/install.sh` 后确认 lifelogd 正常。
