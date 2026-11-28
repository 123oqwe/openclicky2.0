#!/usr/bin/env python3
"""Fail when workspace line coverage is below scripts/coverage-floor.

The summary row is regions, then functions, then lines, then optional branches.
The first percentage is region coverage. Line coverage is the third, which is
what `--fail-under-lines` reads from the JSON export.
"""
import argparse
import re
import sys

TOTAL_LINE = re.compile(r"^TOTAL\b.*$", re.MULTILINE)
PERCENT = re.compile(r"([0-9]+\.[0-9]+)%")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("summary", nargs="?", help="file with the summary; stdin if omitted")
    parser.add_argument("--floor", default="scripts/coverage-floor")
    args = parser.parse_args()

    text = open(args.summary, encoding="utf-8").read() if args.summary else sys.stdin.read()
    row = TOTAL_LINE.search(text)
    if row is None:
        raise SystemExit("VIOLATION: no TOTAL line in the coverage summary")
    percents = PERCENT.findall(row.group(0))
    # Regions, functions, lines. A branch percentage may follow and is not the gate.
    if len(percents) < 3:
        raise SystemExit("VIOLATION: no line-coverage column on the TOTAL line")
    actual = float(percents[2])
    floor = float(open(args.floor, encoding="utf-8").read().strip())
    print(f"coverage: {actual}% lines, floor {floor}%")
    if actual < floor:
        print(f"VIOLATION: line coverage fell to {actual}%, below the floor of {floor}%")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
