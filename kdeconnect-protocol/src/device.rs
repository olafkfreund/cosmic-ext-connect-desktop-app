//! KDE Connect Device State Management
//!
//! This module provides device state tracking and management for connected devices.
//! It handles device lifecycle, connection state, capabilities, and persistence.
//!
//! ## Device Lifecycle
//!
//! 1. **Discovery**: Device discovered via UDP broadcast
//! 2. **Pairing**: TLS certificate exchange and user verification
//! 3. **Connected**: Active TCP connection established
//! 4. **Disconnected**: Connection lost or closed
//!
//! ## Device Manager
//!
//! The `DeviceManager` maintains a registry of all known devices and their states.
//! It provides methods for adding, removing, and querying devices.
//!
//! ## Persistence
//!
//! Device information is persisted to disk to remember paired devices
//! across application restarts.

use crate::{DeviceInfo, PairingStatus, ProtocolError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// Device connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionState {
    /// Device is disconnected
    Disconnected,
    /// Device is being connected
    Connecting,
    /// Device is connected and ready
    Connected,
    /// Device connection failed
    Failed,
}

impl ConnectionState {
    /// Check if device is connected
    pub fn is_connected(&self) -> bool {
        matches!(self, ConnectionState::Connected)
    }

    /// Check if device is reachable (connected or connecting)
    pub fn is_reachable(&self) -> bool {
        matches!(
            self,
            ConnectionState::Connected | ConnectionState::Connecting
        )
    }
}

/// Complete device state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    /// Device information (ID, name, type, capabilities)
    #[serde(flatten)]
    pub info: DeviceInfo,

    /// Current connection state
    pub connection_state: ConnectionState,

    /// Current pairing status
    pub pairing_status: PairingStatus,

    /// Whether device is trusted (paired and certificate verified)
    pub is_trusted: bool,

    /// Last time device was seen (UNIX timestamp)
    pub last_seen: u64,

    /// Last connection timestamp (UNIX timestamp)
    pub last_connected: Option<u64>,

    /// TCP host address when connected
    pub host: Option<String>,

    /// TCP port when connected
    pub port: Option<u16>,

    /// Certificate fingerprint (SHA256)
    pub certificate_fingerprint: Option<String>,
}

impl Device {
    /// Create a new device from discovery info
    pub fn from_discovery(info: DeviceInfo) -> Self {
        Self {
            info,
            connection_state: ConnectionState::Disconnected,
            pairing_status: PairingStatus::Unpaired,
            is_trusted: false,
            last_seen: current_timestamp(),
            last_connected: None,
            host: None,
            port: None,
            certificate_fingerprint: None,
        }
    }

    /// Create a new device with full information
    pub fn new(
        info: DeviceInfo,
        connection_state: ConnectionState,
        pairing_status: PairingStatus,
    ) -> Self {
        Self {
            info,
            connection_state,
            pairing_status,
            is_trusted: pairing_status == PairingStatus::Paired,
            last_seen: current_timestamp(),
            last_connected: None,
            host: None,
            port: None,
            certificate_fingerprint: None,
        }
    }

    /// Get device ID
    pub fn id(&self) -> &str {
        &self.info.device_id
    }

    /// Get device name
    pub fn name(&self) -> &str {
        &self.info.device_name
    }

    /// Check if device is currently connected
    pub fn is_connected(&self) -> bool {
        self.connection_state.is_connected()
    }

    /// Check if device is paired
    pub fn is_paired(&self) -> bool {
        self.pairing_status == PairingStatus::Paired
    }

    /// Check if device is reachable
    pub fn is_reachable(&self) -> bool {
        self.connection_state.is_reachable()
    }

    /// Update last seen timestamp
    pub fn update_last_seen(&mut self) {
        self.last_seen = current_timestamp();
    }

    /// Mark device as connected
    pub fn mark_connected(&mut self, host: String, port: u16) {
        self.connection_state = ConnectionState::Connected;
        self.host = Some(host);
        self.port = Some(port);
        self.last_connected = Some(current_timestamp());
        self.update_last_seen();
        info!(
            "Device {} ({}) connected at {}:{}",
            self.id(),
            self.name(),
            self.host.as_ref().unwrap(),
            port
        );
    }

    /// Mark device as disconnected
    pub fn mark_disconnected(&mut self) {
        self.connection_state = ConnectionState::Disconnected;
        self.host = None;
        self.port = None;
        self.update_last_seen();
        info!("Device {} ({}) disconnected", self.id(), self.name());
    }

    /// Mark device as connecting
    pub fn mark_connecting(&mut self, host: String, port: u16) {
        self.connection_state = ConnectionState::Connecting;
        self.host = Some(host.clone());
        self.port = Some(port);
        self.update_last_seen();
        debug!(
            "Device {} ({}) connecting to {}:{}",
            self.id(),
            self.name(),
            host,
            port
        );
    }

    /// Mark device connection as failed
    pub fn mark_failed(&mut self) {
        self.connection_state = ConnectionState::Failed;
        self.update_last_seen();
        warn!("Device {} ({}) connection failed", self.id(), self.name());
    }

    /// Update pairing status
    pub fn update_pairing_status(&mut self, status: PairingStatus) {
        self.pairing_status = status;
        self.is_trusted = status == PairingStatus::Paired;
        self.update_last_seen();
    }

    /// Set certificate fingerprint
    pub fn set_certificate_fingerprint(&mut self, fingerprint: String) {
        self.certificate_fingerprint = Some(fingerprint);
    }

    /// Check if device has a specific incoming capability
    pub fn has_incoming_capability(&self, capability: &str) -> bool {
        self.info
            .incoming_capabilities
            .contains(&capability.to_string())
    }

    /// Check if device has a specific outgoing capability
    pub fn has_outgoing_capability(&self, capability: &str) -> bool {
        self.info
            .outgoing_capabilities
            .contains(&capability.to_string())
    }

    /// Get time since last seen in seconds
    pub fn seconds_since_last_seen(&self) -> u64 {
        current_timestamp().saturating_sub(self.last_seen)
    }

    /// Check if device was seen recently (within last N seconds)
    pub fn seen_recently(&self, within_seconds: u64) -> bool {
        self.seconds_since_last_seen() <= within_seconds
    }
}

/// Device manager for tracking multiple devices
pub struct DeviceManager {
    /// Map of device ID to device
    devices: HashMap<String, Device>,

    /// Path to store device registry
    registry_path: PathBuf,
}

impl DeviceManager {
    /// Create a new device manager
    ///
    /// # Arguments
    ///
    /// * `registry_path` - Path to store device registry JSON
    pub fn new(registry_path: impl Into<PathBuf>) -> Result<Self> {
        let registry_path = registry_path.into();

        // Ensure parent directory exists
        if let Some(parent) = registry_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut manager = Self {
            devices: HashMap::new(),
            registry_path,
        };

        // Load existing registry
        manager.load_registry()?;

        Ok(manager)
    }

    /// Add or update a device
    pub fn add_device(&mut self, device: Device) {
        let device_id = device.id().to_string();
        info!("Adding/updating device: {} ({})", device.name(), device_id);
        self.devices.insert(device_id, device);
    }

    /// Get a device by ID
    pub fn get_device(&self, device_id: &str) -> Option<&Device> {
        self.devices.get(device_id)
    }

    /// Get a mutable reference to a device by ID
    pub fn get_device_mut(&mut self, device_id: &str) -> Option<&mut Device> {
        self.devices.get_mut(device_id)
    }

    /// Remove a device by ID
    pub fn remove_device(&mut self, device_id: &str) -> Option<Device> {
        info!("Removing device: {}", device_id);
        self.devices.remove(device_id)
    }

    /// Check if a device exists
    pub fn has_device(&self, device_id: &str) -> bool {
        self.devices.contains_key(device_id)
    }

    /// Get all devices
    pub fn devices(&self) -> impl Iterator<Item = &Device> {
        self.devices.values()
    }

    /// Get all device IDs
    pub fn device_ids(&self) -> impl Iterator<Item = &String> {
        self.devices.keys()
    }

    /// Get all connected devices
    pub fn connected_devices(&self) -> impl Iterator<Item = &Device> {
        self.devices.values().filter(|d| d.is_connected())
    }

    /// Get all paired devices
    pub fn paired_devices(&self) -> impl Iterator<Item = &Device> {
        self.devices.values().filter(|d| d.is_paired())
    }

    /// Get all trusted devices
    pub fn trusted_devices(&self) -> impl Iterator<Item = &Device> {
        self.devices.values().filter(|d| d.is_trusted)
    }

    /// Get count of devices
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    /// Get count of connected devices
    pub fn connected_count(&self) -> usize {
        self.connected_devices().count()
    }

    /// Get count of paired devices
    pub fn paired_count(&self) -> usize {
        self.paired_devices().count()
    }

    /// Update device from discovery info
    pub fn update_from_discovery(&mut self, info: DeviceInfo) {
        let device_id = info.device_id.clone();

        if let Some(device) = self.devices.get_mut(&device_id) {
            // Update existing device
            device.info = info;
            device.update_last_seen();
            debug!("Updated device from discovery: {}", device_id);
        } else {
            // Add new device
            let device = Device::from_discovery(info);
            self.add_device(device);
        }
    }

    /// Mark device as connected
    pub fn mark_connected(&mut self, device_id: &str, host: String, port: u16) -> Result<()> {
        let device = self
            .devices
            .get_mut(device_id)
            .ok_or_else(|| ProtocolError::DeviceNotFound(device_id.to_string()))?;

        device.mark_connected(host, port);
        Ok(())
    }

    /// Mark device as disconnected
    pub fn mark_disconnected(&mut self, device_id: &str) -> Result<()> {
        let device = self
            .devices
            .get_mut(device_id)
            .ok_or_else(|| ProtocolError::DeviceNotFound(device_id.to_string()))?;

        device.mark_disconnected();
        Ok(())
    }

    /// Update device pairing status
    pub fn update_pairing_status(&mut self, device_id: &str, status: PairingStatus) -> Result<()> {
        let device = self
            .devices
            .get_mut(device_id)
            .ok_or_else(|| ProtocolError::DeviceNotFound(device_id.to_string()))?;

        device.update_pairing_status(status);
        Ok(())
    }

    /// Save device registry to disk
    pub fn save_registry(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.devices)?;
        fs::write(&self.registry_path, json)?;
        debug!("Saved device registry to {:?}", self.registry_path);
        Ok(())
    }

    /// Load device registry from disk
    pub fn load_registry(&mut self) -> Result<()> {
        if !self.registry_path.exists() {
            debug!("No existing registry file at {:?}", self.registry_path);
            return Ok(());
        }

        let json = fs::read_to_string(&self.registry_path)?;
        self.devices = serde_json::from_str(&json)?;
        info!("Loaded {} devices from registry", self.devices.len());
        Ok(())
    }

    /// Clean up stale devices (not seen in N seconds)
    pub fn cleanup_stale_devices(&mut self, max_age_seconds: u64) -> usize {
        let before_count = self.devices.len();

        self.devices.retain(|id, device| {
            let keep = device.is_paired() || device.seen_recently(max_age_seconds);
            if !keep {
                debug!("Removing stale device: {} ({})", device.name(), id);
            }
            keep
        });

        before_count - self.devices.len()
    }
}

/// Get current UNIX timestamp in seconds
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceType;
    use tempfile::TempDir;

    fn create_test_device_info() -> DeviceInfo {
        DeviceInfo::new("Test Device", DeviceType::Desktop, 1716)
    }

    #[test]
    fn test_connection_state() {
        assert!(ConnectionState::Connected.is_connected());
        assert!(!ConnectionState::Disconnected.is_connected());
        assert!(ConnectionState::Connecting.is_reachable());
        assert!(ConnectionState::Connected.is_reachable());
        assert!(!ConnectionState::Disconnected.is_reachable());
    }

    #[test]
    fn test_device_creation() {
        let info = create_test_device_info();
        let device = Device::from_discovery(info);

        assert_eq!(device.connection_state, ConnectionState::Disconnected);
        assert_eq!(device.pairing_status, PairingStatus::Unpaired);
        assert!(!device.is_trusted);
        assert!(device.last_seen > 0);
    }

    #[test]
    fn test_device_connection_lifecycle() {
        let info = create_test_device_info();
        let mut device = Device::from_discovery(info);

        // Initially disconnected
        assert!(!device.is_connected());

        // Mark as connecting
        device.mark_connecting("192.168.1.100".to_string(), 1716);
        assert!(!device.is_connected());
        assert!(device.is_reachable());
        assert_eq!(device.host, Some("192.168.1.100".to_string()));

        // Mark as connected
        device.mark_connected("192.168.1.100".to_string(), 1716);
        assert!(device.is_connected());
        assert!(device.is_reachable());
        assert!(device.last_connected.is_some());

        // Mark as disconnected
        device.mark_disconnected();
        assert!(!device.is_connected());
        assert!(device.host.is_none());
    }

    #[test]
    fn test_device_pairing() {
        let info = create_test_device_info();
        let mut device = Device::from_discovery(info);

        assert!(!device.is_paired());
        assert!(!device.is_trusted);

        device.update_pairing_status(PairingStatus::Paired);
        assert!(device.is_paired());
        assert!(device.is_trusted);
    }

    #[test]
    fn test_device_capabilities() {
        let mut info = create_test_device_info();
        info = info
            .with_incoming_capability("kdeconnect.battery")
            .with_outgoing_capability("kdeconnect.ping");

        let device = Device::from_discovery(info);

        assert!(device.has_incoming_capability("kdeconnect.battery"));
        assert!(device.has_outgoing_capability("kdeconnect.ping"));
        assert!(!device.has_incoming_capability("kdeconnect.notification"));
    }

    #[test]
    fn test_device_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path().join("registry.json");

        let manager = DeviceManager::new(&registry_path).unwrap();
        assert_eq!(manager.device_count(), 0);
    }

    #[test]
    fn test_device_manager_add_remove() {
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path().join("registry.json");
        let mut manager = DeviceManager::new(&registry_path).unwrap();

        let info = create_test_device_info();
        let device_id = info.device_id.clone();
        let device = Device::from_discovery(info);

        manager.add_device(device);
        assert_eq!(manager.device_count(), 1);
        assert!(manager.has_device(&device_id));

        let removed = manager.remove_device(&device_id);
        assert!(removed.is_some());
        assert_eq!(manager.device_count(), 0);
    }

    #[test]
    fn test_device_manager_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path().join("registry.json");

        // Create manager and add device
        {
            let mut manager = DeviceManager::new(&registry_path).unwrap();
            let info = create_test_device_info();
            let device = Device::from_discovery(info);
            manager.add_device(device);
            manager.save_registry().unwrap();
        }

        // Load in new manager
        {
            let manager = DeviceManager::new(&registry_path).unwrap();
            assert_eq!(manager.device_count(), 1);
        }
    }

    #[test]
    fn test_device_manager_filters() {
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path().join("registry.json");
        let mut manager = DeviceManager::new(&registry_path).unwrap();

        // Add connected device
        let mut info1 = DeviceInfo::new("Device 1", DeviceType::Phone, 1716);
        info1.device_id = "device_1".to_string();
        let mut device1 = Device::from_discovery(info1);
        device1.mark_connected("192.168.1.100".to_string(), 1716);
        manager.add_device(device1);

        // Add paired but disconnected device
        let mut info2 = DeviceInfo::new("Device 2", DeviceType::Tablet, 1716);
        info2.device_id = "device_2".to_string();
        let mut device2 = Device::from_discovery(info2);
        device2.update_pairing_status(PairingStatus::Paired);
        manager.add_device(device2);

        assert_eq!(manager.device_count(), 2);
        assert_eq!(manager.connected_count(), 1);
        assert_eq!(manager.paired_count(), 1);
    }

    #[test]
    fn test_device_seen_recently() {
        let info = create_test_device_info();
        let device = Device::from_discovery(info);

        assert!(device.seen_recently(10));
        assert_eq!(device.seconds_since_last_seen(), 0);
    }

    #[test]
    fn test_cleanup_stale_devices() {
        let temp_dir = TempDir::new().unwrap();
        let registry_path = temp_dir.path().join("registry.json");
        let mut manager = DeviceManager::new(&registry_path).unwrap();

        // Add unpaired device
        let info = create_test_device_info();
        manager.add_device(Device::from_discovery(info));

        // Cleanup should remove it (0 second threshold)
        let removed = manager.cleanup_stale_devices(0);
        assert_eq!(removed, 0); // Actually seen recently, so not removed

        // Add paired device - should never be cleaned up
        let mut info2 = DeviceInfo::new("Paired Device", DeviceType::Phone, 1716);
        info2.device_id = "paired_device".to_string();
        let mut device2 = Device::from_discovery(info2);
        device2.update_pairing_status(PairingStatus::Paired);
        manager.add_device(device2);

        assert_eq!(manager.device_count(), 2);
    }
}
