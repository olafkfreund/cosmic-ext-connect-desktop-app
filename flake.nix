{
  description = "COSMIC applet for KDE Connect - Device synchronization for COSMIC Desktop";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    let
      # Overlay for this package
      overlay = final: prev: {
        cosmic-connect = final.callPackage ./nix/package.nix { };
      };

      # NixOS module
      nixosModule = import ./nix/module.nix;

    in
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Rust toolchain
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" ];
        };

        # COSMIC Desktop libraries
        cosmicLibs = with pkgs; [
          libxkbcommon
          wayland
          wayland-protocols
          libGL
          libglvnd
          mesa
          pixman
          libinput
          libxcb
          xorg.xcbutil
          xorg.xcbutilwm
          xorg.xcbutilimage
          libdrm
          fontconfig
          freetype
          udev
          dbus
          libpulseaudio
          expat

          # Messaging popup dependencies
          webkitgtk_4_1
          glib
          gobject-introspection

          # RemoteDesktop plugin dependencies
          pipewire

          # ScreenShare plugin dependencies (GStreamer)
          gst_all_1.gstreamer
          gst_all_1.gst-plugins-base
          gst_all_1.gst-plugins-good
          gst_all_1.gst-plugins-bad
          gst_all_1.gst-plugins-ugly
          gst_all_1.gst-libav
        ];

        # Build dependencies
        buildInputs = with pkgs; [
          rustToolchain
          pkg-config
          cmake
          just

          # OpenSSL (both runtime and dev headers)
          openssl
          openssl.dev

          # DBus (both runtime and dev headers)
          dbus
          dbus.dev

          # COSMIC specific
          libxkbcommon
          wayland
          libinput

          # Development tools
          git
          gnumake
        ] ++ cosmicLibs;

        # Runtime dependencies
        runtimeInputs = with pkgs; [
          glib
          gtk3
          pango
          cairo
          gdk-pixbuf
          atk
        ];

        # Shell environment
        shellHook = ''
          echo "🚀 COSMIC Connect Development Environment"
          echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
          echo "Rust version: $(rustc --version)"
          echo "Cargo version: $(cargo --version)"
          echo ""
          echo "📦 Available commands:"
          echo "  just build          - Build all components"
          echo "  just run-applet     - Run applet in development"
          echo "  just test           - Run tests"
          echo "  just fmt            - Format code"
          echo "  just lint           - Run clippy"
          echo "  cargo check         - Fast compilation check"
          echo "  cargo build         - Full build"
          echo ""
          echo "🔧 Environment configured for COSMIC Desktop development"
          echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

          # Set up environment variables
          export RUST_BACKTRACE=1
          export RUST_LOG=debug

          # Library paths for runtime
          export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeInputs}:$LD_LIBRARY_PATH"

          # PKG_CONFIG paths - critical for finding dbus-1.pc and openssl.pc
          export PKG_CONFIG_PATH="${pkgs.lib.makeSearchPath "lib/pkgconfig" buildInputs}:$PKG_CONFIG_PATH"

          # Bindgen needs to find C standard library headers for PipeWire bindings
          export BINDGEN_EXTRA_CLANG_ARGS="-I${pkgs.glibc.dev}/include"

          # Verify critical dependencies are available
          echo ""
          echo "🔍 Verifying development dependencies..."

          if pkg-config --exists dbus-1; then
            echo "  ✓ dbus-1 found: $(pkg-config --modversion dbus-1)"
          else
            echo "  ✗ dbus-1 NOT FOUND - build will fail!"
          fi

          if pkg-config --exists openssl; then
            echo "  ✓ openssl found: $(pkg-config --modversion openssl)"
          else
            echo "  ✗ openssl NOT FOUND - build will fail!"
          fi

          if pkg-config --exists gstreamer-1.0; then
            echo "  ✓ gstreamer found: $(pkg-config --modversion gstreamer-1.0)"
          else
            echo "  ℹ gstreamer not found (optional for screenshare feature)"
          fi

          echo ""
          echo "Ready to build! Try: cargo check"
          echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        '';

      in
      {
        # Development shell
        devShells.default = pkgs.mkShell {
          inherit buildInputs shellHook;
          
          nativeBuildInputs = with pkgs; [
            pkg-config
            cmake
          ];

          # Additional environment variables
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        };

        # Package definition - use the one from nix/package.nix
        packages.default = pkgs.callPackage ./nix/package.nix { };

        # Apps for running
        apps.default = flake-utils.lib.mkApp {
          drv = self.packages.${system}.default;
        };

        # Tests
        checks = import ./nix/tests.nix { inherit pkgs; };
      }
    ) // {
      # Flake-level outputs (not system-specific)
      overlays.default = overlay;
      nixosModules.default = nixosModule;

      # Convenience aliases
      nixosModules.cosmic-connect = nixosModule;
      overlays.cosmic-connect = overlay;
    };
}
