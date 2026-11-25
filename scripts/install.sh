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

# The desktop shell. openclicky app looks for it at a fixed path per platform, so
# the installer is what makes "install, then openclicky app" true.
version_bare=""
tag=""

resolve_version() {
    if [ -n "$VERSION" ]; then
        tag="$VERSION"
    else
        tag=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
              | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)
    fi
    [ -n "$tag" ] || return 1
    version_bare="${tag#v}"
}

install_desktop() {
    resolve_version || { say "  could not resolve the release version; skipping the desktop app"; return 0; }
    local base="https://github.com/${REPO}/releases/download/${tag}"

    case "$os" in
        macos)
            # The release carries an arm64 disk image only, so an Intel Mac gets
            # the CLI and an honest note rather than a silent mismatch.
            local name="OpenClicky_${version_bare}_aarch64.dmg" tmpd
            tmpd="$(mktemp -d)"
            if ! curl -fsSL "${base}/${name}" -o "$tmpd/OpenClicky.dmg"; then
                say "  no desktop build for this machine (${name} unavailable); the CLI still works"
                rm -rf "$tmpd"
                return 0
            fi
            if ! hdiutil attach "$tmpd/OpenClicky.dmg" -nobrowse -quiet -mountpoint "$tmpd/mnt"; then
                say "  could not mount the disk image; the CLI still works"
                rm -rf "$tmpd"
                return 0
            fi
            mkdir -p "$HOME/Applications"
            rm -rf "$HOME/Applications/OpenClicky.app"
            cp -R "$tmpd/mnt/OpenClicky.app" "$HOME/Applications/"
            hdiutil detach "$tmpd/mnt" -quiet || true
            rm -rf "$tmpd"
            say "installed the desktop app at ~/Applications/OpenClicky.app"
            ;;
        linux)
            local name="OpenClicky_${version_bare}_amd64.AppImage"
            if curl -fsSL "${base}/${name}" -o "$PREFIX/OpenClicky.AppImage"; then
                chmod +x "$PREFIX/OpenClicky.AppImage"
                ln -sf "$PREFIX/OpenClicky.AppImage" "$PREFIX/openclicky-desktop"
                say "installed the desktop app at $PREFIX/OpenClicky.AppImage"
            else
                say "  could not download the desktop app; the CLI still works"
            fi
            ;;
    esac
}

install_desktop

say ""
say "next:"
say "  openclicky set-key      # paste your provider key"
say "  openclicky doctor       # check permissions"
say "  openclicky app          # start the desktop shell"
