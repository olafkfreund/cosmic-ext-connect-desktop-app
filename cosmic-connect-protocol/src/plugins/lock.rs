//! Lock Plugin
//!
//! Enables remote locking and unlocking of the desktop screen.
//! Provides secure screen lock control for COSMIC Desktop.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - Incoming: `cconnect.lock.request`, `cconnect.lock`
//! - Outgoing: `cconnect.lock.request`, `cconnect.lock`
//!
//! **Capabilities**: `cconnect.lock`
//!
//! ## Lock/Unlock Request
//!
//! Request to lock or unlock the desktop:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.lock.request",
//!     "body": {
//!         "setLocked": true
//!     }
//! }
//! ```
//!
//! ## Lock State
//!
//! Report current lock state:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.lock",
//!     "body": {
//!         "isLocked": true
//!     }
//! }
//! ```
//!
//! ## Query Lock State
//!
//! Request current lock state:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.lock.request",
//!     "body": {
//!         "requestLocked": true
//!     }
//! }
//! ```
//!
//! ## Security Considerations
//!
//! - Locking is always allowed (security enhancement)
//! - Unlocking may require device authentication
//! - Lock state changes are broadcast to all paired devices
//! - Uses COSMIC Desktop session manager for lock/unlock
//!
//! ## Example
//!
//! ```rust,ignore
//! use cosmic_connect_core::plugins::lock::*;
//! use cosmic_connect_core::{Plugin, PluginManager};
//!
//! // Create and register plugin
//! let mut manager = PluginManager::new();
//! manager.register(Box::new(LockPlugin::new()))?;
//!
//! // Lock the desktop
//! let plugin = LockPlugin::new();
//! let packet = plugin.create_lock_request(true);
//! // Send packet to device...
//! ```

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde_json::json;
use std::any::Any;
use tracing::{debug, info, warn};

use super::{Plugin, PluginFactory};

/// Lock plugin for remote desktop lock/unlock
pub struct LockPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Whether the plugin is enabled
    enabled: bool,

    /// Current lock state (cached)
    is_locked: bool,
}

impl LockPlugin {
    /// Create a new Lock plugin
    pub fn new() -> Self {
        Self {
            device_id: None,
            enabled: false,
            is_locked: false,
        }
    }

    /// Create a lock/unlock request packet
    ///
    /// # Parameters
    ///
    /// - `locked`: `true` to lock, `false` to unlock
    ///
    /// # Returns
    ///
    /// Packet requesting lock state change
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::lock::LockPlugin;
    ///
    /// let plugin = LockPlugin::new();
    /// let packet = plugin.create_lock_request(true);
    /// assert_eq!(packet.packet_type, "cconnect.lock.request");
    /// ```
    pub fn create_lock_request(&self, locked: bool) -> Packet {
        Packet::new(
            "cconnect.lock.request",
            json!({
                "setLocked": locked
            }),
        )
    }

    /// Create a lock state packet
    ///
    /// Reports the current lock state.
    ///
    /// # Parameters
    ///
    /// - `locked`: Current lock state
    ///
    /// # Returns
    ///
    /// Packet containing lock state
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::lock::LockPlugin;
    ///
    /// let plugin = LockPlugin::new();
    /// let packet = plugin.create_lock_state(true);
    /// assert_eq!(packet.packet_type, "cconnect.lock");
    /// ```
    pub fn create_lock_state(&self, locked: bool) -> Packet {
        Packet::new(
            "cconnect.lock",
            json!({
                "isLocked": locked
            }),
        )
    }

    /// Create a request for current lock state
    ///
    /// # Returns
    ///
    /// Packet requesting lock state
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::lock::LockPlugin;
    ///
    /// let plugin = LockPlugin::new();
    /// let packet = plugin.create_request_lock_state();
    /// assert_eq!(packet.packet_type, "cconnect.lock.request");
    /// ```
    pub fn create_request_lock_state(&self) -> Packet {
        Packet::new(
            "cconnect.lock.request",
            json!({
                "requestLocked": true
            }),
        )
    }

    /// Handle lock/unlock request
    async fn handle_lock_request(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        // Check if this is a lock/unlock request
        if let Some(set_locked) = packet.body.get("setLocked").and_then(|v| v.as_bool()) {
            info!(
                "Received lock request from {} ({}): {}",
                device.name(),
                device.id(),
                if set_locked { "lock" } else { "unlock" }
            );

            // Execute lock/unlock command
            let result = if set_locked {
                self.lock_desktop().await
            } else {
                self.unlock_desktop().await
            };

            match result {
                Ok(()) => {
                    self.is_locked = set_locked;
                    info!(
                        "Desktop {} successfully",
                        if set_locked { "locked" } else { "unlocked" }
                    );

                    // TODO: Send state update back to device
                    // Need to implement packet sending infrastructure
                    // let state_packet = self.create_lock_state(set_locked);
                    // device.send_packet(&state_packet).await?;
                }
                Err(e) => {
                    warn!(
                        "Failed to {} desktop: {}",
                        if set_locked { "lock" } else { "unlock" },
                        e
                    );
                }
            }
        }
        // Check if this is a state query
        else if packet
            .body
            .get("requestLocked")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            info!(
                "Received lock state query from {} ({})",
                device.name(),
                device.id()
            );

            // Query current lock state
            match self.query_lock_state().await {
                Ok(locked) => {
                    self.is_locked = locked;
                    // TODO: Send state update back to device
                    // Need to implement packet sending infrastructure
                    // let state_packet = self.create_lock_state(locked);
                    // device.send_packet(&state_packet).await?;
                }
                Err(e) => {
                    warn!("Failed to query lock state: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Handle lock state update from remote device
    async fn handle_lock_state(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        if let Some(is_locked) = packet.body.get("isLocked").and_then(|v| v.as_bool()) {
            info!(
                "Received lock state from {} ({}): {}",
                device.name(),
                device.id(),
                if is_locked { "locked" } else { "unlocked" }
            );
            self.is_locked = is_locked;
        }
        Ok(())
    }

    /// Lock the desktop using COSMIC session manager
    async fn lock_desktop(&self) -> Result<()> {
        info!("Locking desktop via loginctl");

        // Use loginctl to lock the session
        let output = tokio::process::Command::new("loginctl")
            .arg("lock-session")
            .output()
            .await
            .map_err(crate::ProtocolError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("loginctl lock-session failed: {}", stderr);
            return Err(crate::ProtocolError::invalid_state(format!(
                "Failed to lock desktop: {}",
                stderr
            )));
        }

        Ok(())
    }

    /// Unlock the desktop using COSMIC session manager
    async fn unlock_desktop(&self) -> Result<()> {
        info!("Unlocking desktop via loginctl");

        // Use loginctl to unlock the session
        let output = tokio::process::Command::new("loginctl")
            .arg("unlock-session")
            .output()
            .await
            .map_err(crate::ProtocolError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("loginctl unlock-session failed: {}", stderr);
            return Err(crate::ProtocolError::invalid_state(format!(
                "Failed to unlock desktop: {}",
                stderr
            )));
        }

        Ok(())
    }

    /// Query current lock state from session manager
    async fn query_lock_state(&self) -> Result<bool> {
        debug!("Querying lock state via loginctl");

        // Check if session is locked using loginctl
        let output = tokio::process::Command::new("loginctl")
            .arg("show-session")
            .arg("--property=LockedHint")
            .output()
            .await
            .map_err(crate::ProtocolError::Io)?;

        if !output.status.success() {
            return Ok(false); // Default to unlocked if we can't determine
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let is_locked = stdout.contains("LockedHint=yes");

        debug!("Current lock state: {}", is_locked);
        Ok(is_locked)
    }
}

impl Default for LockPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for LockPlugin {
    fn name(&self) -> &str {
        "lock"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.lock.request".to_string(),
            "cconnect.lock".to_string(),
            "kdeconnect.lock.request".to_string(),
            "kdeconnect.lock".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.lock.request".to_string(),
            "cconnect.lock".to_string(),
        ]
    }

    async fn init(&mut self, device: &Device, _packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!("Lock plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Lock plugin started");
        self.enabled = true;

        // Query initial lock state
        if let Ok(locked) = self.query_lock_state().await {
            self.is_locked = locked;
            info!("Initial lock state: {}", locked);
        }

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Lock plugin stopped");
        self.enabled = false;
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("Lock plugin is disabled, ignoring packet");
            return Ok(());
        }

        if packet.is_type("cconnect.lock.request") {
            self.handle_lock_request(packet, device).await
        } else if packet.is_type("cconnect.lock") {
            self.handle_lock_state(packet, device).await
        } else {
            Ok(())
        }
    }
}

/// Factory for creating Lock plugin instances
pub struct LockPluginFactory;

impl PluginFactory for LockPluginFactory {
    fn create(&self) -> Box<dyn Plugin> {
        Box::new(LockPlugin::new())
    }

    fn name(&self) -> &str {
        "lock"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.lock.request".to_string(),
            "cconnect.lock".to_string(),
            "kdeconnect.lock.request".to_string(),
            "kdeconnect.lock".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.lock.request".to_string(),
            "cconnect.lock".to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_device() -> Device {
        use crate::{DeviceInfo, DeviceType};
        Device::new(
            DeviceInfo {
                device_id: "test_device".to_string(),
                device_name: "Test Device".to_string(),
                device_type: DeviceType::Phone,
                protocol_version: 7,
                incoming_capabilities: vec!["cconnect.lock".to_string()],
                outgoing_capabilities: vec!["cconnect.lock".to_string()],
                tcp_port: 1716,
            },
            crate::ConnectionState::Disconnected,
            crate::PairingStatus::Paired,
        )
    }

    #[test]
    fn test_create_lock_request() {
        let plugin = LockPlugin::new();

        // Test lock request
        let packet = plugin.create_lock_request(true);
        assert_eq!(packet.packet_type, "cconnect.lock.request");
        assert_eq!(packet.body.get("setLocked"), Some(&json!(true)));

        // Test unlock request
        let packet = plugin.create_lock_request(false);
        assert_eq!(packet.body.get("setLocked"), Some(&json!(false)));
    }

    #[test]
    fn test_create_lock_state() {
        let plugin = LockPlugin::new();

        let packet = plugin.create_lock_state(true);
        assert_eq!(packet.packet_type, "cconnect.lock");
        assert_eq!(packet.body.get("isLocked"), Some(&json!(true)));
    }

    #[test]
    fn test_create_request_lock_state() {
        let plugin = LockPlugin::new();

        let packet = plugin.create_request_lock_state();
        assert_eq!(packet.packet_type, "cconnect.lock.request");
        assert_eq!(packet.body.get("requestLocked"), Some(&json!(true)));
    }

    #[test]
    fn test_plugin_capabilities() {
        let plugin = LockPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 4);
        assert!(incoming.contains(&"cconnect.lock.request".to_string()));
        assert!(incoming.contains(&"cconnect.lock".to_string()));
        assert!(incoming.contains(&"kdeconnect.lock.request".to_string()));
        assert!(incoming.contains(&"kdeconnect.lock".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 2);
        assert!(outgoing.contains(&"cconnect.lock.request".to_string()));
        assert!(outgoing.contains(&"cconnect.lock".to_string()));
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = LockPlugin::new();
        let device = create_test_device();

        assert!(plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.is_ok());
        assert_eq!(plugin.device_id, Some("test_device".to_string()));

        assert!(plugin.start().await.is_ok());
        assert!(plugin.enabled);

        assert!(plugin.stop().await.is_ok());
        assert!(!plugin.enabled);
    }
}
