#!/usr/bin/env bash
# The README ends its install line in `| sh`, which on Ubuntu and Debian is dash, and
# the script used to stop there on `set -o pipefail` having installed nothing. This runs
# a real install through that pipe against a fake release: a check that stops at
# --help never executes a line of the body and says nothing about whether it installs.
#
# Its own file so that `prove-can-fail.sh` can run exactly this and nothing else.
# This proves the README's line on the oldest supported dash. A CI runner's dash may
# already accept `set -o pipefail`, which is why the mutation that proves this guard
# uses an array literal: no version of dash has ever accepted one. The containers this
# guard really serves are ubuntu:22.04 and debian:12.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
fail=0
bad() { echo "VIOLATION: $*"; fail=1; }
note() { printf '%s\n' "$*"; }

# The README ends that line in `| sh`, which on Ubuntu and Debian is dash, and the
# script used to stop there on `set -o pipefail` having installed nothing. This
# runs a real install through the pipe against a fake release, because the earlier
# version of this check stopped at --help and never executed a line of the body.
if [ "$(uname -s)" != "Linux" ]; then
  note "   skipped: the | sh line is unix only"
elif ! command -v dash >/dev/null 2>&1; then
  note "   skipped: no dash on this machine"
else
  if ! grep -q 'curl -fsSL .*install.sh | sh' README.md; then
    bad "README no longer prints the | sh line this check proves"
  fi
  echo "  diag: dash=$(command -v dash)"
echo "  diag: line27=$(sed -n '27p' scripts/install.sh)"
echo "  diag: bash=$(command -v bash || echo none)"
inst_root="$(mktemp -d)"
  inst_keep="${inst_root}"
  trap 'rm -rf "$inst_keep"' EXIT
  mkdir -p "${inst_root}/rel" "${inst_root}/prefix"
  case "$(uname -s)" in
    Darwin) inst_asset=openclicky-macos ;;
    *) inst_asset=openclicky-linux ;;
  esac
  printf '#!/bin/sh\necho "openclicky 9.9.9 (fake install)"\n' > "${inst_root}/rel/${inst_asset}"
  chmod +x "${inst_root}/rel/${inst_asset}"
  if command -v sha256sum >/dev/null 2>&1; then
    ( cd "${inst_root}/rel" && sha256sum "$inst_asset" > SHA256SUMS )
  else
    ( cd "${inst_root}/rel" && shasum -a 256 "$inst_asset" | awk '{print $1"  '"$inst_asset"'"}' > SHA256SUMS )
  fi
  printf '{"assets":[{"name":"%s"}]}\n' "OpenClicky_9.9.9_amd64.AppImage" > "${inst_root}/rel/release.json"
  inst_out="$(cat scripts/install.sh 2>/dev/null | PREFIX="${inst_root}/prefix" NO_LAUNCH=1 \
      RELEASE_BASE="file://${inst_root}/rel" RELEASES_API="file://${inst_root}/rel/release.json" \
      dash -s 2>&1 || true)"
  if [ ! -f "${inst_root}/prefix/openclicky" ]; then
    bad "the README's | sh line installed nothing under dash: $(printf '%s\n' "$inst_out" | head -1)"
  elif [ ! -x "${inst_root}/prefix/openclicky" ]; then
    bad "the README's | sh line left a binary that is not executable"
  elif [ -z "$("${inst_root}/prefix/openclicky" --version 2>/dev/null || true)" ]; then
    bad "the installed binary does not answer --version"
  else
    note "   ok: dash piped an install that landed an executable binary"
  fi
fi

exit "$fail"
