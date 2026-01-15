# KDE Connect Protocol Development Skill

## Purpose

This skill provides guidance for developing the KDE Connect protocol implementation in Rust for the cosmic-applet-kdeconnect project.

## When to Use This Skill

Use this skill when:
- Implementing protocol packet handlers
- Working with device discovery
- Implementing TLS pairing logic
- Creating plugin implementations
- Debugging protocol communication
- Testing protocol compatibility

## Protocol Fundamentals

### Packet Structure

All KDE Connect packets follow this JSON structure:

```json
{
  "id": 1736784000000,
  "type": "kdeconnect.battery",
  "body": {
    "currentCharge": 85,
    "isCharging": true,
    "thresholdEvent": 0
  }
}
```

**Field Requirements:**
- `id`: Timestamp in milliseconds (i64)
- `type`: Packet type string (must start with "kdeconnect.")
- `body`: JSON object with packet-specific data (can be empty)

### Core Packet Types

#### Identity Packet (kdeconnect.identity)

Sent during discovery and connection:

```json
{
  "id": 0,
  "type": "kdeconnect.identity",
  "body": {
    "deviceId": "unique_device_id",
    "deviceName": "Device Name",
    "deviceType": "desktop",
    "protocolVersion": 7,
    "incomingCapabilities": [
      "kdeconnect.battery",
      "kdeconnect.clipboard"
    ],
    "outgoingCapabilities": [
      "kdeconnect.battery.request",
      "kdeconnect.clipboard.connect"
    ]
  }
}
```

**Device Types:**
- `desktop`
- `laptop`
- `phone`
- `tablet`
- `tv`

#### Pairing Packet (kdeconnect.pair)

```json
{
  "id": 0,
  "type": "kdeconnect.pair",
  "body": {
    "pair": true
  }
}
```

**States:**
- `pair: true` - Request pairing or accept request
- `pair: false` - Reject or unpair

## Implementation Patterns

### Packet Serialization

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Packet {
    pub id: i64,
    #[serde(rename = "type")]
    pub packet_type: String,
    pub body: Value,
}

impl Packet {
    pub fn new(packet_type: impl Into<String>, body: Value) -> Self {
        Self {
            id: chrono::Utc::now().timestamp_millis(),
            packet_type: packet_type.into(),
            body,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut json = serde_json::to_vec(self)?;
        json.push(b'\n'); // KDE Connect protocol requires newline terminator
        Ok(json)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, Error> {
        Ok(serde_json::from_slice(data)?)
    }
}
```

### Device Discovery

**UDP Broadcast Pattern:**

```rust
use tokio::net::UdpSocket;

pub async fn broadcast_identity(identity: &Packet) -> Result<(), Error> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.set_broadcast(true)?;
    
    let data = identity.to_bytes()?;
    
    // Broadcast on all KDE Connect ports
    for port in 1714..=1764 {
        let addr = format!("255.255.255.255:{}", port);
        socket.send_to(&data, addr).await?;
    }
    
    Ok(())
}

pub async fn listen_for_devices() -> Result<DeviceReceiver, Error> {
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    
    tokio::spawn(async move {
        let socket = UdpSocket::bind("0.0.0.0:1716").await?;
        let mut buf = vec![0u8; 4096];
        
        loop {
            let (len, addr) = socket.recv_from(&mut buf).await?;
            
            if let Ok(packet) = Packet::from_bytes(&buf[..len]) {
                if packet.packet_type == "kdeconnect.identity" {
                    let _ = tx.send((packet, addr)).await;
                }
            }
        }
    });
    
    Ok(rx)
}
```

### Connection Management

**Rate Limiting** (Added 2026-01-15):

To prevent connection storms from aggressive reconnections, the Connection Manager implements rate limiting:

```rust
const MIN_CONNECTION_DELAY: Duration = Duration::from_millis(1000);

// In handle_incoming_connection:
let now = Instant::now();
let mut last_times = last_connection_time.write().await;

if let Some(&last_time) = last_times.get(id) {
    let elapsed = now.duration_since(last_time);
    if elapsed < MIN_CONNECTION_DELAY {
        info!("Rate limiting: Device {} tried to connect too soon ({}ms < {}ms)",
              id, elapsed.as_millis(), MIN_CONNECTION_DELAY.as_millis());
        drop(last_times);
        let _ = connection.close().await;
        return;
    }
}

last_times.insert(id.to_string(), now);
```

**Duplicate Connection Detection**:

The manager uses IP-based comparison (not full SocketAddr) to handle Android's ephemeral port behavior:

```rust
// Check if device is already connected (IP-based comparison)
if let Some(old_conn) = conns.get(id) {
    // Device trying to reconnect while already connected
    // Keep the existing stable connection and reject the new attempt
    info!("Device {} already connected at {} - rejecting reconnection from {}",
          id, old_conn.remote_addr, remote_addr);
    drop(conns);
    let _ = connection.close().await;
    return;
}
```

**Keepalive Strategy**:

Keepalive pings are disabled to prevent notification spam on Android:

```rust
// DISABLED: Keepalive pings trigger notifications on Android
// The phone sends its own pings to keep the connection alive
// We don't need to send our own keepalive pings
let mut keepalive_timer: Option<tokio::time::Interval> = None;
```

### TLS Connection

**TLS Timeout Configuration** (Increased 2026-01-15):

```rust
/// Default timeout for TLS operations (5 minutes for idle connections)
/// We don't use keepalive pings to avoid notification spam on Android,
/// so this timeout needs to be long enough for normal idle periods
const TLS_TIMEOUT: Duration = Duration::from_secs(300);
```

**Certificate Generation:**

```rust
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rcgen::{Certificate, CertificateParams, KeyPair};

pub fn generate_certificate() -> Result<(Vec<u8>, Vec<u8>), Error> {
    let mut params = CertificateParams::default();
    params.distinguished_name.push(
        rcgen::DnType::CommonName,
        "KDE Connect"
    );
    
    let key_pair = KeyPair::generate()?;
    let cert = Certificate::from_params(params)?;
    
    let cert_pem = cert.serialize_pem_with_signer(&cert)?;
    let key_pem = key_pair.serialize_pem();
    
    Ok((cert_pem.into_bytes(), key_pem.into_bytes()))
}
```

**TLS Connection Setup:**

```rust
use tokio_rustls::{TlsAcceptor, TlsConnector};
use rustls::ServerConfig;

pub async fn connect_tls(
    stream: TcpStream,
    cert: &[u8],
    key: &[u8],
) -> Result<TlsStream<TcpStream>, Error> {
    let certs = rustls_pemfile::certs(&mut cert.as_ref())
        .collect::<Result<Vec<_>, _>>()?;
    
    let key = rustls_pemfile::private_key(&mut key.as_ref())?
        .ok_or(Error::NoPrivateKey)?;
    
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    
    let acceptor = TlsAcceptor::from(Arc::new(config));
    Ok(acceptor.accept(stream).await?)
}
```

### Plugin Implementation

**Plugin Trait:**

```rust
use async_trait::async_trait;

#[async_trait]
pub trait Plugin: Send + Sync {
    /// Plugin identifier (e.g., "battery", "clipboard")
    fn id(&self) -> &str;
    
    /// Incoming capabilities this plugin handles
    fn incoming_capabilities(&self) -> Vec<String>;
    
    /// Outgoing capabilities this plugin provides
    fn outgoing_capabilities(&self) -> Vec<String>;
    
    /// Handle incoming packet
    async fn handle_packet(&mut self, packet: Packet) -> Result<(), Error>;
    
    /// Initialize plugin
    async fn init(&mut self) -> Result<(), Error> {
        Ok(())
    }
    
    /// Cleanup on shutdown
    async fn shutdown(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
```

**Example Plugin - Battery:**

```rust
pub struct BatteryPlugin {
    sender: PacketSender,
}

#[async_trait]
impl Plugin for BatteryPlugin {
    fn id(&self) -> &str {
        "battery"
    }
    
    fn incoming_capabilities(&self) -> Vec<String> {
        vec!["kdeconnect.battery.request".to_string()]
    }
    
    fn outgoing_capabilities(&self) -> Vec<String> {
        vec!["kdeconnect.battery".to_string()]
    }
    
    async fn handle_packet(&mut self, packet: Packet) -> Result<(), Error> {
        match packet.packet_type.as_str() {
            "kdeconnect.battery.request" => {
                self.send_battery_status().await?;
            }
            _ => {}
        }
        Ok(())
    }
    
    async fn init(&mut self) -> Result<(), Error> {
        // Start monitoring battery status
        let sender = self.sender.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Ok(status) = get_battery_status() {
                    let packet = create_battery_packet(status);
                    let _ = sender.send(packet).await;
                }
            }
        });
        Ok(())
    }
}

impl BatteryPlugin {
    async fn send_battery_status(&self) -> Result<(), Error> {
        let status = get_battery_status()?;
        let body = serde_json::json!({
            "currentCharge": status.charge,
            "isCharging": status.is_charging,
            "thresholdEvent": if status.charge < 15 { 1 } else { 0 }
        });
        
        let packet = Packet::new("kdeconnect.battery", body);
        self.sender.send(packet).await?;
        Ok(())
    }
}
```

## Common Plugin Types

### Battery Plugin

**Incoming:** `kdeconnect.battery.request`  
**Outgoing:** `kdeconnect.battery`

```json
{
  "type": "kdeconnect.battery",
  "body": {
    "currentCharge": 85,
    "isCharging": true,
    "thresholdEvent": 0
  }
}
```

### Clipboard Plugin

**Outgoing:** `kdeconnect.clipboard`, `kdeconnect.clipboard.connect`

```json
{
  "type": "kdeconnect.clipboard",
  "body": {
    "content": "clipboard text here"
  }
}
```

### Share Plugin

**Incoming/Outgoing:** `kdeconnect.share.request`, `kdeconnect.share.request.update`

```json
{
  "type": "kdeconnect.share.request",
  "body": {
    "filename": "document.pdf",
    "text": "optional text content",
    "url": "optional URL"
  }
}
```

**With Payload:**

Payload info in packet:
```json
{
  "type": "kdeconnect.share.request",
  "body": {
    "filename": "image.png"
  },
  "payloadSize": 1024000,
  "payloadTransferInfo": {
    "port": 1739
  }
}
```

### Notification Plugin

**Outgoing:** `kdeconnect.notification`, `kdeconnect.notification.request`

```json
{
  "type": "kdeconnect.notification",
  "body": {
    "id": "notification_id",
    "appName": "Application Name",
    "isClearable": true,
    "title": "Notification Title",
    "text": "Notification body text",
    "ticker": "Short description",
    "time": "1736784000000"
  }
}
```

### Ping Plugin

**Bidirectional:** `kdeconnect.ping`

```json
{
  "type": "kdeconnect.ping",
  "body": {
    "message": "optional message"
  }
}
```

### MPRIS Plugin

**Incoming:** `kdeconnect.mpris.request`  
**Outgoing:** `kdeconnect.mpris`

```json
{
  "type": "kdeconnect.mpris",
  "body": {
    "player": "player_name",
    "isPlaying": true,
    "canPause": true,
    "canPlay": true,
    "canGoNext": true,
    "canGoPrevious": true,
    "title": "Song Title",
    "artist": "Artist Name",
    "album": "Album Name",
    "length": 240000,
    "pos": 60000,
    "volume": 75
  }
}
```

## Testing Patterns

### Mock Device for Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    struct MockDevice {
        packets: Arc<Mutex<Vec<Packet>>>,
    }
    
    impl MockDevice {
        async fn send_packet(&self, packet: Packet) -> Result<(), Error> {
            self.packets.lock().await.push(packet);
            Ok(())
        }
        
        async fn receive_packet(&self) -> Option<Packet> {
            self.packets.lock().await.pop()
        }
    }
    
    #[tokio::test]
    async fn test_battery_plugin() {
        let device = MockDevice::new();
        let mut plugin = BatteryPlugin::new(device.sender());
        
        plugin.init().await.unwrap();
        
        let request = Packet::new(
            "kdeconnect.battery.request",
            serde_json::json!({})
        );
        
        plugin.handle_packet(request).await.unwrap();
        
        let response = device.receive_packet().await.unwrap();
        assert_eq!(response.packet_type, "kdeconnect.battery");
    }
}
```

## Error Handling

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("Invalid packet format: {0}")]
    InvalidPacket(String),
    
    #[error("Connection error: {0}")]
    Connection(#[from] std::io::Error),
    
    #[error("TLS error: {0}")]
    Tls(#[from] rustls::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("Device not paired")]
    NotPaired,
    
    #[error("Plugin error: {0}")]
    Plugin(String),
}
```

## Debugging Tips

### Enable Protocol Logging

```rust
// Add to each packet send/receive
tracing::debug!(
    packet_type = %packet.packet_type,
    device = %device.id(),
    "Sending packet"
);
```

### Packet Inspector

```bash
# Capture KDE Connect traffic
tcpdump -i any -A 'port >= 1714 and port <= 1764'

# Or with tshark
tshark -i any -f 'port >= 1714 and port <= 1764' -Y 'tcp or udp'
```

### Test with netcat

```bash
# Listen for discovery broadcasts
nc -lu 1716

# Send test packet
echo '{"id":0,"type":"kdeconnect.ping","body":{}}' | nc localhost 1739
```

## Performance Considerations

### Connection Pooling

```rust
pub struct ConnectionPool {
    connections: Arc<RwLock<HashMap<String, Connection>>>,
}

impl ConnectionPool {
    pub async fn get_or_create(&self, device_id: &str) -> Result<Connection, Error> {
        // Check existing
        {
            let read = self.connections.read().await;
            if let Some(conn) = read.get(device_id) {
                return Ok(conn.clone());
            }
        }
        
        // Create new
        let conn = Connection::new(device_id).await?;
        self.connections.write().await.insert(device_id.to_string(), conn.clone());
        Ok(conn)
    }
}
```

### Packet Buffering

```rust
pub struct PacketBuffer {
    buffer: VecDeque<Packet>,
    max_size: usize,
}

impl PacketBuffer {
    pub fn push(&mut self, packet: Packet) {
        if self.buffer.len() >= self.max_size {
            self.buffer.pop_front();
        }
        self.buffer.push_back(packet);
    }
}
```

## Security Guidelines

### Certificate Validation

```rust
pub fn validate_certificate(cert: &[u8], fingerprint: &str) -> Result<bool, Error> {
    use sha2::{Sha256, Digest};
    
    let mut hasher = Sha256::new();
    hasher.update(cert);
    let hash = hasher.finalize();
    
    let computed = hex::encode(hash);
    Ok(computed == fingerprint)
}
```

### Secure Storage

```rust
use keyring::Entry;

pub struct SecureStorage;

impl SecureStorage {
    pub fn store_certificate(device_id: &str, cert: &[u8]) -> Result<(), Error> {
        let entry = Entry::new("kdeconnect", device_id)?;
        entry.set_password(&base64::encode(cert))?;
        Ok(())
    }
    
    pub fn load_certificate(device_id: &str) -> Result<Vec<u8>, Error> {
        let entry = Entry::new("kdeconnect", device_id)?;
        let encoded = entry.get_password()?;
        Ok(base64::decode(encoded)?)
    }
}
```

## Best Practices

1. **Always validate packet types** before processing
2. **Use timeouts** for network operations
3. **Handle disconnections gracefully**
4. **Log protocol errors** for debugging
5. **Test with multiple devices** and platforms
6. **Follow the specification** exactly for compatibility
7. **Use async/await** throughout
8. **Implement proper error recovery**

## Common Pitfalls

- ❌ Forgetting newline terminator in packets
- ❌ Not handling TLS certificate verification
- ❌ Ignoring protocol version in identity
- ❌ Hardcoding ports instead of range
- ❌ Not implementing proper shutdown
- ❌ Blocking operations in async contexts
- ❌ Ignoring backwards compatibility

## Resources

- Protocol Specification: https://valent.andyholmes.ca/documentation/protocol.html
- KDE Connect Source: https://invent.kde.org/network/kdeconnect-kde
- GSConnect for Reference: https://github.com/GSConnect/gnome-shell-extension-gsconnect
