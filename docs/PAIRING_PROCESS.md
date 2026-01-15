# KDE Connect Pairing Process

## Overview

This document describes the complete pairing process between a desktop daemon and an Android client, including certificate exchange, verification, and state management. This is essential reading for implementing the Android client rewrite.

## Protocol Versions

### Protocol v7 (Legacy)
- Identity exchange over plain TCP
- TLS upgrade after identity
- Less secure against replay attacks

### Protocol v8 (Current)
- **TLS-first connection**
- Post-TLS identity exchange
- More secure, prevents identity packet interception
- Used in all modern implementations

## Pairing State Machine

```
┌─────────────┐
│   Unpaired  │
└──────┬──────┘
       │
       │ Pairing Request Sent/Received
       ▼
┌─────────────┐
│  Requested  │ ◄──────────┐
└──────┬──────┘            │
       │                   │
       │ Accept/Reject     │ Timeout (30s)
       ▼                   │
┌─────────────┐            │
│   Paired    │            │
└──────┬──────┘            │
       │                   │
       │ Unpair            │
       └───────────────────┘
```

## Complete Pairing Flow (Protocol v8)

### Phase 1: Discovery

**Desktop broadcasts identity (UDP):**
```json
{
  "id": 0,
  "type": "kdeconnect.identity",
  "body": {
    "deviceId": "desktop_uuid_here",
    "deviceName": "My Desktop",
    "deviceType": "desktop",
    "protocolVersion": 7,
    "tcpPort": 1716,
    "incomingCapabilities": [
      "kdeconnect.battery",
      "kdeconnect.ping",
      "kdeconnect.clipboard"
    ],
    "outgoingCapabilities": [
      "kdeconnect.battery.request",
      "kdeconnect.ping",
      "kdeconnect.clipboard.connect"
    ]
  }
}
```

**Android client receives broadcast:**
- Parses identity packet
- Extracts `tcpPort` (usually 1716)
- Initiates TCP connection to desktop

### Phase 2: TLS Connection (Protocol v8)

**Android client initiates TLS handshake:**

1. **TCP Connection:**
   ```
   Client connects to: desktop_ip:1716
   ```

2. **TLS ClientHello:**
   ```
   Client → Server: TLS ClientHello
   - Cipher suites
   - TLS version (1.2 or 1.3)
   - Client random
   ```

3. **Server Certificate:**
   ```
   Server → Client: ServerHello + Certificate
   - Server certificate (self-signed)
   - Server key exchange
   - Certificate request (optional)
   ```

4. **Client Certificate:**
   ```
   Client → Server: Certificate + Key Exchange
   - Client certificate (self-signed)
   - Encrypted premaster secret
   ```

5. **TLS Established:**
   ```
   Both: ChangeCipherSpec + Finished
   - TLS 1.2/1.3 established
   - Encrypted channel ready
   ```

**Implementation (Desktop - Server):**

```rust
// kdeconnect-protocol/src/transport/tls.rs
pub async fn accept_connection(
    tcp_stream: TcpStream,
    server_cert: CertificateDer<'static>,
    server_key: PrivateKeyDer<'static>,
) -> Result<(TlsConnection, Packet), Error> {
    // Create TLS acceptor with custom verifier (accept all client certs)
    let config = ServerConfig::builder()
        .with_client_cert_verifier(Arc::new(AcceptAnyCertificateVerifier))
        .with_single_cert(vec![server_cert], server_key)?;

    let acceptor = TlsAcceptor::from(Arc::new(config));
    let tls_stream = acceptor.accept(tcp_stream).await?;

    // IMPORTANT: Protocol v8 requires identity exchange AFTER TLS
    let identity_packet = receive_identity_packet(&tls_stream).await?;

    // Send our identity back
    send_identity_packet(&tls_stream, our_identity).await?;

    Ok((TlsConnection::new(tls_stream), identity_packet))
}
```

**Implementation (Android - Client):**

```java
// Pseudocode for Android client
SSLContext sslContext = SSLContext.getInstance("TLS");
sslContext.init(keyManagers, trustManagers, null);

SSLSocket socket = (SSLSocket) sslContext
    .getSocketFactory()
    .createSocket(host, port);

socket.startHandshake();

// AFTER TLS established, send identity
sendIdentityPacket(socket);

// Wait for server identity
NetworkPacket identity = receivePacket(socket);
```

### Phase 3: Post-TLS Identity Exchange

**Client sends identity over TLS:**
```json
{
  "id": 0,
  "type": "kdeconnect.identity",
  "body": {
    "deviceId": "android_uuid_here",
    "deviceName": "My Phone",
    "deviceType": "phone",
    "protocolVersion": 7,
    "incomingCapabilities": [
      "kdeconnect.battery.request",
      "kdeconnect.ping",
      "kdeconnect.notification"
    ],
    "outgoingCapabilities": [
      "kdeconnect.battery",
      "kdeconnect.ping",
      "kdeconnect.notification.request"
    ]
  }
}
```

**Server responds with its identity:**
```json
{
  "id": 0,
  "type": "kdeconnect.identity",
  "body": {
    "deviceId": "desktop_uuid_here",
    "deviceName": "My Desktop",
    "deviceType": "desktop",
    "protocolVersion": 7,
    "incomingCapabilities": [...],
    "outgoingCapabilities": [...]
  }
}
```

**Desktop verifies:**
```rust
// Extract peer certificate from TLS connection
let peer_cert = tls_stream
    .get_ref()
    .1
    .peer_certificates()
    .ok_or(Error::NoPeerCertificate)?
    .first()
    .ok_or(Error::NoPeerCertificate)?;

// Compute SHA256 fingerprint for verification
let fingerprint = compute_sha256_fingerprint(&peer_cert);

// Store device info with certificate
device_manager.add_device(Device {
    id: identity.device_id,
    name: identity.device_name,
    device_type: identity.device_type,
    certificate: peer_cert.clone(),
    certificate_fingerprint: fingerprint,
    is_paired: false, // Not yet paired
    ...
});
```

### Phase 4: Pairing Request

**User initiates pairing (from either device):**

**Desktop sends pairing request:**
```json
{
  "id": 1736784000000,
  "type": "kdeconnect.pair",
  "body": {
    "pair": true
  }
}
```

**Implementation:**
```rust
// kdeconnect-protocol/src/pairing/handler.rs
pub async fn request_pairing(&mut self) -> Result<(), Error> {
    // Create pairing request packet
    let packet = Packet::new(
        "kdeconnect.pair",
        serde_json::json!({ "pair": true })
    );

    // Send over existing TLS connection
    self.connection.send_packet(packet).await?;

    // Set state to requested
    self.state = PairingState::Requested;

    // Start 30-second timeout
    self.timeout = Some(Instant::now() + Duration::from_secs(30));

    Ok(())
}
```

**Android receives pairing request:**
```java
// Pseudocode
NetworkPacket pairPacket = receivePacket();
if (pairPacket.getType().equals("kdeconnect.pair")) {
    boolean pair = pairPacket.getBoolean("pair");
    if (pair) {
        // Show pairing dialog to user
        showPairingDialog(deviceId, deviceName, certificateFingerprint);
    }
}
```

### Phase 5: Certificate Verification

**CRITICAL: User verifies certificate fingerprint**

Both devices display the **SHA256 fingerprint** of the peer's certificate:

**Desktop UI shows:**
```
Pairing request from: My Phone
Certificate fingerprint:
AB:CD:EF:12:34:56:78:90:AB:CD:EF:12:34:56:78:90:
AB:CD:EF:12:34:56:78:90:AB:CD:EF:12:34:56:78:90

[Accept] [Reject]
```

**Android UI shows:**
```
Pairing request from: My Desktop
Certificate fingerprint:
FE:DC:BA:98:76:54:32:10:FE:DC:BA:98:76:54:32:10:
FE:DC:BA:98:76:54:32:10:FE:DC:BA:98:76:54:32:10

[Accept] [Reject]
```

**Fingerprint Computation:**
```rust
use sha2::{Sha256, Digest};

fn compute_sha256_fingerprint(cert: &CertificateDer) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cert.as_ref());
    let hash = hasher.finalize();

    // Format as colon-separated hex
    hash.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(":")
}
```

### Phase 6: Pairing Acceptance

**User accepts on desktop:**

Desktop sends acceptance:
```json
{
  "id": 1736784000001,
  "type": "kdeconnect.pair",
  "body": {
    "pair": true
  }
}
```

**Android receives acceptance:**

Android responds with its own acceptance:
```json
{
  "id": 1736784000002,
  "type": "kdeconnect.pair",
  "body": {
    "pair": true
  }
}
```

**Both devices:**
1. Mark device as paired
2. **Persist certificate** to secure storage
3. Initialize plugins
4. Emit pairing success event

**Implementation:**
```rust
// kdeconnect-protocol/src/pairing/handler.rs
async fn handle_pair_packet(&mut self, packet: &Packet) -> Result<(), Error> {
    let pair = packet.get_body_field::<bool>("pair")
        .ok_or(Error::InvalidPacket("Missing pair field"))?;

    if pair {
        match self.state {
            PairingState::Requested => {
                // Remote accepted our request
                self.finalize_pairing().await?;
            }
            PairingState::NotPaired => {
                // Remote is requesting pairing
                self.state = PairingState::RequestedByPeer;
                // Notify user
                self.event_tx.send(PairingEvent::RequestReceived {
                    device_id: self.device_id.clone(),
                })?;
            }
            _ => {}
        }
    } else {
        // Pairing rejected
        self.state = PairingState::NotPaired;
        self.event_tx.send(PairingEvent::Rejected {
            device_id: self.device_id.clone(),
        })?;
    }

    Ok(())
}

async fn finalize_pairing(&mut self) -> Result<(), Error> {
    // Mark as paired
    self.state = PairingState::Paired;

    // Persist certificate to disk
    let cert_path = format!("{}/paired/{}.pem",
        config_dir, self.device_id);
    std::fs::write(cert_path, &self.peer_certificate)?;

    // Update device manager
    self.device_manager.write().await
        .mark_paired(&self.device_id)?;

    // Emit success event
    self.event_tx.send(PairingEvent::Paired {
        device_id: self.device_id.clone(),
    })?;

    info!("Successfully paired with device {}", self.device_id);
    Ok(())
}
```

### Phase 7: Plugin Initialization

**After successful pairing:**

```rust
// Initialize all enabled plugins
let plugins = plugin_manager.initialize_plugins(&device_id).await?;

// Plugins start background tasks:
// - Battery: Monitor and report status every 60s
// - Clipboard: Watch for clipboard changes
// - MPRIS: Connect to D-Bus, monitor media players
// - Notification: Subscribe to notification daemon
```

## Pairing Persistence

### Certificate Storage

**Desktop (Linux):**
```
~/.config/kdeconnect/certs/paired/{device_id}.pem
```

**Android:**
```java
// Stored in app's secure storage
File certFile = new File(
    context.getFilesDir(),
    "kdeconnect/paired/" + deviceId + ".pem"
);
```

### Device Configuration

**Desktop stores device metadata:**
```rust
// ~/.config/kdeconnect/devices.json
{
  "devices": [
    {
      "id": "android_uuid_here",
      "name": "My Phone",
      "type": "phone",
      "is_paired": true,
      "certificate_fingerprint": "AB:CD:EF...",
      "last_seen": 1736784000,
      "capabilities": {
        "incoming": ["kdeconnect.battery.request", ...],
        "outgoing": ["kdeconnect.battery", ...]
      }
    }
  ]
}
```

## Reconnection After Pairing

**When previously paired devices reconnect:**

1. **Discovery:** Device broadcasts identity (UDP)
2. **TLS Connection:** Establish TLS with known certificate
3. **Certificate Verification:**
   - Extract peer certificate from TLS
   - Compute fingerprint
   - **Compare with stored fingerprint**
   - If match → trusted connection
   - If mismatch → reject and alert user
4. **No Pairing Dialog:** Skip pairing, go straight to plugin initialization

**Implementation:**
```rust
async fn verify_paired_device(&self, device_id: &str, cert: &CertificateDer)
    -> Result<bool, Error>
{
    // Load stored certificate
    let stored_cert = self.load_certificate(device_id).await?;

    // Compute fingerprints
    let current_fp = compute_sha256_fingerprint(cert);
    let stored_fp = compute_sha256_fingerprint(&stored_cert);

    if current_fp != stored_fp {
        error!("Certificate mismatch for device {}!", device_id);
        error!("Expected: {}", stored_fp);
        error!("Received: {}", current_fp);

        // SECURITY: Potential MITM attack
        return Err(Error::CertificateMismatch);
    }

    Ok(true)
}
```

## Unpairing

**User initiates unpair:**

```json
{
  "id": 1736784000003,
  "type": "kdeconnect.pair",
  "body": {
    "pair": false
  }
}
```

**Both devices:**
1. Close connection
2. Delete stored certificate
3. Mark device as unpaired
4. Stop all plugins
5. Emit unpair event

**Implementation:**
```rust
pub async fn unpair(&mut self) -> Result<(), Error> {
    // Send unpair packet
    let packet = Packet::new(
        "kdeconnect.pair",
        serde_json::json!({ "pair": false })
    );
    self.connection.send_packet(packet).await?;

    // Delete certificate
    let cert_path = format!("{}/paired/{}.pem",
        config_dir, self.device_id);
    let _ = std::fs::remove_file(cert_path);

    // Update state
    self.state = PairingState::NotPaired;
    self.device_manager.write().await
        .mark_unpaired(&self.device_id)?;

    // Cleanup plugins
    plugin_manager.cleanup_device(&self.device_id).await?;

    Ok(())
}
```

## Security Considerations

### Certificate Pinning
- First connection: User verifies fingerprint
- Subsequent connections: Automatic verification against stored cert
- **Certificate change = MITM alert**

### Pairing Timeout
- 30-second timeout prevents stale pairing requests
- User must respond within timeout window

### No Downgrade Attacks
- Protocol v8 requires TLS before identity
- Identity packets over plain TCP are rejected

### Fingerprint Verification
- **CRITICAL:** User must verify fingerprint matches on both devices
- Prevents MITM during initial pairing
- SHA256 provides strong collision resistance

## Common Issues and Solutions

### Issue: Pairing times out

**Cause:** Network latency, user delay

**Solution:**
```rust
const PAIRING_TIMEOUT: Duration = Duration::from_secs(30);

// Extend timeout if needed
if timeout_expired && user_still_deciding {
    timeout = Instant::now() + PAIRING_TIMEOUT;
}
```

### Issue: Certificate mismatch on reconnect

**Cause:** Certificate regenerated, device reinstalled

**Solution:**
1. Show warning to user
2. Offer to re-pair
3. Delete old certificate
4. Initiate new pairing flow

### Issue: Pairing dialog not showing

**Cause:** Notification permissions, background restrictions

**Solution:**
```java
// Android: Request notification permission
if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
    requestPermissions(
        new String[]{Manifest.permission.POST_NOTIFICATIONS},
        REQUEST_CODE
    );
}

// Show foreground notification
NotificationCompat.Builder builder = ...
startForeground(NOTIFICATION_ID, builder.build());
```

## Testing Pairing

### Test with openssl

```bash
# Generate test certificate
openssl req -x509 -newkey rsa:2048 -nodes \
    -keyout test_key.pem -out test_cert.pem \
    -days 365 -subj "/CN=TestDevice"

# Test TLS connection
openssl s_client -connect localhost:1716 \
    -cert test_cert.pem -key test_key.pem
```

### Mock pairing test

```rust
#[tokio::test]
async fn test_pairing_flow() {
    let (desktop, phone) = create_test_devices().await;

    // Phone requests pairing
    phone.request_pairing(&desktop.id).await.unwrap();

    // Desktop receives request
    let event = desktop.next_event().await.unwrap();
    assert_matches!(event, PairingEvent::RequestReceived { .. });

    // Desktop accepts
    desktop.accept_pairing(&phone.id).await.unwrap();

    // Phone receives acceptance
    let event = phone.next_event().await.unwrap();
    assert_matches!(event, PairingEvent::Paired { .. });

    // Verify both paired
    assert!(desktop.is_paired(&phone.id).await);
    assert!(phone.is_paired(&desktop.id).await);
}
```

## References

- [KDE Connect Protocol Specification](https://valent.andyholmes.ca/documentation/protocol.html)
- [KDE Connect Android - PairingHandler](https://github.com/KDE/kdeconnect-android)
- [TLS 1.3 RFC 8446](https://datatracker.ietf.org/doc/html/rfc8446)
- [Certificate Pinning Best Practices](https://owasp.org/www-community/controls/Certificate_and_Public_Key_Pinning)
