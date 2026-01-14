//! DBus Interface for KDE Connect Daemon
//!
//! Provides IPC between the background daemon and COSMIC panel applet.
//! Exposes device management, pairing, and plugin actions via DBus.

use anyhow::{Context, Result};
use kdeconnect_protocol::{ConnectionManager, Device, DeviceManager, PluginManager};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use zbus::object_server::SignalContext;
use zbus::{connection, interface, Connection};

/// DBus service name
pub const SERVICE_NAME: &str = "com.system76.CosmicKdeConnect";

/// DBus object path
pub const OBJECT_PATH: &str = "/com/system76/CosmicKdeConnect";

/// DBus interface name
pub const INTERFACE_NAME: &str = "com.system76.CosmicKdeConnect";

/// Device state for DBus serialization
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, zbus::zvariant::Type)]
pub struct DeviceInfo {
    /// Device ID
    pub id: String,
    /// Device name
    pub name: String,
    /// Device type
    pub device_type: String,
    /// Is device paired
    pub is_paired: bool,
    /// Is device reachable
    pub is_reachable: bool,
    /// Is device connected (TLS)
    pub is_connected: bool,
    /// Last seen timestamp (UNIX timestamp)
    pub last_seen: i64,
    /// Supported incoming plugin capabilities
    pub incoming_capabilities: Vec<String>,
    /// Supported outgoing plugin capabilities
    pub outgoing_capabilities: Vec<String>,
}

impl From<&Device> for DeviceInfo {
    fn from(device: &Device) -> Self {
        Self {
            id: device.id().to_string(),
            name: device.name().to_string(),
            device_type: device.info.device_type.as_str().to_string(),
            is_paired: device.is_paired(),
            is_reachable: device.is_reachable(),
            is_connected: device.is_connected(),
            last_seen: device.last_seen as i64,
            incoming_capabilities: device.info.incoming_capabilities.clone(),
            outgoing_capabilities: device.info.outgoing_capabilities.clone(),
        }
    }
}

/// Battery status for DBus serialization
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, zbus::zvariant::Type)]
pub struct BatteryStatus {
    /// Battery level percentage (0-100)
    pub level: i32,
    /// Is device charging
    pub is_charging: bool,
}

/// DBus interface for KDE Connect daemon
pub struct KdeConnectInterface {
    /// Device manager
    device_manager: Arc<RwLock<DeviceManager>>,
    /// Plugin manager
    plugin_manager: Arc<RwLock<PluginManager>>,
    /// Connection manager
    connection_manager: Arc<RwLock<ConnectionManager>>,
    /// Device configuration registry
    device_config_registry: Arc<RwLock<crate::device_config::DeviceConfigRegistry>>,
    /// Pairing service (optional - may not be started yet)
    pairing_service: Option<Arc<RwLock<kdeconnect_protocol::pairing::PairingService>>>,
    /// MPRIS manager for local media player control (optional)
    mpris_manager: Option<Arc<crate::mpris_manager::MprisManager>>,
}

impl KdeConnectInterface {
    /// Create a new DBus interface
    pub fn new(
        device_manager: Arc<RwLock<DeviceManager>>,
        plugin_manager: Arc<RwLock<PluginManager>>,
        connection_manager: Arc<RwLock<ConnectionManager>>,
        device_config_registry: Arc<RwLock<crate::device_config::DeviceConfigRegistry>>,
        pairing_service: Option<Arc<RwLock<kdeconnect_protocol::pairing::PairingService>>>,
        mpris_manager: Option<Arc<crate::mpris_manager::MprisManager>>,
    ) -> Self {
        Self {
            device_manager,
            plugin_manager,
            connection_manager,
            device_config_registry,
            pairing_service,
            mpris_manager,
        }
    }
}

#[interface(name = "com.system76.CosmicKdeConnect")]
impl KdeConnectInterface {
    /// List all known devices
    ///
    /// Returns a map of device ID to device information for all devices
    /// (paired and unpaired, reachable and unreachable).
    async fn list_devices(&self) -> HashMap<String, DeviceInfo> {
        debug!("DBus: ListDevices called");

        let device_manager = self.device_manager.read().await;
        let devices = device_manager.devices();

        let mut result = HashMap::new();
        for device in devices {
            result.insert(device.id().to_string(), DeviceInfo::from(device));
        }

        info!("DBus: Returning {} devices", result.len());
        result
    }

    /// Get information about a specific device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to query
    ///
    /// # Returns
    /// Device information, or error if device not found
    async fn get_device(&self, device_id: String) -> Result<DeviceInfo, zbus::fdo::Error> {
        debug!("DBus: GetDevice called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        Ok(DeviceInfo::from(device))
    }

    /// Request pairing with a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to pair with
    ///
    /// # Returns
    /// Success or error message
    async fn pair_device(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: PairDevice called for {}", device_id);

        // Check if pairing service is available
        let pairing_service = self.pairing_service.as_ref().ok_or_else(|| {
            zbus::fdo::Error::Failed("Pairing service not initialized".to_string())
        })?;

        // Get device info from device manager
        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        // Check if already paired
        if device.is_paired() {
            return Err(zbus::fdo::Error::Failed(format!(
                "Device {} is already paired",
                device_id
            )));
        }

        let device_info = device.info.clone();
        let remote_addr = format!("{}:{}", device.host.as_deref().unwrap_or("0.0.0.0"), device.port.unwrap_or(1716))
            .parse()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Invalid remote address: {}", e)))?;

        drop(device_manager);

        // Request pairing
        let pairing_service = pairing_service.read().await;
        pairing_service
            .request_pairing(device_info, remote_addr)
            .await
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Failed to request pairing: {}", e))
            })?;

        info!("Pairing request sent to device {}", device_id);
        Ok(())
    }

    /// Unpair a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to unpair
    ///
    /// # Returns
    /// Success or error message
    async fn unpair_device(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: UnpairDevice called for {}", device_id);

        // Check if pairing service is available
        let pairing_service = self.pairing_service.as_ref().ok_or_else(|| {
            zbus::fdo::Error::Failed("Pairing service not initialized".to_string())
        })?;

        // Check if device exists
        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        // Check if device is paired
        if !device.is_paired() {
            return Err(zbus::fdo::Error::Failed(format!(
                "Device {} is not paired",
                device_id
            )));
        }

        drop(device_manager);

        // Unpair the device
        let pairing_service = pairing_service.read().await;
        pairing_service.unpair(&device_id).await.map_err(|e| {
            zbus::fdo::Error::Failed(format!("Failed to unpair device: {}", e))
        })?;

        info!("Device {} unpaired successfully", device_id);
        Ok(())
    }

    /// Trigger device discovery
    ///
    /// Broadcasts UDP discovery packet to find new devices on the network.
    async fn refresh_discovery(&self) -> Result<(), zbus::fdo::Error> {
        info!("DBus: RefreshDiscovery called");

        // Discovery is continuous in the daemon, so this is a no-op
        // In a real implementation, you might trigger a broadcast here
        Ok(())
    }

    /// Get device connection state
    ///
    /// # Arguments
    /// * `device_id` - The device ID to query
    ///
    /// # Returns
    /// Connection state: "connected", "paired", "reachable", or "unknown"
    async fn get_device_state(&self, device_id: String) -> Result<String, zbus::fdo::Error> {
        debug!("DBus: GetDeviceState called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        let state = if device.is_connected() {
            "connected"
        } else if device.is_paired() {
            "paired"
        } else if device.is_reachable() {
            "reachable"
        } else {
            "unknown"
        };

        Ok(state.to_string())
    }

    /// Send a ping to a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to ping
    /// * `message` - Optional message to include in the ping
    async fn send_ping(&self, device_id: String, message: String) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: SendPing called for {} with message '{}'",
            device_id, message
        );

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create ping packet
        use kdeconnect_protocol::Packet;
        use serde_json::json;

        let body = if !message.is_empty() {
            json!({ "message": message })
        } else {
            json!({})
        };

        let packet = Packet::new("kdeconnect.ping", body);

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send ping: {}", e)))?;

        info!("DBus: Ping sent successfully to {}", device_id);
        Ok(())
    }

    /// Trigger find phone on a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to trigger find phone on
    async fn find_phone(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: FindPhone called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create findmyphone packet
        use kdeconnect_protocol::Packet;
        use serde_json::json;

        let packet = Packet::new("kdeconnect.findmyphone.request", json!({}));

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send find phone request: {}", e)))?;

        info!("DBus: Find phone request sent successfully to {}", device_id);
        Ok(())
    }

    /// Share a file with a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to share with
    /// * `path` - Absolute path to the file to share
    async fn share_file(&self, device_id: String, path: String) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: ShareFile called for {} with path '{}'",
            device_id, path
        );

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Extract file metadata
        use kdeconnect_protocol::{FileTransferInfo, PayloadServer};
        let file_info = FileTransferInfo::from_path(&path)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to read file metadata: {}", e)))?;

        info!(
            "DBus: Sharing file '{}' ({} bytes) to {}",
            file_info.filename, file_info.size, device_id
        );

        // Create payload server on available port
        let server = PayloadServer::new()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to create payload server: {}", e)))?;
        let port = server.port();

        info!("DBus: Payload server listening on port {}", port);

        // Create share packet with file info and payload transfer port
        use kdeconnect_protocol::plugins::share::{FileShareInfo, SharePlugin};
        let share_info: FileShareInfo = file_info.clone().into();
        let plugin = SharePlugin::new();
        let packet = plugin.create_file_packet(share_info, port);

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send share packet: {}", e)))?;

        info!("DBus: Share packet sent to {}, waiting for connection", device_id);

        // Spawn background task to handle file transfer
        let file_path = path.clone();
        let device_id_clone = device_id.clone();
        tokio::spawn(async move {
            match server.send_file(&file_path).await {
                Ok(()) => {
                    info!("File transfer completed successfully for device {}", device_id_clone);
                }
                Err(e) => {
                    warn!("File transfer failed for device {}: {}", device_id_clone, e);
                }
            }
        });

        info!("DBus: File sharing initiated for {}", device_id);
        Ok(())
    }

    /// Share text or URL with a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to share with
    /// * `text` - Text or URL to share
    async fn share_text(&self, device_id: String, text: String) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: ShareText called for {} with text '{}'",
            device_id, text
        );

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create share text packet
        use kdeconnect_protocol::Packet;
        use serde_json::json;

        let packet = Packet::new("kdeconnect.share.request", json!({ "text": text }));

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to share text: {}", e)))?;

        info!("DBus: Text shared successfully to {}", device_id);
        Ok(())
    }

    /// Share a URL with a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to share with
    /// * `url` - URL to share (will be opened in default browser on receiving device)
    async fn share_url(&self, device_id: String, url: String) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: ShareUrl called for {} with URL '{}'",
            device_id, url
        );

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create share URL packet
        use kdeconnect_protocol::Packet;
        use serde_json::json;

        let packet = Packet::new("kdeconnect.share.request", json!({ "url": url }));

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to share URL: {}", e)))?;

        info!("DBus: URL shared successfully to {}", device_id);
        Ok(())
    }

    /// Send a notification to a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to send to
    /// * `title` - Notification title
    /// * `body` - Notification body text
    async fn send_notification(
        &self,
        device_id: String,
        title: String,
        body: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: SendNotification called for {} with title '{}'",
            device_id, title
        );

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create notification using NotificationPlugin's helper
        use kdeconnect_protocol::plugins::notification::Notification;
        use kdeconnect_protocol::Packet;
        use std::time::{SystemTime, UNIX_EPOCH};

        // Generate a unique notification ID based on timestamp
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string();

        // Create notification with COSMIC Desktop as the app name
        let notification = Notification::new(
            timestamp.clone(),
            "COSMIC Desktop",
            title,
            body,
            true, // is_clearable
        );

        // Create notification packet
        let packet_body = serde_json::to_value(&notification).map_err(|e| {
            zbus::fdo::Error::Failed(format!("Failed to serialize notification: {}", e))
        })?;
        let packet = Packet::new("kdeconnect.notification", packet_body);

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send notification: {}", e)))?;

        info!(
            "DBus: Notification sent successfully to {} (id: {})",
            device_id, timestamp
        );
        Ok(())
    }

    /// Get battery status from a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to query
    ///
    /// # Returns
    /// Battery status with level and charging state
    async fn get_battery_status(
        &self,
        device_id: String,
    ) -> Result<BatteryStatus, zbus::fdo::Error> {
        debug!("DBus: GetBatteryStatus called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Query battery status from plugin manager
        let plugin_manager = self.plugin_manager.read().await;
        let status = plugin_manager
            .get_device_battery_status(&device_id)
            .ok_or_else(|| {
                zbus::fdo::Error::Failed("No battery status available for device".to_string())
            })?;

        info!(
            "DBus: Battery status for {}: {}%, charging: {}",
            device_id, status.current_charge, status.is_charging
        );

        Ok(BatteryStatus {
            level: status.current_charge,
            is_charging: status.is_charging,
        })
    }

    /// Set device nickname
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `nickname` - The new nickname (empty string to clear)
    async fn set_device_nickname(
        &self,
        device_id: String,
        nickname: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: SetDeviceNickname called for {}: {}",
            device_id, nickname
        );

        let mut registry = self.device_config_registry.write().await;
        let config = registry.get_or_create(&device_id);

        if nickname.is_empty() {
            config.nickname = None;
        } else {
            config.nickname = Some(nickname);
        }

        registry.save().map_err(|e| {
            zbus::fdo::Error::Failed(format!("Failed to save device config: {}", e))
        })?;

        Ok(())
    }

    /// Set plugin enabled state for a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `plugin_name` - The plugin name (ping, battery, notification, share, clipboard, mpris)
    /// * `enabled` - Whether the plugin should be enabled
    async fn set_device_plugin_enabled(
        &self,
        device_id: String,
        plugin_name: String,
        enabled: bool,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: SetDevicePluginEnabled called for {}, plugin {}: {}",
            device_id, plugin_name, enabled
        );

        let mut registry = self.device_config_registry.write().await;
        let config = registry.get_or_create(&device_id);

        config.set_plugin_enabled(&plugin_name, enabled);

        registry.save().map_err(|e| {
            zbus::fdo::Error::Failed(format!("Failed to save device config: {}", e))
        })?;

        info!(
            "DBus: Plugin {} {} for device {}",
            plugin_name,
            if enabled { "enabled" } else { "disabled" },
            device_id
        );

        Ok(())
    }

    /// Clear device-specific plugin override (use global config)
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `plugin_name` - The plugin name
    async fn clear_device_plugin_override(
        &self,
        device_id: String,
        plugin_name: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: ClearDevicePluginOverride called for {}, plugin {}",
            device_id, plugin_name
        );

        let mut registry = self.device_config_registry.write().await;
        let config = registry.get_or_create(&device_id);

        config.clear_plugin_override(&plugin_name);

        registry.save().map_err(|e| {
            zbus::fdo::Error::Failed(format!("Failed to save device config: {}", e))
        })?;

        Ok(())
    }

    /// Get device configuration as JSON string
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    ///
    /// # Returns
    /// JSON string with device configuration
    async fn get_device_config(&self, device_id: String) -> Result<String, zbus::fdo::Error> {
        debug!("DBus: GetDeviceConfig called for {}", device_id);

        let registry = self.device_config_registry.read().await;

        let config = registry.get(&device_id).ok_or_else(|| {
            zbus::fdo::Error::Failed(format!("No config found for device: {}", device_id))
        })?;

        let json = serde_json::to_string_pretty(&config)
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to serialize config: {}", e)))?;

        Ok(json)
    }

    /// Add a run command for a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `command_id` - Unique identifier for the command
    /// * `name` - User-friendly command name
    /// * `command` - Shell command to execute
    async fn add_run_command(
        &self,
        device_id: String,
        command_id: String,
        name: String,
        command: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: AddRunCommand called for {} - ID: {}, Name: {}",
            device_id, command_id, name
        );

        let plugin_manager = self.plugin_manager.read().await;
        let plugin = plugin_manager
            .get_device_plugin(&device_id, "runcommand")
            .ok_or_else(|| {
                zbus::fdo::Error::Failed(format!(
                    "RunCommand plugin not found for device: {}",
                    device_id
                ))
            })?;

        // Downcast to RunCommandPlugin
        use kdeconnect_protocol::plugins::runcommand::RunCommandPlugin;
        let runcommand_plugin = plugin.as_any().downcast_ref::<RunCommandPlugin>().ok_or_else(|| {
            zbus::fdo::Error::Failed("Failed to downcast to RunCommandPlugin".to_string())
        })?;

        // Add the command
        runcommand_plugin
            .add_command(&command_id, &name, &command)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to add command: {}", e)))?;

        info!(
            "DBus: Command '{}' added successfully for device {}",
            command_id, device_id
        );
        Ok(())
    }

    /// Remove a run command from a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `command_id` - Command identifier to remove
    async fn remove_run_command(
        &self,
        device_id: String,
        command_id: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: RemoveRunCommand called for {} - ID: {}",
            device_id, command_id
        );

        let plugin_manager = self.plugin_manager.read().await;
        let plugin = plugin_manager
            .get_device_plugin(&device_id, "runcommand")
            .ok_or_else(|| {
                zbus::fdo::Error::Failed(format!(
                    "RunCommand plugin not found for device: {}",
                    device_id
                ))
            })?;

        // Downcast to RunCommandPlugin
        use kdeconnect_protocol::plugins::runcommand::RunCommandPlugin;
        let runcommand_plugin = plugin.as_any().downcast_ref::<RunCommandPlugin>().ok_or_else(|| {
            zbus::fdo::Error::Failed("Failed to downcast to RunCommandPlugin".to_string())
        })?;

        // Remove the command
        runcommand_plugin
            .remove_command(&command_id)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to remove command: {}", e)))?;

        info!(
            "DBus: Command '{}' removed successfully from device {}",
            command_id, device_id
        );
        Ok(())
    }

    /// Get all run commands for a device as JSON
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    ///
    /// # Returns
    /// JSON string with command map {id: {name: string, command: string}}
    async fn get_run_commands(&self, device_id: String) -> Result<String, zbus::fdo::Error> {
        debug!("DBus: GetRunCommands called for {}", device_id);

        let plugin_manager = self.plugin_manager.read().await;
        let plugin = plugin_manager
            .get_device_plugin(&device_id, "runcommand")
            .ok_or_else(|| {
                zbus::fdo::Error::Failed(format!(
                    "RunCommand plugin not found for device: {}",
                    device_id
                ))
            })?;

        // Downcast to RunCommandPlugin
        use kdeconnect_protocol::plugins::runcommand::RunCommandPlugin;
        let runcommand_plugin = plugin.as_any().downcast_ref::<RunCommandPlugin>().ok_or_else(|| {
            zbus::fdo::Error::Failed("Failed to downcast to RunCommandPlugin".to_string())
        })?;

        // Get all commands
        let commands = runcommand_plugin.get_commands().await;

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&commands)
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to serialize commands: {}", e)))?;

        debug!("DBus: Retrieved {} commands for device {}", commands.len(), device_id);
        Ok(json)
    }

    /// Clear all run commands for a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    async fn clear_run_commands(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: ClearRunCommands called for {}", device_id);

        let plugin_manager = self.plugin_manager.read().await;
        let plugin = plugin_manager
            .get_device_plugin(&device_id, "runcommand")
            .ok_or_else(|| {
                zbus::fdo::Error::Failed(format!(
                    "RunCommand plugin not found for device: {}",
                    device_id
                ))
            })?;

        // Downcast to RunCommandPlugin
        use kdeconnect_protocol::plugins::runcommand::RunCommandPlugin;
        let runcommand_plugin = plugin.as_any().downcast_ref::<RunCommandPlugin>().ok_or_else(|| {
            zbus::fdo::Error::Failed("Failed to downcast to RunCommandPlugin".to_string())
        })?;

        // Clear all commands
        runcommand_plugin
            .clear_commands()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to clear commands: {}", e)))?;

        info!("DBus: All commands cleared for device {}", device_id);
        Ok(())
    }

    /// Signal: Device was added (discovered)
    ///
    /// Emitted when a new device is discovered on the network.
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `device_info` - Device information
    #[zbus(signal)]
    async fn device_added(
        signal_context: &SignalContext<'_>,
        device_id: &str,
        device_info: DeviceInfo,
    ) -> zbus::Result<()>;

    /// Signal: Device was removed (disappeared)
    ///
    /// Emitted when a device is no longer reachable on the network.
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    #[zbus(signal)]
    async fn device_removed(
        signal_context: &SignalContext<'_>,
        device_id: &str,
    ) -> zbus::Result<()>;

    /// Signal: Device state changed
    ///
    /// Emitted when a device's connection state changes (connected, paired, etc).
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `state` - New state: "connected", "paired", "reachable", or "unknown"
    #[zbus(signal)]
    async fn device_state_changed(
        signal_context: &SignalContext<'_>,
        device_id: &str,
        state: &str,
    ) -> zbus::Result<()>;

    /// Signal: Pairing request received
    ///
    /// Emitted when a device requests to pair with us.
    ///
    /// # Arguments
    /// * `device_id` - The device ID requesting pairing
    #[zbus(signal)]
    async fn pairing_request(
        signal_context: &SignalContext<'_>,
        device_id: &str,
    ) -> zbus::Result<()>;

    /// Signal: Pairing status changed
    ///
    /// Emitted when pairing completes or fails.
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `status` - Status: "paired", "rejected", or "failed"
    #[zbus(signal)]
    async fn pairing_status_changed(
        signal_context: &SignalContext<'_>,
        device_id: &str,
        status: &str,
    ) -> zbus::Result<()>;

    /// Signal: Plugin event
    ///
    /// Emitted when a plugin receives data or has something to notify about.
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `plugin` - Plugin name (e.g., "battery", "ping", "share")
    /// * `data` - Plugin-specific JSON data
    #[zbus(signal)]
    async fn plugin_event(
        signal_context: &SignalContext<'_>,
        device_id: &str,
        plugin: &str,
        data: &str,
    ) -> zbus::Result<()>;
}

/// DBus server for the daemon
pub struct DbusServer {
    /// DBus connection
    connection: Connection,
}

impl DbusServer {
    /// Create and start a new DBus server
    ///
    /// # Arguments
    /// * `device_manager` - Device manager reference
    /// * `plugin_manager` - Plugin manager reference
    /// * `connection_manager` - Connection manager reference
    /// * `device_config_registry` - Device configuration registry
    /// * `pairing_service` - Optional pairing service reference
    /// * `mpris_manager` - Optional MPRIS manager for local media player control
    ///
    /// # Returns
    /// DBus server instance with active connection
    pub async fn start(
        device_manager: Arc<RwLock<DeviceManager>>,
        plugin_manager: Arc<RwLock<PluginManager>>,
        connection_manager: Arc<RwLock<ConnectionManager>>,
        device_config_registry: Arc<RwLock<crate::device_config::DeviceConfigRegistry>>,
        pairing_service: Option<Arc<RwLock<kdeconnect_protocol::pairing::PairingService>>>,
        mpris_manager: Option<Arc<crate::mpris_manager::MprisManager>>,
    ) -> Result<Self> {
        info!("Starting DBus server on {}", SERVICE_NAME);

        let interface = KdeConnectInterface::new(
            device_manager,
            plugin_manager,
            connection_manager,
            device_config_registry,
            pairing_service,
            mpris_manager,
        );

        let connection = connection::Builder::session()?
            .name(SERVICE_NAME)?
            .serve_at(OBJECT_PATH, interface)?
            .build()
            .await
            .context("Failed to build DBus connection")?;

        info!("DBus server started successfully");

        Ok(Self { connection })
    }

    /// Get the DBus connection
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Emit a device_added signal
    pub async fn emit_device_added(&self, device: &Device) -> Result<()> {
        let device_info = DeviceInfo::from(device);
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, KdeConnectInterface>(OBJECT_PATH)
            .await?;

        KdeConnectInterface::device_added(iface_ref.signal_context(), device.id(), device_info)
            .await?;

        debug!("Emitted DeviceAdded signal for {}", device.id());
        Ok(())
    }

    /// Emit a device_removed signal
    pub async fn emit_device_removed(&self, device_id: &str) -> Result<()> {
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, KdeConnectInterface>(OBJECT_PATH)
            .await?;

        KdeConnectInterface::device_removed(iface_ref.signal_context(), device_id).await?;

        debug!("Emitted DeviceRemoved signal for {}", device_id);
        Ok(())
    }

    /// Emit a device_state_changed signal
    pub async fn emit_device_state_changed(&self, device_id: &str, state: &str) -> Result<()> {
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, KdeConnectInterface>(OBJECT_PATH)
            .await?;

        KdeConnectInterface::device_state_changed(iface_ref.signal_context(), device_id, state)
            .await?;

        debug!(
            "Emitted DeviceStateChanged signal for {} ({})",
            device_id, state
        );
        Ok(())
    }

    /// Emit a pairing_request signal
    pub async fn emit_pairing_request(&self, device_id: &str) -> Result<()> {
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, KdeConnectInterface>(OBJECT_PATH)
            .await?;

        KdeConnectInterface::pairing_request(iface_ref.signal_context(), device_id).await?;

        debug!("Emitted PairingRequest signal for {}", device_id);
        Ok(())
    }

    /// Emit a pairing_status_changed signal
    pub async fn emit_pairing_status_changed(&self, device_id: &str, status: &str) -> Result<()> {
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, KdeConnectInterface>(OBJECT_PATH)
            .await?;

        KdeConnectInterface::pairing_status_changed(iface_ref.signal_context(), device_id, status)
            .await?;

        debug!(
            "Emitted PairingStatusChanged signal for {} ({})",
            device_id, status
        );
        Ok(())
    }

    /// Emit a plugin_event signal
    pub async fn emit_plugin_event(&self, device_id: &str, plugin: &str, data: &str) -> Result<()> {
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, KdeConnectInterface>(OBJECT_PATH)
            .await?;

        KdeConnectInterface::plugin_event(iface_ref.signal_context(), device_id, plugin, data)
            .await?;

        debug!("Emitted PluginEvent signal for {} ({})", device_id, plugin);
        Ok(())
    }
}
