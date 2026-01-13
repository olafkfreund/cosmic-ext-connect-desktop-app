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
  pname = "cosmic-applet-kdeconnect";
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
    cat > $out/lib/systemd/user/kdeconnect-daemon.service << EOF
    [Unit]
    Description=KDE Connect Daemon for COSMIC Desktop
    After=network.target

    [Service]
    Type=simple
    ExecStart=$out/bin/kdeconnect-daemon
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
    ReadWritePaths=%h/.config/kdeconnect %h/.local/share/kdeconnect

    # Network access required for device discovery and communication
    PrivateNetwork=false

    [Install]
    WantedBy=default.target
    EOF

    # Install desktop entry for applet
    mkdir -p $out/share/applications
    cat > $out/share/applications/cosmic-applet-kdeconnect.desktop << EOF
    [Desktop Entry]
    Type=Application
    Name=KDE Connect
    Comment=Integrate your devices with COSMIC Desktop
    Icon=phone-symbolic
    Exec=$out/bin/cosmic-applet-kdeconnect
    Categories=Network;System;
    NoDisplay=true
    X-COSMIC-Applet=true
    EOF
  '';

  # Don't strip binaries in debug mode
  dontStrip = stdenv.isDarwin;

  meta = with lib; {
    description = "KDE Connect applet for COSMIC Desktop - Device synchronization and integration";
    longDescription = ''
      COSMIC KDE Connect provides seamless integration between your Android/iOS
      devices and COSMIC Desktop. Features include:

      - File sharing between devices
      - Clipboard synchronization
      - Notification mirroring
      - Battery status monitoring
      - Media player control (MPRIS)
      - Remote input (planned)
      - SMS messaging (planned)

      This package includes:
      - cosmic-applet-kdeconnect: Panel applet for COSMIC
      - kdeconnect-daemon: Background service for device communication
      - cosmic-kdeconnect: Full application (future)
    '';
    homepage = "https://github.com/olafkfreund/cosmic-applet-kdeconnect";
    changelog = "https://github.com/olafkfreund/cosmic-applet-kdeconnect/releases";
    license = licenses.gpl3Plus;
    maintainers = with maintainers; [ ]; # Add your maintainer info
    mainProgram = "cosmic-applet-kdeconnect";
    platforms = platforms.linux;

    # Requires COSMIC Desktop Environment
    broken = false;
  };
}
