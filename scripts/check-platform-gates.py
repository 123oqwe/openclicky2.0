#!/usr/bin/env python3
"""A path that only exists on one platform has to sit behind a cfg gate.

Twice now a change compiled on macOS and Linux and failed on Windows: once a set of
types in a struct literal, once `std::os::unix` in a test body. `cargo clippy
--all-targets` on macOS cannot see the second, because the code never compiles there.

A line-based grep cannot answer this: the attribute that decides is on an earlier line.
So this tracks the most recent `#[cfg(...)]` or `#[cfg_attr(...)]` above each use and
accepts the line only if that attribute names the platform the path belongs to.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
# The platform each std::os::* path belongs to, and the cfg that must guard it.
PLATFORMS = {"unix": "unix", "windows": "windows"}

USE = re.compile(r"std::os::(unix|windows)\b")
CFG = re.compile(r"^\s*#\[cfg(_attr)?\(([^)]*)")
OPEN_BLOCK = re.compile(r"^\s*(pub\s+)?(fn|mod|impl|struct|enum|trait|const|static)\b")


def cfg_gates(expr: str, platform: str) -> bool:
    """True when `expr` gates on `platform`.

    The capture stops at the first `)`, so `#[cfg(not(unix))]` arrives as
    `not(unix`. That text contains `unix` and compiles only where the path does
    not exist. A negated predicate is not a gate.
    """
    if re.search(rf"\bnot\s*\(\s*{re.escape(platform)}\b", expr):
        return False
    return re.search(rf"\b{re.escape(platform)}\b", expr) is not None


def guarded(source: str, offset: int, platform: str) -> bool:
    """True when an attribute above `offset` gates on `platform`.

    Walks up past the item's own signature, its other attributes and its comments,
    and stops at the end of the previous item. Both shapes in this tree are covered:
    `#[cfg(unix)] fn test()` and `#[cfg(unix)] { ... }` inside a test body.
    """
    head = source[:offset]
    for line in reversed(head.splitlines()):
        m = CFG.match(line)
        if m:
            return cfg_gates(m.group(2), platform)
        stripped = line.strip()
        if stripped == "}":
            return False
        if stripped == "" or stripped.startswith("//") or stripped.startswith("#["):
            continue
        if OPEN_BLOCK.match(line) or stripped in ("{",) or stripped.endswith("{"):
            # The signature of the item we are inside, or a block opening: keep going
            # up, the attribute lives above it.
            continue
        if stripped in ("};", ");", ")", "});"):
            return False
        continue
    return False


def main() -> int:
    bad = []
    for path in sorted(list(ROOT.glob("crates/**/*.rs")) + list(ROOT.glob("apps/**/*.rs"))):
        if "target" in path.parts:
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for m in USE.finditer(text):
            line_no = text.count("\n", 0, m.start()) + 1
            line = text.splitlines()[line_no - 1]
            if line.lstrip().startswith("//"):
                continue
            platform = PLATFORMS[m.group(1)]
            if not guarded(text, m.start(), platform):
                bad.append(f"{path.relative_to(ROOT)}:{line_no}: std::os::{platform} without #[cfg({platform})]")
    if bad:
        print(f"platform gates: {len(bad)} problem(s)")
        for b in bad[:20]:
            print("  -", b)
        return 1
    print("platform gates: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
