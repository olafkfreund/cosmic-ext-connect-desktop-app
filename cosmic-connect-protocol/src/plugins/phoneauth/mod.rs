//! Phone Authentication Protocol Packets
//!
//! This module defines the packet structures for the phone authentication system.
//! The authentication flow uses Ed25519 digital signatures for cryptographic verification.
//!
//! ## Protocol Overview
//!
//! The phone authentication protocol enables biometric authentication from a paired phone
//! to unlock a desktop or authorize privileged operations.
//!
//! ## Packet Types
//!
//! - `cconnect.auth.request` - Desktop requests authentication from phone
//! - `cconnect.auth.response` - Phone responds with authentication result
//! - `cconnect.auth.cancel` - Cancel pending authentication request
//! - `cconnect.auth.capabilities` - Phone advertises authentication capabilities
//!
//! ## Authentication Flow
//!
//! ```text
//! Desktop                                Phone
//!    |                                      |
//!    |  1. auth.capabilities                |
//!    |<-------------------------------------|
//!    |                                      |
//!    |  2. auth.request                     |
//!    |------------------------------------->|
//!    |     (challenge, nonce, prompt)       |
//!    |                                      |
//!    |              [User sees prompt]      |
//!    |              [Biometric scan]        |
//!    |                                      |
//!    |  3. auth.response                    |
//!    |<-------------------------------------|
//!    |     (approved, signature)            |
//!    |                                      |
//!    |  [Verify signature]                  |
//!    |  [Grant/deny access]                 |
//! ```
//!
//! ## Security Features
//!
//! - Ed25519 public key cryptography
//! - Challenge-response protocol
//! - Nonce for replay prevention
//! - Timestamp-based request expiration
//! - Biometric verification on phone
//!
//! ## Example
//!
//! ```rust,ignore
//! use cosmic_connect_protocol::plugins::phoneauth::*;
//!
//! // Create authentication request
//! let request = AuthRequest {
//!     request_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
//!     username: "alice".to_string(),
//!     auth_type: AuthType::Unlock,
//!     challenge: "base64_encoded_challenge".to_string(),
//!     nonce: "base64_encoded_nonce".to_string(),
//!     timestamp: 1706745600,
//!     desktop_id: "cosmic-desktop-abc123".to_string(),
//!     prompt: "Unlock COSMIC Desktop for alice".to_string(),
//! };
//!
//! // Create response packet
//! let packet = request.to_packet();
//! ```

pub mod capabilities;
pub mod request;
pub mod response;

pub use capabilities::{AuthCapabilities, BiometricType};
pub use request::{AuthRequest, AuthType};
pub use response::AuthResponse;

/// Packet type for authentication request
pub const PACKET_TYPE_AUTH_REQUEST: &str = "cconnect.auth.request";

/// Packet type for authentication response
pub const PACKET_TYPE_AUTH_RESPONSE: &str = "cconnect.auth.response";

/// Packet type for authentication cancellation
pub const PACKET_TYPE_AUTH_CANCEL: &str = "cconnect.auth.cancel";

/// Packet type for authentication capabilities advertisement
pub const PACKET_TYPE_AUTH_CAPABILITIES: &str = "cconnect.auth.capabilities";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_type_constants() {
        assert_eq!(PACKET_TYPE_AUTH_REQUEST, "cconnect.auth.request");
        assert_eq!(PACKET_TYPE_AUTH_RESPONSE, "cconnect.auth.response");
        assert_eq!(PACKET_TYPE_AUTH_CANCEL, "cconnect.auth.cancel");
        assert_eq!(PACKET_TYPE_AUTH_CAPABILITIES, "cconnect.auth.capabilities");
    }
}
