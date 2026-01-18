mod dbus_client;

use std::collections::HashMap;

use cosmic::{
    app::{Core, Task},
    iced::{
        alignment::Horizontal,
        keyboard,
        widget::{column, container, row, scrollable, text},
        window, Color, Length, Padding, Rectangle, Subscription,
    },
    iced_runtime::core::layout::Limits,
    surface::action::{app_popup, destroy_popup},
    theme,
    widget::{button, divider, horizontal_space, icon},
    Element,
};
use cosmic_connect_protocol::{
    ConnectionState, Device, DeviceInfo as ProtocolDeviceInfo, DeviceType, PairingStatus,
};

use cosmic::iced::widget::progress_bar;
use dbus_client::DbusClient;

// COSMIC Design System spacing scale
// Following libcosmic patterns for consistent spacing
const SPACE_XXXS: f32 = 2.0; // Minimal spacing
const SPACE_XXS: f32 = 4.0; // Tight spacing
const SPACE_XS: f32 = 6.0; // Extra small
const SPACE_S: f32 = 8.0; // Small (default for most UI elements)
const SPACE_M: f32 = 12.0; // Medium (sections, groups)
const SPACE_L: f32 = 16.0; // Large (major sections)
const SPACE_XL: f32 = 20.0; // Extra large
const SPACE_XXL: f32 = 24.0; // Double extra large (empty states, major padding)

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
    #[allow(dead_code)]
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

#[derive(Debug, Clone)]
struct TransferState {
    #[allow(dead_code)]
    device_id: String,
    filename: String,
    current: u64,
    total: u64,
    direction: String,
}

struct CConnectApplet {
    core: Core,
    popup: Option<window::Id>,
    devices: Vec<DeviceState>,
    #[allow(dead_code)]
    dbus_client: Option<DbusClient>,
    mpris_players: Vec<String>,
    selected_player: Option<String>,
    // Settings UI state
    expanded_device_settings: Option<String>, // Currently expanded device_id
    device_configs: HashMap<String, dbus_client::DeviceConfig>, // Device-specific configs
    // RemoteDesktop settings UI state
    remotedesktop_settings_device: Option<String>, // device_id showing RemoteDesktop settings
    remotedesktop_settings: HashMap<String, dbus_client::RemoteDesktopSettings>, // In-progress settings
    // RemoteDesktop input state (for validation)
    remotedesktop_width_input: String,
    remotedesktop_height_input: String,
    remotedesktop_error: Option<String>,
    // Search state
    search_query: String,
    // MPRIS state
    mpris_states: std::collections::HashMap<String, dbus_client::PlayerState>,
    mpris_album_art: HashMap<String, cosmic::iced::widget::image::Handle>,
    // File transfers
    active_transfers: HashMap<String, TransferState>,
    // Renaming state
    renaming_device: Option<String>,
    nickname_input: String,
    // History
    history: Vec<HistoryEvent>,
    view_mode: ViewMode,
    // Scanning state
    scanning: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Devices,
    History,
}

#[derive(Debug, Clone)]
struct HistoryEvent {
    #[allow(dead_code)]
    timestamp: std::time::SystemTime,
    event_type: String,
    device_name: String,
    details: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum Message {
    SetViewMode(ViewMode),
    PopupClosed(window::Id),
    PopupOpened,
    DeviceEvent(dbus_client::DaemonEvent),
    SearchChanged(String),
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
    MprisStateUpdated(String, dbus_client::PlayerState),
    MprisAlbumArtLoaded(String, cosmic::iced::widget::image::Handle),
    // Renaming
    StartRenaming(String), // device_id
    CancelRenaming,
    UpdateNicknameInput(String),
    SaveNickname(String), // device_id
    RingDevice(String),   // device_id
    // Settings UI
    ToggleDeviceSettings(String),                          // device_id
    SetDevicePluginEnabled(String, String, bool),          // device_id, plugin, enabled
    ClearDevicePluginOverride(String, String),             // device_id, plugin
    ResetAllPluginOverrides(String),                       // device_id
    DeviceConfigLoaded(String, dbus_client::DeviceConfig), // device_id, config
    // RemoteDesktop settings
    ShowRemoteDesktopSettings(String), // device_id
    CloseRemoteDesktopSettings,
    UpdateRemoteDesktopQuality(String, String), // device_id, quality
    UpdateRemoteDesktopFps(String, u8),         // device_id, fps
    UpdateRemoteDesktopResolution(String, String), // device_id, mode ("native" or "custom")
    UpdateRemoteDesktopCustomWidth(String, String), // device_id, width_str
    UpdateRemoteDesktopCustomHeight(String, String), // device_id, height_str
    SaveRemoteDesktopSettings(String),          // device_id
    RemoteDesktopSettingsLoaded(String, dbus_client::RemoteDesktopSettings), // device_id, settings
    // File Transfer events
    TransferProgress(
        String,
        #[allow(dead_code)] String,
        #[allow(dead_code)] String,
        u64,
        #[allow(dead_code)] u64,
        String,
    ), // id, device, file, cur, tot, dir
    TransferComplete(String, String, String, bool, String), // id, device, file, success, error
    KeyPress(keyboard::Key, keyboard::Modifiers),
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
                tracing::error!("Failed to list devices: {:?}", e);
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
async fn fetch_battery_statuses(
    device_ids: Vec<String>,
) -> HashMap<String, dbus_client::BatteryStatus> {
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
        last_connected: if info.is_connected {
            Some(info.last_seen as u64)
        } else {
            None
        },
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
            remotedesktop_width_input: String::new(),
            remotedesktop_height_input: String::new(),
            remotedesktop_error: None,
            search_query: String::new(),
            mpris_states: std::collections::HashMap::new(),
            mpris_album_art: HashMap::new(),
            active_transfers: std::collections::HashMap::new(),
            renaming_device: None,
            nickname_input: String::new(),
            history: Vec::new(),
            view_mode: ViewMode::Devices,
            scanning: false,
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
            Message::SetViewMode(mode) => {
                self.view_mode = mode;
                Task::none()
            }
            Message::DeviceEvent(event) => {
                let timestamp = std::time::SystemTime::now();
                match &event {
                    dbus_client::DaemonEvent::DeviceAdded {
                        device_id,
                        device_info,
                    } => {
                        self.history.push(HistoryEvent {
                            timestamp,
                            event_type: "Device Found".to_string(),
                            device_name: device_info.name.clone(),
                            details: format!("ID: {}", device_id),
                        });
                    }
                    dbus_client::DaemonEvent::DeviceRemoved { device_id } => {
                        let name = self
                            .devices
                            .iter()
                            .find(|d| d.device.info.device_id == *device_id)
                            .map(|d| d.device.info.device_name.clone())
                            .unwrap_or_else(|| "Unknown".to_string());

                        self.history.push(HistoryEvent {
                            timestamp,
                            event_type: "Device Removed".to_string(),
                            device_name: name,
                            details: format!("ID: {}", device_id),
                        });
                    }
                    dbus_client::DaemonEvent::DeviceStateChanged { device_id, state } => {
                        let name = self
                            .devices
                            .iter()
                            .find(|d| d.device.info.device_id == *device_id)
                            .map(|d| d.device.info.device_name.clone())
                            .unwrap_or_else(|| "Unknown".to_string());

                        self.history.push(HistoryEvent {
                            timestamp,
                            event_type: "State Changed".to_string(),
                            device_name: name,
                            details: state.clone(),
                        });
                    }
                    dbus_client::DaemonEvent::PairingRequest { device_id } => {
                        self.history.push(HistoryEvent {
                            timestamp,
                            event_type: "Pairing Request".to_string(),
                            device_name: "Unknown".to_string(),
                            details: format!("Device {} wants to pair", device_id),
                        });
                    }
                    dbus_client::DaemonEvent::PairingStatusChanged {
                        device_id: _,
                        status,
                    } => {
                        self.history.push(HistoryEvent {
                            timestamp,
                            event_type: "Pairing Status".to_string(),
                            device_name: "Unknown".to_string(),
                            details: status.clone(),
                        });
                    }
                    _ => {}
                }

                if self.history.len() > 50 {
                    self.history.remove(0);
                }

                fetch_devices_task()
            }
            Message::SearchChanged(query) => {
                self.search_query = query;
                Task::none()
            }
            Message::DeviceListUpdated(devices) => {
                tracing::info!("Device list updated: {} devices", devices.len());
                self.scanning = false;

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

                tracing::debug!(
                    "Fetching battery status for {} connected devices",
                    connected_ids.len()
                );
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
                self.scanning = true;
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
                Task::perform(open_file_picker(device_id), |result| match result {
                    Some((device_id, path)) => {
                        cosmic::Action::App(Message::FileSelected(device_id, path))
                    }
                    None => {
                        tracing::debug!("File picker cancelled or no file selected");
                        cosmic::Action::App(Message::RefreshDevices)
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
                    Some(text) => device_operation_task(
                        device_id,
                        "share text",
                        move |client, id| async move { client.share_text(&id, &text).await },
                    ),
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
                        device_operation_task(
                            device_id,
                            "share URL",
                            move |client, id| async move { client.share_url(&id, &text).await },
                        )
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
                self.selected_player = Some(player.clone());
                // Fetch state for selected player
                let player_arg = player.clone();
                let player_closure = player.clone();
                Task::perform(
                    async move {
                        if let Ok((client, _)) = DbusClient::connect().await {
                            match client.get_player_state(&player_arg).await {
                                Ok(state) => Some(state),
                                Err(e) => {
                                    tracing::error!("Failed to get player state: {}", e);
                                    None
                                }
                            }
                        } else {
                            None
                        }
                    },
                    move |state| {
                        if let Some(s) = state {
                            cosmic::Action::App(Message::MprisStateUpdated(
                                player_closure.clone(),
                                s,
                            ))
                        } else {
                            // No-op if failed
                            cosmic::Action::App(Message::MprisStateUpdated(
                                player_closure.clone(),
                                dbus_client::PlayerState {
                                    name: player_closure.clone(),
                                    identity: player_closure.clone(),
                                    playback_status: dbus_client::PlaybackStatus::Stopped,
                                    position: 0,
                                    volume: 0.0,
                                    loop_status: dbus_client::LoopStatus::None,
                                    shuffle: false,
                                    can_play: false,
                                    can_pause: false,
                                    can_go_next: false,
                                    can_go_previous: false,
                                    can_seek: false,
                                    metadata: Default::default(),
                                },
                            ))
                        }
                    },
                )
            }
            Message::MprisStateUpdated(player, state) => {
                if let Some(url) = &state.metadata.album_art_url {
                    if url.starts_with("file://") {
                        let path = url.trim_start_matches("file://");
                        self.mpris_album_art.insert(
                            player.clone(),
                            cosmic::iced::widget::image::Handle::from_path(path),
                        );
                    } else {
                        self.mpris_album_art.remove(&player);
                    }
                } else {
                    self.mpris_album_art.remove(&player);
                }

                self.mpris_states.insert(player, state);
                Task::none()
            }
            Message::MprisAlbumArtLoaded(_, _) => Task::none(),
            Message::MprisControl(player, action) => {
                tracing::info!("MPRIS control: {} on {}", action, player);
                let player_arg = player.clone();
                let player_closure = player.clone();
                Task::perform(
                    async move {
                        if let Ok((client, _)) = DbusClient::connect().await {
                            if let Err(e) = client.mpris_control(&player_arg, &action).await {
                                tracing::error!("Failed to control MPRIS player: {}", e);
                            }
                            // Fetch updated state
                            match client.get_player_state(&player_arg).await {
                                Ok(state) => Some(state),
                                Err(_) => None,
                            }
                        } else {
                            None
                        }
                    },
                    move |state| {
                        if let Some(s) = state {
                            cosmic::Action::App(Message::MprisStateUpdated(
                                player_closure.clone(),
                                s,
                            ))
                        } else {
                            // Dummy state to satisfay type system if we strictly need Message
                            cosmic::Action::App(Message::MprisStateUpdated(
                                player_closure.clone(),
                                dbus_client::PlayerState {
                                    name: player_closure.clone(),
                                    identity: player_closure.clone(),
                                    playback_status: dbus_client::PlaybackStatus::Stopped,
                                    position: 0,
                                    volume: 0.0,
                                    loop_status: dbus_client::LoopStatus::None,
                                    shuffle: false,
                                    can_play: false,
                                    can_pause: false,
                                    can_go_next: false,
                                    can_go_previous: false,
                                    can_seek: false,
                                    metadata: Default::default(),
                                },
                            ))
                        }
                    },
                )
            }
            Message::StartRenaming(device_id) => {
                // Pre-fill input with current nickname if relevant
                let nickname = self
                    .device_configs
                    .get(&device_id)
                    .and_then(|c| c.nickname.clone())
                    .unwrap_or_default();

                self.nickname_input = nickname;
                self.renaming_device = Some(device_id);
                Task::none()
            }
            Message::CancelRenaming => {
                self.renaming_device = None;
                self.nickname_input.clear();
                Task::none()
            }
            Message::UpdateNicknameInput(value) => {
                self.nickname_input = value;
                Task::none()
            }
            Message::SaveNickname(device_id) => {
                let nickname = self.nickname_input.clone();
                self.renaming_device = None;
                self.nickname_input.clear();

                Task::perform(
                    async move {
                        if let Ok((client, _)) = DbusClient::connect().await {
                            if let Err(e) = client.set_device_nickname(&device_id, &nickname).await
                            {
                                tracing::error!("Failed to set nickname: {}", e);
                            }
                        }
                    },
                    |_| cosmic::Action::App(Message::RefreshDevices),
                )
            }
            Message::RingDevice(device_id) => Task::perform(
                async move {
                    if let Ok((client, _)) = DbusClient::connect().await {
                        if let Err(e) = client.find_phone(&device_id).await {
                            tracing::error!("Failed to ring device: {}", e);
                        }
                    }
                },
                |_| cosmic::Action::None,
            ),
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
                                Ok((client, _)) => {
                                    client.get_device_config(&device_id_for_async).await
                                }
                                Err(e) => {
                                    tracing::error!("Failed to connect to daemon: {}", e);
                                    Err(e)
                                }
                            }
                        },
                        move |result| {
                            let device_id = (*device_id_for_msg).clone();
                            match result {
                                Ok(config) => cosmic::Action::App(Message::DeviceConfigLoaded(
                                    device_id, config,
                                )),
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
                tracing::info!(
                    "Setting plugin {} to {} for device {}",
                    plugin,
                    if enabled { "enabled" } else { "disabled" },
                    device_id
                );
                let device_id_for_async = device_id.clone();
                let device_id_for_msg = std::sync::Arc::new(device_id.clone());
                device_operation_task(device_id, "set plugin enabled", move |client, id| {
                    let plugin_clone = plugin.clone();
                    async move {
                        client
                            .set_device_plugin_enabled(&id, &plugin_clone, enabled)
                            .await
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
                            Ok(config) => {
                                cosmic::Action::App(Message::DeviceConfigLoaded(device_id, config))
                            }
                            Err(_) => cosmic::Action::App(Message::RefreshDevices),
                        }
                    },
                ))
            }
            Message::ClearDevicePluginOverride(device_id, plugin) => {
                tracing::info!(
                    "Clearing plugin override for {} on device {}",
                    plugin,
                    device_id
                );
                let device_id_for_async = device_id.clone();
                let device_id_for_msg = std::sync::Arc::new(device_id.clone());
                device_operation_task(device_id, "clear plugin override", move |client, id| {
                    let plugin_clone = plugin.clone();
                    async move {
                        client
                            .clear_device_plugin_override(&id, &plugin_clone)
                            .await
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
                            Ok(config) => {
                                cosmic::Action::App(Message::DeviceConfigLoaded(device_id, config))
                            }
                            Err(_) => cosmic::Action::App(Message::RefreshDevices),
                        }
                    },
                ))
            }
            Message::ResetAllPluginOverrides(device_id) => {
                tracing::info!("Resetting all plugin overrides for device {}", device_id);
                let device_id_for_async = device_id.clone();
                let device_id_for_msg = std::sync::Arc::new(device_id.clone());
                device_operation_task(
                    device_id,
                    "reset all plugin overrides",
                    move |client, id| async move { client.reset_all_plugin_overrides(&id).await },
                )
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
                            Ok(config) => {
                                cosmic::Action::App(Message::DeviceConfigLoaded(device_id, config))
                            }
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
                self.remotedesktop_error = None;

                let device_id_for_async = device_id.clone();
                let device_id_for_msg = std::sync::Arc::new(device_id.clone());

                // Fetch current settings
                Task::perform(
                    async move {
                        match DbusClient::connect().await {
                            Ok((client, _)) => {
                                client
                                    .get_remotedesktop_settings(&device_id_for_async)
                                    .await
                            }
                            Err(e) => Err(e),
                        }
                    },
                    move |result| {
                        let device_id = (*device_id_for_msg).clone();
                        match result {
                            Ok(settings) => cosmic::Action::App(
                                Message::RemoteDesktopSettingsLoaded(device_id, settings),
                            ),
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

                // Initialize input fields from settings
                self.remotedesktop_width_input = settings.custom_width.unwrap_or(1920).to_string();
                self.remotedesktop_height_input =
                    settings.custom_height.unwrap_or(1080).to_string();

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
                // Update input string
                self.remotedesktop_width_input = width_str.clone();

                // Validate
                if let Ok(width) = width_str.parse::<u32>() {
                    if width < 640 || width > 7680 {
                        self.remotedesktop_error =
                            Some("Width must be between 640 and 7680".to_string());
                    } else {
                        // Check height as well to clear error if both are valid
                        if let Ok(height) = self.remotedesktop_height_input.parse::<u32>() {
                            if height >= 480 && height <= 4320 {
                                self.remotedesktop_error = None;
                            }
                        } else {
                            // Wait for height to be valid
                        }
                    }
                } else if !width_str.is_empty() {
                    self.remotedesktop_error = Some("Invalid width format".to_string());
                }

                if let Some(settings) = self.remotedesktop_settings.get_mut(&device_id) {
                    settings.custom_width = width_str.parse().ok();
                }
                Task::none()
            }
            Message::UpdateRemoteDesktopCustomHeight(device_id, height_str) => {
                // Update input string
                self.remotedesktop_height_input = height_str.clone();

                // Validate
                if let Ok(height) = height_str.parse::<u32>() {
                    if height < 480 || height > 4320 {
                        self.remotedesktop_error =
                            Some("Height must be between 480 and 4320".to_string());
                    } else {
                        // Check width as well to clear error if both are valid
                        if let Ok(width) = self.remotedesktop_width_input.parse::<u32>() {
                            if width >= 640 && width <= 7680 {
                                self.remotedesktop_error = None;
                            }
                        }
                    }
                } else if !height_str.is_empty() {
                    self.remotedesktop_error = Some("Invalid height format".to_string());
                }

                if let Some(settings) = self.remotedesktop_settings.get_mut(&device_id) {
                    settings.custom_height = height_str.parse().ok();
                }
                Task::none()
            }
            Message::SaveRemoteDesktopSettings(device_id) => {
                tracing::info!("Saving RemoteDesktop settings for {}", device_id);

                if let Some(mut settings) = self.remotedesktop_settings.get(&device_id).cloned() {
                    // Final validation from inputs
                    let width_res = self.remotedesktop_width_input.parse::<u32>();
                    let height_res = self.remotedesktop_height_input.parse::<u32>();

                    match (width_res, height_res) {
                        (Ok(w), Ok(h)) => {
                            if w < 640 || w > 7680 || h < 480 || h > 4320 {
                                self.remotedesktop_error = Some(
                                    "Resolution out of bounds (640x480 - 7680x4320)".to_string(),
                                );
                                return Task::none();
                            }
                            // Update settings with validated values
                            settings.custom_width = Some(w);
                            settings.custom_height = Some(h);
                        }
                        _ => {
                            self.remotedesktop_error =
                                Some("Invalid resolution values".to_string());
                            return Task::none();
                        }
                    }

                    Task::perform(
                        async move {
                            match DbusClient::connect().await {
                                Ok((client, _)) => {
                                    client
                                        .set_remotedesktop_settings(&device_id, &settings)
                                        .await
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
            Message::TransferProgress(tid, device_id, filename, cur, tot, dir) => {
                self.active_transfers.insert(
                    tid,
                    TransferState {
                        device_id,
                        filename,
                        current: cur,
                        total: tot,
                        direction: dir,
                    },
                );
                Task::none()
            }
            Message::TransferComplete(tid, _, _, success, _) => {
                self.active_transfers.remove(&tid);
                if success {
                    tracing::info!("Transfer {} completed successfully", tid);
                } else {
                    tracing::warn!("Transfer {} failed or cancelled", tid);
                }
                Task::none()
            }

            Message::KeyPress(key, modifiers) => {
                use cosmic::iced::keyboard::key::Named;

                if let keyboard::Key::Named(Named::Escape) = key {
                    if let Some(id) = self.popup {
                        return cosmic::task::message(cosmic::Action::Cosmic(
                            cosmic::app::Action::Surface(destroy_popup(id)),
                        ));
                    }
                }

                if key == keyboard::Key::Character("r".into()) && modifiers.control() {
                    return cosmic::task::message(cosmic::Action::App(Message::RefreshDevices));
                }

                Task::none()
            }
        }
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Self::Message> {
        struct DbusSubscription;

        let dbus_sub = cosmic::iced::Subscription::run_with_id(
            std::any::TypeId::of::<DbusSubscription>(),
            cosmic::iced::futures::stream::unfold(
                None,
                |client_opt: Option<dbus_client::ReconnectingClient>| async move {
                    let mut client = match client_opt {
                        Some(c) => c,
                        None => match dbus_client::ReconnectingClient::new().await {
                            Ok(c) => c,
                            Err(e) => {
                                tracing::error!("Failed to connect to DBus: {}", e);
                                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                return Some((Message::RefreshDevices, None)); // Dummy msg to retry
                            }
                        },
                    };

                    if let Some(event) = client.recv_event().await {
                        let msg = match event {
                            dbus_client::DaemonEvent::TransferProgress {
                                transfer_id,
                                device_id,
                                filename,
                                current,
                                total,
                                direction,
                            } => Some(Message::TransferProgress(
                                transfer_id,
                                device_id,
                                filename,
                                current,
                                total,
                                direction,
                            )),
                            dbus_client::DaemonEvent::TransferComplete {
                                transfer_id,
                                device_id,
                                filename,
                                success,
                                error,
                            } => Some(Message::TransferComplete(
                                transfer_id,
                                device_id,
                                filename,
                                success,
                                error,
                            )),
                            e @ dbus_client::DaemonEvent::DeviceAdded { .. }
                            | e @ dbus_client::DaemonEvent::DeviceRemoved { .. }
                            | e @ dbus_client::DaemonEvent::PairingRequest { .. }
                            | e @ dbus_client::DaemonEvent::PairingStatusChanged { .. }
                            | e @ dbus_client::DaemonEvent::DeviceStateChanged { .. } => {
                                Some(Message::DeviceEvent(e))
                            }
                            _ => None,
                        };

                        if let Some(m) = msg {
                            Some((m, Some(client)))
                        } else {
                            // Loop again for unhandled events
                            Some((Message::RefreshDevices, Some(client))) // Ideally we'd loop internal but this is ok
                        }
                    } else {
                        // Channel closed, reconnect
                        Some((Message::RefreshDevices, None))
                    }
                },
            ),
        );

        let keyboard_sub =
            keyboard::on_key_press(|key, modifiers| Some(Message::KeyPress(key, modifiers)));

        Subscription::batch(vec![dbus_sub, keyboard_sub])
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
            Message::Surface,
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
    fn history_view(&self) -> Element<'_, Message> {
        let mut history_list = column![].spacing(SPACE_XXS);

        if self.history.is_empty() {
            history_list = history_list.push(
                container(cosmic::widget::text::body("No history events"))
                    .width(Length::Fill)
                    .align_x(cosmic::iced::Alignment::Center)
                    .padding(SPACE_XL),
            );
        } else {
            // In reverse order (newest first)
            for event in self.history.iter().rev() {
                let row = row![
                    column![
                        cosmic::widget::text::body(&event.event_type),
                        cosmic::widget::text::caption(&event.device_name),
                    ],
                    horizontal_space(),
                    cosmic::widget::text::caption(&event.details).width(Length::Fixed(150.0)),
                ]
                .width(Length::Fill)
                .align_y(cosmic::iced::Alignment::Center);

                history_list = history_list.push(
                    container(row)
                        .padding(SPACE_S)
                        .class(cosmic::theme::Container::Card),
                );
            }
        }

        scrollable(history_list).into()
    }

    fn popup_view(&self) -> Element<'_, Message> {
        let view_switcher = row![
            button::text("Devices")
                .on_press(Message::SetViewMode(ViewMode::Devices))
                .width(Length::Fill),
            button::text("History")
                .on_press(Message::SetViewMode(ViewMode::History))
                .width(Length::Fill)
        ]
        .spacing(SPACE_XXS)
        .width(Length::Fill);

        if self.view_mode == ViewMode::History {
            return column![
                view_switcher,
                divider::horizontal::default(),
                self.history_view()
            ]
            .spacing(SPACE_S)
            .padding(SPACE_M)
            .into();
        }

        let search_input = cosmic::widget::text_input("Search devices...", &self.search_query)
            .on_input(Message::SearchChanged)
            .width(Length::Fill);

        let header = row![view_switcher,]
            .spacing(SPACE_S)
            .align_y(cosmic::iced::Alignment::Center)
            .width(Length::Fill);

        let controls = if self.scanning {
            row![
                search_input,
                container(
                    row![
                        cosmic::widget::text::caption("Scanning"),
                        icon::from_name("process-working-symbolic").size(16),
                    ]
                    .spacing(SPACE_S)
                    .align_y(cosmic::iced::Alignment::Center)
                )
                .padding(SPACE_XXS)
            ]
            .spacing(SPACE_S)
        } else {
            row![
                search_input,
                button::icon(icon::from_name("view-refresh-symbolic"))
                    .on_press(Message::RefreshDevices)
                    .padding(SPACE_XXS)
            ]
            .spacing(SPACE_S)
        };

        // MPRIS media controls section
        let mpris_section = self.mpris_controls_view();

        let content = if self.devices.is_empty() {
            column![
                container(icon::from_name("phone-disconnected-symbolic").size(48))
                    .padding(Padding::from([SPACE_S, 0.0, SPACE_M, 0.0])),
                cosmic::widget::text::heading("No Devices Found"),
                cosmic::widget::text::body("Make sure:"),
                cosmic::widget::text::caption("• CConnect app is installed on your devices"),
                cosmic::widget::text::caption("• Devices are on the same network"),
                cosmic::widget::text::caption("• Firewall ports 1814-1864 are open"),
                container(
                    button::text("Refresh")
                        .on_press(Message::RefreshDevices)
                        .padding(SPACE_S)
                )
                .padding(Padding::from([SPACE_M, 0.0, 0.0, 0.0])),
            ]
            .spacing(SPACE_S)
            .padding(SPACE_XXL)
            .width(Length::Fill)
            .align_x(Horizontal::Center)
        } else {
            // Group devices by category
            let mut connected = Vec::new();
            let mut available = Vec::new();
            let mut offline = Vec::new();

            for device_state in &self.devices {
                // Filter logic
                if !self.search_query.is_empty() {
                    let q = self.search_query.to_lowercase();
                    let name_match = device_state
                        .device
                        .info
                        .device_name
                        .to_lowercase()
                        .contains(&q);
                    let type_match =
                        device_type_icon(device_state.device.info.device_type).contains(&q); // rough proxy
                    if !name_match && !type_match {
                        continue;
                    }
                }

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
                    container(cosmic::widget::text::caption("Connected"))
                        .padding(Padding::from([SPACE_S, SPACE_M, SPACE_XXS, SPACE_M]))
                        .width(Length::Fill),
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
                    container(cosmic::widget::text::caption("Available"))
                        .padding(Padding::from([SPACE_S, SPACE_M, SPACE_XXS, SPACE_M]))
                        .width(Length::Fill),
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
                    container(cosmic::widget::text::caption("Offline"))
                        .padding(Padding::from([SPACE_S, SPACE_M, SPACE_XXS, SPACE_M]))
                        .width(Length::Fill),
                );
                for device_state in &offline {
                    device_groups = device_groups.push(self.device_row(device_state));
                }
            }

            device_groups
        };

        let popup_content = column![
            container(header)
                .padding(Padding::from([SPACE_S, SPACE_M]))
                .width(Length::Fill),
            container(controls)
                .padding(Padding::from([0.0, SPACE_M, SPACE_S, SPACE_M]))
                .width(Length::Fill),
            divider::horizontal::default(),
            mpris_section,
            self.transfers_view(),
            divider::horizontal::default(),
            scrollable(content).height(Length::Fill),
        ]
        .width(Length::Fill);

        container(popup_content)
            .padding(0)
            .width(Length::Fixed(360.0))
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
            cosmic::widget::text::body(selected_player),
        ]
        .spacing(SPACE_XS)
        .align_y(cosmic::iced::Alignment::Center);

        // Metadata display
        let mut metadata_col = column![];

        let state = self.mpris_states.get(selected_player);

        if let Some(state) = state {
            if let Some(title) = &state.metadata.title {
                metadata_col = metadata_col.push(cosmic::widget::text::body(title));
            }
            if let Some(artist) = &state.metadata.artist {
                metadata_col = metadata_col.push(cosmic::widget::text::body(artist));
            }
            if let Some(album) = &state.metadata.album {
                metadata_col = metadata_col.push(cosmic::widget::text::caption(album));
            }
            // Use album art if available (placeholder for now/Url later)
            // If using libcosmic image support
        } else {
            metadata_col = metadata_col.push(cosmic::widget::text::body("Unknown"));
        }

        // Playback controls
        let status = state
            .map(|s| s.playback_status)
            .unwrap_or(dbus_client::PlaybackStatus::Stopped);
        let play_icon = if status == dbus_client::PlaybackStatus::Playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        };
        let play_action = if status == dbus_client::PlaybackStatus::Playing {
            "Pause"
        } else {
            "Play"
        };

        let controls = row![
            button::icon(icon::from_name("media-skip-backward-symbolic").size(16))
                .on_press(Message::MprisControl(
                    selected_player.clone(),
                    "Previous".to_string()
                ))
                .padding(SPACE_XS),
            button::icon(icon::from_name(play_icon).size(16))
                .on_press(Message::MprisControl(
                    selected_player.clone(),
                    play_action.to_string()
                ))
                .padding(SPACE_XS),
            button::icon(icon::from_name("media-playback-stop-symbolic").size(16))
                .on_press(Message::MprisControl(
                    selected_player.clone(),
                    "Stop".to_string()
                ))
                .padding(SPACE_XS),
            button::icon(icon::from_name("media-skip-forward-symbolic").size(16))
                .on_press(Message::MprisControl(
                    selected_player.clone(),
                    "Next".to_string()
                ))
                .padding(SPACE_XS),
        ]
        .spacing(SPACE_XXS)
        .align_y(cosmic::iced::Alignment::Center);

        let art_handle = self.mpris_album_art.get(selected_player);

        let info_row = if let Some(handle) = art_handle {
            row![
                cosmic::widget::image(handle.clone())
                    .width(Length::Fixed(50.0))
                    .height(Length::Fixed(50.0))
                    .content_fit(cosmic::iced::ContentFit::Cover),
                metadata_col
            ]
            .spacing(SPACE_M)
            .align_y(cosmic::iced::Alignment::Center)
        } else {
            row![
                container(icon::from_name("audio-x-generic-symbolic").size(32))
                    .width(Length::Fixed(50.0))
                    .align_x(cosmic::iced::Alignment::Center),
                metadata_col
            ]
            .spacing(SPACE_M)
            .align_y(cosmic::iced::Alignment::Center)
        };

        let content = column![player_name, info_row, controls]
            .spacing(SPACE_S)
            .padding(Padding::from([SPACE_S, SPACE_M]));

        container(content)
            .width(Length::Fill)
            .class(cosmic::theme::Container::Card)
            .into()
    }

    fn device_row<'a>(&'a self, device_state: &'a DeviceState) -> Element<'a, Message> {
        let device = &device_state.device;
        let device_id = &device.info.device_id;

        let device_icon = device_type_icon(device.info.device_type);
        let status_icon = connection_status_icon(device.connection_state, device.pairing_status);
        let quality_icon = connection_quality_icon(device.connection_state);

        // Device name and status column with last seen for disconnected devices
        let nickname = self
            .device_configs
            .get(device_id)
            .and_then(|c| c.nickname.as_deref());

        let display_name = nickname.unwrap_or(&device.info.device_name);

        let mut name_status_col = column![
            cosmic::widget::text::heading(display_name),
            connection_status_styled_text(device.connection_state, device.pairing_status),
        ]
        .spacing(SPACE_XXXS);

        // Add last seen timestamp for disconnected devices
        if !device.is_connected() && device.last_seen > 0 {
            let last_seen_text = format_last_seen(device.last_seen);
            name_status_col = name_status_col.push(cosmic::widget::text::caption(format!(
                "Last seen: {}",
                last_seen_text
            )));
        }

        // Info row with optional battery indicator
        let info_row = match device_state.battery_level {
            Some(level) => {
                let battery_icon = battery_icon_name(level, device_state.is_charging);
                row![
                    name_status_col,
                    row![
                        icon::from_name(battery_icon).size(14),
                        cosmic::widget::text::caption(format!("{}%", level)),
                    ]
                    .spacing(SPACE_XXS)
                    .align_y(cosmic::iced::Alignment::Center),
                ]
            }
            None => row![name_status_col],
        }
        .spacing(SPACE_S)
        .align_y(cosmic::iced::Alignment::Center);

        // Build actions row
        let actions_row = self.build_device_actions(device, device_id);

        // Main device row layout with connection quality indicator
        let mut content = column![
            row![
                container(icon::from_name(device_icon).size(28))
                    .width(Length::Fixed(44.0))
                    .padding(SPACE_S),
                container(
                    column![
                        icon::from_name(status_icon).size(14),
                        icon::from_name(quality_icon).size(12),
                    ]
                    .spacing(SPACE_XXXS)
                    .align_x(Horizontal::Center)
                )
                .width(Length::Fixed(22.0))
                .padding(Padding::new(0.0).right(SPACE_XXS)),
                container(info_row)
                    .width(Length::Fill)
                    .align_x(Horizontal::Left)
                    .padding(Padding::from([SPACE_XXS, 0.0])),
            ]
            .spacing(SPACE_XXS)
            .align_y(cosmic::iced::Alignment::Center)
            .width(Length::Fill),
            container(actions_row)
                .width(Length::Fill)
                .padding(
                    Padding::new(0.0)
                        .bottom(SPACE_XXS)
                        .left(66.0)
                        .right(SPACE_M)
                )
                .align_x(Horizontal::Left),
        ]
        .spacing(SPACE_XXS)
        .padding(Padding::from([SPACE_M, SPACE_L]))
        .width(Length::Fill);

        // Add settings panel if this device is expanded
        if self.expanded_device_settings.as_ref() == Some(device_id) {
            if let Some(config) = self.device_configs.get(device_id) {
                content = content.push(
                    container(self.device_settings_panel(device_id, device, config))
                        .padding(Padding::from([SPACE_S, 0.0, 0.0, 66.0])), // Indent under device name
                );
            }
        }

        // Add RemoteDesktop settings panel if active
        if self.remotedesktop_settings_device.as_ref() == Some(device_id) {
            if let Some(settings) = self.remotedesktop_settings.get(device_id) {
                content = content.push(
                    container(self.remotedesktop_settings_view(device_id, settings))
                        .padding(Padding::from([SPACE_S, 0.0, 0.0, 66.0])), // Indent under device name
                );
            }
        }

        container(content)
            .width(Length::Fill)
            .class(cosmic::theme::Container::Card)
            .into()
    }

    fn build_device_actions<'a>(
        &self,
        device: &'a Device,
        device_id: &str,
    ) -> cosmic::iced::widget::Row<'a, Message, cosmic::Theme> {
        let mut actions = row![].spacing(SPACE_S);

        // Quick actions for connected & paired devices
        if device.is_connected() && device.is_paired() {
            actions = actions
                .push(action_button_with_tooltip(
                    "user-available-symbolic",
                    "Send ping",
                    Message::SendPing(device_id.to_string()),
                ))
                .push(action_button_with_tooltip(
                    "document-send-symbolic",
                    "Send file",
                    Message::SendFile(device_id.to_string()),
                ))
                .push(action_button_with_tooltip(
                    "insert-text-symbolic",
                    "Share clipboard text",
                    Message::ShareText(device_id.to_string()),
                ));

            // Add Find My Phone if supported (or always for now as capability check might be tricky without exact string)
            if device.has_incoming_capability("cconnect.findmyphone.request") {
                actions = actions.push(action_button_with_tooltip(
                    "find-location-symbolic",
                    "Ring device",
                    Message::RingDevice(device_id.to_string()),
                ));
            }

            actions = actions.push(action_button_with_tooltip(
                "send-to-symbolic",
                "Share URL",
                Message::ShareUrl(device_id.to_string()),
            ));

            if matches!(device.info.device_type, DeviceType::Phone) {
                actions = actions.push(action_button_with_tooltip(
                    "find-location-symbolic",
                    "Find my phone",
                    Message::FindPhone(device_id.to_string()),
                ));
            }

            // Battery refresh button
            actions = actions.push(action_button_with_tooltip(
                "view-refresh-symbolic",
                "Refresh battery status",
                Message::RequestBatteryUpdate(device_id.to_string()),
            ));
        }

        // Settings button (for paired devices)
        if device.is_paired() {
            actions = actions.push(action_button_with_tooltip(
                "emblem-system-symbolic",
                "Plugin settings",
                Message::ToggleDeviceSettings(device_id.to_string()),
            ));
        }

        // Pair/Unpair button
        let (label, message) = if device.is_paired() {
            ("Unpair", Message::UnpairDevice(device_id.to_string()))
        } else {
            ("Pair", Message::PairDevice(device_id.to_string()))
        };
        actions = actions.push(button::text(label).on_press(message).padding(SPACE_XS));
        actions
    }

    /// Builds the device settings panel UI
    fn device_settings_panel<'a>(
        &'a self,
        device_id: &str,
        device: &'a Device,
        config: &'a dbus_client::DeviceConfig,
    ) -> Element<'a, Message> {
        use cosmic::widget::{horizontal_space, toggler};

        // Count overrides for display
        let override_count = config.count_plugin_overrides();

        // Header with close button
        let mut header_row = row![cosmic::widget::text::body("Plugin Settings"),]
            .spacing(SPACE_S)
            .align_y(cosmic::iced::Alignment::Center);

        // Add override count badge if any overrides exist
        if override_count > 0 {
            header_row = header_row.push(
                text(format!(
                    "({} override{})",
                    override_count,
                    if override_count == 1 { "" } else { "s" }
                ))
                .size(12),
            );
        }

        let header = row![
            header_row,
            horizontal_space(),
            button::icon(icon::from_name("window-close-symbolic").size(14))
                .on_press(Message::ToggleDeviceSettings(device_id.to_string()))
                .padding(SPACE_XXS)
        ]
        .width(Length::Fill)
        .align_y(cosmic::iced::Alignment::Center);

        // Renaming UI
        let rename_section = if self.renaming_device.as_deref() == Some(device_id) {
            row![
                cosmic::widget::text_input("Nickname", &self.nickname_input)
                    .on_input(Message::UpdateNicknameInput)
                    .width(Length::Fill),
                button::icon(icon::from_name("emblem-ok-symbolic").size(16))
                    .on_press(Message::SaveNickname(device_id.to_string()))
                    .padding(SPACE_XS),
                button::icon(icon::from_name("process-stop-symbolic").size(16))
                    .on_press(Message::CancelRenaming)
                    .padding(SPACE_XS),
            ]
            .spacing(SPACE_S)
            .align_y(cosmic::iced::Alignment::Center)
        } else {
            row![
                text(
                    config
                        .nickname
                        .as_deref()
                        .unwrap_or(&device.info.device_name)
                )
                .size(14)
                .width(Length::Fill),
                button::icon(icon::from_name("document-edit-symbolic").size(14))
                    .on_press(Message::StartRenaming(device_id.to_string()))
                    .padding(SPACE_XXS)
            ]
            .spacing(SPACE_S)
            .align_y(cosmic::iced::Alignment::Center)
        };

        // Build plugin list
        let mut plugin_list = column![].spacing(SPACE_S);

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
                cosmic::widget::text::caption(plugin_meta.name).width(Length::Fill),
            ]
            .spacing(SPACE_S)
            .align_y(cosmic::iced::Alignment::Center);

            // Override indicator (Icon + Text for accessibility)
            if has_override {
                plugin_row = plugin_row.push(if plugin_enabled {
                    row![
                        icon::from_name("emblem-ok-symbolic").size(12),
                        cosmic::widget::text::caption("Override: On")
                    ]
                    .spacing(SPACE_XXS)
                    .align_y(cosmic::iced::Alignment::Center)
                } else {
                    row![
                        icon::from_name("emblem-important-symbolic").size(12),
                        cosmic::widget::text::caption("Override: Off")
                    ]
                    .spacing(SPACE_XXS)
                    .align_y(cosmic::iced::Alignment::Center)
                });
            } else {
                plugin_row = plugin_row.push(cosmic::widget::text::caption(""));
            }

            // Toggle switch (only enabled for supported plugins)
            if is_supported {
                plugin_row = plugin_row.push(toggler(plugin_enabled).on_toggle({
                    let device_id = device_id.to_string();
                    let plugin_id = plugin_meta.id.to_string();
                    move |enabled| {
                        Message::SetDevicePluginEnabled(
                            device_id.clone(),
                            plugin_id.clone(),
                            enabled,
                        )
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
                        .padding(SPACE_XXS),
                );
            } else {
                plugin_row = plugin_row.push(
                    button::icon(icon::from_name("view-refresh-symbolic").size(12))
                        .padding(SPACE_XXS),
                );
            }

            // Settings button (only for RemoteDesktop plugin)
            if plugin_meta.id == "remotedesktop" {
                plugin_row = plugin_row.push(
                    button::icon(icon::from_name("emblem-system-symbolic").size(12))
                        .on_press(Message::ShowRemoteDesktopSettings(device_id.to_string()))
                        .padding(SPACE_XXS),
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
                .padding(SPACE_S)
        } else {
            button::text("Reset All Overrides").padding(SPACE_S)
        };

        // Combine everything
        container(
            column![
                header,
                rename_section,
                divider::horizontal::default(),
                scrollable(plugin_list).height(Length::Fixed(200.0)),
                divider::horizontal::default(),
                footer,
            ]
            .spacing(SPACE_S),
        )
        .padding(SPACE_M)
        .into()
    }

    /// RemoteDesktop settings view with quality, FPS, and resolution controls
    fn remotedesktop_settings_view(
        &self,
        device_id: &str,
        settings: &dbus_client::RemoteDesktopSettings,
    ) -> Element<'_, Message> {
        use cosmic::widget::{horizontal_space, radio};

        // Header with close button
        let header = row![
            cosmic::widget::text::body("Remote Desktop Settings"),
            horizontal_space(),
            button::icon(icon::from_name("window-close-symbolic").size(14))
                .on_press(Message::CloseRemoteDesktopSettings)
                .padding(SPACE_XXS)
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
            cosmic::widget::dropdown(&["Low", "Medium", "High"], Some(quality_idx), {
                let device_id = device_id.to_string();
                move |idx| {
                    let quality = match idx {
                        0 => "low",
                        1 => "medium",
                        2 => "high",
                        _ => "medium",
                    }
                    .to_string();
                    Message::UpdateRemoteDesktopQuality(device_id.clone(), quality)
                }
            })
        ]
        .spacing(SPACE_S)
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
            cosmic::widget::dropdown(&["15 FPS", "30 FPS", "60 FPS"], Some(fps_idx), {
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
            })
        ]
        .spacing(SPACE_S)
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
                    move |_| {
                        Message::UpdateRemoteDesktopResolution(
                            device_id.clone(),
                            "native".to_string(),
                        )
                    }
                }
            ),
            radio(
                "Custom Resolution",
                "custom",
                Some(settings.resolution_mode.as_str()).filter(|_| !is_native),
                {
                    let device_id = device_id.to_string();
                    move |_| {
                        Message::UpdateRemoteDesktopResolution(
                            device_id.clone(),
                            "custom".to_string(),
                        )
                    }
                }
            ),
        ]
        .spacing(SPACE_XXS);

        let resolution_row = row![
            text("Resolution:").width(Length::Fixed(120.0)),
            resolution_radios
        ]
        .spacing(SPACE_S)
        .align_y(cosmic::iced::Alignment::Start);

        // Build content
        let mut content = column![
            header,
            divider::horizontal::default(),
            quality_row,
            fps_row,
            resolution_row,
        ]
        .spacing(SPACE_M);

        // Add custom resolution inputs if mode is "custom"
        if settings.resolution_mode == "custom" {
            let width_input =
                cosmic::widget::text_input("Width (e.g. 1920)", &self.remotedesktop_width_input)
                    .on_input({
                        let device_id = device_id.to_string();
                        move |s| Message::UpdateRemoteDesktopCustomWidth(device_id.clone(), s)
                    });

            let height_input =
                cosmic::widget::text_input("Height (e.g. 1080)", &self.remotedesktop_height_input)
                    .on_input({
                        let device_id = device_id.to_string();
                        move |s| Message::UpdateRemoteDesktopCustomHeight(device_id.clone(), s)
                    });

            let inputs_row = row![
                column![cosmic::widget::text::caption("Width"), width_input]
                    .spacing(SPACE_XXS)
                    .width(Length::FillPortion(1)),
                column![cosmic::widget::text::caption("Height"), height_input]
                    .spacing(SPACE_XXS)
                    .width(Length::FillPortion(1)),
            ]
            .spacing(SPACE_M);

            content = content.push(inputs_row);
        }

        content = content.push(divider::horizontal::default());

        // Error message if any
        if let Some(error) = &self.remotedesktop_error {
            content = content.push(
                text(error)
                    .size(12)
                    .class(theme::Text::Color(Color::from_rgb(0.9, 0.2, 0.2))),
            );
        }

        // Apply button (disabled if error)
        let mut apply_btn = button::text("Apply Settings").padding(SPACE_S);

        if self.remotedesktop_error.is_none() {
            apply_btn =
                apply_btn.on_press(Message::SaveRemoteDesktopSettings(device_id.to_string()));
        }

        content = content.push(apply_btn);

        container(content).padding(SPACE_M).into()
    }
    fn transfers_view(&self) -> Element<'_, Message> {
        if self.active_transfers.is_empty() {
            return Element::from(cosmic::widget::Space::new(0, 0));
        }

        let mut transfers_col = column![text("Active Transfers")
            .size(14)
            .class(theme::Text::Color(Color::from_rgb(0.5, 0.5, 1.0))),]
        .spacing(SPACE_S);

        for (_id, state) in &self.active_transfers {
            let progress = if state.total > 0 {
                (state.current as f32 / state.total as f32) * 100.0
            } else {
                0.0
            };

            let label = format!(
                "{} {} ({:.0}%)",
                if state.direction == "sending" {
                    "Sending"
                } else {
                    "Receiving"
                },
                state.filename,
                progress
            );

            transfers_col = transfers_col.push(
                column![
                    cosmic::widget::text::caption(label),
                    progress_bar(0.0..=100.0, progress).height(Length::Fixed(6.0))
                ]
                .spacing(SPACE_XXS),
            );
        }

        container(transfers_col)
            .padding(SPACE_M)
            .width(Length::Fill)
            .into()
    }
}

/// Creates a small icon button with tooltip for device quick actions
fn action_button_with_tooltip(
    icon_name: &str,
    tooltip_text: &'static str,
    message: Message,
) -> Element<'static, Message> {
    cosmic::widget::tooltip(
        button::icon(icon::from_name(icon_name).size(16))
            .on_press(message)
            .padding(SPACE_XS),
        tooltip_text,
        cosmic::widget::tooltip::Position::Bottom,
    )
    .into()
}

/// Creates a small icon button for device quick actions (ping, send file, etc.)
/// @deprecated Use action_button_with_tooltip instead
fn action_button(icon_name: &str, message: Message) -> Element<'static, Message> {
    button::icon(icon::from_name(icon_name).size(16))
        .on_press(message)
        .padding(SPACE_XS)
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

/// Returns a styled text element with color-coded status text
fn connection_status_styled_text<'a>(
    connection_state: ConnectionState,
    pairing_status: PairingStatus,
) -> Element<'a, Message> {
    let status_text = connection_status_text(connection_state, pairing_status);

    // Apply color based on connection state using theme-aware colors
    let color = match connection_state {
        ConnectionState::Connected => Color::from_rgb(0.2, 0.8, 0.2), // Green - Success
        ConnectionState::Failed => Color::from_rgb(0.9, 0.2, 0.2),    // Red - Danger
        ConnectionState::Connecting => Color::from_rgb(0.9, 0.7, 0.2), // Yellow/Orange - Warning
        ConnectionState::Disconnected => Color::from_rgb(0.5, 0.5, 0.5), // Gray - Muted
    };

    cosmic::widget::text::caption(status_text)
        .class(theme::Text::Color(color))
        .into()
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
    if count == 1 {
        ""
    } else {
        "s"
    }
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
