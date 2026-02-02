//! Authentication Capabilities Packet
//!
//! Phone advertises its authentication capabilities to the desktop.

use crate::Packet;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Biometric authentication method
///
/// Indicates which biometric method was used (or will be used) for authentication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BiometricType {
    /// Fingerprint sensor
    Fingerprint,
    /// Face recognition
    Face,
    /// PIN code
    Pin,
    /// No biometric method (denied or unavailable)
    None,
}

impl BiometricType {
    /// Get human-readable description of the biometric type
    pub fn description(&self) -> &'static str {
        match self {
            BiometricType::Fingerprint => "Fingerprint",
            BiometricType::Face => "Face recognition",
            BiometricType::Pin => "PIN code",
            BiometricType::None => "None",
        }
    }

    /// Check if this is an actual biometric method (not None or Pin)
    pub fn is_biometric(&self) -> bool {
        matches!(self, BiometricType::Fingerprint | BiometricType::Face)
    }
}

/// Authentication capabilities packet
///
/// Sent from phone to desktop to advertise authentication capabilities.
/// The desktop uses this to determine if phone-based authentication is available.
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_protocol::plugins::phoneauth::{AuthCapabilities, BiometricType};
///
/// let capabilities = AuthCapabilities {
///     biometrics: vec![BiometricType::Fingerprint, BiometricType::Face],
///     public_key: "base64_encoded_ed25519_public_key".to_string(),
///     max_timeout_ms: 60000,
///     device_locked: false,
/// };
///
/// assert!(capabilities.supports_biometric());
/// assert!(capabilities.has_biometric(BiometricType::Fingerprint));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthCapabilities {
    /// Supported biometric types on this device
    pub biometrics: Vec<BiometricType>,

    /// Ed25519 public key (base64-encoded, 32 bytes)
    #[serde(rename = "publicKey")]
    pub public_key: String,

    /// Maximum timeout the phone will wait for user action (milliseconds)
    #[serde(rename = "maxTimeoutMs")]
    pub max_timeout_ms: u64,

    /// Whether the phone device is currently locked
    #[serde(rename = "deviceLocked")]
    pub device_locked: bool,
}

impl AuthCapabilities {
    /// Create new authentication capabilities
    ///
    /// # Parameters
    ///
    /// - `biometrics`: List of supported biometric types
    /// - `public_key`: Base64-encoded Ed25519 public key (32 bytes)
    /// - `max_timeout_ms`: Maximum timeout in milliseconds
    /// - `device_locked`: Whether device is currently locked
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::phoneauth::{AuthCapabilities, BiometricType};
    ///
    /// let capabilities = AuthCapabilities::new(
    ///     vec![BiometricType::Fingerprint, BiometricType::Pin],
    ///     "base64_public_key",
    ///     30000,
    ///     false,
    /// );
    ///
    /// assert_eq!(capabilities.biometrics.len(), 2);
    /// ```
    pub fn new(
        biometrics: Vec<BiometricType>,
        public_key: impl Into<String>,
        max_timeout_ms: u64,
        device_locked: bool,
    ) -> Self {
        Self {
            biometrics,
            public_key: public_key.into(),
            max_timeout_ms,
            device_locked,
        }
    }

    /// Check if device supports any biometric authentication
    ///
    /// Returns true if at least one actual biometric method is available
    /// (excludes PIN and None).
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::phoneauth::{AuthCapabilities, BiometricType};
    ///
    /// let with_bio = AuthCapabilities::new(
    ///     vec![BiometricType::Fingerprint],
    ///     "key",
    ///     30000,
    ///     false,
    /// );
    /// assert!(with_bio.supports_biometric());
    ///
    /// let pin_only = AuthCapabilities::new(
    ///     vec![BiometricType::Pin],
    ///     "key",
    ///     30000,
    ///     false,
    /// );
    /// assert!(!pin_only.supports_biometric());
    /// ```
    pub fn supports_biometric(&self) -> bool {
        self.biometrics.iter().any(|b| b.is_biometric())
    }

    /// Check if a specific biometric type is supported
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::phoneauth::{AuthCapabilities, BiometricType};
    ///
    /// let capabilities = AuthCapabilities::new(
    ///     vec![BiometricType::Fingerprint, BiometricType::Face],
    ///     "key",
    ///     30000,
    ///     false,
    /// );
    ///
    /// assert!(capabilities.has_biometric(BiometricType::Fingerprint));
    /// assert!(capabilities.has_biometric(BiometricType::Face));
    /// assert!(!capabilities.has_biometric(BiometricType::Pin));
    /// ```
    pub fn has_biometric(&self, biometric_type: BiometricType) -> bool {
        self.biometrics.contains(&biometric_type)
    }

    /// Check if device is available for authentication
    ///
    /// Returns true if device is not locked and has biometric capabilities.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::phoneauth::{AuthCapabilities, BiometricType};
    ///
    /// let available = AuthCapabilities::new(
    ///     vec![BiometricType::Fingerprint],
    ///     "key",
    ///     30000,
    ///     false,
    /// );
    /// assert!(available.is_available());
    ///
    /// let locked = AuthCapabilities::new(
    ///     vec![BiometricType::Fingerprint],
    ///     "key",
    ///     30000,
    ///     true,
    /// );
    /// assert!(!locked.is_available());
    /// ```
    pub fn is_available(&self) -> bool {
        !self.device_locked && self.supports_biometric()
    }

    /// Convert to network packet
    ///
    /// Creates a `cconnect.auth.capabilities` packet ready to be sent over the network.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::phoneauth::{AuthCapabilities, BiometricType};
    ///
    /// let capabilities = AuthCapabilities::new(
    ///     vec![BiometricType::Fingerprint],
    ///     "public_key_base64",
    ///     45000,
    ///     false,
    /// );
    ///
    /// let packet = capabilities.to_packet();
    /// assert_eq!(packet.packet_type, "cconnect.auth.capabilities");
    /// ```
    pub fn to_packet(&self) -> Packet {
        let body = json!({
            "biometrics": self.biometrics,
            "publicKey": self.public_key,
            "maxTimeoutMs": self.max_timeout_ms,
            "deviceLocked": self.device_locked,
        });

        Packet::new(super::PACKET_TYPE_AUTH_CAPABILITIES, body)
    }

    /// Create from network packet
    ///
    /// Parses a `cconnect.auth.capabilities` packet into AuthCapabilities.
    ///
    /// # Errors
    ///
    /// Returns error if packet body cannot be deserialized.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::phoneauth::{AuthCapabilities, BiometricType};
    /// use cosmic_connect_protocol::Packet;
    /// use serde_json::json;
    ///
    /// let packet = Packet::new("cconnect.auth.capabilities", json!({
    ///     "biometrics": ["fingerprint", "face"],
    ///     "publicKey": "base64_key",
    ///     "maxTimeoutMs": 60000,
    ///     "deviceLocked": false,
    /// }));
    ///
    /// let capabilities = AuthCapabilities::from_packet(&packet).unwrap();
    /// assert_eq!(capabilities.biometrics.len(), 2);
    /// ```
    pub fn from_packet(packet: &Packet) -> Result<Self, serde_json::Error> {
        serde_json::from_value(packet.body.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_biometric_type_serialization() {
        assert_eq!(
            serde_json::to_string(&BiometricType::Fingerprint).unwrap(),
            r#""fingerprint""#
        );
        assert_eq!(
            serde_json::to_string(&BiometricType::Face).unwrap(),
            r#""face""#
        );
        assert_eq!(
            serde_json::to_string(&BiometricType::Pin).unwrap(),
            r#""pin""#
        );
        assert_eq!(
            serde_json::to_string(&BiometricType::None).unwrap(),
            r#""none""#
        );
    }

    #[test]
    fn test_biometric_type_deserialization() {
        assert_eq!(
            serde_json::from_str::<BiometricType>(r#""fingerprint""#).unwrap(),
            BiometricType::Fingerprint
        );
        assert_eq!(
            serde_json::from_str::<BiometricType>(r#""face""#).unwrap(),
            BiometricType::Face
        );
        assert_eq!(
            serde_json::from_str::<BiometricType>(r#""pin""#).unwrap(),
            BiometricType::Pin
        );
        assert_eq!(
            serde_json::from_str::<BiometricType>(r#""none""#).unwrap(),
            BiometricType::None
        );
    }

    #[test]
    fn test_biometric_type_description() {
        assert_eq!(BiometricType::Fingerprint.description(), "Fingerprint");
        assert_eq!(BiometricType::Face.description(), "Face recognition");
        assert_eq!(BiometricType::Pin.description(), "PIN code");
        assert_eq!(BiometricType::None.description(), "None");
    }

    #[test]
    fn test_biometric_type_is_biometric() {
        assert!(BiometricType::Fingerprint.is_biometric());
        assert!(BiometricType::Face.is_biometric());
        assert!(!BiometricType::Pin.is_biometric());
        assert!(!BiometricType::None.is_biometric());
    }

    #[test]
    fn test_auth_capabilities_new() {
        let capabilities = AuthCapabilities::new(
            vec![BiometricType::Fingerprint, BiometricType::Face],
            "test_public_key",
            60000,
            false,
        );

        assert_eq!(capabilities.biometrics.len(), 2);
        assert_eq!(capabilities.public_key, "test_public_key");
        assert_eq!(capabilities.max_timeout_ms, 60000);
        assert!(!capabilities.device_locked);
    }

    #[test]
    fn test_auth_capabilities_supports_biometric() {
        let with_bio = AuthCapabilities::new(
            vec![BiometricType::Fingerprint, BiometricType::Face],
            "key",
            30000,
            false,
        );
        assert!(with_bio.supports_biometric());

        let pin_only = AuthCapabilities::new(vec![BiometricType::Pin], "key", 30000, false);
        assert!(!pin_only.supports_biometric());

        let none = AuthCapabilities::new(vec![BiometricType::None], "key", 30000, false);
        assert!(!none.supports_biometric());

        let empty = AuthCapabilities::new(vec![], "key", 30000, false);
        assert!(!empty.supports_biometric());
    }

    #[test]
    fn test_auth_capabilities_has_biometric() {
        let capabilities = AuthCapabilities::new(
            vec![BiometricType::Fingerprint, BiometricType::Pin],
            "key",
            30000,
            false,
        );

        assert!(capabilities.has_biometric(BiometricType::Fingerprint));
        assert!(capabilities.has_biometric(BiometricType::Pin));
        assert!(!capabilities.has_biometric(BiometricType::Face));
        assert!(!capabilities.has_biometric(BiometricType::None));
    }

    #[test]
    fn test_auth_capabilities_is_available() {
        let available =
            AuthCapabilities::new(vec![BiometricType::Fingerprint], "key", 30000, false);
        assert!(available.is_available());

        let locked = AuthCapabilities::new(vec![BiometricType::Fingerprint], "key", 30000, true);
        assert!(!locked.is_available());

        let no_bio = AuthCapabilities::new(vec![BiometricType::Pin], "key", 30000, false);
        assert!(!no_bio.is_available());

        let locked_no_bio = AuthCapabilities::new(vec![BiometricType::None], "key", 30000, true);
        assert!(!locked_no_bio.is_available());
    }

    #[test]
    fn test_auth_capabilities_to_packet() {
        let capabilities = AuthCapabilities::new(
            vec![BiometricType::Fingerprint, BiometricType::Face],
            "test_public_key",
            45000,
            false,
        );

        let packet = capabilities.to_packet();

        assert_eq!(packet.packet_type, "cconnect.auth.capabilities");
        assert_eq!(
            packet.body.get("publicKey").and_then(|v| v.as_str()),
            Some("test_public_key")
        );
        assert_eq!(
            packet.body.get("maxTimeoutMs").and_then(|v| v.as_u64()),
            Some(45000)
        );
        assert_eq!(
            packet.body.get("deviceLocked").and_then(|v| v.as_bool()),
            Some(false)
        );

        let biometrics = packet.body.get("biometrics").and_then(|v| v.as_array());
        assert!(biometrics.is_some());
        assert_eq!(biometrics.unwrap().len(), 2);
    }

    #[test]
    fn test_auth_capabilities_from_packet() {
        let packet = Packet::new(
            "cconnect.auth.capabilities",
            json!({
                "biometrics": ["fingerprint", "pin"],
                "publicKey": "packet_key",
                "maxTimeoutMs": 50000,
                "deviceLocked": true,
            }),
        );

        let capabilities = AuthCapabilities::from_packet(&packet).unwrap();

        assert_eq!(capabilities.biometrics.len(), 2);
        assert!(capabilities.has_biometric(BiometricType::Fingerprint));
        assert!(capabilities.has_biometric(BiometricType::Pin));
        assert_eq!(capabilities.public_key, "packet_key");
        assert_eq!(capabilities.max_timeout_ms, 50000);
        assert!(capabilities.device_locked);
    }

    #[test]
    fn test_auth_capabilities_roundtrip() {
        let original = AuthCapabilities::new(
            vec![
                BiometricType::Fingerprint,
                BiometricType::Face,
                BiometricType::Pin,
            ],
            "roundtrip_key",
            60000,
            false,
        );

        let packet = original.to_packet();
        let decoded = AuthCapabilities::from_packet(&packet).unwrap();

        assert_eq!(original, decoded);
    }

    #[test]
    fn test_auth_capabilities_serialization() {
        let capabilities =
            AuthCapabilities::new(vec![BiometricType::Face], "serialize_key", 40000, true);

        let json = serde_json::to_value(&capabilities).unwrap();

        assert_eq!(json["publicKey"], "serialize_key");
        assert_eq!(json["maxTimeoutMs"], 40000);
        assert_eq!(json["deviceLocked"], true);

        let biometrics = json["biometrics"].as_array().unwrap();
        assert_eq!(biometrics.len(), 1);
        assert_eq!(biometrics[0], "face");
    }

    #[test]
    fn test_auth_capabilities_deserialization() {
        let json = json!({
            "biometrics": ["fingerprint", "face", "pin"],
            "publicKey": "deserialize_key",
            "maxTimeoutMs": 70000,
            "deviceLocked": false,
        });

        let capabilities: AuthCapabilities = serde_json::from_value(json).unwrap();

        assert_eq!(capabilities.biometrics.len(), 3);
        assert_eq!(capabilities.public_key, "deserialize_key");
        assert_eq!(capabilities.max_timeout_ms, 70000);
        assert!(!capabilities.device_locked);
    }

    #[test]
    fn test_auth_capabilities_empty_biometrics() {
        let capabilities = AuthCapabilities::new(vec![], "key", 30000, false);

        assert!(!capabilities.supports_biometric());
        assert!(!capabilities.is_available());
        assert!(!capabilities.has_biometric(BiometricType::Fingerprint));
    }
}
