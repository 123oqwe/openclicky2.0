#!/usr/bin/env python3
"""A ratchet on documentation debt.

The rules that judge docs are correct and the current docs do not meet them: 37
places are thin or partial today. A gate that is red on every commit gets deleted
within a week, so the debt is recorded instead and the record may only shrink.

These are red:
  a violation that is not in the record        (new debt)
  a record entry that no longer violates       (stale debt, delete the line)
  a record longer than the one on origin/main  (the record grew)
  a generated record that does not match       (stale coverage or audit output)
"""
import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEBT = ROOT / "docs/.doc-debt"


def violations():
    """Debt rows, and violation lines that are not rows.

    A freshness mismatch is `VIOLATION: path does not match...` with no middle
    dots. Dropping every line with fewer than three parts made a stale
    coverage.md or audit-output.md invisible, and `git diff` cannot see it
    either: --check renders in memory and does not write the file.
    """
    out = []
    other = []
    for script in ("criteria-coverage.py", "doc-audit.py"):
        # The auditors print Chinese headings as UTF-8 (they reconfigure
        # stdout). text=True without an encoding decodes with the locale, which
        # is cp1252 on the Windows rust job, so every key mojibakes.
        run = subprocess.run(
            [sys.executable, str(ROOT / "scripts" / script), "--check"],
            capture_output=True, text=True, encoding="utf-8", cwd=ROOT,
        )
        for line in ((run.stdout or "") + (run.stderr or "")).splitlines():
            if not line.startswith("VIOLATION:"):
                continue
            body = line[len("VIOLATION:"):].strip()
            parts = [p.strip() for p in body.split("·")]
            if len(parts) < 3:
                other.append(line)
                continue
            out.append(" :: ".join((parts[0], parts[1], parts[2])))
    return out, other


def recorded():
    if not DEBT.exists():
        return []
    return [line.rstrip("\n") for line in DEBT.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.startswith("#")]


# Same heading rule as doc-audit.py and criteria-coverage.py. `##` and `###`
# open a numbered section; `01.1`, `4.1` and `22.1` stay inside that number.
# A deeper heading such as `#### 4.1` is body, not a new section.
SECTION = re.compile(r"^#{2,3}\s+(?:(\d{1,2})(?:\.\d+)?)(?![0-9])")


def section_span(text, number):
    """1-based inclusive line range of the section numbered `number`."""
    target = int(number)
    lines = text.splitlines()
    start = None
    for index, line in enumerate(lines):
        match = SECTION.match(line)
        if not match or not match.group(1):
            continue
        found = int(match.group(1))
        if start is None:
            if found == target:
                start = index
            continue
        if found != target:
            return start + 1, index
    if start is None:
        return None
    return start + 1, len(lines)


def touched_entries(known):
    """Entries whose own section this branch edited and did not delete."""
    red = []
    for key in known:
        path, section, _rule = [part.strip() for part in key.split("::")]
        number = section.split(" ")[0]
        target = ROOT / path
        if not target.exists():
            continue
        span = section_span(target.read_text(encoding="utf-8"), number)
        if span is None:
            continue
        diff = subprocess.run(
            ["git", "diff", "-U0", "origin/main...HEAD", "--", path],
            capture_output=True, text=True, encoding="utf-8", cwd=ROOT,
        ).stdout
        for line in diff.splitlines():
            if not line.startswith("@@"):
                continue
            for side in line.split("+")[1:]:
                start = int(side.split(",")[0].split(" ")[0] or 0)
                if span[0] <= start <= span[1]:
                    red.append(key)
                    break
    return red


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--write", action="store_true", help="record today's debt")
    args = parser.parse_args()

    found, other = violations()
    if args.write:
        DEBT.write_text("\n".join(sorted(set(found))) + "\n", encoding="utf-8")
        print(f"recorded {len(set(found))} entries in {DEBT.relative_to(ROOT)}")
        return 0

    known = recorded()
    failed = False
    for line in other:
        print(line)
        failed = True
    for key in sorted(set(found) - set(known)):
        print(f"VIOLATION: new debt, not in {DEBT.name}: {key}")
        failed = True
    for key in sorted(set(known) - set(found)):
        print(f"VIOLATION: stale entry, this no longer violates: {key}")
        failed = True

    base = subprocess.run(["git", "show", "origin/main:docs/.doc-debt"],
                          capture_output=True, text=True, encoding="utf-8", cwd=ROOT)
    if base.returncode == 0:
        was = len([l for l in base.stdout.splitlines() if l.strip() and not l.startswith("#")])
        if len(known) > was:
            print(f"VIOLATION: the debt record grew from {was} to {len(known)} entries")
            failed = True

    # Editing a recorded section without deleting its line turns the record into a
    # lie that only gets found later, by someone who assumed the list was current.
    for key in touched_entries(known):
        print(f"VIOLATION: this branch edited a recorded section without clearing its debt: {key}")
        failed = True

    print(f"doc debt: {len(known)} recorded, {len(set(found))} found")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
