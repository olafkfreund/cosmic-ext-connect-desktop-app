<p align="center">
  <img src="connect_logo.png" alt="COSMIC Connect Logo" width="200"/>
</p>

# COSMIC Connect

A modern, cross-platform device connectivity solution for COSMIC Desktop, written in Rust with 70%+ code sharing between desktop and mobile platforms.

## Overview

**COSMIC Connect** provides seamless integration between your Android devices and COSMIC Desktop, enabling device synchronization, file sharing, notification mirroring, clipboard sync, remote control capabilities, and advanced desktop-to-desktop collaboration features.

This project is part of a **multi-platform ecosystem**:

- **[cosmic-connect-core](https://github.com/olafkfreund/cosmic-connect-core)** - Shared Rust library (protocol, TLS, plugins)
- **[cosmic-connect-desktop-app](https://github.com/olafkfreund/cosmic-connect-desktop-app)** - This repository (COSMIC Desktop)
- **[cosmic-connect-android](https://github.com/olafkfreund/cosmic-connect-android)** - Android app with Kotlin FFI bindings

### Key Innovations

- **70%+ Code Sharing** - Unified Rust core shared between desktop and Android
- **Protocol Independence** - CConnect protocol (v7/8 compatible) with unique port 1816
- **Side-by-Side Operation** - Can run alongside KDE Connect without conflicts
- **No OpenSSL** - Modern rustls-based TLS (better cross-compilation)
- **FFI Bindings** - Kotlin/Swift support via uniffi-rs
- **Modern Async** - Tokio-based concurrent architecture
- **COSMIC Design Compliance** - Hierarchical text, theme integration, WCAG AA+ accessibility

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
                                │    └── cosmic-applet-connect
                                │
                                └──→ Android App
                                     └── Kotlin via FFI
```

### Repository Structure

```
cosmic-connect-desktop-app/
├── cosmic-connect-core/      # Shared TLS & crypto layer (git submodule)
│   └── src/                  # Core connection handling, TLS, discovery
├── cosmic-connect-protocol/  # Desktop-specific protocol extensions
│   ├── connection/           # Connection management
│   ├── device/               # Device state tracking
│   ├── discovery/            # mDNS discovery service
│   ├── pairing/              # Pairing service
│   ├── payload/              # File transfer
│   └── plugins/              # Plugin implementations (20 plugins)
├── cosmic-connect-daemon/    # Background service (systemd)
│   ├── config.rs            # Configuration management
│   ├── dbus.rs              # DBus IPC interface
│   ├── device_config.rs     # Per-device plugin settings
│   └── main.rs              # Daemon entry point
├── cosmic-applet-connect/    # COSMIC panel applet (UI)
│   ├── dbus_client.rs       # DBus communication
│   └── main.rs              # Applet UI with settings panel
└── docs/                     # Documentation
    ├── architecture/         # System design
    ├── development/          # Development guides
    └── project/              # Project management
```

Note: `cosmic-connect-core` is a git submodule. Initialize with `git submodule update --init --recursive`

## Features

### Status: Production Ready

**Version:** 0.1.0
**Protocol:** CConnect v7/8 (KDE Connect compatible)
**Discovery Port:** 1816 (conflicts avoided with KDE Connect's 1716)
**Packet Namespace:** cconnect._ (independent from kdeconnect._)
**UI Compliance:** COSMIC Design System (hierarchical text, theme system, WCAG AA+)
**Plugin Count:** 20 plugins (12 core + 8 advanced desktop features)

#### Core Features

- **Device Discovery** - UDP broadcast + mDNS service discovery
- **Secure Pairing** - TLS certificate exchange with user verification
- **Connection Management** - Automatic reconnection, socket replacement
- **Background Daemon** - Systemd service with DBus interface
- **COSMIC Panel Applet** - Rich UI with device status and quick actions
- **Per-Device Settings** - Plugin enable/disable, RemoteDesktop quality/FPS/resolution configuration

#### Plugin System (20 Plugins)

##### Core Communication Plugins

1. **Ping** - Connection testing and latency measurement
   - Packet types: `cconnect.ping`
   - Features: Echo requests with optional messages
   - Use case: Verify device connectivity

2. **Battery** - Battery status synchronization
   - Packet types: `cconnect.battery`, `cconnect.battery.request`
   - Features: Charge level, charging state, threshold events, low battery alerts
   - Desktop integration: COSMIC notifications for low battery warnings

3. **Notification** - Cross-device notification forwarding
   - Packet types: `cconnect.notification`, `cconnect.notification.request`, `cconnect.notification.action`, `cconnect.notification.reply`
   - Features: Notification mirroring, actions, inline replies, icons
   - Desktop integration: freedesktop.org notification daemon

4. **Share** - Universal file/text/URL sharing
   - Packet types: `cconnect.share.request`, `cconnect.share.request.update`
   - Features: File transfers, text sharing, URL sharing, TCP payload streaming
   - File handling: Auto-save to ~/Downloads, browser integration for URLs
   - Transfer details: 64KB buffer, progress tracking, automatic file naming

5. **Clipboard** - Bidirectional clipboard synchronization
   - Packet types: `cconnect.clipboard`, `cconnect.clipboard.connect`
   - Features: Automatic sync (500ms polling), text content support
   - Desktop integration: X11/Wayland clipboard management

##### Remote Control Plugins

6. **MPRIS** - Media player remote control
   - Packet types: `cconnect.mpris`, `cconnect.mpris.request`
   - Features: Play/pause, next/previous, volume control, seek, playlist management
   - Supported players: Spotify, VLC, Firefox, Chrome, Rhythmbox, and all MPRIS2-compliant players
   - Desktop integration: DBus MPRIS2 interface

7. **Remote Input** - Mouse and keyboard control
   - Packet types: `cconnect.remoteinput`, `cconnect.remoteinput.request`
   - Features: Mouse movement, clicks, keyboard events, scroll wheel
   - Security: Disabled by default, requires explicit user opt-in
   - Platform support: Linux X11/Wayland via uinput

8. **Run Command** - Pre-configured remote command execution
   - Packet types: `cconnect.runcommand`, `cconnect.runcommand.request`
   - Features: Command registry, execution confirmation, output capture
   - Security: Sandboxed execution, whitelist-only commands

9. **Presenter** - Presentation remote control
   - Packet types: `cconnect.presenter`
   - Features: Next/previous slide, pointer control, volume adjustment
   - Use case: Control presentations from mobile device

10. **Find My Phone** - Remote device locator
    - Packet types: `cconnect.findmyphone.request`
    - Features: Ring device with alarm sound
    - Use case: Locate misplaced phone in vicinity

##### Advanced Communication Plugins

11. **Telephony** - Call and SMS notifications
    - Packet types: `cconnect.telephony.request`, `cconnect.telephony.request_mute`, `cconnect.telephony.talking`
    - Features: Incoming call alerts, SMS notifications, call muting
    - Desktop integration: COSMIC notifications for calls/SMS

12. **Contacts** - Contact synchronization
    - Packet types: `cconnect.contacts.request_all_uids_timestamps`, `cconnect.contacts.request_vcards_by_uid`, `cconnect.contacts.response_uids_timestamps`, `cconnect.contacts.response_vcards`
    - Features: vCard sync, incremental updates via UID timestamps
    - Use case: Desktop contact management integration

13. **Chat** - Instant messaging between desktops
    - Packet types: `cconnect.chat.message`, `cconnect.chat.typing`, `cconnect.chat.read_receipt`, `cconnect.chat.history.request`, `cconnect.chat.history.response`
    - Features: Real-time messaging, typing indicators, read receipts, message history
    - Message retention: Configurable (default 30 days, max 1000 messages)
    - Use case: Desktop-to-desktop instant messaging

##### Desktop Collaboration Plugins

14. **System Monitor** - Remote resource monitoring
    - Packet types: `cconnect.systemmonitor.request`, `cconnect.systemmonitor.data`
    - Features: CPU/memory/disk/network stats, process list, real-time updates
    - Update frequency: 1-60 seconds configurable
    - Use case: Monitor remote desktop performance

15. **Wake-on-LAN** - Remote device power-on
    - Packet types: `cconnect.wol.send`, `cconnect.wol.status.request`, `cconnect.wol.status.response`
    - Features: Magic packet transmission, MAC address management, status verification
    - Use case: Wake sleeping desktops remotely

16. **Screenshot** - Remote screen capture
    - Packet types: `cconnect.screenshot.request`, `cconnect.screenshot.data`
    - Features: Full screen or window capture, configurable quality/format
    - Formats: PNG, JPEG, WebP with quality settings
    - Use case: Remote desktop viewing, troubleshooting

##### Advanced Desktop Features

17. **Remote Desktop** - VNC-style remote desktop access
    - Packet types: `cconnect.remotedesktop.start`, `cconnect.remotedesktop.stop`, `cconnect.remotedesktop.frame`, `cconnect.remotedesktop.input`
    - Features: Screen streaming, remote input, configurable quality/FPS/resolution
    - Codecs: H.264, VP8, VP9 with hardware acceleration support
    - Settings: Per-device quality (low/medium/high), FPS (15/30/60), resolution (native/custom)
    - Security: Disabled by default, requires explicit opt-in
    - Use case: Full remote desktop control

18. **Power** - Remote power management
    - Packet types: `cconnect.power.shutdown`, `cconnect.power.reboot`, `cconnect.power.suspend`, `cconnect.power.hibernate`, `cconnect.power.sleep.inhibit`, `cconnect.power.sleep.uninhibit`
    - Features: System shutdown, reboot, suspend, hibernate, sleep inhibition
    - Security: Disabled by default, systemctl integration
    - Use case: Remote system management

19. **Clipboard History** - Enhanced clipboard with persistent history
    - Packet types: `cconnect.clipboardhistory.add`, `cconnect.clipboardhistory.sync`, `cconnect.clipboardhistory.search`, `cconnect.clipboardhistory.pin`, `cconnect.clipboardhistory.delete`
    - Features: Persistent history (max 1000 items), pinned items, search, cross-device sync
    - Retention: 30 days default, automatic cleanup
    - Use case: Enhanced clipboard management with history

20. **Macro** - Automation workflow system
    - Packet types: `cconnect.macro.execute`, `cconnect.macro.list`, `cconnect.macro.status`, `cconnect.macro.cancel`
    - Features: Multi-step workflows, variable substitution, timeout protection
    - Actions: Notify, run command, wait, send file
    - Limits: 100 steps max, 5-minute timeout, 10 concurrent macros
    - Security: Disabled by default, sandboxed execution
    - Use case: Task automation across devices

##### Experimental/Future Plugins

21. **Audio Stream** - Real-time audio streaming (experimental)
    - Packet types: `cconnect.audiostream.start`, `cconnect.audiostream.stop`, `cconnect.audiostream.data`, `cconnect.audiostream.config`
    - Features: Bidirectional audio, multiple codecs (Opus, PCM, AAC)
    - Quality: Configurable sample rate, bitrate, latency
    - Status: Disabled by default, requires audio backend integration

22. **File Sync** - Automatic file synchronization (experimental)
    - Packet types: `cconnect.filesync.status`, `cconnect.filesync.request`, `cconnect.filesync.metadata`, `cconnect.filesync.conflict`
    - Features: Bidirectional sync, conflict resolution, versioning, ignore patterns
    - Conflict strategies: Last modified wins, keep both, manual resolution, size-based
    - Status: Disabled by default, requires file system integration

23. **Screen Share** - One-way presentation screen sharing (experimental)
    - Packet types: `cconnect.screenshare.start`, `cconnect.screenshare.stop`, `cconnect.screenshare.frame`, `cconnect.screenshare.cursor`, `cconnect.screenshare.annotation`
    - Features: Cursor highlighting, annotations, multiple viewers (max 10)
    - Codecs: H.264, VP8, VP9 with configurable FPS/bitrate
    - Status: Disabled by default, requires screen capture backend

24. **Mouse/Keyboard Share** - Synergy-like seamless input sharing (experimental)
    - Packet types: `cconnect.mousekeyboardshare.config`, `cconnect.mousekeyboardshare.input`, `cconnect.mousekeyboardshare.edge`, `cconnect.mousekeyboardshare.clipboard`
    - Features: Edge detection, seamless transitions, screen arrangement, clipboard sync
    - Configuration: Screen positions (left/right/top/bottom), hotkeys
    - Status: Disabled by default, requires input capture backend

#### File Sharing Features

- Bidirectional file transfers (TCP payload streaming)
- Automatic file reception to ~/Downloads
- URL sharing (auto-opens in browser)
- Text sharing (auto-copies to clipboard)
- 64KB streaming buffer for efficiency
- Discovery port: 1816 (fallback range: 1814-1864)
- Transfer port range: 1739-1764 (protocol standard)
- COSMIC Desktop notifications for transfers

#### Desktop Integration

- **COSMIC Notifications** - Full freedesktop.org integration
- **System Clipboard** - Automatic bidirectional sync
- **File Picker** - XDG Desktop Portal integration
- **MPRIS Players** - Spotify, VLC, Firefox, Chrome support
- **Per-Device Configuration** - Custom settings, nicknames, plugin overrides
- **Settings UI** - Per-device plugin enable/disable, RemoteDesktop quality/FPS/resolution

#### Quality Assurance

- **82 Unit Tests** - Comprehensive plugin-level testing
- **43 Integration Tests** - End-to-end plugin workflow validation
- **Automated Testing Suite** - All 20 plugins covered
- **Real Device Testing Tools** - Interactive testing scripts for validation
- **CI/CD Pipeline** - GitHub Actions automation
- **Pre-commit Hooks** - Code quality enforcement
- **Error Diagnostics** - Comprehensive error handling with tracing
- **NixOS Support** - Full flake.nix with dev shell

### Recently Completed

**Q1 2026**

- DBus interface type mismatch fixes for device listing
- Enhanced error handling and logging
- Improved daemon-applet communication stability

### Previous Milestones

**Q1 2025 - Major Plugin Expansion**

- Complete plugin system implementation (20 plugins total)
- Advanced desktop collaboration features:
  - System monitoring and Wake-on-LAN for remote desktop management
  - Screenshot capture and Remote Desktop with VNC-style streaming
  - Power management with system control (shutdown, reboot, suspend)
  - Clipboard History with persistent storage and search
  - Macro automation system for workflow automation
  - Chat plugin for desktop-to-desktop messaging
- Experimental features:
  - Audio streaming with multiple codec support
  - File synchronization with conflict resolution
  - Screen sharing for presentations
  - Mouse/keyboard sharing (Synergy-like functionality)
- Per-device settings UI with plugin toggles
- RemoteDesktop quality/FPS/resolution configuration per device
- DBus signal-based real-time updates
- Comprehensive documentation updates

**Q4 2024**

- COSMIC Design System compliance (hierarchical text, theme integration, accessibility)
- Port independence (1816) for side-by-side operation with KDE Connect
- Protocol namespace (cconnect.\*) establishing project identity
- Settings UI foundation with DaemonConfig data structures
- Connection stability improvements (socket replacement, IP-based detection)

### In Progress

- Phase 3 real-time plugin updates for connected devices (Issue #77)
- Android app synchronization to match port/protocol changes
- Bluetooth transport layer implementation
- iOS support using cosmic-connect-core

### Planned

- Advanced file transfer features (multiple files, drag & drop, resume)
- SMS messaging support with conversation management
- Integration with COSMIC Settings application
- Plugin marketplace/discovery system
- End-to-end encryption for sensitive plugins

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
# 1. Clone this repository with submodules
git clone --recurse-submodules https://github.com/olafkfreund/cosmic-connect-desktop-app
cd cosmic-connect-desktop-app

# 2. Enter development shell (installs all dependencies)
nix develop

# 3. Build the project
cargo build

# 4. Run the daemon (in background)
./target/debug/cosmic-connect-daemon &

# 5. Run the applet
./target/debug/cosmic-applet-connect
```

Note: If you already cloned without `--recurse-submodules`, initialize submodules with:

```bash
git submodule update --init --recursive
```

### Other Linux Distributions

```bash
# 1. Install Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Install system dependencies (see above)

# 3. Clone with submodules and build
git clone --recurse-submodules https://github.com/olafkfreund/cosmic-connect-desktop-app
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

### NixOS (Flake)

Add COSMIC Connect to your NixOS configuration using flakes:

#### 1. Add as a Flake Input

In your `flake.nix`, add cosmic-connect-desktop-app as an input:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    cosmic-connect = {
      url = "github:olafkfreund/cosmic-connect-desktop-app";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, cosmic-connect, ... }: {
    nixosConfigurations.your-hostname = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./configuration.nix
        cosmic-connect.nixosModules.default
      ];
    };
  };
}
```

#### 2. Enable the Service

In your `configuration.nix`:

```nix
{
  services.cosmic-connect = {
    enable = true;

    # Open firewall ports (required for device discovery)
    openFirewall = true;

    # Daemon configuration
    daemon = {
      enable = true;
      autoStart = true;
      logLevel = "info";  # Options: error, warn, info, debug, trace
    };

    # Enable COSMIC panel applet
    applet.enable = true;

    # Core plugin configuration (enabled by default)
    plugins = {
      # Communication
      ping = true;
      battery = true;
      notification = true;
      share = true;
      clipboard = true;
      telephony = true;
      contacts = true;
      chat = true;

      # Remote Control
      mpris = true;
      remoteinput = true;
      findmyphone = true;
      presenter = false;  # Specialized use case

      # Desktop Collaboration
      systemmonitor = true;
      wol = true;
      screenshot = true;

      # Advanced Features (security-sensitive, disabled by default)
      runcommand = false;       # Command execution
      remotedesktop = false;    # VNC-style remote desktop
      power = false;            # System power management
      macro = false;            # Automation workflows

      # Enhanced Features
      clipboardhistory = true;  # Persistent clipboard

      # Experimental (disabled by default)
      audiostream = false;      # Audio streaming
      filesync = false;         # File synchronization
      screenshare = false;      # Presentation sharing
      mousekeyboardshare = false;  # Input sharing
    };
  };
}
```

#### 3. Alternative: Overlay Method

If you prefer using overlays instead of the module:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    cosmic-connect.url = "github:olafkfreund/cosmic-connect-desktop-app";
  };

  outputs = { self, nixpkgs, cosmic-connect, ... }: {
    nixosConfigurations.your-hostname = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        {
          nixpkgs.overlays = [ cosmic-connect.overlays.default ];
          environment.systemPackages = [ pkgs.cosmic-connect ];

          # Manual firewall configuration
          networking.firewall = {
            allowedTCPPortRanges = [
              { from = 1814; to = 1864; }  # Discovery
              { from = 1739; to = 1764; }  # File transfer
            ];
            allowedUDPPortRanges = [
              { from = 1814; to = 1864; }  # Discovery
            ];
          };
        }
        ./configuration.nix
      ];
    };
  };
}
```

#### 4. Rebuild and Activate

```bash
# Rebuild NixOS configuration
sudo nixos-rebuild switch --flake .#your-hostname

# The daemon will start automatically if daemon.autoStart = true
# Otherwise, start it manually:
systemctl --user start cosmic-connect-daemon
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
   - Android: [COSMIC Connect Android](https://github.com/olafkfreund/cosmic-connect-android) (in development)
   - Note: COSMIC Connect uses the CConnect protocol (port 1816, cconnect.\* packets) and is NOT compatible with standard KDE Connect apps

2. **Configure firewall** (required for device discovery):

   ```bash
   # For NixOS (add to configuration.nix)
   networking.firewall = {
     allowedTCPPortRanges = [
       { from = 1814; to = 1864; }  # Discovery (CConnect)
       { from = 1739; to = 1764; }  # File transfer
     ];
     allowedUDPPortRanges = [{ from = 1814; to = 1864; }];  # Discovery (CConnect)
   };

   # For firewalld
   sudo firewall-cmd --zone=public --permanent --add-port=1814-1864/tcp  # Discovery
   sudo firewall-cmd --zone=public --permanent --add-port=1739-1764/tcp  # Transfer
   sudo firewall-cmd --zone=public --permanent --add-port=1814-1864/udp  # Discovery
   sudo firewall-cmd --reload

   # For ufw
   sudo ufw allow 1814:1864/tcp   # Discovery
   sudo ufw allow 1739:1764/tcp   # Transfer
   sudo ufw allow 1814:1864/udp   # Discovery
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
  - Settings - Per-device plugin configuration
- **MPRIS Controls** - Control media players (when available)
- **Settings Panel** - Per-device plugin toggles and RemoteDesktop configuration

### Per-Device Settings

Configure plugins individually for each device:

- Click the Settings button on any paired device
- Toggle plugins on/off per device
- Configure RemoteDesktop quality, FPS, and resolution
- Settings persist across daemon restarts

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

### AI-Assisted Development

This project includes a Claude Code skill with specialized agents for COSMIC Desktop development.

**Install the skill:**

```bash
./.claude/skills/install.sh
```

**Quick usage:**

```bash
# Pre-commit check
@cosmic-code-reviewer /pre-commit-check

# Architecture review
@cosmic-architect review this application structure

# Theming audit
@cosmic-theme-expert /audit-theming

# Error handling
@cosmic-error-handler /remove-unwraps
```

The skill provides 7 specialized agents covering architecture, theming, applets, widgets, error handling, performance, and comprehensive code review.

**Documentation:** `.claude/skills/cosmic-ui-design-skill/README.md`

### Testing

#### Automated Tests

```bash
# Run all tests (unit + integration)
cargo test

# Run integration tests specifically
cargo test --test plugin_integration_tests

# Run with verbose output
cargo test -- --nocapture

# Run with coverage
cargo install cargo-tarpaulin
cargo tarpaulin --out Html --output-dir target/coverage

# Run specific crate tests
cargo test -p cosmic-connect-protocol
cargo test -p cosmic-connect-daemon
```

#### Real Device Testing

```bash
# Interactive testing menu
./scripts/test-plugins.sh --interactive

# Automated tests with auto-detected device
./scripts/test-plugins.sh

# Comprehensive test suite on specific device
./scripts/test-plugins.sh --all <device_id>

# Show help
./scripts/test-plugins.sh --help
```

**Testing Documentation:**

- **[Automated Testing Guide](docs/AUTOMATED_TESTING.md)** - Integration test suite documentation
- **[Plugin Testing Guide](docs/PLUGIN_TESTING_GUIDE.md)** - Manual testing with real devices
- **[Testing Scripts](scripts/README.md)** - Testing script documentation

**Test Coverage:**

- 82 unit tests covering all 20 plugins
- 43 integration tests for end-to-end workflows
- Multi-device scenarios
- Plugin lifecycle testing
- Packet routing and capability matching

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

### Per-Device Configuration

- `GetDeviceConfig(device_id: String) -> String` - Get device plugin config (JSON)
- `SetDevicePluginEnabled(device_id: String, plugin: String, enabled: bool)` - Enable/disable plugin
- `ClearDevicePluginOverride(device_id: String, plugin: String)` - Revert to global config
- `ResetAllPluginOverrides(device_id: String)` - Clear all overrides

### RemoteDesktop Settings

- `GetRemoteDesktopSettings(device_id: String) -> String` - Get RemoteDesktop config (JSON)
- `SetRemoteDesktopSettings(device_id: String, settings: String)` - Set quality/FPS/resolution

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
- `DevicePluginStateChanged(device_id, plugin_name, enabled)` - Plugin toggled

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
- **[Automated Testing](docs/AUTOMATED_TESTING.md)** - Integration test suite guide
- **[Plugin Testing](docs/PLUGIN_TESTING_GUIDE.md)** - Manual testing with real devices
- **[Contributing Guidelines](docs/development/Contributing.md)** - How to contribute
- **[Project Status](docs/project/Status.md)** - Current implementation status
- **[User Guide](docs/USER_GUIDE.md)** - End-user setup and usage
- **[Troubleshooting](docs/TROUBLESHOOTING.md)** - Common issues and solutions
- **[Debugging](docs/DEBUGGING.md)** - Debug tools and techniques

### Development Documentation

- **[Setup Guide](docs/development/Setup.md)** - Environment setup instructions
- **[Build Fixes](docs/development/Build-Fixes.md)** - Common build issues
- **[Applet Development](docs/development/Applet-Development.md)** - COSMIC applet guide
- **[CLAUDE.md](CLAUDE.md)** - AI development guidelines

## Contributing

Contributions are welcome! Please see:

- **[CONTRIBUTING.md](CONTRIBUTING.md)** - Complete contributing guide with AI skill setup
- **[Development Guide](docs/development/Development-Guide.md)** - Development setup and workflow
- **[Architecture](docs/architecture/Architecture.md)** - System architecture understanding
- [CLAUDE.md](CLAUDE.md) - AI development guidelines

**AI-Assisted Development:** Install the Claude Code skill (`./.claude/skills/install.sh`) for specialized agents that help with COSMIC Desktop best practices.

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
- **82 Unit Tests** - All plugins thoroughly tested
- **43 Integration Tests** - All passing with comprehensive plugin coverage
- **Automated Testing Infrastructure** - Integration tests + real device testing tools
- **CI/CD Configured** with GitHub Actions
- **Production Ready** for COSMIC Desktop

Latest updates (Q1 2026):

- Fixed critical DBus interface type mismatch causing device listing failures
- Enhanced daemon-applet communication stability
- 20 plugins fully implemented and tested (8 added in 2025)
- Per-device settings UI with RemoteDesktop configuration
- Comprehensive plugin documentation in README
- All integration and unit tests passing
- Successfully resolved naming conflicts between cosmic-connect-core crates
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
- **COSMIC Community**: [Pop!\_OS Mattermost](https://chat.pop-os.org/)

## Security

Found a security vulnerability? Please email the maintainers instead of opening a public issue.

---

**Part of the COSMIC Connect multi-platform ecosystem with 70%+ code sharing**
