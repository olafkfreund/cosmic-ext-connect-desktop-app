//! Transport Trait Abstraction
//!
//! Defines a common interface for different transport types (TCP, Bluetooth, etc.)
//! that can be used to send and receive CConnect packets.

use crate::{Packet, Result};
use async_trait::async_trait;
use std::fmt::Debug;

/// Transport capabilities and characteristics
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportCapabilities {
    /// Maximum transmission unit (packet size) in bytes
    pub max_packet_size: usize,

    /// Whether this transport supports reliable delivery
    pub reliable: bool,

    /// Whether this transport is connection-oriented
    pub connection_oriented: bool,

    /// Typical latency category
    pub latency: LatencyCategory,
}

/// Latency categories for transports
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyCategory {
    /// Low latency (< 10ms typical)
    Low,

    /// Medium latency (10-50ms typical)
    Medium,

    /// High latency (> 50ms typical)
    High,
}

/// Transport address information
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportAddress {
    /// TCP/IP socket address (IP:port)
    Tcp(std::net::SocketAddr),

    /// Bluetooth device address
    Bluetooth {
        /// Bluetooth MAC address
        address: String,

        /// Service UUID
        service_uuid: Option<uuid::Uuid>,
    },
}

impl std::fmt::Display for TransportAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportAddress::Tcp(addr) => write!(f, "tcp://{}", addr),
            TransportAddress::Bluetooth { address, service_uuid } => {
                if let Some(uuid) = service_uuid {
                    write!(f, "bluetooth://{} ({})", address, uuid)
                } else {
                    write!(f, "bluetooth://{}", address)
                }
            }
        }
    }
}

/// Common transport interface for CConnect
#[async_trait]
pub trait Transport: Send + Sync + Debug {
    /// Get transport capabilities
    fn capabilities(&self) -> TransportCapabilities;

    /// Get remote address
    fn remote_address(&self) -> TransportAddress;

    /// Send a packet
    ///
    /// # Arguments
    ///
    /// * `packet` - The packet to send
    ///
    /// # Errors
    ///
    /// Returns an error if the packet is too large for this transport
    /// or if there's a communication failure.
    async fn send_packet(&mut self, packet: &Packet) -> Result<()>;

    /// Receive a packet
    ///
    /// # Errors
    ///
    /// Returns an error if packet reception fails, times out,
    /// or if the packet is malformed.
    async fn receive_packet(&mut self) -> Result<Packet>;

    /// Close the connection gracefully
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be closed cleanly.
    async fn close(self: Box<Self>) -> Result<()>;

    /// Check if the transport is still connected
    fn is_connected(&self) -> bool {
        true // Default implementation - override if transport has connection state
    }
}

/// Factory trait for creating transport connections
#[async_trait]
pub trait TransportFactory: Send + Sync + Debug {
    /// Connect to a remote device
    ///
    /// # Arguments
    ///
    /// * `address` - Remote transport address to connect to
    async fn connect(&self, address: TransportAddress) -> Result<Box<dyn Transport>>;

    /// Get the transport type this factory creates
    fn transport_type(&self) -> TransportType;
}

/// Transport type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportType {
    /// TCP/IP transport
    Tcp,

    /// Bluetooth transport
    Bluetooth,
}

impl std::fmt::Display for TransportType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportType::Tcp => write!(f, "TCP"),
            TransportType::Bluetooth => write!(f, "Bluetooth"),
        }
    }
}

/// Transport selection preference
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum TransportPreference {
    /// Prefer TCP if available
    #[default]
    PreferTcp,

    /// Prefer Bluetooth if available
    PreferBluetooth,

    /// Try TCP first, fall back to Bluetooth
    TcpFirst,

    /// Try Bluetooth first, fall back to TCP
    BluetoothFirst,

    /// Use the specified transport only
    Only(TransportType),
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_address_display() {
        let tcp_addr = TransportAddress::Tcp("192.168.1.100:1716".parse().unwrap());
        assert_eq!(tcp_addr.to_string(), "tcp://192.168.1.100:1716");

        let bt_addr = TransportAddress::Bluetooth {
            address: "00:11:22:33:44:55".to_string(),
            service_uuid: None,
        };
        assert_eq!(bt_addr.to_string(), "bluetooth://00:11:22:33:44:55");
    }

    #[test]
    fn test_transport_type_display() {
        assert_eq!(TransportType::Tcp.to_string(), "TCP");
        assert_eq!(TransportType::Bluetooth.to_string(), "Bluetooth");
    }

    #[test]
    fn test_default_transport_preference() {
        assert_eq!(
            TransportPreference::default(),
            TransportPreference::PreferTcp
        );
    }
}
