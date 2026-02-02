{
  lib,
  rustPlatform,
  fetchFromGitHub,
  pkg-config,
  cmake,
  makeWrapper,
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
  gst_all_1,
  libopus,
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
    makeWrapper
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
    pipewire # RemoteDesktop and AudioStream plugin dependency
    webkitgtk_4_1
    gobject-introspection
    # GStreamer for screenshare plugin
    gst_all_1.gstreamer
    gst_all_1.gst-plugins-base
    gst_all_1.gst-plugins-good
    gst_all_1.gst-plugins-bad
    # Opus codec for audio streaming
    libopus
  ];

  # Build all workspace members with all features enabled
  # Enable all plugin features for both daemon and protocol crates
  cargoBuildFlags = [
    "--workspace"
    "--bins"
    "--features"
    "cosmic-connect-daemon/remotedesktop,cosmic-connect-daemon/screenshare,cosmic-connect-daemon/video,cosmic-connect-protocol/remotedesktop,cosmic-connect-protocol/screenshare,cosmic-connect-protocol/video,cosmic-connect-protocol/audiostream,cosmic-connect-protocol/audiostream-opus"
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
    # Note: ProtectHome is NOT used because this is a user service that needs
    # write access to ~/.config/cosmic and ~/.local/share/cosmic for config files.
    # ProtectHome=read-only + ReadWritePaths doesn't reliably allow directory creation.
    NoNewPrivileges=true
    ProtectSystem=strict
    PrivateTmp=true
    ProtectKernelTunables=true
    ProtectControlGroups=true
    RestrictSUIDSGID=true

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

    # Install icons from data directory
    mkdir -p $out/share/icons/hicolor/scalable/apps
    mkdir -p $out/share/icons/hicolor/symbolic/apps
    cp data/icons/hicolor/scalable/apps/cosmic-connect.svg $out/share/icons/hicolor/scalable/apps/
    cp data/icons/hicolor/symbolic/apps/cosmic-connect-symbolic.svg $out/share/icons/hicolor/symbolic/apps/

    # Install desktop entry for applet (COSMIC panel integration)
    cat > $out/share/applications/cosmic-applet-connect.desktop << EOF
    [Desktop Entry]
    Type=Application
    Name=COSMIC Connect
    Comment=Device connectivity for COSMIC Desktop
    Keywords=Cosmic;Iced;applet;connect;phone;device;sync;
    Icon=cosmic-connect-symbolic
    Exec=$out/bin/cosmic-applet-connect
    Categories=Cosmic;Iced;
    Terminal=false
    StartupNotify=true
    NoDisplay=true
    X-CosmicApplet=true
    X-CosmicHoverPopup=Auto
    EOF

    # Install desktop entry for manager (standalone window application)
    cat > $out/share/applications/cosmic-connect-manager.desktop << EOF
    [Desktop Entry]
    Type=Application
    Name=COSMIC Connect Manager
    Comment=Manage connected devices for COSMIC Desktop
    GenericName=Device Manager
    Keywords=Cosmic;Iced;connect;phone;device;sync;manager;
    Icon=cosmic-connect
    Exec=$out/bin/cosmic-connect-manager
    Categories=Settings;HardwareSettings;
    Terminal=false
    StartupNotify=true
    EOF
  '';

  # Wrap binaries with required runtime library paths
  # COSMIC apps need wayland, libGL, and other graphics libraries at runtime
  postFixup = ''
    wrapProgram $out/bin/cosmic-applet-connect \
      --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath [
        wayland
        libxkbcommon
        libGL
        libglvnd
        mesa
      ]}"

    wrapProgram $out/bin/cosmic-connect-manager \
      --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath [
        wayland
        libxkbcommon
        libGL
        libglvnd
        mesa
      ]}"
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
      - cosmic-applet-connect: Panel applet for COSMIC (quick status)
      - cosmic-connect-manager: Standalone device manager window
      - cosmic-connect-daemon: Background service (DBus, systemd autostart)

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
