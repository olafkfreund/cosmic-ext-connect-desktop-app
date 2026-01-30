//! Power Management Plugin
//!
//! Enables remote power control of desktop machines (shutdown, reboot, suspend, hibernate).
//! Provides power state monitoring and sleep inhibition.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - Incoming: `cconnect.power.request`, `cconnect.power.inhibit`, `cconnect.power.query`
//! - Outgoing: `cconnect.power.status`
//!
//! **Capabilities**: `cconnect.power`
//!
//! ## Power Action Request
//!
//! Request a power action on the remote desktop:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.power.request",
//!     "body": {
//!         "action": "shutdown"  // "shutdown", "reboot", "suspend", "hibernate"
//!     }
//! }
//! ```
//!
//! ## Sleep Inhibition
//!
//! Prevent the desktop from sleeping:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.power.inhibit",
//!     "body": {
//!         "inhibit": true,
//!         "reason": "File transfer in progress"
//!     }
//! }
//! ```
//!
//! ## Power Status Query
//!
//! Request current power state:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.power.query",
//!     "body": {}
//! }
//! ```
//!
//! ## Power Status Response
//!
//! Report current power state:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.power.status",
//!     "body": {
//!         "state": "running",
//!         "inhibited": false,
//!         "battery_present": false,
//!         "on_battery": false
//!     }
//! }
//! ```
//!
//! ## Security Considerations
//!
//! - Power actions are disabled by default (config: enable_power = false)
//! - Requires explicit opt-in per device
//! - Uses PolicyKit for permission checking
//! - Audit logging for all power actions
//! - Only paired devices can trigger power actions
//!
//! ## System Integration
//!
//! Uses systemd-logind DBus interface for power management:
//! - `PowerOff()` - Shutdown the system
//! - `Reboot()` - Reboot the system
//! - `Suspend()` - Suspend to RAM
//! - `Hibernate()` - Suspend to disk
//! - DBus inhibitor locks for sleep prevention
//!
//! ## Example
//!
//! ```rust,ignore
//! use cosmic_connect_core::plugins::power::*;
//! use cosmic_connect_core::{Plugin, PluginManager};
//!
//! // Create and register plugin
//! let mut manager = PluginManager::new();
//! manager.register(Box::new(PowerPlugin::new()))?;
//!
//! // Shutdown remote desktop
//! let plugin = PowerPlugin::new();
//! let packet = plugin.create_power_request("shutdown");
//! // Send packet to device...
//! ```

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde_json::json;
use std::any::Any;
use tracing::{debug, info, warn};

use super::logind_backend::LogindBackend;
use super::systemd_inhibitor::{InhibitMode, InhibitType, InhibitorLock, SystemdInhibitor};
use super::upower_backend::UPowerBackend;
use super::{Plugin, PluginFactory};

/// Power management plugin for remote power control
pub struct PowerPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Whether the plugin is enabled
    enabled: bool,

    /// Whether sleep is currently inhibited
    sleep_inhibited: bool,

    /// Inhibition reason
    inhibit_reason: Option<String>,

    /// Systemd inhibitor manager
    inhibitor: SystemdInhibitor,

    /// Active inhibitor lock (held to prevent sleep)
    inhibitor_lock: Option<InhibitorLock>,

    /// UPower backend for power state detection
    upower: UPowerBackend,

    /// Logind backend for power actions (shutdown, reboot, suspend, hibernate)
    logind: LogindBackend,
}

impl PowerPlugin {
    /// Create a new Power plugin
    pub fn new() -> Self {
        Self {
            device_id: None,
            enabled: false,
            sleep_inhibited: false,
            inhibit_reason: None,
            inhibitor: SystemdInhibitor::new(),
            inhibitor_lock: None,
            upower: UPowerBackend::new(),
            logind: LogindBackend::new(),
        }
    }

    /// Create a power action request packet
    ///
    /// # Parameters
    ///
    /// - `action`: Power action ("shutdown", "reboot", "suspend", "hibernate")
    ///
    /// # Returns
    ///
    /// Packet requesting power action
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::power::PowerPlugin;
    ///
    /// let plugin = PowerPlugin::new();
    /// let packet = plugin.create_power_request("shutdown");
    /// assert_eq!(packet.packet_type, "cconnect.power.request");
    /// ```
    pub fn create_power_request(&self, action: &str) -> Packet {
        Packet::new(
            "cconnect.power.request",
            json!({
                "action": action
            }),
        )
    }

    /// Create a sleep inhibit request packet
    ///
    /// # Parameters
    ///
    /// - `inhibit`: Whether to inhibit sleep
    /// - `reason`: Reason for inhibition
    ///
    /// # Returns
    ///
    /// Packet requesting sleep inhibition
    pub fn create_inhibit_request(&self, inhibit: bool, reason: &str) -> Packet {
        Packet::new(
            "cconnect.power.inhibit",
            json!({
                "inhibit": inhibit,
                "reason": reason
            }),
        )
    }

    /// Create a power status query packet
    ///
    /// # Returns
    ///
    /// Packet requesting power status
    pub fn create_status_query(&self) -> Packet {
        Packet::new("cconnect.power.query", json!({}))
    }

    /// Create a power status response packet
    ///
    /// # Parameters
    ///
    /// - `state`: Current power state ("running", "charging", "discharging", etc.)
    /// - `inhibited`: Whether sleep is inhibited
    /// - `battery_present`: Whether system has a battery
    /// - `on_battery`: Whether system is running on battery
    /// - `battery_percentage`: Battery charge level (0-100) if available
    /// - `battery_state`: Battery charging state string
    ///
    /// # Returns
    ///
    /// Packet containing power status
    pub fn create_status_response(
        &self,
        state: &str,
        inhibited: bool,
        battery_present: bool,
        on_battery: bool,
        battery_percentage: Option<f64>,
        battery_state: &str,
    ) -> Packet {
        let mut body = json!({
            "state": state,
            "inhibited": inhibited,
            "battery_present": battery_present,
            "on_battery": on_battery,
            "battery_state": battery_state
        });

        // Add battery percentage if available
        if let Some(percentage) = battery_percentage {
            body["battery_percentage"] = json!(percentage);
        }

        Packet::new("cconnect.power.status", body)
    }

    /// Handle power action request
    async fn handle_power_request(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        if let Some(action) = packet.body.get("action").and_then(|v| v.as_str()) {
            info!(
                "Received power request from {} ({}): {}",
                device.name(),
                device.id(),
                action
            );

            // Execute power action
            match action {
                "shutdown" => self.shutdown().await?,
                "reboot" => self.reboot().await?,
                "suspend" => self.suspend().await?,
                "hibernate" => self.hibernate().await?,
                _ => {
                    warn!("Unknown power action: {}", action);
                }
            }
        }

        Ok(())
    }

    /// Handle sleep inhibit request
    async fn handle_inhibit_request(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        if let Some(inhibit) = packet.body.get("inhibit").and_then(|v| v.as_bool()) {
            let reason = packet
                .body
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("Remote device request");

            info!(
                "Received inhibit request from {} ({}): {} - {}",
                device.name(),
                device.id(),
                inhibit,
                reason
            );

            if inhibit {
                // Acquire systemd inhibitor lock
                match self
                    .inhibitor
                    .inhibit(
                        InhibitType::Sleep,
                        "COSMIC Connect",
                        reason,
                        InhibitMode::Block,
                    )
                    .await
                {
                    Ok(lock) => {
                        self.inhibitor_lock = Some(lock);
                        self.sleep_inhibited = true;
                        self.inhibit_reason = Some(reason.to_string());
                        info!("Sleep inhibited via systemd: {}", reason);
                    }
                    Err(e) => {
                        warn!("Failed to acquire systemd inhibitor lock: {}", e);
                        // Still track the request even if lock fails
                        self.sleep_inhibited = true;
                        self.inhibit_reason = Some(reason.to_string());
                    }
                }
            } else {
                // Release inhibitor lock by dropping it
                if self.inhibitor_lock.take().is_some() {
                    info!("Sleep inhibition removed via systemd");
                }
                self.sleep_inhibited = false;
                self.inhibit_reason = None;
            }
        }

        Ok(())
    }

    /// Handle power status query
    async fn handle_status_query(&mut self, _packet: &Packet, device: &Device) -> Result<()> {
        info!(
            "Received status query from {} ({})",
            device.name(),
            device.id()
        );

        // Query current power state via UPower
        let (state, battery_present, on_battery, battery_percentage, battery_state) =
            match self.upower.get_power_status().await {
                Ok(status) => {
                    let battery_state_str = status.battery_state.as_str();
                    let state = if status.battery_present {
                        battery_state_str
                    } else {
                        "running"
                    };
                    (
                        state,
                        status.battery_present,
                        status.on_battery,
                        status.battery_percentage,
                        battery_state_str,
                    )
                }
                Err(e) => {
                    warn!("Failed to query UPower: {}", e);
                    ("running", false, false, None, "unknown")
                }
            };

        debug!(
            "Power status: state={}, battery_present={}, on_battery={}, percentage={:?}",
            state, battery_present, on_battery, battery_percentage
        );

        // Create status response packet
        let _response = self.create_status_response(
            state,
            self.sleep_inhibited,
            battery_present,
            on_battery,
            battery_percentage,
            battery_state,
        );

        // TODO: Send status response packet back to device
        // device.send_packet(&response).await?;

        Ok(())
    }

    /// Get current power status (for external access)
    pub async fn get_power_status(&mut self) -> (bool, bool, Option<f64>, &'static str) {
        match self.upower.get_power_status().await {
            Ok(status) => (
                status.on_battery,
                status.battery_present,
                status.battery_percentage,
                status.battery_state.as_str(),
            ),
            Err(_) => (false, false, None, "unknown"),
        }
    }

    /// Convert logind error to protocol error
    fn logind_error(action: &str, e: String) -> crate::ProtocolError {
        crate::ProtocolError::invalid_state(format!("Failed to {}: {}", action, e))
    }

    /// Shutdown the system via logind DBus
    async fn shutdown(&mut self) -> Result<()> {
        self.logind.power_off(false).await.map_err(|e| Self::logind_error("shutdown", e))
    }

    /// Reboot the system via logind DBus
    async fn reboot(&mut self) -> Result<()> {
        self.logind.reboot(false).await.map_err(|e| Self::logind_error("reboot", e))
    }

    /// Suspend the system (suspend to RAM) via logind DBus
    async fn suspend(&mut self) -> Result<()> {
        self.logind.suspend(false).await.map_err(|e| Self::logind_error("suspend", e))
    }

    /// Hibernate the system (suspend to disk) via logind DBus
    async fn hibernate(&mut self) -> Result<()> {
        self.logind.hibernate(false).await.map_err(|e| Self::logind_error("hibernate", e))
    }
}

impl Default for PowerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for PowerPlugin {
    fn name(&self) -> &str {
        "power"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.power.request".to_string(),
            "cconnect.power.inhibit".to_string(),
            "cconnect.power.query".to_string(),
            "kdeconnect.power.request".to_string(),
            "kdeconnect.power.inhibit".to_string(),
            "kdeconnect.power.query".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec!["cconnect.power.status".to_string()]
    }

    async fn init(&mut self, device: &Device, _packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!("Power plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Power plugin started");
        self.enabled = true;
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Power plugin stopped");
        self.enabled = false;

        // Release any sleep inhibitors
        if self.sleep_inhibited {
            if self.inhibitor_lock.take().is_some() {
                info!("Released systemd inhibitor lock on plugin stop");
            }
            self.sleep_inhibited = false;
            self.inhibit_reason = None;
        }

        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("Power plugin is disabled, ignoring packet");
            return Ok(());
        }

        match packet.packet_type.as_str() {
            "cconnect.power.request" => self.handle_power_request(packet, device).await,
            "cconnect.power.inhibit" => self.handle_inhibit_request(packet, device).await,
            "cconnect.power.query" => self.handle_status_query(packet, device).await,
            _ => Ok(()),
        }
    }
}

/// Factory for creating Power plugin instances
pub struct PowerPluginFactory;

impl PluginFactory for PowerPluginFactory {
    fn create(&self) -> Box<dyn Plugin> {
        Box::new(PowerPlugin::new())
    }

    fn name(&self) -> &str {
        "power"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.power.request".to_string(),
            "cconnect.power.inhibit".to_string(),
            "cconnect.power.query".to_string(),
            "kdeconnect.power.request".to_string(),
            "kdeconnect.power.inhibit".to_string(),
            "kdeconnect.power.query".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec!["cconnect.power.status".to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceInfo, DeviceType};

    fn create_test_device() -> Device {
        Device::new(
            DeviceInfo {
                device_id: "test_device".to_string(),
                device_name: "Test Device".to_string(),
                device_type: DeviceType::Desktop,
                protocol_version: 7,
                incoming_capabilities: vec!["cconnect.power".to_string()],
                outgoing_capabilities: vec!["cconnect.power".to_string()],
                tcp_port: 1716,
            },
            crate::ConnectionState::Disconnected,
            crate::PairingStatus::Paired,
        )
    }

    #[test]
    fn test_create_power_request() {
        let plugin = PowerPlugin::new();

        let packet = plugin.create_power_request("shutdown");
        assert_eq!(packet.packet_type, "cconnect.power.request");
        assert_eq!(packet.body.get("action"), Some(&json!("shutdown")));

        let packet = plugin.create_power_request("reboot");
        assert_eq!(packet.body.get("action"), Some(&json!("reboot")));
    }

    #[test]
    fn test_create_inhibit_request() {
        let plugin = PowerPlugin::new();

        let packet = plugin.create_inhibit_request(true, "File transfer");
        assert_eq!(packet.packet_type, "cconnect.power.inhibit");
        assert_eq!(packet.body.get("inhibit"), Some(&json!(true)));
        assert_eq!(packet.body.get("reason"), Some(&json!("File transfer")));
    }

    #[test]
    fn test_create_status_query() {
        let plugin = PowerPlugin::new();

        let packet = plugin.create_status_query();
        assert_eq!(packet.packet_type, "cconnect.power.query");
    }

    #[test]
    fn test_create_status_response() {
        let plugin = PowerPlugin::new();

        // Test with battery
        let packet = plugin.create_status_response(
            "discharging",
            false,
            true,
            true,
            Some(75.5),
            "discharging",
        );
        assert_eq!(packet.packet_type, "cconnect.power.status");
        assert_eq!(packet.body.get("state"), Some(&json!("discharging")));
        assert_eq!(packet.body.get("inhibited"), Some(&json!(false)));
        assert_eq!(packet.body.get("battery_present"), Some(&json!(true)));
        assert_eq!(packet.body.get("on_battery"), Some(&json!(true)));
        assert_eq!(packet.body.get("battery_percentage"), Some(&json!(75.5)));
        assert_eq!(packet.body.get("battery_state"), Some(&json!("discharging")));

        // Test without battery
        let packet2 = plugin.create_status_response("running", false, false, false, None, "unknown");
        assert_eq!(packet2.body.get("battery_present"), Some(&json!(false)));
        assert!(packet2.body.get("battery_percentage").is_none());
    }

    #[test]
    fn test_plugin_capabilities() {
        let plugin = PowerPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 6);
        assert!(incoming.contains(&"cconnect.power.request".to_string()));
        assert!(incoming.contains(&"cconnect.power.inhibit".to_string()));
        assert!(incoming.contains(&"cconnect.power.query".to_string()));
        assert!(incoming.contains(&"kdeconnect.power.request".to_string()));
        assert!(incoming.contains(&"kdeconnect.power.inhibit".to_string()));
        assert!(incoming.contains(&"kdeconnect.power.query".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 1);
        assert!(outgoing.contains(&"cconnect.power.status".to_string()));
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = PowerPlugin::new();
        let device = create_test_device();

        assert!(plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.is_ok());
        assert_eq!(plugin.device_id, Some("test_device".to_string()));

        assert!(plugin.start().await.is_ok());
        assert!(plugin.enabled);

        assert!(plugin.stop().await.is_ok());
        assert!(!plugin.enabled);
    }
}
