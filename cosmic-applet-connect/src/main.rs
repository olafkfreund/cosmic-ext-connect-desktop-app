mod dbus_client;
mod messages;
mod onboarding_config;
mod pinned_devices_config;
mod state;
mod views;

use std::collections::HashMap;

use messages::{Message, NotificationType, OperationType};
use state::{
    ActiveScreenShare, AppNotification, CameraStats, ConversationSummary, DeviceState, FocusTarget,
    HistoryEvent, ReceivedFile, SmsMessageDisplay, SystemInfo, TransferState, ViewMode,
    MAX_DISPLAYED_HISTORY_ITEMS, MAX_RECEIVED_FILES_HISTORY,
};

use cosmic::{
    app::{Core, Task},
    iced::{
        widget::{column, container, row, text},
        window, Color, Length, Rectangle,
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

use dbus_client::DbusClient;

// COSMIC Design System: Use theme::active().cosmic().space_*() for spacing
// Available: space_none(), space_xxxs(), space_xxs(), space_xs(), space_s(), space_m(), space_l(), space_xl()
// This ensures spacing adapts to user theme preferences and accessibility settings

// Icon sizes - standard design tokens for icons
const ICON_XS: u16 = 12;   // Small metadata icons
const ICON_S: u16 = 16;    // Standard button/action icon
const ICON_M: u16 = 24;    // Medium icons (file icons in lists)
const ICON_L: u16 = 32;    // Large icons (device icons)
const ICON_XL: u16 = 48;   // Hero/Empty state icons
const ICON_14: u16 = 14;   // Compact icons (close buttons)

// Theme spacing helper functions - reduces verbosity throughout the codebase
// These wrap theme::active().cosmic().space_*() for cleaner code

/// No spacing (0)
fn space_none() -> u16 {
    theme::active().cosmic().space_none()
}

/// Minimal spacing (~4px)
fn space_xxxs() -> u16 {
    theme::active().cosmic().space_xxxs()
}

/// Tight spacing (~8px)
fn space_xxs() -> u16 {
    theme::active().cosmic().space_xxs()
}

/// Extra small spacing (~12px)
fn space_xs() -> u16 {
    theme::active().cosmic().space_xs()
}

/// Small spacing (~16px)
fn space_s() -> u16 {
    theme::active().cosmic().space_s()
}

/// Medium spacing (~24px)
fn space_m() -> u16 {
    theme::active().cosmic().space_m()
}

/// Large spacing (~32px) - for major sections
#[allow(dead_code)]
fn space_l() -> u16 {
    theme::active().cosmic().space_l()
}

/// Extra large spacing (~48px) - for hero elements
#[allow(dead_code)]
fn space_xl() -> u16 {
    theme::active().cosmic().space_xl()
}

// Float variants for use with Padding::new() and arithmetic operations

/// Tight spacing as f32
fn space_xxs_f32() -> f32 {
    f32::from(theme::active().cosmic().space_xxs())
}

/// Convert cosmic theme Srgba color to iced Color
fn theme_color_to_iced(srgba: cosmic::theme::CosmicColor) -> Color {
    Color::from_rgba(srgba.red, srgba.green, srgba.blue, srgba.alpha)
}

/// Get theme success color (green)
fn theme_success_color() -> Color {
    theme_color_to_iced(cosmic::theme::active().cosmic().success.base)
}

/// Get theme destructive/error color (red)
fn theme_destructive_color() -> Color {
    theme_color_to_iced(cosmic::theme::active().cosmic().destructive.base)
}

/// Get theme warning color (yellow/orange)
fn theme_warning_color() -> Color {
    theme_color_to_iced(cosmic::theme::active().cosmic().warning.base)
}

/// Get theme muted text color (gray)
fn theme_muted_color() -> Color {
    // Use the "on" color from a neutral component with reduced alpha
    let theme = cosmic::theme::active();
    let cosmic = theme.cosmic();
    let base = cosmic.background.component.on;
    Color::from_rgba(base.red, base.green, base.blue, 0.6)
}

/// Get theme accent color (blue)
fn theme_accent_color() -> Color {
    theme_color_to_iced(cosmic::theme::active().cosmic().accent.base)
}

/// Check if v4l2loopback kernel module is loaded and device exists
fn check_v4l2loopback() -> bool {
    // Check if module is loaded
    if let Ok(modules) = std::fs::read_to_string("/proc/modules") {
        if !modules.contains("v4l2loopback") {
            tracing::warn!("v4l2loopback kernel module not loaded");
            return false;
        }
    } else {
        tracing::warn!("Could not read /proc/modules");
        return false;
    }

    // Check if device exists (default is /dev/video10)
    let device_exists = std::path::Path::new("/dev/video10").exists();
    if !device_exists {
        tracing::warn!("/dev/video10 does not exist");
    }
    device_exists
}

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
#[allow(dead_code)]
struct PluginMetadata {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    icon: &'static str,
    capability: &'static str,
}

/// Available plugins with their metadata
#[allow(dead_code)]
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
    PluginMetadata {
        id: "filesync",
        name: "File Synchronization",
        description: "Sync folders with device",
        icon: "folder-sync-symbolic",
        capability: "cconnect.filesync",
    },
    PluginMetadata {
        id: "runcommand",
        name: "Run Commands",
        description: "Execute remote commands",
        icon: "system-run-symbolic",
        capability: "cconnect.runcommand",
    },
    PluginMetadata {
        id: "contacts",
        name: "Contacts",
        description: "Sync contacts",
        icon: "x-office-address-book-symbolic",
        capability: "cconnect.contacts.request_all_uids_timestamps",
    },
    PluginMetadata {
        id: "networkshare",
        name: "Network Share",
        description: "Mount remote filesystem",
        icon: "folder-remote-symbolic",
        capability: "kdeconnect.sftp",
    },
    PluginMetadata {
        id: "camera",
        name: "Camera/Webcam",
        description: "Use phone camera as webcam",
        icon: "camera-web-symbolic",
        capability: "cconnect.camera",
    },
    PluginMetadata {
        id: "systemvolume",
        name: "Volume Control",
        description: "Control device volume",
        icon: "audio-volume-high-symbolic",
        capability: "cconnect.systemvolume",
    },
    PluginMetadata {
        id: "systemmonitor",
        name: "System Monitor",
        description: "View device system info",
        icon: "utilities-system-monitor-symbolic",
        capability: "cconnect.systemmonitor",
    },
    PluginMetadata {
        id: "screenshot",
        name: "Screenshot",
        description: "Take device screenshots",
        icon: "applets-screenshooter-symbolic",
        capability: "cconnect.screenshot",
    },
    PluginMetadata {
        id: "lock",
        name: "Lock Screen",
        description: "Lock device remotely",
        icon: "system-lock-screen-symbolic",
        capability: "cconnect.lock",
    },
    PluginMetadata {
        id: "power",
        name: "Power Control",
        description: "Shutdown/hibernate device",
        icon: "system-shutdown-symbolic",
        capability: "cconnect.power",
    },
    PluginMetadata {
        id: "wol",
        name: "Wake on LAN",
        description: "Wake device remotely",
        icon: "network-wired-symbolic",
        capability: "cconnect.wol",
    },
    PluginMetadata {
        id: "telephony",
        name: "Phone Calls",
        description: "Handle phone calls",
        icon: "call-start-symbolic",
        capability: "cconnect.telephony",
    },
    PluginMetadata {
        id: "sms",
        name: "SMS Messages",
        description: "Send and receive SMS",
        icon: "mail-message-new-symbolic",
        capability: "cconnect.sms.messages",
    },
    PluginMetadata {
        id: "audiostream",
        name: "Audio Stream",
        description: "Stream audio to/from device",
        icon: "audio-speakers-symbolic",
        capability: "cconnect.audiostream",
    },
    PluginMetadata {
        id: "presenter",
        name: "Presentation Mode",
        description: "Use device as presentation remote",
        icon: "x-office-presentation-symbolic",
        capability: "cconnect.presenter",
    },
];


struct CConnectApplet {
    core: Core,
    popup: Option<window::Id>,
    devices: Vec<DeviceState>,
    #[allow(dead_code)]
    dbus_client: Option<DbusClient>,
    mpris_players: Vec<String>,
    selected_player: Option<String>,
    // Device configs (used for renaming)
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
    received_files_history: Vec<ReceivedFile>,
    // Renaming state
    renaming_device: Option<String>,
    nickname_input: String,
    // History
    history: Vec<HistoryEvent>,
    view_mode: ViewMode,
    // Scanning state
    // Scanning state
    scanning: bool,
    loading_battery: bool,
    // File Sync state
    sync_folders: HashMap<String, Vec<dbus_client::SyncFolderInfo>>,
    add_sync_folder_device: Option<String>,
    add_sync_folder_path: String,
    add_sync_folder_id: String,
    add_sync_folder_strategy: String,
    file_sync_settings_device: Option<String>,
    // Run Command state
    run_commands: HashMap<String, HashMap<String, dbus_client::RunCommand>>,
    // State for run command form
    add_run_command_device: Option<String>,
    add_run_command_name: String,
    add_run_command_cmd: String,
    run_command_settings_device: Option<String>,
    // Generic notification state (Error, Success, Info)
    notification: Option<AppNotification>,
    // Loading state
    pending_operations: std::collections::HashSet<(String, OperationType)>,
    // Help dialog state
    show_keyboard_shortcuts_help: bool,
    // Animation state
    notification_progress: f32,
    // Connection status to daemon
    daemon_connected: bool,
    // Keyboard navigation state
    focus_target: FocusTarget,
    // Drag-and-drop state
    drag_hover_device: Option<String>, // device_id being hovered with files
    dragging_files: bool,              // whether files are being dragged over window
    // Context menu state
    context_menu_device: Option<String>, // device_id with open context menu
    context_menu_transfer: Option<String>, // transfer_id with open context menu
    context_menu_mpris: bool,            // whether MPRIS context menu is open
    // Screen share state
    active_screen_share: Option<ActiveScreenShare>, // Currently active screen share session
    // Audio stream state
    audio_streaming_devices: std::collections::HashSet<String>, // device_ids currently streaming audio
    // Presenter mode state
    presenter_mode_devices: std::collections::HashSet<String>, // device_ids in presenter mode
    // Pinned devices config
    pinned_devices_config: pinned_devices_config::PinnedDevicesConfig,
    // Camera state
    camera_settings_device: Option<String>, // device_id showing Camera settings
    camera_stats: HashMap<String, CameraStats>, // device_id -> stream statistics
    v4l2loopback_available: bool,           // Whether v4l2loopback kernel module is loaded
    // Onboarding state
    #[allow(dead_code)]
    show_onboarding: bool,
    #[allow(dead_code)]
    onboarding_step: u8,
    last_screen_share_stats_poll: Option<std::time::Instant>, // Last time we polled screen share stats
    // Settings window state
    settings_window: Option<(window::Id, String)>, // (window_id, device_id)
    // App Continuity (Open plugin) state
    open_url_dialog_device: Option<String>, // device_id showing open URL dialog
    open_url_input: String,                 // URL input field
    // SMS dialog state
    sms_dialog_device: Option<String>, // device_id showing SMS dialog
    sms_phone_number_input: String,    // Phone number input field
    sms_message_input: String,         // Message body input field
    // Conversations state
    conversations_device: Option<String>,  // device_id showing conversations list
    active_conversation: Option<(String, i64)>, // (device_id, thread_id)
    conversations_cache: HashMap<String, Vec<ConversationSummary>>, // device_id -> summaries
    conversation_messages: HashMap<(String, i64), Vec<SmsMessageDisplay>>, // (device_id, thread_id) -> messages
    // System Monitor state
    system_info: HashMap<String, SystemInfo>, // device_id -> system information
    // Screenshot state
    screenshots: HashMap<String, Vec<u8>>, // device_id -> last screenshot image data
    // Destructive action confirmation
    pending_destructive_confirmation: Option<PendingDestructiveAction>,
}

/// Pending destructive action awaiting user confirmation
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum PendingDestructiveAction {
    UnpairDevice(String),  // device_id
    DismissDevice(String), // device_id
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

/// Opens a file picker dialog and returns device_id with selected file paths
async fn open_file_picker(device_id: String, multiple: bool) -> Option<(String, Vec<String>)> {
    use ashpd::desktop::file_chooser::OpenFileRequest;

    let title = if multiple {
        "Select files to send"
    } else {
        "Select file to send"
    };

    let response = OpenFileRequest::default()
        .title(title)
        .modal(true)
        .multiple(multiple)
        .send()
        .await
        .ok()?
        .response()
        .ok()?;

    let paths: Vec<String> = response
        .uris()
        .iter()
        .map(|uri| uri.path().to_string())
        .collect();

    (!paths.is_empty()).then_some((device_id, paths))
}

/// Gets text from the system clipboard
fn get_clipboard_text() -> Option<String> {
    arboard::Clipboard::new()
        .ok()
        .and_then(|mut clipboard| clipboard.get_text().ok())
}

// Helper to perform device operation with completion event
fn device_operation_with_completion<F, Fut>(
    device_id: String,
    op_type: OperationType,
    operation: F,
) -> Task<Message>
where
    F: FnOnce(DbusClient, String) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let id_cl = device_id.clone();
    let op_cl = op_type;
    Task::perform(
        async move {
            if let Ok((client, _)) = DbusClient::connect().await {
                operation(client, id_cl).await
            } else {
                Err(anyhow::anyhow!("Failed to connect to daemon"))
            }
        },
        move |result| match result {
            Ok(_) => cosmic::Action::App(Message::OperationCompleted(device_id.clone(), op_type)),
            Err(e) => {
                tracing::error!("Device operation {:?} failed: {}", op_cl, e);
                cosmic::Action::App(Message::OperationFailed(
                    device_id.clone(),
                    op_type,
                    e.to_string(),
                ))
            }
        },
    )
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

/// Creates a task for screen share control operations (pause/resume)
///
/// On success, triggers a Tick. On failure, shows an error notification.
fn screen_share_control_task<F, Fut>(
    device_id: String,
    action_name: &'static str,
    operation: F,
) -> Task<Message>
where
    F: FnOnce(DbusClient, String) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
{
    Task::perform(
        async move {
            let (client, _) = DbusClient::connect()
                .await
                .map_err(|e| anyhow::anyhow!("DBus connection failed: {}", e))?;
            operation(client, device_id).await
        },
        move |result| {
            if let Err(e) = result {
                tracing::error!("Failed to {} screen share: {}", action_name, e);
                cosmic::Action::App(Message::ShowNotification(
                    format!("Failed to {} screen share", action_name),
                    NotificationType::Error,
                    None,
                ))
            } else {
                cosmic::Action::App(Message::Tick(std::time::Instant::now()))
            }
        },
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
        // Check if onboarding has been completed
        let show_onboarding = match onboarding_config::AppletConfig::load() {
            Ok(config) => !config.onboarding_complete,
            Err(e) => {
                tracing::warn!("Failed to load applet config: {}, showing onboarding", e);
                true
            }
        };

        // Load pinned devices config
        let pinned_devices_config = match pinned_devices_config::PinnedDevicesConfig::load() {
            Ok(config) => config,
            Err(e) => {
                tracing::warn!("Failed to load pinned devices config: {}, using default", e);
                pinned_devices_config::PinnedDevicesConfig::default()
            }
        };

        let app = Self {
            core,
            popup: None,
            devices: Vec::new(),
            dbus_client: None,
            mpris_players: Vec::new(),
            selected_player: None,
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
            received_files_history: Vec::new(),
            renaming_device: None,
            nickname_input: String::new(),
            history: Vec::new(),
            view_mode: ViewMode::Devices,
            scanning: false,
            loading_battery: false,
            sync_folders: HashMap::new(),
            add_sync_folder_device: None,
            add_sync_folder_path: String::new(),
            add_sync_folder_id: String::new(),
            add_sync_folder_strategy: "last_modified_wins".to_string(),
            file_sync_settings_device: None,
            run_commands: HashMap::new(),
            add_run_command_device: None,
            add_run_command_name: String::new(),
            add_run_command_cmd: String::new(),
            run_command_settings_device: None,
            notification: None,
            pending_operations: std::collections::HashSet::new(),
            notification_progress: 0.0,
            show_keyboard_shortcuts_help: false,
            daemon_connected: true,
            focus_target: FocusTarget::None,
            drag_hover_device: None,
            dragging_files: false,
            context_menu_device: None,
            context_menu_transfer: None,
            context_menu_mpris: false,
            active_screen_share: None,
            audio_streaming_devices: std::collections::HashSet::new(),
            presenter_mode_devices: std::collections::HashSet::new(),
            pinned_devices_config,
            // Camera state initialization
            camera_settings_device: None,
            camera_stats: HashMap::new(),
            v4l2loopback_available: check_v4l2loopback(),
            show_onboarding,
            onboarding_step: 0,
            last_screen_share_stats_poll: None,
            settings_window: None,
            open_url_dialog_device: None,
            open_url_input: String::new(),
            sms_dialog_device: None,
            sms_phone_number_input: String::new(),
            sms_message_input: String::new(),
            conversations_device: None,
            active_conversation: None,
            conversations_cache: HashMap::new(),
            conversation_messages: HashMap::new(),
            system_info: HashMap::new(),
            screenshots: HashMap::new(),
            pending_destructive_confirmation: None,
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
            Message::Loop(inner) => {
                self.update(*inner)
            }
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
            Message::Tick(_) => self.handle_tick(),
            Message::DeviceEvent(event) => self.handle_device_event(event),
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
                self.loading_battery = true;
                Task::perform(fetch_battery_statuses(connected_ids), |statuses| {
                    cosmic::Action::App(Message::BatteryStatusesUpdated(statuses))
                })
            }
            Message::BatteryStatusesUpdated(statuses) => {
                self.loading_battery = false;
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
                let id = device_id.clone();
                Task::batch(vec![
                    Task::done(cosmic::Action::App(Message::OperationStarted(
                        device_id.clone(),
                        OperationType::Pair,
                    ))),
                    device_operation_with_completion(
                        device_id,
                        OperationType::Pair,
                        move |client, _| async move { client.pair_device(&id).await },
                    ),
                ])
            }
            Message::ConfirmUnpairDevice(device_id) => {
                let device_name = self
                    .devices
                    .iter()
                    .find(|d| d.device.id() == device_id)
                    .map(|d| d.device.name().to_string())
                    .unwrap_or_else(|| device_id.clone());
                self.pending_destructive_confirmation =
                    Some(PendingDestructiveAction::UnpairDevice(device_id.clone()));
                self.notification = Some(AppNotification {
                    message: format!("Unpair \"{}\"? You'll need to pair again.", device_name),
                    kind: NotificationType::Info,
                    action: Some((
                        "Unpair".to_string(),
                        Box::new(Message::UnpairDevice(device_id)),
                    )),
                });
                Task::none()
            }
            Message::UnpairDevice(device_id) => {
                self.pending_destructive_confirmation = None;
                let id = device_id.clone();
                Task::batch(vec![
                    Task::done(cosmic::Action::App(Message::OperationStarted(
                        device_id.clone(),
                        OperationType::Unpair,
                    ))),
                    device_operation_with_completion(
                        device_id,
                        OperationType::Unpair,
                        move |client, _| async move { client.unpair_device(&id).await },
                    ),
                ])
            }
            Message::ConfirmDismissDevice(device_id) => {
                let device_name = self
                    .devices
                    .iter()
                    .find(|d| d.device.id() == device_id)
                    .map(|d| d.device.name().to_string())
                    .unwrap_or_else(|| device_id.clone());
                self.pending_destructive_confirmation =
                    Some(PendingDestructiveAction::DismissDevice(device_id.clone()));
                self.notification = Some(AppNotification {
                    message: format!(
                        "Forget \"{}\"? This device will be removed permanently.",
                        device_name
                    ),
                    kind: NotificationType::Info,
                    action: Some((
                        "Forget".to_string(),
                        Box::new(Message::DismissDevice(device_id)),
                    )),
                });
                Task::none()
            }
            Message::CancelDestructiveConfirmation => {
                self.pending_destructive_confirmation = None;
                self.notification = None;
                Task::none()
            }
            Message::DismissDevice(device_id) => {
                self.pending_destructive_confirmation = None;
                tracing::info!("User requested dismiss device: {}", device_id);
                Task::perform(
                    async move {
                        let (client, _) = DbusClient::connect()
                            .await
                            .map_err(|e| anyhow::anyhow!("DBus connection failed: {}", e))?;
                        client.forget_device(&device_id).await
                    },
                    |result| {
                        if let Err(e) = result {
                            tracing::error!("Failed to dismiss device: {}", e);
                            cosmic::Action::App(Message::ShowNotification(
                                "Failed to dismiss device".to_string(),
                                NotificationType::Error,
                                None,
                            ))
                        } else {
                            cosmic::Action::App(Message::RefreshDevices)
                        }
                    },
                )
            }
            Message::RefreshDevices => fetch_devices_task(),
            Message::SendPing(device_id) => {
                let id = device_id.clone();
                Task::batch(vec![
                    Task::done(cosmic::Action::App(Message::OperationStarted(
                        device_id.clone(),
                        OperationType::Ping,
                    ))),
                    device_operation_with_completion(
                        device_id,
                        OperationType::Ping,
                        move |client, _| async move { client.send_ping(&id, "Ping from COSMIC").await },
                    ),
                ])
            }
            Message::SendFile(device_id) => {
                tracing::info!("Opening file picker for device: {}", device_id);
                Task::perform(open_file_picker(device_id, false), |result| {
                    let Some((device_id, mut paths)) = result else {
                        tracing::debug!("File picker cancelled or no file selected");
                        return cosmic::Action::App(Message::RefreshDevices);
                    };
                    // Single file selection returns vector with one element
                    match paths.pop() {
                        Some(path) => cosmic::Action::App(Message::FileSelected(device_id, path)),
                        None => cosmic::Action::App(Message::RefreshDevices),
                    }
                })
            }
            Message::SendFiles(device_id) => {
                tracing::info!("Opening multi-file picker for device: {}", device_id);
                Task::perform(open_file_picker(device_id, true), |result| {
                    let Some((device_id, paths)) = result else {
                        tracing::debug!("File picker cancelled or no files selected");
                        return cosmic::Action::App(Message::RefreshDevices);
                    };
                    cosmic::Action::App(Message::FilesSelected(device_id, paths))
                })
            }
            Message::FileSelected(device_id, file_path) => {
                tracing::info!("Sending file {} to device: {}", file_path, device_id);
                device_operation_task(device_id, "share file", move |client, id| async move {
                    client.share_file(&id, &file_path).await
                })
            }
            Message::FilesSelected(device_id, file_paths) => {
                tracing::info!(
                    "Sending {} files to device: {}",
                    file_paths.len(),
                    device_id
                );

                let tasks = file_paths.into_iter().map(|path| {
                    let id = device_id.clone();
                    device_operation_task(id, "share file", move |client, device_id| async move {
                        client.share_file(&device_id, &path).await
                    })
                });

                Task::batch(tasks)
            }
            Message::FindPhone(device_id) => {
                let id = device_id.clone();
                Task::batch(vec![
                    Task::done(cosmic::Action::App(Message::OperationStarted(
                        device_id.clone(),
                        OperationType::FindPhone,
                    ))),
                    device_operation_with_completion(
                        device_id,
                        OperationType::FindPhone,
                        move |client, _| async move { client.find_phone(&id).await },
                    ),
                ])
            }
            Message::ShareText(device_id) => {
                tracing::info!("Share text to device: {}", device_id);
                match get_clipboard_text() {
                    Some(text) => {
                        let id = device_id.clone();
                        Task::batch(vec![
                            Task::done(cosmic::Action::App(Message::OperationStarted(
                                device_id.clone(),
                                OperationType::ShareText,
                            ))),
                            device_operation_with_completion(
                                device_id,
                                OperationType::ShareText,
                                move |client, _| async move { client.share_text(&id, &text).await },
                            ),
                        ])
                    }
                    None => {
                        tracing::warn!("No text in clipboard to share");
                        Task::done(cosmic::Action::App(Message::ShowNotification(
                            "Clipboard is empty".into(),
                            NotificationType::Error,
                            None,
                        )))
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
                        let id = device_id.clone();
                        Task::batch(vec![
                            Task::done(cosmic::Action::App(Message::OperationStarted(
                                device_id.clone(),
                                OperationType::ShareUrl,
                            ))),
                            device_operation_with_completion(
                                device_id,
                                OperationType::ShareUrl,
                                move |client, _| async move { client.share_url(&id, &text).await },
                            ),
                        ])
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
            Message::ShowOpenUrlDialog(device_id) => {
                tracing::info!("Show open URL dialog for device: {}", device_id);
                self.open_url_dialog_device = Some(device_id);
                // Pre-fill with clipboard if it's a URL
                if let Some(text) = get_clipboard_text() {
                    if text.starts_with("http://")
                        || text.starts_with("https://")
                        || text.starts_with("tel:")
                        || text.starts_with("mailto:")
                    {
                        self.open_url_input = text;
                    }
                }
                Task::none()
            }
            Message::OpenUrlInput(input) => {
                self.open_url_input = input;
                Task::none()
            }
            Message::OpenOnPhone(device_id, url) => {
                tracing::info!("Opening URL on phone: {} -> {}", url, device_id);

                // Clear dialog
                self.open_url_dialog_device = None;
                self.open_url_input.clear();

                // Send URL to device
                Task::perform(
                    async move {
                        let (client, _) = DbusClient::connect()
                            .await
                            .map_err(|e| format!("Failed to connect to daemon: {}", e))?;

                        client
                            .open_on_phone(&url)
                            .await
                            .map_err(|e| format!("Failed to open URL: {}", e))
                    },
                    |result| {
                        cosmic::Action::App(match result {
                            Ok(request_id) => {
                                tracing::info!("URL open request sent: {}", request_id);
                                Message::ShowNotification(
                                    "URL sent to device".to_string(),
                                    NotificationType::Success,
                                    None,
                                )
                            }
                            Err(err) => {
                                tracing::error!("Failed to open URL: {}", err);
                                Message::ShowNotification(err, NotificationType::Error, None)
                            }
                        })
                    },
                )
            }
            Message::CancelOpenUrlDialog => {
                tracing::debug!("Cancel open URL dialog");
                self.open_url_dialog_device = None;
                self.open_url_input.clear();
                Task::none()
            }
            Message::MuteCall(device_id) => {
                tracing::info!("Muting call on device: {}", device_id);
                Task::perform(
                    async move {
                        match DbusClient::connect().await {
                            Ok((client, _)) => {
                                if let Err(e) = client.mute_call(&device_id).await {
                                    tracing::error!("Failed to mute call: {:?}", e);
                                    return Message::OperationFailed(
                                        device_id.clone(),
                                        OperationType::MuteCall,
                                        format!("Failed to mute call: {}", e),
                                    );
                                }
                                Message::OperationSucceeded(
                                    device_id,
                                    OperationType::MuteCall,
                                    "Call muted".to_string(),
                                )
                            }
                            Err(e) => {
                                tracing::error!("Failed to connect to daemon: {:?}", e);
                                Message::OperationFailed(
                                    device_id.clone(),
                                    OperationType::MuteCall,
                                    format!("Failed to connect: {}", e),
                                )
                            }
                        }
                    },
                    cosmic::Action::App,
                )
            }
            Message::ShowSmsDialog(device_id) => {
                tracing::info!("Show SMS dialog for device: {}", device_id);
                self.sms_dialog_device = Some(device_id);
                Task::none()
            }
            Message::CancelSmsDialog => {
                tracing::debug!("Cancel SMS dialog");
                self.sms_dialog_device = None;
                self.sms_phone_number_input.clear();
                self.sms_message_input.clear();
                Task::none()
            }
            Message::UpdateSmsPhoneNumberInput(input) => {
                self.sms_phone_number_input = input;
                Task::none()
            }
            Message::UpdateSmsMessageInput(input) => {
                self.sms_message_input = input;
                Task::none()
            }
            Message::SendSms(device_id, phone_number, message) => {
                tracing::info!("Sending SMS via device {} to {}", device_id, phone_number);

                // Clear dialog
                self.sms_dialog_device = None;
                self.sms_phone_number_input.clear();
                self.sms_message_input.clear();

                // Send SMS
                Task::perform(
                    async move {
                        match DbusClient::connect().await {
                            Ok((client, _)) => {
                                if let Err(e) =
                                    client.send_sms(&device_id, &phone_number, &message).await
                                {
                                    tracing::error!("Failed to send SMS: {:?}", e);
                                    return Message::OperationFailed(
                                        device_id.clone(),
                                        OperationType::SendSms,
                                        format!("Failed to send SMS: {}", e),
                                    );
                                }
                                Message::OperationSucceeded(
                                    device_id,
                                    OperationType::SendSms,
                                    format!("SMS sent to {}", phone_number),
                                )
                            }
                            Err(e) => {
                                tracing::error!("Failed to connect to daemon: {:?}", e);
                                Message::OperationFailed(
                                    device_id.clone(),
                                    OperationType::SendSms,
                                    format!("Failed to connect: {}", e),
                                )
                            }
                        }
                    },
                    cosmic::Action::App,
                )
            }
            Message::ShowConversations(device_id) => {
                tracing::info!("Show conversations for device: {}", device_id);
                self.conversations_device = Some(device_id.clone());
                self.active_conversation = None;
                // Request conversation list from phone
                Task::perform(
                    async move {
                        if let Ok((client, _)) = DbusClient::connect().await {
                            if let Err(e) = client.request_conversations(&device_id).await {
                                tracing::error!("Failed to request conversations: {:?}", e);
                            }
                        }
                        Message::RefreshDevices
                    },
                    cosmic::Action::App,
                )
            }
            Message::CloseConversations => {
                self.conversations_device = None;
                self.active_conversation = None;
                self.sms_message_input.clear();
                Task::none()
            }
            Message::SelectConversation(device_id, thread_id) => {
                tracing::info!(
                    "Select conversation {} for device {}",
                    thread_id,
                    device_id
                );
                self.active_conversation = Some((device_id.clone(), thread_id));
                self.sms_message_input.clear();
                // Request conversation messages from phone
                Task::perform(
                    async move {
                        if let Ok((client, _)) = DbusClient::connect().await {
                            if let Err(e) =
                                client.request_conversation(&device_id, thread_id).await
                            {
                                tracing::error!("Failed to request conversation: {:?}", e);
                            }
                        }
                        Message::RefreshDevices
                    },
                    cosmic::Action::App,
                )
            }
            Message::CloseConversation => {
                self.active_conversation = None;
                self.sms_message_input.clear();
                Task::none()
            }
            Message::ConversationsLoaded(device_id, summaries) => {
                tracing::info!(
                    "Loaded {} conversations for {}",
                    summaries.len(),
                    device_id
                );
                self.conversations_cache.insert(device_id, summaries);
                Task::none()
            }
            Message::ConversationMessagesLoaded(device_id, thread_id, messages) => {
                tracing::info!(
                    "Loaded {} messages for thread {} on {}",
                    messages.len(),
                    thread_id,
                    device_id
                );
                self.conversation_messages
                    .insert((device_id, thread_id), messages);
                Task::none()
            }
            Message::RequestBatteryUpdate(device_id) => {
                let id = device_id.clone();
                Task::batch(vec![
                    Task::done(cosmic::Action::App(Message::OperationStarted(
                        device_id.clone(),
                        OperationType::Battery,
                    ))),
                    device_operation_with_completion(
                        device_id,
                        OperationType::Battery,
                        move |client, _| async move { client.request_battery_update(&id).await },
                    ),
                ])
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
                self.handle_mpris_player_selected(player)
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
            Message::MprisControl(player, action) => self.handle_mpris_control(player, action),

            // Camera streaming controls
            Message::ToggleCameraStreaming(device_id) => {
                let is_streaming = self
                    .camera_stats
                    .get(&device_id)
                    .is_some_and(|s| s.is_streaming);

                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    let device_id_clone = device_id.clone();

                    Task::perform(
                        async move {
                            if is_streaming {
                                client.stop_camera_streaming(&device_id_clone).await
                            } else {
                                client.start_camera_streaming(&device_id_clone).await
                            }
                        },
                        move |result| {
                            if let Err(e) = result {
                                tracing::error!("Failed to toggle camera streaming: {}", e);
                            }
                            cosmic::Action::App(Message::RefreshDevices)
                        },
                    )
                } else {
                    tracing::warn!("DBus client not available for camera streaming");
                    Task::none()
                }
            }
            Message::SelectCamera(device_id, camera_id) => {
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    let device_id_clone = device_id.clone();

                    Task::perform(
                        async move { client.select_camera(&device_id_clone, camera_id).await },
                        move |result| {
                            if let Err(e) = result {
                                tracing::error!("Failed to select camera: {}", e);
                            }
                            cosmic::Action::App(Message::RefreshDevices)
                        },
                    )
                } else {
                    tracing::warn!("DBus client not available for camera selection");
                    Task::none()
                }
            }
            Message::SelectCameraResolution(device_id, resolution) => {
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    let device_id_clone = device_id.clone();
                    let resolution_clone = resolution.clone();

                    Task::perform(
                        async move {
                            client
                                .set_camera_resolution(&device_id_clone, &resolution_clone)
                                .await
                        },
                        move |result| {
                            if let Err(e) = result {
                                tracing::error!("Failed to set camera resolution: {}", e);
                            }
                            cosmic::Action::App(Message::RefreshDevices)
                        },
                    )
                } else {
                    tracing::warn!("DBus client not available for camera resolution");
                    Task::none()
                }
            }
            Message::CameraStatsUpdated(device_id, stats) => {
                self.camera_stats.insert(device_id, stats);
                Task::none()
            }

            // System Volume
            Message::SetDeviceVolume(device_id, volume) => {
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    return cosmic::task::future(async move {
                        match client.set_device_volume(&device_id, volume).await {
                            Ok(_) => Message::ShowNotification(
                                format!("Volume set to {:.0}%", volume * 100.0),
                                NotificationType::Success,
                                None,
                            ),
                            Err(e) => Message::ShowNotification(
                                format!("Failed to set volume: {}", e),
                                NotificationType::Error,
                                None,
                            ),
                        }
                    });
                }
                Task::none()
            }

            // System Monitor
            Message::RequestSystemInfo(device_id) => {
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    return cosmic::task::future(async move {
                        match client.request_system_info(&device_id).await {
                            Ok(_) => Message::ShowNotification(
                                "System info requested".to_string(),
                                NotificationType::Success,
                                None,
                            ),
                            Err(e) => Message::ShowNotification(
                                format!("Failed to request system info: {}", e),
                                NotificationType::Error,
                                None,
                            ),
                        }
                    });
                }
                Task::none()
            }
            Message::SystemInfoReceived(device_id, info) => {
                tracing::info!("Received system info from device {}", device_id);
                self.system_info.insert(device_id, info);
                Task::none()
            }

            // Screenshot
            Message::TakeScreenshot(device_id) => {
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    return cosmic::task::future(async move {
                        match client.take_screenshot(&device_id).await {
                            Ok(_) => Message::ShowNotification(
                                "Screenshot requested".to_string(),
                                NotificationType::Success,
                                None,
                            ),
                            Err(e) => Message::ShowNotification(
                                format!("Failed to request screenshot: {}", e),
                                NotificationType::Error,
                                None,
                            ),
                        }
                    });
                }
                Task::none()
            }
            Message::ScreenshotReceived(device_id, image_data) => {
                tracing::info!(
                    "Received screenshot from device {} ({} bytes)",
                    device_id,
                    image_data.len()
                );

                // Store screenshot in state for preview
                self.screenshots
                    .insert(device_id.clone(), image_data.clone());

                // Save screenshot to disk
                let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                let device_name = self
                    .devices
                    .iter()
                    .find(|d| d.device.info.device_id == device_id)
                    .map(|d| d.device.info.device_name.as_str())
                    .unwrap_or("unknown");

                let filename = format!("cosmic-connect_{}_{}.png", device_name, timestamp);

                if let Some(pictures_dir) = dirs::picture_dir() {
                    let screenshots_dir = pictures_dir.join("Screenshots");

                    // Create screenshots directory if it doesn't exist
                    if let Err(e) = std::fs::create_dir_all(&screenshots_dir) {
                        tracing::error!("Failed to create screenshots directory: {}", e);
                        return Task::none();
                    }

                    let screenshot_path = screenshots_dir.join(&filename);

                    // Save the image file
                    if let Err(e) = std::fs::write(&screenshot_path, &image_data) {
                        tracing::error!("Failed to save screenshot: {}", e);
                        return Task::none();
                    }

                    tracing::info!("Screenshot saved to {:?}", screenshot_path);

                    self.notification = Some(AppNotification {
                        message: format!("Screenshot saved to {}", filename),
                        kind: NotificationType::Success,
                        action: None,
                    });
                } else {
                    tracing::error!("Could not determine pictures directory");
                    self.notification = Some(AppNotification {
                        message: "Failed to save screenshot: Pictures directory not found"
                            .to_string(),
                        kind: NotificationType::Error,
                        action: None,
                    });
                }

                Task::none()
            }

            // Power Control handlers
            Message::LockDevice(device_id) => {
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    return cosmic::task::future(async move {
                        match client.lock_device(&device_id).await {
                            Ok(_) => Message::ShowNotification(
                                "Lock command sent".to_string(),
                                NotificationType::Success,
                                None,
                            ),
                            Err(e) => Message::ShowNotification(
                                format!("Failed to lock device: {}", e),
                                NotificationType::Error,
                                None,
                            ),
                        }
                    });
                }
                Task::none()
            }
            Message::PowerAction(device_id, action) => {
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    let action_clone = action.clone();
                    return cosmic::task::future(async move {
                        match client.power_action(&device_id, &action_clone).await {
                            Ok(_) => Message::ShowNotification(
                                format!("Power action '{}' sent", action_clone),
                                NotificationType::Success,
                                None,
                            ),
                            Err(e) => Message::ShowNotification(
                                format!("Failed to send power action: {}", e),
                                NotificationType::Error,
                                None,
                            ),
                        }
                    });
                }
                Task::none()
            }
            Message::WakeDevice(device_id) => {
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    return cosmic::task::future(async move {
                        match client.wake_device(&device_id).await {
                            Ok(_) => Message::ShowNotification(
                                "Wake-on-LAN sent".to_string(),
                                NotificationType::Success,
                                None,
                            ),
                            Err(e) => Message::ShowNotification(
                                format!("Failed to wake device: {}", e),
                                NotificationType::Error,
                                None,
                            ),
                        }
                    });
                }
                Task::none()
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
                // Form stays open until completion
                let id = device_id.clone();
                Task::batch(vec![
                    Task::done(cosmic::Action::App(Message::OperationStarted(
                        device_id.clone(),
                        OperationType::SaveNickname,
                    ))),
                    device_operation_with_completion(
                        device_id,
                        OperationType::SaveNickname,
                        move |client, _| async move { client.set_device_nickname(&id, &nickname).await },
                    ),
                ])
            }

            Message::ShowDeviceDetails(device_id) => {
                self.view_mode = ViewMode::DeviceDetails(device_id);
                Task::none()
            }
            Message::LaunchScreenMirror(device_id) => {
                let cmd = "cosmic-connect-mirror";
                let args = &[&device_id];

                match std::process::Command::new(cmd).args(args).spawn() {
                    Ok(_) => Task::none(),
                    Err(_) => {
                        // Try fallback path for dev
                        let debug_path = format!("target/debug/{}", cmd);
                        match std::process::Command::new(&debug_path).args(args).spawn() {
                            Ok(_) => Task::none(),
                            Err(e) => {
                                tracing::error!("Failed to launch mirror app: {}", e);
                                cosmic::task::message(cosmic::Action::App(
                                    Message::ShowNotification(
                                        format!("Failed to launch mirror: {}", e),
                                        NotificationType::Error,
                                        None,
                                    ),
                                ))
                            }
                        }
                    }
                }
            }
            Message::ShareScreenTo(device_id) => {
                // Share our screen to the remote device
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    let device_id_clone = device_id.clone();
                    cosmic::task::future(async move {
                        match client.share_screen_to(&device_id_clone).await {
                            Ok(()) => {
                                tracing::info!("Started sharing screen to {}", device_id_clone);
                                cosmic::Action::App(Message::ShowNotification(
                                    format!("Sharing screen to {}", device_id_clone),
                                    NotificationType::Success,
                                    None,
                                ))
                            }
                            Err(e) => {
                                tracing::error!("Failed to share screen: {}", e);
                                cosmic::Action::App(Message::ShowNotification(
                                    format!("Failed to share screen: {}", e),
                                    NotificationType::Error,
                                    None,
                                ))
                            }
                        }
                    })
                } else {
                    Task::none()
                }
            }
            Message::CloseDeviceDetails => {
                self.view_mode = ViewMode::Devices;
                Task::none()
            }
            Message::ShowTransferQueue => {
                self.view_mode = ViewMode::TransferQueue;
                Task::none()
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

            // Camera settings handlers
            Message::ShowCameraSettings(device_id) => {
                tracing::debug!("Showing Camera settings for {}", device_id);
                self.camera_settings_device = Some(device_id);
                Task::none()
            }
            Message::CloseCameraSettings => {
                tracing::debug!("Closing Camera settings");
                self.camera_settings_device = None;
                Task::none()
            }
            Message::CloseSettingsWindow => {
                if let Some((id, _)) = self.settings_window.take() {
                    window::close(id)
                } else {
                    Task::none()
                }
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
                    if !(640..=7680).contains(&width) {
                        self.remotedesktop_error =
                            Some("Width must be between 640 and 7680".to_string());
                    } else {
                        // Check height as well to clear error if both are valid
                        if let Ok(height) = self.remotedesktop_height_input.parse::<u32>() {
                            if (480..=4320).contains(&height) {
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
                    if !(480..=4320).contains(&height) {
                        self.remotedesktop_error =
                            Some("Height must be between 480 and 4320".to_string());
                    } else {
                        // Check width as well to clear error if both are valid
                        if let Ok(width) = self.remotedesktop_width_input.parse::<u32>() {
                            if (640..=7680).contains(&width) {
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
                            if !(640..=7680).contains(&w) || !(480..=4320).contains(&h) {
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
            // Loading state management
            Message::OperationStarted(device_id, op_type) => {
                self.pending_operations.insert((device_id, op_type));
                Task::none()
            }
            Message::OperationCompleted(device_id, op_type) => {
                self.handle_operation_completed(device_id, op_type)
            }
            Message::OperationSucceeded(device_id, op_type, message) => {
                self.pending_operations.remove(&(device_id, op_type));
                self.notification = Some(AppNotification {
                    message,
                    kind: NotificationType::Success,
                    action: None,
                });
                self.notification_progress = 0.0; // Start animation
                Task::perform(
                    async { tokio::time::sleep(std::time::Duration::from_secs(3)).await },
                    |_| cosmic::Action::App(Message::ClearNotification),
                )
            }
            Message::OperationFailed(device_id, op_type, error) => {
                self.pending_operations.remove(&(device_id, op_type));
                self.notification = Some(AppNotification {
                    message: format!("Error: {}", error),
                    kind: NotificationType::Error,
                    action: None,
                });
                self.notification_progress = 0.0; // Start animation
                Task::perform(
                    async { tokio::time::sleep(std::time::Duration::from_secs(5)).await },
                    |_| cosmic::Action::App(Message::ClearNotification),
                )
            }
            Message::ShowNotification(message, kind, action) => {
                self.notification = Some(AppNotification {
                    message,
                    kind,
                    action,
                });
                self.notification_progress = 0.0; // Start animation
                Task::perform(
                    async { tokio::time::sleep(std::time::Duration::from_secs(5)).await },
                    |_| cosmic::Action::App(Message::ClearNotification),
                )
            }
            Message::ClearNotification => {
                self.notification = None;
                Task::none()
            }
            Message::ToggleKeyboardShortcutsHelp => {
                self.show_keyboard_shortcuts_help = !self.show_keyboard_shortcuts_help;
                Task::none()
            }
            Message::OpenManager => {
                // Launch the standalone manager window
                if let Err(e) = std::process::Command::new("cosmic-connect-manager").spawn() {
                    tracing::error!("Failed to launch manager: {}", e);
                }
                Task::none()
            }
            Message::LaunchManager(device_id) => {
                // Launch the manager with a device pre-selected
                if let Err(e) = std::process::Command::new("cosmic-connect-manager")
                    .arg("--device")
                    .arg(&device_id)
                    .spawn()
                {
                    tracing::error!("Failed to launch manager: {}", e);
                }
                Task::none()
            }
            Message::ToggleDevicePin(device_id) => {
                // Toggle pin state
                self.pinned_devices_config.toggle_pin(device_id.clone());

                // Save config
                if let Err(e) = self.pinned_devices_config.save() {
                    tracing::error!("Failed to save pinned devices config: {}", e);
                }

                Task::none()
            }
            Message::DaemonConnected => {
                self.daemon_connected = true;
                cosmic::task::message(cosmic::Action::App(Message::RefreshDevices))
            }
            Message::DaemonDisconnected => {
                self.daemon_connected = false;
                Task::none()
            }
            Message::KeyPress(key, modifiers) => self.handle_key_press(key, modifiers),
            // Focus navigation
            Message::FocusNext => self.focus_next(),
            Message::FocusPrevious => self.focus_previous(),
            Message::FocusUp => self.focus_up(),
            Message::FocusDown => self.focus_down(),
            Message::FocusLeft => self.focus_left(),
            Message::FocusRight => self.focus_right(),
            Message::ActivateFocused => self.activate_focused(),
            Message::SetFocus(target) => {
                self.focus_target = target;
                Task::none()
            }
            // Drag-and-drop file events
            Message::FileDragEnter => {
                self.dragging_files = true;
                Task::none()
            }
            Message::FileDragLeave => {
                self.dragging_files = false;
                self.drag_hover_device = None;
                Task::none()
            }
            Message::FileDropped(path) => {
                self.dragging_files = false;
                let path_str = path.to_string_lossy().into_owned();

                // Determine target device: explicit selection or single connected device
                let target_device = self.drag_hover_device.take().or_else(|| {
                    let connected: Vec<_> = self
                        .devices
                        .iter()
                        .filter(|d| d.device.is_connected() && d.device.is_paired())
                        .collect();
                    (connected.len() == 1).then(|| connected[0].device.id().to_string())
                });

                match target_device {
                    Some(device_id) => {
                        tracing::info!("File dropped on device {}: {}", device_id, path_str);
                        cosmic::task::message(cosmic::Action::App(Message::FileSelected(
                            device_id, path_str,
                        )))
                    }
                    None => {
                        tracing::debug!("File dropped but no target device selected");
                        Task::none()
                    }
                }
            }
            Message::SetDragHoverDevice(device_id) => {
                self.drag_hover_device = device_id;
                Task::none()
            }
            // Context menu
            Message::ShowContextMenu(device_id) => {
                self.context_menu_device = Some(device_id);
                Task::none()
            }
            Message::CloseContextMenu => {
                self.context_menu_device = None;
                Task::none()
            }
            // Transfer context menu
            Message::ShowTransferContextMenu(transfer_id) => {
                self.context_menu_transfer = Some(transfer_id);
                Task::none()
            }
            Message::CloseTransferContextMenu => {
                self.context_menu_transfer = None;
                Task::none()
            }
            Message::CancelTransfer(transfer_id) => {
                tracing::info!("Cancelling transfer {}", transfer_id);
                self.context_menu_transfer = None;

                if let Some(ref client) = self.dbus_client {
                    let client = client.clone();
                    let future = async move {
                        if let Err(e) = client.cancel_transfer(&transfer_id).await {
                            tracing::error!("Failed to cancel transfer: {}", e);
                        }
                    };
                    return Task::perform(future, |_| {
                        cosmic::Action::App(Message::Tick(std::time::Instant::now()))
                    });
                } else {
                    tracing::error!("DBus client not available");
                }
                Task::none()
            }
            Message::OpenTransferFile(filename) => {
                self.context_menu_transfer = None;
                let downloads_dir = std::env::var("HOME")
                    .map(|h| std::path::PathBuf::from(h).join("Downloads"))
                    .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"));
                let file_path = downloads_dir.join(&filename);

                if file_path.exists() {
                    if let Err(e) = std::process::Command::new("xdg-open")
                        .arg(&file_path)
                        .spawn()
                    {
                        tracing::error!("Failed to open file: {}", e);
                    }
                } else {
                    tracing::warn!("File not found: {:?}", file_path);
                }
                Task::none()
            }
            Message::RevealTransferFile(filename) => {
                self.context_menu_transfer = None;
                let downloads_dir = std::env::var("HOME")
                    .map(|h| std::path::PathBuf::from(h).join("Downloads"))
                    .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"));

                // Open the Downloads folder (parent directory of the file)
                if let Err(e) = std::process::Command::new("xdg-open")
                    .arg(&downloads_dir)
                    .spawn()
                {
                    tracing::error!("Failed to open folder: {}", e);
                }
                let _ = filename; // Used to identify which file, but we open parent dir
                Task::none()
            }
            // MPRIS context menu
            Message::ShowMprisContextMenu => {
                self.context_menu_mpris = true;
                Task::none()
            }
            Message::CloseMprisContextMenu => {
                self.context_menu_mpris = false;
                Task::none()
            }
            Message::ShowMprisTrackInfo => {
                self.context_menu_mpris = false;

                let Some(player) = &self.selected_player else {
                    return Task::none();
                };
                let Some(state) = self.mpris_states.get(player) else {
                    return Task::none();
                };

                let title = state.metadata.title.as_deref().unwrap_or("Unknown");
                let artist = state.metadata.artist.as_deref().unwrap_or("Unknown");
                let album = state.metadata.album.as_deref().unwrap_or("");

                let info = match album.is_empty() {
                    true => format!("{} - {}", title, artist),
                    false => format!("{} - {} ({})", title, artist, album),
                };

                tracing::info!("Track info: {}", info);

                if let Err(e) = std::process::Command::new("notify-send")
                    .args(["Now Playing", &info])
                    .spawn()
                {
                    tracing::warn!("Failed to show notification: {}", e);
                }

                Task::none()
            }
            Message::RaiseMprisPlayer => {
                self.context_menu_mpris = false;

                let Some(player) = &self.selected_player else {
                    return Task::none();
                };
                let Some(client) = &self.dbus_client else {
                    return Task::none();
                };

                let player_name = player.clone();
                let client = client.clone();

                Task::perform(
                    async move {
                        if let Err(e) = client.mpris_raise(&player_name).await {
                            tracing::error!("Failed to raise player: {}", e);
                        }
                    },
                    |_| cosmic::Action::App(Message::Tick(std::time::Instant::now())),
                )
            }
            // Screen share control
            Message::ScreenShareStarted(device_id, is_sender) => {
                tracing::info!(
                    "Screen share started with {} (sender: {})",
                    device_id,
                    is_sender
                );
                self.active_screen_share = Some(ActiveScreenShare {
                    device_id,
                    is_sender,
                    is_paused: false,
                    quality: "medium".to_string(),
                    fps: 30,
                    include_audio: false,
                    viewer_count: 0,
                });
                Task::none()
            }
            Message::ScreenShareStopped(device_id) => {
                tracing::info!("Screen share stopped with {}", device_id);
                if matches!(&self.active_screen_share, Some(share) if share.device_id == device_id)
                {
                    self.active_screen_share = None;
                    self.last_screen_share_stats_poll = None;
                }
                Task::none()
            }
            Message::ScreenShareStatsUpdated {
                device_id,
                viewer_count,
            } => {
                if let Some(ref mut share) = self.active_screen_share {
                    if share.device_id == device_id && share.is_sender {
                        share.viewer_count = viewer_count;
                    }
                }
                Task::none()
            }
            Message::PauseScreenShare(device_id) => {
                tracing::info!("User requested pause screen share with {}", device_id);
                self.update_screen_share_pause_state(&device_id, true);
                screen_share_control_task(device_id, "pause", |client, id| async move {
                    client.pause_screen_share(&id).await
                })
            }
            Message::ResumeScreenShare(device_id) => {
                tracing::info!("User requested resume screen share with {}", device_id);
                self.update_screen_share_pause_state(&device_id, false);
                screen_share_control_task(device_id, "resume", |client, id| async move {
                    client.resume_screen_share(&id).await
                })
            }
            Message::StopScreenShare(device_id) => {
                tracing::info!("User requested stop screen share with {}", device_id);
                screen_share_control_task(device_id, "stop", |client, id| async move {
                    client.stop_screen_share(&id).await
                })
            }
            Message::ForgetScreenShareSource => {
                tracing::info!("User requested to forget saved screenshare source");
                Task::perform(
                    async move {
                        let (client, _) = DbusClient::connect()
                            .await
                            .map_err(|e| anyhow::anyhow!("DBus connection failed: {}", e))?;
                        client.forget_screen_share_source().await
                    },
                    |result| {
                        if let Err(e) = result {
                            tracing::error!("Failed to forget screenshare source: {}", e);
                            cosmic::Action::App(Message::ShowNotification(
                                "Failed to clear saved source".to_string(),
                                NotificationType::Error,
                                None,
                            ))
                        } else {
                            cosmic::Action::App(Message::ShowNotification(
                                "Saved capture source cleared".to_string(),
                                NotificationType::Info,
                                None,
                            ))
                        }
                    },
                )
            }
            Message::SetScreenShareQuality(quality) => {
                if let Some(share) = &mut self.active_screen_share {
                    if share.is_sender {
                        tracing::info!("Updating screen share quality to: {}", quality);
                        share.quality = quality;
                        // Note: Quality changes require stopping and restarting the share
                        self.notification = Some(AppNotification {
                            message: "Quality setting updated. Stop and restart sharing to apply changes.".to_string(),
                            kind: NotificationType::Info,
                            action: None,
                        });
                    }
                }
                Task::none()
            }
            Message::SetScreenShareFps(fps) => {
                if let Some(share) = &mut self.active_screen_share {
                    if share.is_sender {
                        tracing::info!("Updating screen share FPS to: {}", fps);
                        share.fps = fps;
                        // Note: FPS changes require stopping and restarting the share
                        self.notification = Some(AppNotification {
                            message:
                                "FPS setting updated. Stop and restart sharing to apply changes."
                                    .to_string(),
                            kind: NotificationType::Info,
                            action: None,
                        });
                    }
                }
                Task::none()
            }
            Message::ToggleScreenShareAudio(_device_id, include_audio) => {
                if let Some(share) = &mut self.active_screen_share {
                    if share.is_sender {
                        tracing::info!("Toggling screen share audio to: {}", include_audio);
                        share.include_audio = include_audio;

                        // FUTURE ENHANCEMENT: Dynamic audio toggling during active screen share
                        // Currently, this only updates the UI state. Audio configuration is set
                        // at stream start and cannot be changed mid-stream.
                        //
                        // To implement dynamic audio toggling, the following changes are needed:
                        // 1. Add DBus method `update_screen_share_audio(device_id: &str, enable: bool)`
                        //    to the daemon interface (cosmic-applet-connect/src/dbus_client.rs)
                        // 2. Implement GStreamer pipeline reconfiguration in the daemon to:
                        //    - Dynamically add/remove audio source elements
                        //    - Handle PipeWire audio node lifecycle
                        //    - Maintain audio/video sync when toggling
                        // 3. Handle XDG Desktop Portal audio permissions when enabling audio
                        //    after stream has started
                        //
                        // For now, users must stop and restart the screen share to change
                        // audio settings. The UI could be improved by:
                        // - Disabling the toggle during active streaming
                        // - Showing a tooltip: "Stop sharing to change audio settings"
                        // - Or auto-restart the stream when toggled (with user confirmation)
                    }
                }
                Task::none()
            }
            // Audio Stream events
            Message::ToggleAudioStream(device_id) => {
                if self.audio_streaming_devices.contains(&device_id) {
                    let client = self.dbus_client.clone();
                    cosmic::task::future(async move {
                        if let Some(client) = client {
                            if let Err(e) = client.stop_audio_stream(&device_id).await {
                                tracing::error!("Failed to stop audio stream: {}", e);
                            }
                        }
                        Message::AudioStreamStopped(device_id)
                    })
                } else {
                    let client = self.dbus_client.clone();
                    cosmic::task::future(async move {
                        if let Some(client) = client {
                            if let Err(e) = client.start_audio_stream(&device_id).await {
                                tracing::error!("Failed to start audio stream: {}", e);
                            }
                        }
                        Message::AudioStreamStarted(device_id)
                    })
                }
            }
            Message::AudioStreamStarted(device_id) => {
                self.audio_streaming_devices.insert(device_id.clone());
                self.notification = Some(AppNotification {
                    message: "Audio streaming started".to_string(),
                    kind: NotificationType::Success,
                    action: None,
                });
                Task::none()
            }
            Message::AudioStreamStopped(device_id) => {
                self.audio_streaming_devices.remove(&device_id);
                self.notification = Some(AppNotification {
                    message: "Audio streaming stopped".to_string(),
                    kind: NotificationType::Info,
                    action: None,
                });
                Task::none()
            }
            // Presenter mode events
            Message::TogglePresenterMode(device_id) => {
                if self.presenter_mode_devices.contains(&device_id) {
                    let client = self.dbus_client.clone();
                    cosmic::task::future(async move {
                        if let Some(client) = client {
                            if let Err(e) = client.stop_presenter(&device_id).await {
                                tracing::error!("Failed to stop presenter mode: {}", e);
                            }
                        }
                        Message::PresenterStopped(device_id)
                    })
                } else {
                    let client = self.dbus_client.clone();
                    cosmic::task::future(async move {
                        if let Some(client) = client {
                            if let Err(e) = client.start_presenter(&device_id).await {
                                tracing::error!("Failed to start presenter mode: {}", e);
                            }
                        }
                        Message::PresenterStarted(device_id)
                    })
                }
            }
            Message::PresenterStarted(device_id) => {
                self.presenter_mode_devices.insert(device_id.clone());
                self.notification = Some(AppNotification {
                    message: "Presenter mode started".to_string(),
                    kind: NotificationType::Success,
                    action: None,
                });
                Task::none()
            }
            Message::PresenterStopped(device_id) => {
                self.presenter_mode_devices.remove(&device_id);
                self.notification = Some(AppNotification {
                    message: "Presenter mode stopped".to_string(),
                    kind: NotificationType::Info,
                    action: None,
                });
                Task::none()
            }
            // File Transfer events
            Message::TransferProgress(tid, device_id, filename, cur, tot, dir) => {
                let now = std::time::Instant::now();
                let entry = self.active_transfers.entry(tid.clone());
                entry
                    .and_modify(|state| {
                        state.last_bytes = state.current;
                        state.current = cur;
                        state.total = tot;
                        state.last_update = now;
                    })
                    .or_insert_with(|| TransferState {
                        device_id,
                        filename,
                        current: cur,
                        total: tot,
                        direction: dir,
                        started_at: now,
                        last_update: now,
                        last_bytes: 0,
                    });
                Task::none()
            }
            Message::TransferComplete(tid, device_id, filename, success, _error) => {
                let transfer_state = self.active_transfers.remove(&tid);

                if success {
                    tracing::info!("Transfer {} completed successfully", tid);
                } else {
                    tracing::warn!("Transfer {} failed or cancelled", tid);
                }

                // Track received files in history (incoming transfers only)
                let is_receiving = transfer_state
                    .as_ref()
                    .is_some_and(|s| s.direction == "receiving");

                if is_receiving {
                    self.record_received_file(device_id, filename, success);
                }
                Task::none()
            }

            Message::ShowFileSyncSettings(device_id) => {
                self.file_sync_settings_device = Some(device_id.clone());
                // Also load folders when showing settings
                cosmic::task::message(cosmic::Action::App(Message::LoadSyncFolders(
                    device_id,
                )))
            }
            Message::CloseFileSyncSettings => {
                self.file_sync_settings_device = None;
                self.add_sync_folder_device = None; // Also close add form if open
                Task::none()
            }
            Message::LoadSyncFolders(device_id) => match tokio::runtime::Handle::try_current() {
                Ok(_) => {
                    let future = async move {
                        match DbusClient::connect().await {
                            Ok((client, _)) => {
                                match client.get_sync_folders(device_id.clone()).await {
                                    Ok(folders) => Some((device_id, folders)),
                                    Err(e) => {
                                        tracing::error!("Failed to get sync folders: {}", e);
                                        None
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to connect to dbus: {}", e);
                                None
                            }
                        }
                    };

                    Task::perform(future, |result| {
                        if let Some((device_id, folders)) = result {
                            cosmic::Action::App(Message::SyncFoldersLoaded(device_id, folders))
                        } else {
                            cosmic::Action::None
                        }
                    })
                }
                Err(_) => Task::none(),
            },
            Message::SyncFoldersLoaded(device_id, folders) => {
                self.sync_folders.insert(device_id, folders);
                Task::none()
            }
            Message::StartAddSyncFolder(device_id) => {
                self.add_sync_folder_device = Some(device_id);
                self.add_sync_folder_path = String::new();
                self.add_sync_folder_id = String::new();
                self.add_sync_folder_strategy = "last_modified_wins".to_string();
                Task::none()
            }
            Message::CancelAddSyncFolder => {
                self.add_sync_folder_device = None;
                Task::none()
            }
            Message::UpdateSyncFolderPathInput(path) => {
                self.add_sync_folder_path = path;
                // Auto-generate folder ID from path if empty
                if self.add_sync_folder_id.is_empty() {
                    if let Some(name) = std::path::Path::new(&self.add_sync_folder_path).file_name()
                    {
                        self.add_sync_folder_id = name.to_string_lossy().to_string();
                    }
                }
                Task::none()
            }
            Message::UpdateSyncFolderIdInput(id) => {
                self.add_sync_folder_id = id;
                Task::none()
            }
            Message::UpdateSyncFolderStrategy(strategy) => {
                self.add_sync_folder_strategy = strategy;
                Task::none()
            }
            Message::AddSyncFolder(device_id) => {
                if self.add_sync_folder_path.is_empty() || self.add_sync_folder_id.is_empty() {
                    return Task::none();
                }

                let folder_id = self.add_sync_folder_id.clone();
                let path = self.add_sync_folder_path.clone();
                let strategy = self.add_sync_folder_strategy.clone();
                let id = device_id.clone();

                // Form stays open until completion
                Task::batch(vec![
                    Task::done(cosmic::Action::App(Message::OperationStarted(
                        device_id.clone(),
                        OperationType::AddSyncFolder,
                    ))),
                    device_operation_with_completion(
                        device_id,
                        OperationType::AddSyncFolder,
                        move |client, _| async move {
                            client
                                .add_sync_folder(id.clone(), folder_id, path, strategy)
                                .await
                        },
                    ),
                ])
            }
            Message::RemoveSyncFolder(device_id, folder_id) => device_operation_task(
                device_id,
                "remove_sync_folder",
                move |client, dev_id| async move { client.remove_sync_folder(dev_id, folder_id).await },
            ),

            // Run Command logic
            Message::ShowRunCommandSettings(device_id) => {
                self.run_command_settings_device = Some(device_id.clone());
                // Also load commands
                let _ = self.update(Message::LoadRunCommands(device_id));
                Task::none()
            }
            Message::CloseRunCommandSettings => {
                self.run_command_settings_device = None;
                self.add_run_command_device = None; // Also close add form
                Task::none()
            }
            Message::LoadRunCommands(device_id) => match tokio::runtime::Handle::try_current() {
                Ok(_) => {
                    let future = async move {
                        match DbusClient::connect().await {
                            Ok((client, _)) => {
                                match client.get_run_commands(device_id.clone()).await {
                                    Ok(commands) => Some((device_id, commands)),
                                    Err(e) => {
                                        tracing::error!("Failed to get run commands: {}", e);
                                        None
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to connect to dbus: {}", e);
                                None
                            }
                        }
                    };
                    Task::perform(future, |result| {
                        if let Some((device_id, commands)) = result {
                            cosmic::Action::App(Message::RunCommandsLoaded(device_id, commands))
                        } else {
                            cosmic::Action::None
                        }
                    })
                }
                Err(_) => Task::none(),
            },
            Message::RunCommandsLoaded(device_id, commands) => {
                self.run_commands.insert(device_id, commands);
                Task::none()
            }
            Message::StartAddRunCommand(device_id) => {
                self.add_run_command_device = Some(device_id);
                self.add_run_command_name = String::new();
                self.add_run_command_cmd = String::new();
                Task::none()
            }
            Message::CancelAddRunCommand => {
                self.add_run_command_device = None;
                Task::none()
            }
            Message::UpdateRunCommandNameInput(name) => {
                self.add_run_command_name = name;
                Task::none()
            }
            Message::UpdateRunCommandCmdInput(cmd) => {
                self.add_run_command_cmd = cmd;
                Task::none()
            }
            Message::AddRunCommand(device_id) => {
                if self.add_run_command_name.is_empty() || self.add_run_command_cmd.is_empty() {
                    return Task::none();
                }

                let name = self.add_run_command_name.clone();
                let command = self.add_run_command_cmd.clone();
                // Generate a command ID (simple slug or UUID-like)
                let command_id = name.to_lowercase().replace(" ", "_");
                let id = device_id.clone();

                // Form stays open until completion
                Task::batch(vec![
                    Task::done(cosmic::Action::App(Message::OperationStarted(
                        device_id.clone(),
                        OperationType::AddRunCommand,
                    ))),
                    device_operation_with_completion(
                        device_id,
                        OperationType::AddRunCommand,
                        move |client, _| async move {
                            client
                                .add_run_command(id.clone(), command_id, name, command)
                                .await
                        },
                    ),
                ])
            }
            Message::RemoveRunCommand(device_id, command_id) => device_operation_task(
                device_id,
                "remove_run_command",
                move |client, dev_id| async move { client.remove_run_command(dev_id, command_id).await },
            ),
        }
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Self::Message> {
        struct DbusSubscription;

        let event_sub = cosmic::iced::event::listen_with(|event, _status, _window_id| {
            match event {
                // Keyboard events
                cosmic::iced::Event::Keyboard(cosmic::iced::keyboard::Event::KeyPressed {
                    key,
                    modifiers,
                    ..
                }) => Some(Message::KeyPress(key, modifiers)),
                // File drag-and-drop events
                cosmic::iced::Event::Window(cosmic::iced::window::Event::FileHovered(_path)) => {
                    Some(Message::FileDragEnter)
                }
                cosmic::iced::Event::Window(cosmic::iced::window::Event::FileDropped(path)) => {
                    Some(Message::FileDropped(path))
                }
                cosmic::iced::Event::Window(cosmic::iced::window::Event::FilesHoveredLeft) => {
                    Some(Message::FileDragLeave)
                }
                _ => None,
            }
        });

        let dbus_sub = cosmic::iced::Subscription::run_with_id(
            std::any::TypeId::of::<DbusSubscription>(),
            cosmic::iced::futures::stream::unfold(
                None,
                |client_opt: Option<dbus_client::ReconnectingClient>| async move {
                    let mut client = match client_opt {
                        Some(c) => c,
                        None => match dbus_client::ReconnectingClient::new().await {
                            Ok(c) => return Some((Message::DaemonConnected, Some(c))),
                            Err(e) => {
                                tracing::error!("Failed to connect to DBus: {}", e);
                                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                return Some((Message::DaemonDisconnected, None));
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
                            | e @ dbus_client::DaemonEvent::DeviceStateChanged { .. }
                            | e @ dbus_client::DaemonEvent::IncomingCall { .. }
                            | e @ dbus_client::DaemonEvent::MissedCall { .. }
                            | e @ dbus_client::DaemonEvent::CallStateChanged { .. }
                            | e @ dbus_client::DaemonEvent::SmsReceived { .. }
                            | e @ dbus_client::DaemonEvent::SmsConversationsUpdated { .. } => {
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

        let tick_sub = cosmic::iced::window::frames().map(|(_, instant)| Message::Tick(instant));

        cosmic::iced::Subscription::batch(vec![dbus_sub, event_sub, tick_sub])
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
                                state.core.main_window_id()
                                    .expect("applet must have a main window"),
                                new_id,
                                None,
                                None,
                                None,
                            );

                            // Popup size limits - use reasonable constraints that adapt
                            // to content while preventing excessive sizes
                            popup_settings.positioner.size_limits = Limits::NONE
                                .min_width(350.0)
                                .max_width(500.0)
                                .min_height(200.0)
                                .max_height(650.0);

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

    fn view_window(&self, id: window::Id) -> Element<'_, Self::Message> {
        // Check if this is the settings window
        if let Some((settings_id, ref device_id)) = self.settings_window {
            if id == settings_id {
                return self.view_settings_window(device_id);
            }
        }

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
    /// Updates the pause state for the active screen share session
    fn update_screen_share_pause_state(&mut self, device_id: &str, paused: bool) {
        if let Some(share) = &mut self.active_screen_share {
            if share.device_id == device_id {
                share.is_paused = paused;
            }
        }
    }

    /// Renders the device settings window
    fn view_settings_window(&self, device_id: &str) -> Element<'_, Message> {
        // Get device info
        let device = self.devices.iter().find(|d| d.device.id() == device_id);
        let device_name = device.map(|d| d.device.name()).unwrap_or("Unknown Device");

        // Header with device name and close button
        let header = row![
            text(format!("Settings: {}", device_name)).size(18.0),
            horizontal_space(),
            button::icon(icon::from_name("window-close-symbolic"))
                .on_press(Message::CloseSettingsWindow)
                .padding(space_xxxs())
        ]
        .spacing(space_xxs())
        .align_y(cosmic::iced::Alignment::Center);

        // Create content sections for different settings
        // FUTURE ENHANCEMENT: Implement per-device settings panels
        //
        // This settings window is a placeholder for device-specific configuration.
        // Currently, all plugin settings are global (configured in daemon config file).
        //
        // Planned settings panels (priority order):
        // 1. **RemoteDesktop Plugin** (High Priority)
        //    - Toggle auto-accept remote control requests
        //    - Set input method restrictions (keyboard/mouse/both)
        //    - Configure clipboard sharing permissions
        //
        // 2. **File Sync Plugin** (High Priority)
        //    - Set custom download directory per device
        //    - Configure auto-accept file size threshold
        //    - Enable/disable automatic file receiving
        //
        // 3. **Run Command Plugin** (Medium Priority)
        //    - Manage allowed commands list per device
        //    - Set command execution timeout
        //    - Configure command output handling
        //
        // 4. **Plugin Overrides** (Medium Priority)
        //    - Enable/disable specific plugins per device
        //    - Override global plugin settings
        //
        // 5. **Notification Preferences** (Low Priority)
        //    - Filter which notification types to display
        //    - Set notification priority levels
        //
        // Implementation requires:
        // - Per-device config storage (likely SQLite or TOML per device)
        // - DBus methods to get/set device-specific configs
        // - UI widgets for each setting category (cosmic::widget::settings)
        // - Validation and error handling for config updates
        let content = column![
            header,
            divider::horizontal::default(),
            text("Device-specific settings coming soon:"),
            text("• RemoteDesktop configuration"),
            text("• File Sync folder management"),
            text("• Run Command setup"),
            text("• Plugin overrides"),
            text("• Notification preferences"),
            text(""),
            text("For now, configure plugins globally in:").size(12),
            text("~/.config/cosmic-connect/config.toml").size(12),
        ]
        .spacing(space_xs())
        .padding(space_xs());

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Records a received file in the history for display in the transfer queue view.
    fn record_received_file(&mut self, device_id: String, filename: String, success: bool) {
        let device_name = self
            .devices
            .iter()
            .find(|d| d.device.id() == device_id)
            .map(|d| d.device.name().to_string())
            .unwrap_or_else(|| {
                tracing::debug!(
                    "Device {} not found when recording transfer history, using ID as name",
                    device_id
                );
                device_id.clone()
            });

        let received_file = ReceivedFile {
            filename,
            device_id,
            device_name,
            timestamp: std::time::Instant::now(),
            success,
        };

        self.received_files_history.insert(0, received_file);
        self.received_files_history
            .truncate(MAX_RECEIVED_FILES_HISTORY);
    }

    // ==================== Message Handler Methods ====================
    // These methods handle related groups of messages to keep update() organized

    /// Handle device event from daemon (device added, removed, state changed, etc.)
    fn handle_device_event(&mut self, event: dbus_client::DaemonEvent) -> Task<Message> {
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

                // Remove from local device list (matches manager behavior)
                self.devices
                    .retain(|d| d.device.info.device_id != *device_id);

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
            dbus_client::DaemonEvent::ScreenShareRequested { device_id } => {
                self.history.push(HistoryEvent {
                    timestamp,
                    event_type: "Screen Share".to_string(),
                    device_name: "Unknown".to_string(),
                    details: format!("Device {} requested screen share", device_id),
                });

                // Keep history bounded
                if self.history.len() > 50 {
                    self.history.remove(0);
                }

                return cosmic::task::message(cosmic::Action::App(Message::ShowNotification(
                    "Remote device requested screen share".into(),
                    NotificationType::Info,
                    Some((
                        "Accept".into(),
                        Box::new(Message::LaunchScreenMirror(device_id.to_string())),
                    )),
                )));
            }
            dbus_client::DaemonEvent::ScreenShareOutgoingRequest { device_id } => {
                // Remote device is requesting US to share our screen with them
                self.history.push(HistoryEvent {
                    timestamp,
                    event_type: "Screen Share Request".to_string(),
                    device_name: "Unknown".to_string(),
                    details: format!("Device {} wants to view your screen", device_id),
                });

                // Keep history bounded
                if self.history.len() > 50 {
                    self.history.remove(0);
                }

                return cosmic::task::message(cosmic::Action::App(Message::ShowNotification(
                    "Remote device wants to view your screen".into(),
                    NotificationType::Info,
                    Some((
                        "Share Screen".into(),
                        Box::new(Message::ShareScreenTo(device_id.to_string())),
                    )),
                )));
            }
            dbus_client::DaemonEvent::ScreenShareStarted {
                device_id,
                is_sender,
            } => {
                return cosmic::task::message(cosmic::Action::App(Message::ScreenShareStarted(
                    device_id.clone(),
                    *is_sender,
                )));
            }
            dbus_client::DaemonEvent::ScreenShareStopped { device_id } => {
                return cosmic::task::message(cosmic::Action::App(Message::ScreenShareStopped(
                    device_id.clone(),
                )));
            }
            dbus_client::DaemonEvent::IncomingCall {
                device_id,
                phone_number,
                contact_name,
                ..
            } => {
                let caller = if contact_name.is_empty() || contact_name == "Unknown contact" {
                    phone_number.clone()
                } else {
                    format!("{} ({})", contact_name, phone_number)
                };
                let device_name = self
                    .devices
                    .iter()
                    .find(|d| d.device.info.device_id == *device_id)
                    .map(|d| d.device.name().to_string())
                    .unwrap_or_else(|| device_id.clone());
                self.history.push(HistoryEvent {
                    timestamp,
                    event_type: "Incoming Call".to_string(),
                    device_name,
                    details: format!("From {}", caller),
                });
                return cosmic::task::message(cosmic::Action::App(Message::ShowNotification(
                    format!("Incoming call from {}", caller),
                    NotificationType::Info,
                    Some((
                        "Mute".into(),
                        Box::new(Message::MuteCall(device_id.clone())),
                    )),
                )));
            }
            dbus_client::DaemonEvent::MissedCall {
                device_id,
                phone_number,
                contact_name,
            } => {
                let caller = if contact_name.is_empty() || contact_name == "Unknown contact" {
                    phone_number.clone()
                } else {
                    format!("{} ({})", contact_name, phone_number)
                };
                let device_name = self
                    .devices
                    .iter()
                    .find(|d| d.device.info.device_id == *device_id)
                    .map(|d| d.device.name().to_string())
                    .unwrap_or_else(|| device_id.clone());
                self.history.push(HistoryEvent {
                    timestamp,
                    event_type: "Missed Call".to_string(),
                    device_name,
                    details: format!("From {}", caller),
                });
                return cosmic::task::message(cosmic::Action::App(Message::ShowNotification(
                    format!("Missed call from {}", caller),
                    NotificationType::Info,
                    None,
                )));
            }
            dbus_client::DaemonEvent::CallStateChanged {
                device_id,
                state,
                phone_number,
                contact_name,
            } => {
                let caller = if contact_name.is_empty() || contact_name == "Unknown contact" {
                    phone_number.clone()
                } else {
                    contact_name.clone()
                };
                let device_name = self
                    .devices
                    .iter()
                    .find(|d| d.device.info.device_id == *device_id)
                    .map(|d| d.device.name().to_string())
                    .unwrap_or_else(|| device_id.clone());
                self.history.push(HistoryEvent {
                    timestamp,
                    event_type: "Call State".to_string(),
                    device_name,
                    details: format!("{} with {}", state, caller),
                });
            }
            dbus_client::DaemonEvent::SmsReceived {
                device_id,
                address,
                body,
                ..
            } => {
                let preview: String = body.chars().take(50).collect();
                let device_name = self
                    .devices
                    .iter()
                    .find(|d| d.device.info.device_id == *device_id)
                    .map(|d| d.device.name().to_string())
                    .unwrap_or_else(|| device_id.clone());
                self.history.push(HistoryEvent {
                    timestamp,
                    event_type: "SMS Received".to_string(),
                    device_name,
                    details: format!("From {}: {}", address, preview),
                });
                return cosmic::task::message(cosmic::Action::App(Message::ShowNotification(
                    format!("SMS from {}: {}", address, preview),
                    NotificationType::Info,
                    Some((
                        "Reply".into(),
                        Box::new(Message::ShowSmsDialog(device_id.clone())),
                    )),
                )));
            }
            dbus_client::DaemonEvent::SmsConversationsUpdated { device_id, count } => {
                tracing::debug!(
                    "SMS conversations updated for {}: {} conversations",
                    device_id, count
                );
            }
            _ => {}
        }

        // Keep history bounded
        if self.history.len() > 50 {
            self.history.remove(0);
        }

        fetch_devices_task()
    }

    /// Handle MPRIS player selection with state fetch
    fn handle_mpris_player_selected(&self, player: String) -> Task<Message> {
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
                    cosmic::Action::App(Message::MprisStateUpdated(player_closure.clone(), s))
                } else {
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

    /// Handle MPRIS control action (play, pause, next, previous)
    fn handle_mpris_control(&self, player: String, action: String) -> Task<Message> {
        tracing::info!("MPRIS control: {} on {}", action, player);
        let player_arg = player.clone();
        let player_closure = player.clone();
        Task::perform(
            async move {
                if let Ok((client, _)) = DbusClient::connect().await {
                    if let Err(e) = client.mpris_control(&player_arg, &action).await {
                        tracing::error!("Failed to control MPRIS player: {}", e);
                    }
                    (client.get_player_state(&player_arg).await).ok()
                } else {
                    None
                }
            },
            move |state| {
                if let Some(s) = state {
                    cosmic::Action::App(Message::MprisStateUpdated(player_closure.clone(), s))
                } else {
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

    /// Handle keyboard shortcut press
    fn handle_key_press(
        &mut self,
        key: cosmic::iced::keyboard::Key,
        modifiers: cosmic::iced::keyboard::Modifiers,
    ) -> Task<Message> {
        if let cosmic::iced::keyboard::Key::Named(cosmic::iced::keyboard::key::Named::Escape) = key
        {
            // Handle Esc key to close overlays/forms one by one
            if self.show_keyboard_shortcuts_help {
                self.show_keyboard_shortcuts_help = false;
                return Task::none();
            } else if self.notification.is_some() {
                self.notification = None;
                return Task::none();
            } else if self.open_url_dialog_device.is_some() {
                self.open_url_dialog_device = None;
                self.open_url_input.clear();
                return Task::none();
            } else if self.sms_dialog_device.is_some() {
                self.sms_dialog_device = None;
                self.sms_phone_number_input.clear();
                self.sms_message_input.clear();
                return Task::none();
            } else if self.add_run_command_device.is_some() {
                self.add_run_command_device = None;
                return Task::none();
            } else if self.add_sync_folder_device.is_some() {
                self.add_sync_folder_device = None;
                return Task::none();
            } else if self.renaming_device.is_some() {
                self.renaming_device = None;
                return Task::none();
            } else if self.run_command_settings_device.is_some() {
                self.run_command_settings_device = None;
                return Task::none();
            } else if self.file_sync_settings_device.is_some() {
                self.file_sync_settings_device = None;
                return Task::none();
            } else if self.remotedesktop_settings_device.is_some() {
                self.remotedesktop_settings_device = None;
                return Task::none();
            } else if self.view_mode == ViewMode::History {
                self.view_mode = ViewMode::Devices;
                return Task::none();
            }

            // If nothing was handled, close the popup
            if let Some(id) = self.popup {
                return cosmic::task::message(cosmic::Action::Cosmic(
                    cosmic::app::Action::Surface(destroy_popup(id)),
                ));
            }
        }

        // F1 key for help
        if let cosmic::iced::keyboard::Key::Named(cosmic::iced::keyboard::key::Named::F1) = key {
            return cosmic::task::message(cosmic::Action::App(
                Message::ToggleKeyboardShortcutsHelp,
            ));
        }

        // Question mark for help (? key)
        if let cosmic::iced::keyboard::Key::Character(c) = &key {
            if c.as_str() == "?" && !modifiers.control() && !modifiers.alt() {
                return cosmic::task::message(cosmic::Action::App(
                    Message::ToggleKeyboardShortcutsHelp,
                ));
            }
        }

        // Keyboard shortcuts with Ctrl modifier
        if modifiers.control() {
            if let cosmic::iced::keyboard::Key::Character(c) = &key {
                return match c.as_str() {
                    "r" => cosmic::task::message(cosmic::Action::App(Message::RefreshDevices)),
                    "f" => cosmic::task::message(cosmic::Action::App(Message::SetFocus(
                        FocusTarget::Search,
                    ))),
                    "," => match self.get_settings_device_id() {
                        Some(id) => {
                            cosmic::task::message(cosmic::Action::App(Message::LaunchManager(id)))
                        }
                        None => {
                            cosmic::task::message(cosmic::Action::App(Message::ShowNotification(
                                "No paired devices available".into(),
                                NotificationType::Info,
                                None,
                            )))
                        }
                    },
                    "m" => cosmic::task::message(cosmic::Action::App(Message::OpenManager)),
                    _ => Task::none(),
                };
            }
        }

        // Keyboard navigation
        use cosmic::iced::keyboard::key::Named;
        use cosmic::iced::keyboard::Key;
        let message = match &key {
            Key::Named(Named::Tab) if modifiers.shift() => Some(Message::FocusPrevious),
            Key::Named(Named::Tab) => Some(Message::FocusNext),
            Key::Named(Named::ArrowUp) => Some(Message::FocusUp),
            Key::Named(Named::ArrowDown) => Some(Message::FocusDown),
            Key::Named(Named::ArrowLeft) => Some(Message::FocusLeft),
            Key::Named(Named::ArrowRight) => Some(Message::FocusRight),
            Key::Named(Named::Enter | Named::Space) => Some(Message::ActivateFocused),
            _ => None,
        };

        message
            .map(|m| cosmic::task::message(cosmic::Action::App(m)))
            .unwrap_or_else(Task::none)
    }

    /// Get list of focusable elements in current view
    fn get_focusable_elements(&self) -> Vec<FocusTarget> {
        let device_count = if self.view_mode == ViewMode::Devices {
            self.filtered_devices().len()
        } else {
            0
        };
        let mpris_count = if self.selected_player.is_some() { 3 } else { 0 };

        // Pre-allocate: 2 (search + refresh) + 4 per device + 3 for MPRIS
        let capacity = 2 + (device_count * 4) + mpris_count;
        let mut elements = Vec::with_capacity(capacity);

        // Devices view elements
        if self.view_mode == ViewMode::Devices {
            elements.push(FocusTarget::Search);
            elements.push(FocusTarget::Refresh);

            // Device + actions for each device
            for i in 0..device_count {
                elements.push(FocusTarget::Device(i));
                elements.push(FocusTarget::DeviceAction(i, 0)); // Ping
                elements.push(FocusTarget::DeviceAction(i, 1)); // Send file / other actions
            }
        }

        // MPRIS controls
        if let Some(player) = &self.selected_player {
            for ctrl in ["prev", "play", "next"] {
                elements.push(FocusTarget::MprisControl(player.clone(), ctrl.into()));
            }
        }

        elements
    }

    /// Get filtered devices based on search query
    fn filtered_devices(&self) -> Vec<&DeviceState> {
        if self.search_query.is_empty() {
            self.devices.iter().collect()
        } else {
            let query = self.search_query.to_lowercase();
            self.devices
                .iter()
                .filter(|d| d.device.name().to_lowercase().contains(&query))
                .collect()
        }
    }

    /// Move focus to next element
    fn focus_next(&mut self) -> Task<Message> {
        let mut elements = self.get_focusable_elements();
        if elements.is_empty() {
            return Task::none();
        }

        let current_idx = elements.iter().position(|e| *e == self.focus_target);
        let next_idx = current_idx.map_or(0, |idx| (idx + 1) % elements.len());

        self.focus_target = elements.swap_remove(next_idx);
        Task::none()
    }

    /// Move focus to previous element
    fn focus_previous(&mut self) -> Task<Message> {
        let mut elements = self.get_focusable_elements();
        if elements.is_empty() {
            return Task::none();
        }

        let len = elements.len();
        let current_idx = elements.iter().position(|e| *e == self.focus_target);
        let prev_idx = current_idx.map_or(len - 1, |idx| idx.checked_sub(1).unwrap_or(len - 1));

        self.focus_target = elements.swap_remove(prev_idx);
        Task::none()
    }

    /// Extract device index from current focus target if applicable
    fn focused_device_index(&self) -> Option<usize> {
        match &self.focus_target {
            FocusTarget::Device(idx) | FocusTarget::DeviceAction(idx, _) => Some(*idx),
            _ => None,
        }
    }

    /// Get device ID for opening settings: uses focused device or falls back to first paired device
    fn get_settings_device_id(&self) -> Option<String> {
        self.focused_device_index()
            .and_then(|idx| {
                self.filtered_devices()
                    .get(idx)
                    .map(|d| d.device.id().to_string())
            })
            .or_else(|| {
                self.devices
                    .iter()
                    .find(|d| d.device.is_paired())
                    .map(|d| d.device.id().to_string())
            })
    }

    /// Move focus up (within device list)
    fn focus_up(&mut self) -> Task<Message> {
        if let Some(idx) = self.focused_device_index() {
            if idx > 0 {
                self.focus_target = FocusTarget::Device(idx - 1);
                return Task::none();
            }
        }
        self.focus_previous()
    }

    /// Move focus down (within device list)
    fn focus_down(&mut self) -> Task<Message> {
        let device_count = self.filtered_devices().len();
        if let Some(idx) = self.focused_device_index() {
            if idx + 1 < device_count {
                self.focus_target = FocusTarget::Device(idx + 1);
                return Task::none();
            }
        }
        self.focus_next()
    }

    /// Move focus left (within quick actions)
    fn focus_left(&mut self) -> Task<Message> {
        self.focus_target = match &self.focus_target {
            FocusTarget::DeviceAction(idx, action) if *action > 0 => {
                FocusTarget::DeviceAction(*idx, action - 1)
            }
            FocusTarget::MprisControl(player, ctrl) => match ctrl.as_str() {
                "next" => FocusTarget::MprisControl(player.clone(), "play".into()),
                "play" => FocusTarget::MprisControl(player.clone(), "prev".into()),
                _ => return Task::none(),
            },
            _ => return Task::none(),
        };
        Task::none()
    }

    /// Move focus right (within quick actions)
    fn focus_right(&mut self) -> Task<Message> {
        self.focus_target = match &self.focus_target {
            FocusTarget::DeviceAction(idx, 0) => FocusTarget::DeviceAction(*idx, 1),
            FocusTarget::Device(idx) => FocusTarget::DeviceAction(*idx, 0),
            FocusTarget::MprisControl(player, ctrl) => match ctrl.as_str() {
                "prev" => FocusTarget::MprisControl(player.clone(), "play".into()),
                "play" => FocusTarget::MprisControl(player.clone(), "next".into()),
                _ => return Task::none(),
            },
            _ => return Task::none(),
        };
        Task::none()
    }

    /// Activate the currently focused element
    fn activate_focused(&mut self) -> Task<Message> {
        // Helper to get device ID at index
        let get_device_id = |idx: usize| -> Option<String> {
            self.filtered_devices()
                .get(idx)
                .map(|d| d.device.id().to_string())
        };

        let message = match &self.focus_target {
            FocusTarget::Device(idx) => get_device_id(*idx).map(Message::ShowDeviceDetails),
            FocusTarget::DeviceAction(idx, 0) => get_device_id(*idx).map(Message::SendPing),
            FocusTarget::DeviceAction(idx, 1) => get_device_id(*idx).map(Message::SendFile),
            FocusTarget::DeviceAction(_, _) => None,
            FocusTarget::MprisControl(player, ctrl) => {
                Some(Message::MprisControl(player.clone(), ctrl.clone()))
            }
            FocusTarget::Refresh => Some(Message::RefreshDevices),
            FocusTarget::Search | FocusTarget::ViewTab(_) | FocusTarget::None => None,
        };

        message
            .map(|m| cosmic::task::message(cosmic::Action::App(m)))
            .unwrap_or_else(Task::none)
    }

    /// Handle operation completion with success notifications
    fn handle_operation_completed(
        &mut self,
        device_id: String,
        op_type: OperationType,
    ) -> Task<Message> {
        self.pending_operations
            .remove(&(device_id.clone(), op_type));

        // Cleanup forms on completion
        match op_type {
            OperationType::AddRunCommand => self.add_run_command_device = None,
            OperationType::AddSyncFolder => self.add_sync_folder_device = None,
            OperationType::SaveNickname => self.renaming_device = None,
            OperationType::Ping => {
                return cosmic::task::message(cosmic::Action::App(Message::OperationSucceeded(
                    device_id,
                    op_type,
                    "Ping sent successfully".into(),
                )));
            }
            OperationType::ShareText => {
                return cosmic::task::message(cosmic::Action::App(Message::OperationSucceeded(
                    device_id,
                    op_type,
                    "Text shared successfully".into(),
                )));
            }
            OperationType::ShareUrl => {
                return cosmic::task::message(cosmic::Action::App(Message::OperationSucceeded(
                    device_id,
                    op_type,
                    "URL shared successfully".into(),
                )));
            }
            OperationType::FindPhone => {
                return cosmic::task::message(cosmic::Action::App(Message::OperationSucceeded(
                    device_id,
                    op_type,
                    "Find Phone request sent".into(),
                )));
            }
            _ => {}
        }

        cosmic::task::message(cosmic::Action::App(Message::RefreshDevices))
    }

    /// Handle tick animation for notifications
    fn handle_tick(&mut self) -> Task<Message> {
        let mut needs_redux = false;

        if self.notification.is_some() {
            if self.notification_progress < 1.0 {
                self.notification_progress += 0.1;
                if self.notification_progress > 1.0 {
                    self.notification_progress = 1.0;
                }
                needs_redux = true;
            }
        } else if self.notification_progress > 0.0 {
            self.notification_progress -= 0.1;
            if self.notification_progress < 0.0 {
                self.notification_progress = 0.0;
            }
            needs_redux = true;
        }

        // Poll screen share stats every 2 seconds when actively sharing
        if let Some(ref share) = self.active_screen_share {
            if share.is_sender {
                let should_poll = self
                    .last_screen_share_stats_poll
                    .map(|last| last.elapsed() >= std::time::Duration::from_secs(2))
                    .unwrap_or(true);

                if should_poll {
                    if let Some(client) = self.dbus_client.clone() {
                        self.last_screen_share_stats_poll = Some(std::time::Instant::now());
                        let device_id = share.device_id.clone();

                        return Task::perform(
                            async move {
                                match client.get_screen_share_stats(&device_id).await {
                                    Ok(stats) => Some((device_id, stats.viewer_count)),
                                    Err(_) => None,
                                }
                            },
                            |result| {
                                if let Some((device_id, viewer_count)) = result {
                                    cosmic::Action::App(Message::ScreenShareStatsUpdated {
                                        device_id,
                                        viewer_count,
                                    })
                                } else {
                                    cosmic::Action::App(Message::Tick(std::time::Instant::now()))
                                }
                            },
                        );
                    }
                }
            }
        }

        if needs_redux {
            cosmic::task::message(cosmic::Action::None)
        } else {
            Task::none()
        }
    }
}
