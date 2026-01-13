# KDE Connect Daemon

Background daemon service for KDE Connect protocol implementation.

## Features

- Device discovery and management
- TLS certificate management
- Plugin system with support for:
  - Ping (connectivity testing)
  - Battery status monitoring
  - Notification mirroring
  - File/text/URL sharing
  - Clipboard synchronization
  - MPRIS media control
- Configuration management
- Graceful shutdown handling

## Installation

Build and install the daemon:

```bash
cargo build --release -p kdeconnect-daemon
cargo install --path kdeconnect-daemon
```

## Configuration

The daemon creates a default configuration file at:
- Linux: `~/.config/kdeconnect/daemon.toml`

### Configuration Format

```toml
[device]
name = "My Computer"
device_type = "desktop"  # desktop, laptop, phone, tablet, tv
# device_id = "optional-custom-id"

[network]
discovery_port = 1716
transfer_port_start = 1739
transfer_port_end = 1764
discovery_interval = 5

[plugins]
enable_ping = true
enable_battery = true
enable_notification = true
enable_share = true
enable_clipboard = true
enable_mpris = true

[paths]
config_dir = "/home/user/.config/kdeconnect"
data_dir = "/home/user/.local/share/kdeconnect"
cert_dir = "/home/user/.config/kdeconnect/certs"
```

## Running

### Manual Execution

```bash
kdeconnect-daemon
```

With custom log level:

```bash
RUST_LOG=debug kdeconnect-daemon
```

### As Systemd Service

Install the service file:

```bash
mkdir -p ~/.config/systemd/user
cp kdeconnect-daemon.service ~/.config/systemd/user/
systemctl --user daemon-reload
```

Enable and start:

```bash
systemctl --user enable kdeconnect-daemon
systemctl --user start kdeconnect-daemon
```

Check status:

```bash
systemctl --user status kdeconnect-daemon
```

View logs:

```bash
journalctl --user -u kdeconnect-daemon -f
```

## Certificate Management

The daemon automatically generates a self-signed TLS certificate on first run:
- Certificate: `~/.config/kdeconnect/certs/device.crt`
- Private key: `~/.config/kdeconnect/certs/device.key`

These are used for secure device pairing and encrypted communication.

## Directories

- Configuration: `~/.config/kdeconnect/`
- Data (received files, etc.): `~/.local/share/kdeconnect/`
- Certificates: `~/.config/kdeconnect/certs/`

## Development

Run in development mode:

```bash
cargo run -p kdeconnect-daemon
```

With debug logging:

```bash
RUST_LOG=kdeconnect_daemon=debug,kdeconnect_protocol=debug cargo run -p kdeconnect-daemon
```

## Architecture

```
┌─────────────────────────────────────┐
│      KDE Connect Daemon             │
├─────────────────────────────────────┤
│  Configuration Management           │
│  ├─ Device Configuration            │
│  ├─ Network Settings                │
│  └─ Plugin Configuration            │
├─────────────────────────────────────┤
│  Certificate Management             │
│  ├─ Auto-generation                 │
│  └─ Secure Storage                  │
├─────────────────────────────────────┤
│  Plugin System                      │
│  ├─ Plugin Manager                  │
│  ├─ Plugin Registration             │
│  ├─ Lifecycle Management            │
│  └─ Packet Routing                  │
├─────────────────────────────────────┤
│  KDE Connect Protocol Library       │
│  ├─ Device Discovery                │
│  ├─ Pairing Management              │
│  ├─ Packet Handling                 │
│  └─ TLS Communication               │
└─────────────────────────────────────┘
```

## Future Enhancements

The current daemon implementation provides the foundation. Future versions will add:

- UDP device discovery broadcasting
- TCP connection handling
- TLS secure communication
- Active pairing management
- Device connection lifecycle
- Packet routing to plugins
- D-Bus integration for system integration

## License

See LICENSE file in the repository root.
