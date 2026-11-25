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
    """stdout, or None when the command failed.

    Returning "" made a failed command look like an empty diff, and an empty diff
    is inside every scope. That is the same false pass this rule exists to stop.
    """
    out = subprocess.run(
        ["git", *args], cwd=ROOT, capture_output=True, text=True, encoding="utf-8"
    )
    return out.stdout if out.returncode == 0 else None


def changed_files():
    base = (git("merge-base", "origin/main", "HEAD") or "").strip()
    if not base:
        raise SystemExit(
            "cannot find the merge base with origin/main, so the diff is unknown. "
            "Fetch origin/main, then re-run. An unknown diff is not an empty diff."
        )
    diff = git("diff", "--name-only", f"{base}...HEAD")
    if diff is None:
        raise SystemExit(f"git diff against {base[:7]} failed; the diff is unknown")
    return [line for line in diff.splitlines() if line]


def main(argv):
    if "--write" in argv:
        files = changed_files()
        prefixes = sorted({pathlib.PurePosixPath(f).parts[0] for f in files})
        ALLOW.write_text("\n".join(prefixes) + "\n", encoding="utf-8")
        print(f"scope recorded: {', '.join(prefixes)}")
        return 0

    if not ALLOW.exists():
        # Nothing is declared, so there is nothing to compare against. Reading
        # the diff here would fail on a shallow clone for no reason.
        print("scope: no task declared, nothing to check")
        return 0

    files = changed_files()
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
