#!/usr/bin/env python3
"""One task, one set of files.

A change that quietly edits files outside its declared scope is how a "small fix"
turns into an unreviewable diff. The scope is declared in `docs/.scope-allow`,
one path per line, rewritten by the task that owns it.

    check-scope.py             compare origin/main...HEAD against the allowlist
    check-scope.py --write     record the full paths of the current diff

Full paths, not top-level directories: `scripts` allowed every script in the
repository to change under a rule that claimed to be about one task's files.
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


def allowlist():
    entries = []
    for line in ALLOW.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            entries.append(line)
    return entries


def main(argv):
    if "--write" in argv:
        files = changed_files()
        ALLOW.write_text("\n".join(files) + "\n", encoding="utf-8")
        print(f"scope recorded: {len(files)} file(s)")
        return 0

    if not ALLOW.exists():
        # A missing allowlist is a deleted one. Passing here is how a task that
        # deleted the file it was judged against skipped the judgement.
        print(f"VIOLATION: {ALLOW.relative_to(ROOT)} is missing, so the declared scope is unknown")
        print("Restore it, or record the scope with scripts/check-scope.py --write.")
        return 1

    files = changed_files()
    allowed = allowlist()
    if not allowed:
        print(f"VIOLATION: {ALLOW.relative_to(ROOT)} lists no path, so nothing is allowed")
        return 1

    bare = [a for a in allowed if "/" not in a.rstrip("/") and (ROOT / a.rstrip("/")).is_dir()]
    outside = [
        f for f in files if not any(f == a or f.startswith(a.rstrip("/") + "/") for a in allowed)
    ]
    if bare or outside:
        for entry in bare:
            print(f"VIOLATION: {entry!r} is a bare top-level directory; list the files it stands for")
        if outside:
            print(f"VIOLATION: {len(outside)} file(s) outside the declared scope:")
            for f in outside:
                print("  -", f)
        print("Either narrow the diff or widen docs/.scope-allow with a reason.")
        return 1
    print(f"scope: all {len(files)} changed file(s) are inside the declared scope")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
