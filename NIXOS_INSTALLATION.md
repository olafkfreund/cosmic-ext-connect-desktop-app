# COSMIC Connect - NixOS Multi-Host Installation Guide

This guide shows how to install COSMIC Connect with RemoteDesktop plugin support on multiple NixOS hosts using the flake.

## Quick Start

### 1. Add to Your NixOS Flake

In your NixOS configuration flake, add COSMIC Connect as an input:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    cosmic-connect.url = "github:olafkfreund/cosmic-connect-desktop-app";
    # Or use a local path during development:
    # cosmic-connect.url = "path:/home/user/cosmic-connect-desktop-app";
  };

  outputs = { self, nixpkgs, cosmic-connect, ... }: {
    nixosConfigurations = {
      my-desktop = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          ./configuration.nix
          cosmic-connect.nixosModules.default
        ];
      };
    };
  };
}
```

### 2. Enable in Configuration

Add to your `configuration.nix`:

```nix
{ config, pkgs, ... }:

{
  services.cosmic-connect = {
    enable = true;
    openFirewall = true;  # Opens ports 1814-1864 for discovery

    daemon = {
      enable = true;
      autoStart = true;
      logLevel = "info";  # Change to "debug" for troubleshooting
    };

    applet = {
      enable = true;  # System tray applet
    };

    plugins = {
      # Core plugins (enabled by default)
      battery = true;
      clipboard = true;
      notification = true;
      share = true;
      mpris = true;
      ping = true;

      # RemoteDesktop plugin (DISABLED by default for security)
      remotedesktop = true;  # Enable VNC-based screen sharing
    };
  };
}
```

### 3. Rebuild and Deploy

```bash
# Rebuild on current host
sudo nixos-rebuild switch --flake .#my-desktop

# Or deploy to remote host
nixos-rebuild switch --flake .#my-desktop \
  --target-host user@remote-host --use-remote-sudo
```

## Configuration Examples

### Desktop with RemoteDesktop Enabled

```nix
# desktop.nix - Full-featured desktop configuration
services.cosmic-connect = {
  enable = true;
  openFirewall = true;

  daemon = {
    enable = true;
    autoStart = true;
    logLevel = "info";
  };

  applet.enable = true;

  plugins = {
    # All plugins enabled including RemoteDesktop
    battery = true;
    clipboard = true;
    notification = true;
    share = true;
    mpris = true;
    ping = true;
    remotedesktop = true;  # Screen sharing enabled
  };

  security = {
    certificateDirectory = "~/.config/cosmic-connect/certs";
    trustOnFirstPair = true;
  };

  storage = {
    downloadDirectory = "~/Downloads";
    dataDirectory = "~/.local/share/cosmic-connect";
  };
};
```

### Laptop with RemoteDesktop Disabled

```nix
# laptop.nix - Secure mobile configuration
services.cosmic-connect = {
  enable = true;
  openFirewall = true;

  plugins = {
    battery = true;
    clipboard = true;
    notification = true;
    share = true;
    mpris = true;
    ping = true;
    remotedesktop = false;  # Disabled for security on mobile
  };
};
```

### Server (Headless)

```nix
# server.nix - Daemon-only configuration
services.cosmic-connect = {
  enable = true;
  openFirewall = true;

  daemon.enable = true;
  applet.enable = false;  # No GUI on server

  plugins = {
    battery = false;  # No battery on server
    clipboard = false;  # No clipboard without GUI
    notification = true;  # Can still receive notifications
    share = true;  # File sharing works
    mpris = false;  # No media player
    ping = true;
    remotedesktop = false;  # No display to share
  };
};
```

## Multi-Host Deployment

### Using a Shared Configuration Module

Create `cosmic-connect-shared.nix`:

```nix
{ config, lib, ... }:

{
  services.cosmic-connect = {
    enable = true;
    openFirewall = true;

    daemon = {
      enable = true;
      autoStart = true;
      logLevel = "info";
    };

    # Common plugins enabled on all hosts
    plugins = {
      battery = true;
      clipboard = true;
      notification = true;
      share = true;
      mpris = true;
      ping = true;
      # RemoteDesktop disabled by default, override per-host
      remotedesktop = false;
    };
  };
}
```

Then in each host configuration:

```nix
# host1/configuration.nix
{
  imports = [ ../cosmic-connect-shared.nix ];

  # Override for this host: enable RemoteDesktop
  services.cosmic-connect.plugins.remotedesktop = true;
}

# host2/configuration.nix
{
  imports = [ ../cosmic-connect-shared.nix ];

  # Use defaults (RemoteDesktop disabled)
}
```

### Using Host-Specific Overlays

```nix
# flake.nix
{
  outputs = { self, nixpkgs, cosmic-connect, ... }: {
    nixosConfigurations = {
      desktop = nixpkgs.lib.nixosSystem {
        modules = [
          cosmic-connect.nixosModules.default
          ./hosts/desktop/configuration.nix
          {
            services.cosmic-connect = {
              enable = true;
              plugins.remotedesktop = true;  # Enabled on desktop
            };
          }
        ];
      };

      laptop = nixpkgs.lib.nixosSystem {
        modules = [
          cosmic-connect.nixosModules.default
          ./hosts/laptop/configuration.nix
          {
            services.cosmic-connect = {
              enable = true;
              plugins.remotedesktop = false;  # Disabled on laptop
            };
          }
        ];
      };
    };
  };
}
```

## Firewall Ports

COSMIC Connect requires the following ports:

- **TCP/UDP 1814-1864**: Device discovery (CConnect protocol)
- **TCP 1739-1764**: File transfer
- **TCP 5900**: VNC server (when RemoteDesktop is active)

The module automatically opens these ports when `openFirewall = true`.

## Security Considerations

### RemoteDesktop Plugin

The RemoteDesktop plugin:
- Is **disabled by default** for security
- Requires **explicit opt-in** via `plugins.remotedesktop = true`
- Generates a **random VNC password** per session
- Requires **pairing** before screen sharing is allowed
- Uses **TLS encryption** for the connection

### Recommended Security Practices

1. **Enable RemoteDesktop only on trusted networks**
2. **Use firewall rules** to restrict access:
   ```nix
   networking.firewall.extraCommands = ''
     # Only allow RemoteDesktop from local network
     iptables -A nixos-fw -p tcp --dport 5900 \
       -s 192.168.1.0/24 -j nixos-fw-accept
   '';
   ```

3. **Disable on mobile/untrusted devices**
4. **Monitor active sessions** via the applet

## Testing RemoteDesktop

### On Host 1 (Desktop - Sharing Screen)

```nix
services.cosmic-connect.plugins.remotedesktop = true;
```

### On Host 2 (Laptop - Viewing Screen)

1. **Install a VNC client**:
   ```nix
   environment.systemPackages = [ pkgs.tigervnc ];
   ```

2. **Pair the devices** through the COSMIC Connect applet

3. **Request remote desktop** from Host 1

4. **Connect with VNC client**:
   ```bash
   vncviewer <host1-ip>:5900
   # Enter the password shown in Host 1's notification
   ```

## Troubleshooting

### Check Service Status

```bash
# Check if daemon is running
systemctl --user status cosmic-connect-daemon

# View logs
journalctl --user -u cosmic-connect-daemon -f
```

### Enable Debug Logging

```nix
services.cosmic-connect.daemon.logLevel = "debug";
```

### Verify Firewall

```bash
# Check open ports
sudo ss -tulpn | grep cosmic-connect

# Test connectivity from another host
nc -zv <host-ip> 1816
```

### RemoteDesktop Not Working

1. **Verify PipeWire is running**:
   ```bash
   systemctl --user status pipewire
   ```

2. **Check Wayland portal**:
   ```bash
   # Should show xdg-desktop-portal running
   ps aux | grep portal
   ```

3. **Test VNC server manually**:
   ```bash
   # Check if VNC server is listening
   sudo ss -tulpn | grep 5900
   ```

## Advanced Configuration

### Custom Daemon Settings

```nix
services.cosmic-connect.daemon.settings = {
  discovery = {
    broadcast_interval = 5000;
    listen_port = 1816;
  };
  security = {
    certificate_dir = "~/.config/cosmic-connect/certs";
  };
};
```

### Integration with Home Manager

```nix
# home.nix
{ config, pkgs, ... }:

{
  # User-specific configuration
  xdg.configFile."cosmic-connect/daemon.toml".text = ''
    [device]
    name = "My Desktop"
    device_type = "desktop"

    [plugins]
    enable_remotedesktop = true
  '';
}
```

## Building from Source

### Local Development

```bash
# Enter development shell
nix develop

# Build
just build

# Run locally
./run-local-test.sh
```

### Build Flake Package

```bash
# Build the package
nix build .#

# Run from result
./result/bin/cosmic-connect-daemon --help
```

## Updating

### Update Flake Input

```bash
# Update to latest commit
nix flake update cosmic-connect

# Update and rebuild
sudo nixos-rebuild switch --flake .#
```

### Pin to Specific Version

```nix
{
  inputs.cosmic-connect.url = "github:olafkfreund/cosmic-connect-desktop-app?ref=v0.1.0";
}
```

## Support

- **Issues**: https://github.com/olafkfreund/cosmic-connect-desktop-app/issues
- **Protocol Docs**: See `cosmic-connect-protocol/src/plugins/remotedesktop/README.md`
- **Implementation Status**: See `IMPLEMENTATION_STATUS.md`

---

**Note**: This guide assumes you're using NixOS with flakes enabled. Add `experimental-features = nix-command flakes` to your `nix.conf` if not already enabled.
