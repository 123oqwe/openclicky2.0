#!/usr/bin/env python3
"""A new public function must be named in the test matrix.

The matrix in `docs/04-test-design.md` is what tells a reader which case proves
which promise. It went stale on b6dd818: the billing waiver changed and no row
appeared, so every gate stayed green while the matrix described the old code.

Scoped by a ratchet. `docs/.surface-baseline` records the commit the surface was
last reconciled at; only functions added after it are demanded. Without a
baseline the whole tree would be "new", which is true for the initial import and
useless as a rule.

    check-new-tests.py            compare HEAD against the baseline
    check-new-tests.py --rebase   record HEAD as the new baseline
"""
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BASELINE = ROOT / "docs" / ".surface-baseline"
MATRIX = ROOT / "docs" / "04-test-design.md"


def git(*args):
    out = subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True)
    return out.stdout if out.returncode == 0 else ""


def head():
    return git("rev-parse", "HEAD").strip()


def new_public_fns(base):
    diff = git("diff", f"{base}...HEAD", "-U0", "--", "crates", "apps")
    added = {m.group(1) for m in re.finditer(r"^\+\s*pub fn (\w+)", diff, re.M)}
    removed = {m.group(1) for m in re.finditer(r"^-\s*pub fn (\w+)", diff, re.M)}
    return sorted(added - removed)


def main(argv):
    if "--rebase" in argv:
        BASELINE.write_text(head() + "\n", encoding="utf-8")
        print(f"surface baseline recorded at {head()[:7]}")
        return 0

    if not BASELINE.exists():
        BASELINE.write_text(head() + "\n", encoding="utf-8")
        print(f"surface baseline created at {head()[:7]}; nothing to compare yet")
        return 0

    base = BASELINE.read_text(encoding="utf-8").strip()
    if not git("rev-parse", "--verify", "--quiet", base):
        print(f"surface baseline {base[:7]} is not in this clone; run --rebase")
        return 1

    matrix = MATRIX.read_text(encoding="utf-8")
    missing = [name for name in new_public_fns(base) if name not in matrix]
    if missing:
        print(f"{len(missing)} new public function(s) have no line in the test matrix:")
        for name in missing:
            print("  -", name)
        print("Add the case to docs/04-test-design.md, or drop the function.")
        return 1
    print("new public surface is reflected in the test matrix")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
