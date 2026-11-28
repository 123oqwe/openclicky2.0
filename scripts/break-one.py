#!/usr/bin/env python3
"""Replace one literal in one file, and refuse to pretend it worked.

    break-one.py <path> <old> <new>
    break-one.py <path> <old> <new> --append

A break command that quietly matches nothing is worse than no break at all: the check
stays green, the pair is printed as proof, and the guard has been validated by a test
that never ran. So this asserts the text is there, and names the file when it is not.

utf-8 both ways on purpose. A Windows runner defaults to cp1252, and these files contain
comments the runner cannot decode.
"""
import pathlib
import sys

if len(sys.argv) < 4:
    print("usage: break-one.py <path> <old> <new> [--append]", file=sys.stderr)
    sys.exit(2)

path = pathlib.Path(sys.argv[1])
old = sys.argv[2]
new = sys.argv[3]
append = "--append" in sys.argv[4:]

if not path.exists():
    print(f"break-one: {path} does not exist", file=sys.stderr)
    sys.exit(1)

# Bytes, not text. `read_text` normalises CRLF to LF and `write_text` writes LF back,
# so a one-line edit would rewrite every line ending in the file: the diff becomes
# unreadable and the mutation looks bigger than it is. The old and new strings carry no
# line endings of their own, so replacing them as bytes touches nothing else.
# A replacement that needs a newline writes one. Without this, "set -eu\nBASHISM=(a b c)"
# landed in the file as a literal backslash and an n, and dash happened to reject that
# line too, so the break looked correct for the wrong reason.
old = old.replace("\\n", "\n")
new = new.replace("\\n", "\n")

raw = path.read_bytes()
old_b = old.encode("utf-8")
new_b = (old + new).encode("utf-8") if append else new.encode("utf-8")
if old_b not in raw:
    print(f"break-one: {old!r} is not in {path}", file=sys.stderr)
    sys.exit(1)

path.write_bytes(raw.replace(old_b, new_b, 1))
print(f"break-one: broke {path}")
