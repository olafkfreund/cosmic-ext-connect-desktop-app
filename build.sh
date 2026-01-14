#!/usr/bin/env bash
# Build script for cosmic-applet-kdeconnect
# Sets up PKG_CONFIG_PATH to find system libraries

set -e

# Find xkbcommon.pc on the system
XKBCOMMON_PATH=$(find /nix/store -name "xkbcommon.pc" 2>/dev/null | grep -E "libxkbcommon-[0-9]+\.[0-9]+\.[0-9]+-dev" | head -1 | xargs dirname)

if [ -z "$XKBCOMMON_PATH" ]; then
    echo "Error: Could not find xkbcommon.pc in /nix/store"
    echo "Please ensure libxkbcommon-dev is installed"
    exit 1
fi

echo "Using xkbcommon from: $XKBCOMMON_PATH"

# Set PKG_CONFIG_PATH and build
export PKG_CONFIG_PATH="$XKBCOMMON_PATH:$PKG_CONFIG_PATH"

# Build requested packages or all by default
if [ $# -eq 0 ]; then
    echo "Building all packages..."
    cargo build
else
    echo "Building: $@"
    cargo build "$@"
fi
