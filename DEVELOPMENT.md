# Development Setup Guide

## Prerequisites

- NixOS or Nix package manager installed
- `direnv` and `nix-direnv` installed

## Quick Start (NixOS)

### 1. Install direnv and nix-direnv

Add to your NixOS configuration:

```nix
{ pkgs, ... }:
{
  environment.systemPackages = with pkgs; [
    direnv
    nix-direnv
  ];

  programs.direnv.enable = true;
}
```

Or using Home Manager:

```nix
{ pkgs, ... }:
{
  programs.direnv = {
    enable = true;
    nix-direnv.enable = true;
  };
}
```

### 2. Enable direnv in this repository

```bash
cd /path/to/cosmic-connect-desktop-app
direnv allow
```

The development environment will automatically load when you `cd` into the directory!

### 3. Verify the environment

After `direnv allow`, you should see:

```
🚀 COSMIC Connect Development Environment
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Rust version: rustc 1.x.x
Cargo version: cargo 1.x.x

📦 Available commands:
  just build          - Build all components
  just run-applet     - Run applet in development
  just test           - Run tests
  just fmt            - Format code
  just lint           - Run clippy
  cargo check         - Fast compilation check
  cargo build         - Full build

🔧 Environment configured for COSMIC Desktop development
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

🔍 Verifying development dependencies...
  ✓ dbus-1 found: 1.x.x
  ✓ openssl found: 3.x.x

Ready to build! Try: cargo check
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

### 4. Build the project

```bash
# Fast check (no code generation)
cargo check

# Full build
cargo build

# Or using just
just build
```

## Manual Setup (without direnv)

If you prefer not to use direnv:

```bash
nix develop
```

This will drop you into the development shell with all dependencies available.

## Troubleshooting

### "direnv: error .envrc is blocked"

Run:
```bash
direnv allow
```

### "dbus-1 NOT FOUND" or "openssl NOT FOUND"

Exit and re-enter the directory to reload direnv, or manually run:
```bash
nix develop
```

### Flake changes not detected

If you modify `flake.nix`, run:
```bash
direnv reload
```

### Build still fails with missing dependencies

Ensure you're inside the development shell:
```bash
# Check if PKG_CONFIG_PATH is set
echo $PKG_CONFIG_PATH

# Verify pkg-config can find dependencies
pkg-config --exists dbus-1 && echo "dbus-1 OK"
pkg-config --exists openssl && echo "openssl OK"
```

## Development Workflow

1. Make changes to code
2. Run `cargo check` for fast feedback
3. Run `cargo test` to verify tests pass
4. Run `just lint` before committing
5. Run `just fmt` to format code

## Additional Tools

The development shell includes:

- **Rust**: Latest stable toolchain with rust-analyzer, clippy
- **just**: Command runner (see `justfile` for available commands)
- **pkg-config**: For finding system libraries
- **cmake**: For building C/C++ dependencies
- **COSMIC libraries**: libxkbcommon, wayland, etc.
- **DBus & OpenSSL**: Development headers included

## Environment Variables

The shell automatically sets:

- `RUST_BACKTRACE=1` - Show full backtraces on panic
- `RUST_LOG=debug` - Enable debug logging
- `PKG_CONFIG_PATH` - Includes all Nix package .pc files
- `LD_LIBRARY_PATH` - Runtime library paths
- `BINDGEN_EXTRA_CLANG_ARGS` - For PipeWire bindings

## CI/CD Integration

For CI systems, use:

```bash
nix build
```

Or to run tests:

```bash
nix flake check
```

## Updating Dependencies

To update the flake inputs (nixpkgs, rust-overlay, etc.):

```bash
nix flake update
```

Then commit the updated `flake.lock`.

## NixOS Module

To use COSMIC Connect as a NixOS module:

```nix
{
  inputs.cosmic-connect.url = "github:yourusername/cosmic-connect-desktop-app";

  outputs = { self, nixpkgs, cosmic-connect, ... }: {
    nixosConfigurations.yourhostname = nixpkgs.lib.nixosSystem {
      modules = [
        cosmic-connect.nixosModules.default
        {
          services.cosmic-connect = {
            enable = true;
            # Additional configuration here
          };
        }
      ];
    };
  };
}
```

## Support

For issues related to the development environment, please file an issue at:
https://github.com/yourusername/cosmic-connect-desktop-app/issues
