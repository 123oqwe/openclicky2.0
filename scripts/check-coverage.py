#!/usr/bin/env python3
"""Fail when workspace line coverage is below scripts/coverage-floor.

Comparing the number here instead of trusting `--fail-under-lines` is deliberate: that
flag was measured to exit 0 on 70.88% with a floor of 70.89, so it proved nothing. This
script reads the same summary and does the comparison where it can be watched failing.
"""
import argparse
import re
import sys

TOTAL = re.compile(r"^TOTAL\s+.*?([0-9]+\.[0-9]+)%", re.MULTILINE)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("summary", nargs="?", help="file with the summary; stdin if omitted")
    parser.add_argument("--floor", default="scripts/coverage-floor")
    args = parser.parse_args()

    text = open(args.summary, encoding="utf-8").read() if args.summary else sys.stdin.read()
    match = TOTAL.search(text)
    if match is None:
        raise SystemExit("VIOLATION: no TOTAL line in the coverage summary")
    actual = float(match.group(1))
    floor = float(open(args.floor, encoding="utf-8").read().strip())
    print(f"coverage: {actual}% lines, floor {floor}%")
    if actual < floor:
        print(f"VIOLATION: line coverage fell to {actual}%, below the floor of {floor}%")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
