#!/usr/bin/env bash
# Install the OpenClicky CLI from the latest release.
#
#   curl -fsSL https://raw.githubusercontent.com/123oqwe/openclicky2.0/main/scripts/install.sh | sh
#
# The release assets carry a platform suffix (openclicky-macos, openclicky-linux,
# openclicky-windows.exe) because three files called "openclicky" would collide
# in a flat download. This installs the right one under the plain name
# "openclicky", which is what every example in the README and CHANGELOG assumes.
#
# Override where it lands with PREFIX (default ~/.local/bin) and pin a version
# with VERSION (default: the latest release).
set -euo pipefail

REPO="123oqwe/openclicky2.0"
PREFIX="${PREFIX:-$HOME/.local/bin}"
VERSION="${VERSION:-}"

say() { printf '%s\n' "$*"; }
die() { printf 'install: %s\n' "$*" >&2; exit 1; }

case "$(uname -s)" in
    Darwin) os=macos ;;
    Linux) os=linux ;;
    *) die "unsupported system $(uname -s). Windows: use install.ps1." ;;
esac

case "$(uname -m)" in
    x86_64 | amd64) : ;;   # the released CLI is x86_64 on both
    arm64 | aarch64)
        # The macOS desktop app ships aarch64; the CLI binary is universal in the
        # same archive, so the macos asset is the right one on either arch.
        ;;
    *) die "unsupported architecture $(uname -m)" ;;
esac

asset="openclicky-${os}"

if [ -n "$VERSION" ]; then
    url="https://github.com/${REPO}/releases/download/${VERSION}/${asset}"
else
    url="https://github.com/${REPO}/releases/latest/download/${asset}"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

say "downloading ${asset}"
command -v curl >/dev/null 2>&1 || die "curl is required"
curl -fsSL "$url" -o "$tmp/openclicky" || die "download failed: $url"

# Artifact uploads do not keep the executable bit, so the download is 644 and
# has to be made runnable before anything else.
chmod +x "$tmp/openclicky"

mkdir -p "$PREFIX"
mv "$tmp/openclicky" "$PREFIX/openclicky"
chmod +x "$PREFIX/openclicky"

say "installed $("$PREFIX/openclicky" --version 2>/dev/null || echo "openclicky")"
say "  at ${PREFIX}/openclicky"

case ":$PATH:" in
    *":$PREFIX:"*) ;;
    *)
        say ""
        say "  ${PREFIX} is not on PATH. Add it:"
        say "    echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.zshrc"
        ;;
esac

say ""
say "next:"
say "  openclicky set-key      # paste your provider key"
say "  openclicky doctor       # check permissions"
say "  openclicky app          # start the desktop shell (once it exists)"
