//! CConnect Protocol Implementation
//!
//! This library provides a pure Rust implementation of the CConnect protocol,
//! enabling device synchronization and communication between computers and mobile devices.

pub mod bluetooth_connection_manager;
pub mod connection;
pub mod device;
pub mod discovery;
pub mod fs_utils;
pub mod packet;
pub mod pairing;
pub mod payload;
pub mod plugins;
pub mod recovery;
pub mod recovery_coordinator;
pub mod resource_manager;
pub mod transport;
pub mod transport_manager;

mod error;

// Re-export from cosmic-connect-core
pub use cosmic_connect_core::crypto::{
    should_initiate_connection, CertificateInfo, DeviceInfo as TlsDeviceInfo, TlsConfig,
    TlsConnection, TlsServer,
};
pub use cosmic_connect_core::{Packet as CorePacket, ProtocolError as CoreProtocolError};

// Re-export local types
pub use bluetooth_connection_manager::BluetoothConnectionManager;
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
pub use payload::{FileTransferInfo, PayloadClient, PayloadServer, TlsPayloadClient};
pub use plugins::{Plugin, PluginManager};
pub use recovery::{ReconnectionStrategy, RecoveryManager, TransferState};
pub use recovery_coordinator::RecoveryCoordinator;
pub use resource_manager::{MemoryStats, ResourceConfig, ResourceManager, TransferInfo};
pub use transport::{
    BluetoothConnection, BluetoothTransportFactory, LatencyCategory, TcpConnection,
    TcpTransportFactory, Transport, TransportAddress, TransportCapabilities, TransportFactory,
    TransportPreference, TransportType, CCONNECT_SERVICE_UUID, RFCOMM_READ_CHAR_UUID,
    RFCOMM_WRITE_CHAR_UUID,
};
pub use transport_manager::{TransportManager, TransportManagerConfig, TransportManagerEvent};

/// Protocol version we implement
/// Updated to version 8 to match latest CConnect Android app
pub const PROTOCOL_VERSION: u32 = 8;

#[cfg(test)]
pub mod test_utils;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_version() {
        assert_eq!(PROTOCOL_VERSION, 8);
    }
}
