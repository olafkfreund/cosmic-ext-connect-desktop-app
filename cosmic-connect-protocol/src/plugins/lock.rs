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
//! use cosmic_connect_protocol::plugins::lock::*;
//! use cosmic_connect_protocol::{Plugin, PluginManager};
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
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

use super::logind_backend::LogindBackend;
use super::{Plugin, PluginFactory};

/// Lock plugin for remote desktop lock/unlock
pub struct LockPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Whether the plugin is enabled
    enabled: bool,

    /// Current lock state (thread-safe cached)
    lock_state: Arc<RwLock<bool>>,

    /// Logind DBus backend for screen lock control
    logind_backend: LogindBackend,

    /// Packet sender for response packets
    packet_sender: Option<tokio::sync::mpsc::Sender<(String, Packet)>>,
}

impl LockPlugin {
    /// Create a new Lock plugin
    pub fn new() -> Self {
        Self {
            device_id: None,
            enabled: false,
            lock_state: Arc::new(RwLock::new(false)),
            logind_backend: LogindBackend::new(),
            packet_sender: None,
        }
    }

    /// Check if the desktop is currently locked
    ///
    /// Returns the cached lock state. This is updated when lock state
    /// changes are received from remote devices or when queried locally.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::lock::LockPlugin;
    ///
    /// let plugin = LockPlugin::new();
    /// let is_locked = plugin.is_locked();
    /// println!("Desktop locked: {}", is_locked);
    /// ```
    pub fn is_locked(&self) -> bool {
        self.lock_state.read().map(|guard| *guard).unwrap_or(false)
    }

    /// Get the lock state Arc for external monitoring
    ///
    /// This allows external code to hold a reference to the lock state
    /// and receive updates when the state changes.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::lock::LockPlugin;
    ///
    /// let plugin = LockPlugin::new();
    /// let lock_state = plugin.get_lock_state();
    /// // Can be cloned and passed to UI components
    /// ```
    pub fn get_lock_state(&self) -> Arc<RwLock<bool>> {
        Arc::clone(&self.lock_state)
    }

    /// Update the cached lock state
    fn set_lock_state(&self, locked: bool) {
        if let Ok(mut guard) = self.lock_state.write() {
            *guard = locked;
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
    /// use cosmic_connect_protocol::plugins::lock::LockPlugin;
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
    /// use cosmic_connect_protocol::plugins::lock::LockPlugin;
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
    /// use cosmic_connect_protocol::plugins::lock::LockPlugin;
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
            self.handle_set_locked(set_locked, device).await?;
            return Ok(());
        }

        // Check if this is a state query
        let is_state_query = packet
            .body
            .get("requestLocked")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if is_state_query {
            self.handle_lock_state_query(device).await?;
        }

        Ok(())
    }

    /// Handle a set locked request
    async fn handle_set_locked(&mut self, set_locked: bool, device: &Device) -> Result<()> {
        let action = if set_locked { "lock" } else { "unlock" };
        info!(
            "Received {} request from {} ({})",
            action,
            device.name(),
            device.id()
        );

        let result = if set_locked {
            self.lock_desktop().await
        } else {
            self.unlock_desktop().await
        };

        match result {
            Ok(()) => {
                self.set_lock_state(set_locked);

                // Send state update back to device
                let state_packet = self.create_lock_state(set_locked);
                if let (Some(device_id), Some(sender)) = (&self.device_id, &self.packet_sender) {
                    if let Err(e) = sender.send((device_id.clone(), state_packet)).await {
                        warn!("Failed to send lock state packet: {}", e);
                    }
                } else {
                    warn!("Cannot send lock state - plugin not properly initialized");
                }
            }
            Err(e) => {
                warn!("Failed to {} desktop: {}", action, e);
            }
        }

        Ok(())
    }

    /// Handle a lock state query request
    async fn handle_lock_state_query(&mut self, device: &Device) -> Result<()> {
        info!(
            "Received lock state query from {} ({})",
            device.name(),
            device.id()
        );

        match self.query_lock_state().await {
            Ok(locked) => {
                self.set_lock_state(locked);

                // Send state update back to device
                let state_packet = self.create_lock_state(locked);
                if let (Some(device_id), Some(sender)) = (&self.device_id, &self.packet_sender) {
                    if let Err(e) = sender.send((device_id.clone(), state_packet)).await {
                        warn!("Failed to send lock state packet: {}", e);
                    }
                } else {
                    warn!("Cannot send lock state - plugin not properly initialized");
                }
            }
            Err(e) => {
                warn!("Failed to query lock state: {}", e);
            }
        }

        Ok(())
    }

    /// Handle lock state update from remote device
    async fn handle_lock_state(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        if let Some(locked) = packet.body.get("isLocked").and_then(|v| v.as_bool()) {
            info!(
                "Received lock state from {} ({}): {}",
                device.name(),
                device.id(),
                if locked { "locked" } else { "unlocked" }
            );
            self.set_lock_state(locked);
        }
        Ok(())
    }

    /// Lock the desktop using logind DBus
    async fn lock_desktop(&mut self) -> Result<()> {
        self.logind_backend.lock().await.map_err(|e| {
            crate::ProtocolError::invalid_state(format!("Failed to lock desktop: {}", e))
        })
    }

    /// Unlock the desktop using logind DBus
    async fn unlock_desktop(&mut self) -> Result<()> {
        self.logind_backend.unlock().await.map_err(|e| {
            crate::ProtocolError::invalid_state(format!("Failed to unlock desktop: {}", e))
        })
    }

    /// Query current lock state from logind DBus
    async fn query_lock_state(&mut self) -> Result<bool> {
        debug!("Querying lock state via logind DBus");

        let is_locked = self.logind_backend.is_locked().await.unwrap_or(false);

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

    async fn init(
        &mut self,
        device: &Device,
        packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>,
    ) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        self.packet_sender = Some(packet_sender);
        info!("Lock plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Lock plugin started");
        self.enabled = true;

        // Connect to logind DBus
        if let Err(e) = self.logind_backend.connect().await {
            warn!("Failed to connect to logind DBus: {}", e);
            // Continue anyway - will try to connect on first use
        }

        // Query initial lock state
        if let Ok(locked) = self.query_lock_state().await {
            self.set_lock_state(locked);
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

        if packet.is_type("cconnect.lock.request") || packet.is_type("kdeconnect.lock.request") {
            self.handle_lock_request(packet, device).await
        } else if packet.is_type("cconnect.lock") || packet.is_type("kdeconnect.lock") {
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
    use crate::{DeviceInfo, DeviceType};

    fn create_test_device() -> Device {
        let info = DeviceInfo::new("Test Device", DeviceType::Phone, 1716);
        Device::from_discovery(info)
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

        plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .unwrap();
        assert!(plugin.device_id.is_some());

        plugin.start().await.unwrap();
        assert!(plugin.enabled);

        plugin.stop().await.unwrap();
        assert!(!plugin.enabled);
    }

    #[test]
    fn test_is_locked_initial_state() {
        let plugin = LockPlugin::new();
        // Initially not locked
        assert!(!plugin.is_locked());
    }

    #[test]
    fn test_lock_state_updates() {
        let plugin = LockPlugin::new();

        // Initial state is unlocked
        assert!(!plugin.is_locked());

        // Set to locked
        plugin.set_lock_state(true);
        assert!(plugin.is_locked());

        // Set back to unlocked
        plugin.set_lock_state(false);
        assert!(!plugin.is_locked());
    }

    #[test]
    fn test_get_lock_state_arc() {
        let plugin = LockPlugin::new();
        let lock_state = plugin.get_lock_state();

        // Initial state
        assert!(!*lock_state.read().unwrap());

        // Update via plugin
        plugin.set_lock_state(true);

        // Arc should reflect the change
        assert!(*lock_state.read().unwrap());
    }

    #[test]
    fn test_lock_state_thread_safety() {
        use std::thread;

        let plugin = LockPlugin::new();
        let lock_state = plugin.get_lock_state();

        // Spawn multiple threads that read the lock state
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let state = Arc::clone(&lock_state);
                thread::spawn(move || {
                    for _ in 0..100 {
                        let _ = *state.read().unwrap();
                    }
                })
            })
            .collect();

        // Update state while threads are reading
        for i in 0..100 {
            plugin.set_lock_state(i % 2 == 0);
        }

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[tokio::test]
    async fn test_handle_lock_state_packet() {
        let mut plugin = LockPlugin::new();
        let device = create_test_device();

        plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .unwrap();
        plugin.start().await.unwrap();

        // Initial state is unlocked
        assert!(!plugin.is_locked());

        // Receive locked state from remote device
        let packet = Packet::new("cconnect.lock", json!({ "isLocked": true }));
        let mut test_device = create_test_device();
        plugin
            .handle_packet(&packet, &mut test_device)
            .await
            .unwrap();

        // State should be updated
        assert!(plugin.is_locked());

        // Receive unlocked state
        let packet = Packet::new("cconnect.lock", json!({ "isLocked": false }));
        plugin
            .handle_packet(&packet, &mut test_device)
            .await
            .unwrap();

        assert!(!plugin.is_locked());
    }
}
