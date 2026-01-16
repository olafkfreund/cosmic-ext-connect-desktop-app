{ lib
, rustPlatform
, fetchFromGitHub
, pkg-config
, cmake
, openssl
, libxkbcommon
, wayland
, wayland-protocols
, libGL
, libglvnd
, mesa
, pixman
, libinput
, libxcb
, xcbutil
, xcbutilwm
, xcbutilimage
, libdrm
, fontconfig
, freetype
, udev
, dbus
, libpulseaudio
, expat
, glib
, gtk3
, pango
, cairo
, gdk-pixbuf
, atk
, stdenv
}:

rustPlatform.buildRustPackage rec {
  pname = "cosmic-connect";
  version = "0.1.0";

  src = lib.cleanSource ../.;

  cargoLock = {
    lockFile = ../Cargo.lock;
  };

  nativeBuildInputs = [
    pkg-config
    cmake
  ];

  buildInputs = [
    openssl
    libxkbcommon
    wayland
    wayland-protocols
    libGL
    libglvnd
    mesa
    pixman
    libinput
    libxcb
    xcbutil
    xcbutilwm
    xcbutilimage
    libdrm
    fontconfig
    freetype
    udev
    dbus
    libpulseaudio
    expat
    glib
    gtk3
    pango
    cairo
    gdk-pixbuf
    atk
  ];

  # Build all workspace members
  cargoBuildFlags = [
    "--workspace"
    "--bins"
  ];

  # Test all workspace members
  cargoTestFlags = [
    "--workspace"
  ];

  # Set environment variables for build
  LIBCLANG_PATH = "${stdenv.cc.cc.lib}/lib";

  # Ensure proper library paths at runtime
  postInstall = ''
    # Install systemd service
    mkdir -p $out/lib/systemd/user
    cat > $out/lib/systemd/user/cosmic-connect-daemon.service << EOF
    [Unit]
    Description=COSMIC Connect Daemon - Device connectivity service
    After=network.target

    [Service]
    Type=simple
    ExecStart=$out/bin/cosmic-connect-daemon
    Restart=on-failure
    RestartSec=5

    # Security hardening
    NoNewPrivileges=true
    ProtectSystem=strict
    ProtectHome=true
    PrivateTmp=true
    ProtectKernelTunables=true
    ProtectControlGroups=true
    RestrictSUIDSGID=true

    # Allow access to config and data directories
    ReadWritePaths=%h/.config/cosmic/cosmic-connect %h/.local/share/cosmic/cosmic-connect

    # Network access required for device discovery and communication
    PrivateNetwork=false

    [Install]
    WantedBy=default.target
    EOF

    # Install desktop entry for applet
    mkdir -p $out/share/applications
    cat > $out/share/applications/cosmic-applet-connect.desktop << EOF
    [Desktop Entry]
    Type=Application
    Name=COSMIC Connect
    Comment=Device connectivity for COSMIC Desktop
    Icon=phone-symbolic
    Exec=$out/bin/cosmic-applet-connect
    Categories=Network;System;
    NoDisplay=true
    X-COSMIC-Applet=true
    EOF
  '';

  # Don't strip binaries in debug mode
  dontStrip = stdenv.isDarwin;

  meta = with lib; {
    description = "COSMIC Connect - Device connectivity for COSMIC Desktop";
    longDescription = ''
      COSMIC Connect provides seamless integration between your Android devices
      and COSMIC Desktop. Features include:

      - File sharing between devices
      - Clipboard synchronization
      - Notification mirroring
      - Battery status monitoring
      - Media player control (MPRIS)
      - Remote input
      - SMS messaging
      - CConnect protocol (port 1816, side-by-side with KDE Connect)

      This package includes:
      - cosmic-applet-connect: Panel applet for COSMIC
      - cosmic-connect-daemon: Background service (DBus, systemd)
      - cosmic-connect: CLI tool for device management
    '';
    homepage = "https://github.com/olafkfreund/cosmic-connect-desktop-app";
    changelog = "https://github.com/olafkfreund/cosmic-connect-desktop-app/releases";
    license = licenses.gpl3Plus;
    maintainers = with maintainers; [ ]; # Add your maintainer info
    mainProgram = "cosmic-applet-connect";
    platforms = platforms.linux;

    # Requires COSMIC Desktop Environment
    broken = false;
  };
}
