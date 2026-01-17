mod dbus_client;

use std::collections::HashMap;

use cosmic::{
    app::{Core, Task},
    iced::{
        alignment::Horizontal,
        widget::{column, container, row, scrollable, text},
        window, Length, Padding, Rectangle,
    },
    iced_runtime::core::layout::Limits,
    surface::action::{app_popup, destroy_popup},
    widget::{button, divider, icon},
    Element,
};
use cosmic_connect_protocol::{ConnectionState, Device, DeviceInfo as ProtocolDeviceInfo, DeviceType, PairingStatus};

use dbus_client::DbusClient;

fn main() -> cosmic::iced::Result {
    // Initialize logging with environment variable support
    // Set RUST_LOG=debug for verbose output, defaults to info level
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .compact()
        .init();

    tracing::info!("COSMIC Connect applet starting");

    cosmic::applet::run::<CConnectApplet>(())
}

/// Plugin metadata for UI display
struct PluginMetadata {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    icon: &'static str,
    capability: &'static str,
}

/// Available plugins with their metadata
const PLUGINS: &[PluginMetadata] = &[
    PluginMetadata {
        id: "ping",
        name: "Ping",
        description: "Send and receive pings",
        icon: "user-available-symbolic",
        capability: "cconnect.ping",
    },
    PluginMetadata {
        id: "battery",
        name: "Battery Monitor",
        description: "Share battery status",
        icon: "battery-symbolic",
        capability: "cconnect.battery",
    },
    PluginMetadata {
        id: "notification",
        name: "Notifications",
        description: "Sync notifications",
        icon: "notification-symbolic",
        capability: "cconnect.notification",
    },
    PluginMetadata {
        id: "share",
        name: "File Sharing",
        description: "Send and receive files",
        icon: "document-send-symbolic",
        capability: "cconnect.share",
    },
    PluginMetadata {
        id: "clipboard",
        name: "Clipboard Sync",
        description: "Share clipboard content",
        icon: "edit-paste-symbolic",
        capability: "cconnect.clipboard",
    },
    PluginMetadata {
        id: "mpris",
        name: "Media Control",
        description: "Control media players",
        icon: "multimedia-player-symbolic",
        capability: "cconnect.mpris",
    },
    PluginMetadata {
        id: "remotedesktop",
        name: "Remote Desktop",
        description: "VNC screen sharing",
        icon: "preferences-desktop-remote-desktop-symbolic",
        capability: "cconnect.remotedesktop",
    },
    PluginMetadata {
        id: "findmyphone",
        name: "Find My Phone",
        description: "Ring device remotely",
        icon: "find-location-symbolic",
        capability: "cconnect.findmyphone",
    },
];

#[derive(Debug, Clone)]
struct DeviceState {
    device: Device,
    battery_level: Option<u8>,
    is_charging: bool,
}

struct CConnectApplet {
    core: Core,
    popup: Option<window::Id>,
    devices: Vec<DeviceState>,
    dbus_client: Option<DbusClient>,
    mpris_players: Vec<String>,
    selected_player: Option<String>,
    // Settings UI state
    expanded_device_settings: Option<String>,                  // Currently expanded device_id
    device_configs: HashMap<String, dbus_client::DeviceConfig>, // Device-specific configs
    // RemoteDesktop settings UI state
    remotedesktop_settings_device: Option<String>,             // device_id showing RemoteDesktop settings
    remotedesktop_settings: HashMap<String, dbus_client::RemoteDesktopSettings>, // In-progress settings
}

#[derive(Debug, Clone)]
enum Message {
    PopupClosed(window::Id),
    PopupOpened,
    PairDevice(String),
    UnpairDevice(String),
    RefreshDevices,
    SendPing(String),
    SendFile(String),
    FileSelected(String, String), // device_id, file_path
    FindPhone(String),
    ShareText(String),            // device_id
    ShareUrl(String),             // device_id
    RequestBatteryUpdate(String), // device_id
    Surface(cosmic::surface::Action),
    // Daemon responses
    DeviceListUpdated(HashMap<String, dbus_client::DeviceInfo>),
    BatteryStatusesUpdated(HashMap<String, dbus_client::BatteryStatus>),
    // MPRIS control
    MprisPlayersUpdated(Vec<String>),
    MprisPlayerSelected(String),
    MprisControl(String, String), // player, action
    MprisSetVolume(String, f64),  // player, volume
    MprisSeek(String, i64),       // player, offset_microseconds
    // Settings UI
    ToggleDeviceSettings(String),                    // device_id
    SetDevicePluginEnabled(String, String, bool),   // device_id, plugin, enabled
    ClearDevicePluginOverride(String, String),      // device_id, plugin
    ResetAllPluginOverrides(String),                // device_id
    DeviceConfigLoaded(String, dbus_client::DeviceConfig), // device_id, config
    // RemoteDesktop settings
    ShowRemoteDesktopSettings(String),              // device_id
    CloseRemoteDesktopSettings,
    UpdateRemoteDesktopQuality(String, String),      // device_id, quality
    UpdateRemoteDesktopFps(String, u8),              // device_id, fps
    UpdateRemoteDesktopResolution(String, String),   // device_id, mode ("native" or "custom")
    UpdateRemoteDesktopCustomWidth(String, String),  // device_id, width_str
    UpdateRemoteDesktopCustomHeight(String, String), // device_id, height_str
    SaveRemoteDesktopSettings(String),               // device_id
    RemoteDesktopSettingsLoaded(String, dbus_client::RemoteDesktopSettings), // device_id, settings
}

/// Fetches device list from the daemon via D-Bus
async fn fetch_devices() -> HashMap<String, dbus_client::DeviceInfo> {
    match DbusClient::connect().await {
        Ok((client, _)) => match client.list_devices().await {
            Ok(devices) => {
                tracing::info!("Fetched {} devices from daemon", devices.len());
                devices
            }
            Err(e) => {
                tracing::error!("Failed to list devices: {}", e);
                HashMap::new()
            }
        },
        Err(e) => {
            tracing::warn!("Failed to connect to daemon: {}", e);
            HashMap::new()
        }
    }
}

/// Executes a device operation via D-Bus and logs any errors
async fn execute_device_operation<F, Fut>(device_id: String, operation_name: &str, operation: F)
where
    F: FnOnce(DbusClient, String) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    match DbusClient::connect().await {
        Ok((client, _)) => {
            if let Err(e) = operation(client, device_id.clone()).await {
                tracing::error!("Failed to {} device {}: {}", operation_name, device_id, e);
            }
        }
        Err(e) => {
            tracing::warn!("Failed to connect to daemon: {}", e);
        }
    }
}

/// Creates a task that fetches devices and returns DeviceListUpdated message
fn fetch_devices_task() -> Task<Message> {
    Task::perform(fetch_devices(), |devices| {
        cosmic::Action::App(Message::DeviceListUpdated(devices))
    })
}

/// Fetches battery status for a list of device IDs
async fn fetch_battery_statuses(device_ids: Vec<String>) -> HashMap<String, dbus_client::BatteryStatus> {
    let mut statuses = HashMap::new();
    let Ok((client, _)) = DbusClient::connect().await else {
        return statuses;
    };
    for device_id in device_ids {
        if let Ok(status) = client.get_battery_status(&device_id).await {
            statuses.insert(device_id, status);
        }
    }
    statuses
}

/// Fetches list of available MPRIS media players
async fn fetch_mpris_players() -> Vec<String> {
    let Ok((client, _)) = DbusClient::connect().await else {
        tracing::warn!("Failed to connect to daemon for MPRIS players");
        return Vec::new();
    };

    match client.get_mpris_players().await {
        Ok(players) => {
            tracing::info!("Fetched {} MPRIS players", players.len());
            players
        }
        Err(e) => {
            tracing::error!("Failed to get MPRIS players: {}", e);
            Vec::new()
        }
    }
}

/// Opens a file picker dialog and returns device_id and selected file path
async fn open_file_picker(device_id: String) -> Option<(String, String)> {
    use ashpd::desktop::file_chooser::OpenFileRequest;

    let response = OpenFileRequest::default()
        .title("Select file to send")
        .modal(true)
        .multiple(false)
        .send()
        .await
        .ok()?
        .response()
        .ok()?;

    response
        .uris()
        .first()
        .map(|uri| (device_id, uri.path().to_string()))
}

/// Gets text from the system clipboard
fn get_clipboard_text() -> Option<String> {
    arboard::Clipboard::new()
        .ok()
        .and_then(|mut clipboard| clipboard.get_text().ok())
}

/// Creates a task that executes a device operation then refreshes the device list
fn device_operation_task<F, Fut>(
    device_id: String,
    operation_name: &'static str,
    operation: F,
) -> Task<Message>
where
    F: FnOnce(DbusClient, String) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
{
    Task::perform(
        async move { execute_device_operation(device_id, operation_name, operation).await },
        |_| cosmic::Action::App(Message::RefreshDevices),
    )
}

/// Converts a D-Bus DeviceInfo to our internal DeviceState
fn convert_device_info(info: &dbus_client::DeviceInfo) -> DeviceState {
    let device_type = match info.device_type.as_str() {
        "phone" => DeviceType::Phone,
        "tablet" => DeviceType::Tablet,
        "laptop" => DeviceType::Laptop,
        "tv" => DeviceType::Tv,
        _ => DeviceType::Desktop,
    };

    let connection_state = if info.is_connected {
        ConnectionState::Connected
    } else {
        ConnectionState::Disconnected
    };

    let pairing_status = if info.is_paired {
        PairingStatus::Paired
    } else {
        PairingStatus::Unpaired
    };

    let mut protocol_info = ProtocolDeviceInfo::new(&info.name, device_type, 1716);
    protocol_info.device_id = info.id.clone();

    let device = Device {
        info: protocol_info,
        connection_state,
        pairing_status,
        is_trusted: info.is_paired,
        last_seen: info.last_seen as u64,
        last_connected: if info.is_connected { Some(info.last_seen as u64) } else { None },
        host: None,
        port: None,
        certificate_fingerprint: None,
        certificate_data: None,
    };

    DeviceState {
        device,
        battery_level: None,
        is_charging: false,
    }
}

impl cosmic::Application for CConnectApplet {
    type Message = Message;
    type Executor = cosmic::executor::multi::Executor;
    type Flags = ();
    const APP_ID: &'static str = "com.system76.CosmicAppletConnect";

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        let app = Self {
            core,
            popup: None,
            devices: Vec::new(),
            dbus_client: None,
            mpris_players: Vec::new(),
            selected_player: None,
            expanded_device_settings: None,
            device_configs: HashMap::new(),
            remotedesktop_settings_device: None,
            remotedesktop_settings: HashMap::new(),
        };
        (app, Task::none())
    }

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                }
                Task::none()
            }
            Message::PopupOpened => {
                tracing::info!("Popup opened, fetching devices and MPRIS players");
                Task::batch(vec![
                    fetch_devices_task(),
                    Task::perform(fetch_mpris_players(), |players| {
                        cosmic::Action::App(Message::MprisPlayersUpdated(players))
                    }),
                ])
            }
            Message::DeviceListUpdated(devices) => {
                tracing::info!("Device list updated: {} devices", devices.len());

                self.devices = devices.values().map(convert_device_info).collect();

                let connected_ids: Vec<String> = self
                    .devices
                    .iter()
                    .filter(|d| d.device.is_connected())
                    .map(|d| d.device.info.device_id.clone())
                    .collect();

                if connected_ids.is_empty() {
                    return Task::none();
                }

                tracing::debug!("Fetching battery status for {} connected devices", connected_ids.len());
                Task::perform(fetch_battery_statuses(connected_ids), |statuses| {
                    cosmic::Action::App(Message::BatteryStatusesUpdated(statuses))
                })
            }
            Message::BatteryStatusesUpdated(statuses) => {
                tracing::debug!("Battery statuses updated for {} devices", statuses.len());

                for device_state in &mut self.devices {
                    if let Some(status) = statuses.get(&device_state.device.info.device_id) {
                        device_state.battery_level = Some((status.level as u8).min(100));
                        device_state.is_charging = status.is_charging;
                    }
                }

                Task::none()
            }
            Message::PairDevice(device_id) => {
                tracing::info!("Pairing device: {}", device_id);
                device_operation_task(device_id, "pair", |client, id| async move {
                    client.pair_device(&id).await
                })
            }
            Message::UnpairDevice(device_id) => {
                tracing::info!("Unpairing device: {}", device_id);
                device_operation_task(device_id, "unpair", |client, id| async move {
                    client.unpair_device(&id).await
                })
            }
            Message::RefreshDevices => {
                tracing::info!("Refreshing device list");
                fetch_devices_task()
            }
            Message::SendPing(device_id) => {
                tracing::info!("Sending ping to device: {}", device_id);
                device_operation_task(device_id, "ping", |client, id| async move {
                    client.send_ping(&id, "Ping from COSMIC").await
                })
            }
            Message::SendFile(device_id) => {
                tracing::info!("Opening file picker for device: {}", device_id);
                Task::perform(open_file_picker(device_id), |result| {
                    match result {
                        Some((device_id, path)) => {
                            cosmic::Action::App(Message::FileSelected(device_id, path))
                        }
                        None => {
                            tracing::debug!("File picker cancelled or no file selected");
                            cosmic::Action::App(Message::RefreshDevices)
                        }
                    }
                })
            }
            Message::FileSelected(device_id, file_path) => {
                tracing::info!("Sending file {} to device: {}", file_path, device_id);
                device_operation_task(device_id, "share file", move |client, id| async move {
                    client.share_file(&id, &file_path).await
                })
            }
            Message::FindPhone(device_id) => {
                tracing::info!("Finding phone: {}", device_id);
                device_operation_task(device_id, "find phone", |client, id| async move {
                    client.find_phone(&id).await
                })
            }
            Message::ShareText(device_id) => {
                tracing::info!("Share text to device: {}", device_id);
                match get_clipboard_text() {
                    Some(text) => device_operation_task(device_id, "share text", move |client, id| {
                        async move { client.share_text(&id, &text).await }
                    }),
                    None => {
                        tracing::warn!("No text in clipboard to share");
                        Task::none()
                    }
                }
            }
            Message::ShareUrl(device_id) => {
                tracing::info!("Share URL to device: {}", device_id);
                match get_clipboard_text() {
                    Some(text)
                        if text.starts_with("http://")
                            || text.starts_with("https://")
                            || text.starts_with("www.") =>
                    {
                        device_operation_task(device_id, "share URL", move |client, id| {
                            async move { client.share_url(&id, &text).await }
                        })
                    }
                    Some(_) => {
                        tracing::warn!("Clipboard text is not a valid URL");
                        Task::none()
                    }
                    None => {
                        tracing::warn!("No text in clipboard to share as URL");
                        Task::none()
                    }
                }
            }
            Message::RequestBatteryUpdate(device_id) => {
                tracing::info!("Requesting battery update for device: {}", device_id);
                device_operation_task(device_id, "request battery", |client, id| async move {
                    client.request_battery_update(&id).await
                })
            }
            Message::MprisPlayersUpdated(players) => {
                tracing::info!("MPRIS players updated: {} players", players.len());
                self.mpris_players = players;
                // Auto-select first player if none selected
                if self.selected_player.is_none() && !self.mpris_players.is_empty() {
                    self.selected_player = Some(self.mpris_players[0].clone());
                }
                Task::none()
            }
            Message::MprisPlayerSelected(player) => {
                tracing::info!("MPRIS player selected: {}", player);
                self.selected_player = Some(player);
                Task::none()
            }
            Message::MprisControl(player, action) => {
                tracing::info!("MPRIS control: {} on {}", action, player);
                Task::perform(
                    async move {
                        if let Ok((client, _)) = DbusClient::connect().await {
                            if let Err(e) = client.mpris_control(&player, &action).await {
                                tracing::error!("Failed to control MPRIS player: {}", e);
                            }
                        }
                    },
                    |_| cosmic::Action::None,
                )
            }
            Message::MprisSetVolume(player, volume) => {
                tracing::info!("MPRIS set volume: {} to {}", player, volume);
                Task::perform(
                    async move {
                        if let Ok((client, _)) = DbusClient::connect().await {
                            if let Err(e) = client.mpris_set_volume(&player, volume).await {
                                tracing::error!("Failed to set MPRIS volume: {}", e);
                            }
                        }
                    },
                    |_| cosmic::Action::None,
                )
            }
            Message::MprisSeek(player, offset) => {
                tracing::info!("MPRIS seek: {} by {}μs", player, offset);
                Task::perform(
                    async move {
                        if let Ok((client, _)) = DbusClient::connect().await {
                            if let Err(e) = client.mpris_seek(&player, offset).await {
                                tracing::error!("Failed to seek MPRIS player: {}", e);
                            }
                        }
                    },
                    |_| cosmic::Action::None,
                )
            }
            Message::ToggleDeviceSettings(device_id) => {
                if self.expanded_device_settings.as_ref() == Some(&device_id) {
                    // Collapse
                    self.expanded_device_settings = None;
                    Task::none()
                } else {
                    // Expand - fetch config first
                    self.expanded_device_settings = Some(device_id.clone());
                    let device_id_for_async = device_id.clone();
                    let device_id_for_msg = std::sync::Arc::new(device_id.clone());
                    Task::perform(
                        async move {
                            match DbusClient::connect().await {
                                Ok((client, _)) => client.get_device_config(&device_id_for_async).await,
                                Err(e) => {
                                    tracing::error!("Failed to connect to daemon: {}", e);
                                    Err(e)
                                }
                            }
                        },
                        move |result| {
                            let device_id = (*device_id_for_msg).clone();
                            match result {
                                Ok(config) => cosmic::Action::App(Message::DeviceConfigLoaded(device_id, config)),
                                Err(e) => {
                                    tracing::error!("Failed to load device config: {}", e);
                                    cosmic::Action::App(Message::RefreshDevices)
                                }
                            }
                        },
                    )
                }
            }
            Message::SetDevicePluginEnabled(device_id, plugin, enabled) => {
                tracing::info!("Setting plugin {} to {} for device {}", plugin, if enabled { "enabled" } else { "disabled" }, device_id);
                let device_id_for_async = device_id.clone();
                let device_id_for_msg = std::sync::Arc::new(device_id.clone());
                device_operation_task(device_id, "set plugin enabled", move |client, id| {
                    let plugin_clone = plugin.clone();
                    async move {
                        client.set_device_plugin_enabled(&id, &plugin_clone, enabled).await
                    }
                })
                .chain(Task::perform(
                    async move {
                        match DbusClient::connect().await {
                            Ok((client, _)) => client.get_device_config(&device_id_for_async).await,
                            Err(e) => Err(e),
                        }
                    },
                    move |result| {
                        let device_id = (*device_id_for_msg).clone();
                        match result {
                            Ok(config) => cosmic::Action::App(Message::DeviceConfigLoaded(device_id, config)),
                            Err(_) => cosmic::Action::App(Message::RefreshDevices),
                        }
                    },
                ))
            }
            Message::ClearDevicePluginOverride(device_id, plugin) => {
                tracing::info!("Clearing plugin override for {} on device {}", plugin, device_id);
                let device_id_for_async = device_id.clone();
                let device_id_for_msg = std::sync::Arc::new(device_id.clone());
                device_operation_task(device_id, "clear plugin override", move |client, id| {
                    let plugin_clone = plugin.clone();
                    async move {
                        client.clear_device_plugin_override(&id, &plugin_clone).await
                    }
                })
                .chain(Task::perform(
                    async move {
                        match DbusClient::connect().await {
                            Ok((client, _)) => client.get_device_config(&device_id_for_async).await,
                            Err(e) => Err(e),
                        }
                    },
                    move |result| {
                        let device_id = (*device_id_for_msg).clone();
                        match result {
                            Ok(config) => cosmic::Action::App(Message::DeviceConfigLoaded(device_id, config)),
                            Err(_) => cosmic::Action::App(Message::RefreshDevices),
                        }
                    },
                ))
            }
            Message::ResetAllPluginOverrides(device_id) => {
                tracing::info!("Resetting all plugin overrides for device {}", device_id);
                let device_id_for_async = device_id.clone();
                let device_id_for_msg = std::sync::Arc::new(device_id.clone());
                device_operation_task(device_id, "reset all plugin overrides", move |client, id| {
                    async move {
                        client.reset_all_plugin_overrides(&id).await
                    }
                })
                .chain(Task::perform(
                    async move {
                        match DbusClient::connect().await {
                            Ok((client, _)) => client.get_device_config(&device_id_for_async).await,
                            Err(e) => Err(e),
                        }
                    },
                    move |result| {
                        let device_id = (*device_id_for_msg).clone();
                        match result {
                            Ok(config) => cosmic::Action::App(Message::DeviceConfigLoaded(device_id, config)),
                            Err(_) => cosmic::Action::App(Message::RefreshDevices),
                        }
                    },
                ))
            }
            Message::DeviceConfigLoaded(device_id, config) => {
                tracing::debug!("Device config loaded for {}", device_id);
                self.device_configs.insert(device_id, config);
                Task::none()
            }
            // RemoteDesktop settings handlers
            Message::ShowRemoteDesktopSettings(device_id) => {
                tracing::debug!("Showing RemoteDesktop settings for {}", device_id);
                self.remotedesktop_settings_device = Some(device_id.clone());

                let device_id_for_async = device_id.clone();
                let device_id_for_msg = std::sync::Arc::new(device_id.clone());

                // Fetch current settings
                Task::perform(
                    async move {
                        match DbusClient::connect().await {
                            Ok((client, _)) => client.get_remotedesktop_settings(&device_id_for_async).await,
                            Err(e) => Err(e),
                        }
                    },
                    move |result| {
                        let device_id = (*device_id_for_msg).clone();
                        match result {
                            Ok(settings) => cosmic::Action::App(Message::RemoteDesktopSettingsLoaded(device_id, settings)),
                            Err(e) => {
                                tracing::error!("Failed to load RemoteDesktop settings: {}", e);
                                cosmic::Action::App(Message::RefreshDevices)
                            }
                        }
                    },
                )
            }
            Message::CloseRemoteDesktopSettings => {
                tracing::debug!("Closing RemoteDesktop settings");
                self.remotedesktop_settings_device = None;
                Task::none()
            }
            Message::RemoteDesktopSettingsLoaded(device_id, settings) => {
                tracing::debug!("RemoteDesktop settings loaded for {}", device_id);
                self.remotedesktop_settings.insert(device_id, settings);
                Task::none()
            }
            Message::UpdateRemoteDesktopQuality(device_id, quality) => {
                if let Some(settings) = self.remotedesktop_settings.get_mut(&device_id) {
                    settings.quality = quality;
                }
                Task::none()
            }
            Message::UpdateRemoteDesktopFps(device_id, fps) => {
                if let Some(settings) = self.remotedesktop_settings.get_mut(&device_id) {
                    settings.fps = fps;
                }
                Task::none()
            }
            Message::UpdateRemoteDesktopResolution(device_id, mode) => {
                if let Some(settings) = self.remotedesktop_settings.get_mut(&device_id) {
                    settings.resolution_mode = mode;
                }
                Task::none()
            }
            Message::UpdateRemoteDesktopCustomWidth(device_id, width_str) => {
                if let Some(settings) = self.remotedesktop_settings.get_mut(&device_id) {
                    settings.custom_width = width_str.parse().ok();
                }
                Task::none()
            }
            Message::UpdateRemoteDesktopCustomHeight(device_id, height_str) => {
                if let Some(settings) = self.remotedesktop_settings.get_mut(&device_id) {
                    settings.custom_height = height_str.parse().ok();
                }
                Task::none()
            }
            Message::SaveRemoteDesktopSettings(device_id) => {
                tracing::info!("Saving RemoteDesktop settings for {}", device_id);

                if let Some(settings) = self.remotedesktop_settings.get(&device_id).cloned() {
                    Task::perform(
                        async move {
                            match DbusClient::connect().await {
                                Ok((client, _)) => {
                                    client.set_remotedesktop_settings(&device_id, &settings).await
                                }
                                Err(e) => Err(e),
                            }
                        },
                        move |result| match result {
                            Ok(_) => {
                                tracing::info!("RemoteDesktop settings saved successfully");
                                cosmic::Action::App(Message::CloseRemoteDesktopSettings)
                            }
                            Err(e) => {
                                tracing::error!("Failed to save RemoteDesktop settings: {}", e);
                                cosmic::Action::App(Message::RefreshDevices)
                            }
                        },
                    )
                } else {
                    Task::none()
                }
            }
            Message::Surface(action) => {
                cosmic::task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(action)))
            }
        }
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let have_popup = self.popup;

        let btn = self
            .core
            .applet
            .icon_button("phone-symbolic")
            .on_press_with_rectangle(move |offset, bounds| {
                if let Some(id) = have_popup {
                    Message::Surface(destroy_popup(id))
                } else {
                    Message::Surface(app_popup::<CConnectApplet>(
                        move |state: &mut CConnectApplet| {
                            let new_id = window::Id::unique();
                            state.popup = Some(new_id);

                            let mut popup_settings = state.core.applet.get_popup_settings(
                                state.core.main_window_id().unwrap(),
                                new_id,
                                None,
                                None,
                                None,
                            );

                            popup_settings.positioner.size_limits = Limits::NONE
                                .min_width(300.0)
                                .max_width(400.0)
                                .min_height(200.0)
                                .max_height(600.0);

                            popup_settings.positioner.anchor_rect = Rectangle {
                                x: (bounds.x - offset.x) as i32,
                                y: (bounds.y - offset.y) as i32,
                                width: bounds.width as i32,
                                height: bounds.height as i32,
                            };

                            popup_settings
                        },
                        Some(Box::new(|state: &CConnectApplet| {
                            let content = state.popup_view();
                            Element::from(state.core.applet.popup_container(content))
                                .map(cosmic::Action::App)
                        })),
                    ))
                }
            });

        Element::from(self.core.applet.applet_tooltip::<Message>(
            btn,
            "CConnect",
            self.popup.is_some(),
            |a| Message::Surface(a),
            None,
        ))
    }

    fn view_window(&self, _id: window::Id) -> Element<'_, Self::Message> {
        text("CConnect").into()
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}

impl CConnectApplet {
    fn popup_view(&self) -> Element<'_, Message> {
        let header = row![
            text("CConnect").size(18),
            button::icon(icon::from_name("view-refresh-symbolic"))
                .on_press(Message::RefreshDevices)
                .padding(4)
        ]
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Center)
        .width(Length::Fill);

        // MPRIS media controls section
        let mpris_section = self.mpris_controls_view();

        let content = if self.devices.is_empty() {
            column![
                text("No devices found").size(14),
                text("Make sure CConnect is installed on your devices").size(12),
            ]
            .spacing(4)
            .padding(16)
            .width(Length::Fill)
        } else {
            // Group devices by category
            let mut connected = Vec::new();
            let mut available = Vec::new();
            let mut offline = Vec::new();

            for device_state in &self.devices {
                match categorize_device(device_state) {
                    DeviceCategory::Connected => connected.push(device_state),
                    DeviceCategory::Available => available.push(device_state),
                    DeviceCategory::Offline => offline.push(device_state),
                }
            }

            let mut device_groups = column![].spacing(0).width(Length::Fill);

            // Connected devices section
            if !connected.is_empty() {
                device_groups = device_groups.push(
                    container(text("Connected").size(12))
                        .padding(Padding::from([8.0, 12.0, 4.0, 12.0]))
                        .width(Length::Fill)
                );
                for device_state in &connected {
                    device_groups = device_groups.push(self.device_row(device_state));
                }
            }

            // Available devices section
            if !available.is_empty() {
                if !connected.is_empty() {
                    device_groups = device_groups.push(divider::horizontal::default());
                }
                device_groups = device_groups.push(
                    container(text("Available").size(12))
                        .padding(Padding::from([8.0, 12.0, 4.0, 12.0]))
                        .width(Length::Fill)
                );
                for device_state in &available {
                    device_groups = device_groups.push(self.device_row(device_state));
                }
            }

            // Offline devices section
            if !offline.is_empty() {
                if !connected.is_empty() || !available.is_empty() {
                    device_groups = device_groups.push(divider::horizontal::default());
                }
                device_groups = device_groups.push(
                    container(text("Offline").size(12))
                        .padding(Padding::from([8.0, 12.0, 4.0, 12.0]))
                        .width(Length::Fill)
                );
                for device_state in &offline {
                    device_groups = device_groups.push(self.device_row(device_state));
                }
            }

            device_groups
        };

        let popup_content = column![
            container(header)
                .padding(Padding::from([8.0, 12.0]))
                .width(Length::Fill),
            divider::horizontal::default(),
            mpris_section,
            divider::horizontal::default(),
            scrollable(content).height(Length::Fill),
        ]
        .width(Length::Fill);

        container(popup_content)
            .padding(0)
            .width(Length::Fill)
            .height(Length::Shrink)
            .into()
    }

    fn mpris_controls_view(&self) -> Element<'_, Message> {
        // If no players available, return empty space
        if self.mpris_players.is_empty() {
            return Element::from(cosmic::iced::widget::Space::new(0, 0));
        }

        let Some(selected_player) = &self.selected_player else {
            return Element::from(cosmic::iced::widget::Space::new(0, 0));
        };

        // Player name/label
        let player_name = row![
            icon::from_name("multimedia-player-symbolic").size(16),
            text(selected_player).size(13),
        ]
        .spacing(6)
        .align_y(cosmic::iced::Alignment::Center);

        // Playback controls
        let controls = row![
            button::icon(icon::from_name("media-skip-backward-symbolic").size(16))
                .on_press(Message::MprisControl(
                    selected_player.clone(),
                    "Previous".to_string()
                ))
                .padding(6),
            button::icon(icon::from_name("media-playback-start-symbolic").size(16))
                .on_press(Message::MprisControl(
                    selected_player.clone(),
                    "PlayPause".to_string()
                ))
                .padding(6),
            button::icon(icon::from_name("media-playback-stop-symbolic").size(16))
                .on_press(Message::MprisControl(selected_player.clone(), "Stop".to_string()))
                .padding(6),
            button::icon(icon::from_name("media-skip-forward-symbolic").size(16))
                .on_press(Message::MprisControl(selected_player.clone(), "Next".to_string()))
                .padding(6),
        ]
        .spacing(4)
        .align_y(cosmic::iced::Alignment::Center);

        let content = column![player_name, controls]
            .spacing(8)
            .padding(Padding::from([8.0, 12.0]));

        container(content).width(Length::Fill).into()
    }

    fn device_row<'a>(&self, device_state: &'a DeviceState) -> Element<'a, Message> {
        let device = &device_state.device;
        let device_id = &device.info.device_id;

        let device_icon = device_type_icon(device.info.device_type);
        let status_icon = connection_status_icon(device.connection_state, device.pairing_status);
        let status_text = connection_status_text(device.connection_state, device.pairing_status);
        let quality_icon = connection_quality_icon(device.connection_state);

        // Device name and status column with last seen for disconnected devices
        let mut name_status_col = column![
            text(&device.info.device_name).size(14),
            text(status_text).size(11),
        ]
        .spacing(2);

        // Add last seen timestamp for disconnected devices
        if !device.is_connected() && device.last_seen > 0 {
            let last_seen_text = format_last_seen(device.last_seen);
            name_status_col = name_status_col.push(
                text(format!("Last seen: {}", last_seen_text))
                    .size(10)
            );
        }

        // Info row with optional battery indicator
        let info_row = match device_state.battery_level {
            Some(level) => {
                let battery_icon = battery_icon_name(level, device_state.is_charging);
                row![
                    name_status_col,
                    row![
                        icon::from_name(battery_icon).size(14),
                        text(format!("{}%", level)).size(11),
                    ]
                    .spacing(4)
                    .align_y(cosmic::iced::Alignment::Center),
                ]
            }
            None => row![name_status_col],
        }
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Center);

        // Build actions row
        let actions_row = self.build_device_actions(device, device_id);

        // Main device row layout with connection quality indicator
        let mut content = column![
            row![
                container(icon::from_name(device_icon).size(28))
                    .width(Length::Fixed(44.0))
                    .padding(8),
                container(
                    column![
                        icon::from_name(status_icon).size(14),
                        icon::from_name(quality_icon).size(12),
                    ]
                    .spacing(2)
                    .align_x(Horizontal::Center)
                )
                .width(Length::Fixed(22.0))
                .padding(Padding::new(0.0).right(4.0)),
                container(info_row)
                    .width(Length::Fill)
                    .align_x(Horizontal::Left)
                    .padding(Padding::from([4.0, 0.0])),
            ]
            .spacing(4)
            .align_y(cosmic::iced::Alignment::Center)
            .width(Length::Fill),
            container(actions_row)
                .width(Length::Fill)
                .padding(Padding::new(0.0).bottom(4.0).left(66.0).right(12.0))
                .align_x(Horizontal::Left),
        ]
        .spacing(0)
        .padding(Padding::from([8.0, 4.0]))
        .width(Length::Fill);

        // Add settings panel if this device is expanded
        if self.expanded_device_settings.as_ref() == Some(device_id) {
            if let Some(config) = self.device_configs.get(device_id) {
                content = content.push(
                    container(self.device_settings_panel(device_id, device, config))
                        .padding(Padding::from([8, 0, 0, 66])) // Indent under device name
                );
            }
        }

        // Add RemoteDesktop settings panel if active
        if self.remotedesktop_settings_device.as_ref() == Some(device_id) {
            if let Some(settings) = self.remotedesktop_settings.get(device_id) {
                content = content.push(
                    container(self.remotedesktop_settings_view(device_id, settings))
                        .padding(Padding::from([8, 0, 0, 66])) // Indent under device name
                );
            }
        }

        container(content).width(Length::Fill).into()
    }

    fn build_device_actions<'a>(
        &self,
        device: &'a Device,
        device_id: &str,
    ) -> cosmic::iced::widget::Row<'a, Message, cosmic::Theme> {
        let mut actions = row![].spacing(4);

        // Quick actions for connected & paired devices
        if device.is_connected() && device.is_paired() {
            actions = actions
                .push(action_button(
                    "user-available-symbolic",
                    Message::SendPing(device_id.to_string()),
                ))
                .push(action_button(
                    "document-send-symbolic",
                    Message::SendFile(device_id.to_string()),
                ))
                .push(action_button(
                    "insert-text-symbolic",
                    Message::ShareText(device_id.to_string()),
                ))
                .push(action_button(
                    "send-to-symbolic",
                    Message::ShareUrl(device_id.to_string()),
                ));

            if matches!(device.info.device_type, DeviceType::Phone) {
                actions = actions.push(action_button(
                    "find-location-symbolic",
                    Message::FindPhone(device_id.to_string()),
                ));
            }

            // Battery refresh button
            actions = actions.push(action_button(
                "view-refresh-symbolic",
                Message::RequestBatteryUpdate(device_id.to_string()),
            ));
        }

        // Settings button (for paired devices)
        if device.is_paired() {
            actions = actions.push(action_button(
                "emblem-system-symbolic",
                Message::ToggleDeviceSettings(device_id.to_string()),
            ));
        }

        // Pair/Unpair button
        let (label, message) = if device.is_paired() {
            ("Unpair", Message::UnpairDevice(device_id.to_string()))
        } else {
            ("Pair", Message::PairDevice(device_id.to_string()))
        };
        actions = actions.push(button::text(label).on_press(message).padding(6));
        actions
    }

    /// Builds the device settings panel UI
    fn device_settings_panel<'a>(
        &self,
        device_id: &str,
        device: &Device,
        config: &dbus_client::DeviceConfig,
    ) -> Element<'a, Message> {
        use cosmic::widget::{horizontal_space, toggler};

        // Count overrides for display
        let override_count = config.count_plugin_overrides();

        // Header with close button
        let mut header_row = row![
            text("Plugin Settings").size(14),
        ]
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Center);

        // Add override count badge if any overrides exist
        if override_count > 0 {
            header_row = header_row.push(
                text(format!("({} override{})", override_count, if override_count == 1 { "" } else { "s" }))
                    .size(12)
            );
        }

        let header = row![
            header_row,
            horizontal_space(),
            button::icon(icon::from_name("window-close-symbolic").size(14))
                .on_press(Message::ToggleDeviceSettings(device_id.to_string()))
                .padding(4)
        ]
        .width(Length::Fill)
        .align_y(cosmic::iced::Alignment::Center);

        // Build plugin list
        let mut plugin_list = column![].spacing(8);

        for plugin_meta in PLUGINS {
            // Check if device supports this plugin
            let is_supported = device.has_incoming_capability(plugin_meta.capability)
                || device.has_outgoing_capability(plugin_meta.capability);

            // Get current state (device override or global)
            let plugin_enabled = config.get_plugin_enabled(plugin_meta.id);
            let has_override = config.has_plugin_override(plugin_meta.id);

            // Build plugin row
            let mut plugin_row = row![
                icon::from_name(plugin_meta.icon).size(16),
                text(plugin_meta.name).size(12).width(Length::Fill),
            ]
            .spacing(8)
            .align_y(cosmic::iced::Alignment::Center);

            // Override indicator (🟢 or 🔴)
            if has_override {
                plugin_row = plugin_row.push(if plugin_enabled {
                    text("🟢").size(10)
                } else {
                    text("🔴").size(10)
                });
            } else {
                plugin_row = plugin_row.push(text("").size(10));
            }

            // Toggle switch (only enabled for supported plugins)
            if is_supported {
                plugin_row = plugin_row.push(toggler(plugin_enabled).on_toggle({
                    let device_id = device_id.to_string();
                    let plugin_id = plugin_meta.id.to_string();
                    move |enabled| {
                        Message::SetDevicePluginEnabled(device_id.clone(), plugin_id.clone(), enabled)
                    }
                }));
            } else {
                // Show disabled toggle for unsupported plugins
                plugin_row = plugin_row.push(toggler(plugin_enabled));
            }

            // Reset button (if override exists)
            if has_override {
                plugin_row = plugin_row.push(
                    button::icon(icon::from_name("view-refresh-symbolic").size(12))
                        .on_press({
                            let device_id = device_id.to_string();
                            let plugin_id = plugin_meta.id.to_string();
                            Message::ClearDevicePluginOverride(device_id, plugin_id)
                        })
                        .padding(4),
                );
            } else {
                plugin_row = plugin_row.push(
                    button::icon(icon::from_name("view-refresh-symbolic").size(12))
                        .padding(4),
                );
            }

            // Settings button (only for RemoteDesktop plugin)
            if plugin_meta.id == "remotedesktop" {
                plugin_row = plugin_row.push(
                    button::icon(icon::from_name("emblem-system-symbolic").size(12))
                        .on_press(Message::ShowRemoteDesktopSettings(device_id.to_string()))
                        .padding(4),
                );
            }

            // Add to list (grey out if not supported)
            if is_supported {
                plugin_list = plugin_list.push(plugin_row);
            } else {
                // Grey out unsupported plugins (just show them dimmed)
                plugin_list = plugin_list.push(plugin_row);
            }
        }

        // Footer with reset all button (only enabled if there are overrides)
        let footer = if override_count > 0 {
            button::text("Reset All Overrides")
                .on_press(Message::ResetAllPluginOverrides(device_id.to_string()))
                .padding(8)
        } else {
            button::text("Reset All Overrides")
                .padding(8)
        };

        // Combine everything
        container(
            column![
                header,
                divider::horizontal::default(),
                scrollable(plugin_list).height(Length::Fixed(200.0)),
                divider::horizontal::default(),
                footer,
            ]
            .spacing(8),
        )
        .padding(12)
        .into()
    }

    /// RemoteDesktop settings view with quality, FPS, and resolution controls
    fn remotedesktop_settings_view<'a>(
        &self,
        device_id: &str,
        settings: &dbus_client::RemoteDesktopSettings,
    ) -> Element<'a, Message> {
        use cosmic::widget::{horizontal_space, radio, text_input};

        // Header with close button
        let header = row![
            text("Remote Desktop Settings").size(14),
            horizontal_space(),
            button::icon(icon::from_name("window-close-symbolic").size(14))
                .on_press(Message::CloseRemoteDesktopSettings)
                .padding(4)
        ]
        .width(Length::Fill)
        .align_y(cosmic::iced::Alignment::Center);

        // Quality dropdown
        let quality_idx = match settings.quality.as_str() {
            "low" => 0,
            "medium" => 1,
            "high" => 2,
            _ => 1,
        };

        let quality_row = row![
            text("Quality:").width(Length::Fixed(120.0)),
            cosmic::widget::dropdown(
                &["Low", "Medium", "High"],
                Some(quality_idx),
                {
                    let device_id = device_id.to_string();
                    move |idx| {
                        let quality = match idx {
                            0 => "low",
                            1 => "medium",
                            2 => "high",
                            _ => "medium",
                        }.to_string();
                        Message::UpdateRemoteDesktopQuality(device_id.clone(), quality)
                    }
                }
            )
        ]
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Center);

        // FPS dropdown
        let fps_idx = match settings.fps {
            15 => 0,
            30 => 1,
            60 => 2,
            _ => 1,
        };

        let fps_row = row![
            text("Frame Rate:").width(Length::Fixed(120.0)),
            cosmic::widget::dropdown(
                &["15 FPS", "30 FPS", "60 FPS"],
                Some(fps_idx),
                {
                    let device_id = device_id.to_string();
                    move |idx| {
                        let fps = match idx {
                            0 => 15,
                            1 => 30,
                            2 => 60,
                            _ => 30,
                        };
                        Message::UpdateRemoteDesktopFps(device_id.clone(), fps)
                    }
                }
            )
        ]
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Center);

        // Resolution mode radio buttons
        let is_native = settings.resolution_mode == "native";
        let resolution_radios = column![
            radio(
                "Native Resolution",
                "native",
                Some(settings.resolution_mode.as_str()).filter(|_| is_native),
                {
                    let device_id = device_id.to_string();
                    move |_| Message::UpdateRemoteDesktopResolution(device_id.clone(), "native".to_string())
                }
            ),
            radio(
                "Custom Resolution",
                "custom",
                Some(settings.resolution_mode.as_str()).filter(|_| !is_native),
                {
                    let device_id = device_id.to_string();
                    move |_| Message::UpdateRemoteDesktopResolution(device_id.clone(), "custom".to_string())
                }
            ),
        ]
        .spacing(4);

        let resolution_row = row![
            text("Resolution:").width(Length::Fixed(120.0)),
            resolution_radios
        ]
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Start);

        // Build content
        let content = column![
            header,
            divider::horizontal::default(),
            quality_row,
            fps_row,
            resolution_row,
            divider::horizontal::default(),
            button::text("Apply Settings")
                .on_press(Message::SaveRemoteDesktopSettings(device_id.to_string()))
                .padding(8),
        ]
        .spacing(12);

        container(content)
            .padding(12)
            .into()
    }
}

/// Creates a small icon button for device quick actions (ping, send file, etc.)
fn action_button(icon_name: &str, message: Message) -> Element<'static, Message> {
    button::icon(icon::from_name(icon_name).size(16))
        .on_press(message)
        .padding(6)
        .into()
}

/// Returns the icon name for a device type
fn device_type_icon(device_type: DeviceType) -> &'static str {
    match device_type {
        DeviceType::Phone => "phone-symbolic",
        DeviceType::Tablet => "tablet-symbolic",
        DeviceType::Desktop => "computer-symbolic",
        DeviceType::Laptop => "laptop-symbolic",
        DeviceType::Tv => "tv-symbolic",
    }
}

/// Returns the status indicator icon based on connection and pairing state
fn connection_status_icon(
    connection_state: ConnectionState,
    pairing_status: PairingStatus,
) -> &'static str {
    match (connection_state, pairing_status) {
        (ConnectionState::Connected, PairingStatus::Paired) => "emblem-ok-symbolic",
        (_, PairingStatus::Paired) => "emblem-default-symbolic",
        (_, PairingStatus::RequestedByPeer | PairingStatus::Requested) => {
            "emblem-synchronizing-symbolic"
        }
        _ => "dialog-question-symbolic",
    }
}

/// Returns human-readable status text based on connection and pairing state
fn connection_status_text(
    connection_state: ConnectionState,
    pairing_status: PairingStatus,
) -> &'static str {
    match (connection_state, pairing_status) {
        (ConnectionState::Connected, _) => "Connected",
        (ConnectionState::Connecting, _) => "Connecting...",
        (ConnectionState::Failed, _) => "Connection failed",
        (ConnectionState::Disconnected, PairingStatus::Paired) => "Disconnected",
        (ConnectionState::Disconnected, _) => "Not paired",
    }
}

/// Returns the appropriate battery icon name based on charge level and charging state
fn battery_icon_name(level: u8, is_charging: bool) -> &'static str {
    if is_charging {
        "battery-good-charging-symbolic"
    } else {
        match level {
            80..=100 => "battery-full-symbolic",
            50..=79 => "battery-good-symbolic",
            20..=49 => "battery-low-symbolic",
            _ => "battery-caution-symbolic",
        }
    }
}

/// Returns connection quality indicator (signal strength bars) based on connection state
fn connection_quality_icon(connection_state: ConnectionState) -> &'static str {
    match connection_state {
        ConnectionState::Connected => "network-wireless-signal-excellent-symbolic",
        ConnectionState::Connecting => "network-wireless-signal-weak-symbolic",
        ConnectionState::Failed => "network-wireless-signal-none-symbolic",
        ConnectionState::Disconnected => "network-wireless-offline-symbolic",
    }
}

/// Device category for grouping in popup
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceCategory {
    Connected,
    Available,
    Offline,
}

/// Categorize a device based on its state
fn categorize_device(device_state: &DeviceState) -> DeviceCategory {
    let device = &device_state.device;
    if device.is_connected() && device.is_paired() {
        DeviceCategory::Connected
    } else if device.is_reachable() || !device.is_paired() {
        DeviceCategory::Available
    } else {
        DeviceCategory::Offline
    }
}

/// Helper function for pluralization
fn pluralize(count: u64) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// Format last seen timestamp to human-readable string
fn format_last_seen(last_seen: u64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let elapsed = now.saturating_sub(last_seen);

    match elapsed {
        0..60 => "Just now".to_string(),
        60..3600 => {
            let mins = elapsed / MINUTE;
            format!("{} min{} ago", mins, pluralize(mins))
        }
        3600..86400 => {
            let hours = elapsed / HOUR;
            format!("{} hour{} ago", hours, pluralize(hours))
        }
        _ => {
            let days = elapsed / DAY;
            format!("{} day{} ago", days, pluralize(days))
        }
    }
}
