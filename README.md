# stash

随手记下要做的事、闪过的念头、想读的链接，并尽可能多地留下当时的上下文；忙完再整理。主要环境：Arch + niri。

> 开发中。设计见 [docs/specs/2026-11-17-stash-design.md](docs/specs/2026-11-17-stash-design.md)，进度见 [docs/plans/PROGRESS.md](docs/plans/PROGRESS.md)。

- 档位：中等；配套：数据应用、命令行、桌面工具
- kernel 和通用系统服务来自共享仓库 [mk-kit](../mk-kit)（path 依赖，需要和本仓库放在同一个目录下）

```sh
./scripts/check.sh   # 格式、clippy、架构规则、全部测试
lefthook install     # 提交前自动跑上面的检查
```
