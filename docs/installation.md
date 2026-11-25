# 安装

## 依赖

| 程序 | 用来做什么 | 缺了会怎样 |
|---|---|---|
| Rust stable（edition 2024） | 编译 | 装不上 |
| [niri](https://github.com/YaLTeR/niri) | 知道按键时在哪个窗口、截窗口 | 没有来源窗口、终端、浏览器上下文和截图；`stash add`、`stash-tui` 照常可用 |
| [vicinae](https://github.com/vicinaehq/vicinae) | `stash capture` 弹输入框 | `stash capture` 报「不可用」；可以用 `stash capture --message …` 或 `stash add` |
| `wl-clipboard` | 截图后恢复剪贴板（niri 截图会占用剪贴板） | 截图照常，剪贴板会被换成截图；或者设 `screenshot-niri.restore_clipboard = false` |
| `git` | 终端上下文里的仓库、分支、提交 | 只记目录，不记 git 信息 |
| 通知服务（mako、dunst、swaync……） | `stash remind` 的桌面通知 | `stash remind` 退出码 69，下次再试 |
| `xdg-utils` | `stash-tui` 里打开链接和截图 | 打不开；或者改 `tui-ratatui.opener` |
| Zen / Firefox / Chrome / qutebrowser | 浏览器上下文里的网址 | 只记网页标题 |

Arch 上：

```bash
sudo pacman -S --needed rust niri wl-clipboard git xdg-utils mako
paru -S vicinae          # AUR，或者按 vicinae 的说明安装
```

## 安装

stash 用到的 kernel 和通用系统服务在共享仓库 mk-kit 里，滴答清单的组件在共享仓库 dida 里，三个仓库要放在同一个目录下：

```
~/code/personal/
├── dida/
├── mk-kit/
└── stash/
```

```bash
cd stash
scripts/install.sh
```

脚本会做三件事：

1. 用 `cargo install` 把 `stash`、`stash-tui` 装到 `~/.cargo/bin`（设了 `CARGO_INSTALL_ROOT` 或 `CARGO_HOME` 时装到对应的 `bin` 目录）。
2. 把 `stash-remind.service`、`stash-remind.timer` 放到 `~/.config/systemd/user/`，并执行 `daemon-reload`。不会自动启用。
3. 检查上面那些运行时依赖。缺了的列在 stderr 上，但不会中断安装。

## 绑快捷键

把 [dist/niri-binds.kdl](../dist/niri-binds.kdl) 里需要的几行抄进 `~/.config/niri/config.kdl` 的 `binds { }`：

```kdl
binds {
    Mod+N       { spawn "stash" "capture"; }
    Mod+Alt+T   { spawn "stash" "capture" "--kind" "todo"; }
    Mod+Alt+I   { spawn "stash" "capture" "--kind" "idea"; }
    Mod+Alt+R   { spawn "stash" "capture" "--kind" "read"; }
    Mod+Alt+N   { spawn "ghostty" "-e" "stash-tui"; }
}
```

niri 会自动重新加载配置。按 `Mod+N` 没反应时，先在终端里跑 `stash capture` 看报什么错；如果终端里能用、快捷键不能用，多半是 niri 的 `PATH` 里没有 `~/.cargo/bin`，这时 `spawn` 里要写完整路径。

## 到期提醒

```bash
stash remind --dry-run                              # 看看现在会发什么（不发、也不记）
systemctl --user enable --now stash-remind.timer    # 登录 5 分钟后一次，之后每小时一次
systemctl --user list-timers stash-remind.timer     # 下次什么时候跑
journalctl --user -u stash-remind                   # 出错时看这里
```

## 配置

不写配置也能用。要改的话，在 `~/.config/stash/config.toml` 里只写想改的键，全部配置项见 [配置](configuration.md)。常改的几项：

```toml
# 例子
[components.capture.tags]          # 换成你习惯的 # 标记
todo = ["t", "todo", "做"]
idea = ["i", "idea", "想"]
read_later = ["r", "read", "读"]
inbox = ["in"]

[components.context-screenshot]
skip_apps = ["org.keepassxc.KeePassXC"]   # 这些窗口不截图

[components.remind]
inbox_threshold = 10
due_days_ahead = 1                 # 提前一天提醒
```

## 数据在哪

| 内容 | 位置 |
|---|---|
| 条目 | `~/.local/share/stash/stash.db`（SQLite，可以直接用 `sqlite3` 查） |
| 截图 | `~/.local/share/stash/shots/<条目 id>.png` |
| 配置 | `~/.config/stash/config.toml` |

`$XDG_DATA_HOME`、`$XDG_CONFIG_HOME` 设了的话，以它们为准。备份时把 `~/.local/share/stash` 整个拷走就行。

## 升级

```bash
git -C ../mk-kit pull && git -C ../dida pull && git pull
scripts/install.sh
```

数据库的表结构变化由程序在启动时自动迁移。

## 卸载

```bash
systemctl --user disable --now stash-remind.timer
rm ~/.config/systemd/user/stash-remind.{service,timer}
systemctl --user daemon-reload
cargo uninstall stash stash-tui
rm -r ~/.local/share/stash ~/.config/stash     # 数据和配置，确定不要了再删
```

最后把 niri 配置里的快捷键删掉。
