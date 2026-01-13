//! KDE Connect Device Discovery
//!
//! This module implements UDP broadcast-based device discovery for KDE Connect.
//! Devices announce themselves by broadcasting identity packets on UDP port 1716.
//!
//! ## Discovery Protocol
//!
//! 1. **Broadcast**: Send identity packet via UDP broadcast on port 1716
//! 2. **Listen**: Listen for identity packets from other devices
//! 3. **Connect**: Establish TCP connection to discovered device's announced port
//!
//! ## Identity Packet Format
//!
//! Identity packets (`kdeconnect.identity`) contain:
//! - `deviceId`: UUIDv4 with underscores (e.g., `740bd4b9_b418_4ee4_97d6_caf1da8151be`)
//! - `deviceName`: 1-32 character device name
//! - `deviceType`: desktop, laptop, phone, tablet, or tv
//! - `protocolVersion`: Protocol version (currently 7)
//! - `incomingCapabilities`: List of packet types this device can receive
//! - `outgoingCapabilities`: List of packet types this device can send
//! - `tcpPort`: TCP port for establishing connections
//!
//! ## Port Configuration
//!
//! - Primary port: UDP 1716
//! - Fallback range: 1714-1764
//! - Listen on 0.0.0.0 for incoming broadcasts
//!
//! ## References
//! - [KDE Connect Protocol](https://invent.kde.org/network/kdeconnect-kde)
//! - [Valent Protocol Reference](https://valent.andyholmes.ca/documentation/protocol.html)

use crate::{Packet, ProtocolError, Result, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Default UDP port for device discovery
pub const DISCOVERY_PORT: u16 = 1716;

/// Port range for fallback when primary port is unavailable
pub const PORT_RANGE_START: u16 = 1714;
pub const PORT_RANGE_END: u16 = 1764;

/// Broadcast address for IPv4
pub const BROADCAST_ADDR: Ipv4Addr = Ipv4Addr::new(255, 255, 255, 255);

/// Default timeout for discovery operations
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);

/// Device types supported by KDE Connect
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceType {
    Desktop,
    Laptop,
    Phone,
    Tablet,
    Tv,
}

impl DeviceType {
    /// Convert device type to string
    pub fn as_str(&self) -> &'static str {
        match self {
            DeviceType::Desktop => "desktop",
            DeviceType::Laptop => "laptop",
            DeviceType::Phone => "phone",
            DeviceType::Tablet => "tablet",
            DeviceType::Tv => "tv",
        }
    }
}

/// Device identity information
///
/// Contains all information about a device needed for discovery and pairing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// Unique device identifier (UUIDv4 with underscores)
    pub device_id: String,

    /// Human-readable device name (1-32 characters)
    pub device_name: String,

    /// Type of device
    pub device_type: DeviceType,

    /// Protocol version (currently 7)
    pub protocol_version: u32,

    /// Packet types this device can receive
    pub incoming_capabilities: Vec<String>,

    /// Packet types this device can send
    pub outgoing_capabilities: Vec<String>,

    /// TCP port for connections
    pub tcp_port: u16,
}

impl DeviceInfo {
    /// Create a new DeviceInfo
    ///
    /// # Arguments
    ///
    /// * `device_name` - Human-readable name (1-32 characters)
    /// * `device_type` - Type of device
    /// * `tcp_port` - TCP port for connections
    ///
    /// # Examples
    ///
    /// ```
    /// use kdeconnect_protocol::discovery::{DeviceInfo, DeviceType};
    ///
    /// let info = DeviceInfo::new("My Computer", DeviceType::Desktop, 1716);
    /// ```
    pub fn new(device_name: impl Into<String>, device_type: DeviceType, tcp_port: u16) -> Self {
        let device_name = device_name.into();
        if device_name.is_empty() || device_name.len() > 32 {
            warn!(
                "Device name should be 1-32 characters, got: {}",
                device_name
            );
        }

        Self {
            device_id: Self::generate_device_id(),
            device_name,
            device_type,
            protocol_version: PROTOCOL_VERSION,
            incoming_capabilities: Vec::new(),
            outgoing_capabilities: Vec::new(),
            tcp_port,
        }
    }

    /// Generate a UUIDv4 device ID with underscores
    ///
    /// KDE Connect uses UUIDs with underscores instead of hyphens
    fn generate_device_id() -> String {
        Uuid::new_v4().to_string().replace('-', "_")
    }

    /// Create a DeviceInfo with explicit device ID
    pub fn with_id(
        device_id: impl Into<String>,
        device_name: impl Into<String>,
        device_type: DeviceType,
        tcp_port: u16,
    ) -> Self {
        Self {
            device_id: device_id.into(),
            device_name: device_name.into(),
            device_type,
            protocol_version: PROTOCOL_VERSION,
            incoming_capabilities: Vec::new(),
            outgoing_capabilities: Vec::new(),
            tcp_port,
        }
    }

    /// Add an incoming capability
    pub fn with_incoming_capability(mut self, capability: impl Into<String>) -> Self {
        self.incoming_capabilities.push(capability.into());
        self
    }

    /// Add an outgoing capability
    pub fn with_outgoing_capability(mut self, capability: impl Into<String>) -> Self {
        self.outgoing_capabilities.push(capability.into());
        self
    }

    /// Set all incoming capabilities at once
    pub fn with_incoming_capabilities(mut self, capabilities: Vec<String>) -> Self {
        self.incoming_capabilities = capabilities;
        self
    }

    /// Set all outgoing capabilities at once
    pub fn with_outgoing_capabilities(mut self, capabilities: Vec<String>) -> Self {
        self.outgoing_capabilities = capabilities;
        self
    }

    /// Convert DeviceInfo to an identity packet
    pub fn to_identity_packet(&self) -> Packet {
        Packet::new(
            "kdeconnect.identity",
            json!({
                "deviceId": self.device_id,
                "deviceName": self.device_name,
                "deviceType": self.device_type.as_str(),
                "protocolVersion": self.protocol_version,
                "incomingCapabilities": self.incoming_capabilities,
                "outgoingCapabilities": self.outgoing_capabilities,
                "tcpPort": self.tcp_port,
            }),
        )
    }

    /// Parse DeviceInfo from an identity packet
    pub fn from_identity_packet(packet: &Packet) -> Result<Self> {
        if !packet.is_type("kdeconnect.identity") {
            return Err(ProtocolError::InvalidPacket(
                "Not an identity packet".to_string(),
            ));
        }

        let device_id = packet
            .get_body_field::<String>("deviceId")
            .ok_or_else(|| ProtocolError::InvalidPacket("Missing deviceId".to_string()))?;

        let device_name = packet
            .get_body_field::<String>("deviceName")
            .ok_or_else(|| ProtocolError::InvalidPacket("Missing deviceName".to_string()))?;

        let device_type_str = packet
            .get_body_field::<String>("deviceType")
            .ok_or_else(|| ProtocolError::InvalidPacket("Missing deviceType".to_string()))?;

        let device_type = match device_type_str.as_str() {
            "desktop" => DeviceType::Desktop,
            "laptop" => DeviceType::Laptop,
            "phone" => DeviceType::Phone,
            "tablet" => DeviceType::Tablet,
            "tv" => DeviceType::Tv,
            _ => {
                return Err(ProtocolError::InvalidPacket(format!(
                    "Unknown device type: {}",
                    device_type_str
                )))
            }
        };

        let protocol_version = packet
            .get_body_field::<u32>("protocolVersion")
            .unwrap_or(PROTOCOL_VERSION);

        let tcp_port = packet
            .get_body_field::<u16>("tcpPort")
            .ok_or_else(|| ProtocolError::InvalidPacket("Missing tcpPort".to_string()))?;

        let incoming_capabilities = packet
            .get_body_field::<Vec<String>>("incomingCapabilities")
            .unwrap_or_default();

        let outgoing_capabilities = packet
            .get_body_field::<Vec<String>>("outgoingCapabilities")
            .unwrap_or_default();

        Ok(Self {
            device_id,
            device_name,
            device_type,
            protocol_version,
            incoming_capabilities,
            outgoing_capabilities,
            tcp_port,
        })
    }
}

/// Device discovery manager
pub struct Discovery {
    socket: UdpSocket,
    device_info: DeviceInfo,
}

impl Discovery {
    /// Create a new discovery instance
    ///
    /// Attempts to bind to port 1716, falling back to ports 1714-1764 if unavailable.
    ///
    /// # Arguments
    ///
    /// * `device_info` - Information about this device
    ///
    /// # Errors
    ///
    /// Returns error if unable to bind to any port in the range
    pub fn new(device_info: DeviceInfo) -> Result<Self> {
        // Try primary port first
        let socket = match UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)) {
            Ok(s) => {
                info!("Bound to UDP port {}", DISCOVERY_PORT);
                s
            }
            Err(e) => {
                warn!(
                    "Failed to bind to primary port {}: {}. Trying fallback range...",
                    DISCOVERY_PORT, e
                );

                // Try fallback ports
                let mut last_err = e;
                let mut socket = None;

                for port in PORT_RANGE_START..=PORT_RANGE_END {
                    if port == DISCOVERY_PORT {
                        continue; // Already tried
                    }

                    match UdpSocket::bind(("0.0.0.0", port)) {
                        Ok(s) => {
                            info!("Bound to fallback UDP port {}", port);
                            socket = Some(s);
                            break;
                        }
                        Err(e) => last_err = e,
                    }
                }

                socket.ok_or_else(|| {
                    ProtocolError::Io(std::io::Error::new(
                        std::io::ErrorKind::AddrInUse,
                        format!(
                            "Failed to bind to any port in range {}-{}: {}",
                            PORT_RANGE_START, PORT_RANGE_END, last_err
                        ),
                    ))
                })?
            }
        };

        // Enable broadcast
        socket.set_broadcast(true)?;

        // Set read timeout
        socket.set_read_timeout(Some(DISCOVERY_TIMEOUT))?;

        Ok(Self {
            socket,
            device_info,
        })
    }

    /// Broadcast identity to discover devices
    ///
    /// Sends an identity packet via UDP broadcast to port 1716.
    /// Other KDE Connect devices on the network will receive this and can respond.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use kdeconnect_protocol::discovery::{Discovery, DeviceInfo, DeviceType};
    ///
    /// let device_info = DeviceInfo::new("My Computer", DeviceType::Desktop, 1716);
    /// let discovery = Discovery::new(device_info).unwrap();
    /// discovery.broadcast_identity().unwrap();
    /// ```
    pub fn broadcast_identity(&self) -> Result<()> {
        let packet = self.device_info.to_identity_packet();
        let bytes = packet.to_bytes()?;

        let broadcast_addr = SocketAddr::new(IpAddr::V4(BROADCAST_ADDR), DISCOVERY_PORT);

        debug!(
            "Broadcasting identity packet ({} bytes) to {}",
            bytes.len(),
            broadcast_addr
        );

        self.socket.send_to(&bytes, broadcast_addr)?;
        info!(
            "Broadcasted identity for device: {}",
            self.device_info.device_name
        );

        Ok(())
    }

    /// Listen for device identity broadcasts
    ///
    /// Blocks until a device is discovered or timeout occurs.
    ///
    /// # Returns
    ///
    /// Returns tuple of (DeviceInfo, source SocketAddr) for discovered device
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use kdeconnect_protocol::discovery::{Discovery, DeviceInfo, DeviceType};
    ///
    /// let device_info = DeviceInfo::new("My Computer", DeviceType::Desktop, 1716);
    /// let discovery = Discovery::new(device_info).unwrap();
    ///
    /// match discovery.listen_for_devices() {
    ///     Ok((info, addr)) => println!("Discovered: {} at {}", info.device_name, addr),
    ///     Err(e) => eprintln!("Discovery error: {}", e),
    /// }
    /// ```
    pub fn listen_for_devices(&self) -> Result<(DeviceInfo, SocketAddr)> {
        let mut buf = [0u8; 4096]; // Large buffer to handle all identity packet sizes

        debug!("Listening for device broadcasts...");

        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((size, src_addr)) => {
                    debug!("Received {} bytes from {}", size, src_addr);

                    // Parse packet
                    match Packet::from_bytes(&buf[..size]) {
                        Ok(packet) => {
                            if !packet.is_type("kdeconnect.identity") {
                                debug!("Ignoring non-identity packet from {}", src_addr);
                                continue;
                            }

                            // Parse device info
                            match DeviceInfo::from_identity_packet(&packet) {
                                Ok(device_info) => {
                                    // Don't discover ourselves
                                    if device_info.device_id == self.device_info.device_id {
                                        debug!("Ignoring our own broadcast");
                                        continue;
                                    }

                                    info!(
                                        "Discovered device: {} ({}) at {}",
                                        device_info.device_name,
                                        device_info.device_type.as_str(),
                                        src_addr
                                    );

                                    return Ok((device_info, src_addr));
                                }
                                Err(e) => {
                                    warn!("Failed to parse device info from {}: {}", src_addr, e);
                                    continue;
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse packet from {}: {}", src_addr, e);
                            continue;
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Timeout
                    return Err(ProtocolError::Io(e));
                }
                Err(e) => {
                    error!("Error receiving packet: {}", e);
                    return Err(ProtocolError::Io(e));
                }
            }
        }
    }

    /// Get the local port this discovery instance is bound to
    pub fn local_port(&self) -> Result<u16> {
        Ok(self.socket.local_addr()?.port())
    }

    /// Get a reference to this device's info
    pub fn device_info(&self) -> &DeviceInfo {
        &self.device_info
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_device_type_serialization() {
        assert_eq!(DeviceType::Desktop.as_str(), "desktop");
        assert_eq!(DeviceType::Laptop.as_str(), "laptop");
        assert_eq!(DeviceType::Phone.as_str(), "phone");
        assert_eq!(DeviceType::Tablet.as_str(), "tablet");
        assert_eq!(DeviceType::Tv.as_str(), "tv");
    }

    #[test]
    fn test_device_info_creation() {
        let info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1716);

        assert!(!info.device_id.is_empty());
        assert!(info.device_id.contains('_')); // UUID with underscores
        assert_eq!(info.device_name, "Test Device");
        assert_eq!(info.device_type, DeviceType::Desktop);
        assert_eq!(info.protocol_version, PROTOCOL_VERSION);
        assert_eq!(info.tcp_port, 1716);
    }

    #[test]
    fn test_device_info_with_capabilities() {
        let info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1716)
            .with_incoming_capability("kdeconnect.battery")
            .with_incoming_capability("kdeconnect.ping")
            .with_outgoing_capability("kdeconnect.notification");

        assert_eq!(info.incoming_capabilities.len(), 2);
        assert_eq!(info.outgoing_capabilities.len(), 1);
        assert!(info
            .incoming_capabilities
            .contains(&"kdeconnect.battery".to_string()));
    }

    #[test]
    fn test_identity_packet_conversion() {
        let info = DeviceInfo::new("Test Device", DeviceType::Laptop, 1739)
            .with_incoming_capability("kdeconnect.ping")
            .with_outgoing_capability("kdeconnect.battery");

        let packet = info.to_identity_packet();

        assert!(packet.is_type("kdeconnect.identity"));
        assert_eq!(
            packet.get_body_field::<String>("deviceName"),
            Some("Test Device".to_string())
        );
        assert_eq!(
            packet.get_body_field::<String>("deviceType"),
            Some("laptop".to_string())
        );
        assert_eq!(packet.get_body_field::<u16>("tcpPort"), Some(1739));
    }

    #[test]
    fn test_device_info_roundtrip() {
        let original =
            DeviceInfo::with_id("test_device_id", "Test Device", DeviceType::Phone, 1740)
                .with_incoming_capability("kdeconnect.ping")
                .with_outgoing_capability("kdeconnect.battery");

        let packet = original.to_identity_packet();
        let parsed = DeviceInfo::from_identity_packet(&packet).unwrap();

        assert_eq!(parsed.device_id, "test_device_id");
        assert_eq!(parsed.device_name, "Test Device");
        assert_eq!(parsed.device_type, DeviceType::Phone);
        assert_eq!(parsed.tcp_port, 1740);
        assert_eq!(parsed.incoming_capabilities, vec!["kdeconnect.ping"]);
        assert_eq!(parsed.outgoing_capabilities, vec!["kdeconnect.battery"]);
    }

    #[test]
    fn test_invalid_identity_packet() {
        let packet = Packet::new("kdeconnect.ping", json!({}));
        let result = DeviceInfo::from_identity_packet(&packet);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_fields_in_identity() {
        let packet = Packet::new(
            "kdeconnect.identity",
            json!({
                "deviceId": "test_id",
                // Missing deviceName, deviceType, tcpPort
            }),
        );

        let result = DeviceInfo::from_identity_packet(&packet);
        assert!(result.is_err());
    }

    #[test]
    fn test_device_id_format() {
        let info = DeviceInfo::new("Test", DeviceType::Desktop, 1716);
        // UUID format: xxxxxxxx_xxxx_xxxx_xxxx_xxxxxxxxxxxx
        assert_eq!(info.device_id.len(), 36); // 32 hex chars + 4 underscores
        assert_eq!(info.device_id.matches('_').count(), 4);
    }

    // Integration tests that require network are marked as ignored
    #[test]
    #[ignore]
    fn test_discovery_broadcast() {
        let device_info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1716);
        let discovery = Discovery::new(device_info).unwrap();
        let result = discovery.broadcast_identity();
        assert!(result.is_ok());
    }
}
