{
  lib,
  rustPlatform,
  fetchFromGitHub,
  pkg-config,
  cmake,
  openssl,
  libxkbcommon,
  wayland,
  wayland-protocols,
  libGL,
  libglvnd,
  mesa,
  pixman,
  libinput,
  libxcb,
  xcbutil,
  xcbutilwm,
  xcbutilimage,
  libdrm,
  fontconfig,
  freetype,
  udev,
  dbus,
  libpulseaudio,
  expat,
  glib,
  gtk3,
  pango,
  cairo,
  gdk-pixbuf,
  atk,
  pipewire,
  stdenv,
}:

rustPlatform.buildRustPackage rec {
  pname = "cosmic-connect";
  version = "0.1.0";

  # Use cleanSourceWith to exclude cosmic-connect-core (git submodule)
  # Cargo will fetch cosmic-connect-core as a git dependency via allowBuiltinFetchGit
  src = lib.cleanSourceWith {
    src = ../.;
    filter =
      path: type:
      let
        baseName = baseNameOf path;
        relativePath = lib.removePrefix (toString ../. + "/") (toString path);
      in
      # Exclude cosmic-connect-core subdirectory (git submodule)
      !lib.hasPrefix "cosmic-connect-core" relativePath
      # Exclude .gitmodules to prevent git submodule conflicts
      && baseName != ".gitmodules"
      # Include everything else
      && (lib.cleanSourceFilter path type);
  };

  cargoLock = {
    lockFile = ../Cargo.lock;
    # Allow Nix to fetch git dependencies automatically without manual hashes
    # This is acceptable for external flakes (not for nixpkgs submission)
    allowBuiltinFetchGit = true;
  };

  nativeBuildInputs = [
    pkg-config
    cmake
    rustPlatform.bindgenHook # Automatically configures bindgen for PipeWire
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
    pipewire # RemoteDesktop plugin dependency
  ];

  # Build all workspace members with RemoteDesktop feature
  # Enable remotedesktop for both daemon and protocol crates
  cargoBuildFlags = [
    "--workspace"
    "--bins"
    "--features"
    "cosmic-connect-daemon/remotedesktop,cosmic-connect-protocol/remotedesktop"
  ];

  # Skip tests for now - test compilation has issues with json! macro imports
  doCheck = false;

  # Test all workspace members (when tests are fixed)
  # cargoTestFlags = [
  #   "--workspace"
  # ];

  # bindgenHook automatically sets LIBCLANG_PATH and BINDGEN_EXTRA_CLANG_ARGS

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
      - Remote desktop (VNC-based screen sharing)
      - SMS messaging
      - CConnect protocol (port 1816, side-by-side with KDE Connect)

      This package includes:
      - cosmic-applet-connect: Panel applet for COSMIC
      - cosmic-connect-daemon: Background service (DBus, systemd)

      Built with RemoteDesktop plugin support (requires PipeWire).
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
