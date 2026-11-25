#!/usr/bin/env bash
# The one list of Linux packages. ci.yml and release.yml call it, so the three
# build jobs cannot drift: they did, and the release build failed on
# libwayland/libpipewire while CI passed on the same commit.
#
#   linux-deps.sh build   what compiling the workspace needs
#   linux-deps.sh smoke   what running the installed bundle needs
set -euo pipefail

mode="${1:-build}"

sudo apt-get update

case "$mode" in
  build)
    sudo apt-get install -y \
      libasound2-dev libxcb1-dev libxrandr-dev libdbus-1-dev pkg-config \
      libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf \
      libsoup-3.0-dev libssl-dev libxkbcommon-dev libxkbcommon-x11-dev \
      libpipewire-0.3-dev libgbm-dev libdrm-dev libegl1-mesa-dev libwayland-dev
    ;;
  smoke)
    # A virtual display plus a session bus, because a WebKit GTK window exits
    # immediately without one.
    sudo apt-get install -y xvfb dbus-x11 libgl1-mesa-dri
    ;;
  *)
    echo "usage: $0 [build|smoke]" >&2
    exit 2
    ;;
esac
