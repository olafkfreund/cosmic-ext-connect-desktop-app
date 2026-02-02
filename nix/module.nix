{
  config,
  lib,
  pkgs,
  ...
}:

with lib;

let
  cfg = config.services.cosmic-connect;

  # Import the package
  cosmic-connect-pkg = pkgs.callPackage ./package.nix { };

  # TOML format generator
  tomlFormat = pkgs.formats.toml { };

in
{
  # PAM module import disabled until Phone Auth is fully implemented
  # imports = [ ./modules/pam-cosmic-connect.nix ];

  options.services.cosmic-connect = {
    enable = mkEnableOption "COSMIC Connect - Device connectivity for COSMIC Desktop";

    package = mkOption {
      type = types.package;
      default = cosmic-connect-pkg;
      example = literalExpression "pkgs.cosmic-connect";
      description = "The cosmic-connect package to use.";
    };

    openFirewall = mkOption {
      type = types.bool;
      default = true;
      description = ''
        Whether to open the firewall for COSMIC Connect.
        Opens TCP and UDP ports 1814-1864 for device discovery and TCP ports 1739-1764 for file transfer.
      '';
    };

    daemon = {
      enable = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Whether to enable the COSMIC Connect daemon as a user service.
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
        type = types.enum [
          "error"
          "warn"
          "info"
          "debug"
          "trace"
        ];
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
              listen_port = 1816;
            };
            security = {
              certificate_dir = "~/.config/cosmic-connect/certs";
            };
          }
        '';
        description = ''
          Additional configuration for the COSMIC Connect daemon.
          These settings are merged with plugin configuration and written to /etc/xdg/cosmic-connect/daemon.toml
          Plugin settings are automatically configured based on services.cosmic-connect.plugins options.
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

      remotedesktop = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable RemoteDesktop plugin (VNC-based remote desktop).
          Allows screen sharing and remote control between devices.
          Requires PipeWire and Wayland portal support.
        '';
      };

      runcommand = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable RunCommand plugin for remote command execution.
          Allows executing predefined commands on paired devices.
        '';
      };

      remoteinput = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable RemoteInput plugin for remote mouse and keyboard control.
          Useful for presentations and remote assistance.
        '';
      };

      findmyphone = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable FindMyPhone plugin to trigger audio alerts on devices.
          Emergency feature to help locate misplaced devices.
        '';
      };

      lock = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable Lock plugin for remote desktop lock/unlock control.
          Allows locking and unlocking the desktop session remotely.
        '';
      };

      telephony = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable Telephony plugin for SMS and call notifications.
          Displays incoming calls and text messages from mobile devices.
        '';
      };

      presenter = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable Presenter plugin for laser pointer and presentation controls.
          Useful for presentations and remote control.
        '';
      };

      contacts = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable Contacts plugin for contact list synchronization.
          Syncs contact information between devices via vCard format.
        '';
      };

      systemmonitor = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable SystemMonitor plugin for desktop-to-desktop resource monitoring.
          Shares CPU, memory, disk, and network usage statistics.
        '';
      };

      wol = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable Wake-on-LAN plugin for remote desktop power management.
          Send magic packets to wake sleeping desktops over the network.
        '';
      };

      screenshot = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable Screenshot plugin for remote desktop screen capture.
          Capture and transfer screenshots from remote desktops.
        '';
      };

      camera = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable Camera plugin for remote camera/webcam access.
          Use mobile device camera as webcam on desktop.
        '';
      };

      screenshare = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable ScreenShare plugin for one-way screen sharing.
          Useful for presentations and demonstrations.
        '';
      };

      power = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable Power plugin for remote power management.
          Control shutdown, reboot, and sleep on remote devices.
        '';
      };

      clipboardhistory = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable ClipboardHistory plugin for persistent clipboard history.
          Maintains a history of clipboard entries with sync support.
        '';
      };

      macro = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable Macro plugin for automation scripts.
          Execute predefined automation sequences on devices.
        '';
      };

      chat = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable Chat plugin for instant messaging.
          Send and receive messages between connected devices.
        '';
      };

      audiostream = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable AudioStream plugin for audio streaming.
          Stream audio between desktop devices via PipeWire.
        '';
      };

      filesync = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable FileSync plugin for automatic file synchronization.
          Keep directories in sync between connected desktops.
        '';
      };

      mousekeyboardshare = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable MouseKeyboardShare plugin (Synergy-like input sharing).
          Share mouse and keyboard seamlessly across multiple desktops.
        '';
      };

      networkshare = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable NetworkShare plugin for SFTP mounting.
          Mount remote device filesystems via secure SFTP.
        '';
      };
    };

    transport = {
      enableTcp = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Enable TCP/IP transport for device communication.
          Used for WiFi and Ethernet connections.
        '';
      };

      enableBluetooth = mkOption {
        type = types.bool;
        default = false;
        description = ''
          Enable Bluetooth transport for device communication.
          Allows connecting to devices via Bluetooth when WiFi is unavailable.
        '';
      };

      preference = mkOption {
        type = types.enum [ "prefer_tcp" "prefer_bluetooth" "tcp_first" "bluetooth_first" "only_tcp" "only_bluetooth" ];
        default = "prefer_tcp";
        description = ''
          Transport preference for new connections.
          - prefer_tcp: Prefer TCP if available (default)
          - prefer_bluetooth: Prefer Bluetooth if available
          - tcp_first: Try TCP first, fallback to Bluetooth
          - bluetooth_first: Try Bluetooth first, fallback to TCP
          - only_tcp: Only use TCP
          - only_bluetooth: Only use Bluetooth
        '';
      };

      autoFallback = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Automatically fallback to alternative transport if primary fails.
        '';
      };

      tcpTimeout = mkOption {
        type = types.int;
        default = 10;
        description = "TCP operation timeout in seconds.";
      };

      bluetoothTimeout = mkOption {
        type = types.int;
        default = 15;
        description = "Bluetooth operation timeout in seconds (higher due to BLE latency).";
      };
    };

    security = {
      certificateDirectory = mkOption {
        type = types.str;
        default = "~/.config/cosmic-connect/certs";
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
        default = "~/Downloads";
        description = ''
          Directory where received files are stored.
        '';
      };

      dataDirectory = mkOption {
        type = types.str;
        default = "~/.local/share/cosmic-connect";
        description = ''
          Base directory for COSMIC Connect data.
        '';
      };
    };
  };

  config = mkIf cfg.enable {
    # Assertion checks
    assertions = [
      {
        assertion = cfg.daemon.enable -> cfg.enable;
        message = "The COSMIC Connect daemon requires services.cosmic-connect.enable to be true.";
      }
      {
        assertion = cfg.applet.enable -> cfg.enable;
        message = "The COSMIC Connect applet requires services.cosmic-connect.enable to be true.";
      }
    ];

    # Install the package system-wide
    environment.systemPackages = [ cfg.package ];

    # Register DBus services so the session bus finds our .service files
    services.dbus.packages = [ cfg.package ];

    # Open firewall ports if requested
    networking.firewall = mkIf cfg.openFirewall {
      allowedTCPPortRanges = [
        {
          from = 1814;
          to = 1864;
        } # Discovery (CConnect)
        {
          from = 1739;
          to = 1764;
        } # File transfer (protocol standard)
      ];
      allowedUDPPortRanges = [
        {
          from = 1814;
          to = 1864;
        } # Discovery (CConnect)
      ];
    };

    # User systemd service for the daemon
    systemd.user.services.cosmic-connect-daemon = mkIf cfg.daemon.enable {
      description = "COSMIC Connect Daemon - Device connectivity service";
      documentation = [ "https://github.com/olafkfreund/cosmic-connect-desktop-app" ];

      after = [ "network.target" ];
      wantedBy = mkIf cfg.daemon.autoStart [ "default.target" ];

      serviceConfig = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/cosmic-connect-daemon";
        Restart = "on-failure";
        RestartSec = 5;

        # Security hardening
        # Note: ProtectHome is NOT used because this is a user service that needs
        # write access to ~/.config/cosmic and ~/.local/share/cosmic for config files.
        # ProtectHome=true + ReadWritePaths doesn't reliably allow directory creation.
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        PrivateTmp = true;
        ProtectKernelTunables = true;
        ProtectControlGroups = true;
        RestrictSUIDSGID = true;

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
    environment.etc."xdg/cosmic-connect/daemon.toml" = mkIf cfg.daemon.enable {
      source =
        let
          daemonConfig = {
            plugins = {
              enable_ping = cfg.plugins.ping;
              enable_battery = cfg.plugins.battery;
              enable_notification = cfg.plugins.notification;
              enable_share = cfg.plugins.share;
              enable_clipboard = cfg.plugins.clipboard;
              enable_mpris = cfg.plugins.mpris;
              enable_runcommand = cfg.plugins.runcommand;
              enable_remoteinput = cfg.plugins.remoteinput;
              enable_findmyphone = cfg.plugins.findmyphone;
              enable_lock = cfg.plugins.lock;
              enable_telephony = cfg.plugins.telephony;
              enable_presenter = cfg.plugins.presenter;
              enable_contacts = cfg.plugins.contacts;
              enable_systemmonitor = cfg.plugins.systemmonitor;
              enable_wol = cfg.plugins.wol;
              enable_screenshot = cfg.plugins.screenshot;
              enable_remotedesktop = cfg.plugins.remotedesktop;
              enable_camera = cfg.plugins.camera;
              enable_screenshare = cfg.plugins.screenshare;
              enable_power = cfg.plugins.power;
              enable_clipboardhistory = cfg.plugins.clipboardhistory;
              enable_macro = cfg.plugins.macro;
              enable_chat = cfg.plugins.chat;
              enable_audiostream = cfg.plugins.audiostream;
              enable_filesync = cfg.plugins.filesync;
              enable_mousekeyboardshare = cfg.plugins.mousekeyboardshare;
              enable_networkshare = cfg.plugins.networkshare;
            };
            transport = {
              enable_tcp = cfg.transport.enableTcp;
              enable_bluetooth = cfg.transport.enableBluetooth;
              preference = cfg.transport.preference;
              auto_fallback = cfg.transport.autoFallback;
              tcp_timeout_secs = cfg.transport.tcpTimeout;
              bluetooth_timeout_secs = cfg.transport.bluetoothTimeout;
            };
          };
          # Merge user settings with daemon config
          finalConfig = lib.recursiveUpdate daemonConfig cfg.daemon.settings;
        in
        tomlFormat.generate "daemon.toml" finalConfig;
    };

    # Create necessary directories
    system.activationScripts.cosmic-connect = ''
      # Ensure config directory exists
      mkdir -p /etc/xdg/cosmic-connect

      # Set proper permissions
      chmod 755 /etc/xdg/cosmic-connect
    '';

    # Warnings for common misconfigurations
    warnings =
      (optional (
        !cfg.openFirewall
      ) "COSMIC Connect firewall ports are not open. Device discovery may not work.")
      ++ (optional (!cfg.daemon.enable && cfg.applet.enable)
        "The COSMIC Connect applet is enabled but the daemon is not. The applet requires the daemon to function."
      )
      ++ (optional (
        !cfg.plugins.share && !cfg.plugins.notification && !cfg.plugins.clipboard
      ) "All major plugins are disabled. Consider enabling at least one plugin for functionality.");

  };

  meta = {
    maintainers = with maintainers; [ ]; # Add your maintainer info
    # doc = ./module.md; # Disabled: Causes NixOS manual redirect requirements for third-party modules
  };
}
