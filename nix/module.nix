{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.services.cosmic-kdeconnect;

  # Import the package
  cosmic-kdeconnect-pkg = pkgs.callPackage ./package.nix { };

in {
  options.services.cosmic-kdeconnect = {
    enable = mkEnableOption "KDE Connect for COSMIC Desktop";

    package = mkPackageOption pkgs "cosmic-applet-kdeconnect" {
      default = cosmic-kdeconnect-pkg;
      example = literalExpression "pkgs.cosmic-applet-kdeconnect";
      description = "The cosmic-applet-kdeconnect package to use.";
    };

    openFirewall = mkOption {
      type = types.bool;
      default = true;
      description = ''
        Whether to open the firewall for KDE Connect.
        Opens TCP and UDP ports 1714-1764 for device discovery and communication.
      '';
    };

    daemon = {
      enable = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Whether to enable the KDE Connect daemon as a user service.
          The daemon handles device discovery, pairing, and plugin communication.
        '';
      };

      autoStart = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Whether to automatically start the daemon on login.
        '';
      };

      logLevel = mkOption {
        type = types.enum [ "error" "warn" "info" "debug" "trace" ];
        default = "info";
        example = "debug";
        description = ''
          Logging level for the KDE Connect daemon.
          Use "debug" or "trace" for troubleshooting.
        '';
      };

      settings = mkOption {
        type = with types; attrsOf anything;
        default = { };
        example = literalExpression ''
          {
            discovery = {
              broadcast_interval = 5000;
              listen_port = 1716;
            };
            security = {
              certificate_dir = "~/.config/kdeconnect/certs";
            };
          }
        '';
        description = ''
          Configuration for the KDE Connect daemon.
          Settings are written to ~/.config/kdeconnect/config.toml
        '';
      };
    };

    applet = {
      enable = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Whether to enable the COSMIC panel applet.
          The applet provides quick access to connected devices and features.
        '';
      };
    };

    plugins = {
      battery = mkOption {
        type = types.bool;
        default = true;
        description = "Enable battery status monitoring from mobile devices.";
      };

      clipboard = mkOption {
        type = types.bool;
        default = true;
        description = "Enable clipboard synchronization between devices.";
      };

      notification = mkOption {
        type = types.bool;
        default = true;
        description = "Enable notification mirroring from mobile devices.";
      };

      share = mkOption {
        type = types.bool;
        default = true;
        description = "Enable file and URL sharing between devices.";
      };

      mpris = mkOption {
        type = types.bool;
        default = true;
        description = "Enable media player control (MPRIS) integration.";
      };

      ping = mkOption {
        type = types.bool;
        default = true;
        description = "Enable ping functionality for testing connectivity.";
      };
    };

    security = {
      certificateDirectory = mkOption {
        type = types.str;
        default = "~/.config/kdeconnect/certs";
        description = ''
          Directory where device certificates are stored.
          Each paired device has its own certificate for TLS communication.
        '';
      };

      trustOnFirstPair = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Whether to trust devices on first pairing without manual verification.
          Disable for enhanced security in untrusted network environments.
        '';
      };
    };

    storage = {
      downloadDirectory = mkOption {
        type = types.str;
        default = "~/.local/share/kdeconnect/downloads";
        description = ''
          Directory where received files are stored.
        '';
      };

      dataDirectory = mkOption {
        type = types.str;
        default = "~/.local/share/kdeconnect";
        description = ''
          Base directory for KDE Connect data.
        '';
      };
    };
  };

  config = mkIf cfg.enable {
    # Assertion checks
    assertions = [
      {
        assertion = cfg.daemon.enable -> cfg.enable;
        message = "The KDE Connect daemon requires services.cosmic-kdeconnect.enable to be true.";
      }
      {
        assertion = cfg.applet.enable -> cfg.enable;
        message = "The COSMIC applet requires services.cosmic-kdeconnect.enable to be true.";
      }
    ];

    # Install the package system-wide
    environment.systemPackages = [ cfg.package ];

    # Open firewall ports if requested
    networking.firewall = mkIf cfg.openFirewall {
      allowedTCPPortRanges = [
        { from = 1714; to = 1764; }
      ];
      allowedUDPPortRanges = [
        { from = 1714; to = 1764; }
      ];
    };

    # User systemd service for the daemon
    systemd.user.services.kdeconnect-daemon = mkIf cfg.daemon.enable {
      description = "KDE Connect Daemon for COSMIC Desktop";
      documentation = [ "https://github.com/olafkfreund/cosmic-applet-kdeconnect" ];

      after = [ "network.target" ];
      wantedBy = mkIf cfg.daemon.autoStart [ "default.target" ];

      serviceConfig = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/kdeconnect-daemon";
        Restart = "on-failure";
        RestartSec = 5;

        # Security hardening
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        ProtectKernelTunables = true;
        ProtectControlGroups = true;
        RestrictSUIDSGID = true;
        RestrictRealtime = true;
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        SystemCallArchitectures = "native";
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        RestrictNamespaces = true;

        # File system access
        ReadWritePaths = [
          "%h/.config/kdeconnect"
          "%h/.local/share/kdeconnect"
        ];

        # Network access required
        PrivateNetwork = false;
        IPAddressDeny = mkIf (!cfg.openFirewall) [ "any" ];

        # Resource limits
        TasksMax = 1000;
        MemoryMax = "1G";
      };

      environment = {
        RUST_LOG = cfg.daemon.logLevel;
        RUST_BACKTRACE = "1";
      };
    };

    # Generate configuration file
    environment.etc."xdg/kdeconnect/config.toml" = mkIf (cfg.daemon.settings != { }) {
      text = generators.toINI { } cfg.daemon.settings;
    };

    # Create necessary directories
    system.activationScripts.cosmic-kdeconnect = ''
      # Ensure config directory exists
      mkdir -p /etc/xdg/kdeconnect

      # Set proper permissions
      chmod 755 /etc/xdg/kdeconnect
    '';

    # Warnings for common misconfigurations
    warnings =
      (optional (!cfg.openFirewall)
        "KDE Connect firewall ports are not open. Device discovery may not work.")
      ++
      (optional (!cfg.daemon.enable && cfg.applet.enable)
        "The COSMIC applet is enabled but the daemon is not. The applet requires the daemon to function.")
      ++
      (optional (!cfg.plugins.share && !cfg.plugins.notification && !cfg.plugins.clipboard)
        "All major plugins are disabled. Consider enabling at least one plugin for functionality.");

    # Add documentation links
    documentation.man.man1 = [ "${cfg.package}/share/man/man1/kdeconnect-daemon.1.gz" ];
  };

  meta = {
    maintainers = with maintainers; [ ]; # Add your maintainer info
    doc = ./module.md; # Optional: Add module documentation
  };
}
