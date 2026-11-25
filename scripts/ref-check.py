#!/usr/bin/env python3
"""Validate document references: section numbers, ADR ids, file paths.

Documents cite each other with `§13`, `SDD §04.11`, `ADR-0007`, and paths like
`docs/06-acceptance.md`. Nothing used to check those, so they rot silently.
This script turns each reference into a lookup and exits non-zero on a miss.
"""
import re, sys, pathlib, collections

# A Windows console defaults to cp1252; printing Chinese would raise before any
# check ran. The scripts are the portable part, so they force their own encoding.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"

# short names used inside prose -> file
ALIASES = {
    "sdd": "02-sdd.md",
    "tds": "04-test-design.md",
    "tech-prd": "01-tech-prd.md",
    "tech prd": "01-tech-prd.md",
    "playbook": "03-agent-playbook.md",
    "baseline": "00-baseline.md",
    "deployment": "05-deployment.md",
    "acceptance": "06-acceptance.md",
    "product-prd": "00-baseline.md",
}
_UNUSED = {
    "SDD": "02-sdd.md",
    "TDS": "04-test-design.md",
    "tech-prd": "01-tech-prd.md",
    "tech prd": "01-tech-prd.md",
    "playbook": "03-agent-playbook.md",
    "baseline": "00-baseline.md",
    "deployment": "05-deployment.md",
    "acceptance": "06-acceptance.md",
    "product-prd": "00-baseline.md",
}

def docs_files():
    return sorted([p for p in DOCS.rglob("*.md") if "reviews/standards" not in str(p)])

def norm(sid):
    """'04.2' and '4.2' are the same section; 'A-04' keeps its letter."""
    if sid.startswith(("A-", "B-", "C-", "D-", "E-", "F-")):
        letter, num = sid.split("-")
        return (letter, int(num))
    parts = sid.split(".")
    return tuple(int(p) for p in parts)

def section_ids(text):
    ids = set()
    for line in text.splitlines():
        m = re.match(r'^#{2,4}\s+(\d{1,2}(?:\.\d+)?|[A-F]-\d{2})\b', line)
        if m:
            ids.add(norm(m.group(1)))
    return ids

def build_index():
    index = {}
    for p in docs_files():
        index[p.name] = section_ids(p.read_text(encoding="utf-8"))
    return index

def main():
    index = build_index()
    problems = []
    for p in docs_files():
        rel = p.relative_to(ROOT)
        text = p.read_text(encoding="utf-8")
        # 1) ADR ids
        for m in re.finditer(r'ADR-(\d{4})', text):
            target = DOCS / "adr" / f"ADR-{m.group(1)}"
            if not list((DOCS / "adr").glob(f"ADR-{m.group(1)}-*.md")):
                problems.append(f"{rel}: ADR-{m.group(1)} does not exist")
        # 2) explicit paths
        for m in re.finditer(r'(?:docs/)([0-9a-z\-]+\.md)', text):
            if not (DOCS / m.group(1)).exists():
                problems.append(f"{rel}: path docs/{m.group(1)} does not exist")
        # 3) "<alias> §N" or "§N.M"
        for m in re.finditer(r'(SDD|TDS|tech-prd|tech prd|playbook|baseline|deployment|acceptance)\s*§\s*(\d{1,2}(?:\.\d+)?|[A-F]-\d{2})', text):
            target = ALIASES[m.group(1).lower()]
            if norm(m.group(2)) not in index.get(target, set()):
                problems.append(f"{rel}: {m.group(1)} §{m.group(2)} not found in {target}")
        # 4) "<file>.md §N"
        for m in re.finditer(r'([0-9]{2}-[a-z0-9\-]+\.md)\s*§\s*(\d{1,2}(?:\.\d+)?|[A-F]-\d{2})', text):
            target = m.group(1)
            if norm(m.group(2)) not in index.get(target, set()):
                problems.append(f"{rel}: {target} §{m.group(2)} not a heading")

    # dedupe while keeping order
    seen = set()
    unique = [x for x in problems if not (x in seen or seen.add(x))]
    if unique:
        print(f"reference check: {len(unique)} problem(s)")
        for u in unique[:80]:
            print("  -", u)
        if len(unique) > 80:
            print(f"  ... {len(unique)-80} more")
        return 1
    print("reference check: OK")
    return 0

if __name__ == "__main__":
    sys.exit(main())
