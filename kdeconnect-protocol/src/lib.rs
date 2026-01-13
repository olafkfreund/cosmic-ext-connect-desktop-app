//! KDE Connect Protocol Implementation
//!
//! This library provides a pure Rust implementation of the KDE Connect protocol,
//! enabling device synchronization and communication between computers and mobile devices.

pub mod device;
pub mod discovery;
pub mod packet;
pub mod pairing;
pub mod plugins;
pub mod transport;

mod error;
pub use device::{ConnectionState, Device, DeviceManager};
pub use discovery::{DeviceInfo, DeviceType, Discovery};
pub use error::{ProtocolError, Result};
pub use packet::{current_timestamp, Packet};
pub use pairing::{CertificateInfo, PairingHandler, PairingPacket, PairingStatus};

/// Protocol version we implement
pub const PROTOCOL_VERSION: u32 = 7;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_version() {
        assert_eq!(PROTOCOL_VERSION, 7);
    }
}
