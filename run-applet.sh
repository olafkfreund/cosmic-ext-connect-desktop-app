#!/bin/bash
# Wrapper script to run the Cosmic Connect Applet with correct environment variables

# Add Nix store paths for Wayland and xkbcommon if not already present
export LD_LIBRARY_PATH=$LD_LIBRARY_PATH:$(pkg-config --libs-only-L wayland-client | sed 's/-L//'):$(pkg-config --libs-only-L xkbcommon | sed 's/-L//')

echo "Starting Cosmic Connect Applet..."
cargo run --bin cosmic-applet-connect
