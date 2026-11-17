#!/usr/bin/env bash
# 行覆盖率门槛：features/*/core 90%，features/*/component 80%（设计 §8）。
# 需要 cargo-llvm-cov（`pacman -S cargo-llvm-cov`）和 llvm-tools（`rustup component add llvm-tools-preview`）。
# 没装时报错退出；不在 check.sh 里，按需单独跑。
set -euo pipefail
cd "$(dirname "$0")/.."

if ! cargo llvm-cov --version >/dev/null 2>&1; then
    echo "coverage.sh：没有 cargo-llvm-cov，见脚本开头的说明" >&2
    exit 69
fi

out=target/coverage.json
cargo llvm-cov --workspace --all-features --json --summary-only --output-path "$out"
python3 scripts/coverage_gate.py "$out"
