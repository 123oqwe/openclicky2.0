#!/usr/bin/env bash
# The one list of Linux build dependencies. ci.yml and release.yml both call this
# so the three jobs cannot drift: they did, and the release build failed on
# libwayland/libpipewire while CI passed on the same commit.
set -euo pipefail

sudo apt-get update
sudo apt-get install -y \
  libasound2-dev libxcb1-dev libxrandr-dev libdbus-1-dev pkg-config \
  libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf \
  libsoup-3.0-dev libssl-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libpipewire-0.3-dev libgbm-dev libdrm-dev libegl1-mesa-dev libwayland-dev
