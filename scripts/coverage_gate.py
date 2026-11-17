"""按包核对行覆盖率。输入是 `cargo llvm-cov --json --summary-only` 的输出。

只看本仓库 features/ 下的包：core 至少 90%，component 至少 80%。
没事不输出；不达标时每个包一行写到 stderr，退出码 1。
"""

import json
import os
import re
import sys

THRESHOLDS = {"core": 90.0, "component": 80.0}
PACKAGE = re.compile(r"(features/[^/]+/(core|component))/")


def totals(report, root):
    """包目录 → [覆盖的行数, 总行数, 门槛]。"""
    out = {}
    for data in report["data"]:
        for f in data["files"]:
            rel = os.path.relpath(f["filename"], root)
            m = PACKAGE.match(rel)
            if not m:
                continue
            lines = f["summary"]["lines"]
            entry = out.setdefault(m.group(1), [0, 0, THRESHOLDS[m.group(2)]])
            entry[0] += lines["covered"]
            entry[1] += lines["count"]
    return out


def failures(report, root):
    bad = []
    for pkg, (covered, count, need) in sorted(totals(report, root).items()):
        percent = 100.0 if count == 0 else covered * 100.0 / count
        if percent < need:
            bad.append(f"{pkg}：{percent:.1f}%，要求 {need:.0f}%")
    return bad


def main():
    with open(sys.argv[1], encoding="utf-8") as f:
        report = json.load(f)
    bad = failures(report, os.getcwd())
    for line in bad:
        print(line, file=sys.stderr)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
