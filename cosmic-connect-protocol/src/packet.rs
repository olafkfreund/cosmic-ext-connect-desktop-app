//! CConnect Network Packet
//!
//! This module implements the core packet structure for the CConnect protocol.
//! Packets are JSON-formatted messages with a newline terminator.
//!
//! ## Packet Structure
//!
//! Each packet contains:
//! - `id`: UNIX epoch timestamp in milliseconds
//! - `type`: Packet type in format `cconnect.<plugin>[.<action>]`
//! - `body`: JSON dictionary of plugin-specific parameters
//! - `payloadSize`: (optional) Size of payload data in bytes
//! - `payloadTransferInfo`: (optional) Transfer negotiation parameters
//!
//! ## References
//! - [Valent Protocol Reference](https://valent.andyholmes.ca/documentation/protocol.html)
//! - [CConnect Repository](https://invent.kde.org/network/cconnect-kde)

use crate::{ProtocolError, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Represents a CConnect network packet
///
/// # Examples
///
/// ```
/// use cosmic_connect_core::Packet;
/// use serde_json::json;
///
/// // Create identity packet
/// let packet = Packet::new(
///     "cconnect.identity",
///     json!({
///         "deviceId": "my-device-id",
///         "deviceName": "My Computer",
///         "protocolVersion": 7,
///         "deviceType": "desktop"
///     })
/// );
///
/// // Serialize to bytes
/// let bytes = packet.to_bytes().unwrap();
///
/// // Deserialize from bytes
/// let parsed = Packet::from_bytes(&bytes).unwrap();
/// assert_eq!(parsed.packet_type, "cconnect.identity");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Packet {
    /// UNIX timestamp in milliseconds
    /// Note: Some CConnect clients may send this as a string
    #[serde(deserialize_with = "deserialize_id", serialize_with = "serialize_id")]
    pub id: i64,

    /// Packet type in format: kdeconnect.<plugin>[.<action>]
    ///
    /// Examples: "cconnect.battery", "cconnect.mpris.request"
    #[serde(rename = "type")]
    pub packet_type: String,

    /// Plugin-specific parameters
    #[serde(default)]
    pub body: Value,

    /// Optional payload size in bytes (-1 for indefinite streams)
    #[serde(rename = "payloadSize", skip_serializing_if = "Option::is_none")]
    pub payload_size: Option<i64>,

    /// Optional payload transfer negotiation info
    #[serde(
        rename = "payloadTransferInfo",
        skip_serializing_if = "Option::is_none"
    )]
    pub payload_transfer_info: Option<HashMap<String, Value>>,
}

impl Packet {
    /// Convert to cosmic-connect-core::Packet for TLS communication
    pub fn to_core_packet(&self) -> cosmic_connect_core::Packet {
        cosmic_connect_core::Packet {
            id: self.id,
            packet_type: self.packet_type.clone(),
            body: self.body.clone(),
            payload_size: self.payload_size,
            payload_transfer_info: self.payload_transfer_info.clone(),
        }
    }

    /// Convert from cosmic-connect-core::Packet
    pub fn from_core_packet(packet: cosmic_connect_core::Packet) -> Self {
        Self {
            id: packet.id,
            packet_type: packet.packet_type,
            body: packet.body,
            payload_size: packet.payload_size,
            payload_transfer_info: packet.payload_transfer_info,
        }
    }

    /// Creates a new packet with the specified type and body
    ///
    /// The packet ID is automatically set to the current timestamp in milliseconds.
    ///
    /// # Arguments
    ///
    /// * `packet_type` - Packet type string (e.g., "cconnect.battery")
    /// * `body` - JSON value containing packet parameters
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmic_connect_core::Packet;
    /// use serde_json::json;
    ///
    /// let packet = Packet::new("cconnect.ping", json!({}));
    /// ```
    pub fn new(packet_type: impl Into<String>, body: Value) -> Self {
        Self {
            id: current_timestamp(),
            packet_type: packet_type.into(),
            body,
            payload_size: None,
            payload_transfer_info: None,
        }
    }

    /// Create a new packet with an explicit timestamp
    ///
    /// Useful for testing or when you need specific timestamp control
    pub fn with_id(id: i64, packet_type: impl Into<String>, body: Value) -> Self {
        Self {
            id,
            packet_type: packet_type.into(),
            body,
            payload_size: None,
            payload_transfer_info: None,
        }
    }

    /// Serialize packet to bytes with newline terminator
    ///
    /// CConnect packets are JSON-formatted and terminated with a single
    /// newline character (`\n`). This format allows packets to be easily
    /// delimited when sent over TCP streams.
    ///
    /// # Errors
    ///
    /// Returns `ProtocolError::Json` if serialization fails
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmic_connect_core::Packet;
    /// use serde_json::json;
    ///
    /// let packet = Packet::new("cconnect.ping", json!({}));
    /// let bytes = packet.to_bytes().unwrap();
    ///
    /// // Packet ends with newline
    /// assert_eq!(bytes.last(), Some(&b'\n'));
    /// ```
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let json = serde_json::to_string(self)?;
        let mut bytes = json.into_bytes();
        // Add newline terminator as per CConnect protocol specification
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Deserialize a packet from bytes
    ///
    /// Accepts both newline-terminated and non-terminated JSON.
    /// Some implementations may send `\r\n` (CRLF) or `\n` (LF) terminators.
    ///
    /// # Errors
    ///
    /// Returns `ProtocolError::InvalidPacket` if the data is not valid JSON
    /// or doesn't conform to the packet structure.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmic_connect_core::Packet;
    ///
    /// let json_data = r#"{"id":123456789,"type":"cconnect.ping","body":{}}"#;
    /// let packet = Packet::from_bytes(json_data.as_bytes()).unwrap();
    /// assert_eq!(packet.packet_type, "cconnect.ping");
    /// ```
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        // Strip trailing newline if present (handles both \n and \r\n)
        let trimmed = data
            .strip_suffix(b"\r\n")
            .or_else(|| data.strip_suffix(b"\n"))
            .unwrap_or(data);

        serde_json::from_slice(trimmed).map_err(|e| {
            ProtocolError::InvalidPacket(format!("Failed to deserialize packet: {}", e))
        })
    }

    /// Builder pattern: Set payload size
    pub fn with_payload_size(mut self, size: i64) -> Self {
        self.payload_size = Some(size);
        self
    }

    /// Builder pattern: Set payload transfer info
    pub fn with_payload_transfer_info(mut self, info: HashMap<String, Value>) -> Self {
        self.payload_transfer_info = Some(info);
        self
    }

    /// Builder pattern: Add a key-value pair to the body
    pub fn with_body_field(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        if let Value::Object(ref mut map) = self.body {
            map.insert(key.into(), value.into());
        }
        self
    }

    /// Check if packet is of a specific type
    pub fn is_type(&self, packet_type: &str) -> bool {
        if self.packet_type == packet_type {
            return true;
        }

        // Handle COSMIC/KDE prefixes interchangeably
        if packet_type.starts_with("cconnect.") {
            let kde_type = packet_type.replace("cconnect.", "kdeconnect.");
            if self.packet_type == kde_type {
                return true;
            }
        } else if packet_type.starts_with("kdeconnect.") {
            let c_type = packet_type.replace("kdeconnect.", "cconnect.");
            if self.packet_type == c_type {
                return true;
            }
        }

        false
    }

    /// Get a field from the body as a specific type
    pub fn get_body_field<T>(&self, key: &str) -> Option<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.body
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

/// Custom deserializer for the `id` field to handle both string and number formats
fn deserialize_id<'de, D>(deserializer: D) -> std::result::Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    let value: Value = Deserialize::deserialize(deserializer)?;
    match value {
        Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| Error::custom("Invalid number for id")),
        Value::String(s) => s
            .parse::<i64>()
            .map_err(|_| Error::custom("Invalid string for id")),
        _ => Err(Error::custom("id must be a number or string")),
    }
}

/// Custom serializer for the `id` field - always serialize as a number
fn serialize_id<S>(id: &i64, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_i64(*id)
}

/// Generate current UNIX timestamp in milliseconds
pub fn current_timestamp() -> i64 {
    Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_new_packet() {
        let packet = Packet::new("cconnect.ping", json!({}));
        assert_eq!(packet.packet_type, "cconnect.ping");
        assert!(packet.body.is_object());
        assert!(packet.id > 0);
    }

    #[test]
    fn test_packet_serialization() {
        let packet = Packet::new(
            "cconnect.identity",
            json!({
                "deviceId": "test-device",
                "deviceName": "Test Device",
                "protocolVersion": 7
            }),
        );

        let bytes = packet.to_bytes().unwrap();
        let json_str = String::from_utf8_lossy(&bytes);

        // Check that it ends with newline
        assert!(json_str.ends_with('\n'));

        // Check that it's valid JSON (without the newline)
        let json_part = json_str.trim_end();
        assert!(serde_json::from_str::<Value>(json_part).is_ok());
    }

    #[test]
    fn test_packet_deserialization() {
        let json_data = r#"{"id":1234567890,"type":"cconnect.ping","body":{}}"#;
        let packet = Packet::from_bytes(json_data.as_bytes()).unwrap();

        assert_eq!(packet.id, 1234567890);
        assert_eq!(packet.packet_type, "cconnect.ping");
        assert!(packet.body.is_object());
    }

    #[test]
    fn test_packet_deserialization_with_newline() {
        let json_data = r#"{"id":1234567890,"type":"cconnect.ping","body":{}}"#.to_string() + "\n";
        let packet = Packet::from_bytes(json_data.as_bytes()).unwrap();

        assert_eq!(packet.id, 1234567890);
        assert_eq!(packet.packet_type, "cconnect.ping");
    }

    #[test]
    fn test_packet_deserialization_with_crlf() {
        let json_data =
            r#"{"id":1234567890,"type":"cconnect.ping","body":{}}"#.to_string() + "\r\n";
        let packet = Packet::from_bytes(json_data.as_bytes()).unwrap();

        assert_eq!(packet.id, 1234567890);
        assert_eq!(packet.packet_type, "cconnect.ping");
    }

    #[test]
    fn test_roundtrip() {
        let original = Packet::new(
            "cconnect.battery",
            json!({
                "isCharging": true,
                "currentCharge": 85,
                "thresholdEvent": 0
            }),
        );

        let bytes = original.to_bytes().unwrap();
        let parsed = Packet::from_bytes(&bytes).unwrap();

        assert_eq!(original.packet_type, parsed.packet_type);
        assert_eq!(original.body, parsed.body);
    }

    #[test]
    fn test_id_as_string() {
        // Some CConnect clients send id as string
        let json_data = r#"{"id":"1234567890","type":"cconnect.ping","body":{}}"#;
        let packet = Packet::from_bytes(json_data.as_bytes()).unwrap();

        assert_eq!(packet.id, 1234567890);
    }

    #[test]
    fn test_with_payload_size() {
        let packet = Packet::new("cconnect.share", json!({})).with_payload_size(1024);

        assert_eq!(packet.payload_size, Some(1024));
    }

    #[test]
    fn test_with_payload_transfer_info() {
        let mut info = HashMap::new();
        info.insert("port".to_string(), json!(1739));

        let packet = Packet::new("cconnect.share", json!({})).with_payload_transfer_info(info);

        assert!(packet.payload_transfer_info.is_some());
        let port = packet
            .payload_transfer_info
            .as_ref()
            .and_then(|i| i.get("port"))
            .and_then(|v| v.as_i64());
        assert_eq!(port, Some(1739));
    }

    #[test]
    fn test_builder_pattern() {
        let packet = Packet::new("cconnect.identity", json!({}))
            .with_body_field("deviceId", "test-device")
            .with_body_field("deviceName", "Test Device")
            .with_body_field("protocolVersion", 7);

        assert_eq!(
            packet.get_body_field::<String>("deviceId"),
            Some("test-device".to_string())
        );
        assert_eq!(packet.get_body_field::<i64>("protocolVersion"), Some(7));
    }

    #[test]
    fn test_is_type() {
        let packet = Packet::new("cconnect.ping", json!({}));
        assert!(packet.is_type("cconnect.ping"));
        assert!(!packet.is_type("cconnect.battery"));
    }

    #[test]
    fn test_get_body_field() {
        let packet = Packet::new(
            "cconnect.battery",
            json!({
                "isCharging": true,
                "currentCharge": 85
            }),
        );

        assert_eq!(packet.get_body_field::<bool>("isCharging"), Some(true));
        assert_eq!(packet.get_body_field::<i64>("currentCharge"), Some(85));
        assert_eq!(packet.get_body_field::<String>("nonexistent"), None);
    }

    #[test]
    fn test_invalid_packet() {
        let invalid_json = b"not json data";
        let result = Packet::from_bytes(invalid_json);
        assert!(result.is_err());
    }

    #[test]
    fn test_timestamp_generation() {
        let timestamp = current_timestamp();
        assert!(timestamp > 0);

        // Verify it's in milliseconds (should be 13 digits for current time)
        let timestamp_str = timestamp.to_string();
        assert!(timestamp_str.len() >= 13);
    }

    #[test]
    fn test_complex_body() {
        let packet = Packet::new(
            "cconnect.notification",
            json!({
                "id": "notification-123",
                "appName": "Test App",
                "ticker": "New notification",
                "isClearable": true,
                "actions": ["Reply", "Dismiss"]
            }),
        );

        let bytes = packet.to_bytes().unwrap();
        let parsed = Packet::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.packet_type, "cconnect.notification");
        assert_eq!(
            parsed.get_body_field::<String>("appName"),
            Some("Test App".to_string())
        );
        assert_eq!(parsed.get_body_field::<bool>("isClearable"), Some(true));
    }
}
