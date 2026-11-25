#!/usr/bin/env bash
# Portable sha256. macOS ships `shasum -a 256`, GNU systems ship `sha256sum`,
# and Git for Windows ships the GNU one. Both print "<hash>  <file>", so a
# checksum written by either can be verified by either.
#
# Usage:  source scripts/sha256.sh && sha256 <file>...
sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$@"
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$@"
  else
    echo "no sha256 tool found (tried sha256sum, shasum)" >&2
    return 1
  fi
}
