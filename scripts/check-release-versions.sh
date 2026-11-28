#!/usr/bin/env bash
# The version lives in two files that have to move together, and nothing compared them.
# One missed edit publishes OpenClicky_2.0.3.dmg with 2.1.0 inside it, and a version
# number is public the moment it lands.
#
# Its own file so that `prove-can-fail.sh` can break one line and run exactly this.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

crate_version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
bundle_version=$(python3 -c 'import json;print(json.load(open("apps/desktop/tauri.conf.json"))["version"])' 2>/dev/null || true)
if [ "$crate_version" != "$bundle_version" ]; then
    echo "VIOLATION: Cargo.toml says ${crate_version:-?} and tauri.conf.json says ${bundle_version:-?}"
    exit 1
fi
echo "versions agree: ${crate_version:-?}"
