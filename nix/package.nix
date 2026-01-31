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
  webkitgtk_4_1,
  gobject-introspection,
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
    webkitgtk_4_1
    gobject-introspection
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

    # Security hardening (ProtectHome=read-only allows reading but not writing to unspecified home paths)
    NoNewPrivileges=true
    ProtectSystem=strict
    ProtectHome=read-only
    PrivateTmp=true
    ProtectKernelTunables=true
    ProtectControlGroups=true
    RestrictSUIDSGID=true

    # Allow write access to config and data directories
    ReadWritePaths=%h/.config/cosmic %h/.local/share/cosmic

    # Network access required for device discovery and communication
    PrivateNetwork=false

    [Install]
    WantedBy=default.target
    EOF

    # Install DBus service for activation
    mkdir -p $out/share/dbus-1/services
    cat > $out/share/dbus-1/services/com.system76.CosmicConnect.service << EOF
    [D-BUS Service]
    Name=com.system76.CosmicConnect
    Exec=$out/bin/cosmic-connect-daemon
    SystemdService=cosmic-connect-daemon.service
    EOF

    # Install desktop entries
    mkdir -p $out/share/applications

    # Install desktop entry for cosmic-messages
    cat > $out/share/applications/org.cosmicde.Messages.desktop << EOF
    [Desktop Entry]
    Type=Application
    Name=Cosmic Messages
    Comment=Web-based messaging for COSMIC
    Icon=mail-message-new-symbolic
    Exec=$out/bin/cosmic-messages
    Categories=Network;Chat;
    NoDisplay=false
    EOF

    # Install applet icon
    mkdir -p $out/share/icons/hicolor/scalable/apps
    cat > $out/share/icons/hicolor/scalable/apps/cosmic-applet-connect-symbolic.svg << 'ICON_EOF'
    <?xml version="1.0" encoding="UTF-8"?>
    <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16">
      <style>
        path { fill: currentColor; }
      </style>
      <!-- Phone outline -->
      <path d="M4 1C3.45 1 3 1.45 3 2v12c0 .55.45 1 1 1h6c.55 0 1-.45 1-1V2c0-.55-.45-1-1-1H4zm0 1h6v10H4V2zm3 10.5a.5.5 0 1 1 0 1 .5.5 0 0 1 0-1z"/>
      <!-- Sync arrows -->
      <path d="M13 4l2 2-2 2v-1.5h-1.5v-1H13V4zM13 8l2 2-2 2v-1.5h-1.5v-1H13V8z" opacity="0.7"/>
    </svg>
    ICON_EOF

    # Install desktop entry for applet (COSMIC panel integration)
    cat > $out/share/applications/cosmic-applet-connect.desktop << EOF
    [Desktop Entry]
    Type=Application
    Name=COSMIC Connect
    Comment=Device connectivity for COSMIC Desktop
    Keywords=Cosmic;Iced;applet;connect;phone;device;sync;
    Icon=cosmic-applet-connect-symbolic
    Exec=$out/bin/cosmic-applet-connect
    Categories=Cosmic;Iced;
    Terminal=false
    StartupNotify=true
    NoDisplay=true
    X-CosmicApplet=true
    X-CosmicHoverPopup=Auto
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
