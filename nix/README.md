# NixOS Packaging

This directory contains NixOS packaging files for COSMIC KDE Connect.

## Files

- **package.nix** - Package derivation for building cosmic-applet-kdeconnect
- **module.nix** - NixOS module with configuration options
- **tests.nix** - NixOS VM tests for the package and module
- **README.md** - This file

## Usage

### Using the Flake

Add to your `flake.nix`:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    cosmic-kdeconnect.url = "github:olafkfreund/cosmic-applet-kdeconnect";
  };

  outputs = { self, nixpkgs, cosmic-kdeconnect }: {
    nixosConfigurations.your-hostname = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        cosmic-kdeconnect.nixosModules.default
        {
          services.cosmic-kdeconnect = {
            enable = true;
            openFirewall = true;
          };
        }
      ];
    };
  };
}
```

### Using the Overlay

```nix
{
  inputs.cosmic-kdeconnect.url = "github:olafkfreund/cosmic-applet-kdeconnect";

  outputs = { nixpkgs, cosmic-kdeconnect, ... }: {
    nixosConfigurations.your-hostname = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [{
        nixpkgs.overlays = [ cosmic-kdeconnect.overlays.default ];
        environment.systemPackages = [ pkgs.cosmic-applet-kdeconnect ];
      }];
    };
  };
}
```

### Traditional NixOS Configuration

If not using flakes, copy the files to your configuration:

```nix
{ config, pkgs, ... }:

{
  imports = [ /path/to/cosmic-applet-kdeconnect/nix/module.nix ];

  services.cosmic-kdeconnect = {
    enable = true;
    openFirewall = true;
  };
}
```

## Module Options

### Basic Options

```nix
services.cosmic-kdeconnect = {
  # Enable the service
  enable = true;

  # Open firewall ports (1714-1764 TCP/UDP)
  openFirewall = true;

  # Package to use
  package = pkgs.cosmic-applet-kdeconnect;
};
```

### Daemon Configuration

```nix
services.cosmic-kdeconnect.daemon = {
  # Enable daemon service
  enable = true;

  # Auto-start on login
  autoStart = true;

  # Logging level: "error" | "warn" | "info" | "debug" | "trace"
  logLevel = "info";

  # Custom settings (written to config.toml)
  settings = {
    discovery = {
      broadcast_interval = 5000;
      listen_port = 1716;
    };
    security = {
      certificate_dir = "~/.config/kdeconnect/certs";
    };
  };
};
```

### Plugin Configuration

```nix
services.cosmic-kdeconnect.plugins = {
  battery = true;        # Battery monitoring
  clipboard = true;      # Clipboard sync
  notification = true;   # Notification mirroring
  share = true;          # File sharing
  mpris = true;          # Media control
  ping = true;           # Connectivity testing
};
```

### Security Options

```nix
services.cosmic-kdeconnect.security = {
  # Certificate storage directory
  certificateDirectory = "~/.config/kdeconnect/certs";

  # Trust on first pair (disable for enhanced security)
  trustOnFirstPair = true;
};
```

### Storage Options

```nix
services.cosmic-kdeconnect.storage = {
  # Where received files are stored
  downloadDirectory = "~/.local/share/kdeconnect/downloads";

  # Base data directory
  dataDirectory = "~/.local/share/kdeconnect";
};
```

## Complete Example

```nix
{ config, pkgs, ... }:

{
  imports = [ ./nix/module.nix ];

  services.cosmic-kdeconnect = {
    enable = true;
    openFirewall = true;

    daemon = {
      enable = true;
      autoStart = true;
      logLevel = "info";
    };

    plugins = {
      battery = true;
      clipboard = true;
      notification = true;
      share = true;
      mpris = true;
      ping = true;
    };

    security = {
      trustOnFirstPair = false;  # Enhanced security
    };

    storage = {
      downloadDirectory = "~/Downloads/KDEConnect";
    };
  };

  # Additional firewall configuration if needed
  networking.firewall = {
    allowedTCPPortRanges = [
      { from = 1714; to = 1764; }
    ];
    allowedUDPPortRanges = [
      { from = 1714; to = 1764; }
    ];
  };
}
```

## Building the Package

### From Flake

```bash
# Build the package
nix build .#cosmic-applet-kdeconnect

# Or just
nix build

# Install to user profile
nix profile install .#cosmic-applet-kdeconnect

# Run directly
nix run .#cosmic-applet-kdeconnect
```

### Traditional Nix

```bash
# Build package
nix-build -A cosmic-applet-kdeconnect

# Install to profile
nix-env -if nix/package.nix
```

## Running Tests

### All Tests

```bash
nix flake check
```

### Specific Tests

```bash
# Package build test
nix build .#checks.x86_64-linux.package-build

# Basic module test
nix build .#checks.x86_64-linux.module-basic

# Custom config test
nix build .#checks.x86_64-linux.module-custom-config

# Firewall test
nix build .#checks.x86_64-linux.module-no-firewall

# Two machines test
nix build .#checks.x86_64-linux.two-machines

# Plugin test
nix build .#checks.x86_64-linux.plugin-test

# Service recovery test
nix build .#checks.x86_64-linux.service-recovery

# Security test
nix build .#checks.x86_64-linux.security-test
```

### Interactive Test Debugging

```bash
# Run a test interactively
nix build .#checks.x86_64-linux.module-basic --keep-going -L

# Or use nixos-test-driver
nix-build nix/tests.nix -A module-basic
./result/bin/nixos-test-driver
```

## Development

### Enter Development Shell

```bash
nix develop
```

This provides:
- Rust toolchain with rust-analyzer
- All build dependencies
- Development tools (just, etc.)

### Update Dependencies

```bash
# Update flake inputs
nix flake update

# Or update specific input
nix flake lock --update-input nixpkgs
```

### Testing Module Changes

```bash
# Build a test VM with your configuration
nixos-rebuild build-vm --flake .#

# Run the VM
./result/bin/run-*-vm
```

## Package Maintenance

### Updating Version

1. Update `version` in `nix/package.nix`
2. Update `Cargo.toml` workspace version
3. Update `flake.nix` if needed
4. Run tests: `nix flake check`
5. Commit and tag: `git tag v0.2.0`

### Updating Dependencies

The package uses `cargoLock.lockFile` to pin Rust dependencies:

```bash
# Update Cargo.lock
cargo update

# Rebuild package
nix build .#cosmic-applet-kdeconnect
```

### Submitting to nixpkgs

To submit this package to nixpkgs:

1. **Fork nixpkgs**: Fork github.com/NixOS/nixpkgs
2. **Add package**: Copy `nix/package.nix` to `pkgs/by-name/co/cosmic-applet-kdeconnect/package.nix`
3. **Add module**: Copy `nix/module.nix` to `nixos/modules/services/desktop-managers/cosmic/kdeconnect.nix`
4. **Add to all-packages**: Update `nixos/modules/module-list.nix`
5. **Test**: Run `nix-build -A cosmic-applet-kdeconnect`
6. **Create PR**: Submit to nixpkgs with description

See: https://github.com/NixOS/nixpkgs/blob/master/CONTRIBUTING.md

## Troubleshooting

### Build Failures

```bash
# Check build logs
nix log .#cosmic-applet-kdeconnect

# Build with verbose output
nix build .#cosmic-applet-kdeconnect --print-build-logs
```

### Missing Dependencies

If build fails due to missing libraries:

1. Check `buildInputs` in `nix/package.nix`
2. Add missing dependencies
3. Test build: `nix build .#cosmic-applet-kdeconnect`

### Module Errors

```bash
# Check module options
nix eval .#nixosModules.default.options --json | jq

# Test module configuration
nixos-rebuild dry-build --flake .#
```

### Test Failures

```bash
# Run test with debugging
nix build .#checks.x86_64-linux.module-basic --show-trace

# Check test output
cat result
```

## Resources

- [NixOS Manual - Packaging](https://nixos.org/manual/nixpkgs/stable/#chap-stdenv)
- [NixOS Manual - Modules](https://nixos.org/manual/nixos/stable/#sec-writing-modules)
- [NixOS Wiki - Packaging](https://nixos.wiki/wiki/Packaging)
- [Rust in Nixpkgs](https://github.com/NixOS/nixpkgs/blob/master/doc/languages-frameworks/rust.section.md)

## License

GPL-3.0-or-later - Same as the main project
