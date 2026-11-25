# 使用

## 记录

### 用快捷键（`stash capture`）

按快捷键后输入框弹出，输入，回车。输入框消失，焦点回到原来的窗口。按 Esc 或者什么都不写就回车，都不保存。

输入里可以写：

| 写法 | 效果 |
|---|---|
| `帮老王看 PR #t` | `#` 加标记指定分类：`#t` 待办、`#i` 想法、`#r` 稍后读、`#in` 收件箱。可以写在任意位置，保存时从文字里去掉。标记可以在配置里改 |
| `#C++` | 不是配置里的标记，原样保留 |
| `@今天` `@明天` `@后天` `@周五` `@下周一` `@3天后` `@+3` `@11-20` `@2026-11-20` | 截止日期，保存时从文字里去掉。`@周五` 指今天之后最近的那个周五（今天是周五就指下周五） |
| 只有一个链接 | 进稍后读（`capture.url_kind`），链接单独存一份 |
| 文字里带链接 | 分类不变，第一个链接单独存一份 |
| `买 3 个 =3*45` | 记成 `买 3 个 135`，原式记进上下文 |
| `;会`（配置的缩写） | 换成模板文字 |
| `!gh 123`（配置的前缀） | 交给你的脚本改写 |
| `看 PR #滴` | 存好后再推一份到滴答清单（见下面的「滴答清单」），`#滴` 从文字里去掉。命令行写 `--to dida` 一样 |

什么都没写时进收件箱。快捷键带了 `--kind`、文字里又写了 `#`，以 `#` 为准。

按键的那一刻，stash 还会记下「当时在干什么」：

| 什么时候 | 记什么 |
|---|---|
| 总是 | 来源窗口的应用、标题、工作区、pid |
| 来源是终端 | 前台命令、工作目录、git 仓库、分支、提交、远端、有没有没提交的改动 |
| 来源是浏览器 | 当时那一页的标题和网址 |
| 默认 | 来源窗口的截图（`context-screenshot.skip_apps` 里的应用除外） |
| 你配置的 | 外部脚本的输出（[配置](configuration.md#context-script)） |

这些都在你打字的同时采集，回车后最多再等 1 秒。哪一项失败或超时都不影响保存，失败原因会记在条目里。

### 不弹框（`stash add`、`stash capture --message`）

```bash
stash add "给房东转账 #t @今天"            # 不采上下文，适合脚本
stash capture --message "这个报错之后再查"   # 不弹框，但照样采上下文
```

### 先试试会记成什么样（`stash parse`）

```console
$ stash parse "买 3 个 =3*45 #t @明天"
文字  买 3 个 135
分类  待办
截止  2026-11-19
上下文  算式  3*45 = 135
```

只展开和解析，不保存。调试缩写和脚本时用。

## 整理（`stash-tui`）

```bash
stash-tui
```

| 页 | 内容 | 顺序 |
|---|---|---|
| `1` 收件箱 | 还没分类的 | 最新的在上面 |
| `2` 待办 | | 手动排序；过期的、今天到期的用醒目的样式标出 |
| `3` 想法 | | 手动排序 |
| `4` 稍后读 | | 手动排序 |
| `5` 已完成 | 完成的（归档的不在这里，用 `stash ls --status archived` 看） | 最近的在上面 |

上半部分是列表，下半部分是选中条目的详情：全部上下文。最下面一行是当前页能用的按键。

| 键 | 作用 |
|---|---|
| `j` `k`（或 `↓` `↑`） | 选择 |
| `1`–`5`、`Tab`、`Shift+Tab` | 换页 |
| `PageUp` `PageDown` | 一次移 10 行 |
| `b` `t` `i` `r` | 移到收件箱 / 待办 / 想法 / 稍后读（当前页那一类不显示） |
| `x` | 完成 |
| `a` | 归档 |
| `u` | 恢复成未完成（在已完成页） |
| `e` | 改文字 |
| `D` | 设截止日期：写法和记录时的 `@` 一样（`2026-11-20`、`11-20`、`明天`、`周五`、`下周一`、`+3`……），清空后回车表示去掉截止日期 |
| `K` `J` | 上移 / 下移（待办、想法、稍后读） |
| `o` | 打开链接 |
| `O` | 看截图 |
| `d` | 删除，再按一次 `d` 确认 |
| `P` | 推到滴答清单（见下面） |
| `F5` | 刷新（默认每 30 秒也会自动刷新） |
| `Esc` | 取消确认或输入；没在确认时退出 |
| `q` | 退出 |

打开链接和截图用的是 `xdg-open`，可以在 `tui-ratatui.opener` 里换。

## 命令行

```
stash ls [--kind inbox|todo|idea|read] [--status open|done|archived] [--due overdue|today|week] [--limit N]
stash show <id>
stash move <id> <inbox|todo|idea|read>
stash done <id>  |  archive <id>  |  reopen <id>  |  rm <id>
stash due <id> <YYYY-MM-DD|none>
stash edit <id> <新的文字>
stash remind [--dry-run]
```

```console
$ stash ls
CTTMH5  [收件箱]  看看 vicinae 的插件
5MSYGS  [待办]  @2026-11-20  回复老王关于周会的消息
HF74E7  [待办]  @2026-11-18  给房东转账
ZCBR2V  [想法]  stash 可以加个 MPRIS 上下文
ZTKWT0  [稍后读]  https://blog.rust-lang.org/2026/11/01/async.html
$ stash done hf74
```

- **id**：前面那 6 位是完整 id 的最后 6 位。写最后 4 位以上就行，大小写都可以；对上不止一条时会报错。
- **输出**：stdout 是终端时输出给人看的文本，否则每行一个 JSON（JSONL）。`--json`、`--text` 可以强制选一种。
- **修改类命令**（`move`、`done`、`due` …）成功时什么都不输出。
- **退出码**：`0` 成功；`64` 用法不对；`65` 输入不对（日期写错、文字是空的）；`66` 找不到条目；`69` 需要的服务不可用（比如没有通知服务）；`78` 配置写错了。

```bash
# 例子：今天到期的待办，发到手机上
stash ls --kind todo --due today --json | jq -r .text | …
```

## 滴答清单

stash 可以把条目推成[滴答清单](https://dida365.com)的任务，也能直接用滴答的命令。这部分来自共享仓库 dida，配置见 [配置](configuration.md#滴答清单)。先登录一次（和独立的 `dida` 共用）：

```bash
stash dida login            # 打开浏览器授权；ssh 里用 --manual，再 --callback '<网址>'
```

**推到滴答**：记录时写 `#滴`，或者 `stash dida push <id>`，或者在 `stash-tui` 里按 `P`。条目原样留在 stash，只多一条「滴答任务」链接；两边以后各管各的。没推过去（比如断网）时条目照样存着，记一条失败原因：

```bash
stash dida push --retry     # 重推所有失败过的
stash dida push <id> --force  # 推过的再推一次
```

**滴答自己的命令**：`stash dida ls [today|week|inbox|all|<清单>]`、`stash dida add "买菜 @明天 !高 ~生活"`、`done`、`due`、`mv`、`projects`、`sync`，和独立的 `dida` 一样。

## 隐私

stash 会记下你按键那一刻的窗口标题、终端里的命令和目录、浏览器网址，还有窗口截图。这些都只存在本机（`~/.local/share/stash`），不会发到任何地方。

- 密码管理器这类窗口，放进 `context-screenshot.skip_apps` 就不截图。
- 不想要哪一类上下文，就把对应组件关掉，比如 `[components.context-browser] enabled = false`。
- 删除条目（`stash rm`、`stash-tui` 里按 `d`）**不会**删掉它的截图。截图在 `~/.local/share/stash/shots/<条目 id>.png`，要清理的话自己删。
