#!/usr/bin/env bash
# Install or remove OpenClicky: the CLI and, when the release has one for this
# machine, the desktop shell.
#
#   curl -fsSL https://raw.githubusercontent.com/123oqwe/openclicky2.0/main/scripts/install.sh | sh
#   scripts/install.sh --uninstall
#
# The release assets carry a platform suffix (openclicky-macos, openclicky-linux,
# openclicky-windows.exe) because three files called "openclicky" would collide
# in a flat download. This installs the right one under the plain name, which is
# what every example in the README and CHANGELOG assumes.
#
# Override the destination with PREFIX (default ~/.local/bin), pin a version with
# VERSION (default: the latest release), and skip the launch at the end with
# NO_LAUNCH=1, which is what CI and a headless machine need.
set -euo pipefail

REPO="123oqwe/openclicky2.0"
PREFIX="${PREFIX:-$HOME/.local/bin}"
VERSION="${VERSION:-}"
RELEASES_API="${RELEASES_API:-https://api.github.com/repos/${REPO}/releases/latest}"

say() { printf '%s\n' "$*"; }
die() { printf 'install: %s\n' "$*" >&2; exit 1; }

action=install
for arg in "$@"; do
    case "$arg" in
        --uninstall) action=uninstall ;;
        -h | --help)
            say "usage: install.sh [--uninstall]"
            say "  PREFIX        where the CLI goes (default ~/.local/bin)"
            say "  VERSION       pin a release tag (default: latest)"
            say "  NO_LAUNCH=1   install without starting OpenClicky"
            say "  RELEASE_BASE  download from somewhere else (a mirror, or file://)"
            exit 0
            ;;
        *) die "unknown option ${arg}" ;;
    esac
done

case "$(uname -s)" in
    Darwin) os=macos ;;
    Linux) os=linux ;;
    *) die "unsupported system $(uname -s). Windows: use install.ps1." ;;
esac

apps_dir="${APPS_DIR:-$HOME/Applications}"
mac_app="${apps_dir}/OpenClicky.app"
desktop_entry="${XDG_DATA_HOME:-$HOME/.local/share}/applications/openclicky.desktop"

release_base() {
    if [ -n "${RELEASE_BASE:-}" ]; then
        printf '%s' "$RELEASE_BASE"
    elif [ -n "$VERSION" ]; then
        printf 'https://github.com/%s/releases/download/%s' "$REPO" "$VERSION"
    else
        printf 'https://github.com/%s/releases/latest/download' "$REPO"
    fi
}

# Print the name of the single asset matching a pattern, or nothing. The name
# comes from the release rather than the tag because the disk image and the zip
# are named after the crate version: a tag v2.0.1 with Cargo at 2.0.0 would make
# a constructed name 404 while the real asset sits right there.
asset_name() {
    local pattern="$1"
    # `|| true`: no match is a normal answer here, and under `set -e` an empty
    # grep would kill the installer silently instead of saying the release has no
    # desktop build.
    curl -fsSL "$RELEASES_API" 2>/dev/null \
        | sed -n 's/.*"name": *"\([^"]*\)".*/\1/p' \
        | grep -E "$pattern" | head -1 || true
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        die "no sha256 tool: install coreutils, or use a system with shasum"
    fi
}

# Integrity, not authenticity: SHA256SUMS is unsigned and comes from the host that
# served the asset, so it catches a truncated or swapped download and nothing more.
verify_asset() {
    local file="$1" name="$2" base="$3" expected actual
    local sums
    if ! sums="$(curl -fsSL "${base}/SHA256SUMS")"; then
        die "could not fetch ${base}/SHA256SUMS, so nothing can be verified. Nothing was installed."
    fi
    expected="$(printf '%s\n' "$sums" | awk -v n="$name" '$2 == n { print $1 }' | head -1)"
    if [ -z "$expected" ]; then
        die "${name} is not listed in SHA256SUMS, so it cannot be verified. Nothing was installed."
    fi
    actual="$(sha256_of "$file")"
    if [ "$actual" != "$expected" ]; then
        die "${name} does not match SHA256SUMS. Nothing was installed.
  expected ${expected}
  got      ${actual}"
    fi
    say "verified ${name}"
}

uninstall() {
    rm -f "$PREFIX/openclicky"
    say "removed ${PREFIX}/openclicky"
    case "$os" in
        macos)
            rm -rf "$mac_app"
            say "removed ${mac_app}"
            ;;
        linux)
            rm -f "$PREFIX/OpenClicky.AppImage" "$PREFIX/openclicky-desktop"
            rm -f "$desktop_entry"
            say "removed the AppImage, its symlink and the desktop entry"
            ;;
    esac
    say ""
    say "your data is still in place. To remove it as well: openclicky delete --yes"
}

install_cli() {
    local asset="openclicky-${os}"
    local base url
    base="$(release_base)"
    url="${base}/${asset}"

    # global on purpose: the EXIT trap outlives this function, and a local would
    # be gone by the time it fires.
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT

    say "downloading ${asset}"
    command -v curl >/dev/null 2>&1 || die "curl is required"
    curl -fsSL "$url" -o "$tmp/openclicky" || die "download failed: $url"
    verify_asset "$tmp/openclicky" "$asset" "$base"

    # Artifact uploads do not keep the executable bit, so the download is 644 and
    # has to be made runnable before anything else.
    chmod +x "$tmp/openclicky"
    mkdir -p "$PREFIX"
    mv "$tmp/openclicky" "$PREFIX/openclicky"
    chmod +x "$PREFIX/openclicky"
    say "installed $("$PREFIX/openclicky" --version 2>/dev/null || echo openclicky)"
    say "  at ${PREFIX}/openclicky"
}

install_desktop() {
    local base pattern name
    base="$(release_base)"

    case "$os" in
        macos) pattern='^OpenClicky\.app\.zip$' ;;
        linux) pattern='^OpenClicky_.*_amd64\.AppImage$' ;;
        *) return 0 ;;
    esac

    name="$(asset_name "$pattern")"
    if [ -z "$name" ]; then
        say "  this release has no desktop build for ${os}; the CLI still works"
        return 0
    fi

    local tmp
    tmp="$(mktemp -d)"
    say "downloading ${name}"
    if ! curl -fsSL "${base}/${name}" -o "$tmp/${name}"; then
        say "  could not download ${name}; the CLI still works"
        rm -rf "$tmp"
        return 0
    fi
    verify_asset "$tmp/${name}" "$name" "$base"

    if [ "$os" = "macos" ]; then
        mkdir -p "$apps_dir"
        rm -rf "$mac_app"
        # ditto, not unzip: unzip can drop resource forks and symlink layout, and
        # a partially restored bundle is worse than no bundle.
        if ! ditto -x -k "$tmp/${name}" "$apps_dir"; then
            say "  could not expand ${name}; the CLI still works"
            rm -rf "$tmp"
            return 0
        fi
        if [ ! -d "$mac_app" ]; then
            say "  ${name} did not contain OpenClicky.app; the CLI still works"
            rm -rf "$tmp"
            return 0
        fi
        say "installed the desktop app at ${mac_app}"
    else
        cp "$tmp/${name}" "$PREFIX/OpenClicky.AppImage"
        chmod +x "$PREFIX/OpenClicky.AppImage"
        ln -sf "$PREFIX/OpenClicky.AppImage" "$PREFIX/openclicky-desktop"
        mkdir -p "$(dirname "$desktop_entry")"
        cat >"$desktop_entry" <<DESKTOP
[Desktop Entry]
Type=Application
Name=OpenClicky
Comment=An AI buddy that lives on your computer
Exec=${PREFIX}/OpenClicky.AppImage
Terminal=false
Categories=Utility;
DESKTOP
        say "installed the desktop app at ${PREFIX}/OpenClicky.AppImage"
    fi
    rm -rf "$tmp"
}

if [ "$action" = uninstall ]; then
    uninstall
    exit 0
fi

install_cli
install_desktop

case ":$PATH:" in
    *":$PREFIX:"*) ;;
    *)
        say ""
        say "  ${PREFIX} is not on PATH. Add it:"
        say "    echo 'export PATH=\"${PREFIX}:\$PATH\"' >> ~/.zshrc"
        ;;
esac

say ""
say "next:"
say "  openclicky set-key      # paste your provider key"
say "  openclicky doctor       # check permissions"
say "  openclicky app          # start the desktop shell"

launch() {
    [ "${NO_LAUNCH:-0}" = "1" ] && return 0
    case "$os" in
        macos)
            [ -d "$mac_app" ] || return 0
            say "starting OpenClicky"
            open "$mac_app" || say "  could not start it; run: open \"$mac_app\""
            ;;
        linux)
            [ -x "$PREFIX/OpenClicky.AppImage" ] || return 0
            say "starting OpenClicky"
            exec "$PREFIX/OpenClicky.AppImage"
            ;;
    esac
}

launch
