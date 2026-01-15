# COSMIC Connect

A modern, cross-platform device connectivity solution for COSMIC Desktop, written in Rust with 70%+ code sharing between desktop and mobile platforms.

## Overview

**COSMIC Connect** provides seamless integration between your Android devices and COSMIC Desktop, enabling device synchronization, file sharing, notification mirroring, clipboard sync, and remote control capabilities.

This project is part of a **multi-platform ecosystem**:
- **[cosmic-connect-core](https://github.com/olafkfreund/cosmic-connect-core)** - Shared Rust library (protocol, TLS, plugins)
- **[cosmic-connect-desktop-app](https://github.com/olafkfreund/cosmic-connect-desktop-app)** - This repository (COSMIC Desktop)
- **[cosmic-connect-android](https://github.com/olafkfreund/cosmic-connect-android)** - Android app with Kotlin FFI bindings

### Key Innovations

- **70%+ Code Sharing** - Unified Rust core shared between desktop and Android
- **100% Protocol Compatibility** - Full KDE Connect protocol v7/8 implementation
- **No OpenSSL** - Modern rustls-based TLS (better cross-compilation)
- **FFI Bindings** - Kotlin/Swift support via uniffi-rs
- **Modern Async** - Tokio-based concurrent architecture

## Architecture

See **[Architecture Documentation](docs/architecture/Architecture.md)** for comprehensive documentation.

```
cosmic-connect-core (Shared Library)
├── Protocol v7 implementation
├── TLS/crypto layer (rustls)
├── Network & discovery
├── Plugin system
└── FFI bindings (uniffi-rs) ──┐
                                │
                                ├──→ Desktop (This Repo)
                                │    ├── cosmic-connect-protocol
                                │    ├── cosmic-connect-daemon
                                │    ├── cosmic-applet-connect
                                │    └── cosmic-connect (CLI)
                                │
                                └──→ Android App
                                     └── Kotlin via FFI
```

### Repository Structure

```
cosmic-connect-desktop-app/
├── cosmic-connect-protocol/  # Desktop-specific protocol extensions
│   ├── connection/           # Connection management
│   ├── device/               # Device state tracking
│   ├── discovery/            # mDNS discovery service
│   ├── pairing/              # Pairing service
│   ├── payload/              # File transfer
│   └── plugins/              # Plugin implementations
├── cosmic-connect-daemon/    # Background service (systemd)
│   ├── config.rs            # Configuration management
│   ├── dbus.rs              # DBus IPC interface
│   └── main.rs              # Daemon entry point
├── cosmic-applet-connect/    # COSMIC panel applet (UI)
├── cosmic-connect/           # CLI tool
└── docs/                     # Documentation
    ├── architecture/         # System design
    ├── development/          # Development guides
    └── project/              # Project management
```

## Features

### Status: Production Ready (98% Complete)

#### Core Features

- **Device Discovery** - UDP broadcast + mDNS service discovery
- **Secure Pairing** - TLS certificate exchange with user verification
- **Connection Management** - Automatic reconnection, socket replacement
- **Background Daemon** - Systemd service with DBus interface
- **COSMIC Panel Applet** - Rich UI with device status and quick actions

#### Plugin System (12 Plugins)

- **Ping** - Connection testing
- **Battery** - Battery status sync with low battery alerts
- **Clipboard** - Bidirectional clipboard sync (500ms polling)
- **Share** - File/text/URL sharing with TCP payload transfer
- **Notification** - Notification forwarding to desktop
- **Find My Phone** - Ring device remotely
- **MPRIS** - Media player control (DBus integration)
- **Run Command** - Pre-configured remote command execution
- **Presenter** - Remote presentation control
- **Remote Input** - Mouse/keyboard control
- **Telephony** - Call/SMS notifications
- **Contacts** - Contact synchronization

#### File Sharing Features

- Bidirectional file transfers (TCP payload streaming)
- Automatic file reception to ~/Downloads
- URL sharing (auto-opens in browser)
- Text sharing (auto-copies to clipboard)
- 64KB streaming buffer for efficiency
- Port range: 1739-1764 (KDE Connect standard)
- COSMIC Desktop notifications for transfers

#### Desktop Integration

- **COSMIC Notifications** - Full freedesktop.org integration
- **System Clipboard** - Automatic bidirectional sync
- **File Picker** - XDG Desktop Portal integration
- **MPRIS Players** - Spotify, VLC, Firefox, Chrome support
- **Per-Device Configuration** - Custom settings, nicknames, plugin overrides

#### Quality Assurance

- **114 Unit Tests** + 12 Integration Tests
- **CI/CD Pipeline** - GitHub Actions automation
- **Pre-commit Hooks** - Code quality enforcement
- **Error Diagnostics** - Comprehensive error handling
- **NixOS Support** - Full flake.nix with dev shell

### In Progress

- Transfer progress tracking (progress bars, cancellation)
- iOS support using same cosmic-connect-core

### Planned

- Advanced file transfer features (multiple files, drag & drop)
- SMS messaging support
- Bluetooth transport layer

## Technology Stack

- **Language**: Rust (100%)
- **Shared Core**: [cosmic-connect-core](https://github.com/olafkfreund/cosmic-connect-core) (TLS, protocol, plugins)
- **GUI Framework**: [libcosmic](https://github.com/pop-os/libcosmic) (COSMIC native, based on iced)
- **Async Runtime**: tokio with async/await
- **TLS**: rustls (no OpenSSL dependency)
- **DBus**: zbus for IPC
- **FFI**: uniffi-rs for Kotlin/Swift bindings
- **Discovery**: mDNS service discovery (mdns-sd)
- **Serialization**: serde + serde_json

## Prerequisites

### System Requirements

- **COSMIC Desktop Environment** (recommended) or Wayland compositor
- **Rust 1.70+** and Cargo
- **Just** command runner (optional, recommended)
- **NixOS** (recommended) or Linux with development libraries

### Required Libraries

For non-NixOS systems:

```bash
# Ubuntu/Debian
sudo apt install libxkbcommon-dev libwayland-dev libdbus-1-dev \
                 pkg-config cmake

# Fedora
sudo dnf install libxkbcommon-devel wayland-devel dbus-devel \
                 pkg-config cmake

# Arch
sudo pacman -S libxkbcommon wayland dbus pkg-config cmake
```

## Quick Start

### NixOS (Recommended)

```bash
# 1. Clone cosmic-connect-core (required dependency)
cd ~/Source/GitHub/
git clone https://github.com/olafkfreund/cosmic-connect-core

# 2. Clone this repository
git clone https://github.com/olafkfreund/cosmic-connect-desktop-app
cd cosmic-connect-desktop-app

# 3. Enter development shell (installs all dependencies)
nix develop

# 4. Build the project
cargo build

# 5. Run the daemon (in background)
./target/debug/cosmic-connect-daemon &

# 6. Run the applet
./target/debug/cosmic-applet-connect
```

### Other Linux Distributions

```bash
# 1. Install Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Install system dependencies (see above)

# 3. Clone cosmic-connect-core
cd ~/Source/GitHub/
git clone https://github.com/olafkfreund/cosmic-connect-core

# 4. Clone and build
git clone https://github.com/olafkfreund/cosmic-connect-desktop-app
cd cosmic-connect-desktop-app
cargo build --release
```

## Building

```bash
# Build all components (requires Nix shell for dependencies)
nix develop
cargo build

# Build with optimizations
cargo build --release

# Build specific components
cargo build -p cosmic-connect-daemon
cargo build -p cosmic-applet-connect
cargo build -p cosmic-connect-protocol
```

## Installation

### NixOS

Add to your `configuration.nix`:

```nix
# TODO: Package will be published to nixpkgs
environment.systemPackages = [ pkgs.cosmic-connect ];
```

### Manual Installation

```bash
# Build release binaries
cargo build --release

# Install daemon
sudo install -Dm755 target/release/cosmic-connect-daemon /usr/local/bin/
sudo install -Dm644 cosmic-connect-daemon/cosmic-connect-daemon.service \
  /usr/lib/systemd/user/

# Install applet
sudo install -Dm755 target/release/cosmic-applet-connect /usr/local/bin/

# Enable and start daemon
systemctl --user enable cosmic-connect-daemon
systemctl --user start cosmic-connect-daemon
```

## Usage

### Initial Setup

1. **Install companion app on your mobile device**:
   - Android: [KDE Connect on Google Play](https://play.google.com/store/apps/details?id=org.kde.kdeconnect_tp)
   - Or use [COSMIC Connect Android](https://github.com/olafkfreund/cosmic-connect-android) (when released)

2. **Configure firewall** (required for device discovery):

   ```bash
   # For NixOS (add to configuration.nix)
   networking.firewall = {
     allowedTCPPortRanges = [{ from = 1714; to = 1764; }];
     allowedUDPPortRanges = [{ from = 1714; to = 1764; }];
   };

   # For firewalld
   sudo firewall-cmd --zone=public --permanent --add-port=1714-1764/tcp
   sudo firewall-cmd --zone=public --permanent --add-port=1714-1764/udp
   sudo firewall-cmd --reload

   # For ufw
   sudo ufw allow 1714:1764/tcp
   sudo ufw allow 1714:1764/udp
   ```

3. **Start the daemon**:

   ```bash
   systemctl --user start cosmic-connect-daemon
   ```

4. **Launch the applet**:
   - Add "COSMIC Connect" applet to your COSMIC panel via Settings → Panel → Applets
   - Or run manually: `cosmic-applet-connect`

5. **Pair your device**:
   - Open KDE Connect / COSMIC Connect on your mobile device
   - Devices should auto-discover on the same network
   - Click "Pair" in the applet or mobile app
   - Accept the pairing request on both devices

### Using the Applet

The COSMIC panel applet provides:
- **Device List** - View all discovered and paired devices
- **Battery Status** - See battery level and charging status
- **Quick Actions**:
  - Ping - Test connection
  - Send File - Share files via file picker
  - Find Phone - Ring your device remotely
  - Pair/Unpair - Manage device pairing
- **MPRIS Controls** - Control media players (when available)

### DBus API

The daemon exposes a comprehensive DBus interface at `com.system76.CosmicConnect`:

```bash
# List all devices
busctl call com.system76.CosmicConnect /com/system76/CosmicConnect \
  com.system76.CosmicConnect GetDevices

# Send a ping
busctl call com.system76.CosmicConnect /com/system76/CosmicConnect \
  com.system76.CosmicConnect SendPing ss "device-id" "Hello!"

# Share a file
busctl call com.system76.CosmicConnect /com/system76/CosmicConnect \
  com.system76.CosmicConnect ShareFile ss "device-id" "/path/to/file.pdf"

# List MPRIS players
busctl call com.system76.CosmicConnect /com/system76/CosmicConnect \
  com.system76.CosmicConnect GetMprisPlayers

# Control playback
busctl call com.system76.CosmicConnect /com/system76/CosmicConnect \
  com.system76.CosmicConnect MprisControl ss "org.mpris.MediaPlayer2.spotify" "PlayPause"
```

**Full API documentation**: See [DBus Interface](#dbus-interface-reference) section below.

## Development

### Development Setup

```bash
# Clone cosmic-connect-core (required)
git clone https://github.com/olafkfreund/cosmic-connect-core ../cosmic-connect-core

# Clone this repository
git clone https://github.com/olafkfreund/cosmic-connect-desktop-app
cd cosmic-connect-desktop-app

# Enter Nix development shell (recommended)
nix develop

# Or install dependencies manually (see Prerequisites)
```

### Testing

```bash
# Run all tests
cargo test

# Run specific crate tests
cargo test -p cosmic-connect-protocol
cargo test -p cosmic-connect-daemon

# Run with verbose output
cargo test -- --nocapture

# Run integration tests only
cargo test --test '*'
```

### Code Quality

```bash
# Format code
cargo fmt

# Lint code
cargo clippy --all-targets --all-features

# Check for security issues
cargo audit
```

### Git Hooks (Recommended)

Pre-commit hooks automatically run on every commit:

```bash
# Install hooks
cp hooks/pre-commit .git/hooks/
chmod +x .git/hooks/pre-commit
```

Hooks will automatically:
- Format code (`cargo fmt`)
- Run linting (`cargo clippy`)
- Run tests (`cargo test`)
- Enforce commit message format

### Adding New Plugins

Plugins are defined in `cosmic-connect-protocol/src/plugins/`:

```rust
use crate::{Plugin, Packet, Device, Result};
use async_trait::async_trait;

pub struct MyPlugin {
    device_id: String,
}

#[async_trait]
impl Plugin for MyPlugin {
    fn name(&self) -> &str {
        "myplugin"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec!["kdeconnect.myplugin".to_string()]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec!["kdeconnect.myplugin.request".to_string()]
    }

    async fn handle_packet(&mut self, packet: Packet) -> Result<()> {
        // Handle incoming packets
        Ok(())
    }
}
```

Register in `cosmic-connect-daemon/src/main.rs`:

```rust
plugin_manager.register_factory(Box::new(MyPluginFactory::new()));
```

## DBus Interface Reference

### Device Management

- `GetDevices() -> Vec<DeviceInfo>` - List all known devices
- `GetDevice(device_id: String) -> DeviceInfo` - Get specific device
- `GetConnectedDevices() -> Vec<DeviceInfo>` - List connected devices only

### Pairing

- `RequestPairing(device_id: String)` - Initiate pairing
- `AcceptPairing(device_id: String)` - Accept pairing request
- `RejectPairing(device_id: String)` - Reject pairing request
- `UnpairDevice(device_id: String)` - Remove device pairing

### Communication

- `SendPing(device_id: String, message: String)` - Send ping
- `ShareFile(device_id: String, path: String)` - Send file
- `ShareText(device_id: String, text: String)` - Send text
- `ShareUrl(device_id: String, url: String)` - Send URL
- `SendNotification(device_id: String, title: String, body: String)` - Send notification

### Run Commands

- `AddRunCommand(device_id, command_id, name, command)` - Add command
- `RemoveRunCommand(device_id, command_id)` - Remove command
- `GetRunCommands(device_id) -> String` - Get commands (JSON)
- `ClearRunCommands(device_id)` - Clear all commands

### MPRIS Media Control

- `GetMprisPlayers() -> Vec<String>` - List media players
- `MprisControl(player, action)` - Control playback (Play, Pause, Stop, Next, Previous)
- `MprisSetVolume(player, volume)` - Set volume (0.0-1.0)
- `MprisSeek(player, offset_microseconds)` - Seek position

### Signals

- `DeviceDiscovered(device_id)` - New device found
- `DeviceStateChanged(device_id, state)` - Connection state changed
- `PairingStatusChanged(device_id, status)` - Pairing status changed
- `PluginEvent(device_id, plugin, data)` - Plugin-specific events

## Protocol Compatibility

**Implements**: KDE Connect Protocol v7/8

**Compatible with:**
- KDE Connect Desktop (Linux, Windows, macOS)
- KDE Connect Android
- KDE Connect iOS
- GSConnect (GNOME)
- Valent (GTK)
- COSMIC Connect Android (via shared core)

**Protocol References:**
- [KDE Connect Protocol](https://invent.kde.org/network/kdeconnect-kde)
- [Valent Protocol Reference](https://valent.andyholmes.ca/documentation/protocol.html)
- [Our Architecture Documentation](docs/architecture/Architecture.md)

## Connection Stability

This implementation includes advanced connection management:

- **Socket Replacement** - Handles Android's aggressive reconnection behavior
- **Rate Limiting** - 1-second minimum delay between attempts
- **IP-Based Detection** - Handles ephemeral port changes correctly
- **5-Minute TLS Timeout** - Prevents premature disconnections
- **No Keepalive Pings** - Reduces mobile notification spam

See [Issue #52](https://github.com/olafkfreund/cosmic-connect-desktop-app/issues/52) for implementation details.

## Documentation

**[Complete Documentation](docs/Home.md)** - Full documentation index

### Quick Links

- **[Architecture](docs/architecture/Architecture.md)** - System design and multi-platform architecture
- **[Protocol Specification](docs/architecture/Protocol.md)** - KDE Connect protocol details
- **[Development Guide](docs/development/Development-Guide.md)** - Complete development documentation
- **[Contributing Guidelines](docs/development/Contributing.md)** - How to contribute
- **[Project Status](docs/project/Status.md)** - Current implementation status
- **[User Guide](docs/USER_GUIDE.md)** - End-user setup and usage
- **[Troubleshooting](docs/TROUBLESHOOTING.md)** - Common issues and solutions

### Development Documentation

- **[Setup Guide](docs/development/Setup.md)** - Environment setup instructions
- **[Build Fixes](docs/development/Build-Fixes.md)** - Common build issues
- **[Applet Development](docs/development/Applet-Development.md)** - COSMIC applet guide
- **[CLAUDE.md](CLAUDE.md)** - AI development guidelines

## Contributing

Contributions are welcome! Please see:
- **[Contributing Guidelines](docs/development/Contributing.md)** - Code style, workflow, and standards
- **[Development Guide](docs/development/Development-Guide.md)** - Development setup and workflow
- **[Architecture](docs/architecture/Architecture.md)** - System architecture understanding
- [CLAUDE.md](CLAUDE.md) - AI development guidelines

### Development Workflow

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Install git hooks: `cp hooks/pre-commit .git/hooks/`
4. Make your changes
5. Commit with conventional format: `git commit -m 'feat(scope): add amazing feature'`
6. Push to the branch: `git push origin feature/amazing-feature`
7. Open a Pull Request

**Commit Convention**: `type(scope): description`
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `refactor`: Code refactoring
- `test`: Test additions/changes
- `chore`: Build/tooling changes

## Resources

- **[COSMIC Desktop](https://system76.com/cosmic)** - Modern desktop environment
- **[libcosmic](https://pop-os.github.io/libcosmic-book/)** - COSMIC widget toolkit
- **[cosmic-connect-core](https://github.com/olafkfreund/cosmic-connect-core)** - Shared Rust library
- **[cosmic-connect-android](https://github.com/olafkfreund/cosmic-connect-android)** - Android app
- **[KDE Connect](https://kdeconnect.kde.org/)** - Original protocol and apps
- **[uniffi-rs](https://github.com/mozilla/uniffi-rs)** - FFI binding generator
- **[rustls](https://github.com/rustls/rustls)** - Modern TLS implementation

## Build Status

- **Builds Successfully** on NixOS with Nix flake
- **114 Tests Passing** + 12 integration tests
- **CI/CD Configured** with GitHub Actions
- **Production Ready** for COSMIC Desktop

Latest updates:
- Successfully resolved naming conflicts between cosmic-connect-core crates
- Reorganized documentation into structured docs/ directory
- Architecture refactoring completed
- All builds passing in Nix environment

## License

This project is licensed under the **GNU General Public License v3.0 or later** - see the [LICENSE](LICENSE) file for details.

**Trademarks:**
- KDE Connect is a trademark of KDE e.V.
- COSMIC is a trademark of System76, Inc.

## Acknowledgments

- **KDE Connect Team** - Original protocol and applications
- **System76** - COSMIC Desktop and libcosmic
- **GSConnect/Valent** - Implementation insights and protocol documentation
- **Rust Community** - Amazing ecosystem and tooling
- **Mozilla** - uniffi-rs for FFI bindings

## Support

- **Issues**: [GitHub Issues](https://github.com/olafkfreund/cosmic-connect-desktop-app/issues)
- **Discussions**: [GitHub Discussions](https://github.com/olafkfreund/cosmic-connect-desktop-app/discussions)
- **COSMIC Community**: [Pop!_OS Mattermost](https://chat.pop-os.org/)

## Security

Found a security vulnerability? Please email the maintainers instead of opening a public issue.

---

**Part of the COSMIC Connect multi-platform ecosystem with 70%+ code sharing**
