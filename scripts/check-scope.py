#!/usr/bin/env python3
"""One task, one set of files.

A change that quietly edits files outside its declared scope is how a "small fix"
turns into an unreviewable diff. The scope is declared in `docs/.scope-allow`,
one path prefix per line, next to the task it belongs to.

    check-scope.sh             compare origin/main...HEAD against the allowlist
    check-scope.sh --write <f> record the prefixes of the current diff

No allowlist means no declared task, so nothing to check: the rule stays quiet
rather than failing every branch that predates it.
"""
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
ALLOW = ROOT / "docs" / ".scope-allow"


def git(*args):
    out = subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True)
    return out.stdout if out.returncode == 0 else ""


def changed_files():
    base = git("merge-base", "origin/main", "HEAD").strip() or "HEAD~1"
    return [line for line in git("diff", "--name-only", f"{base}...HEAD").splitlines() if line]


def main(argv):
    files = changed_files()
    if "--write" in argv:
        prefixes = sorted({pathlib.PurePosixPath(f).parts[0] for f in files})
        ALLOW.write_text("\n".join(prefixes) + "\n", encoding="utf-8")
        print(f"scope recorded: {', '.join(prefixes)}")
        return 0

    if not ALLOW.exists():
        print("scope: no task declared, nothing to check")
        return 0

    allowed = [
        line.strip()
        for line in ALLOW.read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.startswith("#")
    ]
    outside = [
        f for f in files if not any(f == a or f.startswith(a.rstrip("/") + "/") for a in allowed)
    ]
    if outside:
        print(f"{len(outside)} file(s) outside the declared scope ({', '.join(allowed)}):")
        for f in outside:
            print("  -", f)
        print("Either narrow the diff or widen docs/.scope-allow with a reason.")
        return 1
    print(f"scope: every changed file is inside {', '.join(allowed)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
