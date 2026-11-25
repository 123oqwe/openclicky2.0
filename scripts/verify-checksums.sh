#!/usr/bin/env bash
# Verify the release downloads against SHA256SUMS, on any of the three platforms.
#
# macOS ships `shasum` (Perl); Linux and Git for Windows ship `sha256sum`. Both
# print and accept "<hash>  <file>", so either can check the other's output.
#
# Usage, in the directory where you downloaded the files:
#   scripts/verify-checksums.sh [SHA256SUMS]
set -euo pipefail

sums="${1:-SHA256SUMS}"

if [ ! -f "$sums" ]; then
    echo "no $sums here. Run this in the directory where you downloaded the release files." >&2
    exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$sums"
elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "$sums"
else
    echo "neither sha256sum nor shasum is installed; cannot verify." >&2
    exit 1
fi
