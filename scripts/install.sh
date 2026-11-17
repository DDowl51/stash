#!/usr/bin/env bash
# 把 stash、stash-tui 装到 cargo 的 bin 目录，放好提醒用的 systemd 用户服务（不自动启用）。
# 可以重复运行：升级时再跑一遍就行。
set -euo pipefail
cd "$(dirname "$0")/.."

cargo install --locked --path apps/stash
cargo install --locked --path apps/stash-tui

# cargo install 装到哪：CARGO_INSTALL_ROOT → CARGO_HOME → ~/.cargo
bin="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin"

unit_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
mkdir -p "$unit_dir"
# 单元文件里写的是默认位置；装到别处时换成实际路径
sed "s|%h/.cargo/bin/stash|$bin/stash|" dist/stash-remind.service >"$unit_dir/stash-remind.service"
chmod 644 "$unit_dir/stash-remind.service"
install -m 644 dist/stash-remind.timer "$unit_dir/stash-remind.timer"
if systemctl --user show-environment >/dev/null 2>&1; then
    systemctl --user daemon-reload
fi

# 运行时要用的外部程序。缺了不影响安装，只是对应的功能用不了
missing=()
for cmd in niri vicinae wl-copy wl-paste git xdg-open; do
    command -v "$cmd" >/dev/null 2>&1 || missing+=("$cmd")
done
if ((${#missing[@]})); then
    echo "提醒：找不到 ${missing[*]}。见 docs/installation.md 的「依赖」。" >&2
fi

cat <<MSG

装好了：$bin/stash、$bin/stash-tui

在 niri 里绑快捷键（例子见 dist/niri-binds.kdl），然后：

    stash capture                 # 按快捷键时运行的就是这个
    stash-tui                     # 整理
    stash ls                      # 命令行看

到期和收件箱积压的桌面通知：

    stash remind --dry-run        # 先看看现在会发什么
    systemctl --user enable --now stash-remind.timer

MSG
