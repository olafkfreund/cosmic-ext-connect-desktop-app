//! DBus Client for CConnect Applet
//!
//! Provides communication with the CConnect daemon via DBus.
//! Handles method calls, signal subscription, and error recovery.

use anyhow::{Context, Result};
use futures::stream::StreamExt;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use zbus::{proxy, Connection};

/// DBus service name
pub const SERVICE_NAME: &str = "com.system76.CosmicConnect";

/// DBus object path
pub const OBJECT_PATH: &str = "/com/system76/CosmicConnect";

/// Device information from DBus
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

/// Battery status from DBus
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, zbus::zvariant::Type)]
pub struct BatteryStatus {
    /// Battery level percentage (0-100)
    pub level: i32,
    /// Is device charging
    pub is_charging: bool,
}

/// Device-specific configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceConfig {
    /// Device ID
    pub device_id: String,
    /// Optional nickname for the device
    pub nickname: Option<String>,
    /// Plugin configuration
    pub plugins: DevicePluginConfig,
    /// RemoteDesktop plugin-specific settings
    #[serde(default)]
    pub remotedesktop_settings: Option<RemoteDesktopSettings>,
}

/// Per-device plugin configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DevicePluginConfig {
    /// Ping plugin (None = use global config)
    pub enable_ping: Option<bool>,
    /// Battery plugin
    pub enable_battery: Option<bool>,
    /// Notification plugin
    pub enable_notification: Option<bool>,
    /// Share plugin
    pub enable_share: Option<bool>,
    /// Clipboard plugin
    pub enable_clipboard: Option<bool>,
    /// MPRIS plugin
    pub enable_mpris: Option<bool>,
    /// RemoteDesktop plugin
    pub enable_remotedesktop: Option<bool>,
    /// FindMyPhone plugin
    pub enable_findmyphone: Option<bool>,
}

/// RemoteDesktop plugin-specific settings
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteDesktopSettings {
    /// Quality preset: "low", "medium", "high"
    pub quality: String,
    /// Frames per second: 15, 30, or 60
    pub fps: u8,
    /// Resolution mode: "native" or "custom"
    pub resolution_mode: String,
    /// Custom width (only used if resolution_mode = "custom")
    pub custom_width: Option<u32>,
    /// Custom height (only used if resolution_mode = "custom")
    pub custom_height: Option<u32>,
}

impl Default for RemoteDesktopSettings {
    fn default() -> Self {
        Self {
            quality: "medium".to_string(),
            fps: 30,
            resolution_mode: "native".to_string(),
            custom_width: None,
            custom_height: None,
        }
    }
}

impl DeviceConfig {
    /// Get whether a plugin is enabled (considering device override vs global)
    pub fn get_plugin_enabled(&self, plugin: &str) -> bool {
        match plugin {
            "ping" => self.plugins.enable_ping.unwrap_or(true),
            "battery" => self.plugins.enable_battery.unwrap_or(true),
            "notification" => self.plugins.enable_notification.unwrap_or(true),
            "share" => self.plugins.enable_share.unwrap_or(true),
            "clipboard" => self.plugins.enable_clipboard.unwrap_or(true),
            "mpris" => self.plugins.enable_mpris.unwrap_or(true),
            "remotedesktop" => self.plugins.enable_remotedesktop.unwrap_or(false),
            "findmyphone" => self.plugins.enable_findmyphone.unwrap_or(true),
            _ => false,
        }
    }

    /// Check if a plugin has a device-specific override
    pub fn has_plugin_override(&self, plugin: &str) -> bool {
        match plugin {
            "ping" => self.plugins.enable_ping.is_some(),
            "battery" => self.plugins.enable_battery.is_some(),
            "notification" => self.plugins.enable_notification.is_some(),
            "share" => self.plugins.enable_share.is_some(),
            "clipboard" => self.plugins.enable_clipboard.is_some(),
            "mpris" => self.plugins.enable_mpris.is_some(),
            "remotedesktop" => self.plugins.enable_remotedesktop.is_some(),
            "findmyphone" => self.plugins.enable_findmyphone.is_some(),
            _ => false,
        }
    }
}

/// Events emitted by the daemon
#[derive(Debug, Clone)]
pub enum DaemonEvent {
    /// Device was added (discovered)
    DeviceAdded {
        device_id: String,
        device_info: DeviceInfo,
    },
    /// Device was removed (disappeared)
    DeviceRemoved { device_id: String },
    /// Device state changed
    DeviceStateChanged { device_id: String, state: String },
    /// Pairing request received
    PairingRequest { device_id: String },
    /// Pairing status changed
    PairingStatusChanged { device_id: String, status: String },
    /// Plugin event
    PluginEvent {
        device_id: String,
        plugin: String,
        data: String,
    },
    /// Device plugin state changed
    DevicePluginStateChanged {
        device_id: String,
        plugin_name: String,
        enabled: bool,
    },
    /// Daemon disconnected
    DaemonDisconnected,
    /// Daemon reconnected
    DaemonReconnected,
}

/// DBus proxy for COSMIC Connect daemon interface
#[proxy(
    interface = "com.system76.CosmicConnect",
    default_service = "com.system76.CosmicConnect",
    default_path = "/com/system76/CosmicConnect"
)]
trait CConnect {
    /// List all known devices
    async fn list_devices(&self) -> zbus::Result<HashMap<String, DeviceInfo>>;

    /// Get information about a specific device
    async fn get_device(&self, device_id: &str) -> zbus::Result<DeviceInfo>;

    /// Request pairing with a device
    async fn pair_device(&self, device_id: &str) -> zbus::Result<()>;

    /// Unpair a device
    async fn unpair_device(&self, device_id: &str) -> zbus::Result<()>;

    /// Trigger device discovery
    async fn refresh_discovery(&self) -> zbus::Result<()>;

    /// Get device connection state
    async fn get_device_state(&self, device_id: &str) -> zbus::Result<String>;

    /// Send a ping to a device
    async fn send_ping(&self, device_id: &str, message: &str) -> zbus::Result<()>;

    /// Trigger find phone on a device
    async fn find_phone(&self, device_id: &str) -> zbus::Result<()>;

    /// Share a file with a device
    async fn share_file(&self, device_id: &str, path: &str) -> zbus::Result<()>;

    /// Share text or URL with a device
    async fn share_text(&self, device_id: &str, text: &str) -> zbus::Result<()>;

    /// Share URL with a device
    async fn share_url(&self, device_id: &str, url: &str) -> zbus::Result<()>;

    /// Send a notification to a device
    async fn send_notification(&self, device_id: &str, title: &str, body: &str)
        -> zbus::Result<()>;

    /// Get battery status from a device
    async fn get_battery_status(&self, device_id: &str) -> zbus::Result<BatteryStatus>;

    /// Request battery update from a device
    async fn request_battery_update(&self, device_id: &str) -> zbus::Result<()>;

    /// Get list of available MPRIS media players
    async fn get_mpris_players(&self) -> zbus::Result<Vec<String>>;

    /// Control MPRIS player playback
    async fn mpris_control(&self, player: &str, action: &str) -> zbus::Result<()>;

    /// Set MPRIS player volume
    async fn mpris_set_volume(&self, player: &str, volume: f64) -> zbus::Result<()>;

    /// Seek MPRIS player position
    async fn mpris_seek(&self, player: &str, offset_microseconds: i64) -> zbus::Result<()>;

    /// Get device configuration (plugin settings)
    async fn get_device_config(&self, device_id: &str) -> zbus::Result<String>;

    /// Set plugin enabled state for a device
    async fn set_device_plugin_enabled(
        &self,
        device_id: &str,
        plugin: &str,
        enabled: bool,
    ) -> zbus::Result<()>;

    /// Clear device-specific plugin override
    async fn clear_device_plugin_override(&self, device_id: &str, plugin: &str)
        -> zbus::Result<()>;

    /// Get RemoteDesktop settings for a device
    async fn get_remotedesktop_settings(&self, device_id: &str) -> zbus::Result<String>;

    /// Set RemoteDesktop settings for a device
    async fn set_remotedesktop_settings(&self, device_id: &str, settings_json: &str)
        -> zbus::Result<()>;

    /// Signal: Device was added
    #[zbus(signal)]
    fn device_added(device_id: &str, device_info: DeviceInfo) -> zbus::Result<()>;

    /// Signal: Device was removed
    #[zbus(signal)]
    fn device_removed(device_id: &str) -> zbus::Result<()>;

    /// Signal: Device state changed
    #[zbus(signal)]
    fn device_state_changed(device_id: &str, state: &str) -> zbus::Result<()>;

    /// Signal: Pairing request received
    #[zbus(signal)]
    fn pairing_request(device_id: &str) -> zbus::Result<()>;

    /// Signal: Pairing status changed
    #[zbus(signal)]
    fn pairing_status_changed(device_id: &str, status: &str) -> zbus::Result<()>;

    /// Signal: Plugin event
    #[zbus(signal)]
    fn plugin_event(device_id: &str, plugin: &str, data: &str) -> zbus::Result<()>;

    /// Signal: Device plugin state changed
    #[zbus(signal)]
    fn device_plugin_state_changed(device_id: &str, plugin_name: &str, enabled: bool) -> zbus::Result<()>;
}

/// DBus client for communicating with the daemon
pub struct DbusClient {
    /// DBus connection
    connection: Connection,
    /// Proxy to daemon interface
    proxy: CConnectProxy<'static>,
    /// Event sender
    event_tx: mpsc::UnboundedSender<DaemonEvent>,
}

impl DbusClient {
    /// Create a new DBus client and connect to the daemon
    ///
    /// # Returns
    /// DBus client instance and event receiver
    pub async fn connect() -> Result<(Self, mpsc::UnboundedReceiver<DaemonEvent>)> {
        info!("Connecting to CConnect daemon via DBus");

        let connection = Connection::session()
            .await
            .context("Failed to connect to session bus")?;

        let proxy = CConnectProxy::new(&connection)
            .await
            .context("Failed to create proxy")?;

        let (event_tx, event_rx) = mpsc::unbounded_channel();

        info!("Connected to daemon successfully");

        Ok((
            Self {
                connection,
                proxy,
                event_tx,
            },
            event_rx,
        ))
    }

    /// Start listening for signals from the daemon
    pub async fn start_signal_listener(&self) -> Result<()> {
        debug!("Starting signal listener");

        let event_tx = self.event_tx.clone();
        let mut device_added_stream = self.proxy.receive_device_added().await?;
        tokio::spawn(async move {
            while let Some(signal) = device_added_stream.next().await {
                if let Ok(args) = signal.args() {
                    let device_id = args.device_id().to_string();
                    let device_info = args.device_info().clone();
                    let _ = event_tx.send(DaemonEvent::DeviceAdded {
                        device_id,
                        device_info,
                    });
                }
            }
        });

        let event_tx = self.event_tx.clone();
        let mut device_removed_stream = self.proxy.receive_device_removed().await?;
        tokio::spawn(async move {
            while let Some(signal) = device_removed_stream.next().await {
                if let Ok(args) = signal.args() {
                    let device_id = args.device_id().to_string();
                    let _ = event_tx.send(DaemonEvent::DeviceRemoved { device_id });
                }
            }
        });

        let event_tx = self.event_tx.clone();
        let mut device_state_changed_stream = self.proxy.receive_device_state_changed().await?;
        tokio::spawn(async move {
            while let Some(signal) = device_state_changed_stream.next().await {
                if let Ok(args) = signal.args() {
                    let device_id = args.device_id().to_string();
                    let state = args.state().to_string();
                    let _ = event_tx.send(DaemonEvent::DeviceStateChanged { device_id, state });
                }
            }
        });

        let event_tx = self.event_tx.clone();
        let mut pairing_request_stream = self.proxy.receive_pairing_request().await?;
        tokio::spawn(async move {
            while let Some(signal) = pairing_request_stream.next().await {
                if let Ok(args) = signal.args() {
                    let device_id = args.device_id().to_string();
                    let _ = event_tx.send(DaemonEvent::PairingRequest { device_id });
                }
            }
        });

        let event_tx = self.event_tx.clone();
        let mut pairing_status_changed_stream = self.proxy.receive_pairing_status_changed().await?;
        tokio::spawn(async move {
            while let Some(signal) = pairing_status_changed_stream.next().await {
                if let Ok(args) = signal.args() {
                    let device_id = args.device_id().to_string();
                    let status = args.status().to_string();
                    let _ = event_tx.send(DaemonEvent::PairingStatusChanged { device_id, status });
                }
            }
        });

        let event_tx = self.event_tx.clone();
        let mut plugin_event_stream = self.proxy.receive_plugin_event().await?;
        tokio::spawn(async move {
            while let Some(signal) = plugin_event_stream.next().await {
                if let Ok(args) = signal.args() {
                    let device_id = args.device_id().to_string();
                    let plugin = args.plugin().to_string();
                    let data = args.data().to_string();
                    let _ = event_tx.send(DaemonEvent::PluginEvent {
                        device_id,
                        plugin,
                        data,
                    });
                }
            }
        });

        let event_tx = self.event_tx.clone();
        let mut plugin_state_changed_stream = self.proxy.receive_device_plugin_state_changed().await?;
        tokio::spawn(async move {
            while let Some(signal) = plugin_state_changed_stream.next().await {
                if let Ok(args) = signal.args() {
                    let device_id = args.device_id().to_string();
                    let plugin_name = args.plugin_name().to_string();
                    let enabled = *args.enabled();
                    let _ = event_tx.send(DaemonEvent::DevicePluginStateChanged {
                        device_id,
                        plugin_name,
                        enabled,
                    });
                }
            }
        });

        info!("Signal listener started");
        Ok(())
    }

    /// List all devices from the daemon
    pub async fn list_devices(&self) -> Result<HashMap<String, DeviceInfo>> {
        debug!("Listing devices from daemon");
        self.proxy
            .list_devices()
            .await
            .context("Failed to list devices")
    }

    /// Get information about a specific device
    pub async fn get_device(&self, device_id: &str) -> Result<DeviceInfo> {
        debug!("Getting device info for {}", device_id);
        self.proxy
            .get_device(device_id)
            .await
            .context("Failed to get device info")
    }

    /// Request pairing with a device
    pub async fn pair_device(&self, device_id: &str) -> Result<()> {
        info!("Requesting pairing with device {}", device_id);
        self.proxy
            .pair_device(device_id)
            .await
            .context("Failed to pair device")
    }

    /// Unpair a device
    pub async fn unpair_device(&self, device_id: &str) -> Result<()> {
        info!("Unpairing device {}", device_id);
        self.proxy
            .unpair_device(device_id)
            .await
            .context("Failed to unpair device")
    }

    /// Trigger device discovery
    pub async fn refresh_discovery(&self) -> Result<()> {
        debug!("Refreshing device discovery");
        self.proxy
            .refresh_discovery()
            .await
            .context("Failed to refresh discovery")
    }

    /// Get device connection state
    pub async fn get_device_state(&self, device_id: &str) -> Result<String> {
        debug!("Getting device state for {}", device_id);
        self.proxy
            .get_device_state(device_id)
            .await
            .context("Failed to get device state")
    }

    /// Send a ping to a device
    pub async fn send_ping(&self, device_id: &str, message: &str) -> Result<()> {
        info!("Sending ping to device {}: {}", device_id, message);
        self.proxy
            .send_ping(device_id, message)
            .await
            .context("Failed to send ping")
    }

    /// Trigger find phone on a device
    pub async fn find_phone(&self, device_id: &str) -> Result<()> {
        info!("Triggering find phone for device {}", device_id);
        self.proxy
            .find_phone(device_id)
            .await
            .context("Failed to trigger find phone")
    }

    /// Share a file with a device
    pub async fn share_file(&self, device_id: &str, path: &str) -> Result<()> {
        info!("Sharing file {} with device {}", path, device_id);
        self.proxy
            .share_file(device_id, path)
            .await
            .context("Failed to share file")
    }

    /// Share text with a device
    pub async fn share_text(&self, device_id: &str, text: &str) -> Result<()> {
        info!("Sharing text with device {}: {}", device_id, text);
        self.proxy
            .share_text(device_id, text)
            .await
            .context("Failed to share text")
    }

    /// Share URL with a device
    pub async fn share_url(&self, device_id: &str, url: &str) -> Result<()> {
        info!("Sharing URL with device {}: {}", device_id, url);
        self.proxy
            .share_url(device_id, url)
            .await
            .context("Failed to share URL")
    }

    /// Send a notification to a device
    pub async fn send_notification(&self, device_id: &str, title: &str, body: &str) -> Result<()> {
        info!("Sending notification to device {}: {}", device_id, title);
        self.proxy
            .send_notification(device_id, title, body)
            .await
            .context("Failed to send notification")
    }

    /// Get battery status from a device
    pub async fn get_battery_status(&self, device_id: &str) -> Result<BatteryStatus> {
        debug!("Getting battery status for device {}", device_id);
        self.proxy
            .get_battery_status(device_id)
            .await
            .context("Failed to get battery status")
    }

    /// Request battery update from a device
    pub async fn request_battery_update(&self, device_id: &str) -> Result<()> {
        info!("Requesting battery update from device {}", device_id);
        self.proxy
            .request_battery_update(device_id)
            .await
            .context("Failed to request battery update")
    }

    /// Get list of available MPRIS media players
    pub async fn get_mpris_players(&self) -> Result<Vec<String>> {
        debug!("Getting MPRIS player list");
        self.proxy
            .get_mpris_players()
            .await
            .context("Failed to get MPRIS players")
    }

    /// Control MPRIS player playback
    ///
    /// # Arguments
    /// * `player` - Player name (e.g., "spotify", "vlc")
    /// * `action` - Action: "Play", "Pause", "PlayPause", "Stop", "Next", "Previous"
    pub async fn mpris_control(&self, player: &str, action: &str) -> Result<()> {
        info!("Sending MPRIS control {} to player {}", action, player);
        self.proxy
            .mpris_control(player, action)
            .await
            .context("Failed to control MPRIS player")
    }

    /// Set MPRIS player volume
    ///
    /// # Arguments
    /// * `player` - Player name
    /// * `volume` - Volume level (0.0 to 1.0)
    pub async fn mpris_set_volume(&self, player: &str, volume: f64) -> Result<()> {
        info!("Setting MPRIS volume to {} for player {}", volume, player);
        self.proxy
            .mpris_set_volume(player, volume)
            .await
            .context("Failed to set MPRIS volume")
    }

    /// Seek MPRIS player position
    ///
    /// # Arguments
    /// * `player` - Player name
    /// * `offset_microseconds` - Seek offset in microseconds (can be negative)
    pub async fn mpris_seek(&self, player: &str, offset_microseconds: i64) -> Result<()> {
        info!("Seeking MPRIS player {} by {}μs", player, offset_microseconds);
        self.proxy
            .mpris_seek(player, offset_microseconds)
            .await
            .context("Failed to seek MPRIS player")
    }

    /// Get device configuration (plugin settings)
    pub async fn get_device_config(&self, device_id: &str) -> Result<DeviceConfig> {
        debug!("Getting device config for {}", device_id);
        let json = self
            .proxy
            .get_device_config(device_id)
            .await
            .context("Failed to get device config")?;

        serde_json::from_str(&json).context("Failed to parse device config")
    }

    /// Set plugin enabled state for a device
    ///
    /// # Arguments
    /// * `device_id` - Device ID
    /// * `plugin` - Plugin name (e.g., "ping", "battery", "remotedesktop")
    /// * `enabled` - Enable or disable the plugin
    pub async fn set_device_plugin_enabled(
        &self,
        device_id: &str,
        plugin: &str,
        enabled: bool,
    ) -> Result<()> {
        info!(
            "Setting plugin {} to {} for device {}",
            plugin,
            if enabled { "enabled" } else { "disabled" },
            device_id
        );
        self.proxy
            .set_device_plugin_enabled(device_id, plugin, enabled)
            .await
            .context("Failed to set device plugin enabled")
    }

    /// Clear device-specific plugin override
    ///
    /// # Arguments
    /// * `device_id` - Device ID
    /// * `plugin` - Plugin name
    pub async fn clear_device_plugin_override(&self, device_id: &str, plugin: &str) -> Result<()> {
        info!("Clearing plugin override for {} on device {}", plugin, device_id);
        self.proxy
            .clear_device_plugin_override(device_id, plugin)
            .await
            .context("Failed to clear device plugin override")
    }

    /// Get RemoteDesktop settings for a device
    ///
    /// # Arguments
    /// * `device_id` - Device ID
    pub async fn get_remotedesktop_settings(&self, device_id: &str) -> Result<RemoteDesktopSettings> {
        debug!("Getting RemoteDesktop settings for {}", device_id);
        let json = self
            .proxy
            .get_remotedesktop_settings(device_id)
            .await
            .context("Failed to get RemoteDesktop settings")?;

        serde_json::from_str(&json).context("Failed to parse RemoteDesktop settings")
    }

    /// Set RemoteDesktop settings for a device
    ///
    /// # Arguments
    /// * `device_id` - Device ID
    /// * `settings` - RemoteDesktop settings to save
    pub async fn set_remotedesktop_settings(
        &self,
        device_id: &str,
        settings: &RemoteDesktopSettings,
    ) -> Result<()> {
        info!("Setting RemoteDesktop settings for {}", device_id);
        let json = serde_json::to_string(settings)
            .context("Failed to serialize RemoteDesktop settings")?;

        self.proxy
            .set_remotedesktop_settings(device_id, &json)
            .await
            .context("Failed to set RemoteDesktop settings")
    }

    /// Check if daemon is available
    pub async fn is_daemon_available(&self) -> bool {
        // Try to list devices as a health check
        self.proxy.list_devices().await.is_ok()
    }
}

/// Auto-reconnecting DBus client wrapper
pub struct ReconnectingClient {
    /// Current client (None if disconnected)
    client: Option<DbusClient>,
    /// Event receiver
    event_rx: mpsc::UnboundedReceiver<DaemonEvent>,
    /// Event sender for reconnection
    reconnect_tx: mpsc::UnboundedSender<DaemonEvent>,
}

impl ReconnectingClient {
    /// Create a new reconnecting client
    pub async fn new() -> Result<Self> {
        let (client, event_rx) = DbusClient::connect().await?;
        client.start_signal_listener().await?;

        let reconnect_tx = client.event_tx.clone();

        Ok(Self {
            client: Some(client),
            event_rx,
            reconnect_tx,
        })
    }

    /// Get a reference to the current client
    ///
    /// Returns None if disconnected
    pub fn client(&self) -> Option<&DbusClient> {
        self.client.as_ref()
    }

    /// Attempt to reconnect to the daemon
    pub async fn reconnect(&mut self) -> Result<()> {
        info!("Attempting to reconnect to daemon");

        match DbusClient::connect().await {
            Ok((client, new_event_rx)) => {
                client.start_signal_listener().await?;

                // Notify about reconnection
                let _ = self.reconnect_tx.send(DaemonEvent::DaemonReconnected);

                self.client = Some(client);
                self.event_rx = new_event_rx;

                info!("Reconnected to daemon successfully");
                Ok(())
            }
            Err(e) => {
                warn!("Failed to reconnect: {}", e);
                Err(e)
            }
        }
    }

    /// Receive the next daemon event
    pub async fn recv_event(&mut self) -> Option<DaemonEvent> {
        self.event_rx.recv().await
    }

    /// Try to receive an event without blocking
    pub fn try_recv_event(&mut self) -> Result<DaemonEvent, mpsc::error::TryRecvError> {
        self.event_rx.try_recv()
    }
}
