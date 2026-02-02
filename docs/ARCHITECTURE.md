# Architecture Documentation

> Version: 1.0.0
> Last Updated: 2026-01-15
> Project: cosmic-applet-kdeconnect

## Table of Contents

1. [System Overview](#system-overview)
2. [Component Architecture](#component-architecture)
3. [Connection Management](#connection-management)
4. [Plugin System](#plugin-system)
5. [Data Flow](#data-flow)
6. [Threading Model](#threading-model)
7. [Security Architecture](#security-architecture)
8. [Storage and Persistence](#storage-and-persistence)
9. [Inter-Process Communication](#inter-process-communication)
10. [Network Architecture](#network-architecture)

---

## System Overview

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     COSMIC Desktop                          │
│  ┌──────────────────────────────────────────────────────┐  │
│  │            COSMIC Panel (Wayland)                    │  │
│  │  ┌────────────────────────────────────────────────┐  │  │
│  │  │    cosmic-applet-kdeconnect (UI Component)     │  │  │
│  │  │  - Device list display                         │  │  │
│  │  │  - Quick actions (ping, file share, find)      │  │  │
│  │  │  - MPRIS media controls                        │  │  │
│  │  │  - Battery indicators                          │  │  │
│  │  └───────────┬────────────────────────────────────┘  │  │
│  └──────────────┼───────────────────────────────────────┘  │
│                 │ D-Bus IPC                                 │
│  ┌──────────────┼───────────────────────────────────────┐  │
│  │              ▼                                        │  │
│  │    kdeconnect-daemon (Background Service)            │  │
│  │  ┌──────────────────────────────────────────────┐   │  │
│  │  │        Connection Manager                     │   │  │
│  │  │  - Rate limiting (1s minimum delay)           │   │  │
│  │  │  - TLS connection handling (5min timeout)     │   │  │
│  │  │  - Duplicate connection detection (IP-based)  │   │  │
│  │  │  - Keepalive disabled (prevents spam)         │   │  │
│  │  └──────────────────────────────────────────────┘   │  │
│  │  ┌──────────────────────────────────────────────┐   │  │
│  │  │        Device Manager                         │   │  │
│  │  │  - Device registry & state                    │   │  │
│  │  │  - Pairing management                         │   │  │
│  │  │  - Certificate verification (SHA256)          │   │  │
│  │  └──────────────────────────────────────────────┘   │  │
│  │  ┌──────────────────────────────────────────────┐   │  │
│  │  │        Plugin Manager                         │   │  │
│  │  │  - Plugin lifecycle                           │   │  │
│  │  │  - Packet routing                             │   │  │
│  │  │  - 8 active plugins                           │   │  │
│  │  └──────────────────────────────────────────────┘   │  │
│  │  ┌──────────────────────────────────────────────┐   │  │
│  │  │        Discovery Service                      │   │  │
│  │  │  - UDP broadcast (1714-1764)                  │   │  │
│  │  │  - mDNS advertisement                         │   │  │
│  │  │  - Device announcement                        │   │  │
│  │  └──────────────────────────────────────────────┘   │  │
│  └──────────────┬────────────────────────────────────────┘  │
│                 │                                           │
└─────────────────┼───────────────────────────────────────────┘
                  │
                  │ TCP/TLS (1714-1764)
                  │ UDP Broadcast (1714-1764)
                  ▼
         ┌─────────────────┐
         │  Android/iOS    │
         │  KDE Connect    │
         │     Client      │
         └─────────────────┘
```

### Component Overview

| Component | Language | Purpose | Process Type |
|-----------|----------|---------|--------------|
| kdeconnect-protocol | Rust | Core protocol library | Library |
| kdeconnect-daemon | Rust | Background service | System daemon |
| cosmic-applet-kdeconnect | Rust | Panel UI | Applet process |
| cosmic-kdeconnect | Rust | Full application UI | Desktop app |

---

## Component Architecture

### 1. kdeconnect-protocol (Core Library)

**Location**: `kdeconnect-protocol/src/`

**Responsibilities**:
- Protocol implementation (v7/8)
- Packet serialization/deserialization
- TLS connection management
- Device discovery
- Plugin interface definitions
- Certificate handling

**Key Modules**:

```rust
kdeconnect-protocol/
├── src/
│   ├── lib.rs                      // Public API
│   ├── packet.rs                   // Packet structure & serialization
│   ├── device.rs                   // Device info & state
│   ├── error.rs                    // Error types
│   ├── connection/
│   │   ├── mod.rs                  // Connection abstraction
│   │   └── manager.rs              // Connection lifecycle & rate limiting
│   ├── transport/
│   │   ├── mod.rs
│   │   ├── tcp.rs                  // TCP transport layer
│   │   └── tls.rs                  // TLS wrapper (5min timeout)
│   ├── discovery/
│   │   ├── mod.rs
│   │   ├── udp.rs                  // UDP broadcast discovery
│   │   └── mdns.rs                 // mDNS service advertisement
│   ├── pairing/
│   │   ├── mod.rs
│   │   ├── manager.rs              // Pairing state machine
│   │   └── certificate.rs          // Certificate generation & verification
│   └── plugins/
│       ├── mod.rs                  // Plugin trait & manager
│       ├── battery.rs              // Battery status plugin
│       ├── clipboard.rs            // Clipboard sync plugin
│       ├── notification.rs         // Notification forwarding
│       ├── share.rs                // File/text/URL sharing
│       ├── ping.rs                 // Ping/pong plugin
│       ├── findmyphone.rs          // Phone finder plugin
│       ├── runcommand.rs           // Remote command execution
│       └── mpris.rs                // Media player control
```

### 2. kdeconnect-daemon (Background Service)

**Location**: `kdeconnect-daemon/src/`

**Responsibilities**:
- Maintain device connections
- Handle pairing requests
- Route packets to plugins
- Expose D-Bus API
- Send COSMIC notifications
- Persist configuration

**Architecture**:

```rust
kdeconnect-daemon/
├── src/
│   ├── main.rs                     // Entry point & tokio runtime
│   ├── daemon.rs                   // Main daemon logic
│   ├── dbus_interface.rs           // D-Bus service implementation
│   ├── config.rs                   // Configuration management
│   ├── storage.rs                  // Persistent storage
│   └── notifications.rs            // COSMIC Desktop notifications
```

**Process Lifecycle**:
1. Load configuration from `~/.config/cosmic-kdeconnect/`
2. Initialize connection manager
3. Start discovery service (UDP + mDNS)
4. Expose D-Bus interface
5. Accept incoming connections
6. Route packets to plugins
7. Handle graceful shutdown

### 3. cosmic-applet-kdeconnect (Panel Applet)

**Location**: `cosmic-applet-kdeconnect/src/`

**Responsibilities**:
- Display device list in panel
- Show battery status indicators
- Provide quick action buttons
- MPRIS media controls
- File picker integration

**UI Structure**:

```rust
cosmic-applet-kdeconnect/
├── src/
│   ├── main.rs                     // Applet entry point
│   ├── app.rs                      // Application state
│   ├── dbus_client.rs              // D-Bus client wrapper
│   ├── ui/
│   │   ├── mod.rs
│   │   ├── device_list.rs          // Device list view
│   │   ├── device_item.rs          // Individual device item
│   │   ├── mpris_controls.rs       // Media player controls
│   │   └── file_picker.rs          // File picker dialog
│   └── icons.rs                    // Device type icons
```

---

## Connection Management

### Connection Manager Architecture

**File**: `kdeconnect-protocol/src/connection/manager.rs`

The Connection Manager is the heart of the system, handling all device connections with advanced features to ensure stability.

#### Key Features

1. **Rate Limiting** (Added 2026-01-15)
   - 1-second minimum delay between connection attempts to same device
   - Prevents connection storms from aggressive reconnections
   - Tracks last connection time per device ID

2. **TLS Timeout Configuration** (Increased 2026-01-15)
   - 5-minute idle timeout (was 30 seconds)
   - Prevents premature disconnections on idle connections
   - No keepalive pings to avoid Android notification spam

3. **Duplicate Connection Detection** (IP-based)
   - Compares IP addresses only, not full SocketAddr
   - Handles Android's ephemeral source port behavior
   - Rejects new connections when device already connected

4. **Connection State Machine**

```
┌──────────────┐
│ Disconnected │
└──────┬───────┘
       │ Discovery packet received
       ▼
┌──────────────┐
│  Discovered  │
└──────┬───────┘
       │ TCP connection initiated
       ▼
┌──────────────┐
│   TLS Setup  │ (5-minute timeout)
└──────┬───────┘
       │ TLS handshake complete
       ▼
┌──────────────┐
│   Identity   │
│   Exchange   │ (Protocol v8 - post-TLS)
└──────┬───────┘
       │ Identity packets exchanged
       ▼
┌──────────────┐     ┌──────────────┐
│   Unpaired   │────│   Pairing    │
└──────┬───────┘     └──────┬───────┘
       │                    │ User accepts
       │                    ▼
       │             ┌──────────────┐
       └────────────│    Paired    │
                     └──────┬───────┘
                            │ Certificate verified
                            ▼
                     ┌──────────────┐
                     │   Connected  │
                     └──────────────┘
```

#### Data Structures

```rust
pub struct ConnectionManager {
    /// Active connections keyed by device ID
    connections: Arc<RwLock<HashMap<String, ActiveConnection>>>,

    /// Device manager for pairing state
    device_manager: Arc<RwLock<DeviceManager>>,

    /// Event channel for connection events
    event_tx: mpsc::UnboundedSender<ConnectionEvent>,

    /// Last connection time per device (rate limiting)
    last_connection_time: Arc<RwLock<HashMap<String, Instant>>>,
}

pub struct ActiveConnection {
    /// Device identifier
    pub device_id: String,

    /// Remote socket address
    pub remote_addr: SocketAddr,

    /// TLS connection stream
    pub connection: TlsConnection,

    /// Command channel for control
    pub command_tx: mpsc::UnboundedSender<ConnectionCommand>,

    /// Connection established time
    pub connected_at: Instant,
}
```

#### Rate Limiting Implementation

```rust
const MIN_CONNECTION_DELAY: Duration = Duration::from_millis(1000);

// In handle_incoming_connection:
let now = Instant::now();
let mut last_times = last_connection_time.write().await;

if let Some(&last_time) = last_times.get(id) {
    let elapsed = now.duration_since(last_time);
    if elapsed < MIN_CONNECTION_DELAY {
        info!("Rate limiting: Device {} tried to connect too soon", id);
        return; // Reject connection
    }
}

last_times.insert(id.to_string(), now);
```

#### Duplicate Connection Handling

```rust
// Check if device is already connected (IP-based comparison)
if let Some(old_conn) = conns.get(id) {
    // Same IP - reject new connection, keep existing
    info!("Device {} already connected - rejecting reconnection", id);
    let _ = connection.close().await;
    return;
}
```

**Rationale**: Android clients aggressively reconnect with new source ports. Rejecting preserves the stable existing connection and prevents cycling.

---

## Plugin System

### Plugin Architecture

**Trait Definition**: `kdeconnect-protocol/src/plugins/mod.rs`

```rust
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Plugin identifier (e.g., "battery")
    fn id(&self) -> &str;

    /// Packet types this plugin handles
    fn handles_packet(&self, packet_type: &str) -> bool;

    /// Handle incoming packet
    async fn handle_packet(&mut self, packet: Packet) -> Result<(), Error>;

    /// Initialize plugin (called on connection)
    async fn initialize(&mut self) -> Result<(), Error>;

    /// Cleanup plugin (called on disconnection)
    async fn cleanup(&mut self) -> Result<(), Error>;
}

#[async_trait]
pub trait PluginFactory: Send + Sync {
    /// Create a new plugin instance
    async fn create(&self, context: PluginContext) -> Result<Box<dyn Plugin>, Error>;
}
```

### Plugin Manager

**File**: `kdeconnect-protocol/src/plugins/mod.rs`

```rust
pub struct PluginManager {
    /// Registered plugin factories
    factories: HashMap<String, Box<dyn PluginFactory>>,

    /// Active plugin instances per device
    active_plugins: Arc<RwLock<HashMap<String, Vec<Box<dyn Plugin>>>>>,
}

impl PluginManager {
    /// Initialize plugins for a device connection
    pub async fn initialize_plugins(
        &self,
        device_id: &str,
        context: PluginContext,
    ) -> Result<(), Error> {
        let mut plugins = Vec::new();

        for (name, factory) in &self.factories {
            match factory.create(context.clone()).await {
                Ok(plugin) => {
                    plugin.initialize().await?;
                    plugins.push(plugin);
                }
                Err(e) => error!("Failed to create plugin {}: {}", name, e),
            }
        }

        self.active_plugins.write().await.insert(
            device_id.to_string(),
            plugins
        );

        Ok(())
    }

    /// Route packet to appropriate plugin
    pub async fn route_packet(
        &self,
        device_id: &str,
        packet: Packet,
    ) -> Result<(), Error> {
        let plugins = self.active_plugins.read().await;
        let device_plugins = plugins.get(device_id)
            .ok_or(Error::DeviceNotConnected)?;

        for plugin in device_plugins {
            if plugin.handles_packet(&packet.packet_type) {
                plugin.handle_packet(packet.clone()).await?;
            }
        }

        Ok(())
    }
}
```

### Implemented Plugins

| Plugin | ID | Incoming Packets | Outgoing Packets | Purpose |
|--------|-------|------------------|------------------|---------|
| Ping | ping | kdeconnect.ping | kdeconnect.ping | Send/receive pings |
| Battery | battery | kdeconnect.battery.request | kdeconnect.battery | Battery status |
| Notification | notification | - | kdeconnect.notification | Forward notifications |
| Share | share | kdeconnect.share.request | kdeconnect.share.request | File/text/URL sharing |
| Clipboard | clipboard | kdeconnect.clipboard | kdeconnect.clipboard | Clipboard sync |
| FindMyPhone | findmyphone | kdeconnect.findmyphone.request | - | Trigger phone ringer |
| RunCommand | runcommand | kdeconnect.runcommand.request | kdeconnect.runcommand | Execute commands |
| MPRIS | mpris | kdeconnect.mpris.request | kdeconnect.mpris | Media control |

---

## Data Flow

### Packet Flow (Incoming)

```
Network
  │
  │ TCP packet received
  ▼
TlsConnection
  │
  │ Decrypt & deserialize
  ▼
Connection Manager
  │
  │ Validate packet
  ▼
Plugin Manager
  │
  │ Route by packet type
  ▼
Plugin Instance
  │
  │ Process packet
  ▼
[Action]
  ├─ Update state
  ├─ Send notification
  ├─ Trigger system action
  └─ Send response packet
```

### Packet Flow (Outgoing)

```
UI/D-Bus Command
  │
  ▼
Daemon
  │
  │ Create packet
  ▼
Plugin Manager
  │
  │ Get device connection
  ▼
Connection Manager
  │
  │ Lookup active connection
  ▼
TlsConnection
  │
  │ Serialize & encrypt
  ▼
Network
```

### File Transfer Flow

```
Sender                                 Receiver
  │                                       │
  │ 1. ShareFile D-Bus call               │
  │                                       │
  │ 2. Create share.request packet        │
  │    with payloadTransferInfo           │
  ├──────────────────────────────────────│
  │                                       │
  │                                       │ 3. Listen on transfer port
  │                                       │
  │ 4. Connect to transfer port           │
  │◀──────────────────────────────────────┤
  │                                       │
  │ 5. Stream file data (64KB chunks)     │
  ├──────────────────────────────────────│
  │                                       │
  │                                       │ 6. Write to ~/Downloads
  │                                       │
  │                                       │ 7. Send notification
  │                                       │
  │ 8. Send share.request.update          │
  │    (finished: true)                   │
  │◀──────────────────────────────────────┤
  │                                       │
```

---

## Threading Model

### Tokio Async Runtime

The entire system uses Tokio for asynchronous I/O and concurrency.

**Main Runtime**: `kdeconnect-daemon/src/main.rs`

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let daemon = Daemon::new().await?;
    daemon.run().await?;
    Ok(())
}
```

### Task Organization

```
Main Tokio Runtime
├── Discovery Task (UDP listening)
├── mDNS Task (service advertisement)
├── D-Bus Server Task (zbus interface)
├── Connection Accept Task (TCP listener)
├── Per-Device Connection Tasks
│   ├── Packet Read Loop
│   ├── Packet Write Loop
│   └── Plugin Tasks (per plugin)
└── Notification Task (COSMIC Desktop)
```

### Synchronization Primitives

| Type | Usage | Example |
|------|-------|---------|
| `Arc<RwLock<T>>` | Shared mutable state | Device registry, connections |
| `mpsc::UnboundedSender` | Event channels | Connection events, commands |
| `oneshot::channel` | Single response | Pairing approval |
| `Mutex<T>` | Short-lived locks | Counter updates |

---

## Security Architecture

### TLS Implementation

**File**: `kdeconnect-protocol/src/transport/tls.rs`

#### Certificate Generation

- **Algorithm**: RSA 2048-bit
- **Validity**: 10 years
- **Subject**: CN=<device_id>
- **Self-signed**: Each device is its own CA

```rust
pub fn generate_certificate() -> Result<(CertificateDer, PrivateKeyDer), Error> {
    let mut params = CertificateParams::default();
    params.distinguished_name = DistinguishedName::new();
    params.distinguished_name.push(
        DnType::CommonName,
        device_id.as_str(),
    );

    // 10-year validity
    params.not_before = OffsetDateTime::now_utc();
    params.not_after = params.not_before + Duration::days(3650);

    // Generate RSA 2048-bit key
    let key_pair = KeyPair::generate(&rcgen::PKCS_RSA_SHA256)?;
    let cert = Certificate::from_params(params)?;

    Ok((cert, key_pair))
}
```

#### TLS Handshake (Protocol v8)

**IMPORTANT**: Protocol v8 uses TLS-first, then identity exchange:

1. Establish TCP connection
2. Perform TLS handshake (accept any certificate)
3. Exchange identity packets **over TLS connection**
4. Verify certificate fingerprint against stored pairing data

```rust
// Server side
pub async fn accept_connection(
    tcp_stream: TcpStream,
) -> Result<(TlsConnection, Packet), Error> {
    // Accept any certificate during handshake
    let config = ServerConfig::builder()
        .with_client_cert_verifier(Arc::new(AcceptAnyCertificateVerifier))
        .with_single_cert(vec![server_cert], server_key)?;

    let acceptor = TlsAcceptor::from(Arc::new(config));
    let tls_stream = acceptor.accept(tcp_stream).await?;

    // NOW exchange identity (post-TLS in v8)
    let identity = receive_identity_packet(&tls_stream).await?;
    send_identity_packet(&tls_stream, our_identity).await?;

    Ok((TlsConnection::new(tls_stream), identity))
}
```

#### Certificate Pinning

**File**: `kdeconnect-protocol/src/pairing/certificate.rs`

```rust
pub fn compute_sha256_fingerprint(cert: &CertificateDer) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cert.as_ref());
    let hash = hasher.finalize();

    hash.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(":")
}

pub fn verify_certificate(
    device_id: &str,
    cert: &CertificateDer,
    storage: &PairingStorage,
) -> Result<bool, Error> {
    let stored_fingerprint = storage.get_certificate_fingerprint(device_id)?;
    let current_fingerprint = compute_sha256_fingerprint(cert);

    if stored_fingerprint != current_fingerprint {
        error!("Certificate mismatch for device {}!", device_id);
        return Err(Error::CertificateMismatch);
    }

    Ok(true)
}
```

### Attack Mitigation

| Attack Vector | Mitigation |
|---------------|------------|
| Man-in-the-Middle | Certificate pinning with SHA256 fingerprint |
| Replay attacks | Packet ID with timestamp validation |
| Connection flooding | Rate limiting (1-second minimum delay) |
| Downgrade attacks | Protocol version check, reject < v7 |
| Certificate substitution | Stored fingerprint verification on reconnect |

---

## Storage and Persistence

### Configuration Directory

**Location**: `~/.config/cosmic-kdeconnect/`

```
~/.config/cosmic-kdeconnect/
├── config.json                    # Global configuration
├── device_registry.json           # All known devices
├── devices/
│   ├── <device-id-1>/
│   │   ├── config.json           # Device-specific config
│   │   ├── certificate.pem       # Peer certificate
│   │   ├── pairing_state.json    # Pairing status
│   │   └── plugins/
│   │       └── runcommand.json   # Plugin-specific data
│   └── <device-id-2>/
│       └── ...
├── certificates/
│   ├── device.crt                # Our certificate
│   └── device.key                # Our private key
└── logs/
    └── daemon.log                # Daemon logs
```

### Data Structures

**Global Config** (`config.json`):
```json
{
  "device_name": "My COSMIC Desktop",
  "device_id": "uuid-v4-string",
  "device_type": "desktop",
  "protocol_version": 8,
  "enabled_plugins": ["ping", "battery", "share", "clipboard", "mpris"],
  "autostart": true,
  "notification_settings": {
    "show_connection_notifications": true,
    "show_pairing_notifications": true,
    "show_file_transfer_notifications": true
  }
}
```

**Device Registry** (`device_registry.json`):
```json
{
  "devices": [
    {
      "device_id": "android-device-uuid",
      "device_name": "My Android Phone",
      "device_type": "phone",
      "last_seen": "2026-01-15T10:30:00Z",
      "is_paired": true,
      "certificate_fingerprint": "AB:CD:EF:...:12:34",
      "addresses": ["192.168.1.100"]
    }
  ]
}
```

**Per-Device Config** (`devices/<id>/config.json`):
```json
{
  "nickname": "Work Phone",
  "plugin_overrides": {
    "notification": false,
    "clipboard": true
  },
  "connection_settings": {
    "auto_connect": true,
    "preferred_address": "192.168.1.100"
  }
}
```

---

## Inter-Process Communication

### D-Bus Interface

**Service**: `com.system76.CosmicKdeConnect`
**Object Path**: `/com/system76/CosmicKdeConnect`
**Interface**: `com.system76.CosmicKdeConnect`

#### Interface Definition

**File**: `kdeconnect-daemon/src/dbus_interface.rs`

```rust
#[dbus_interface(name = "com.system76.CosmicKdeConnect")]
impl DbusInterface {
    /// Get all known devices
    async fn get_devices(&self) -> Vec<DeviceInfo>;

    /// Get specific device details
    async fn get_device(&self, device_id: String) -> Result<DeviceInfo, Error>;

    /// Request pairing with a device
    async fn request_pairing(&self, device_id: String) -> Result<(), Error>;

    /// Send ping to device
    async fn send_ping(&self, device_id: String, message: String)
        -> Result<(), Error>;

    /// Share file with device
    async fn share_file(&self, device_id: String, path: String)
        -> Result<(), Error>;

    /// Get available MPRIS players
    async fn get_mpris_players(&self) -> Vec<String>;

    /// Control MPRIS player
    async fn mpris_control(&self, player: String, action: String)
        -> Result<(), Error>;

    // Signals

    /// Emitted when a new device is discovered
    #[dbus_interface(signal)]
    async fn device_discovered(
        signal_context: &SignalContext<'_>,
        device_id: String,
    ) -> zbus::Result<()>;

    /// Emitted when device state changes
    #[dbus_interface(signal)]
    async fn device_state_changed(
        signal_context: &SignalContext<'_>,
        device_id: String,
        state: String,
    ) -> zbus::Result<()>;
}
```

#### D-Bus Communication Flow

```
cosmic-applet-kdeconnect
         │
         │ 1. Connect to session bus
         ▼
    Session D-Bus
         │
         │ 2. Call method
         │    com.system76.CosmicKdeConnect.GetDevices
         ▼
 kdeconnect-daemon
         │
         │ 3. Query device manager
         ▼
   Device Manager
         │
         │ 4. Return device list
         ▼
cosmic-applet-kdeconnect
         │
         │ 5. Update UI
         ▼
      [Display]
```

---

## Network Architecture

### Port Usage

| Port Range | Protocol | Purpose |
|------------|----------|---------|
| 1714-1764 | UDP | Device discovery broadcasts |
| 1716 | UDP | Primary discovery port |
| 1714-1764 | TCP | Device connections (TLS) |
| 1739-1764 | TCP | File transfer payload ports |

### Discovery Protocol

#### UDP Broadcast

```
Source: 0.0.0.0:random
Destination: 255.255.255.255:1714-1764
Protocol: UDP
Payload: {"id":0,"type":"kdeconnect.identity","body":{...}}
```

#### mDNS Advertisement

**Service Type**: `_kdeconnect._tcp.local`

```
Service Name: cosmic-desktop._kdeconnect._tcp.local
Port: 1716
TXT Records:
  - id=<device-id>
  - name=<device-name>
  - type=desktop
  - protocol=8
```

### Connection Establishment

```
Device A                              Device B
   │                                     │
   │ 1. UDP broadcast identity           │
   ├────────────────────────────────────│
   │                                     │
   │◀────────────────────────────────────┤
   │ 2. UDP response identity            │
   │                                     │
   │ 3. TCP connection to port 1716      │
   ├────────────────────────────────────│
   │                                     │
   │ 4. TLS handshake                    │
   │◀───────────────────────────────────│
   │    (accept any cert)                │
   │                                     │
   │ 5. Exchange identity over TLS (v8)  │
   │◀───────────────────────────────────│
   │                                     │
   │ 6. Verify certificate fingerprint   │
   │    (if paired)                      │
   │                                     │
   │ 7. Connection established           │
   │    [Bidirectional communication]    │
   │◀───────────────────────────────────│
   │                                     │
```

### Payload Transfer

File transfers use a separate TCP connection:

1. Sender announces file in share.request packet with `payloadTransferInfo.port`
2. Sender listens on specified port (1739-1764)
3. Receiver connects to that port
4. Sender streams file data (64KB chunks)
5. Receiver writes to ~/Downloads
6. Sender closes connection
7. Sender sends share.request.update with `finished: true`

---

## Performance Characteristics

### Resource Usage

| Component | Memory (Typical) | CPU (Idle) | CPU (Active) |
|-----------|------------------|------------|--------------|
| kdeconnect-daemon | ~15MB | <1% | 2-5% |
| cosmic-applet | ~8MB | <1% | 1-2% |

### Scalability

- **Max devices**: Limited by system resources (tested with 5 devices)
- **Max concurrent transfers**: 4 (limited by available payload ports)
- **Packet throughput**: ~1000 packets/second per device
- **File transfer speed**: Limited by network (typically 10-50 MB/s on LAN)

### Optimization Techniques

1. **Connection Pooling**: Reuse TLS connections
2. **Async I/O**: Non-blocking operations with Tokio
3. **Zero-Copy**: Use `Bytes` crate for packet data
4. **Buffered Streaming**: 64KB chunks for file transfers
5. **Rate Limiting**: Prevent connection storms (1-second minimum delay)

---

## Error Handling

### Error Categories

```rust
#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TLS error: {0}")]
    Tls(#[from] rustls::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Device not paired: {0}")]
    DeviceNotPaired(String),

    #[error("Certificate verification failed")]
    CertificateVerificationFailed,

    #[error("Connection timeout")]
    ConnectionTimeout,

    #[error("Protocol error: {0}")]
    Protocol(String),
}
```

### Recovery Strategies

| Error | Recovery Action |
|-------|-----------------|
| Connection timeout | Retry with exponential backoff |
| Certificate mismatch | Alert user, require re-pairing |
| Device not found | Remove from active connections |
| IO error | Log and attempt reconnection |
| Plugin error | Disable plugin, continue operation |

---

## Known Issues & Limitations

### Issue #52: Connection Cycling

**Status**: Workaround implemented with rate limiting

**Description**: Android client reconnects every ~5 seconds with new source port, causing connection cycling.

**Root Cause**: Android uses different ephemeral port for each TCP connection attempt. When we reject the new connection, Android closes both the new AND existing connections.

**Current Solution**:
- Rate limiting (1-second minimum delay)
- Reject duplicate connections to preserve existing stable connection
- Accept that cycling will occur but at reduced frequency

**Future Solution**:
- Implement socket replacement in Connection Manager (complex)
- Fix in Android client rewrite to stop aggressive reconnections

**Reference**: [GitHub Issue #52](https://github.com/olafkfreund/cosmic-applet-kdeconnect/issues/52)

---

## Future Enhancements

1. **Transfer Progress Tracking**
   - Real-time progress bars in UI
   - Cancellation support
   - Resume capability

2. **Bluetooth Transport**
   - Alternative to TCP/TLS for nearby devices
   - Lower latency for small packets

3. **Multiple Device Sync**
   - Sync clipboard across all devices simultaneously
   - Coordinated file sharing

4. **Plugin Marketplace**
   - Community-developed plugins
   - Plugin discovery and installation

---

## References

- [Pairing Process Documentation](./PAIRING_PROCESS.md)
- [TLS Implementation Guide](./TLS_IMPLEMENTATION.md)
- [KDE Connect Protocol Specification](https://community.kde.org/KDEConnect)
- [Valent Protocol Reference](https://valent.andyholmes.ca/documentation/protocol.html)
- [GitHub Issue #52 - Connection Cycling](https://github.com/olafkfreund/cosmic-applet-kdeconnect/issues/52)

---

*Last Updated: 2026-01-15 - Reflects connection management improvements, rate limiting implementation, and TLS timeout configuration changes.*
