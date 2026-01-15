# cosmic-applet-kdeconnect

A native implementation of KDE Connect for COSMIC Desktop, written in Rust.

## Overview

`cosmic-applet-kdeconnect` provides seamless integration between your Android/iOS devices and COSMIC Desktop, enabling device synchronization, file sharing, notification mirroring, and remote control capabilities.

This project consists of:
- **Protocol Library**: Pure Rust implementation of the KDE Connect protocol
- **COSMIC Applet**: Panel/dock applet for quick access to connected devices
- **Full Application**: Comprehensive device management and configuration
- **Background Daemon**: Service for maintaining device connections

## Features

### Current Status: 🚧 In Development (~98% Complete)

#### Completed ✅
- [x] Core Protocol Library (v7/8)
- [x] Device State Management
- [x] TLS Certificate Generation
- [x] Packet Serialization/Deserialization
- [x] **Device Discovery** (UDP broadcast + mDNS)
- [x] **Active Pairing Flow** (request/accept/reject)
- [x] **TLS Connection Handling** (per-device connections)
- [x] **Plugin Packet Routing** (PluginManager with factories)
- [x] Plugin Architecture with 8 plugins:
  - [x] Ping Plugin (send/receive pings)
  - [x] **Battery Plugin** (status queries + **low battery alerts**)
  - [x] Notification Plugin (forwarding)
  - [x] **Share Plugin** (file/text/URL - **full TCP transfer**)
  - [x] **Clipboard Plugin** (bidirectional sync with **system integration**)
  - [x] **RunCommand Plugin** (remote shell command execution - **full implementation**)
  - [x] **FindMyPhone Plugin** (remote phone finder trigger)
  - [x] **MPRIS Plugin** (media control - **full DBus integration**)
- [x] **Background Daemon Service** (full implementation)
- [x] **DBus Interface** (complete IPC layer)
- [x] **COSMIC Notifications Integration** (freedesktop.org)
- [x] **Per-Device Configuration System** (JSON persistence)
- [x] **Pairing Control via DBus** (pair/unpair methods)
- [x] **TCP Payload Transfer** (bidirectional file sharing)
- [x] **File Sharing via DBus** (share_file method)
- [x] **Automatic File Reception** (downloads to ~/Downloads)
- [x] **File Transfer Notifications** (COSMIC Desktop integration)
- [x] **Clipboard System Integration** (automatic sync with system clipboard)
- [x] **URL Opening** (automatic browser launch for shared URLs)
- [x] **Text Sharing** (automatic clipboard copy for shared text)
- [x] **Low Battery Notifications** (alerts for connected devices)
- [x] **Pairing Notifications** (timeout and error feedback)
- [x] **COSMIC Panel Applet** (fully functional with daemon integration):
  - [x] Real device data from daemon via D-Bus
  - [x] Device list with connection/pairing status
  - [x] Battery level indicators with charging status
  - [x] Quick action buttons (ping, send file, find phone)
  - [x] Pair/unpair device operations
  - [x] Automatic device list refresh
  - [x] Device type icons (phone, tablet, desktop, laptop, TV)
  - [x] File picker integration (XDG Desktop Portal)
  - [x] MPRIS media controls (player discovery, playback control, volume, seek)
- [x] Comprehensive Test Suite (114 tests, 12 integration tests)
- [x] CI/CD Pipeline with GitHub Actions
- [x] Pre-commit hooks for code quality
- [x] Error handling and diagnostics infrastructure

#### In Progress 🔨
- [ ] **Transfer progress tracking** (progress bars, cancellation)

#### Planned 📋
- [ ] Real device testing (requires Android/iOS device)
- [ ] Advanced file transfer features (multiple files, drag & drop)
- [ ] Remote Input
- [ ] SMS Messaging
- [ ] Bluetooth Transport

### Implemented Features

- ✅ **File Sharing** - Bidirectional file transfers with TCP payload streaming
  - Send files via DBus: `ShareFile` method
  - Automatic file reception to ~/Downloads
  - 64KB streaming buffer for efficiency
  - COSMIC Desktop notifications for received files
  - Port range: 1739-1764 (KDE Connect standard)
  - Compatible with Android/iOS KDE Connect apps
- ✅ **URL Sharing** - Share links between devices
  - Automatically opens received URLs in default browser
  - Uses xdg-open for cross-desktop compatibility
  - Non-blocking background processing
- ✅ **Text Sharing** - Share text snippets between devices
  - Automatically copies received text to system clipboard
  - Instant availability for pasting
  - Works with clipboard sync for seamless experience
- ✅ **Clipboard Sync** - Automatic bidirectional clipboard synchronization
  - Monitors local clipboard changes (500ms polling)
  - Automatically syncs to all connected devices
  - Applies remote clipboard changes to system clipboard
  - Loop prevention with timestamp validation
  - Works with all text clipboard content
- ✅ **Battery Monitoring** - Track power status of connected devices
  - Receive battery level updates from devices
  - Automatic low battery notifications
  - Shows charging status
  - Threshold-based alerts (configurable on device)
- ✅ **COSMIC Notifications** - Comprehensive desktop notifications
  - Pings and messages
  - Pairing requests, timeouts, and errors
  - Device connection/disconnection
  - File transfers
  - Low battery alerts
  - Forwarded device notifications
- ✅ **Run Commands** - Execute pre-configured shell commands remotely
  - Pre-configure commands on desktop
  - Trigger from mobile device
  - Persistent command storage per device
  - Non-blocking execution
  - Security: Only pre-configured commands can be run
  - Compatible with Android/iOS KDE Connect apps
- ✅ **MPRIS Media Control** - Control media players on the local system
  - Automatic discovery of MPRIS-compatible players
  - Playback control (Play, Pause, Stop, Next, Previous)
  - Volume control (0-100%)
  - Seek position control
  - Integrated UI in COSMIC panel applet
  - Works with Spotify, VLC, Firefox, Chrome, and all MPRIS2-compatible players
- ✅ **Per-device Configuration** - Custom settings per device (nicknames, plugin overrides)
- ✅ **Plugin Management** - Enable/disable plugins globally and per-device
- ✅ **Device Pairing** - Full pairing flow with fingerprint verification
- ✅ **Connection Management** - Automatic reconnection, connection state tracking
- ✅ **Configuration Persistence** - Device registry, pairing data, preferences

### DBus API

The daemon exposes a comprehensive DBus interface at `com.system76.CosmicKdeConnect` for UI integration:

**Device Management:**
- `GetDevices() -> Vec<DeviceInfo>` - List all known devices
- `GetDevice(device_id: String) -> DeviceInfo` - Get specific device details
- `GetConnectedDevices() -> Vec<DeviceInfo>` - List connected devices only

**Pairing:**
- `RequestPairing(device_id: String)` - Initiate pairing with a device
- `AcceptPairing(device_id: String)` - Accept incoming pairing request
- `RejectPairing(device_id: String)` - Reject incoming pairing request
- `UnpairDevice(device_id: String)` - Remove device pairing

**Communication:**
- `SendPing(device_id: String, message: String)` - Send ping to device
- `ShareFile(device_id: String, path: String)` - Send file to device
- `ShareText(device_id: String, text: String)` - Send text to device
- `ShareUrl(device_id: String, url: String)` - Send URL to device (opens in browser)
- `SendNotification(device_id: String, title: String, body: String)` - Send notification

**Run Commands:**
- `AddRunCommand(device_id: String, command_id: String, name: String, command: String)` - Add command
- `RemoveRunCommand(device_id: String, command_id: String)` - Remove command
- `GetRunCommands(device_id: String) -> String` - Get all commands (JSON)
- `ClearRunCommands(device_id: String)` - Clear all commands

**Configuration:**
- `GetDeviceConfig(device_id: String) -> DeviceConfig` - Get device-specific settings
- `SetDeviceNickname(device_id: String, nickname: String)` - Set custom device name
- `SetPluginEnabled(device_id: String, plugin: String, enabled: bool)` - Toggle plugin
- `ResetDeviceConfig(device_id: String)` - Reset to global defaults

**MPRIS Media Control:**
- `GetMprisPlayers() -> Vec<String>` - List available MPRIS media players
- `MprisControl(player: String, action: String)` - Control playback (Play, Pause, PlayPause, Stop, Next, Previous)
- `MprisSetVolume(player: String, volume: f64)` - Set player volume (0.0-1.0)
- `MprisSeek(player: String, offset_microseconds: i64)` - Seek position by offset in microseconds

**Signals:**
- `DeviceDiscovered(device_id: String)` - New device found on network
- `DeviceStateChanged(device_id: String, state: String)` - Connection state updated
- `PairingStatusChanged(device_id: String, status: String)` - Pairing status updated

**Example Usage:**
```bash
# List all devices
busctl call com.system76.CosmicKdeConnect /com/system76/CosmicKdeConnect com.system76.CosmicKdeConnect GetDevices

# Share a file
busctl call com.system76.CosmicKdeConnect /com/system76/CosmicKdeConnect com.system76.CosmicKdeConnect ShareFile ss "device-id" "/path/to/file.pdf"

# Send a ping
busctl call com.system76.CosmicKdeConnect /com/system76/CosmicKdeConnect com.system76.CosmicKdeConnect SendPing ss "device-id" "Hello!"

# Add a run command
busctl call com.system76.CosmicKdeConnect /com/system76/CosmicKdeConnect com.system76.CosmicKdeConnect AddRunCommand ssss "device-id" "backup" "Backup Home" "tar -czf ~/backup.tar.gz ~"

# Get all commands (returns JSON)
busctl call com.system76.CosmicKdeConnect /com/system76/CosmicKdeConnect com.system76.CosmicKdeConnect GetRunCommands s "device-id"

# Remove a command
busctl call com.system76.CosmicKdeConnect /com/system76/CosmicKdeConnect com.system76.CosmicKdeConnect RemoveRunCommand ss "device-id" "backup"

# List available media players
busctl call com.system76.CosmicKdeConnect /com/system76/CosmicKdeConnect com.system76.CosmicKdeConnect GetMprisPlayers

# Control media playback
busctl call com.system76.CosmicKdeConnect /com/system76/CosmicKdeConnect com.system76.CosmicKdeConnect MprisControl ss "org.mpris.MediaPlayer2.spotify" "PlayPause"

# Set volume (0.0-1.0)
busctl call com.system76.CosmicKdeConnect /com/system76/CosmicKdeConnect com.system76.CosmicKdeConnect MprisSetVolume sd "org.mpris.MediaPlayer2.spotify" 0.5

# Seek forward 10 seconds (10000000 microseconds)
busctl call com.system76.CosmicKdeConnect /com/system76/CosmicKdeConnect com.system76.CosmicKdeConnect MprisSeek sx "org.mpris.MediaPlayer2.spotify" 10000000
```

## Architecture

```
cosmic-applet-kdeconnect/
├── kdeconnect-protocol/          # Core protocol library
│   ├── src/
│   │   ├── lib.rs                # Public API
│   │   ├── discovery.rs          # Device discovery via UDP/mDNS
│   │   ├── pairing.rs            # TLS pairing and certificates
│   │   ├── packet.rs             # Packet serialization/deserialization
│   │   ├── device.rs             # Device state management
│   │   ├── transport/            # Network and Bluetooth transports
│   │   └── plugins/              # Plugin implementations
│   │       ├── battery.rs
│   │       ├── clipboard.rs
│   │       ├── notification.rs
│   │       ├── share.rs
│   │       └── ...
│   └── Cargo.toml
├── cosmic-applet-kdeconnect/     # COSMIC panel applet
│   ├── src/
│   │   └── main.rs               # Applet implementation
│   ├── data/
│   │   └── cosmic-applet-kdeconnect.desktop
│   └── Cargo.toml
├── cosmic-kdeconnect/            # Full desktop application
│   ├── src/
│   │   └── main.rs               # Application implementation
│   └── Cargo.toml
├── kdeconnect-daemon/            # Background service
│   ├── src/
│   │   └── main.rs               # Daemon implementation
│   └── Cargo.toml
└── Cargo.toml                     # Workspace configuration
```

## Technology Stack

- **Language**: Rust 🦀
- **GUI Framework**: [libcosmic](https://github.com/pop-os/libcosmic) (based on iced)
- **Async Runtime**: tokio
- **Network**: tokio/async-std, rustls for TLS
- **DBus**: zbus for system integration
- **Serialization**: serde + serde_json

## Prerequisites

### System Requirements

- COSMIC Desktop Environment
- Rust 1.70+ and Cargo
- Just command runner
- NixOS (recommended) or Linux with development libraries

### Required Libraries

- libxkbcommon-dev
- libwayland-dev
- libdbus-1-dev
- libssl-dev
- libfontconfig-dev
- libfreetype-dev
- pkg-config

## Development Setup

### NixOS (Recommended)

```bash
# Clone the repository
git clone https://github.com/yourusername/cosmic-applet-kdeconnect.git
cd cosmic-applet-kdeconnect

# Enter development shell
nix develop

# Build the project
just build

# Run tests
just test
```

### Other Linux Distributions

```bash
# Install Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install just
cargo install just

# Install system dependencies (Ubuntu/Debian)
sudo apt install libxkbcommon-dev libwayland-dev libdbus-1-dev \
                 libssl-dev libfontconfig-dev libfreetype-dev pkg-config

# Clone and build
git clone https://github.com/yourusername/cosmic-applet-kdeconnect.git
cd cosmic-applet-kdeconnect
just build
```

## Building

```bash
# Build all components
just build

# Build only the applet
just build-applet

# Build only the protocol library
just build-protocol

# Build with optimizations
just build-release
```

## Installation

```bash
# Install all components
sudo just install

# Install only the applet
sudo just install-applet

# For NixOS users, add to configuration.nix:
# environment.systemPackages = [ pkgs.cosmic-applet-kdeconnect ];
```

## Usage

### Setting Up

1. **Install KDE Connect on your mobile device**:
   - Android: [Google Play](https://play.google.com/store/apps/details?id=org.kde.kdeconnect_tp) or [F-Droid](https://f-droid.org/packages/org.kde.kdeconnect_tp/)
   - iOS: [App Store](https://apps.apple.com/app/kde-connect/id1580245991)

2. **Launch the applet**:
   - Add "KDE Connect" applet to your COSMIC panel via Settings → Panel → Applets

3. **Pair your device**:
   - Open KDE Connect on your mobile device
   - Click the applet icon in the panel
   - Select your device and click "Pair"
   - Accept the pairing request on your mobile device

### Firewall Configuration

KDE Connect requires ports 1714-1764 (TCP and UDP) to be open:

```bash
# For firewalld
sudo firewall-cmd --zone=public --permanent --add-port=1714-1764/tcp
sudo firewall-cmd --zone=public --permanent --add-port=1714-1764/udp
sudo firewall-cmd --reload

# For ufw
sudo ufw allow 1714:1764/tcp
sudo ufw allow 1714:1764/udp
```

### NixOS Firewall

Add to your `configuration.nix`:

```nix
networking.firewall = {
  allowedTCPPortRanges = [
    { from = 1714; to = 1764; }
  ];
  allowedUDPPortRanges = [
    { from = 1714; to = 1764; }
  ];
};
```

## Development

### Project Structure

The project uses a Cargo workspace with multiple crates:

- **kdeconnect-protocol**: Core protocol implementation (library)
- **cosmic-applet-kdeconnect**: Panel applet (binary)
- **cosmic-kdeconnect**: Full application (binary)
- **kdeconnect-daemon**: Background service (binary)

### Adding New Plugins

Plugins follow the KDE Connect plugin architecture:

```rust
// kdeconnect-protocol/src/plugins/example.rs
use crate::packet::Packet;
use async_trait::async_trait;

#[async_trait]
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    async fn handle_packet(&mut self, packet: Packet) -> Result<(), Error>;
    async fn send_packet(&self, packet: Packet) -> Result<(), Error>;
}
```

### Testing

```bash
# Run all tests
just test

# Run protocol tests only
cargo test -p kdeconnect-protocol

# Run with verbose output
just test-verbose

# Test device discovery (requires network)
just test-discovery
```

### Code Quality

```bash
# Format code
just fmt

# Lint code
just lint

# Check for security issues
just audit
```

## Contributing

Contributions are welcome! Please see:
- [CONTRIBUTING.md](CONTRIBUTING.md) - Development workflow and guidelines
- [ACCEPTANCE_CRITERIA.md](ACCEPTANCE_CRITERIA.md) - Quality standards and definition of done

All contributions must meet the acceptance criteria to ensure consistent quality.

### Development Workflow

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Set up git hooks: `just setup` or `just install-hooks`
4. Make your changes
5. Git hooks will automatically:
   - Format your code (`cargo fmt`)
   - Run linting checks (`cargo clippy`)
   - Run tests (`cargo test`)
   - Enforce commit message format
6. Commit your changes (`git commit -m 'feat(scope): add amazing feature'`)
7. Push to the branch (`git push origin feature/amazing-feature`)
8. Open a Pull Request

**Note**: Git hooks automatically check code quality. See [hooks/README.md](hooks/README.md) for details.

## Connection Stability

This implementation includes advanced connection management features for reliable device communication:

- **Rate Limiting**: 1-second minimum delay between connection attempts to prevent connection storms
- **TLS Timeout**: 5-minute idle timeout to prevent premature disconnections
- **IP-Based Duplicate Detection**: Handles Android's ephemeral port behavior correctly
- **No Keepalive Pings**: Prevents notification spam on mobile devices

**Known Issue**: Some Android clients may reconnect every ~5 seconds due to aggressive reconnection behavior. The rate limiting system mitigates this, and functionality remains stable during cycling. See [Issue #52](https://github.com/olafkfreund/cosmic-applet-kdeconnect/issues/52) for details.

## Documentation

Comprehensive documentation is available in the `/docs` directory:

- **[Architecture Documentation](docs/ARCHITECTURE.md)** - System design, component overview, and implementation details
- **[Pairing Process](docs/PAIRING_PROCESS.md)** - Complete pairing flow and protocol v8 implementation
- **[TLS Implementation Guide](docs/TLS_IMPLEMENTATION.md)** - Certificate generation, TLS setup, and Android client examples
- **[User Guide](docs/USER_GUIDE.md)** - End-user setup and usage instructions
- **[Troubleshooting](docs/TROUBLESHOOTING.md)** - Common issues and solutions
- **[Contributing Guidelines](docs/CONTRIBUTING.md)** - Development workflow and standards

## Protocol Compatibility

This implementation follows the KDE Connect protocol specification version 7/8.

**Compatible with:**
- KDE Connect Desktop (Linux, Windows, macOS)
- KDE Connect Android
- KDE Connect iOS
- GSConnect (GNOME)
- Valent (GTK)

**Protocol Documentation:**
- [KDE Connect Protocol](https://invent.kde.org/network/kdeconnect-kde)
- [Valent Protocol Reference](https://valent.andyholmes.ca/documentation/protocol.html)
- [Our Protocol Implementation](kdeconnect-protocol.md)

## Resources

- [COSMIC Desktop](https://system76.com/cosmic)
- [libcosmic Documentation](https://pop-os.github.io/libcosmic-book/)
- [KDE Connect](https://kdeconnect.kde.org/)
- [KDE Connect Android](https://invent.kde.org/network/kdeconnect-android)

## License

This project is licensed under the GNU General Public License v3.0 or later - see the [LICENSE](LICENSE) file for details.

KDE Connect is a trademark of KDE e.V.

## Acknowledgments

- **KDE Connect Team** for the original protocol and applications
- **System76** for COSMIC Desktop and libcosmic
- **GSConnect/Valent** developers for implementation insights
- All contributors to the Rust and COSMIC ecosystems

## Status & Roadmap

### Current Phase: Foundation (Q1 2026)
- [x] Project structure
- [x] Development environment setup
- [ ] Core protocol implementation
- [ ] Device discovery
- [ ] TLS pairing

### Phase 2: Basic Functionality (Q2 2026)
- [ ] Basic applet UI
- [ ] File sharing
- [ ] Notification sync
- [ ] Battery status

### Phase 3: Advanced Features (Q3 2026)
- [ ] Clipboard sync
- [ ] Media control
- [ ] Remote input
- [ ] Bluetooth support

### Phase 4: Polish & Release (Q4 2026)
- [ ] Full COSMIC integration
- [ ] Performance optimization
- [ ] Documentation
- [ ] Public release

## Support

- **Issues**: [GitHub Issues](https://github.com/yourusername/cosmic-applet-kdeconnect/issues)
- **Discussions**: [GitHub Discussions](https://github.com/yourusername/cosmic-applet-kdeconnect/discussions)
- **COSMIC Community**: [Pop!_OS Mattermost](https://chat.pop-os.org/)

## Security

Found a security vulnerability? Please email security@yourproject.org instead of opening a public issue.

---

**Note**: This project is under active development. Features and APIs may change.
