//! Pairing Event System
//!
//! This module defines events emitted during the pairing process.

use crate::PairingStatus;

/// Events emitted by the pairing service
#[derive(Debug, Clone)]
pub enum PairingEvent {
    /// Pairing request was sent to a device
    RequestSent {
        /// ID of the device we sent the request to
        device_id: String,
        /// Certificate fingerprint for user verification
        our_fingerprint: String,
    },

    /// Pairing request was received from a device
    RequestReceived {
        /// ID of the device requesting pairing
        device_id: String,
        /// Name of the device requesting pairing
        device_name: String,
        /// Certificate fingerprint for user verification
        their_fingerprint: String,
    },

    /// Pairing was accepted (by us or by peer)
    PairingAccepted {
        /// ID of the paired device
        device_id: String,
        /// Name of the paired device
        device_name: String,
        /// Certificate fingerprint of the paired device
        certificate_fingerprint: String,
    },

    /// Pairing was rejected (by us or by peer)
    PairingRejected {
        /// ID of the device
        device_id: String,
        /// Reason for rejection (optional)
        reason: Option<String>,
    },

    /// Pairing status changed
    StatusChanged {
        /// ID of the device
        device_id: String,
        /// New pairing status
        status: PairingStatus,
    },

    /// Device was unpaired
    DeviceUnpaired {
        /// ID of the unpaired device
        device_id: String,
    },

    /// Pairing timeout occurred
    PairingTimeout {
        /// ID of the device
        device_id: String,
    },

    /// An error occurred during pairing
    Error {
        /// ID of the device (if applicable)
        device_id: Option<String>,
        /// Error message
        message: String,
    },
}

impl PairingEvent {
    /// Check if this is a request received event
    pub fn is_request_received(&self) -> bool {
        matches!(self, PairingEvent::RequestReceived { .. })
    }

    /// Check if this is a pairing accepted event
    pub fn is_pairing_accepted(&self) -> bool {
        matches!(self, PairingEvent::PairingAccepted { .. })
    }

    /// Check if this is a pairing rejected event
    pub fn is_pairing_rejected(&self) -> bool {
        matches!(self, PairingEvent::PairingRejected { .. })
    }

    /// Get device ID if this event is device-related
    pub fn device_id(&self) -> Option<&str> {
        match self {
            PairingEvent::RequestSent { device_id, .. } => Some(device_id),
            PairingEvent::RequestReceived { device_id, .. } => Some(device_id),
            PairingEvent::PairingAccepted { device_id, .. } => Some(device_id),
            PairingEvent::PairingRejected { device_id, .. } => Some(device_id),
            PairingEvent::StatusChanged { device_id, .. } => Some(device_id),
            PairingEvent::DeviceUnpaired { device_id } => Some(device_id),
            PairingEvent::PairingTimeout { device_id } => Some(device_id),
            PairingEvent::Error { device_id, .. } => device_id.as_deref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_checking() {
        let request_received = PairingEvent::RequestReceived {
            device_id: "test".to_string(),
            device_name: "Test Device".to_string(),
            their_fingerprint: "AA:BB:CC".to_string(),
        };
        assert!(request_received.is_request_received());
        assert!(!request_received.is_pairing_accepted());

        let accepted = PairingEvent::PairingAccepted {
            device_id: "test".to_string(),
            device_name: "Test Device".to_string(),
            certificate_fingerprint: "AA:BB:CC".to_string(),
        };
        assert!(accepted.is_pairing_accepted());
        assert!(!accepted.is_request_received());
    }

    #[test]
    fn test_device_id_extraction() {
        let event = PairingEvent::RequestSent {
            device_id: "device_123".to_string(),
            our_fingerprint: "AA:BB:CC".to_string(),
        };
        assert_eq!(event.device_id(), Some("device_123"));

        let error = PairingEvent::Error {
            device_id: None,
            message: "General error".to_string(),
        };
        assert_eq!(error.device_id(), None);
    }
}
