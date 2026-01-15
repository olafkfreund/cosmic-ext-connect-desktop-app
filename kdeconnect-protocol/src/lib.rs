//! KDE Connect Protocol Implementation
//!
//! This library provides a pure Rust implementation of the KDE Connect protocol,
//! enabling device synchronization and communication between computers and mobile devices.

pub mod connection;
pub mod device;
pub mod discovery;
pub mod packet;
pub mod pairing;
pub mod payload;
pub mod plugins;
pub mod transport;

mod error;

// Re-export from cosmic-connect-core
pub use cosmic_connect_core::crypto::{
    CertificateInfo, DeviceInfo as TlsDeviceInfo, TlsConfig, TlsConnection, TlsServer,
    should_initiate_connection,
};
pub use cosmic_connect_core::{Packet as CorePacket, ProtocolError as CoreProtocolError};

// Re-export local types
pub use connection::{ConnectionConfig, ConnectionEvent, ConnectionManager};
pub use device::{ConnectionState, Device, DeviceManager};
pub use discovery::{
    DeviceInfo, DeviceType, Discovery, DiscoveryConfig, DiscoveryEvent, DiscoveryService,
    DISCOVERY_PORT,
};
pub use error::{ProtocolError, Result};
pub use packet::{current_timestamp, Packet};
pub use pairing::{
    PairingConfig, PairingEvent, PairingHandler, PairingPacket, PairingService, PairingStatus,
    PAIRING_TIMEOUT,
};
pub use payload::{FileTransferInfo, PayloadClient, PayloadServer};
pub use plugins::{Plugin, PluginManager};
pub use transport::TcpConnection;

/// Protocol version we implement
/// Updated to version 8 to match latest KDE Connect Android app
pub const PROTOCOL_VERSION: u32 = 8;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_version() {
        assert_eq!(PROTOCOL_VERSION, 7);
    }
}
