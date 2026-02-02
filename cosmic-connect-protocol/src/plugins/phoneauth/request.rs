//! Authentication Request Packet
//!
//! Sent from desktop to phone to request biometric authentication.

use crate::Packet;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Type of authentication being requested
///
/// Determines the context and UI presentation on the phone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    /// Desktop login/unlock screen
    Unlock,
    /// Sudo command authorization
    Sudo,
    /// Polkit privilege escalation dialog
    Polkit,
}

impl AuthType {
    /// Get human-readable description of the auth type
    pub fn description(&self) -> &'static str {
        match self {
            AuthType::Unlock => "Desktop unlock",
            AuthType::Sudo => "Administrator command",
            AuthType::Polkit => "System authorization",
        }
    }
}

/// Authentication request packet
///
/// Sent from desktop to phone to request biometric authentication.
/// The phone will display the prompt and request biometric verification.
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_protocol::plugins::phoneauth::{AuthRequest, AuthType};
///
/// let request = AuthRequest {
///     request_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
///     username: "alice".to_string(),
///     auth_type: AuthType::Unlock,
///     challenge: "base64_encoded_challenge_32_bytes".to_string(),
///     nonce: "base64_encoded_nonce_16_bytes".to_string(),
///     timestamp: 1706745600,
///     desktop_id: "cosmic-desktop-abc123".to_string(),
///     prompt: "Unlock COSMIC Desktop for alice".to_string(),
/// };
///
/// assert_eq!(request.auth_type, AuthType::Unlock);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthRequest {
    /// Unique request identifier (UUID format recommended)
    #[serde(rename = "requestId")]
    pub request_id: String,

    /// Username requesting authentication
    pub username: String,

    /// Type of authentication: unlock, sudo, or polkit
    #[serde(rename = "authType")]
    pub auth_type: AuthType,

    /// Cryptographic challenge (base64-encoded, 32 bytes)
    pub challenge: String,

    /// Nonce for replay prevention (base64-encoded, 16 bytes)
    pub nonce: String,

    /// Request timestamp (Unix epoch milliseconds)
    pub timestamp: u64,

    /// Desktop device identifier
    #[serde(rename = "desktopId")]
    pub desktop_id: String,

    /// Human-readable prompt for phone UI
    pub prompt: String,
}

impl AuthRequest {
    /// Create a new authentication request
    ///
    /// # Parameters
    ///
    /// - `request_id`: Unique identifier for this request (UUID recommended)
    /// - `username`: The username requesting authentication
    /// - `auth_type`: Type of authentication (unlock, sudo, polkit)
    /// - `challenge`: Base64-encoded challenge bytes (32 bytes)
    /// - `nonce`: Base64-encoded nonce bytes (16 bytes)
    /// - `timestamp`: Unix epoch milliseconds
    /// - `desktop_id`: Desktop device identifier
    /// - `prompt`: Human-readable prompt text
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::phoneauth::{AuthRequest, AuthType};
    ///
    /// let request = AuthRequest::new(
    ///     "550e8400-e29b-41d4-a716-446655440000",
    ///     "alice",
    ///     AuthType::Unlock,
    ///     "base64_challenge",
    ///     "base64_nonce",
    ///     1706745600,
    ///     "cosmic-desktop-abc123",
    ///     "Unlock COSMIC Desktop for alice",
    /// );
    /// ```
    pub fn new(
        request_id: impl Into<String>,
        username: impl Into<String>,
        auth_type: AuthType,
        challenge: impl Into<String>,
        nonce: impl Into<String>,
        timestamp: u64,
        desktop_id: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            username: username.into(),
            auth_type,
            challenge: challenge.into(),
            nonce: nonce.into(),
            timestamp,
            desktop_id: desktop_id.into(),
            prompt: prompt.into(),
        }
    }

    /// Convert to network packet
    ///
    /// Creates a `cconnect.auth.request` packet ready to be sent over the network.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::phoneauth::{AuthRequest, AuthType};
    ///
    /// let request = AuthRequest::new(
    ///     "550e8400-e29b-41d4-a716-446655440000",
    ///     "alice",
    ///     AuthType::Unlock,
    ///     "base64_challenge",
    ///     "base64_nonce",
    ///     1706745600,
    ///     "cosmic-desktop",
    ///     "Unlock desktop",
    /// );
    ///
    /// let packet = request.to_packet();
    /// assert_eq!(packet.packet_type, "cconnect.auth.request");
    /// ```
    pub fn to_packet(&self) -> Packet {
        let body = json!({
            "requestId": self.request_id,
            "username": self.username,
            "authType": self.auth_type,
            "challenge": self.challenge,
            "nonce": self.nonce,
            "timestamp": self.timestamp,
            "desktopId": self.desktop_id,
            "prompt": self.prompt,
        });

        Packet::new(super::PACKET_TYPE_AUTH_REQUEST, body)
    }

    /// Create from network packet
    ///
    /// Parses a `cconnect.auth.request` packet into an AuthRequest.
    ///
    /// # Errors
    ///
    /// Returns error if packet body cannot be deserialized.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::phoneauth::{AuthRequest, AuthType};
    /// use cosmic_connect_protocol::Packet;
    /// use serde_json::json;
    ///
    /// let packet = Packet::new("cconnect.auth.request", json!({
    ///     "requestId": "550e8400-e29b-41d4-a716-446655440000",
    ///     "username": "alice",
    ///     "authType": "unlock",
    ///     "challenge": "base64_challenge",
    ///     "nonce": "base64_nonce",
    ///     "timestamp": 1706745600,
    ///     "desktopId": "cosmic-desktop",
    ///     "prompt": "Unlock desktop",
    /// }));
    ///
    /// let request = AuthRequest::from_packet(&packet).unwrap();
    /// assert_eq!(request.username, "alice");
    /// assert_eq!(request.auth_type, AuthType::Unlock);
    /// ```
    pub fn from_packet(packet: &Packet) -> Result<Self, serde_json::Error> {
        serde_json::from_value(packet.body.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_type_serialization() {
        assert_eq!(
            serde_json::to_string(&AuthType::Unlock).unwrap(),
            r#""unlock""#
        );
        assert_eq!(serde_json::to_string(&AuthType::Sudo).unwrap(), r#""sudo""#);
        assert_eq!(
            serde_json::to_string(&AuthType::Polkit).unwrap(),
            r#""polkit""#
        );
    }

    #[test]
    fn test_auth_type_deserialization() {
        assert_eq!(
            serde_json::from_str::<AuthType>(r#""unlock""#).unwrap(),
            AuthType::Unlock
        );
        assert_eq!(
            serde_json::from_str::<AuthType>(r#""sudo""#).unwrap(),
            AuthType::Sudo
        );
        assert_eq!(
            serde_json::from_str::<AuthType>(r#""polkit""#).unwrap(),
            AuthType::Polkit
        );
    }

    #[test]
    fn test_auth_type_description() {
        assert_eq!(AuthType::Unlock.description(), "Desktop unlock");
        assert_eq!(AuthType::Sudo.description(), "Administrator command");
        assert_eq!(AuthType::Polkit.description(), "System authorization");
    }

    #[test]
    fn test_auth_request_new() {
        let request = AuthRequest::new(
            "550e8400-e29b-41d4-a716-446655440000",
            "alice",
            AuthType::Unlock,
            "base64_challenge",
            "base64_nonce",
            1706745600,
            "cosmic-desktop-abc123",
            "Unlock COSMIC Desktop",
        );

        assert_eq!(request.request_id, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(request.username, "alice");
        assert_eq!(request.auth_type, AuthType::Unlock);
        assert_eq!(request.challenge, "base64_challenge");
        assert_eq!(request.nonce, "base64_nonce");
        assert_eq!(request.timestamp, 1706745600);
        assert_eq!(request.desktop_id, "cosmic-desktop-abc123");
        assert_eq!(request.prompt, "Unlock COSMIC Desktop");
    }

    #[test]
    fn test_auth_request_to_packet() {
        let request = AuthRequest::new(
            "550e8400-e29b-41d4-a716-446655440000",
            "alice",
            AuthType::Sudo,
            "challenge123",
            "nonce456",
            1706745600,
            "desktop-001",
            "Run sudo command",
        );

        let packet = request.to_packet();

        assert_eq!(packet.packet_type, "cconnect.auth.request");
        assert_eq!(
            packet.body.get("requestId").and_then(|v| v.as_str()),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );
        assert_eq!(
            packet.body.get("username").and_then(|v| v.as_str()),
            Some("alice")
        );
        assert_eq!(
            packet.body.get("authType").and_then(|v| v.as_str()),
            Some("sudo")
        );
        assert_eq!(
            packet.body.get("challenge").and_then(|v| v.as_str()),
            Some("challenge123")
        );
        assert_eq!(
            packet.body.get("nonce").and_then(|v| v.as_str()),
            Some("nonce456")
        );
        assert_eq!(
            packet.body.get("timestamp").and_then(|v| v.as_u64()),
            Some(1706745600)
        );
        assert_eq!(
            packet.body.get("desktopId").and_then(|v| v.as_str()),
            Some("desktop-001")
        );
        assert_eq!(
            packet.body.get("prompt").and_then(|v| v.as_str()),
            Some("Run sudo command")
        );
    }

    #[test]
    fn test_auth_request_from_packet() {
        let packet = Packet::new(
            "cconnect.auth.request",
            json!({
                "requestId": "test-request-id",
                "username": "bob",
                "authType": "polkit",
                "challenge": "challenge_data",
                "nonce": "nonce_data",
                "timestamp": 1706745700,
                "desktopId": "desktop-002",
                "prompt": "Authorize system change",
            }),
        );

        let request = AuthRequest::from_packet(&packet).unwrap();

        assert_eq!(request.request_id, "test-request-id");
        assert_eq!(request.username, "bob");
        assert_eq!(request.auth_type, AuthType::Polkit);
        assert_eq!(request.challenge, "challenge_data");
        assert_eq!(request.nonce, "nonce_data");
        assert_eq!(request.timestamp, 1706745700);
        assert_eq!(request.desktop_id, "desktop-002");
        assert_eq!(request.prompt, "Authorize system change");
    }

    #[test]
    fn test_auth_request_roundtrip() {
        let original = AuthRequest::new(
            "roundtrip-test-id",
            "charlie",
            AuthType::Unlock,
            "challenge_roundtrip",
            "nonce_roundtrip",
            1706745800,
            "desktop-roundtrip",
            "Test roundtrip",
        );

        let packet = original.to_packet();
        let decoded = AuthRequest::from_packet(&packet).unwrap();

        assert_eq!(original, decoded);
    }

    #[test]
    fn test_auth_request_serialization() {
        let request = AuthRequest::new(
            "serialize-test",
            "dave",
            AuthType::Sudo,
            "test_challenge",
            "test_nonce",
            1706745900,
            "desktop-serialize",
            "Serialization test",
        );

        let json = serde_json::to_value(&request).unwrap();

        assert_eq!(json["requestId"], "serialize-test");
        assert_eq!(json["username"], "dave");
        assert_eq!(json["authType"], "sudo");
        assert_eq!(json["challenge"], "test_challenge");
        assert_eq!(json["nonce"], "test_nonce");
        assert_eq!(json["timestamp"], 1706745900);
        assert_eq!(json["desktopId"], "desktop-serialize");
        assert_eq!(json["prompt"], "Serialization test");
    }

    #[test]
    fn test_auth_request_deserialization() {
        let json = json!({
            "requestId": "deserialize-test",
            "username": "eve",
            "authType": "unlock",
            "challenge": "deserialize_challenge",
            "nonce": "deserialize_nonce",
            "timestamp": 1706746000,
            "desktopId": "desktop-deserialize",
            "prompt": "Deserialization test",
        });

        let request: AuthRequest = serde_json::from_value(json).unwrap();

        assert_eq!(request.request_id, "deserialize-test");
        assert_eq!(request.username, "eve");
        assert_eq!(request.auth_type, AuthType::Unlock);
        assert_eq!(request.challenge, "deserialize_challenge");
        assert_eq!(request.nonce, "deserialize_nonce");
        assert_eq!(request.timestamp, 1706746000);
        assert_eq!(request.desktop_id, "desktop-deserialize");
        assert_eq!(request.prompt, "Deserialization test");
    }
}
