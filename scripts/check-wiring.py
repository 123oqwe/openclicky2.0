#!/usr/bin/env python3
"""Every core module must be reachable from production code, or say out loud that it is not.

A module with unit tests and zero callers passes the test suite and the doc
matrix while doing nothing for the user. This check makes that state explicit:
a module either has a caller outside its own file, or it carries a
`UNWIRED: <reason>` marker naming the issue that tracks the gap.

Exit 1 when a module is unwired and silent. Exit 0 otherwise.

Usage: check-wiring.py [--quiet]
"""
import glob
import os
import re
import sys

CORE = "crates/core/src"
SKIP = {"lib", "main"}


def rust_sources():
    return glob.glob("crates/*/src/**/*.rs", recursive=True) + glob.glob(
        "apps/*/src/**/*.rs", recursive=True
    )


def reference_counts(sources):
    texts = {f: open(f, encoding="utf-8", errors="replace").read() for f in sources}
    rows = []
    for path in sorted(glob.glob(os.path.join(CORE, "*.rs"))):
        name = os.path.basename(path)[:-3]
        if name in SKIP:
            continue
        hits = 0
        for f, text in texts.items():
            if os.path.basename(f) == name + ".rs":
                continue
            hits += len(re.findall(r"\b" + re.escape(name) + r"::", text))
        rows.append((name, path, hits))
    return rows


def declared_unwired(path):
    text = open(path, encoding="utf-8", errors="replace").read()
    return re.search(r"UNWIRED:", text) is not None


def main(argv):
    quiet = "--quiet" in argv
    sources = rust_sources()
    if not sources:
        print("no rust sources found, run from the repository root", file=sys.stderr)
        return 2

    rows = reference_counts(sources)
    silent = [(n, p) for n, p, hits in rows if hits == 0 and not declared_unwired(p)]

    if not quiet:
        by_count = sorted(rows, key=lambda r: (r[2], r[0]))
        for name, path, hits in by_count:
            if hits == 0:
                state = "unwired, declared" if declared_unwired(path) else "unwired, SILENT"
            else:
                state = f"{hits} refs"
            print(f"{name:18s} {state}")

    for name, path in silent:
        print(
            f"VIOLATION: {path} is not referenced by any production code "
            f"and has no UNWIRED marker",
            file=sys.stderr,
        )
    return 1 if silent else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
