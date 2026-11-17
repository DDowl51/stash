<div align="center">

# stash

**按一个键，把要做的事、闪过的念头、想读的链接记下来，同时留下「当时在干什么」；忙完再整理。**

为 Arch + [niri](https://github.com/YaLTeR/niri) 写的。没有常驻进程，数据都在本机的一个 SQLite 文件里。

[安装](docs/installation.md) · [使用](docs/usage.md) · [配置](docs/configuration.md) · [架构](docs/architecture.md) · [全部文档](docs/README.md)

</div>

---

在终端里写代码时，有人发消息请你帮忙看个 PR。按 `Mod+N`，输入 `帮老王看 PR #t @周五`，回车。输入框消失，焦点回到终端。

晚上打开 `stash-tui`，看到的是：

```
 1 收件箱   2 待办   3 想法   4 稍后读   5 已完成
› 5MSYGS  @2026-11-20  帮老王看 PR  · ghostty · ~/code/stash (main)
  HF74E7  @今天  给房东转账

  帮老王看 PR
  待办 · 未完成 · 记于 11-18 15:42 · 01M7N89YA8YD398XMMCA5MSYGS
  截止  2026-11-20
  来源  ghostty · ~/code/stash (main)
  应用  com.mitchellh.ghostty
  窗口标题  nvim src/flow.rs
  前台命令  nvim src/flow.rs
  目录  /home/me/code/stash
  分支  main
  提交  8097587
  截图  /home/me/.local/share/stash/shots/01M7N89YA8YD398XMMCA5MSYGS.png
 x 完成  b 收件箱  i 想法  r 稍后读  D 截止  e 改  K 上移  J 下移  O 看截图  d 删除
```

（示意：上半部分是列表，下半部分是选中条目的详情，最下面一行是当前页的按键。）

## 特点

- **快**：从按键到输入框出现，只取一次窗口信息、截一张图。回车后最多再等 1 秒采完上下文，然后退出。按 Esc 什么都不留下，截图也会删掉。
- **尽量多留上下文**：来源窗口的应用、标题和工作区；如果是终端，还有前台命令、目录、git 分支和提交；如果是浏览器，还有当时那一页的网址；外加窗口截图，以及你自己写的脚本给出的任何信息。任何一项失败都不影响保存。
- **当场分类，也可以不分**：`#t` 待办、`#i` 想法、`#r` 稍后读，`@明天`、`@周五`、`@11-20` 设截止日期。什么都不写就进收件箱；只贴一个链接时进稍后读。
- **输入展开**：`=3*45` 算出结果，`;会` 这种缩写换成模板，`!gh 123` 这种前缀交给你的脚本改写。
- **整理在终端里做**：`stash-tui` 分五页，单键操作：分流、完成、归档、设截止、排序、打开链接、看截图。
- **到期会提醒**：systemd timer 每小时看一次，收件箱积压或条目到期时发桌面通知，同一件事一天只提醒一次。
- **命令行友好**：在管道里输出 JSONL，退出码遵循 sysexits，id 只写最后几位也行。
- **每样能力都是组件**：截图、浏览器、终端探测都可以单独关掉（`enabled = false`），也可以换成别的实现。

## 快速开始

需要 niri、[vicinae](https://github.com/vicinaehq/vicinae)（弹输入框用）、`wl-clipboard`、stable Rust（edition 2024）。本仓库和共享仓库 [mk-kit](../mk-kit) 要放在同一个目录下。

```bash
scripts/install.sh       # 装 stash、stash-tui 到 ~/.cargo/bin，放好提醒用的 systemd 单元
```

把 [dist/niri-binds.kdl](dist/niri-binds.kdl) 里的快捷键抄进 niri 的配置，然后：

```bash
stash add "试一下 #t @明天"   # 不弹框也能记
stash ls                      # 看
stash-tui                     # 整理
systemctl --user enable --now stash-remind.timer   # 到期提醒
```

详细步骤见 [安装](docs/installation.md)。

## 开发

```sh
scripts/check.sh     # 格式、clippy、架构规则、全部测试（含配置文档检查）
lefthook install     # 提交前自动跑上面的检查
```

改动放在哪、怎么加一个上下文或展开器，见 [架构](docs/architecture.md)。设计和进度见 [docs/specs](docs/specs/2026-11-17-stash-design.md)、[docs/plans/PROGRESS.md](docs/plans/PROGRESS.md)。

## 许可

MIT 或 Apache-2.0，任选其一。
