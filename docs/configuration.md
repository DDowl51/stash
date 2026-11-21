# 配置

这里列出全部配置项、默认值和含义。每个组件的默认值写成一个 TOML 块，`tests/config-doc` 会核对这些块和代码里的默认值是否一致。改了配置项而没改这里，`scripts/check.sh` 会失败。

## 文件

- 位置按顺序找：`$STASH_CONFIG` → `$XDG_CONFIG_HOME/stash/config.toml` → `~/.config/stash/config.toml`。也可以用 `--config <文件>` 指定。`stash` 和 `stash-tui` 读同一个文件。
- 文件可以不存在，那样全部用默认值。只写想改的键，没写的取下面的默认值。
- 文件里有一段 `[kernel]`，每个组件一段 `[components.<组件 id>]`。

## 通用规则

| 规则 | 说明 |
|---|---|
| `enabled = false` | 每个组件都可以关掉。必须依赖它的组件会在启动时报错；可选依赖它的组件会跳过那部分功能 |
| 写错键名 | 启动时报错，不会被悄悄忽略 |
| 写错组件 id | 启动时报错，并列出所有组件 id |
| 别的进程的组件 | 比如 `stash-tui` 读到 `[components.capture]` 会直接忽略，因为它不在这个进程里 |
| `<键>_cmd = "命令"` | 任何键都可以这样写：启动时用 `sh -c` 运行这条命令，把输出（去掉末尾换行）当作 `<键>` 的值。`<键>` 和 `<键>_cmd` 不能同时写；关掉的组件不会运行 |
| `~` | 路径开头的 `~` 换成主目录 |
| 注释掉的键 | 下面的块里注释掉的键（`# path = …`）表示默认不写。不写时另有规则，见那一节的说明 |

不在 niri 会话里（没有 `NIRI_SOCKET`，比如 ssh 登录）时，`stash` 的启动器默认关掉 `wm-niri`、`screenshot-niri`、`context-screenshot`，命令行照样能用。配置里写了 `enabled = true` 就以配置为准。

### 哪个组件在哪个进程里

没有常驻进程。每次运行 `stash` 或 `stash-tui`，进程自己打开数据库，用完就退出。

| 进程 | 组件 |
|---|---|
| `stash`（命令行、快捷键、提醒） | 除 `review`、`tui-ratatui` 之外的全部组件 |
| `stash-tui`（整理界面） | `review`、`tui-ratatui`、`items`、`storage-sqlite`、`clock-system`、`log-stderr` |

`items` 这类两个进程都有的组件，两个进程读的是同一段配置。

## kernel

```toml
[kernel]
queue_capacity = 1024
stop_timeout_secs = 5
```

| 键 | 说明 |
|---|---|
| `queue_capacity` | 事件队列最多能排多少条还没投递的事件。满了以后，发布方会等待 |
| `stop_timeout_secs` | 退出时等每个组件收尾多久，超时就不再等 |

## 条目

### items

条目的存储和读写，以及 `stash ls / show / move / done / …` 命令。没有配置项。

```toml
[components.items]
```

### capture

快速记录：按快捷键时拍下当前窗口，弹出输入框，同时采集上下文，回车后展开输入、解析 `#分类 @日期`，然后保存。

```toml
[components.capture]
placeholder = "记点什么…  #t 待办  #i 想法  #r 稍后读  @明天"
url_kind = "read_later"
window_timeout_ms = 200
before_prompt_timeout_ms = 300
context_timeout_ms = 1000
context_order = ["window", "terminal", "browser", "screenshot", "script"]
expander_order = ["calc", "snippets", "script"]
tags = { inbox = ["inbox", "in"], todo = ["todo", "t", "待办"], idea = ["idea", "i", "想法"], read_later = ["read", "r", "读"] }
```

| 键 | 说明 |
|---|---|
| `placeholder` | 输入框里的提示文字。用 `--kind` 指定分类时，后面会加上「→ 待办」之类 |
| `url_kind` | 整段只有一个 URL、又没写 `#` 时归到哪类：`inbox`、`todo`、`idea`、`read_later` |
| `window_timeout_ms` | 问窗口管理器「当前是哪个窗口」最多等多久。超时就当作没有窗口信息 |
| `before_prompt_timeout_ms` | 弹出输入框之前要跑完的上下文（截图）最多等多久 |
| `context_timeout_ms` | 回车之后，还没采完的上下文最多再等多久。超时的那一项丢掉，并在条目里记一条「超时」 |
| `context_order` | 条目里各项上下文的排列顺序，写提供者 id。没列出的按 id 排在后面 |
| `expander_order` | 输入展开的执行顺序，写展开器 id。没列出的按 id 排在后面 |
| `tags` | `#` 后面的词对应哪个分类。不在这里的 `#词`（比如 `#C++`）原样留在文字里 |

## 上下文

按快捷键的那一刻采集「当时在干什么」。任何一项失败都不影响保存：失败原因会记成一条文字上下文。

### context-window

来源窗口：应用、标题、工作区、pid。没有配置项。

```toml
[components.context-window]
```

### context-terminal

来源窗口是终端时，记下前台命令、工作目录，以及 git 仓库、分支、提交和远端。做法是从终端的 pid 往下找子进程。

```toml
[components.context-terminal]
apps = ["com.mitchellh.ghostty", "kitty", "foot", "footclient", "Alacritty", "org.wezfurlong.wezterm"]
max_depth = 6
max_processes = 300
```

| 键 | 说明 |
|---|---|
| `apps` | 哪些 app_id 算终端。用 `niri msg windows` 可以查到 app_id |
| `max_depth` | 从终端进程往下最多找几层子进程 |
| `max_processes` | 最多看多少个进程，防止进程树太大时拖慢记录 |

终端复用器（tmux、zellij）里的 shell 不是终端窗口的子进程，所以只能记下复用器本身。这种情况下可以用 `context-script` 自己查，比如 `tmux display -p "#{pane_current_path}"`。

### context-browser

来源窗口是浏览器时，记下当时在看的网页的标题和网址。做法是从窗口标题去掉浏览器后缀，得到网页标题，再在这个浏览器最近的历史里找标题相同的最新一条。找不到时只记标题，并注明「没找到网址」。

```toml
[components.context-browser]
lookback_minutes = 30
max_visits = 300
apps = { zen = { browser = "zen", title_suffixes = [" — Zen Browser", " - Zen Browser"] }, firefox = { browser = "firefox", title_suffixes = [" — Mozilla Firefox", " - Mozilla Firefox"] }, google-chrome = { browser = "chrome", title_suffixes = [" - Google Chrome"] }, chromium = { browser = "chromium", title_suffixes = [" - Chromium"] }, "org.qutebrowser.qutebrowser" = { browser = "qutebrowser", title_suffixes = [" - qutebrowser"] } }
```

| 键 | 说明 |
|---|---|
| `lookback_minutes` | 往回找多久的历史 |
| `max_visits` | 每个浏览器最多看多少条历史 |
| `apps` | app_id → `browser`：历史来源里的浏览器名，也就是下面 `browser-*` 组件里的 `name`；`title_suffixes`：窗口标题末尾的浏览器后缀，按顺序试，去掉第一个匹配上的。键以 `*` 结尾表示前缀，见下面 |

写成多行更好读：

```toml
# 例子
[components.context-browser.apps]
zen = { browser = "zen", title_suffixes = [" — Zen Browser"] }
"org.qutebrowser.qutebrowser" = { browser = "qutebrowser", title_suffixes = [" - qutebrowser"] }
```

注意：写了 `apps` 就会整个替换默认值，不是在默认值上追加。

键以 `*` 结尾时，表示「app_id 以 `*` 前面的文字开头的窗口都算」。适合每个 profile 一个 app_id 的情况，比如用 `--desktop-file-name qutebrowser-<名字>` 打开的 qutebrowser：

```toml
# 例子
[components.context-browser.apps]
"org.qutebrowser.qutebrowser" = { browser = "qutebrowser", title_suffixes = [" - qutebrowser"] }
"qutebrowser-*" = { browser = "qutebrowser", title_suffixes = [" - qutebrowser"] }
```

精确的键优先；都不精确匹配时，取前缀最长的那个。`*` 只能写在末尾，写在别处启动时报错。只有这里的 `apps` 支持前缀，`context-terminal.apps`、`context-screenshot.skip_apps`、`context-script` 的 `when_apps` 都是精确匹配。

### context-screenshot

截下来源窗口。截图放在 `<dir>/<条目 id>.png`；按 Esc 取消时会删掉。

```toml
[components.context-screenshot]
# dir = "~/.local/share/stash/shots"
skip_apps = []
```

| 键 | 说明 |
|---|---|
| `dir` | 截图目录。不写时用 `$XDG_DATA_HOME/stash/shots`（通常是 `~/.local/share/stash/shots`），这个默认值由启动器给出 |
| `skip_apps` | 这些 app_id 不截图，比如密码管理器 |

### context-script

记录时运行你配置的外部命令，把它的输出记成上下文。可以用来接入 stash 不认识的程序。

```toml
[components.context-script]
scripts = []
```

每个脚本是一项 `[[components.context-script.scripts]]`：

```toml
# 例子：记下当时在放的音乐
[[components.context-script.scripts]]
name = "music"
command = ["playerctl", "metadata", "--format", "{{artist}} - {{title}}"]
when_apps = []
timeout_ms = 800
```

| 键 | 说明 |
|---|---|
| `name` | 出现在上下文的 key 里：`script.<name>…` |
| `command` | 程序和参数，不经过 shell。要用管道就写 `["sh", "-c", "…"]` |
| `when_apps` | 只在来源窗口是这些 app_id 时运行；空表示总是运行 |
| `timeout_ms` | 超过这个时间就不等了，默认 1000 |

命令能从环境变量拿到来源窗口的信息：`STASH_APP_ID`、`STASH_TITLE`、`STASH_PID`、`STASH_WINDOW_ID`、`STASH_WORKSPACE`、`STASH_ITEM_ID`，以及按键的时刻 `STASH_AT`（RFC 3339，带本地时区）和 `STASH_AT_MS`（毫秒）。

输出有两种写法：每行一个 JSON `{"key": "…", "label": "…", "kind": "text|url|path|image|command", "value": "…"}`；或者是纯文本，这时整段记成一条。退出码不是 0 时只记失败原因。

## 输入展开

回车以后，按 `capture.expander_order` 的顺序依次处理输入的文字。展开失败时按原文保存，失败原因记进上下文，记下的东西永远不会丢。想看一段文字最后会记成什么样，可以用 `stash parse <文字>` 试。

### expand-calc

把 `=算式` 换成结果：`买 3 个 =3*45` 记成 `买 3 个 135`，原式记进上下文。支持 `+ - * / % ^`、括号、小数。没有配置项。

```toml
[components.expand-calc]
```

### expand-snippets

把缩写换成模板文字。

```toml
[components.expand-snippets]
snippets = {}
```

```toml
# 例子
[components.expand-snippets.snippets]
";会" = "开会时提到："
";回" = "回复："
```

### expand-script

输入以某个前缀开头时，把前缀后面的文字交给外部命令改写。

```toml
[components.expand-script]
rules = []
```

```toml
# 例子：!gh 123 → 换成 issue 的标题和链接
[[components.expand-script.rules]]
prefix = "!gh "
command = ["stash-gh-issue"]
timeout_ms = 1500
```

| 键 | 说明 |
|---|---|
| `prefix` | 输入以它开头时触发 |
| `command` | 程序和参数，不经过 shell。前缀后面的文字从 stdin 传入 |
| `timeout_ms` | 超时按原文保存，默认 1000 |

命令的 stdout 有两种写法：纯文本，作为新的文字；或者一个 JSON 对象，可以同时改 `text`、`kind`、`due_on`、`url`，并追加 `context`（格式和 `stash parse <文字> --json` 的输出相同）。

## 提醒

### remind

`stash remind` 检查收件箱积压和到期的条目，发桌面通知。同一件事一天只提醒一次。由 `stash-remind.timer` 定时运行。

```toml
[components.remind]
inbox_threshold = 5
due_days_ahead = 0
max_listed = 5
keep_days = 30
```

| 键 | 说明 |
|---|---|
| `inbox_threshold` | 收件箱超过几条就提醒 |
| `due_days_ahead` | 提前几天提醒到期，`0` 表示当天 |
| `max_listed` | 一条通知里最多列出几个条目，多出来的写「……还有 N 条」 |
| `keep_days` | 「今天提醒过什么」的记录保留多少天 |

## 整理界面

### review

`stash-tui` 里的五个页面：收件箱、待办、想法、稍后读、已完成。按键见 [usage.md](usage.md#整理stash-tui)。没有配置项。

```toml
[components.review]
```

### tui-ratatui

终端界面的外壳，负责选择、按键、输入、确认和重绘。

```toml
[components.tui-ratatui]
refresh_secs = 30
opener = ["xdg-open"]
```

| 键 | 说明 |
|---|---|
| `refresh_secs` | 每隔多久自动刷新一次（别的进程可能刚记了新条目）。`0` 表示不自动刷新，可以按 F5 手动刷新 |
| `opener` | 打开链接和截图用的命令，网址或路径加在最后 |

## 系统服务

### wm-niri

从 niri 读窗口和工作区信息。

```toml
[components.wm-niri]
# socket = "/run/user/1000/niri.….sock"
reconnect_initial_ms = 1000
reconnect_max_ms = 60000
```

| 键 | 说明 |
|---|---|
| `socket` | niri 的 socket。不写就用环境变量 `NIRI_SOCKET` |
| `reconnect_initial_ms` | 断线后第一次重连前等多久，之后每次翻倍 |
| `reconnect_max_ms` | 重连等待时间的上限 |

### screenshot-niri

用 `niri msg action screenshot-window` 截窗口。niri 截图时会把图片放进剪贴板，所以默认截完后把剪贴板恢复原样。

```toml
[components.screenshot-niri]
niri = "niri"
wl_paste = "wl-paste"
wl_copy = "wl-copy"
restore_clipboard = true
timeout_ms = 3000
```

| 键 | 说明 |
|---|---|
| `niri`、`wl_paste`、`wl_copy` | 这三个命令。不在 PATH 里时写完整路径 |
| `restore_clipboard` | 截完是否把剪贴板恢复成截图之前的样子（需要 `wl-clipboard`） |
| `timeout_ms` | 最多等多久截图文件写完 |

### prompt-vicinae

用 `vicinae dmenu` 弹输入框。

```toml
[components.prompt-vicinae]
command = "vicinae"
# width = 700
# height = 400
```

| 键 | 说明 |
|---|---|
| `command` | vicinae 命令。不在 PATH 里时写完整路径 |
| `width`、`height` | 窗口大小（像素）。不写就用 vicinae 的默认值 |

### process-procfs

读 `/proc`，按 pid 查进程，以及它的子进程、工作目录、命令行、控制终端。

```toml
[components.process-procfs]
root = "/proc"
```

| 键 | 说明 |
|---|---|
| `root` | proc 文件系统挂载在哪里。只有测试时才需要改 |

### vcs-git

调用 git：按目录查仓库根目录、分支、提交、远端，以及有没有没提交的改动。

```toml
[components.vcs-git]
command = "git"
remote = "origin"
```

| 键 | 说明 |
|---|---|
| `command` | git 命令 |
| `remote` | 记哪个远端的地址 |

### browser-firefox

读 Firefox 系浏览器（Zen、Firefox）的历史。先把数据库复制出来再读，不会锁住浏览器正在用的文件。

```toml
[components.browser-firefox]
installs = [{ name = "zen", dir = "~/.zen" }, { name = "firefox", dir = "~/.mozilla/firefox" }]
```

| 键 | 说明 |
|---|---|
| `installs` | 每项是一个浏览器：`name` 是历史来源里的浏览器名，`dir` 是 `profiles.ini` 所在的目录。不存在的目录会跳过 |

### browser-chromium

读 Chrome 系浏览器的历史。

```toml
[components.browser-chromium]
installs = [{ name = "chrome", dir = "~/.config/google-chrome" }, { name = "chromium", dir = "~/.config/chromium" }]
skip_profiles = []
```

| 键 | 说明 |
|---|---|
| `installs` | 每项是一个浏览器：`name` 是历史来源里的浏览器名，`dir` 是 `Local State` 所在的目录。不存在的目录会跳过 |
| `skip_profiles` | 不读的 profile，写显示名或目录名（`Default`、`Profile 3`） |

### browser-qutebrowser

读 qutebrowser 的历史，包括用 `--basedir` 打开的独立实例。

```toml
[components.browser-qutebrowser]
data_dir = "~/.local/share/qutebrowser"
basedirs = ["~/.local/share/qutebrowser-profiles"]
```

| 键 | 说明 |
|---|---|
| `data_dir` | 默认实例的数据目录。不存在就跳过 |
| `basedirs` | 放独立实例的目录：每个子目录是一个实例，子目录名就是 profile 名。不存在的会跳过 |

### browser-history

把上面几个浏览器组件合成一个历史来源。没有配置项。

```toml
[components.browser-history]
```

### notify-dbus

通过 D-Bus 发桌面通知，需要有一个通知服务（mako、dunst、swaync……）在运行。

```toml
[components.notify-dbus]
app_name = ""
# bus = "unix:path=/run/user/1000/bus"
expire_ms = -1
timeout_ms = 3000
```

| 键 | 说明 |
|---|---|
| `app_name` | 通知上显示的应用名。stash 的启动器把它设成 `stash` |
| `bus` | 总线地址。不写就用会话总线（`DBUS_SESSION_BUS_ADDRESS`） |
| `expire_ms` | 通知显示多久（毫秒）。`-1` 由通知服务决定，`0` 表示一直显示 |
| `timeout_ms` | 连接总线、等待回复最多多久 |

### exec-tokio

运行外部命令，`context-script` 和 `expand-script` 用它。没有配置项。

```toml
[components.exec-tokio]
```

### storage-sqlite

SQLite 数据库。

```toml
[components.storage-sqlite]
# path = "~/.local/share/stash/stash.db"
read_only = false
busy_timeout_ms = 5000
```

| 键 | 说明 |
|---|---|
| `path` | 数据库文件，写 `:memory:` 是内存数据库。不写就用 `$XDG_DATA_HOME/stash/stash.db`（通常是 `~/.local/share/stash/stash.db`），这个默认值由启动器给出 |
| `read_only` | 只读打开：不创建文件、不执行迁移、拒绝写入 |
| `busy_timeout_ms` | 别的进程占着锁时最多等多久。快捷键记录和 `stash-tui` 可能同时写 |

### cli-clap

命令行界面，负责解析参数并分派给各组件登记的子命令。没有配置项。

```toml
[components.cli-clap]
```

### log-stderr

日志写到 stderr。`stash remind` 的日志在 `journalctl --user -u stash-remind` 里。

```toml
[components.log-stderr]
level = "info"
```

| 键 | 说明 |
|---|---|
| `level` | 低于这个级别的不写：`debug`、`info`、`warn`、`error` |

### clock-system

系统时钟和时区。没有配置项。

```toml
[components.clock-system]
```
