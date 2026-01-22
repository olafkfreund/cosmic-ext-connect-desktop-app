//! Contacts Plugin
//!
//! Synchronizes contacts from mobile device to desktop in vCard format.
//! Provides one-way sync (mobile → desktop) with timestamp-based updates.
//!
//! ## Protocol
//!
//! ### Packet Types
//! - `cconnect.contacts.request_all_uids_timestamps` - Request all contact UIDs with timestamps
//! - `cconnect.contacts.request_vcards_by_uid` - Request specific vCards by UID
//! - `cconnect.contacts.response_uids_timestamps` - Response with UID/timestamp pairs
//! - `cconnect.contacts.response_vcards` - Response with vCard data
//!
//! ### vCard Format
//! - Standard: vCard 2.1
//! - Extensions:
//!   - `X-KDECONNECT-ID-DEV-[device-id]` - Device-specific contact ID
//!   - `X-KDECONNECT-TIMESTAMP` - Last modification time (milliseconds)
//!
//! ## References
//! - [Valent Protocol](https://valent.andyholmes.ca/documentation/protocol.html)

pub mod database;
pub mod signals;

use crate::plugins::{Plugin, PluginFactory};
use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use database::{Contact, ContactsDatabase, Email, PhoneNumber};
use serde::{Deserialize, Serialize};
use serde_json::json;
use signals::{ContactEvent, ContactsSignals};
use std::any::Any;
use std::collections::HashMap;
use tracing::{debug, info, warn};

// Re-export for external use
pub use database::{Contact as ContactData, Email as EmailAddress, PhoneNumber as PhoneInfo};

/// Packet type for requesting all contact UIDs with timestamps
pub const PACKET_TYPE_REQUEST_ALL_UIDS_TIMESTAMPS: &str =
    "cconnect.contacts.request_all_uids_timestamps";

/// Packet type for requesting specific vCards by UID
pub const PACKET_TYPE_REQUEST_VCARDS_BY_UID: &str = "cconnect.contacts.request_vcards_by_uid";

/// Packet type for response with UIDs and timestamps
pub const PACKET_TYPE_RESPONSE_UIDS_TIMESTAMPS: &str = "cconnect.contacts.response_uids_timestamps";

/// Packet type for response with vCard data
pub const PACKET_TYPE_RESPONSE_VCARDS: &str = "cconnect.contacts.response_vcards";

/// Contact UID with modification timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactTimestamp {
    /// Contact unique identifier
    pub uid: String,
    /// Last modification timestamp (milliseconds since epoch)
    pub timestamp: i64,
}

/// vCard data for a contact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactVCard {
    /// Contact unique identifier
    pub uid: String,
    /// vCard 2.1 formatted data
    pub vcard: String,
}

/// Request for specific vCards by UID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VCardRequest {
    /// List of contact UIDs to retrieve
    pub uids: Vec<String>,
}

/// Contacts plugin for synchronizing contacts from mobile device
pub struct ContactsPlugin {
    /// Device ID this plugin is associated with
    device_id: Option<String>,

    /// Cache of contact UIDs with timestamps
    contacts_cache: HashMap<String, i64>,

    /// Cache of vCard data
    vcards_cache: HashMap<String, String>,

    /// Database for persistent storage (optional - requires rusqlite)
    /// For now, this is None (stub implementation)
    database: Option<ContactsDatabase>,

    /// DBus signals for contact updates (optional)
    signals: Option<ContactsSignals>,

    /// Channel to send packets
    packet_sender: Option<tokio::sync::mpsc::Sender<(String, Packet)>>,
}

impl ContactsPlugin {
    /// Create a new contacts plugin instance
    pub fn new() -> Self {
        Self {
            device_id: None,
            contacts_cache: HashMap::new(),
            vcards_cache: HashMap::new(),
            database: None,
            signals: None,
            packet_sender: None,
        }
    }

    /// Initialize database storage
    ///
    /// Call this to enable persistent storage for contacts.
    /// Requires rusqlite dependency to be added.
    pub async fn init_database(&mut self, db_path: &str) -> Result<()> {
        info!("Initializing contacts database at: {}", db_path);

        match ContactsDatabase::new(db_path).await {
            Ok(db) => {
                self.database = Some(db);
                info!("Contacts database initialized successfully");
                Ok(())
            }
            Err(e) => {
                warn!("Failed to initialize contacts database: {}", e);
                Err(ProtocolError::Plugin(format!(
                    "Failed to init database: {}",
                    e
                )))
            }
        }
    }

    /// Initialize DBus signals
    ///
    /// Call this to enable real-time notifications for contact changes.
    pub async fn init_signals(&mut self) -> Result<()> {
        info!("Initializing contacts DBus signals");

        match ContactsSignals::new().await {
            Ok(signals) => {
                self.signals = Some(signals);
                info!("Contacts DBus signals initialized successfully");
                Ok(())
            }
            Err(e) => {
                warn!("Failed to initialize DBus signals: {}", e);
                Err(ProtocolError::Plugin(format!(
                    "Failed to init signals: {}",
                    e
                )))
            }
        }
    }

    /// Create a packet to request all contact UIDs with timestamps
    pub fn create_request_all_uids_timestamps(&self) -> Packet {
        debug!("Creating request for all contact UIDs with timestamps");
        Packet::new(PACKET_TYPE_REQUEST_ALL_UIDS_TIMESTAMPS, json!({}))
    }

    /// Create a packet to request specific vCards by UID
    pub fn create_request_vcards_by_uid(&self, uids: Vec<String>) -> Packet {
        debug!("Creating request for {} vCards by UID", uids.len());
        Packet::new(
            PACKET_TYPE_REQUEST_VCARDS_BY_UID,
            json!({
                "uids": uids,
            }),
        )
    }

    /// Handle response with contact UIDs and timestamps
    async fn handle_uids_timestamps_response(&mut self, packet: &Packet) -> Result<()> {
        debug!("Processing UIDs/timestamps response");

        // The body contains UID -> timestamp mappings
        if let Some(uids_obj) = packet.body.get("uids").and_then(|v| v.as_object()) {
            let mut updated_count = 0;
            let mut new_count = 0;

            for (uid, timestamp_value) in uids_obj {
                if let Some(timestamp) = timestamp_value.as_i64() {
                    // Check if this is a new contact or an update
                    match self.contacts_cache.get(uid) {
                        Some(&cached_timestamp) if cached_timestamp < timestamp => {
                            debug!(
                                "Contact {} updated: {} -> {}",
                                uid, cached_timestamp, timestamp
                            );
                            self.contacts_cache.insert(uid.clone(), timestamp);
                            updated_count += 1;
                        }
                        None => {
                            debug!("New contact: {}", uid);
                            self.contacts_cache.insert(uid.clone(), timestamp);
                            new_count += 1;
                        }
                        _ => {
                            // Contact unchanged
                        }
                    }
                }
            }

            info!(
                "Contacts sync: {} new, {} updated, {} total",
                new_count,
                updated_count,
                self.contacts_cache.len()
            );

            Ok(())
        } else {
            warn!("Invalid UIDs/timestamps response format");
            Err(ProtocolError::Plugin(
                "Invalid response format for UIDs/timestamps".to_string(),
            ))
        }
    }

    /// Handle response with vCard data
    async fn handle_vcards_response(&mut self, packet: &Packet) -> Result<()> {
        debug!("Processing vCards response");

        if let Some(vcards_obj) = packet.body.get("vcards").and_then(|v| v.as_object()) {
            let mut processed_count = 0;

            for (uid, vcard_value) in vcards_obj {
                if let Some(vcard_str) = vcard_value.as_str() {
                    debug!("Received vCard for contact: {}", uid);
                    self.vcards_cache.insert(uid.clone(), vcard_str.to_string());
                    processed_count += 1;

                    // Parse vCard to extract contact information and store/emit
                    self.parse_and_store_vcard(uid, vcard_str).await;
                }
            }

            info!("Processed {} vCards", processed_count);
            Ok(())
        } else {
            warn!("Invalid vCards response format");
            Err(ProtocolError::Plugin(
                "Invalid response format for vCards".to_string(),
            ))
        }
    }

    /// Parse vCard data, store to database, and emit DBus signals
    async fn parse_and_store_vcard(&mut self, uid: &str, vcard_data: &str) {
        // Parse basic vCard fields
        let mut name = None;
        let mut phone_numbers = Vec::new();
        let mut emails = Vec::new();

        for line in vcard_data.lines() {
            let line = line.trim();
            if line.starts_with("FN:") {
                name = Some(line[3..].to_string());
            } else if line.starts_with("TEL") {
                if let Some(number) = line.split(':').nth(1) {
                    phone_numbers.push(PhoneNumber {
                        number: number.to_string(),
                        phone_type: None, // TODO: Parse type from vCard
                    });
                }
            } else if line.starts_with("EMAIL") {
                if let Some(email) = line.split(':').nth(1) {
                    emails.push(Email {
                        address: email.to_string(),
                        email_type: None, // TODO: Parse type from vCard
                    });
                }
            }
        }

        debug!(
            "Parsed contact {}: name={:?}, {} phones, {} emails",
            uid,
            name,
            phone_numbers.len(),
            emails.len()
        );

        // Get device ID and timestamp
        let device_id = self
            .device_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let timestamp = self.contacts_cache.get(uid).copied().unwrap_or(0);

        // Check if this is a new contact or update
        let is_new = !self.vcards_cache.contains_key(uid);

        // Store to database if available
        if let Some(ref mut db) = self.database {
            let contact = Contact {
                uid: uid.to_string(),
                device_id: device_id.clone(),
                name: name.clone(),
                vcard_data: vcard_data.to_string(),
                timestamp,
                phone_numbers,
                emails,
            };

            match db.upsert_contact(contact).await {
                Ok(_) => {
                    debug!("Contact {} stored to database", uid);
                }
                Err(e) => {
                    warn!("Failed to store contact {} to database: {}", uid, e);
                }
            }
        }

        // Emit DBus signal if available
        if let Some(ref signals) = self.signals {
            let event = if is_new {
                ContactEvent::Added {
                    device_id,
                    uid: uid.to_string(),
                    name,
                }
            } else {
                ContactEvent::Updated {
                    device_id,
                    uid: uid.to_string(),
                    name,
                }
            };

            if let Err(e) = event.emit(signals).await {
                warn!("Failed to emit contact event: {}", e);
            }
        }
    }

    /// Get all cached contact UIDs
    pub fn get_all_contact_uids(&self) -> Vec<String> {
        self.contacts_cache.keys().cloned().collect()
    }

    /// Get vCard data for a specific contact
    pub fn get_vcard(&self, uid: &str) -> Option<&String> {
        self.vcards_cache.get(uid)
    }

    /// Get number of cached contacts
    pub fn get_contact_count(&self) -> usize {
        self.contacts_cache.len()
    }

    /// Clear all cached data
    pub fn clear_cache(&mut self) {
        self.contacts_cache.clear();
        self.vcards_cache.clear();
        info!("Cleared contacts cache");
    }
}

impl Default for ContactsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ContactsPlugin {
    fn name(&self) -> &'static str {
        "contacts"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_RESPONSE_UIDS_TIMESTAMPS.to_string(),
            PACKET_TYPE_RESPONSE_VCARDS.to_string(),
            "kdeconnect.contacts.response_uids_timestamps".to_string(),
            "kdeconnect.contacts.response_vcards".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_REQUEST_ALL_UIDS_TIMESTAMPS.to_string(),
            PACKET_TYPE_REQUEST_VCARDS_BY_UID.to_string(),
        ]
    }

    async fn init(&mut self, device: &Device, packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        self.packet_sender = Some(packet_sender);
        info!("Contacts plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Starting contacts plugin");
        // Automatically request contacts on start
        if let Some(sender) = &self.packet_sender {
            if let Some(device_id) = &self.device_id {
                let packet = self.create_request_all_uids_timestamps();
                if let Err(e) = sender.send((device_id.clone(), packet)).await {
                    warn!("Failed to send contacts request: {}", e);
                } else {
                    debug!("Sent contacts request");
                }
            }
        }
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Stopping contacts plugin");
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, _device: &mut Device) -> Result<()> {
        if packet.is_type(PACKET_TYPE_RESPONSE_UIDS_TIMESTAMPS) {
            self.handle_uids_timestamps_response(packet).await
        } else if packet.is_type(PACKET_TYPE_RESPONSE_VCARDS) {
            self.handle_vcards_response(packet).await
        } else {
            Ok(())
        }
    }
}

/// Factory for creating contacts plugin instances
pub struct ContactsPluginFactory;

impl PluginFactory for ContactsPluginFactory {
    fn name(&self) -> &str {
        "contacts"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_RESPONSE_UIDS_TIMESTAMPS.to_string(),
            PACKET_TYPE_RESPONSE_VCARDS.to_string(),
            "kdeconnect.contacts.response_uids_timestamps".to_string(),
            "kdeconnect.contacts.response_vcards".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_REQUEST_ALL_UIDS_TIMESTAMPS.to_string(),
            PACKET_TYPE_REQUEST_VCARDS_BY_UID.to_string(),
        ]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(ContactsPlugin::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceInfo, DeviceType};

    fn create_test_plugin() -> ContactsPlugin {
        ContactsPlugin::new()
    }

    fn create_test_device() -> Device {
        let info = DeviceInfo::new("Test Device", DeviceType::Phone, 1716);
        Device::from_discovery(info)
    }

    #[test]
    fn test_plugin_name() {
        let plugin = create_test_plugin();
        assert_eq!(plugin.name(), "contacts");
    }

    #[test]
    fn test_capabilities() {
        let plugin = create_test_plugin();

        let incoming = plugin.incoming_capabilities();
        assert!(incoming.contains(&PACKET_TYPE_RESPONSE_UIDS_TIMESTAMPS.to_string()));
        assert!(incoming.contains(&PACKET_TYPE_RESPONSE_VCARDS.to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert!(outgoing.contains(&PACKET_TYPE_REQUEST_ALL_UIDS_TIMESTAMPS.to_string()));
        assert!(outgoing.contains(&PACKET_TYPE_REQUEST_VCARDS_BY_UID.to_string()));
    }

    #[test]
    fn test_create_request_all_uids() {
        let plugin = create_test_plugin();
        let packet = plugin.create_request_all_uids_timestamps();

        assert_eq!(packet.packet_type, PACKET_TYPE_REQUEST_ALL_UIDS_TIMESTAMPS);
        assert!(packet.body.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_create_request_vcards() {
        let plugin = create_test_plugin();
        let uids = vec!["contact1".to_string(), "contact2".to_string()];
        let packet = plugin.create_request_vcards_by_uid(uids);

        assert_eq!(packet.packet_type, PACKET_TYPE_REQUEST_VCARDS_BY_UID);
        assert_eq!(
            packet.body.get("uids").unwrap().as_array().unwrap().len(),
            2
        );
    }

    #[tokio::test]
    async fn test_handle_uids_timestamps_response() {
        let mut plugin = create_test_plugin();
        let mut device = create_test_device();

        let packet = Packet::new(
            PACKET_TYPE_RESPONSE_UIDS_TIMESTAMPS,
            json!({
                "uids": {
                    "contact1": 1234567890000i64,
                    "contact2": 1234567891000i64,
                }
            }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        assert_eq!(plugin.get_contact_count(), 2);
        assert!(plugin
            .get_all_contact_uids()
            .contains(&"contact1".to_string()));
        assert!(plugin
            .get_all_contact_uids()
            .contains(&"contact2".to_string()));
    }

    #[tokio::test]
    async fn test_handle_vcards_response() {
        let mut plugin = create_test_plugin();
        let mut device = create_test_device();

        let vcard_data = "BEGIN:VCARD\nVERSION:2.1\nFN:John Doe\nTEL:+1234567890\nEND:VCARD";
        let packet = Packet::new(
            PACKET_TYPE_RESPONSE_VCARDS,
            json!({
                "vcards": {
                    "contact1": vcard_data,
                }
            }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        assert_eq!(plugin.vcards_cache.len(), 1);
        assert!(plugin.get_vcard("contact1").is_some());
        assert_eq!(plugin.get_vcard("contact1").unwrap(), vcard_data);
    }

    #[test]
    fn test_clear_cache() {
        let mut plugin = create_test_plugin();
        plugin.contacts_cache.insert("test".to_string(), 123);
        plugin
            .vcards_cache
            .insert("test".to_string(), "data".to_string());

        plugin.clear_cache();

        assert_eq!(plugin.get_contact_count(), 0);
        assert!(plugin.get_vcard("test").is_none());
    }
}
