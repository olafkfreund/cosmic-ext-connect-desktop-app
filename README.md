<p align="center">
  <img src="connect_logo.png" alt="COSMIC Connect Logo" width="200"/>
</p>

# COSMIC Connect

A modern, cross-platform device connectivity solution for COSMIC Desktop, written in Rust with 70%+ code sharing between desktop and mobile platforms.

## Overview

**COSMIC Connect** provides seamless integration between your Android devices and COSMIC Desktop, enabling device synchronization, file sharing, notification mirroring, clipboard sync, remote control capabilities, and advanced desktop-to-desktop collaboration features.

This project is part of a **multi-platform ecosystem**:

- **[cosmic-ext-connect-core](https://github.com/olafkfreund/cosmic-ext-connect-core)** - Shared Rust library (protocol, TLS, plugins)
- **[cosmic-ext-connect-desktop-app](https://github.com/olafkfreund/cosmic-ext-connect-desktop-app)** - This repository (COSMIC Desktop)
- **[cosmic-connect-android](https://github.com/olafkfreund/cosmic-connect-android)** - Android app with Kotlin FFI bindings

### Key Innovations

- **70%+ Code Sharing** - Unified Rust core shared between desktop and Android
- **Protocol Independence** - CConnect protocol (v7/8 compatible) with unique port 1816
- **Side-by-Side Operation** - Can run alongside KDE Connect without conflicts
- **No OpenSSL** - Modern rustls-based TLS (better cross-compilation)
- **FFI Bindings** - Kotlin/Swift support via uniffi-rs
- **Modern Async** - Tokio-based concurrent architecture
- **COSMIC Design Compliance** - Hierarchical text, theme integration, WCAG AA+ accessibility
- **Trademark Compliant** - Uses `cosmic-ext-` prefix per [COSMIC Trademark Policy](https://github.com/pop-os/cosmic-epoch/blob/master/TRADEMARK.md)

## Architecture

See **[Architecture Documentation](docs/architecture/Architecture.md)** for comprehensive documentation.

```
cosmic-ext-connect-core (Shared Library)
├── Protocol v7 implementation
├── TLS/crypto layer (rustls)
├── Network & discovery
├── Plugin system
└── FFI bindings (uniffi-rs) ──┐
                                │
                                ├──→ Desktop (This Repo)
                                │    ├── cosmic-ext-connect-protocol
                                │    ├── cosmic-ext-connect-daemon
                                │    ├── cosmic-ext-applet-connect
                                │    ├── cosmic-ext-connect-manager
                                │    ├── cosmic-ext-display-stream
                                │    ├── cosmic-ext-messages
                                │    └── cosmic-ext-messages-popup
                                │
                                └──→ Android App
                                     └── Kotlin via FFI
```

## Components

This repository contains seven main components that work together to provide the full COSMIC Connect experience:

### cosmic-ext-connect-protocol

The **protocol library** implements the CConnect/KDE Connect v7/v8 protocol specification in pure Rust.

| Feature | Description |
|---------|-------------|
| **Packet Handling** | Serialization/deserialization of JSON protocol packets |
| **TLS Layer** | Certificate generation, exchange, and secure channel establishment using rustls |
| **Device Discovery** | UDP broadcast and mDNS service discovery mechanisms |
| **Plugin Framework** | Trait-based plugin system for extensible functionality |
| **Connection Management** | Socket handling, reconnection logic, and transport abstraction |

**Key Modules:**
- `connection.rs` - TCP/TLS connection management with auto-reconnect
- `discovery.rs` - UDP broadcast (port 1816) and mDNS discovery
- `pairing.rs` - Certificate exchange and verification workflow
- `plugins/` - All 30 plugin implementations

**Usage:** This crate is used internally by the daemon and manager; it's not typically used directly.

---

### cosmic-ext-connect-daemon

The **background service** that handles all device communication, running as a systemd user service.

| Feature | Description |
|---------|-------------|
| **Device Management** | Tracks paired devices, connection state, and trust levels |
| **Plugin Orchestration** | Loads and manages plugins per device based on capabilities |
| **D-Bus Interface** | Exposes `io.github.olafkfreund.CosmicExtConnect` for IPC with applet/manager |
| **Notification Forwarding** | Captures desktop notifications via D-Bus and forwards to devices |
| **Telephony Signals** | Incoming calls, missed calls, SMS received signals via D-Bus |
| **Desktop Icons** | Creates `.desktop` files for paired devices in `~/.local/share/applications/` |
| **Bluetooth Transport** | RFCOMM via BlueZ for Bluetooth device connections |

**D-Bus Methods:**
```
io.github.olafkfreund.CosmicExtConnect
├── GetDevices() → Array<Device>
├── PairDevice(device_id: String)
├── UnpairDevice(device_id: String)
├── ForgetDevice(device_id: String)
├── SendPing(device_id: String)
├── SendFile(device_id: String, path: String)
├── GetClipboard(device_id: String) → String
├── SetClipboard(device_id: String, content: String)
├── StartExtendedDisplay(device_id: String)
├── StopExtendedDisplay(device_id: String)
├── ForgetScreenShareSource()
├── GetSmsConversations(device_id: String) → Array<Conversation>
└── ... (60+ methods for all plugins)
```

**D-Bus Signals:**
```
├── DeviceAdded(device_id)
├── DeviceRemoved(device_id)
├── DeviceStateChanged(device_id, state)
├── IncomingCall(device_id, caller, phone_number)
├── MissedCall(device_id, caller, phone_number)
├── SmsReceived(device_id, sender, message)
├── ExtendedDisplayStarted(device_id)
├── ExtendedDisplayStopped(device_id)
└── ExtendedDisplayError(device_id, error)
```

**Configuration:** `~/.config/cosmic-ext-connect/config.toml`

---

### cosmic-ext-applet-connect

The **COSMIC panel applet** that provides quick access to device status and common actions.

| Feature | Description |
|---------|-------------|
| **Status Overview** | Shows connected device count with status indicators |
| **Quick Actions** | Ping, send file, clipboard sync, extended display, camera from dropdown |
| **Device Cards** | Expandable cards showing battery, connection quality, and action buttons |
| **Onboarding** | First-run wizard for daemon setup and firewall configuration |
| **Pinned Devices** | Quick access to favorite devices in collapsed view |
| **SMS Conversations** | View and reply to SMS conversations from connected Android devices |
| **Call Notifications** | Incoming and missed call notifications with caller info |
| **Context Menu** | Right-click actions including dismiss device for offline unpaired devices |

**Panel Integration:**
- Appears in COSMIC panel's system tray area
- Shows device count badge when devices are connected
- Dropdown provides device list, action buttons, and "Open Manager" button

---

### cosmic-ext-connect-manager

The **standalone window application** for comprehensive device management.

| Feature | Description |
|---------|-------------|
| **Sidebar Navigation** | Device list with search, filtering, and status indicators |
| **Device Details** | Full device info, capabilities, and connection stats |
| **Action Grid** | Context-aware actions based on device type (mobile vs desktop) |
| **File Transfers** | Progress tracking for active transfers |
| **Plugin Settings** | Per-device enable/disable toggles for each plugin |
| **Media Controls** | MPRIS remote control for music/video playback |
| **Unpair/Dismiss** | Two-step destructive confirmation for device removal |

**Device-Type Actions:**

| Action | Mobile | Desktop | Description |
|--------|--------|---------|-------------|
| Ping | Y | Y | Test connectivity |
| Send File | Y | Y | Share files via dialog |
| Clipboard | Y | Y | Sync clipboard content |
| Find Phone | Y | - | Ring device to locate |
| SMS | Y | - | View/compose text messages |
| Camera | Y | - | Use phone as webcam |
| Audio Stream | Y | - | Stream phone audio to desktop |
| Contacts | Y | - | Sync contact database |
| Screen Share | Y | Y | H.264 desktop sharing |
| Extended Display | Y | - | Use tablet as extra monitor |
| Remote Desktop | Y | Y | VNC screen sharing |
| Run Command | Y | Y | Execute remote scripts |
| Presenter | Y | - | Presentation remote control |
| Power | Y | Y | Shutdown/suspend remote |

**Launch:**
```bash
cosmic-ext-connect-manager                    # Open manager
cosmic-ext-connect-manager --select-device ID # Open with device selected
cosmic-ext-connect-manager --device-action ID ping  # Execute action directly
```

---

### cosmic-ext-display-stream

The **display streaming library** for using Android tablets as extended displays.

| Feature | Description |
|---------|-------------|
| **Screen Capture** | PipeWire-based desktop capture with portal integration |
| **H.264 Encoding** | Hardware-accelerated video encoding via GStreamer |
| **WebRTC Transport** | Low-latency streaming using WebRTC data channels |
| **Input Forwarding** | Touch/stylus input sent back to desktop (libei/reis) |
| **Multi-Monitor** | Select specific outputs or capture entire workspace |
| **Display Transforms** | Handles 90/180/270 rotation via PipeWire `SPA_META_VideoTransform` |
| **RTP Fragmentation** | FU-A fragmentation for reliable H.264 NAL unit delivery |

**Architecture:**
```
┌──────────────┐    PipeWire    ┌──────────────┐    WebRTC    ┌──────────────┐
│ COSMIC       │ ───────────  │  Encoder     │ ────────── │   Android    │
│ Desktop      │               │  (H.264)     │              │   Tablet     │
└──────────────┘               └──────────────┘              └──────────────┘
                                      ▲
                                      │ Touch events
                                      ▼
                               ┌──────────────┐
                               │  Input       │
                               │  (libei)     │
                               └──────────────┘
```

**Signaling Flow:**
1. Android sends `cconnect.extendeddisplay.request` to desktop
2. Desktop starts PipeWire capture + WebRTC signaling server on port 18080
3. Desktop sends `cconnect.extendeddisplay` with `{ "action": "ready", "address", "port" }`
4. Android connects WebSocket for SDP/ICE exchange
5. H.264 RTP stream begins, touch events return via data channel

---

### cosmic-ext-messages-popup

The **web messenger popup** for responding to messages directly from desktop notifications.

| Feature | Description |
|---------|-------------|
| **WebView Integration** | Embedded browser using wry/WebKitGTK |
| **Session Persistence** | Maintains login state per messenger service |
| **Notification Trigger** | Opens automatically when message notification received |
| **D-Bus Interface** | `io.github.olafkfreund.CosmicExtMessagesPopup` for daemon integration |
| **Multi-Service** | Supports Google Messages, WhatsApp, Telegram, Signal, Discord, Slack |

**Supported Messengers:**

| Service | Package Name | Web URL |
|---------|--------------|---------|
| Google Messages | com.google.android.apps.messaging | messages.google.com |
| WhatsApp | com.whatsapp | web.whatsapp.com |
| Telegram | org.telegram.messenger | web.telegram.org |
| Signal | org.thoughtcrime.securesms | signal.link |
| Discord | com.discord | discord.com/app |
| Slack | com.Slack | app.slack.com |

**Why Web-Based:**
- Google Messages RCS has no public API
- Maintains end-to-end encryption
- Works with any web messenger
- No reverse-engineering required

---

### cosmic-ext-messages

A **lightweight messages utility** for command-line message operations and testing.

| Feature | Description |
|---------|-------------|
| **CLI Interface** | Send/receive messages from terminal |
| **D-Bus Client** | Communicates with daemon's message queue |
| **Testing Tool** | Useful for debugging notification flow |

**Usage:**
```bash
cosmic-ext-messages list              # Show message queue
cosmic-ext-messages send DEVICE TEXT  # Send message to device
cosmic-ext-messages dismiss ID        # Dismiss notification
```

---

## Features

### Status: Production Ready

**Version:** 0.18.0
**Protocol:** CConnect v7/8 (KDE Connect compatible)
**Discovery Port:** 1816
**Plugin Count:** 30 plugins
**Test Suite:** 1,068 tests (901 unit/integration + 167 doc-tests)

#### Core Features

- **Device Discovery** - UDP broadcast + mDNS service discovery
- **Secure Pairing** - TLS certificate exchange with user verification
- **Connection Management** - Auto-reconnect, exponential backoff, socket replacement
- **Background Daemon** - Systemd service with D-Bus interface (60+ methods, 9+ signals)
- **COSMIC Panel Applet** - Modern UI with device cards, action buttons, SMS conversations view
- **Per-Device Settings** - Plugin enable/disable per device (all 30 enabled by default)
- **Bluetooth Transport** - RFCOMM via BlueZ for Bluetooth device connections
- **Adaptive Bitrate** - AIMD-based bitrate control for screen sharing streams
- **Display Transforms** - Automatic rotation handling for PipeWire video streams
- **SIGTERM Handling** - Graceful 30-second shutdown timeout for daemon

#### Implemented Plugins

| Category | Plugin | Description |
|----------|--------|-------------|
| **Communication** | Ping | Test connectivity |
| | Battery | Monitor battery level and charge state |
| | Notification | Bidirectional notification mirroring with rich content |
| | Share | File, text, and URL sharing |
| | Clipboard | Bidirectional clipboard sync |
| | Clipboard History | Persistent clipboard history with search |
| | Telephony | Incoming/missed call and SMS notifications |
| | Contacts | Contact synchronization (SQLite backend) |
| | Chat | Direct messaging between devices |
| | Connectivity Report | Network connectivity status reporting |
| **Media** | MPRIS | Media player remote control (play/pause/skip/volume) |
| | Audio Stream | Stream phone audio to desktop speakers |
| | System Volume | Remote volume control |
| | Presenter | Presentation remote control (next/prev slide) |
| **Control** | Remote Input | Mouse and keyboard control |
| | Mouse/Keyboard Share | Cross-device mouse and keyboard sharing |
| | Run Command | Execute desktop commands remotely |
| | Find My Phone | Ring remote device to locate |
| | Macro | Record and replay input sequences |
| **System** | System Monitor | Remote CPU/RAM/disk stats |
| | Lock | Remote lock/unlock |
| | Power | Shutdown/reboot/suspend |
| | Wake-on-LAN | Wake sleeping devices over network |
| | Screenshot | Capture remote screen |
| **Files** | Network Share | SFTP filesystem mounting |
| | File Sync | Automatic folder synchronization |
| **Display** | Remote Desktop | VNC screen sharing (receiver) |
| | Screen Share | H.264 screen sharing with adaptive bitrate |
| | Extended Display | Use Android tablet as wireless extended monitor |
| **Auth** | Phone Auth | Phone-based biometric authentication for desktop (D-Bus + Polkit) |

### Rich Notifications (Desktop to Android)

COSMIC Connect supports forwarding desktop notifications to connected Android devices with full rich content preservation. Notifications are captured via D-Bus using the freedesktop.org notification specification and transmitted as extended protocol packets.

#### Supported Rich Content

| Content Type | Description |
|-------------|-------------|
| **Images** | Notification images from `image-data` hint or file paths (resized to 256x256, PNG encoded) |
| **App Icons** | Application icons transmitted as base64-encoded PNG |
| **Urgency** | Three levels: Low (0), Normal (1), Critical (2) |
| **Categories** | Standard categories: `email`, `im.received`, `device`, `network`, etc. |
| **Actions** | Interactive buttons with ID/label pairs (Reply, Mark Read, etc.) |
| **HTML Body** | Rich text formatting preserved in `richBody` field |

#### Bidirectional Sync

- **Dismissal Sync**: Dismissing a notification on Android sends `isCancel: true` back to desktop
- **Action Invocation**: Tapping action buttons sends `cconnect.notification.action` packet with action ID
- **Request All**: Android can request all active notifications via `cconnect.notification.request`

### Telephony and SMS

Full telephony integration with Android devices through a D-Bus signal pipeline:

| Feature | Description |
|---------|-------------|
| **Incoming Calls** | Desktop notification with caller name and number |
| **Missed Calls** | Notification after call ends unanswered |
| **SMS Received** | Real-time SMS notifications |
| **Conversations** | Browse SMS conversation history from the applet |
| **Conversation Detail** | Read individual message threads |

The signal flow is: plugin emits internal packet -> daemon routes to D-Bus signal -> applet receives and displays notification/updates UI.

### Extended Display (Android as Monitor)

Use your Android tablet as a wireless extended display for your COSMIC Desktop:

- **PipeWire Capture** - Portal-integrated screen capture with output selection
- **H.264 Encoding** - GStreamer-based hardware-accelerated encoding
- **WebRTC Streaming** - Low-latency delivery with ICE/STUN negotiation
- **Touch Input** - Forward touch and stylus events back to desktop via libei
- **One-Click Toggle** - Start/stop from applet action buttons or context menu

### Adaptive Bitrate Control

Screen sharing automatically adjusts quality based on network conditions:

- **AIMD Algorithm** - Additive Increase / Multiplicative Decrease
- **Congestion Detection** - Throughput monitoring + broadcast lag detection
- **Per-Viewer Tracking** - Individual network reports per connected viewer
- **Auto-Recovery** - Bitrate increases after 4-second cooldown when conditions improve
- **Bounds** - 200 kbps floor, 2x target ceiling

### Camera as Webcam

Use your Android device's camera as a virtual webcam on COSMIC Desktop.

**System Requirements:**
- **v4l2loopback** kernel module installed
- Linux kernel with V4L2 support
- Connected and paired Android device with camera access granted

**Setup:**
```bash
sudo modprobe v4l2loopback exclusive_caps=1
```

**Supported Resolutions:**

| Resolution | Aspect Ratio | Use Case |
|------------|--------------|----------|
| 480p (640x480) | 4:3 | Low bandwidth, older apps |
| 720p (1280x720) | 16:9 | Standard video calls |
| 1080p (1920x1080) | 16:9 | High quality streaming |

### Desktop Device Icons

When a device is paired, COSMIC Connect generates a `.desktop` file in:
```
~/.local/share/applications/cosmic-ext-connect-<device-id>.desktop
```

These integrate with COSMIC launcher, providing device-specific actions (Send File, Ping, Find Phone) without opening the full manager.

### Phone-Based Authentication

COSMIC Connect includes a D-Bus service for phone-based biometric authentication, allowing you to authenticate desktop actions (sudo, login) using your phone's biometrics.

**D-Bus Interface:** `io.github.olafkfreund.CosmicExtPhoneAuth`
**Polkit Integration:** Four authorization levels (request, cancel, admin, configure)

See **[Phone Auth Guide](docs/cosmic-phone-auth-guide.md)** for setup instructions.

## Installation

### NixOS (Flake)

Add to your `flake.nix`:

```nix
{
  inputs.cosmic-ext-connect.url = "github:olafkfreund/cosmic-ext-connect-desktop-app";

  outputs = { self, nixpkgs, cosmic-ext-connect, ... }:
    {
      nixosConfigurations.your-hostname = nixpkgs.lib.nixosSystem {
        modules = [
          cosmic-ext-connect.nixosModules.default
          {
            services.cosmic-ext-connect.enable = true;
            services.cosmic-ext-connect.openFirewall = true;
          }
        ];
      };
    };
}
```

This installs:
- `cosmic-ext-connect-daemon` (systemd user service)
- `cosmic-ext-applet-connect` (panel applet)
- `cosmic-ext-connect-manager` (standalone manager)
- `cosmic-ext-messages-popup` (web messenger popup)
- `cosmic-ext-messages` (CLI utility)
- D-Bus service files, desktop entries, and icons

### Manual Installation

```bash
# Build release binaries
cargo build --release

# Install daemon
sudo install -Dm755 target/release/cosmic-ext-connect-daemon /usr/local/bin/
sudo install -Dm644 cosmic-ext-connect-daemon/cosmic-ext-connect-daemon.service \
  /usr/lib/systemd/user/

# Install applet
sudo install -Dm755 target/release/cosmic-ext-applet-connect /usr/local/bin/

# Install manager
sudo install -Dm755 target/release/cosmic-ext-connect-manager /usr/local/bin/

# Install D-Bus service
sudo install -Dm644 io.github.olafkfreund.CosmicExtConnect.service \
  /usr/share/dbus-1/services/

# Enable and start daemon
systemctl --user enable --now cosmic-ext-connect-daemon
```

### Firewall Configuration

COSMIC Connect uses UDP port 1816 for device discovery and dynamic TCP ports for data transfer:

```bash
# UFW
sudo ufw allow 1816/udp
sudo ufw allow 1716:1764/tcp

# firewalld
sudo firewall-cmd --permanent --add-port=1816/udp
sudo firewall-cmd --permanent --add-port=1716-1764/tcp
sudo firewall-cmd --reload
```

## D-Bus Interfaces

| Interface | Bus | Purpose |
|-----------|-----|---------|
| `io.github.olafkfreund.CosmicExtConnect` | Session | Main daemon IPC (devices, plugins, signals) |
| `io.github.olafkfreund.CosmicExtMessagesPopup` | Session | Web messenger popup control |
| `io.github.olafkfreund.CosmicExtPhoneAuth` | Session | Phone-based biometric authentication |

## Documentation

- **[User Guide](docs/USER_GUIDE.md)** - Setup and usage instructions
- **[Architecture](docs/architecture/Architecture.md)** - System design
- **[Phone Auth Guide](docs/cosmic-phone-auth-guide.md)** - Phone authentication setup
- **[Plugin Testing Guide](docs/PLUGIN_TESTING_GUIDE.md)** - Testing individual plugins
- **[Contributing](CONTRIBUTING.md)** - Development guide

## Building from Source

```bash
# Enter Nix development shell (recommended)
nix develop

# Or install dependencies manually:
# Rust 1.70+, GStreamer 1.20+, PipeWire 0.3+, BlueZ 5.60+, libei

# Build all workspace members
cargo build --workspace

# Build with extended display support
cargo build --workspace --features extendeddisplay

# Run tests
cargo test --workspace

# Run tests including feature-gated plugins
cargo test --workspace --features extendeddisplay
```

## License

GNU General Public License v3.0 or later.
