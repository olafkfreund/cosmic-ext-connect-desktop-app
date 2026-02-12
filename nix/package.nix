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
  libgbm,
  stdenv,
}:

rustPlatform.buildRustPackage rec {
  pname = "cosmic-ext-connect";
  version = "0.18.0";

  # Use cleanSourceWith to exclude cosmic-ext-connect-core (git submodule)
  # Cargo will fetch cosmic-ext-connect-core as a git dependency via outputHashes
  src = lib.cleanSourceWith {
    src = ../.;
    filter =
      path: type:
      let
        baseName = baseNameOf path;
        relativePath = lib.removePrefix (toString ../. + "/") (toString path);
      in
      # Exclude cosmic-ext-connect-core subdirectory (git submodule)
      !lib.hasPrefix "cosmic-ext-connect-core" relativePath
      # Exclude .gitmodules to prevent git submodule conflicts
      && baseName != ".gitmodules"
      # Include everything else
      && (lib.cleanSourceFilter path type);
  };

  cargoLock = {
    lockFile = ../Cargo.lock;
    outputHashes = {
      "accesskit-0.16.0" = "sha256-uoLcd116WXQTu1ZTfJDEl9+3UPpGBN/QuJpkkGyRADQ=";
      "atomicwrites-0.4.2" = "sha256-QZSuGPrJXh+svMeFWqAXoqZQxLq/WfIiamqvjJNVhxA=";
      "clipboard_macos-0.1.0" = "sha256-+8CGmBf1Gl9gnBDtuKtkzUE5rySebhH7Bsq/kNlJofY=";
      "cosmic-client-toolkit-0.1.0" = "sha256-KvXQJ/EIRyrlmi80WKl2T9Bn+j7GCfQlcjgcEVUxPkc=";
      "cosmic-config-1.0.0" = "sha256-pfT6/cYjA3CGrXr2d7aAwfW+7FUNdfQvAeOWkknu/Y8=";
      "cosmic-ext-connect-core-0.9.0" = "sha256-KRwM9DA8yoUJiJlLLrcrjhTa9D3X6wZYEhyA7/1X6zk=";
      "cosmic-freedesktop-icons-0.4.0" = "sha256-D4bWHQ4Dp8UGiZjc6geh2c2SGYhB7mX13THpCUie1c4=";
      "cosmic-panel-config-0.1.0" = "sha256-1Xwe1uONJbl4wq6QBbTI1suLiSlTzU4e/5WBccvghHE=";
      "cosmic-settings-daemon-0.1.0" = "sha256-1yVIL3SQnOEtTHoLiZgBH21holNxcOuToyQ+QdvqoBg=";
      "cosmic-text-0.17.1" = "sha256-NHjJBE/WSMhN29CKTuB7PyJv4y2JByi5pyTUDtVoF7g=";
      "dpi-0.1.1" = "sha256-Saw9LIWIbOaxD5/yCSqaN71Tzn2NXFzJMorH8o58ktw=";
      "iced_glyphon-0.6.0" = "sha256-u1vnsOjP8npQ57NNSikotuHxpi4Mp/rV9038vAgCsfQ=";
      "smithay-clipboard-0.8.0" = "sha256-4InFXm0ahrqFrtNLeqIuE3yeOpxKZJZx+Bc0yQDtv34=";
      "softbuffer-0.4.1" = "sha256-/ocK79Lr5ywP/bb5mrcm7eTzeBbwpOazojvFUsAjMKM=";
    };
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
    gst_all_1.gst-plugins-ugly # H.264 codec support
    # Opus codec for audio streaming
    libopus
    # DMA-BUF / GBM support for extended display capture
    libgbm
  ];

  # Build all workspace members with all features enabled
  # Enable all plugin features for daemon, protocol, and applet crates
  # Features:
  #   - remotedesktop: VNC-based remote desktop (requires pipewire, openh264)
  #   - screenshare: One-way screen sharing (requires gstreamer)
  #   - video: V4L2 camera support
  #   - audiostream: Audio streaming between devices (requires pipewire)
  #   - audiostream-opus: Opus codec for audio (requires libopus)
  #   - extendeddisplay: Extended display to Android tablet (requires libgbm, gstreamer)
  #   - low_latency: Performance optimizations for remote desktop
  cargoBuildFlags = [
    "--workspace"
    "--bins"
    "--features"
    "cosmic-ext-connect-daemon/remotedesktop,cosmic-ext-connect-daemon/screenshare,cosmic-ext-connect-daemon/video,cosmic-ext-connect-daemon/audiostream,cosmic-ext-connect-daemon/audiostream-opus,cosmic-ext-connect-daemon/extendeddisplay,cosmic-ext-connect-protocol/remotedesktop,cosmic-ext-connect-protocol/screenshare,cosmic-ext-connect-protocol/video,cosmic-ext-connect-protocol/audiostream,cosmic-ext-connect-protocol/audiostream-opus,cosmic-ext-connect-protocol/extendeddisplay,cosmic-ext-connect-protocol/low_latency,cosmic-ext-applet-connect/screenshare"
  ];

  # Skip tests for now - test compilation has issues with json! macro imports
  doCheck = false;

  # Test all workspace members (when tests are fixed)
  # cargoTestFlags = [
  #   "--workspace"
  # ];

  # bindgenHook automatically sets LIBCLANG_PATH and BINDGEN_EXTRA_CLANG_ARGS

  # Tell audiopus_sys to use system opus library instead of building from source
  OPUS_LIB_DIR = "${libopus}/lib";
  OPUS_INCLUDE_DIR = "${libopus}/include";

  # Ensure proper library paths at runtime
  postInstall = ''
    # Install systemd service
    mkdir -p $out/lib/systemd/user
    cat > $out/lib/systemd/user/cosmic-ext-connect-daemon.service << EOF
    [Unit]
    Description=COSMIC Connect Daemon - Device connectivity service
    After=network.target

    [Service]
    Type=simple
    ExecStart=$out/bin/cosmic-ext-connect-daemon
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
    cat > $out/share/dbus-1/services/io.github.olafkfreund.CosmicExtConnect.service << EOF
    [D-BUS Service]
    Name=io.github.olafkfreund.CosmicExtConnect
    Exec=$out/bin/cosmic-ext-connect-daemon
    SystemdService=cosmic-ext-connect-daemon.service
    EOF

    # Install desktop entries
    mkdir -p $out/share/applications

    # Install desktop entry for cosmic-messages
    cat > $out/share/applications/io.github.olafkfreund.CosmicExtMessages.desktop << EOF
    [Desktop Entry]
    Type=Application
    Name=Cosmic Messages
    Comment=Web-based messaging for COSMIC
    Icon=mail-message-new-symbolic
    Exec=$out/bin/cosmic-ext-messages
    Categories=Network;Chat;
    NoDisplay=false
    EOF

    # Install icons from data directory
    mkdir -p $out/share/icons/hicolor/scalable/apps
    mkdir -p $out/share/icons/hicolor/symbolic/apps
    cp data/icons/hicolor/scalable/apps/cosmic-ext-connect.svg $out/share/icons/hicolor/scalable/apps/
    cp data/icons/hicolor/symbolic/apps/cosmic-ext-connect-symbolic.svg $out/share/icons/hicolor/symbolic/apps/
    cp data/icons/hicolor/symbolic/apps/cosmic-ext-connect-phone-symbolic.svg $out/share/icons/hicolor/symbolic/apps/
    cp data/icons/hicolor/symbolic/apps/cosmic-ext-connect-tablet-symbolic.svg $out/share/icons/hicolor/symbolic/apps/
    cp data/icons/hicolor/symbolic/apps/cosmic-ext-connect-laptop-symbolic.svg $out/share/icons/hicolor/symbolic/apps/
    cp data/icons/hicolor/symbolic/apps/cosmic-ext-connect-desktop-symbolic.svg $out/share/icons/hicolor/symbolic/apps/
    cp data/icons/hicolor/symbolic/apps/cosmic-ext-connect-tv-symbolic.svg $out/share/icons/hicolor/symbolic/apps/

    # Install desktop entry for applet (COSMIC panel integration)
    cat > $out/share/applications/cosmic-ext-applet-connect.desktop << EOF
    [Desktop Entry]
    Type=Application
    Name=COSMIC Connect
    Comment=Device connectivity for COSMIC Desktop
    Keywords=Cosmic;Iced;applet;connect;phone;device;sync;
    Icon=cosmic-ext-connect-symbolic
    Exec=$out/bin/cosmic-ext-applet-connect
    Categories=Cosmic;Iced;
    Terminal=false
    StartupNotify=true
    NoDisplay=true
    X-CosmicApplet=true
    X-CosmicHoverPopup=Auto
    EOF

    # Install desktop entry for manager (standalone window application)
    cat > $out/share/applications/cosmic-ext-connect-manager.desktop << EOF
    [Desktop Entry]
    Type=Application
    Name=COSMIC Connect Manager
    Comment=Manage connected devices for COSMIC Desktop
    GenericName=Device Manager
    Keywords=Cosmic;Iced;connect;phone;device;sync;manager;
    Icon=cosmic-ext-connect
    Exec=$out/bin/cosmic-ext-connect-manager
    Categories=Settings;HardwareSettings;
    Terminal=false
    StartupNotify=true
    EOF
  '';

  # Wrap binaries with required runtime library paths
  # COSMIC apps need wayland, libGL, and other graphics libraries at runtime
  # The daemon needs GStreamer plugin paths for screenshare functionality
  postFixup = let
    gstPluginPath = lib.makeSearchPathOutput "lib" "lib/gstreamer-1.0" [
      gst_all_1.gstreamer
      gst_all_1.gst-plugins-base
      gst_all_1.gst-plugins-good
      gst_all_1.gst-plugins-bad
      gst_all_1.gst-plugins-ugly
      pipewire # Contains pipewiresrc element
    ];
  in ''
    wrapProgram $out/bin/cosmic-ext-applet-connect \
      --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath [
        wayland
        libxkbcommon
        libGL
        libglvnd
        mesa
      ]}"

    wrapProgram $out/bin/cosmic-ext-connect-manager \
      --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath [
        wayland
        libxkbcommon
        libGL
        libglvnd
        mesa
      ]}"

    # Wrap mirror viewer with GStreamer plugin paths for screenshare decoding
    wrapProgram $out/bin/cosmic-ext-connect-mirror \
      --prefix GST_PLUGIN_SYSTEM_PATH_1_0 : "${gstPluginPath}" \
      --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath [
        wayland
        libxkbcommon
        libGL
        libglvnd
        mesa
        gst_all_1.gstreamer
        gst_all_1.gst-plugins-base
      ]}"

    # Wrap daemon with GStreamer plugin paths for screenshare capture
    wrapProgram $out/bin/cosmic-ext-connect-daemon \
      --prefix GST_PLUGIN_SYSTEM_PATH_1_0 : "${gstPluginPath}" \
      --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath [
        pipewire
        gst_all_1.gstreamer
        gst_all_1.gst-plugins-base
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
      - Extended display (use Android tablet as second monitor)
      - CConnect protocol (port 1816, side-by-side with KDE Connect)

      This package includes:
      - cosmic-ext-applet-connect: Panel applet for COSMIC (quick status)
      - cosmic-ext-connect-manager: Standalone device manager window
      - cosmic-ext-connect-daemon: Background service (DBus, systemd autostart)

      Built with RemoteDesktop plugin support (requires PipeWire).
    '';
    homepage = "https://github.com/olafkfreund/cosmic-ext-connect-desktop-app";
    changelog = "https://github.com/olafkfreund/cosmic-ext-connect-desktop-app/releases";
    license = licenses.gpl3Plus;
    maintainers = with maintainers; [ ]; # Add your maintainer info
    mainProgram = "cosmic-ext-applet-connect";
    platforms = platforms.linux;

    # Requires COSMIC Desktop Environment
    broken = false;
  };
}
