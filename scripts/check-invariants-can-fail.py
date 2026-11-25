#!/usr/bin/env python3
"""An invariant test that cannot fail is a comment.

Every case below names a test that is supposed to guard an invariant, plus one
edit that violates the invariant while still compiling. The harness applies the
edit, requires the test to go red, restores the file, and requires it to go
green again. A mutation that breaks the build counts as a failure of the harness,
not a pass for the test, so the output is checked for a real assertion failure.

    check-invariants-can-fail.py            run every case
    check-invariants-can-fail.py --list     print the registry
"""
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# (file, old, new, test filter, what the mutation violates)
CASES = [
    (
        "crates/core/src/privacy.rs",
        "        self.opens.fetch_add(1, Ordering::SeqCst);",
        "        // mutated: the gate no longer counts",
        "capture_gate_counts_only_user_actions",
        "INV-D1: capture is only counted when the gate opened",
    ),
    (
        "crates/core/src/usage.rs",
        "        let waived = self.is_waived(input);",
        "        let waived = true;",
        "voice_costs_one_unit",
        "§4.8: a completed ask costs one unit",
    ),
    (
        "crates/core/src/tools/fs.rs",
        "pub fn escapes_workspace(root: &Path, rel: &str) -> bool {",
        "pub fn escapes_workspace(root: &Path, rel: &str) -> bool {\n    return false;",
        "the_policy_and_the_writer_agree_about_what_is_outside",
        "INV-5: a path that leaves the workspace is reported as leaving it",
    ),
]


def run(test_filter):
    return subprocess.run(
        ["cargo", "test", "--quiet", "-p", "openclicky-core", test_filter],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )


def main(argv):
    if "--list" in argv:
        for path, _, _, test, what in CASES:
            print(f"{test:52} {path:32} {what}")
        return 0

    bad = 0
    for path, old, new, test, what in CASES:
        target = ROOT / path
        original = target.read_text(encoding="utf-8")
        original_bytes = target.read_bytes()
        if old not in original:
            print(f"FAIL {test}: the line this mutation edits is gone from {path}")
            bad += 1
            continue

        # The backup is the text, not a file. A temp file kept its descriptor
        # open, and Windows refuses to unlink an open handle (WinError 32) —
        # the same trap the product hit when it deleted the sqlite file.
        try:
            target.write_text(original.replace(old, new, 1), encoding="utf-8")
            mutated = run(test)
            broke_build = "error[" in mutated.stdout + mutated.stderr
            went_red = mutated.returncode != 0 and not broke_build
        finally:
            # Bytes, not text: write_text translates \n to the platform
            # separator, so a restore on Windows would leave CRLF behind in a
            # file that had LF. This also bumps the mtime, which is what makes
            # cargo rebuild instead of reusing the mutated artifact.
            target.write_bytes(original_bytes)

        if not went_red:
            reason = "the mutation did not compile" if broke_build else "the test still passed"
            print(f"FAIL {test}: {reason} — this case cannot fail, so it proves nothing")
            bad += 1
            continue

        if target.read_bytes() != original_bytes:
            print(f"FAIL {test}: the restore did not reproduce the file byte for byte")
            bad += 1
            continue

        restored = run(test)
        if restored.returncode != 0:
            print(f"FAIL {test}: the test does not pass once the tree is restored")
            bad += 1
            continue

        print(f"OK   {test}: red under violation, green when restored  ({what})")

    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
