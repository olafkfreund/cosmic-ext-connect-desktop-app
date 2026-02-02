# Transport Layer Architecture

## Overview

The COSMIC Connect transport layer provides a pluggable architecture for supporting multiple network transports (TCP, Bluetooth) through a common trait interface. This design allows devices to connect over different physical mediums while maintaining protocol compatibility.

## Architecture

### Transport Trait

The `Transport` trait defines the common interface all transports must implement:

```rust
#[async_trait]
pub trait Transport: Send + Sync + Debug {
    fn capabilities(&self) -> TransportCapabilities;
    fn remote_address(&self) -> TransportAddress;
    async fn send_packet(&mut self, packet: &Packet) -> Result<()>;
    async fn receive_packet(&mut self) -> Result<Packet>;
    async fn close(self: Box<Self>) -> Result<()>;
    fn is_connected(&self) -> bool;
}
```

### Transport Capabilities

Each transport declares its capabilities:

```rust
pub struct TransportCapabilities {
    pub max_packet_size: usize,       // MTU limit
    pub reliable: bool,                // Guaranteed delivery
    pub connection_oriented: bool,     // Connection vs datagram
    pub latency: LatencyCategory,     // Low/Medium/High
}
```

### Transport Types

Currently supported transports:

#### 1. TCP Transport

- **MTU**: 1 MB (1024 * 1024 bytes)
- **Latency**: Low
- **Reliability**: Guaranteed delivery
- **Use Case**: Primary transport, best for local networks

**Implementation Details:**
- Uses standard TCP sockets
- 4-byte big-endian length prefix
- 10-second timeout for operations
- Automatic error recovery

#### 2. Bluetooth Transport

- **MTU**: 512 bytes (RFCOMM conservative limit)
- **Latency**: Medium
- **Reliability**: Guaranteed delivery
- **Use Case**: Alternative when WiFi unavailable

**Implementation Details:**
- Uses BLE (Bluetooth Low Energy) via btleplug
- Custom KDE Connect service UUID: `185f3df4-3268-4e3f-9fca-d4d5059915bd`
- Read characteristic: `8667556c-9a37-4c91-84ed-54ee27d90049`
- Write characteristic: `d0e8434d-cd29-0996-af41-6c90f4e0eb2a`
- 15-second timeout for operations
- Notification-based packet reception

## Usage

### Creating Connections

#### Using TCP:

```rust
use cosmic_connect_protocol::TcpConnection;

let addr = "192.168.1.100:1716".parse()?;
let mut connection = TcpConnection::connect(addr).await?;

// Send a packet
let packet = Packet::new("kdeconnect.ping", json!({}));
connection.send_packet(&packet).await?;

// Receive a response
let response = connection.receive_packet().await?;
```

#### Using Bluetooth:

```rust
use cosmic_connect_protocol::BluetoothConnection;

let bt_addr = "00:11:22:33:44:55".to_string();
let service_uuid = cosmic_connect_protocol::KDECONNECT_SERVICE_UUID;

let mut connection = BluetoothConnection::connect(bt_addr, service_uuid).await?;

// Send/receive packets same as TCP
connection.send_packet(&packet).await?;
let response = connection.receive_packet().await?;
```

### Using Transport Factories

For abstraction and dynamic transport selection:

```rust
use cosmic_connect_protocol::{
    Transport, TransportFactory, TransportAddress,
    TcpTransportFactory, BluetoothTransportFactory
};

// TCP Factory
let tcp_factory = TcpTransportFactory::new();
let addr = TransportAddress::Tcp("192.168.1.100:1716".parse()?);
let mut transport = tcp_factory.connect(addr).await?;

// Bluetooth Factory
let bt_factory = BluetoothTransportFactory::default();
let addr = TransportAddress::Bluetooth {
    address: "00:11:22:33:44:55".to_string(),
    service_uuid: None, // Uses default KDE Connect UUID
};
let mut transport = bt_factory.connect(addr).await?;

// Use transport trait methods
transport.send_packet(&packet).await?;
println!("Capabilities: {:?}", transport.capabilities());
```

### Transport Selection

Use `TransportPreference` to specify preferred transport:

```rust
pub enum TransportPreference {
    PreferTcp,              // Prefer TCP if available
    PreferBluetooth,        // Prefer Bluetooth if available
    TcpFirst,              // Try TCP first, fall back to Bluetooth
    BluetoothFirst,        // Try Bluetooth first, fall back to TCP
    Only(TransportType),   // Use specific transport only
}
```

## Implementation Status

###  Completed

1. **Transport Trait Abstraction**
   - Core trait interface
   - Transport capabilities
   - Address abstraction
   - Factory pattern

2. **TCP Implementation**
   - Full implementation with Transport trait
   - Backward compatible with existing code
   - Comprehensive tests
   - Factory support

3. **Bluetooth Implementation**
   - BLE-based connection
   - KDE Connect service UUIDs
   - Packet fragmentation handling
   - Factory support

4. **Dependencies**
   - btleplug added to workspace
   - All required crates configured

###  In Progress

5. **Bluetooth Discovery**
   - Need to integrate with existing UDP discovery
   - Add BLE advertisement support
   - Device information exchange over Bluetooth

###  Planned

6. **Integration with Existing Code**
   - Update ConnectionManager to use Transport trait
   - Abstract transport in PairingService
   - Plugin compatibility verification

7. **Configuration Options**
   - Transport enable/disable flags
   - Bluetooth device filtering
   - Transport priority settings
   - Per-device transport preferences

8. **Comprehensive Testing**
   - Unit tests for all transports
   - Integration tests with real hardware
   - Transport switching tests
   - Performance benchmarks
   - Fallback behavior tests

## Design Decisions

### Why Trait-Based Abstraction?

1. **Extensibility**: Easy to add new transports (USB, NFC, etc.)
2. **Testing**: Can mock transports for testing
3. **Flexibility**: Runtime transport selection
4. **Maintainability**: Clear interface contracts

### Why BLE Instead of Classic Bluetooth?

1. **Modern Standard**: BLE is the modern Bluetooth standard
2. **Power Efficient**: Better for mobile devices
3. **Cross-Platform**: Better library support (btleplug)
4. **Security**: Built-in encryption support

### Packet Size Limits

Different transports have different MTU limits:

| Transport | MTU | Reasoning |
|-----------|-----|-----------|
| TCP | 1 MB | Practical limit, can handle large transfers |
| Bluetooth | 512 bytes | Conservative RFCOMM limit, ensures compatibility |

For Bluetooth, packets larger than 512 bytes will fail with an error. Applications should:
1. Check transport capabilities before sending
2. Fragment large data appropriately
3. Use payload protocol for file transfers (which handles fragmentation)

## Future Enhancements

### USB Direct Connection
- For wired connections when available
- Highest bandwidth, lowest latency
- Useful for large file transfers

### NFC Pairing
- Quick pairing via NFC tap
- Exchange Bluetooth/WiFi credentials
- Seamless handoff to primary transport

### Transport Quality Monitoring
- Track latency, packet loss, throughput
- Automatic transport switching based on quality
- Smart transport selection

### Multi-Transport Multiplexing
- Use multiple transports simultaneously
- Load balancing across transports
- Redundancy for critical packets

## Testing Recommendations

### Hardware Testing

1. **TCP Testing**
   - Test over WiFi (2.4GHz and 5GHz)
   - Test over Ethernet
   - Test across network segments
   - Test with firewall rules

2. **Bluetooth Testing**
   - Test with different Bluetooth chipsets
   - Test BLE range limits
   - Test with multiple paired devices
   - Test interference scenarios

3. **Transport Switching**
   - Test fallback from TCP to Bluetooth
   - Test fallback from Bluetooth to TCP
   - Test connection quality transitions
   - Test during active data transfers

### Performance Benchmarks

Measure and document:
- Connection establishment time
- Packet latency (round-trip)
- Throughput (bytes/sec)
- CPU usage per transport
- Memory usage per connection
- Battery impact (for mobile)

## Migration Guide

### For Existing Code Using TcpConnection

Old code continues to work without changes:

```rust
// This still works
let mut conn = TcpConnection::connect(addr).await?;
conn.send_packet(&packet).await?;
```

To use the transport abstraction:

```rust
// New approach with abstraction
let factory = TcpTransportFactory::new();
let addr = TransportAddress::Tcp(addr);
let mut transport: Box<dyn Transport> = factory.connect(addr).await?;
transport.send_packet(&packet).await?;
```

### Adding Bluetooth Support to Existing Features

1. Accept `Box<dyn Transport>` instead of `TcpConnection`
2. Check `transport.capabilities()` before sending large packets
3. Handle transport-specific errors appropriately
4. Allow configuration of preferred transport

Example:

```rust
// Before
async fn handle_device(mut conn: TcpConnection) {
    conn.send_packet(&packet).await?;
}

// After
async fn handle_device(mut transport: Box<dyn Transport>) {
    // Check capabilities
    let caps = transport.capabilities();
    if packet.size() > caps.max_packet_size {
        // Fragment or use payload protocol
    }
    transport.send_packet(&packet).await?;
}
```

## Security Considerations

### Transport Security

1. **TCP**:
   - Uses TLS after pairing
   - Certificate verification required
   - No plaintext after handshake

2. **Bluetooth**:
   - BLE has built-in encryption
   - Pairing establishes shared key
   - Service UUID prevents unauthorized access

### Recommendations

1. Always use encrypted transport after initial pairing
2. Validate device identity regardless of transport
3. Implement transport-specific security where needed
4. Monitor for transport downgrade attacks

## References

- [KDE Connect Protocol Documentation](https://community.kde.org/KDEConnect)
- [btleplug Documentation](https://docs.rs/btleplug/)
- [Bluetooth RFCOMM Specification](https://www.bluetooth.com/specifications/specs/rfcomm-1-1/)
- [BLE GATT Services](https://www.bluetooth.com/specifications/specs/gatt-specification-supplement/)

## Contributing

When adding new transports:

1. Implement the `Transport` trait
2. Create a corresponding `TransportFactory`
3. Add comprehensive tests
4. Document MTU limits and characteristics
5. Update this documentation
6. Consider security implications
7. Add integration tests with existing features

---

*Last Updated: 2025-01-16*
*Version: 0.1.0*
