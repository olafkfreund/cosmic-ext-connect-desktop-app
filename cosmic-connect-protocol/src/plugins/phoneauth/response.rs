//! Authentication Response Packet
//!
//! Sent from phone to desktop with the result of biometric authentication.

use crate::Packet;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub use super::capabilities::BiometricType;

/// Authentication response packet
///
/// Sent from phone to desktop after user completes (or rejects) biometric authentication.
/// Contains the result and cryptographic signature if approved.
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_protocol::plugins::phoneauth::{AuthResponse, BiometricType};
///
/// // Approved response
/// let approved = AuthResponse::approved(
///     "550e8400-e29b-41d4-a716-446655440000",
///     "base64_nonce",
///     "base64_signature",
///     BiometricType::Fingerprint,
///     "phone-xyz789",
/// );
/// assert!(approved.approved);
///
/// // Denied response
/// let denied = AuthResponse::denied(
///     "550e8400-e29b-41d4-a716-446655440000",
///     "base64_nonce",
///     "User cancelled",
/// );
/// assert!(!denied.approved);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthResponse {
    /// Original request ID this responds to
    #[serde(rename = "requestId")]
    pub request_id: String,

    /// Original nonce from request (for verification)
    pub nonce: String,

    /// Whether authentication was approved
    pub approved: bool,

    /// Ed25519 signature over challenge (base64-encoded)
    /// Only present if approved is true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,

    /// Biometric method used for authentication
    #[serde(rename = "biometricType")]
    pub biometric_type: BiometricType,

    /// Phone device identifier
    #[serde(rename = "phoneId")]
    pub phone_id: String,

    /// Error message if not approved
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl AuthResponse {
    /// Create an approved authentication response
    ///
    /// # Parameters
    ///
    /// - `request_id`: Original request ID
    /// - `nonce`: Original nonce from request
    /// - `signature`: Base64-encoded Ed25519 signature
    /// - `biometric_type`: Type of biometric used
    /// - `phone_id`: Phone device identifier
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::phoneauth::{AuthResponse, BiometricType};
    ///
    /// let response = AuthResponse::approved(
    ///     "request-123",
    ///     "nonce-456",
    ///     "signature-789",
    ///     BiometricType::Fingerprint,
    ///     "phone-001",
    /// );
    ///
    /// assert!(response.approved);
    /// assert!(response.signature.is_some());
    /// assert!(response.error.is_none());
    /// ```
    pub fn approved(
        request_id: impl Into<String>,
        nonce: impl Into<String>,
        signature: impl Into<String>,
        biometric_type: BiometricType,
        phone_id: impl Into<String>,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            nonce: nonce.into(),
            approved: true,
            signature: Some(signature.into()),
            biometric_type,
            phone_id: phone_id.into(),
            error: None,
        }
    }

    /// Create a denied authentication response
    ///
    /// # Parameters
    ///
    /// - `request_id`: Original request ID
    /// - `nonce`: Original nonce from request
    /// - `error`: Error message explaining denial
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::phoneauth::AuthResponse;
    ///
    /// let response = AuthResponse::denied(
    ///     "request-123",
    ///     "nonce-456",
    ///     "User cancelled authentication",
    /// );
    ///
    /// assert!(!response.approved);
    /// assert!(response.signature.is_none());
    /// assert!(response.error.is_some());
    /// ```
    pub fn denied(
        request_id: impl Into<String>,
        nonce: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            nonce: nonce.into(),
            approved: false,
            signature: None,
            biometric_type: BiometricType::None,
            phone_id: String::new(), // Will be set by sender
            error: Some(error.into()),
        }
    }

    /// Set the phone ID (used by phone when creating response)
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::phoneauth::AuthResponse;
    ///
    /// let response = AuthResponse::denied("req", "nonce", "error")
    ///     .with_phone_id("phone-001");
    ///
    /// assert_eq!(response.phone_id, "phone-001");
    /// ```
    pub fn with_phone_id(mut self, phone_id: impl Into<String>) -> Self {
        self.phone_id = phone_id.into();
        self
    }

    /// Convert to network packet
    ///
    /// Creates a `cconnect.auth.response` packet ready to be sent over the network.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::phoneauth::{AuthResponse, BiometricType};
    ///
    /// let response = AuthResponse::approved(
    ///     "request-id",
    ///     "nonce-data",
    ///     "signature-data",
    ///     BiometricType::Face,
    ///     "phone-id",
    /// );
    ///
    /// let packet = response.to_packet();
    /// assert_eq!(packet.packet_type, "cconnect.auth.response");
    /// ```
    pub fn to_packet(&self) -> Packet {
        let mut body = json!({
            "requestId": self.request_id,
            "nonce": self.nonce,
            "approved": self.approved,
            "biometricType": self.biometric_type,
            "phoneId": self.phone_id,
        });

        if let Some(ref signature) = self.signature {
            body["signature"] = json!(signature);
        }

        if let Some(ref error) = self.error {
            body["error"] = json!(error);
        }

        Packet::new(super::PACKET_TYPE_AUTH_RESPONSE, body)
    }

    /// Create from network packet
    ///
    /// Parses a `cconnect.auth.response` packet into an AuthResponse.
    ///
    /// # Errors
    ///
    /// Returns error if packet body cannot be deserialized.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::phoneauth::{AuthResponse, BiometricType};
    /// use cosmic_connect_protocol::Packet;
    /// use serde_json::json;
    ///
    /// let packet = Packet::new("cconnect.auth.response", json!({
    ///     "requestId": "req-123",
    ///     "nonce": "nonce-456",
    ///     "approved": true,
    ///     "signature": "sig-789",
    ///     "biometricType": "fingerprint",
    ///     "phoneId": "phone-001",
    /// }));
    ///
    /// let response = AuthResponse::from_packet(&packet).unwrap();
    /// assert!(response.approved);
    /// ```
    pub fn from_packet(packet: &Packet) -> Result<Self, serde_json::Error> {
        serde_json::from_value(packet.body.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_response_approved() {
        let response = AuthResponse::approved(
            "request-123",
            "nonce-456",
            "signature-789",
            BiometricType::Fingerprint,
            "phone-001",
        );

        assert_eq!(response.request_id, "request-123");
        assert_eq!(response.nonce, "nonce-456");
        assert!(response.approved);
        assert_eq!(response.signature, Some("signature-789".to_string()));
        assert_eq!(response.biometric_type, BiometricType::Fingerprint);
        assert_eq!(response.phone_id, "phone-001");
        assert!(response.error.is_none());
    }

    #[test]
    fn test_auth_response_denied() {
        let response = AuthResponse::denied("request-456", "nonce-789", "User cancelled");

        assert_eq!(response.request_id, "request-456");
        assert_eq!(response.nonce, "nonce-789");
        assert!(!response.approved);
        assert!(response.signature.is_none());
        assert_eq!(response.biometric_type, BiometricType::None);
        assert_eq!(response.error, Some("User cancelled".to_string()));
    }

    #[test]
    fn test_auth_response_with_phone_id() {
        let response = AuthResponse::denied("req", "nonce", "error").with_phone_id("phone-002");

        assert_eq!(response.phone_id, "phone-002");
    }

    #[test]
    fn test_auth_response_approved_to_packet() {
        let response = AuthResponse::approved(
            "req-001",
            "nonce-001",
            "sig-001",
            BiometricType::Face,
            "phone-001",
        );

        let packet = response.to_packet();

        assert_eq!(packet.packet_type, "cconnect.auth.response");
        assert_eq!(
            packet.body.get("requestId").and_then(|v| v.as_str()),
            Some("req-001")
        );
        assert_eq!(
            packet.body.get("nonce").and_then(|v| v.as_str()),
            Some("nonce-001")
        );
        assert_eq!(
            packet.body.get("approved").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            packet.body.get("signature").and_then(|v| v.as_str()),
            Some("sig-001")
        );
        assert_eq!(
            packet.body.get("biometricType").and_then(|v| v.as_str()),
            Some("face")
        );
        assert_eq!(
            packet.body.get("phoneId").and_then(|v| v.as_str()),
            Some("phone-001")
        );
        assert!(packet.body.get("error").is_none());
    }

    #[test]
    fn test_auth_response_denied_to_packet() {
        let response = AuthResponse::denied("req-002", "nonce-002", "Biometric scan failed")
            .with_phone_id("phone-002");

        let packet = response.to_packet();

        assert_eq!(packet.packet_type, "cconnect.auth.response");
        assert_eq!(
            packet.body.get("approved").and_then(|v| v.as_bool()),
            Some(false)
        );
        assert!(packet.body.get("signature").is_none());
        assert_eq!(
            packet.body.get("error").and_then(|v| v.as_str()),
            Some("Biometric scan failed")
        );
    }

    #[test]
    fn test_auth_response_from_packet_approved() {
        let packet = Packet::new(
            "cconnect.auth.response",
            json!({
                "requestId": "from-packet-001",
                "nonce": "packet-nonce-001",
                "approved": true,
                "signature": "packet-sig-001",
                "biometricType": "fingerprint",
                "phoneId": "packet-phone-001",
            }),
        );

        let response = AuthResponse::from_packet(&packet).unwrap();

        assert_eq!(response.request_id, "from-packet-001");
        assert_eq!(response.nonce, "packet-nonce-001");
        assert!(response.approved);
        assert_eq!(response.signature, Some("packet-sig-001".to_string()));
        assert_eq!(response.biometric_type, BiometricType::Fingerprint);
        assert_eq!(response.phone_id, "packet-phone-001");
        assert!(response.error.is_none());
    }

    #[test]
    fn test_auth_response_from_packet_denied() {
        let packet = Packet::new(
            "cconnect.auth.response",
            json!({
                "requestId": "from-packet-002",
                "nonce": "packet-nonce-002",
                "approved": false,
                "biometricType": "none",
                "phoneId": "packet-phone-002",
                "error": "Timeout",
            }),
        );

        let response = AuthResponse::from_packet(&packet).unwrap();

        assert_eq!(response.request_id, "from-packet-002");
        assert!(!response.approved);
        assert!(response.signature.is_none());
        assert_eq!(response.error, Some("Timeout".to_string()));
    }

    #[test]
    fn test_auth_response_roundtrip_approved() {
        let original = AuthResponse::approved(
            "roundtrip-001",
            "roundtrip-nonce",
            "roundtrip-sig",
            BiometricType::Pin,
            "roundtrip-phone",
        );

        let packet = original.to_packet();
        let decoded = AuthResponse::from_packet(&packet).unwrap();

        assert_eq!(original, decoded);
    }

    #[test]
    fn test_auth_response_roundtrip_denied() {
        let original =
            AuthResponse::denied("roundtrip-002", "nonce", "Device locked").with_phone_id("phone");

        let packet = original.to_packet();
        let decoded = AuthResponse::from_packet(&packet).unwrap();

        assert_eq!(original, decoded);
    }

    #[test]
    fn test_auth_response_serialization_approved() {
        let response = AuthResponse::approved(
            "serialize-001",
            "serialize-nonce",
            "serialize-sig",
            BiometricType::Face,
            "serialize-phone",
        );

        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["requestId"], "serialize-001");
        assert_eq!(json["nonce"], "serialize-nonce");
        assert_eq!(json["approved"], true);
        assert_eq!(json["signature"], "serialize-sig");
        assert_eq!(json["biometricType"], "face");
        assert_eq!(json["phoneId"], "serialize-phone");
        assert!(json.get("error").is_none() || json["error"].is_null());
    }

    #[test]
    fn test_auth_response_serialization_denied() {
        let response = AuthResponse::denied("serialize-002", "nonce", "Too many attempts")
            .with_phone_id("phone");

        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["approved"], false);
        assert!(json.get("signature").is_none() || json["signature"].is_null());
        assert_eq!(json["error"], "Too many attempts");
    }

    #[test]
    fn test_auth_response_deserialization() {
        let json = json!({
            "requestId": "deserialize-001",
            "nonce": "deserialize-nonce",
            "approved": true,
            "signature": "deserialize-sig",
            "biometricType": "fingerprint",
            "phoneId": "deserialize-phone",
        });

        let response: AuthResponse = serde_json::from_value(json).unwrap();

        assert_eq!(response.request_id, "deserialize-001");
        assert_eq!(response.nonce, "deserialize-nonce");
        assert!(response.approved);
        assert_eq!(response.signature, Some("deserialize-sig".to_string()));
        assert_eq!(response.biometric_type, BiometricType::Fingerprint);
        assert_eq!(response.phone_id, "deserialize-phone");
    }
}
