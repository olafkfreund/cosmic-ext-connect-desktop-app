//! KDE Connect Plugin Architecture
//!
//! This module provides the plugin trait and architecture for extending KDE Connect
//! functionality. Plugins handle specific packet types and provide features like battery
//! monitoring, notifications, media control, etc.
//!
//! ## Plugin Architecture
//!
//! KDE Connect uses a **capability-based plugin system** where devices advertise their
//! capabilities through identity packets. This enables selective feature negotiation
//! without requiring all implementations to support every feature.
//!
//! ### Core Principles
//!
//! 1. **Capability-Driven**: Plugins declare incoming/outgoing capabilities
//! 2. **Idempotent Handling**: Plugins must handle unexpected/repeated packets gracefully
//! 3. **No Guarantee Semantics**: Packets may be lost; plugins shouldn't depend on responses
//! 4. **Async-First**: All plugin operations are async for network I/O
//!
//! ### Packet Types
//!
//! Plugin packet types follow the pattern `kdeconnect.<plugin>[.<action>]`:
//! - `kdeconnect.battery` - Battery status broadcast
//! - `kdeconnect.battery.request` - Request battery status
//! - `kdeconnect.mpris` - Media player state
//! - `kdeconnect.mpris.request` - Media player commands
//!
//! ### Plugin Categories
//!
//! - **Device Status**: Battery, Connectivity Report, Lock
//! - **Communication**: SMS, Telephony, Notifications
//! - **Content Sharing**: Clipboard, Share, Contacts
//! - **Remote Control**: MousePad, MPRIS, RunCommand
//! - **Utility**: Ping, FindMyPhone, Presenter
//! - **File Access**: SFTP
//!
//! ## Plugin Lifecycle
//!
//! ```text
//! Created → Initialized → Started → Running → Stopped
//!                ↑                              ↓
//!                └──────────── Restart ─────────┘
//! ```
//!
//! - **Created**: Plugin instance created
//! - **Initialized**: Plugin configured with device context
//! - **Started**: Plugin begins processing packets
//! - **Stopped**: Plugin cleanly shuts down
//!
//! ## Example Plugin
//!
//! ```rust,ignore
//! use kdeconnect_protocol::plugins::*;
//! use async_trait::async_trait;
//!
//! struct PingPlugin;
//!
//! #[async_trait]
//! impl Plugin for PingPlugin {
//!     fn name(&self) -> &str {
//!         "ping"
//!     }
//!
//!     fn incoming_capabilities(&self) -> Vec<String> {
//!         vec!["kdeconnect.ping".to_string()]
//!     }
//!
//!     fn outgoing_capabilities(&self) -> Vec<String> {
//!         vec!["kdeconnect.ping".to_string()]
//!     }
//!
//!     async fn init(&mut self, _device: &Device) -> Result<()> {
//!         Ok(())
//!     }
//!
//!     async fn start(&mut self) -> Result<()> {
//!         info!("Ping plugin started");
//!         Ok(())
//!     }
//!
//!     async fn stop(&mut self) -> Result<()> {
//!         info!("Ping plugin stopped");
//!         Ok(())
//!     }
//!
//!     async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
//!         if packet.packet_type == "kdeconnect.ping" {
//!             info!("Received ping from {}", device.name());
//!             // Handle ping...
//!         }
//!         Ok(())
//!     }
//! }
//! ```
//!
//! ## References
//!
//! - [Valent Protocol Reference](https://valent.andyholmes.ca/documentation/protocol.html)
//! - [KDE Connect Community Wiki](https://community.kde.org/KDEConnect)
//! - [KDE Connect GitHub](https://github.com/KDE/kdeconnect-kde)

pub mod battery;
pub mod clipboard;
pub mod notification;
pub mod ping;
pub mod share;

use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Plugin trait for extending KDE Connect functionality
///
/// Plugins must implement this trait to handle specific packet types and provide
/// protocol features. All methods are async to support network I/O operations.
///
/// ## Thread Safety
///
/// Plugins must be `Send + Sync` to support concurrent access across async tasks.
///
/// ## Packet Handling
///
/// Plugins should:
/// - Handle packets idempotently (repeated packets should not cause errors)
/// - Not depend on receiving responses to sent packets
/// - Gracefully handle unexpected packet formats
/// - Log errors but continue operation when possible
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Get the plugin name
    ///
    /// This should be a short, lowercase identifier like "ping", "battery", "mpris".
    fn name(&self) -> &str;

    /// Get list of incoming packet types this plugin can handle
    ///
    /// These are packet types the plugin can **receive** and process.
    /// Format: `kdeconnect.<plugin>[.<action>]`
    ///
    /// Example: `["kdeconnect.ping", "kdeconnect.battery"]`
    fn incoming_capabilities(&self) -> Vec<String>;

    /// Get list of outgoing packet types this plugin can send
    ///
    /// These are packet types the plugin can **send** to other devices.
    /// Format: `kdeconnect.<plugin>[.<action>]`
    ///
    /// Example: `["kdeconnect.ping", "kdeconnect.battery.request"]`
    fn outgoing_capabilities(&self) -> Vec<String>;

    /// Initialize the plugin with device context
    ///
    /// Called once after plugin creation to provide device-specific configuration.
    /// The plugin should store any needed device information but not start
    /// processing packets yet.
    ///
    /// # Errors
    ///
    /// Returns error if plugin initialization fails (e.g., invalid device state,
    /// missing required capabilities).
    async fn init(&mut self, device: &Device) -> Result<()>;

    /// Start the plugin
    ///
    /// Called when the plugin should begin processing packets and performing
    /// background tasks. The plugin should be ready to handle packets after
    /// this method returns.
    ///
    /// # Errors
    ///
    /// Returns error if plugin cannot start (e.g., resource allocation failure).
    async fn start(&mut self) -> Result<()>;

    /// Stop the plugin
    ///
    /// Called when the plugin should cleanly shut down. The plugin should:
    /// - Stop processing new packets
    /// - Complete or cancel in-flight operations
    /// - Release resources
    /// - Save state if necessary
    ///
    /// # Errors
    ///
    /// Returns error if plugin cannot stop cleanly.
    async fn stop(&mut self) -> Result<()>;

    /// Handle an incoming packet
    ///
    /// Called when a packet matching one of the plugin's incoming capabilities
    /// is received. The plugin should:
    /// - Validate the packet format
    /// - Process the packet idempotently
    /// - Update device state if needed
    /// - Send response packets if appropriate
    /// - Return Ok(()) even if packet is malformed (log error instead)
    ///
    /// # Parameters
    ///
    /// - `packet`: The received packet
    /// - `device`: Mutable reference to the device for state updates
    ///
    /// # Errors
    ///
    /// Should only return error for critical failures (e.g., device disconnection).
    /// Malformed packets should be logged but not cause errors.
    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()>;

    /// Check if plugin is ready to handle packets
    ///
    /// Optional method for plugins that need startup time (e.g., loading state).
    /// Default implementation returns true.
    fn is_ready(&self) -> bool {
        true
    }

    /// Get plugin version for compatibility checking
    ///
    /// Optional method for plugins that track version compatibility.
    /// Default returns protocol version 7.
    fn version(&self) -> u32 {
        7
    }
}

/// Plugin registry and packet router
///
/// Manages multiple plugins and routes incoming packets to the appropriate plugin
/// based on packet type. Handles plugin lifecycle (init, start, stop) and maintains
/// capability-to-plugin mappings.
///
/// ## Example
///
/// ```rust,ignore
/// use kdeconnect_protocol::plugins::*;
///
/// # async fn example() -> Result<()> {
/// let mut manager = PluginManager::new();
///
/// // Register plugins
/// manager.register(Box::new(PingPlugin))?;
/// manager.register(Box::new(BatteryPlugin))?;
///
/// // Initialize and start all plugins
/// manager.init_all(&device).await?;
/// manager.start_all().await?;
///
/// // Route packet to appropriate plugin
/// if let Some(packet) = receive_packet().await {
///     manager.handle_packet(&packet, &mut device).await?;
/// }
///
/// // Shutdown
/// manager.stop_all().await?;
/// # Ok(())
/// # }
/// ```
pub struct PluginManager {
    /// Registered plugins by name
    plugins: HashMap<String, Box<dyn Plugin>>,

    /// Mapping from incoming capability to plugin name
    capability_map: HashMap<String, String>,
}

impl PluginManager {
    /// Create a new empty plugin manager
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            capability_map: HashMap::new(),
        }
    }

    /// Register a new plugin
    ///
    /// Adds the plugin to the registry and builds capability mappings.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - A plugin with the same name is already registered
    /// - A capability is already handled by another plugin
    pub fn register(&mut self, plugin: Box<dyn Plugin>) -> Result<()> {
        let name = plugin.name().to_string();

        // Check for duplicate plugin name
        if self.plugins.contains_key(&name) {
            return Err(ProtocolError::Plugin(format!(
                "Plugin '{}' is already registered",
                name
            )));
        }

        // Build capability mappings
        for capability in plugin.incoming_capabilities() {
            if let Some(existing) = self.capability_map.get(&capability) {
                return Err(ProtocolError::Plugin(format!(
                    "Capability '{}' already handled by plugin '{}'",
                    capability, existing
                )));
            }
            self.capability_map.insert(capability, name.clone());
        }

        info!("Registered plugin: {}", name);
        self.plugins.insert(name, plugin);
        Ok(())
    }

    /// Unregister a plugin by name
    ///
    /// Removes the plugin and clears its capability mappings.
    /// The plugin should be stopped before unregistering.
    pub fn unregister(&mut self, name: &str) -> Option<Box<dyn Plugin>> {
        // Remove capability mappings
        self.capability_map
            .retain(|_, plugin_name| plugin_name != name);

        // Remove plugin
        let plugin = self.plugins.remove(name);
        if plugin.is_some() {
            info!("Unregistered plugin: {}", name);
        }
        plugin
    }

    /// Get a reference to a plugin by name
    pub fn get(&self, name: &str) -> Option<&dyn Plugin> {
        self.plugins.get(name).map(|p| p.as_ref())
    }

    /// Get list of all registered plugin names
    pub fn list_plugins(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }

    /// Get all incoming capabilities from all plugins
    pub fn get_all_incoming_capabilities(&self) -> Vec<String> {
        self.capability_map.keys().cloned().collect()
    }

    /// Get all outgoing capabilities from all plugins
    pub fn get_all_outgoing_capabilities(&self) -> Vec<String> {
        let mut capabilities = Vec::new();
        for plugin in self.plugins.values() {
            capabilities.extend(plugin.outgoing_capabilities());
        }
        capabilities.sort();
        capabilities.dedup();
        capabilities
    }

    /// Initialize all plugins with device context
    ///
    /// Calls `init()` on each registered plugin in arbitrary order.
    ///
    /// # Errors
    ///
    /// Returns error if any plugin initialization fails. Already-initialized
    /// plugins are not rolled back.
    pub async fn init_all(&mut self, device: &Device) -> Result<()> {
        info!("Initializing {} plugins", self.plugins.len());
        for (name, plugin) in &mut self.plugins {
            debug!("Initializing plugin: {}", name);
            plugin.init(device).await?;
        }
        Ok(())
    }

    /// Start all plugins
    ///
    /// Calls `start()` on each registered plugin in arbitrary order.
    ///
    /// # Errors
    ///
    /// Returns error if any plugin fails to start. Already-started plugins
    /// are not stopped.
    pub async fn start_all(&mut self) -> Result<()> {
        info!("Starting {} plugins", self.plugins.len());
        for (name, plugin) in &mut self.plugins {
            debug!("Starting plugin: {}", name);
            plugin.start().await?;
        }
        Ok(())
    }

    /// Stop all plugins
    ///
    /// Calls `stop()` on each registered plugin in arbitrary order.
    /// Continues stopping remaining plugins even if some fail.
    ///
    /// # Errors
    ///
    /// Returns error if any plugin fails to stop, but all plugins are attempted.
    pub async fn stop_all(&mut self) -> Result<()> {
        info!("Stopping {} plugins", self.plugins.len());
        let mut errors = Vec::new();

        for (name, plugin) in &mut self.plugins {
            debug!("Stopping plugin: {}", name);
            if let Err(e) = plugin.stop().await {
                warn!("Failed to stop plugin {}: {}", name, e);
                errors.push((name.clone(), e));
            }
        }

        if !errors.is_empty() {
            return Err(ProtocolError::Plugin(format!(
                "Failed to stop {} plugins: {:?}",
                errors.len(),
                errors.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
            )));
        }

        Ok(())
    }

    /// Handle an incoming packet by routing to appropriate plugin
    ///
    /// Looks up the plugin that handles the packet's type and delegates
    /// packet processing to that plugin.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - No plugin handles the packet type
    /// - Plugin packet handling fails critically
    pub async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        let packet_type = &packet.packet_type;

        // Find plugin for this packet type
        let plugin_name = self.capability_map.get(packet_type).ok_or_else(|| {
            ProtocolError::Plugin(format!("No plugin handles packet type: {}", packet_type))
        })?;

        // Get plugin and handle packet
        let plugin = self
            .plugins
            .get_mut(plugin_name)
            .ok_or_else(|| ProtocolError::Plugin(format!("Plugin '{}' not found", plugin_name)))?;

        debug!("Routing packet {} to plugin {}", packet_type, plugin_name);
        plugin.handle_packet(packet, device).await
    }

    /// Check if a packet type is supported
    pub fn supports_packet_type(&self, packet_type: &str) -> bool {
        self.capability_map.contains_key(packet_type)
    }

    /// Get the plugin name that handles a packet type
    pub fn get_plugin_for_packet(&self, packet_type: &str) -> Option<&str> {
        self.capability_map.get(packet_type).map(|s| s.as_str())
    }

    /// Get number of registered plugins
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceInfo, DeviceType};
    use serde_json::json;

    // Mock plugin for testing
    struct MockPlugin {
        name: String,
        incoming: Vec<String>,
        outgoing: Vec<String>,
        initialized: bool,
        started: bool,
        packets_handled: usize,
    }

    impl MockPlugin {
        fn new(name: &str, incoming: Vec<&str>, outgoing: Vec<&str>) -> Self {
            Self {
                name: name.to_string(),
                incoming: incoming.iter().map(|s| s.to_string()).collect(),
                outgoing: outgoing.iter().map(|s| s.to_string()).collect(),
                initialized: false,
                started: false,
                packets_handled: 0,
            }
        }
    }

    #[async_trait]
    impl Plugin for MockPlugin {
        fn name(&self) -> &str {
            &self.name
        }

        fn incoming_capabilities(&self) -> Vec<String> {
            self.incoming.clone()
        }

        fn outgoing_capabilities(&self) -> Vec<String> {
            self.outgoing.clone()
        }

        async fn init(&mut self, _device: &Device) -> Result<()> {
            self.initialized = true;
            Ok(())
        }

        async fn start(&mut self) -> Result<()> {
            self.started = true;
            Ok(())
        }

        async fn stop(&mut self) -> Result<()> {
            self.started = false;
            Ok(())
        }

        async fn handle_packet(&mut self, _packet: &Packet, _device: &mut Device) -> Result<()> {
            self.packets_handled += 1;
            Ok(())
        }
    }

    fn create_test_device() -> Device {
        let info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1716);
        Device::from_discovery(info)
    }

    #[test]
    fn test_plugin_manager_creation() {
        let manager = PluginManager::new();
        assert_eq!(manager.plugin_count(), 0);
        assert!(manager.list_plugins().is_empty());
    }

    #[test]
    fn test_plugin_registration() {
        let mut manager = PluginManager::new();
        let plugin = Box::new(MockPlugin::new(
            "test",
            vec!["kdeconnect.test"],
            vec!["kdeconnect.test.response"],
        ));

        manager.register(plugin).unwrap();
        assert_eq!(manager.plugin_count(), 1);
        assert!(manager.list_plugins().contains(&"test".to_string()));
        assert!(manager.supports_packet_type("kdeconnect.test"));
    }

    #[test]
    fn test_duplicate_plugin_registration() {
        let mut manager = PluginManager::new();
        let plugin1 = Box::new(MockPlugin::new("test", vec!["kdeconnect.test"], vec![]));
        let plugin2 = Box::new(MockPlugin::new("test", vec!["kdeconnect.other"], vec![]));

        manager.register(plugin1).unwrap();
        assert!(manager.register(plugin2).is_err());
    }

    #[test]
    fn test_duplicate_capability_registration() {
        let mut manager = PluginManager::new();
        let plugin1 = Box::new(MockPlugin::new("test1", vec!["kdeconnect.test"], vec![]));
        let plugin2 = Box::new(MockPlugin::new("test2", vec!["kdeconnect.test"], vec![]));

        manager.register(plugin1).unwrap();
        assert!(manager.register(plugin2).is_err());
    }

    #[test]
    fn test_plugin_unregistration() {
        let mut manager = PluginManager::new();
        let plugin = Box::new(MockPlugin::new("test", vec!["kdeconnect.test"], vec![]));

        manager.register(plugin).unwrap();
        assert_eq!(manager.plugin_count(), 1);

        let removed = manager.unregister("test");
        assert!(removed.is_some());
        assert_eq!(manager.plugin_count(), 0);
        assert!(!manager.supports_packet_type("kdeconnect.test"));
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut manager = PluginManager::new();
        let plugin = Box::new(MockPlugin::new("test", vec!["kdeconnect.test"], vec![]));

        manager.register(plugin).unwrap();

        let device = create_test_device();
        manager.init_all(&device).await.unwrap();
        manager.start_all().await.unwrap();

        let plugin = manager.get("test").unwrap();
        assert!(plugin.is_ready());

        manager.stop_all().await.unwrap();
    }

    #[tokio::test]
    async fn test_packet_routing() {
        let mut manager = PluginManager::new();
        let plugin = Box::new(MockPlugin::new("test", vec!["kdeconnect.test"], vec![]));

        manager.register(plugin).unwrap();

        let device = create_test_device();
        manager.init_all(&device).await.unwrap();
        manager.start_all().await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new("kdeconnect.test", json!({}));
        manager.handle_packet(&packet, &mut device).await.unwrap();

        // Check packet was handled
        let _plugin = manager.get("test").unwrap();
        // Note: We can't directly access packets_handled in the trait object
        // This test verifies routing works without errors
    }

    #[test]
    fn test_capability_aggregation() {
        let mut manager = PluginManager::new();

        let plugin1 = Box::new(MockPlugin::new(
            "test1",
            vec!["kdeconnect.test1"],
            vec!["kdeconnect.test1.out"],
        ));
        let plugin2 = Box::new(MockPlugin::new(
            "test2",
            vec!["kdeconnect.test2"],
            vec!["kdeconnect.test2.out"],
        ));

        manager.register(plugin1).unwrap();
        manager.register(plugin2).unwrap();

        let incoming = manager.get_all_incoming_capabilities();
        assert_eq!(incoming.len(), 2);
        assert!(incoming.contains(&"kdeconnect.test1".to_string()));
        assert!(incoming.contains(&"kdeconnect.test2".to_string()));

        let outgoing = manager.get_all_outgoing_capabilities();
        assert_eq!(outgoing.len(), 2);
        assert!(outgoing.contains(&"kdeconnect.test1.out".to_string()));
        assert!(outgoing.contains(&"kdeconnect.test2.out".to_string()));
    }

    #[test]
    fn test_plugin_lookup() {
        let mut manager = PluginManager::new();
        let plugin = Box::new(MockPlugin::new("test", vec!["kdeconnect.test"], vec![]));

        manager.register(plugin).unwrap();

        assert_eq!(
            manager.get_plugin_for_packet("kdeconnect.test"),
            Some("test")
        );
        assert_eq!(manager.get_plugin_for_packet("kdeconnect.other"), None);
    }

    #[tokio::test]
    async fn test_unsupported_packet_type() {
        let mut manager = PluginManager::new();
        let device = create_test_device();
        manager.init_all(&device).await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new("kdeconnect.unsupported", json!({}));
        let result = manager.handle_packet(&packet, &mut device).await;

        assert!(result.is_err());
    }
}
