//! CConnect Plugin Architecture
//!
//! This module provides the plugin trait and architecture for extending CConnect
//! functionality. Plugins handle specific packet types and provide features like battery
//! monitoring, notifications, media control, etc.
//!
//! ## Plugin Architecture
//!
//! CConnect uses a **capability-based plugin system** where devices advertise their
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
//! Plugin packet types follow the pattern `cconnect.<plugin>[.<action>]`:
//! - `cconnect.battery` - Battery status broadcast
//! - `cconnect.battery.request` - Request battery status
//! - `cconnect.mpris` - Media player state
//! - `cconnect.mpris.request` - Media player commands
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
//! use cosmic_connect_core::plugins::*;
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
//!         vec!["cconnect.ping".to_string()]
//!     }
//!
//!     fn outgoing_capabilities(&self) -> Vec<String> {
//!         vec!["cconnect.ping".to_string()]
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
//!         if packet.is_type("cconnect.ping") {
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
//! - [CConnect Community Wiki](https://community.kde.org/KDEConnect)
//! - [CConnect GitHub](https://github.com/KDE/cconnect-kde)

pub mod audio_backend;
pub mod audiostream;
pub mod battery;
pub mod camera;
pub mod chat;
pub mod chat_storage;
pub mod clipboard;
pub mod clipboard_backend;
pub mod clipboard_storage;
pub mod clipboardhistory;
pub mod connectivity_report;
pub mod contacts;
pub mod filesync;
pub mod findmyphone;
pub mod lock;
pub mod logind_backend;
pub mod r#macro;
pub mod mkshare;
pub mod mousekeyboardshare;
pub mod mpris;
pub mod mpris_backend;
pub mod networkshare;
pub mod notification;
pub mod phoneauth;
pub mod ping;
pub mod power;
pub mod presenter;
pub mod remotedesktop;
pub mod remoteinput;
pub mod runcommand;
pub mod screenshare;
pub mod screenshot;
pub mod share;
pub mod systemd_inhibitor;
pub mod systemmonitor;
pub mod systemvolume;
pub mod telephony;
pub mod upower_backend;
pub mod wol;

#[cfg(feature = "extendeddisplay")]
pub mod extendeddisplay;

use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tracing::{debug, error, info, warn};

/// Factory trait for creating plugin instances
///
/// Plugins must implement this trait to support per-device instances.
/// The factory creates new plugin instances for each device connection.
///
/// ## Example
///
/// ```rust,ignore
/// struct PingPluginFactory;
///
/// impl PluginFactory for PingPluginFactory {
///     fn name(&self) -> &str {
///         "ping"
///     }
///
///     fn create(&self) -> Box<dyn Plugin> {
///         Box::new(PingPlugin::new())
///     }
/// }
/// ```
pub trait PluginFactory: Send + Sync {
    /// Get the plugin name this factory creates
    fn name(&self) -> &str;

    /// Get incoming capabilities for this plugin type
    fn incoming_capabilities(&self) -> Vec<String>;

    /// Get outgoing capabilities for this plugin type
    fn outgoing_capabilities(&self) -> Vec<String>;

    /// Create a new plugin instance
    fn create(&self) -> Box<dyn Plugin>;
}

/// Plugin trait for extending CConnect functionality
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
pub trait Plugin: Send + Sync + Any {
    /// Get the plugin name
    ///
    /// This should be a short, lowercase identifier like "ping", "battery", "mpris".
    fn name(&self) -> &str;

    /// Downcast to Any for type-specific queries
    ///
    /// This allows querying plugin-specific state through the trait object.
    fn as_any(&self) -> &dyn Any;

    /// Downcast to mutable Any for type-specific state modification
    ///
    /// This allows modifying plugin-specific state through the trait object.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Get list of incoming packet types this plugin can handle
    ///
    /// These are packet types the plugin can **receive** and process.
    /// Format: `cconnect.<plugin>[.<action>]`
    ///
    /// Example: `["cconnect.ping", "cconnect.battery"]`
    fn incoming_capabilities(&self) -> Vec<String>;

    /// Get list of outgoing packet types this plugin can send
    ///
    /// These are packet types the plugin can **send** to other devices.
    /// Format: `cconnect.<plugin>[.<action>]`
    ///
    /// Example: `["cconnect.ping", "cconnect.battery.request"]`
    fn outgoing_capabilities(&self) -> Vec<String>;

    /// Initialize the plugin with device context and packet sender
    ///
    /// Called once after plugin creation to provide device-specific configuration
    /// and a channel for sending proactive packets.
    ///
    /// # Errors
    ///
    /// Returns error if plugin initialization fails (e.g., invalid device state,
    /// missing required capabilities).
    async fn init(
        &mut self,
        device: &Device,
        packet_sender: Sender<(String, Packet)>,
    ) -> Result<()>;

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
/// Manages plugin factories and per-device plugin instances. Routes incoming packets
/// to the appropriate plugin based on packet type and device.
///
/// ## Per-Device Architecture
///
/// Each device gets its own set of plugin instances, allowing plugins to maintain
/// independent state per device. Plugin factories are registered once, and instances
/// are created on-demand when devices connect.
///
/// ## Example
///
/// ```rust,ignore
/// use cosmic_connect_core::plugins::*;
///
/// # async fn example() -> Result<()> {
/// let mut manager = PluginManager::new();
///
/// // Register plugin factories
/// manager.register_factory(Arc::new(PingPluginFactory))?;
/// manager.register_factory(Arc::new(BatteryPluginFactory))?;
///
/// // Create and initialize plugins for a specific device
/// manager.init_device_plugins(&device_id, &device).await?;
///
/// // Route packet to appropriate plugin for this device
/// if let Some(packet) = receive_packet().await {
///     manager.handle_packet(&device_id, &packet, &mut device).await?;
/// }
///
/// // Cleanup when device disconnects
/// manager.cleanup_device_plugins(&device_id).await?;
/// # Ok(())
/// # }
/// ```
pub struct PluginManager {
    /// Registered plugin factories by name
    factories: HashMap<String, Arc<dyn PluginFactory>>,

    /// Per-device plugin instances
    /// Outer key: device_id, Inner key: plugin_name
    device_plugins: HashMap<String, HashMap<String, Box<dyn Plugin>>>,

    /// Mapping from incoming capability to plugin name
    capability_map: HashMap<String, String>,
}

impl PluginManager {
    /// Create a new empty plugin manager
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
            device_plugins: HashMap::new(),
            capability_map: HashMap::new(),
        }
    }

    /// Register a plugin factory
    ///
    /// Adds the plugin factory to the registry and builds capability mappings.
    /// The factory will be used to create plugin instances for each device.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - A plugin factory with the same name is already registered
    /// - A capability is already handled by another plugin
    pub fn register_factory(&mut self, factory: Arc<dyn PluginFactory>) -> Result<()> {
        let name = factory.name().to_string();

        // Check for duplicate plugin name
        if self.factories.contains_key(&name) {
            return Err(ProtocolError::Plugin(format!(
                "Plugin factory '{}' is already registered",
                name
            )));
        }

        // Build capability mappings
        for capability in factory.incoming_capabilities() {
            if let Some(existing) = self.capability_map.get(&capability) {
                return Err(ProtocolError::Plugin(format!(
                    "Capability '{}' already handled by plugin '{}'",
                    capability, existing
                )));
            }
            self.capability_map.insert(capability, name.clone());
        }

        info!("Registered plugin factory: {}", name);
        self.factories.insert(name, factory);
        Ok(())
    }

    /// Register a new plugin (legacy API for backward compatibility)
    ///
    /// This method exists for backward compatibility but is deprecated.
    /// Use `register_factory` instead for per-device plugin instances.
    ///
    /// # Errors
    ///
    /// Returns error indicating this API is deprecated
    #[deprecated(note = "Use register_factory instead for per-device plugin support")]
    pub fn register(&mut self, _plugin: Box<dyn Plugin>) -> Result<()> {
        Err(ProtocolError::Plugin(
            "register() is deprecated - use register_factory() instead".to_string(),
        ))
    }

    /// Initialize plugins for a specific device
    ///
    /// Creates plugin instances from registered factories and initializes them
    /// for the given device. Each device gets its own set of plugin instances.
    ///
    /// # Errors
    ///
    /// Returns error if plugin creation or initialization fails
    pub async fn init_device_plugins(
        &mut self,
        device_id: &str,
        device: &Device,
        packet_sender: Sender<(String, Packet)>,
    ) -> Result<()> {
        info!(
            "Initializing {} plugins for device {}",
            self.factories.len(),
            device_id
        );

        let mut device_plugins = HashMap::new();

        for (name, factory) in &self.factories {
            debug!("Creating plugin {} for device {}", name, device_id);

            // Create plugin instance
            let mut plugin = factory.create();

            // Initialize plugin
            if let Err(e) = plugin.init(device, packet_sender.clone()).await {
                error!(
                    "Failed to initialize plugin {} for device {}: {}",
                    name, device_id, e
                );
                // Continue with other plugins rather than failing completely
                continue;
            }

            // Start plugin
            if let Err(e) = plugin.start().await {
                error!(
                    "Failed to start plugin {} for device {}: {}",
                    name, device_id, e
                );
                // Continue with other plugins
                continue;
            }

            device_plugins.insert(name.clone(), plugin);
        }

        info!(
            "Initialized {} plugins for device {}",
            device_plugins.len(),
            device_id
        );

        self.device_plugins
            .insert(device_id.to_string(), device_plugins);

        Ok(())
    }

    /// Cleanup plugins for a specific device
    ///
    /// Stops and removes all plugin instances for the given device.
    /// Called when a device disconnects.
    ///
    /// # Errors
    ///
    /// Returns error if plugin cleanup fails, but attempts to cleanup all plugins
    pub async fn cleanup_device_plugins(&mut self, device_id: &str) -> Result<()> {
        if let Some(mut plugins) = self.device_plugins.remove(device_id) {
            info!(
                "Cleaning up {} plugins for device {}",
                plugins.len(),
                device_id
            );

            let mut errors = Vec::new();

            for (name, mut plugin) in plugins.drain() {
                debug!("Stopping plugin {} for device {}", name, device_id);
                if let Err(e) = plugin.stop().await {
                    warn!(
                        "Failed to stop plugin {} for device {}: {}",
                        name, device_id, e
                    );
                    errors.push((name, e));
                }
            }

            if !errors.is_empty() {
                return Err(ProtocolError::Plugin(format!(
                    "Failed to stop {} plugins for device {}: {:?}",
                    errors.len(),
                    device_id,
                    errors.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
                )));
            }
        }

        Ok(())
    }

    /// Get reference to a plugin for a specific device
    pub fn get_device_plugin(&self, device_id: &str, plugin_name: &str) -> Option<&dyn Plugin> {
        self.device_plugins
            .get(device_id)
            .and_then(|plugins| plugins.get(plugin_name))
            .map(|p| p.as_ref())
    }

    /// Get mutable reference to a plugin for a specific device
    pub fn get_device_plugin_mut(
        &mut self,
        device_id: &str,
        plugin_name: &str,
    ) -> Option<&mut dyn Plugin> {
        self.device_plugins
            .get_mut(device_id)
            .and_then(|plugins| plugins.get_mut(plugin_name))
            .map(|p| p.as_mut())
    }

    /// Unregister a plugin factory by name
    ///
    /// Removes the plugin factory and clears its capability mappings.
    /// Device plugin instances should be cleaned up before unregistering.
    pub fn unregister_factory(&mut self, name: &str) -> Option<Arc<dyn PluginFactory>> {
        // Remove capability mappings
        self.capability_map
            .retain(|_, plugin_name| plugin_name != name);

        // Remove factory
        let factory = self.factories.remove(name);
        if factory.is_some() {
            info!("Unregistered plugin factory: {}", name);
        }
        factory
    }

    /// Get a reference to a plugin by name (deprecated)
    ///
    /// Use `get_device_plugin(device_id, plugin_name)` instead for per-device instances.
    #[deprecated(note = "Use get_device_plugin instead for per-device plugin support")]
    pub fn get(&self, _name: &str) -> Option<&dyn Plugin> {
        None
    }

    /// Get list of all registered plugin factory names
    pub fn list_plugins(&self) -> Vec<String> {
        self.factories.keys().cloned().collect()
    }

    /// Get all incoming capabilities from registered factories
    pub fn get_all_incoming_capabilities(&self) -> Vec<String> {
        self.capability_map.keys().cloned().collect()
    }

    /// Get all outgoing capabilities from registered factories
    pub fn get_all_outgoing_capabilities(&self) -> Vec<String> {
        let mut capabilities: Vec<String> = self
            .factories
            .values()
            .flat_map(|f| f.outgoing_capabilities())
            .collect();
        capabilities.sort();
        capabilities.dedup();
        capabilities
    }

    /// Initialize all plugins with device context (deprecated)
    ///
    /// Use `init_device_plugins(device_id, device)` instead for per-device plugin instances.
    #[deprecated(note = "Use init_device_plugins instead for per-device plugin support")]
    pub async fn init_all(&mut self, _device: &Device) -> Result<()> {
        Err(ProtocolError::Plugin(
            "init_all() is deprecated - use init_device_plugins() instead".to_string(),
        ))
    }

    /// Start all plugins (deprecated)
    ///
    /// Use `init_device_plugins(device_id, device)` instead, which both initializes and starts plugins.
    #[deprecated(note = "Use init_device_plugins instead for per-device plugin support")]
    pub async fn start_all(&mut self) -> Result<()> {
        Err(ProtocolError::Plugin(
            "start_all() is deprecated - use init_device_plugins() instead".to_string(),
        ))
    }

    /// Stop all plugins (deprecated)
    ///
    /// Use `cleanup_device_plugins(device_id)` instead for per-device plugin instances.
    #[deprecated(note = "Use cleanup_device_plugins instead for per-device plugin support")]
    pub async fn stop_all(&mut self) -> Result<()> {
        Err(ProtocolError::Plugin(
            "stop_all() is deprecated - use cleanup_device_plugins() instead".to_string(),
        ))
    }

    /// Stop all device plugins (for daemon shutdown)
    ///
    /// Cleans up all plugin instances for all devices. Used during daemon shutdown.
    pub async fn shutdown_all(&mut self) -> Result<()> {
        info!("Shutting down all device plugins");
        let device_ids: Vec<String> = self.device_plugins.keys().cloned().collect();

        let mut errors = Vec::new();
        for device_id in device_ids {
            if let Err(e) = self.cleanup_device_plugins(&device_id).await {
                warn!("Failed to cleanup plugins for device {}: {}", device_id, e);
                errors.push((device_id, e));
            }
        }

        if !errors.is_empty() {
            return Err(ProtocolError::Plugin(format!(
                "Failed to shutdown plugins for {} devices",
                errors.len()
            )));
        }

        Ok(())
    }

    /// Handle an incoming packet by routing to appropriate device-specific plugin
    ///
    /// Looks up the plugin that handles the packet's type for the given device
    /// and delegates packet processing to that plugin instance.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - No plugin handles the packet type
    /// - Device has no initialized plugins
    /// - Plugin packet handling fails critically
    pub async fn handle_packet(
        &mut self,
        device_id: &str,
        packet: &Packet,
        device: &mut Device,
    ) -> Result<()> {
        let mut packet_type = packet.packet_type.clone();

        // Find plugin name for this packet type
        let plugin_name = if let Some(name) = self.capability_map.get(&packet_type) {
            name.clone()
        } else if packet_type.starts_with("kdeconnect.") {
            // Auto-alias kdeconnect.* to cconnect.* if not handled
            let aliased = packet_type.replace("kdeconnect.", "cconnect.");
            if let Some(name) = self.capability_map.get(&aliased) {
                debug!(
                    "Aliasing packet type {} to {} for routing",
                    packet_type, aliased
                );
                packet_type = aliased;
                name.clone()
            } else {
                return Err(ProtocolError::Plugin(format!(
                    "No plugin handles packet type: {} (nor aliased {})",
                    packet.packet_type, aliased
                )));
            }
        } else {
            return Err(ProtocolError::Plugin(format!(
                "No plugin handles packet type: {}",
                packet_type
            )));
        };

        // Get device plugins
        let device_plugins = self.device_plugins.get_mut(device_id).ok_or_else(|| {
            ProtocolError::Plugin(format!("No plugins initialized for device {}", device_id))
        })?;

        // Get plugin instance for this device
        let plugin = device_plugins.get_mut(&plugin_name).ok_or_else(|| {
            ProtocolError::Plugin(format!(
                "Plugin '{}' not found for device {}",
                plugin_name, device_id
            ))
        })?;

        debug!(
            "Routing packet {} (effective: {}) to plugin {} for device {}",
            packet.packet_type, packet_type, plugin_name, device_id
        );

        // Handle packet with error isolation
        match plugin.handle_packet(packet, device).await {
            Ok(()) => Ok(()),
            Err(e) => {
                // Check if error is recoverable
                if e.is_recoverable() {
                    // Log recoverable errors but don't propagate
                    warn!(
                        "Plugin {} encountered recoverable error handling packet {} for device {}: {}",
                        plugin_name, packet_type, device_id, e
                    );
                    // Plugin continues to function
                    Ok(())
                } else if e.requires_user_action() {
                    // User action required - log with appropriate level
                    warn!(
                        "Plugin {} requires user action for packet {} on device {}: {}",
                        plugin_name,
                        packet_type,
                        device_id,
                        e.user_message()
                    );
                    // Don't crash the plugin, but return error for UI notification
                    Err(e)
                } else {
                    // Critical error - plugin may be in bad state
                    error!(
                        "Plugin {} critically failed handling packet {} for device {}: {}",
                        plugin_name, packet_type, device_id, e
                    );
                    // Propagate critical errors
                    Err(e)
                }
            }
        }
    }

    /// Check if a packet type is supported
    pub fn supports_packet_type(&self, packet_type: &str) -> bool {
        self.capability_map.contains_key(packet_type)
    }

    /// Get the plugin name that handles a packet type
    pub fn get_plugin_for_packet(&self, packet_type: &str) -> Option<&str> {
        self.capability_map.get(packet_type).map(|s| s.as_str())
    }

    /// Get number of registered plugin factories
    pub fn factory_count(&self) -> usize {
        self.factories.len()
    }

    /// Get number of devices with initialized plugins
    pub fn device_count(&self) -> usize {
        self.device_plugins.len()
    }

    /// Get number of plugins initialized for a specific device
    pub fn device_plugin_count(&self, device_id: &str) -> usize {
        self.device_plugins
            .get(device_id)
            .map(|plugins| plugins.len())
            .unwrap_or(0)
    }

    /// Get battery status for a specific device
    ///
    /// Queries the battery plugin for the device and returns the latest battery status.
    ///
    /// # Parameters
    ///
    /// - `device_id`: The device ID to query
    ///
    /// # Returns
    ///
    /// `Some(BatteryStatus)` if the device has a battery plugin with status data,
    /// `None` if the device is not found, has no battery plugin, or no status has been received.
    pub fn get_device_battery_status(&self, device_id: &str) -> Option<battery::BatteryStatus> {
        // Get the device's plugins
        let device_plugins = self.device_plugins.get(device_id)?;

        // Get the battery plugin
        let battery_plugin = device_plugins.get("battery")?;

        // Downcast to BatteryPlugin
        let battery_plugin = battery_plugin
            .as_any()
            .downcast_ref::<battery::BatteryPlugin>()?;

        // Get the battery status
        battery_plugin.get_battery_status()
    }

    /// Get screen share statistics for a device
    ///
    /// Returns viewer count and other sharing metrics when the device is sharing its screen
    pub fn get_device_screen_share_stats(
        &self,
        device_id: &str,
    ) -> Option<screenshare::ShareStats> {
        // Get the device's plugins
        let device_plugins = self.device_plugins.get(device_id)?;

        // Get the screenshare plugin
        let screenshare_plugin = device_plugins.get("screenshare")?;

        // Downcast to ScreenSharePlugin
        let screenshare_plugin = screenshare_plugin
            .as_any()
            .downcast_ref::<screenshare::ScreenSharePlugin>()?;

        // Get the share stats
        screenshare_plugin.get_stats()
    }

    /// Get number of registered plugins (deprecated)
    ///
    /// Use `factory_count()` to get number of registered factories.
    #[deprecated(note = "Use factory_count() instead")]
    pub fn plugin_count(&self) -> usize {
        self.factories.len()
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

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }

        fn incoming_capabilities(&self) -> Vec<String> {
            self.incoming.clone()
        }

        fn outgoing_capabilities(&self) -> Vec<String> {
            self.outgoing.clone()
        }

        async fn init(
            &mut self,
            _device: &Device,
            _packet_sender: Sender<(String, Packet)>,
        ) -> Result<()> {
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
        assert_eq!(manager.factory_count(), 0);
        assert!(manager.list_plugins().is_empty());
    }

    // MockPluginFactory for testing
    #[derive(Debug, Clone)]
    struct MockPluginFactory {
        name: String,
        incoming: Vec<String>,
        outgoing: Vec<String>,
    }

    impl MockPluginFactory {
        fn new(name: &str, incoming: Vec<&str>, outgoing: Vec<&str>) -> Self {
            Self {
                name: name.to_string(),
                incoming: incoming.iter().map(|s| s.to_string()).collect(),
                outgoing: outgoing.iter().map(|s| s.to_string()).collect(),
            }
        }
    }

    impl PluginFactory for MockPluginFactory {
        fn name(&self) -> &str {
            &self.name
        }

        fn incoming_capabilities(&self) -> Vec<String> {
            self.incoming.clone()
        }

        fn outgoing_capabilities(&self) -> Vec<String> {
            self.outgoing.clone()
        }

        fn create(&self) -> Box<dyn Plugin> {
            let incoming: Vec<&str> = self.incoming.iter().map(|s| s.as_str()).collect();
            let outgoing: Vec<&str> = self.outgoing.iter().map(|s| s.as_str()).collect();
            Box::new(MockPlugin::new(&self.name, incoming, outgoing))
        }
    }

    #[test]
    fn test_factory_registration() {
        let mut manager = PluginManager::new();
        let factory = Arc::new(MockPluginFactory::new(
            "test_plugin",
            vec!["cconnect.test"],
            vec!["cconnect.test.response"],
        ));

        assert!(manager.register_factory(factory).is_ok());
        assert_eq!(manager.factory_count(), 1);
    }

    #[test]
    fn test_duplicate_factory_registration() {
        let mut manager = PluginManager::new();
        let factory1 = Arc::new(MockPluginFactory::new(
            "test_plugin",
            vec!["cconnect.test"],
            vec![],
        ));
        let factory2 = Arc::new(MockPluginFactory::new(
            "test_plugin",
            vec!["cconnect.test2"],
            vec![],
        ));

        assert!(manager.register_factory(factory1).is_ok());
        let result = manager.register_factory(factory2);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("already registered"));
    }

    #[test]
    fn test_duplicate_capability_registration() {
        let mut manager = PluginManager::new();
        let factory1 = Arc::new(MockPluginFactory::new(
            "plugin1",
            vec!["cconnect.test"],
            vec![],
        ));
        let factory2 = Arc::new(MockPluginFactory::new(
            "plugin2",
            vec!["cconnect.test"],
            vec![],
        ));

        assert!(manager.register_factory(factory1).is_ok());
        let result = manager.register_factory(factory2);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already handled"));
    }

    #[tokio::test]
    async fn test_per_device_plugin_initialization() {
        let mut manager = PluginManager::new();
        let factory = Arc::new(MockPluginFactory::new(
            "test_plugin",
            vec!["cconnect.test"],
            vec![],
        ));

        manager.register_factory(factory).unwrap();

        let device = create_test_device();
        let device_id = device.id();

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        assert!(manager
            .init_device_plugins(device_id, &device, tx)
            .await
            .is_ok());
        assert_eq!(manager.device_plugin_count(device_id), 1);
    }

    #[tokio::test]
    async fn test_per_device_plugin_cleanup() {
        let mut manager = PluginManager::new();
        let factory = Arc::new(MockPluginFactory::new(
            "test_plugin",
            vec!["cconnect.test"],
            vec![],
        ));

        manager.register_factory(factory).unwrap();

        let device = create_test_device();
        let device_id = device.id();

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        manager
            .init_device_plugins(device_id, &device, tx)
            .await
            .unwrap();
        assert_eq!(manager.device_plugin_count(device_id), 1);

        assert!(manager.cleanup_device_plugins(device_id).await.is_ok());
        assert_eq!(manager.device_plugin_count(device_id), 0);
    }

    #[tokio::test]
    async fn test_per_device_packet_routing() {
        let mut manager = PluginManager::new();
        let factory = Arc::new(MockPluginFactory::new(
            "test_plugin",
            vec!["cconnect.test"],
            vec![],
        ));

        manager.register_factory(factory).unwrap();

        let mut device = create_test_device();
        let device_id = device.id().to_string();

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        manager
            .init_device_plugins(&device_id, &device, tx)
            .await
            .unwrap();

        let packet = Packet::new("cconnect.test", serde_json::json!({}));
        assert!(manager
            .handle_packet(&device_id, &packet, &mut device)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_multiple_devices_independent_state() {
        let mut manager = PluginManager::new();
        let factory = Arc::new(MockPluginFactory::new(
            "test_plugin",
            vec!["cconnect.test"],
            vec![],
        ));

        manager.register_factory(factory).unwrap();

        let device1 = create_test_device();
        let device1_id = device1.id();
        let device2 = create_test_device();
        let device2_id = device2.id();

        let (tx1, _rx1) = tokio::sync::mpsc::channel(100);
        manager
            .init_device_plugins(device1_id, &device1, tx1)
            .await
            .unwrap();
        let (tx2, _rx2) = tokio::sync::mpsc::channel(100);
        manager
            .init_device_plugins(device2_id, &device2, tx2)
            .await
            .unwrap();

        assert_eq!(manager.device_plugin_count(device1_id), 1);
        assert_eq!(manager.device_plugin_count(device2_id), 1);

        manager.cleanup_device_plugins(device1_id).await.unwrap();
        assert_eq!(manager.device_plugin_count(device1_id), 0);
        assert_eq!(manager.device_plugin_count(device2_id), 1);
    }

    #[test]
    fn test_capability_lookup() {
        let mut manager = PluginManager::new();
        let factory = Arc::new(MockPluginFactory::new(
            "test_plugin",
            vec!["cconnect.test", "cconnect.test2"],
            vec![],
        ));

        manager.register_factory(factory).unwrap();
        let capabilities = manager.get_all_incoming_capabilities();
        assert_eq!(capabilities.len(), 2);
        assert!(capabilities.contains(&"cconnect.test".to_string()));
        assert!(capabilities.contains(&"cconnect.test2".to_string()));
    }

    #[tokio::test]
    async fn test_unsupported_packet_type() {
        let mut manager = PluginManager::new();
        let factory = Arc::new(MockPluginFactory::new(
            "test_plugin",
            vec!["cconnect.test"],
            vec![],
        ));

        manager.register_factory(factory).unwrap();

        let mut device = create_test_device();
        let device_id = device.id().to_string();

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        manager
            .init_device_plugins(&device_id, &device, tx)
            .await
            .unwrap();

        let packet = Packet::new("cconnect.unsupported", serde_json::json!({}));
        let result = manager
            .handle_packet(&device_id, &packet, &mut device)
            .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No plugin handles"));
    }
}
