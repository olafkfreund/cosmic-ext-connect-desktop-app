//! Battery Plugin
//!
//! Reports battery status to connected devices and receives battery status from remote devices.
//! Enables power monitoring across devices in the CConnect network.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `cconnect.battery` - Battery status update
//! - `cconnect.battery.request` - Request battery status (deprecated)
//!
//! **Capabilities**:
//! - Incoming: `cconnect.battery`, `cconnect.battery.request`
//! - Outgoing: `cconnect.battery`, `cconnect.battery.request`
//!
//! ## Packet Formats
//!
//! ### Battery Status (`cconnect.battery`)
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.battery",
//!     "body": {
//!         "currentCharge": 75,
//!         "isCharging": true,
//!         "thresholdEvent": 0
//!     }
//! }
//! ```
//!
//! **Fields**:
//! - `currentCharge` (i32): Battery percentage (0-100, or -1 for no battery)
//! - `isCharging` (bool): Whether device is charging
//! - `thresholdEvent` (i32): 0 = above threshold, 1 = below threshold
//!
//! ### Battery Request (`cconnect.battery.request`) - Deprecated
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.battery.request",
//!     "body": {
//!         "request": true
//!     }
//! }
//! ```
//!
//! ## Behavior
//!
//! - **Proactive Updates**: Send battery status when it changes
//! - **Polling (Deprecated)**: Respond to battery requests
//! - **Idempotent**: Multiple status updates are safe
//! - **No Battery**: Use -1 for currentCharge if device has no battery
//!
//! ## Use Cases
//!
//! - Monitor remote device battery levels
//! - Display low battery warnings
//! - Track charging status
//! - Power management decisions
//!
//! ## Example
//!
//! ```rust,ignore
//! use cosmic_connect_core::plugins::battery::{BatteryPlugin, BatteryStatus};
//! use cosmic_connect_core::{Plugin, PluginManager};
//!
//! // Create plugin
//! let mut plugin = BatteryPlugin::new();
//!
//! // Register with manager
//! let mut manager = PluginManager::new();
//! manager.register(Box::new(plugin))?;
//!
//! // Get remote device battery status
//! if let Some(status) = plugin.get_battery_status() {
//!     println!("Battery: {}%", status.current_charge);
//!     println!("Charging: {}", status.is_charging);
//! }
//!
//! // Create battery status packet to send
//! let status = BatteryStatus {
//!     current_charge: 85,
//!     is_charging: false,
//!     threshold_event: 0,
//! };
//! let packet = plugin.create_battery_packet(&status);
//! ```
//!
//! ## References
//!
//! - [Valent Protocol - Battery](https://valent.andyholmes.ca/documentation/protocol.html)

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

use super::{Plugin, PluginFactory};

/// Battery status information
///
/// Represents the power state of a device.
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_core::plugins::battery::BatteryStatus;
///
/// let status = BatteryStatus {
///     current_charge: 75,
///     is_charging: true,
///     threshold_event: 0,
/// };
///
/// assert_eq!(status.current_charge, 75);
/// assert!(status.is_charging);
/// assert!(!status.is_low_battery());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatteryStatus {
    /// Battery percentage (0-100, or -1 for no battery)
    #[serde(rename = "currentCharge")]
    pub current_charge: i32,

    /// Whether the device is currently charging
    #[serde(rename = "isCharging")]
    pub is_charging: bool,

    /// Threshold event: 0 = above threshold, 1 = below threshold
    #[serde(rename = "thresholdEvent")]
    pub threshold_event: i32,
}

impl BatteryStatus {
    /// Create a new battery status
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::battery::BatteryStatus;
    ///
    /// let status = BatteryStatus::new(50, false, 0);
    /// assert_eq!(status.current_charge, 50);
    /// assert!(!status.is_charging);
    /// ```
    pub fn new(current_charge: i32, is_charging: bool, threshold_event: i32) -> Self {
        Self {
            current_charge,
            is_charging,
            threshold_event,
        }
    }

    /// Create a status indicating no battery present
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::battery::BatteryStatus;
    ///
    /// let status = BatteryStatus::no_battery();
    /// assert_eq!(status.current_charge, -1);
    /// assert!(!status.has_battery());
    /// ```
    pub fn no_battery() -> Self {
        Self {
            current_charge: -1,
            is_charging: false,
            threshold_event: 0,
        }
    }

    /// Check if device has a battery
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::battery::BatteryStatus;
    ///
    /// let with_battery = BatteryStatus::new(75, false, 0);
    /// assert!(with_battery.has_battery());
    ///
    /// let without_battery = BatteryStatus::no_battery();
    /// assert!(!without_battery.has_battery());
    /// ```
    pub fn has_battery(&self) -> bool {
        self.current_charge >= 0
    }

    /// Check if battery is below threshold
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::battery::BatteryStatus;
    ///
    /// let low = BatteryStatus::new(15, false, 1);
    /// assert!(low.is_low_battery());
    ///
    /// let normal = BatteryStatus::new(75, false, 0);
    /// assert!(!normal.is_low_battery());
    /// ```
    pub fn is_low_battery(&self) -> bool {
        self.threshold_event == 1
    }
}

/// Battery plugin for power status monitoring
///
/// Handles battery status updates from remote devices and can send local battery status.
///
/// ## Features
///
/// - Receive battery status from remote devices
/// - Store latest battery status
/// - Respond to battery requests (deprecated protocol)
/// - Create battery status packets
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_core::plugins::battery::{BatteryPlugin, BatteryStatus};
/// use cosmic_connect_core::Plugin;
///
/// let plugin = BatteryPlugin::new();
/// assert_eq!(plugin.name(), "battery");
///
/// // Initially no status
/// assert!(plugin.get_battery_status().is_none());
/// ```
#[derive(Debug)]
pub struct BatteryPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Latest battery status from remote device
    battery_status: Arc<RwLock<Option<BatteryStatus>>>,
}

impl BatteryPlugin {
    /// Create a new battery plugin
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::battery::BatteryPlugin;
    ///
    /// let plugin = BatteryPlugin::new();
    /// assert!(plugin.get_battery_status().is_none());
    /// ```
    pub fn new() -> Self {
        Self {
            device_id: None,
            battery_status: Arc::new(RwLock::new(None)),
        }
    }

    /// Get the current battery status of the remote device
    ///
    /// Returns `None` if no status has been received yet.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::battery::BatteryPlugin;
    ///
    /// let plugin = BatteryPlugin::new();
    /// assert!(plugin.get_battery_status().is_none());
    /// ```
    pub fn get_battery_status(&self) -> Option<BatteryStatus> {
        self.battery_status.read().ok()?.clone()
    }

    /// Create a battery status packet
    ///
    /// Creates a `cconnect.battery` packet with the given status.
    ///
    /// # Parameters
    ///
    /// - `status`: The battery status to send
    ///
    /// # Returns
    ///
    /// A `Packet` ready to be sent to the device
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::battery::{BatteryPlugin, BatteryStatus};
    ///
    /// let plugin = BatteryPlugin::new();
    /// let status = BatteryStatus::new(75, true, 0);
    /// let packet = plugin.create_battery_packet(&status);
    ///
    /// assert_eq!(packet.packet_type, "cconnect.battery");
    /// ```
    pub fn create_battery_packet(&self, status: &BatteryStatus) -> Packet {
        let body = json!({
            "currentCharge": status.current_charge,
            "isCharging": status.is_charging,
            "thresholdEvent": status.threshold_event,
        });

        Packet::new("cconnect.battery", body)
    }

    /// Create a battery request packet (deprecated)
    ///
    /// Creates a `cconnect.battery.request` packet.
    /// Note: This packet type is deprecated in the protocol.
    ///
    /// # Returns
    ///
    /// A `Packet` ready to be sent to request battery status
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::battery::BatteryPlugin;
    ///
    /// let plugin = BatteryPlugin::new();
    /// let packet = plugin.create_battery_request();
    ///
    /// assert_eq!(packet.packet_type, "cconnect.battery.request");
    /// ```
    pub fn create_battery_request(&self) -> Packet {
        let body = json!({ "request": true });
        Packet::new("cconnect.battery.request", body)
    }

    /// Handle incoming battery status packet
    fn handle_battery_status(&self, packet: &Packet, device: &Device) {
        match serde_json::from_value::<BatteryStatus>(packet.body.clone()) {
            Ok(status) => {
                // Store battery status
                if let Ok(mut battery) = self.battery_status.write() {
                    *battery = Some(status.clone());
                }

                // Log battery status
                if status.has_battery() {
                    let charging_str = if status.is_charging {
                        "charging"
                    } else {
                        "not charging"
                    };
                    let threshold_str = if status.is_low_battery() {
                        " (LOW BATTERY)"
                    } else {
                        ""
                    };

                    info!(
                        "Battery status from {} ({}): {}%, {}{}",
                        device.name(),
                        device.id(),
                        status.current_charge,
                        charging_str,
                        threshold_str
                    );
                } else {
                    info!("Device {} ({}) has no battery", device.name(), device.id());
                }

                debug!("Battery status: {:?}", status);
            }
            Err(e) => {
                warn!(
                    "Failed to parse battery status from {}: {}",
                    device.name(),
                    e
                );
            }
        }
    }

    /// Handle incoming battery request packet
    fn handle_battery_request(&self, _packet: &Packet, device: &Device) {
        info!(
            "Received battery request from {} ({})",
            device.name(),
            device.id()
        );
        // Note: In a full implementation, this would trigger sending our battery status
        // For now, just log the request
        debug!("Battery request handling (deprecated protocol feature)");
    }
}

impl Default for BatteryPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for BatteryPlugin {
    fn name(&self) -> &str {
        "battery"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.battery".to_string(),
            "cconnect.battery.request".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.battery".to_string(),
            "cconnect.battery.request".to_string(),
        ]
    }

    async fn init(&mut self, device: &Device) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!("Battery plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Battery plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Battery plugin stopped");
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        match packet.packet_type.as_str() {
            "cconnect.battery" => {
                self.handle_battery_status(packet, device);
            }
            "cconnect.battery.request" => {
                self.handle_battery_request(packet, device);
            }
            _ => {
                // Ignore other packet types
            }
        }
        Ok(())
    }
}

/// Factory for creating BatteryPlugin instances
#[derive(Debug, Clone, Copy)]
pub struct BatteryPluginFactory;

impl PluginFactory for BatteryPluginFactory {
    fn name(&self) -> &str {
        "battery"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.battery".to_string(),
            "cconnect.battery.request".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.battery".to_string(),
            "cconnect.battery.request".to_string(),
        ]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(BatteryPlugin::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceInfo, DeviceType};

    fn create_test_device() -> Device {
        let info = DeviceInfo::new("Test Device", DeviceType::Phone, 1716);
        Device::from_discovery(info)
    }

    #[test]
    fn test_battery_status_new() {
        let status = BatteryStatus::new(75, true, 0);
        assert_eq!(status.current_charge, 75);
        assert!(status.is_charging);
        assert_eq!(status.threshold_event, 0);
        assert!(status.has_battery());
        assert!(!status.is_low_battery());
    }

    #[test]
    fn test_battery_status_no_battery() {
        let status = BatteryStatus::no_battery();
        assert_eq!(status.current_charge, -1);
        assert!(!status.is_charging);
        assert!(!status.has_battery());
    }

    #[test]
    fn test_battery_status_low_battery() {
        let status = BatteryStatus::new(15, false, 1);
        assert!(status.has_battery());
        assert!(status.is_low_battery());
    }

    #[test]
    fn test_plugin_creation() {
        let plugin = BatteryPlugin::new();
        assert_eq!(plugin.name(), "battery");
        assert!(plugin.get_battery_status().is_none());
    }

    #[test]
    fn test_capabilities() {
        let plugin = BatteryPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 2);
        assert!(incoming.contains(&"cconnect.battery".to_string()));
        assert!(incoming.contains(&"cconnect.battery.request".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 2);
        assert!(outgoing.contains(&"cconnect.battery".to_string()));
        assert!(outgoing.contains(&"cconnect.battery.request".to_string()));
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = BatteryPlugin::new();
        let device = create_test_device();

        plugin.init(&device).await.unwrap();
        assert!(plugin.device_id.is_some());

        plugin.start().await.unwrap();
        plugin.stop().await.unwrap();
    }

    #[test]
    fn test_create_battery_packet() {
        let plugin = BatteryPlugin::new();
        let status = BatteryStatus::new(75, true, 0);
        let packet = plugin.create_battery_packet(&status);

        assert_eq!(packet.packet_type, "cconnect.battery");
        assert_eq!(
            packet.body.get("currentCharge").and_then(|v| v.as_i64()),
            Some(75)
        );
        assert_eq!(
            packet.body.get("isCharging").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            packet.body.get("thresholdEvent").and_then(|v| v.as_i64()),
            Some(0)
        );
    }

    #[test]
    fn test_create_battery_request() {
        let plugin = BatteryPlugin::new();
        let packet = plugin.create_battery_request();

        assert_eq!(packet.packet_type, "cconnect.battery.request");
        assert_eq!(
            packet.body.get("request").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[tokio::test]
    async fn test_handle_battery_status() {
        let mut plugin = BatteryPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();
        let status = BatteryStatus::new(85, false, 0);
        let packet = plugin.create_battery_packet(&status);

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        let stored_status = plugin.get_battery_status().unwrap();
        assert_eq!(stored_status.current_charge, 85);
        assert!(!stored_status.is_charging);
        assert_eq!(stored_status.threshold_event, 0);
    }

    #[tokio::test]
    async fn test_handle_low_battery() {
        let mut plugin = BatteryPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();
        let status = BatteryStatus::new(15, false, 1);
        let packet = plugin.create_battery_packet(&status);

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        let stored_status = plugin.get_battery_status().unwrap();
        assert!(stored_status.is_low_battery());
    }

    #[tokio::test]
    async fn test_handle_no_battery() {
        let mut plugin = BatteryPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();
        let status = BatteryStatus::no_battery();
        let packet = plugin.create_battery_packet(&status);

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        let stored_status = plugin.get_battery_status().unwrap();
        assert!(!stored_status.has_battery());
    }

    #[tokio::test]
    async fn test_handle_battery_request() {
        let mut plugin = BatteryPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();
        let packet = plugin.create_battery_request();

        // Should not error
        plugin.handle_packet(&packet, &mut device).await.unwrap();
    }

    #[tokio::test]
    async fn test_ignore_non_battery_packets() {
        let mut plugin = BatteryPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new("cconnect.ping", json!({}));

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        // Should not have battery status
        assert!(plugin.get_battery_status().is_none());
    }

    #[test]
    fn test_battery_status_serialization() {
        let status = BatteryStatus::new(75, true, 0);
        let json = serde_json::to_value(&status).unwrap();

        assert_eq!(json["currentCharge"], 75);
        assert_eq!(json["isCharging"], true);
        assert_eq!(json["thresholdEvent"], 0);
    }

    #[test]
    fn test_battery_status_deserialization() {
        let json = json!({
            "currentCharge": 50,
            "isCharging": false,
            "thresholdEvent": 1
        });

        let status: BatteryStatus = serde_json::from_value(json).unwrap();
        assert_eq!(status.current_charge, 50);
        assert!(!status.is_charging);
        assert!(status.is_low_battery());
    }
}
