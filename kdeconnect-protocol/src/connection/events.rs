//! Connection Events
//!
//! Events emitted by the connection manager for device connectivity changes.

use crate::Packet;
use std::net::SocketAddr;

/// Connection event types
#[derive(Debug, Clone)]
pub enum ConnectionEvent {
    /// A connection has been established to a device
    Connected {
        /// Device ID
        device_id: String,
        /// Remote address
        remote_addr: SocketAddr,
    },

    /// A connection to a device has been lost
    Disconnected {
        /// Device ID
        device_id: String,
        /// Reason for disconnection (if known)
        reason: Option<String>,
    },

    /// A packet has been received from a device
    PacketReceived {
        /// Device ID that sent the packet
        device_id: String,
        /// The received packet
        packet: Packet,
        /// Remote address of the connection
        remote_addr: SocketAddr,
    },

    /// An error occurred with a connection
    ConnectionError {
        /// Device ID (if known)
        device_id: Option<String>,
        /// Error message
        message: String,
    },

    /// Connection manager started
    ManagerStarted {
        /// Local port listening on
        port: u16,
    },

    /// Connection manager stopped
    ManagerStopped,
}
