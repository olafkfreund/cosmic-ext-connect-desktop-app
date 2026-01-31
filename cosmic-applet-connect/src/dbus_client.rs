//! DBus Client for CConnect Applet
//!
//! Provides communication with the CConnect daemon via DBus.
//! Handles method calls, signal subscription, and error recovery.

use anyhow::{Context, Result};
#[allow(dead_code)]
use futures::stream::StreamExt;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use zbus::{proxy, Connection};

/// DBus service name
#[allow(dead_code)]
pub const SERVICE_NAME: &str = "com.system76.CosmicConnect";

/// DBus object path
#[allow(dead_code)]
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
    /// Has pending pairing request
    pub has_pairing_request: bool,
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

/// Screen share statistics from DBus
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, zbus::zvariant::Type)]
pub struct ScreenShareStats {
    /// Current number of viewers
    pub viewer_count: u32,
    /// Session duration in seconds
    pub duration_secs: u64,
    /// Total frames sent
    pub frames_sent: u64,
    /// Average FPS
    pub avg_fps: u64,
}

/// Notification preference for a device
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationPreference {
    /// Show all notifications from this device
    All,
    /// Show only important notifications (messaging apps, calls, etc.)
    Important,
    /// Don't show any notifications from this device
    None,
}

impl Default for NotificationPreference {
    fn default() -> Self {
        Self::All
    }
}

impl std::fmt::Display for NotificationPreference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotificationPreference::All => write!(f, "All"),
            NotificationPreference::Important => write!(f, "Important only"),
            NotificationPreference::None => write!(f, "None"),
        }
    }
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
    /// Notification preference for this device
    #[serde(default)]
    pub notification_preference: NotificationPreference,
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

/// Sync Folder configuration from DBus
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, zbus::zvariant::Type)]
pub struct SyncFolderInfo {
    pub folder_id: String,
    pub path: String,
    pub strategy: String,
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

/// Playback status from MPRIS2
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

/// Loop status from MPRIS2
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LoopStatus {
    None,
    Track,
    Playlist,
}

/// Media player metadata
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PlayerMetadata {
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub album_art_url: Option<String>,
    pub length: i64, // microseconds
}

/// Run Command definition
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunCommand {
    pub name: String,
    pub command: String,
}

/// Player state
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerState {
    pub name: String,
    pub identity: String,
    pub playback_status: PlaybackStatus,
    pub position: i64, // microseconds
    pub volume: f64,   // 0.0 to 1.0
    pub loop_status: LoopStatus,
    pub shuffle: bool,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub can_seek: bool,
    pub metadata: PlayerMetadata,
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

    /// Count how many plugin overrides this device has
    pub fn count_plugin_overrides(&self) -> usize {
        let mut count = 0;
        if self.plugins.enable_ping.is_some() {
            count += 1;
        }
        if self.plugins.enable_battery.is_some() {
            count += 1;
        }
        if self.plugins.enable_notification.is_some() {
            count += 1;
        }
        if self.plugins.enable_share.is_some() {
            count += 1;
        }
        if self.plugins.enable_clipboard.is_some() {
            count += 1;
        }
        if self.plugins.enable_mpris.is_some() {
            count += 1;
        }
        if self.plugins.enable_remotedesktop.is_some() {
            count += 1;
        }
        if self.plugins.enable_findmyphone.is_some() {
            count += 1;
        }
        count
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
    PairingStatusChanged {
        #[allow(dead_code)]
        device_id: String,
        status: String,
    },
    /// Plugin event
    PluginEvent {
        #[allow(dead_code)]
        device_id: String,
        #[allow(dead_code)]
        plugin: String,
        #[allow(dead_code)]
        data: String,
    },
    /// Device plugin state changed
    DevicePluginStateChanged {
        #[allow(dead_code)]
        device_id: String,
        #[allow(dead_code)]
        plugin_name: String,
        #[allow(dead_code)]
        enabled: bool,
    },
    /// Daemon disconnected
    #[allow(dead_code)]
    DaemonDisconnected,
    /// Daemon reconnected
    #[allow(dead_code)]
    DaemonReconnected,
    /// File transfer progress
    TransferProgress {
        transfer_id: String,
        device_id: String,
        filename: String,
        current: u64,
        total: u64,
        direction: String,
    },
    /// File transfer complete
    TransferComplete {
        transfer_id: String,
        device_id: String,
        filename: String,
        success: bool,
        error: String,
    },
    /// Screen share requested by remote device
    ScreenShareRequested { device_id: String },
    /// Screen share cursor position update
    ScreenShareCursorUpdate {
        device_id: String,
        x: i32,
        y: i32,
        visible: bool,
    },
    /// Screen share annotation received
    ScreenShareAnnotation {
        device_id: String,
        annotation_type: String,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        color: String,
        width: u8,
    },
    /// Screen share session started
    ScreenShareStarted { device_id: String, is_sender: bool },
    /// Screen share session stopped
    ScreenShareStopped { device_id: String },
}

/// DBus proxy for COSMIC Connect daemon interface
#[proxy(
    interface = "com.system76.CosmicConnect",
    default_service = "com.system76.CosmicConnect",
    default_path = "/com/system76/CosmicConnect"
)]
trait CConnect {
    /// List all known devices
    async fn list_devices(&self) -> zbus::fdo::Result<HashMap<String, DeviceInfo>>;

    /// Get information about a specific device
    async fn get_device(&self, device_id: &str) -> zbus::fdo::Result<DeviceInfo>;

    /// Request pairing with a device
    async fn pair_device(&self, device_id: &str) -> zbus::fdo::Result<()>;

    /// Unpair a device
    async fn unpair_device(&self, device_id: &str) -> zbus::fdo::Result<()>;

    /// Trigger device discovery
    async fn refresh_discovery(&self) -> zbus::fdo::Result<()>;

    /// Connect to a device at a specific address
    async fn connect_to_address(&self, address: &str) -> zbus::fdo::Result<()>;

    /// Get device connection state
    async fn get_device_state(&self, device_id: &str) -> zbus::fdo::Result<String>;

    /// Send a ping to a device
    async fn send_ping(&self, device_id: &str, message: &str) -> zbus::fdo::Result<()>;

    /// Trigger find phone on a device
    async fn find_phone(&self, device_id: &str) -> zbus::fdo::Result<()>;

    /// Share a file with a device
    async fn share_file(&self, device_id: &str, path: &str) -> zbus::fdo::Result<()>;

    /// Cancel an active file transfer
    async fn cancel_transfer(&self, transfer_id: &str) -> zbus::fdo::Result<()>;

    /// Share text or URL with a device
    async fn share_text(&self, device_id: &str, text: &str) -> zbus::fdo::Result<()>;

    /// Share URL with a device
    async fn share_url(&self, device_id: &str, url: &str) -> zbus::fdo::Result<()>;

    /// Send a notification to a device
    async fn send_notification(
        &self,
        device_id: &str,
        title: &str,
        body: &str,
    ) -> zbus::fdo::Result<()>;

    /// Get battery status from a device
    async fn get_battery_status(&self, device_id: &str) -> zbus::fdo::Result<BatteryStatus>;

    /// Get screen share statistics from a device
    async fn get_screen_share_stats(&self, device_id: &str) -> zbus::fdo::Result<ScreenShareStats>;

    /// Request battery update from a device
    async fn request_battery_update(&self, device_id: &str) -> zbus::fdo::Result<()>;

    /// Get list of available MPRIS media players
    async fn get_mpris_players(&self) -> zbus::fdo::Result<Vec<String>>;

    /// Get detailed state for a specific MPRIS player
    async fn get_player_state(&self, player: &str) -> zbus::fdo::Result<String>;

    /// Control MPRIS player playback
    async fn mpris_control(&self, player: &str, action: &str) -> zbus::fdo::Result<()>;

    /// Set MPRIS player volume
    async fn mpris_set_volume(&self, player: &str, volume: f64) -> zbus::fdo::Result<()>;

    /// Seek MPRIS player position
    async fn mpris_seek(&self, player: &str, offset_microseconds: i64) -> zbus::fdo::Result<()>;

    /// Raise MPRIS player window (bring to front)
    async fn mpris_raise(&self, player: &str) -> zbus::fdo::Result<()>;

    /// Get device configuration (plugin settings)
    async fn get_device_config(&self, device_id: &str) -> zbus::fdo::Result<String>;

    /// Set plugin enabled state for a device
    async fn set_device_plugin_enabled(
        &self,
        device_id: &str,
        plugin: &str,
        enabled: bool,
    ) -> zbus::fdo::Result<()>;

    /// Clear device-specific plugin override
    async fn clear_device_plugin_override(
        &self,
        device_id: &str,
        plugin: &str,
    ) -> zbus::fdo::Result<()>;

    /// Reset all plugin overrides for a device (revert to global config)
    async fn reset_all_plugin_overrides(&self, device_id: &str) -> zbus::fdo::Result<()>;

    /// Get RemoteDesktop settings for a device
    async fn get_remotedesktop_settings(&self, device_id: &str) -> zbus::fdo::Result<String>;

    /// Set a custom nickname for a device
    async fn set_device_nickname(&self, device_id: &str, nickname: &str) -> zbus::fdo::Result<()>;

    /// Set notification preference for a device
    async fn set_device_notification_preference(
        &self,
        device_id: &str,
        preference: &str,
    ) -> zbus::fdo::Result<()>;

    /// Set RemoteDesktop settings for a device
    async fn set_remotedesktop_settings(
        &self,
        device_id: &str,
        settings_json: &str,
    ) -> zbus::fdo::Result<()>;

    /// Add a folder to sync with a device
    async fn add_sync_folder(
        &self,
        device_id: String,
        folder_id: String,
        path: String,
        strategy: String,
    ) -> zbus::fdo::Result<()>;

    /// Remove a sync folder from a device
    async fn remove_sync_folder(
        &self,
        device_id: String,
        folder_id: String,
    ) -> zbus::fdo::Result<()>;

    /// Get list of synced folders for a device
    async fn get_sync_folders(&self, device_id: String) -> zbus::fdo::Result<Vec<SyncFolderInfo>>;

    /// Signal: Device was added
    #[zbus(signal)]
    fn device_added(device_id: &str, device_info: DeviceInfo) -> zbus::fdo::Result<()>;

    /// Signal: Device was removed
    #[zbus(signal)]
    fn device_removed(device_id: &str) -> zbus::fdo::Result<()>;

    /// Signal: Device state changed
    #[zbus(signal)]
    fn device_state_changed(device_id: &str, state: &str) -> zbus::fdo::Result<()>;

    /// Signal: Pairing request received
    #[zbus(signal)]
    fn pairing_request(device_id: &str) -> zbus::fdo::Result<()>;

    /// Signal: Pairing status changed
    #[zbus(signal)]
    fn pairing_status_changed(device_id: &str, status: &str) -> zbus::fdo::Result<()>;

    /// Signal: Plugin event
    #[zbus(signal)]
    fn plugin_event(device_id: &str, plugin: &str, data: &str) -> zbus::fdo::Result<()>;

    /// Signal: Device plugin state changed
    #[zbus(signal)]
    fn device_plugin_state_changed(
        device_id: &str,
        plugin_name: &str,
        enabled: bool,
    ) -> zbus::fdo::Result<()>;

    /// Signal: File transfer progress
    #[zbus(signal)]
    fn transfer_progress(
        transfer_id: &str,
        device_id: &str,
        filename: &str,
        current: u64,
        total: u64,
        direction: &str,
    ) -> zbus::fdo::Result<()>;

    /// Add a run command
    async fn add_run_command(
        &self,
        device_id: String,
        command_id: String,
        name: String,
        command: String,
    ) -> zbus::fdo::Result<()>;

    /// Remove a run command
    async fn remove_run_command(
        &self,
        device_id: String,
        command_id: String,
    ) -> zbus::fdo::Result<()>;

    /// Get run commands (returns JSON string map of id -> Command)
    async fn get_run_commands(&self, device_id: String) -> zbus::fdo::Result<String>;

    /// Start screen share
    async fn start_screen_share(&self, device_id: &str, port: u16) -> zbus::fdo::Result<()>;

    /// Stop screen share session
    async fn stop_screen_share(&self, device_id: &str) -> zbus::fdo::Result<()>;

    /// Pause screen share session
    async fn pause_screen_share(&self, device_id: &str) -> zbus::fdo::Result<()>;

    /// Resume screen share session
    async fn resume_screen_share(&self, device_id: &str) -> zbus::fdo::Result<()>;

    /// Send screen mirror input
    async fn send_mirror_input(
        &self,
        device_id: String,
        x: f32,
        y: f32,
        action: String,
    ) -> zbus::fdo::Result<()>;

    /// Signal: File transfer complete
    #[zbus(signal)]
    fn transfer_complete(
        transfer_id: &str,
        device_id: &str,
        filename: &str,
        success: bool,
        error: &str,
    ) -> zbus::fdo::Result<()>;

    /// Signal: Screen share requested
    #[zbus(signal)]
    fn screen_share_requested(device_id: &str) -> zbus::fdo::Result<()>;

    /// Signal: Screen share started
    #[zbus(signal)]
    fn screen_share_started(device_id: &str, is_sender: bool) -> zbus::fdo::Result<()>;

    /// Signal: Screen share stopped
    #[zbus(signal)]
    fn screen_share_stopped(device_id: &str) -> zbus::fdo::Result<()>;

    /// Signal: Screen share cursor update
    #[zbus(signal)]
    fn screen_share_cursor_update(
        device_id: &str,
        x: i32,
        y: i32,
        visible: bool,
    ) -> zbus::fdo::Result<()>;

    /// Signal: Screen share annotation
    #[zbus(signal)]
    fn screen_share_annotation(
        device_id: &str,
        annotation_type: &str,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        color: &str,
        width: u8,
    ) -> zbus::fdo::Result<()>;
}

/// DBus proxy for COSMIC Connect Open interface (App Continuity)
#[proxy(
    interface = "com.system76.CosmicConnect.Open",
    default_service = "com.system76.CosmicConnect",
    default_path = "/com/system76/CosmicConnect/Open"
)]
trait CConnectOpen {
    /// Open a URL on a connected Android device
    async fn open_on_phone(&self, url: &str) -> zbus::fdo::Result<String>;

    /// Open a file on a connected Android device (transfer + open)
    async fn open_file_on_phone(&self, path: &str, device_id: &str) -> zbus::fdo::Result<String>;

    /// List devices that support opening content
    async fn list_open_capable_devices(&self) -> zbus::fdo::Result<Vec<String>>;
}

/// DBus client for communicating with the daemon
#[derive(Clone, Debug)]
pub struct DbusClient {
    /// DBus connection
    #[allow(dead_code)]
    connection: Connection,
    /// Proxy to daemon interface
    proxy: CConnectProxy<'static>,
    /// Proxy to Open interface (App Continuity)
    open_proxy: CConnectOpenProxy<'static>,
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

        let open_proxy = CConnectOpenProxy::new(&connection)
            .await
            .context("Failed to create Open proxy")?;

        let (event_tx, event_rx) = mpsc::unbounded_channel();

        info!("Connected to daemon successfully");

        Ok((
            Self {
                connection,
                proxy,
                open_proxy,
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
        let mut plugin_state_changed_stream =
            self.proxy.receive_device_plugin_state_changed().await?;
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

        let event_tx = self.event_tx.clone();
        let mut progress_stream = self.proxy.receive_transfer_progress().await?;
        tokio::spawn(async move {
            while let Some(signal) = progress_stream.next().await {
                if let Ok(args) = signal.args() {
                    let _ = event_tx.send(DaemonEvent::TransferProgress {
                        transfer_id: args.transfer_id().to_string(),
                        device_id: args.device_id().to_string(),
                        filename: args.filename().to_string(),
                        current: *args.current(),
                        total: *args.total(),
                        direction: args.direction().to_string(),
                    });
                }
            }
        });

        let event_tx = self.event_tx.clone();
        let mut complete_stream = self.proxy.receive_transfer_complete().await?;
        tokio::spawn(async move {
            while let Some(signal) = complete_stream.next().await {
                if let Ok(args) = signal.args() {
                    let _ = event_tx.send(DaemonEvent::TransferComplete {
                        transfer_id: args.transfer_id().to_string(),
                        device_id: args.device_id().to_string(),
                        filename: args.filename().to_string(),
                        success: *args.success(),
                        error: args.error().to_string(),
                    });
                }
            }
        });

        let event_tx = self.event_tx.clone();
        let mut screen_share_stream = self.proxy.receive_screen_share_requested().await?;
        tokio::spawn(async move {
            while let Some(signal) = screen_share_stream.next().await {
                if let Ok(args) = signal.args() {
                    let device_id = args.device_id().to_string();
                    let _ = event_tx.send(DaemonEvent::ScreenShareRequested { device_id });
                }
            }
        });

        let event_tx = self.event_tx.clone();
        let mut cursor_stream = self.proxy.receive_screen_share_cursor_update().await?;
        tokio::spawn(async move {
            while let Some(signal) = cursor_stream.next().await {
                if let Ok(args) = signal.args() {
                    let _ = event_tx.send(DaemonEvent::ScreenShareCursorUpdate {
                        device_id: args.device_id().to_string(),
                        x: *args.x(),
                        y: *args.y(),
                        visible: *args.visible(),
                    });
                }
            }
        });

        let event_tx = self.event_tx.clone();
        let mut annotation_stream = self.proxy.receive_screen_share_annotation().await?;
        tokio::spawn(async move {
            while let Some(signal) = annotation_stream.next().await {
                if let Ok(args) = signal.args() {
                    let _ = event_tx.send(DaemonEvent::ScreenShareAnnotation {
                        device_id: args.device_id().to_string(),
                        annotation_type: args.annotation_type().to_string(),
                        x1: *args.x1(),
                        y1: *args.y1(),
                        x2: *args.x2(),
                        y2: *args.y2(),
                        color: args.color().to_string(),
                        width: *args.width(),
                    });
                }
            }
        });

        let event_tx = self.event_tx.clone();
        let mut started_stream = self.proxy.receive_screen_share_started().await?;
        tokio::spawn(async move {
            while let Some(signal) = started_stream.next().await {
                if let Ok(args) = signal.args() {
                    let _ = event_tx.send(DaemonEvent::ScreenShareStarted {
                        device_id: args.device_id().to_string(),
                        is_sender: *args.is_sender(),
                    });
                }
            }
        });

        let event_tx = self.event_tx.clone();
        let mut stopped_stream = self.proxy.receive_screen_share_stopped().await?;
        tokio::spawn(async move {
            while let Some(signal) = stopped_stream.next().await {
                if let Ok(args) = signal.args() {
                    let _ = event_tx.send(DaemonEvent::ScreenShareStopped {
                        device_id: args.device_id().to_string(),
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub async fn refresh_discovery(&self) -> Result<()> {
        debug!("Refreshing device discovery");
        self.proxy
            .refresh_discovery()
            .await
            .context("Failed to refresh discovery")
    }

    /// Connect to a device at a specific address
    pub async fn connect_to_address(&self, address: &str) -> Result<()> {
        debug!("Connecting to address: {}", address);
        self.proxy
            .connect_to_address(address)
            .await
            .context("Failed to connect to address")
    }

    /// Get device connection state
    #[allow(dead_code)]
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

    /// Cancel an active file transfer
    pub async fn cancel_transfer(&self, transfer_id: &str) -> Result<()> {
        info!("Cancelling transfer {}", transfer_id);
        self.proxy
            .cancel_transfer(transfer_id)
            .await
            .context("Failed to cancel transfer")
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
    #[allow(dead_code)]
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

    /// Get screen share statistics from a device
    pub async fn get_screen_share_stats(&self, device_id: &str) -> Result<ScreenShareStats> {
        debug!("Getting screen share stats for device {}", device_id);
        self.proxy
            .get_screen_share_stats(device_id)
            .await
            .context("Failed to get screen share stats")
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

    /// Get detailed state for a specific MPRIS player
    pub async fn get_player_state(&self, player: &str) -> Result<PlayerState> {
        debug!("Getting player state for {}", player);
        let json = self
            .proxy
            .get_player_state(player)
            .await
            .context("Failed to get player state")?;

        serde_json::from_str(&json).context("Failed to parse player state")
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
        info!(
            "Seeking MPRIS player {} by {}μs",
            player, offset_microseconds
        );
        self.proxy
            .mpris_seek(player, offset_microseconds)
            .await
            .context("Failed to seek MPRIS player")
    }

    /// Raise MPRIS player window (bring to front)
    ///
    /// # Arguments
    /// * `player` - Player name
    pub async fn mpris_raise(&self, player: &str) -> Result<()> {
        info!("Raising MPRIS player window: {}", player);
        self.proxy
            .mpris_raise(player)
            .await
            .context("Failed to raise MPRIS player")
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
        info!(
            "Clearing plugin override for {} on device {}",
            plugin, device_id
        );
        self.proxy
            .clear_device_plugin_override(device_id, plugin)
            .await
            .context("Failed to clear device plugin override")
    }

    /// Reset all plugin overrides for a device (revert all to global config)
    ///
    /// # Arguments
    /// * `device_id` - Device ID
    pub async fn reset_all_plugin_overrides(&self, device_id: &str) -> Result<()> {
        info!("Resetting all plugin overrides for device {}", device_id);
        self.proxy
            .reset_all_plugin_overrides(device_id)
            .await
            .context("Failed to reset all plugin overrides")
    }

    /// Get RemoteDesktop settings for a device
    ///
    /// # Arguments
    /// * `device_id` - Device ID
    pub async fn get_remotedesktop_settings(
        &self,
        device_id: &str,
    ) -> Result<RemoteDesktopSettings> {
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

    /// Set a custom nickname for a device
    pub async fn set_device_nickname(&self, device_id: &str, nickname: &str) -> Result<()> {
        info!("Setting nickname for {}: '{}'", device_id, nickname);
        self.proxy
            .set_device_nickname(device_id, nickname)
            .await
            .context("Failed to set device nickname")
    }

    /// Set notification preference for a device
    pub async fn set_device_notification_preference(
        &self,
        device_id: &str,
        preference: NotificationPreference,
    ) -> Result<()> {
        let pref_str = match preference {
            NotificationPreference::All => "all",
            NotificationPreference::Important => "important",
            NotificationPreference::None => "none",
        };
        info!(
            "Setting notification preference for {}: '{}'",
            device_id, pref_str
        );
        self.proxy
            .set_device_notification_preference(device_id, pref_str)
            .await
            .context("Failed to set notification preference")
    }

    /// Check if daemon is available
    #[allow(dead_code)]
    pub async fn is_daemon_available(&self) -> bool {
        // Try to list devices as a health check
        self.proxy.list_devices().await.is_ok()
    }

    /// Add a folder to sync with a device
    pub async fn add_sync_folder(
        &self,
        device_id: String,
        folder_id: String,
        path: String,
        strategy: String,
    ) -> Result<()> {
        self.proxy
            .add_sync_folder(device_id, folder_id, path, strategy)
            .await
            .context("Failed to call add_sync_folder")
    }

    /// Remove a sync folder from a device
    pub async fn remove_sync_folder(&self, device_id: String, folder_id: String) -> Result<()> {
        self.proxy
            .remove_sync_folder(device_id, folder_id)
            .await
            .context("Failed to call remove_sync_folder")
    }

    /// Get list of synced folders for a device
    pub async fn get_sync_folders(&self, device_id: String) -> Result<Vec<SyncFolderInfo>> {
        self.proxy
            .get_sync_folders(device_id)
            .await
            .context("Failed to call get_sync_folders")
    }

    /// Add a run command
    pub async fn add_run_command(
        &self,
        device_id: String,
        command_id: String,
        name: String,
        command: String,
    ) -> Result<()> {
        self.proxy
            .add_run_command(device_id, command_id, name, command)
            .await
            .context("Failed to call add_run_command")
    }

    /// Remove a run command
    pub async fn remove_run_command(&self, device_id: String, command_id: String) -> Result<()> {
        self.proxy
            .remove_run_command(device_id, command_id)
            .await
            .context("Failed to call remove_run_command")
    }

    /// Start screen share
    #[allow(dead_code)]
    pub async fn start_screen_share(&self, device_id: &str, port: u16) -> Result<()> {
        self.proxy
            .start_screen_share(device_id, port)
            .await
            .context("Failed to call start_screen_share")
    }

    /// Stop screen share session
    pub async fn stop_screen_share(&self, device_id: &str) -> Result<()> {
        self.proxy
            .stop_screen_share(device_id)
            .await
            .context("Failed to call stop_screen_share")
    }

    /// Pause screen share session
    pub async fn pause_screen_share(&self, device_id: &str) -> Result<()> {
        self.proxy
            .pause_screen_share(device_id)
            .await
            .context("Failed to call pause_screen_share")
    }

    /// Resume screen share session
    pub async fn resume_screen_share(&self, device_id: &str) -> Result<()> {
        self.proxy
            .resume_screen_share(device_id)
            .await
            .context("Failed to call resume_screen_share")
    }

    /// Send screen mirror input
    #[allow(dead_code)]
    pub async fn send_mirror_input(
        &self,
        device_id: String,
        x: f32,
        y: f32,
        action: String,
    ) -> Result<()> {
        self.proxy
            .send_mirror_input(device_id, x, y, action)
            .await
            .context("Failed to send mirror input")
    }

    /// Get run commands
    pub async fn get_run_commands(&self, device_id: String) -> Result<HashMap<String, RunCommand>> {
        let json = self
            .proxy
            .get_run_commands(device_id)
            .await
            .context("Failed to call get_run_commands")?;

        serde_json::from_str(&json).context("Failed to parse run commands JSON")
    }

    /// Open a URL on a connected Android device (App Continuity)
    ///
    /// # Arguments
    /// * `url` - The URL to open (http, https, tel, mailto, etc.)
    ///
    /// # Returns
    /// Request ID for tracking
    pub async fn open_on_phone(&self, url: &str) -> Result<String> {
        info!("Opening URL on phone: {}", url);
        self.open_proxy
            .open_on_phone(url)
            .await
            .context("Failed to open URL on phone")
    }

    /// List devices that support opening content
    ///
    /// # Returns
    /// List of device IDs that are paired, reachable, and have share capability
    pub async fn list_open_capable_devices(&self) -> Result<Vec<String>> {
        debug!("Listing open-capable devices");
        self.open_proxy
            .list_open_capable_devices()
            .await
            .context("Failed to list open-capable devices")
    }

    /// Open a file on a connected Android device (transfer + open)
    ///
    /// # Arguments
    /// * `path` - Absolute path to the file
    /// * `device_id` - Target device ID (empty for default device)
    ///
    /// # Returns
    /// Transfer ID for tracking
    #[allow(dead_code)]
    pub async fn open_file_on_phone(&self, path: &str, device_id: &str) -> Result<String> {
        info!("Opening file on phone: {} -> {}", path, device_id);
        self.open_proxy
            .open_file_on_phone(path, device_id)
            .await
            .context("Failed to open file on phone")
    }
}

/// Auto-reconnecting DBus client wrapper
pub struct ReconnectingClient {
    /// Current client (None if disconnected)
    #[allow(dead_code)]
    client: Option<DbusClient>,
    /// Event receiver
    event_rx: mpsc::UnboundedReceiver<DaemonEvent>,
    /// Event sender for reconnection
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub fn client(&self) -> Option<&DbusClient> {
        self.client.as_ref()
    }

    /// Attempt to reconnect to the daemon
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub fn try_recv_event(&mut self) -> Result<DaemonEvent, mpsc::error::TryRecvError> {
        self.event_rx.try_recv()
    }
}
