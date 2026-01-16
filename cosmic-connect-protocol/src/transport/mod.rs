//! CConnect Transport Layer
//!
//! This module provides network transport for CConnect protocol.
//! TLS implementation moved to cosmic-connect-core (rustls-based).
//!
//! The transport layer supports multiple transport types (TCP, Bluetooth)
//! through a common trait interface.

mod r#trait;
pub mod bluetooth;
pub mod tcp;

// TLS modules removed - now using cosmic-connect-core
// Old OpenSSL-based TLS moved to cosmic-connect-core with rustls
// pub mod tls;
// pub mod tls_config;

pub use bluetooth::{
    BluetoothConnection, BluetoothTransportFactory, CCONNECT_SERVICE_UUID,
    RFCOMM_READ_CHAR_UUID, RFCOMM_WRITE_CHAR_UUID,
};
pub use r#trait::{
    LatencyCategory, Transport, TransportAddress, TransportCapabilities, TransportFactory,
    TransportPreference, TransportType,
};
pub use tcp::{TcpConnection, TcpTransportFactory};

// TLS types now re-exported from cosmic-connect-core in lib.rs
// pub use cosmic_connect_core::crypto::{TlsConnection, TlsServer, TlsConfig};
