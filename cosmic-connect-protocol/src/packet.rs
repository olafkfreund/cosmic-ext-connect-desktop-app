//! CConnect Network Packet
//!
//! This module implements the core packet structure for the CConnect protocol.
//! Packets are JSON-formatted messages with a newline terminator.

use crate::{ProtocolError, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Packet {
    #[serde(deserialize_with = "deserialize_id", serialize_with = "serialize_id")]
    pub id: i64,

    #[serde(rename = "type")]
    pub packet_type: String,

    #[serde(default)]
    pub body: Value,

    #[serde(rename = "payloadSize", skip_serializing_if = "Option::is_none")]
    pub payload_size: Option<i64>,

    #[serde(
        rename = "payloadTransferInfo",
        skip_serializing_if = "Option::is_none"
    )]
    pub payload_transfer_info: Option<HashMap<String, Value>>,
}

impl Packet {
    pub fn to_core_packet(&self) -> cosmic_connect_core::Packet {
        cosmic_connect_core::Packet {
            id: self.id,
            packet_type: self.packet_type.clone(),
            body: self.body.clone(),
            payload_size: self.payload_size,
            payload_transfer_info: self.payload_transfer_info.clone(),
        }
    }

    pub fn from_core_packet(packet: cosmic_connect_core::Packet) -> Self {
        Self {
            id: packet.id,
            packet_type: packet.packet_type,
            body: packet.body,
            payload_size: packet.payload_size,
            payload_transfer_info: packet.payload_transfer_info,
        }
    }

    pub fn new(packet_type: impl Into<String>, body: Value) -> Self {
        Self {
            id: current_timestamp(),
            packet_type: packet_type.into(),
            body,
            payload_size: None,
            payload_transfer_info: None,
        }
    }

    pub fn with_id(id: i64, packet_type: impl Into<String>, body: Value) -> Self {
        Self {
            id,
            packet_type: packet_type.into(),
            body,
            payload_size: None,
            payload_transfer_info: None,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let json = serde_json::to_string(self)?;
        let mut bytes = json.into_bytes();
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let trimmed = data
            .iter()
            .skip_while(|&&b| b == 0 || b.is_ascii_whitespace())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .skip_while(|&&b| b == 0 || b.is_ascii_whitespace())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .cloned()
            .collect::<Vec<u8>>();

        serde_json::from_slice(&trimmed).map_err(|e| {
            ProtocolError::InvalidPacket(format!("Failed to deserialize packet: {}", e))
        })
    }

    pub fn with_payload_size(mut self, size: i64) -> Self {
        self.payload_size = Some(size);
        self
    }

    pub fn with_payload_transfer_info(mut self, info: HashMap<String, Value>) -> Self {
        self.payload_transfer_info = Some(info);
        self
    }

    pub fn with_body_field(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        if let Value::Object(ref mut map) = self.body {
            map.insert(key.into(), value.into());
        }
        self
    }

    pub fn is_type(&self, packet_type: &str) -> bool {
        if self.packet_type == packet_type {
            return true;
        }

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

    pub fn get_body_field<T>(&self, key: &str) -> Option<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.body
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

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

fn serialize_id<S>(id: &i64, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_i64(*id)
}

pub fn current_timestamp() -> i64 {
    Utc::now().timestamp_millis()
}
