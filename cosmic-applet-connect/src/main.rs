mod dbus_client;
mod onboarding_config;
mod pinned_devices_config;

use std::collections::HashMap;

use cosmic::{
    app::{Core, Task},
    iced::{
        alignment::Horizontal,
        widget::{column, container, row, scrollable, text},
        window, Color, Length, Padding, Rectangle,
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

// TODO: Uncomment when camera module is implemented
// use cosmic_connect_core::plugins::camera::{
//     CameraCapability, CameraFacing, CameraInfo, CameraStart, Resolution, StreamStats, StreamingStatus,
// };

use cosmic::iced::widget::progress_bar;
use dbus_client::DbusClient;

// COSMIC Design System spacing scale
// Following libcosmic patterns for consistent spacing
const SPACE_XXXS: f32 = 2.0; // Minimal spacing
const SPACE_XXS: f32 = 4.0; // Tight spacing
const SPACE_XS: f32 = 6.0; // Extra small
const SPACE_S: f32 = 8.0; // Small (default for most UI elements)
const SPACE_M: f32 = 12.0; // Medium (sections, groups)
const SPACE_XL: f32 = 20.0; // Extra large
const SPACE_XXL: f32 = 24.0; // Double extra large (empty states, major padding)

// Icon sizes
const ICON_XS: u16 = 12;
const ICON_S: u16 = 16; // Standard button/action icon
#[allow(dead_code)]
const ICON_M: u16 = 24;
const ICON_L: u16 = 32;
const ICON_XL: u16 = 48; // Hero/Empty state
                         // Specific sizes
const ICON_14: u16 = 14; // Text-aligned small

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
        capability: "cconnect.sms",
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
    started_at: std::time::Instant,
    last_update: std::time::Instant,
    last_bytes: u64,
}

/// A recently received file for history tracking
#[derive(Debug, Clone)]
struct ReceivedFile {
    filename: String,
    #[allow(dead_code)]
    device_id: String,
    device_name: String,
    timestamp: std::time::Instant,
    success: bool,
}

/// Maximum number of received files to track in history (memory limit)
const MAX_RECEIVED_FILES_HISTORY: usize = 50;
/// Number of recent files to display in the UI
const MAX_DISPLAYED_HISTORY_ITEMS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum OperationType {
    Ping,
    Pair,
    Unpair,

    Battery,
    FindPhone,
    ShareText,
    ShareUrl,
    AddRunCommand,
    AddSyncFolder,
    SaveNickname,
    MuteCall,
    SendSms,
}

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
}

/// Active screen share session information
#[derive(Debug, Clone)]
struct ActiveScreenShare {
    device_id: String,
    is_sender: bool,     // true if we are sharing, false if receiving
    is_paused: bool,     // true if the share is paused
    quality: String,     // quality preset: "low", "medium", "high"
    fps: u8,             // target framerate: 15, 30, or 60
    include_audio: bool, // whether system audio is included in the share
    viewer_count: u32,   // number of active viewers (only for sender)
}

// TODO: uncomment when camera module is implemented
// /// Camera streaming state for a device
// #[derive(Debug, Clone)]
// struct CameraStreamingState {
//     is_streaming: bool,
//     selected_camera_id: u32,
//     selected_resolution: Resolution,
//     status: StreamingStatus,
//     error: Option<String>,
// }

#[derive(Debug, Clone)]
struct AppNotification {
    message: String,
    kind: NotificationType,
    action: Option<(String, Box<Message>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotificationType {
    Error,
    Success,
    #[allow(dead_code)]
    Info,
}

/// Focus targets for keyboard navigation
#[derive(Debug, Clone, PartialEq, Eq)]
enum FocusTarget {
    /// Search input field
    Search,
    /// Device at index in the filtered list
    Device(usize),
    /// Quick action button for device (device_index, action_index)
    DeviceAction(usize, usize),
    /// MPRIS player control (player_name, control: "prev", "play", "next")
    MprisControl(String, String),
    /// View mode tab (Devices, History, Transfers) - reserved for future use
    #[allow(dead_code)]
    ViewTab(ViewMode),
    /// Refresh button
    Refresh,
    /// None - nothing focused
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ViewMode {
    Devices,
    History,
    TransferQueue,
    DeviceDetails(String),
}

#[derive(Debug, Clone)]
struct HistoryEvent {
    #[allow(dead_code)]
    timestamp: std::time::SystemTime,
    event_type: String,
    device_name: String,
    details: String,
}

/// Camera streaming statistics
#[derive(Debug, Clone)]
struct CameraStats {
    /// Current frames per second
    fps: u32,
    /// Current bitrate in kbps
    bitrate: u32,
    /// Is currently streaming
    is_streaming: bool,
    /// Current camera ID
    camera_id: u32,
    /// Current resolution (e.g., "720p")
    resolution: String,
}

/// System information from remote device
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct SystemInfo {
    cpu_usage: f64,
    memory_usage: f64,
    total_memory: u64,
    used_memory: u64,
    disk_usage: f64,
    uptime: u64,
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
    SendFiles(String),            // device_id - opens picker for multiple files
    FileSelected(String, String), // device_id, file_path (single file)
    FilesSelected(String, Vec<String>), // device_id, file_paths (multiple files)
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

    // Camera streaming controls
    ToggleCameraStreaming(String),           // device_id
    SelectCamera(String, u32),               // device_id, camera_id
    SelectCameraResolution(String, String),  // device_id, resolution ("480p", "720p", "1080p")
    CameraStatsUpdated(String, CameraStats), // device_id, stats

    // System Volume controls
    SetDeviceVolume(String, f64), // device_id, volume (0.0-1.0)

    // System Monitor
    RequestSystemInfo(String),              // device_id
    SystemInfoReceived(String, SystemInfo), // device_id, info

    // Screenshot
    TakeScreenshot(String),              // device_id
    ScreenshotReceived(String, Vec<u8>), // device_id, image data

    // Power Control
    LockDevice(String),          // device_id
    PowerAction(String, String), // device_id, action ("shutdown", "hibernate", "suspend")
    WakeDevice(String),          // device_id

    // Renaming
    StartRenaming(String), // device_id
    CancelRenaming,
    UpdateNicknameInput(String),
    SaveNickname(String), // device_id

    // Navigation
    ShowDeviceDetails(String),
    CloseDeviceDetails,
    ShowTransferQueue,
    LaunchScreenMirror(String), // device_id

    // Device config (used for renaming)
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
    // Run Commands
    ShowRunCommandSettings(String), // device_id
    CloseRunCommandSettings,
    LoadRunCommands(String), // device_id
    RunCommandsLoaded(String, HashMap<String, dbus_client::RunCommand>), // device_id, commands
    StartAddRunCommand(String), // device_id
    CancelAddRunCommand,
    UpdateRunCommandNameInput(String),
    UpdateRunCommandCmdInput(String),
    AddRunCommand(String),            // device_id
    RemoveRunCommand(String, String), // device_id, command_id
    // Loading state management
    OperationStarted(String, OperationType),
    OperationCompleted(String, OperationType),
    OperationFailed(String, OperationType, String),
    OperationSucceeded(String, OperationType, String),
    ClearNotification,
    ShowNotification(String, NotificationType, Option<(String, Box<Message>)>),
    // Help dialog
    ToggleKeyboardShortcutsHelp,
    OpenManager,           // Launch standalone manager window
    LaunchManager(String), // Launch manager with device_id pre-selected
    // Pinned devices
    ToggleDevicePin(String), // device_id
    // Daemon status
    DaemonConnected,
    DaemonDisconnected,
    // Keyboard events
    KeyPress(
        cosmic::iced::keyboard::Key,
        cosmic::iced::keyboard::Modifiers,
    ),
    // Focus navigation
    FocusNext,
    FocusPrevious,
    FocusUp,
    FocusDown,
    FocusLeft,
    FocusRight,
    ActivateFocused,
    SetFocus(FocusTarget),
    // Drag-and-drop file events
    FileDragEnter,
    FileDragLeave,
    FileDropped(std::path::PathBuf),
    SetDragHoverDevice(Option<String>),
    // Context menu (device)
    ShowContextMenu(String), // device_id
    CloseContextMenu,
    // Context menu (transfer)
    ShowTransferContextMenu(String), // transfer_id
    CloseTransferContextMenu,
    CancelTransfer(String),     // transfer_id
    OpenTransferFile(String),   // filename
    RevealTransferFile(String), // filename
    // Context menu (MPRIS)
    ShowMprisContextMenu,
    CloseMprisContextMenu,
    ShowMprisTrackInfo, // Show notification with track info
    RaiseMprisPlayer,   // Bring player window to front
    // Screen share control
    ScreenShareStarted(String, bool), // device_id, is_sender
    ScreenShareStopped(String),       // device_id
    StopScreenShare(String),          // device_id - user action to stop sharing
    ScreenShareStatsUpdated {
        device_id: String,
        viewer_count: u32,
    },
    PauseScreenShare(String),  // device_id - user action to pause sharing
    ResumeScreenShare(String), // device_id - user action to resume sharing
    SetScreenShareQuality(String), // quality preset: "low", "medium", "high"
    SetScreenShareFps(u8),     // fps: 15, 30, or 60
    ToggleScreenShareAudio(String, bool), // device_id, include_audio - toggle audio in screen share
    // Audio Stream events
    ToggleAudioStream(String),  // device_id - toggle audio streaming on/off
    AudioStreamStarted(String), // device_id - audio stream started
    AudioStreamStopped(String), // device_id - audio stream stopped
    // Presenter mode events
    TogglePresenterMode(String), // device_id - toggle presenter mode on/off
    PresenterStarted(String),    // device_id - presenter mode started
    PresenterStopped(String),    // device_id - presenter mode stopped
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
    // File Sync
    LoadSyncFolders(String),
    SyncFoldersLoaded(String, Vec<dbus_client::SyncFolderInfo>),
    UpdateSyncFolderPathInput(String),
    UpdateSyncFolderIdInput(String),
    UpdateSyncFolderStrategy(String),
    AddSyncFolder(String),            // device_id
    RemoveSyncFolder(String, String), // device_id, folder_id
    StartAddSyncFolder(String),       // device_id
    CancelAddSyncFolder,
    ShowFileSyncSettings(String), // device_id
    CloseFileSyncSettings,
    // Camera settings
    ShowCameraSettings(String), // device_id
    CloseCameraSettings,
    // App Continuity (Open plugin)
    ShowOpenUrlDialog(String),   // device_id
    OpenUrlInput(String),        // url input text
    OpenOnPhone(String, String), // device_id, url
    CancelOpenUrlDialog,
    // Telephony and SMS
    MuteCall(String),      // device_id
    ShowSmsDialog(String), // device_id
    CancelSmsDialog,
    UpdateSmsPhoneNumberInput(String), // phone number text
    UpdateSmsMessageInput(String),     // message body text
    SendSms(String, String, String),   // device_id, phone_number, message
    // Animation
    Tick(std::time::Instant),
    // Recursive loop
    Loop(Box<Message>),
    // Settings window
    CloseSettingsWindow,
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
    let op_cl = op_type.clone();
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
                return self.update(*inner);
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
            Message::UnpairDevice(device_id) => {
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
                    |msg| cosmic::Action::App(msg),
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
                    |msg| cosmic::Action::App(msg),
                )
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
                    .map_or(false, |s| s.is_streaming);

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
            Message::SystemInfoReceived(_device_id, _info) => {
                // TODO: Store and display system info
                tracing::info!("SystemInfoReceived not yet implemented");
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
            Message::ScreenshotReceived(_device_id, _image_data) => {
                // TODO: Save and display screenshot
                tracing::info!("ScreenshotReceived not yet implemented");
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
                return screen_share_control_task(device_id, "pause", |client, id| async move {
                    client.pause_screen_share(&id).await
                });
            }
            Message::ResumeScreenShare(device_id) => {
                tracing::info!("User requested resume screen share with {}", device_id);
                self.update_screen_share_pause_state(&device_id, false);
                return screen_share_control_task(device_id, "resume", |client, id| async move {
                    client.resume_screen_share(&id).await
                });
            }
            Message::StopScreenShare(device_id) => {
                tracing::info!("User requested stop screen share with {}", device_id);
                return screen_share_control_task(device_id, "stop", |client, id| async move {
                    client.stop_screen_share(&id).await
                });
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
                        // TODO: Implement backend support for toggling audio mid-stream
                        // This currently updates UI state only. Full implementation requires:
                        // - DBus method to update stream configuration
                        // - GStreamer pipeline modification to add/remove audio elements
                        // - PipeWire audio node handling via XDG Desktop Portal
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
                return cosmic::task::message(cosmic::Action::App(Message::LoadSyncFolders(
                    device_id,
                )));
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
                                state.core.main_window_id().unwrap(),
                                new_id,
                                None,
                                None,
                                None,
                            );

                            popup_settings.positioner.size_limits = Limits::NONE
                                .min_width(380.0)
                                .max_width(480.0)
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
                .padding(SPACE_XXS)
        ]
        .spacing(SPACE_S)
        .align_y(cosmic::iced::Alignment::Center);

        // Create content sections for different settings
        let content = column![
            header,
            divider::horizontal::default(),
            // TODO: Add the actual settings panels here
            text("Device settings will be moved here:"),
            text("• RemoteDesktop configuration"),
            text("• File Sync folder management"),
            text("• Run Command setup"),
            text("• Plugin overrides"),
            text("• Notification preferences"),
        ]
        .spacing(SPACE_M)
        .padding(SPACE_M);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Formats a duration into a human-readable relative time string.
    fn format_elapsed(elapsed: std::time::Duration) -> String {
        let secs = elapsed.as_secs();
        match secs {
            0..=59 => "Just now".to_string(),
            60..=3599 => format!("{}m ago", secs / 60),
            3600..=86399 => format!("{}h ago", secs / 3600),
            _ => format!("{}d ago", secs / 86400),
        }
    }

    /// Maps file extension to appropriate icon name
    fn file_type_icon(filename: &str) -> &'static str {
        let extension = std::path::Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match extension.as_str() {
            // Images
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "svg" | "webp" | "ico" => {
                "image-x-generic-symbolic"
            }
            // Documents
            "pdf" | "doc" | "docx" | "odt" | "txt" | "rtf" => "x-office-document-symbolic",
            // Spreadsheets
            "xls" | "xlsx" | "ods" | "csv" => "x-office-spreadsheet-symbolic",
            // Presentations
            "ppt" | "pptx" | "odp" => "x-office-presentation-symbolic",
            // Audio
            "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "wma" => "audio-x-generic-symbolic",
            // Video
            "mp4" | "avi" | "mkv" | "mov" | "wmv" | "flv" | "webm" => "video-x-generic-symbolic",
            // Archives
            "zip" | "tar" | "gz" | "bz2" | "7z" | "rar" | "xz" => "package-x-generic-symbolic",
            // Code
            "rs" | "py" | "js" | "ts" | "java" | "c" | "cpp" | "h" | "hpp" => {
                "text-x-script-symbolic"
            }
            // Default
            _ => "text-x-generic-symbolic",
        }
    }

    /// Formats bytes into human-readable size (KB, MB, GB)
    fn format_file_size(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }

    /// Calculates estimated time remaining for a transfer
    fn estimate_time_remaining(state: &TransferState) -> Option<String> {
        if state.total == 0 || state.current >= state.total {
            return None;
        }

        let elapsed = state.last_update.duration_since(state.started_at);
        if elapsed.as_secs() < 1 {
            return Some("Calculating...".to_string());
        }

        let bytes_transferred = state.current;
        let bytes_remaining = state.total.saturating_sub(state.current);

        // Calculate speed based on total elapsed time
        let speed = bytes_transferred as f64 / elapsed.as_secs_f64();

        if speed < 1.0 {
            return Some("Calculating...".to_string());
        }

        let seconds_remaining = (bytes_remaining as f64 / speed) as u64;

        match seconds_remaining {
            0 => Some("Almost done".to_string()),
            1..=59 => Some(format!("{}s left", seconds_remaining)),
            60..=3599 => Some(format!(
                "{}m {}s left",
                seconds_remaining / 60,
                seconds_remaining % 60
            )),
            3600..=86399 => Some(format!(
                "{}h {}m left",
                seconds_remaining / 3600,
                (seconds_remaining % 3600) / 60
            )),
            _ => Some(format!("{}d left", seconds_remaining / 86400)),
        }
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

    fn transfer_queue_view(&self) -> Element<'_, Message> {
        let mut transfers_list = column![].spacing(SPACE_S);

        if self.active_transfers.is_empty() {
            transfers_list = transfers_list.push(
                container(
                    column![
                        icon::from_name("folder-download-symbolic").size(ICON_XL),
                        text("No active transfers"),
                    ]
                    .spacing(SPACE_S)
                    .align_x(Horizontal::Center),
                )
                .width(Length::Fill)
                .align_x(cosmic::iced::Alignment::Center)
                .padding(SPACE_XL),
            );
        } else {
            for (id, state) in &self.active_transfers {
                let progress = if state.total > 0 {
                    (state.current as f32 / state.total as f32) * 100.0
                } else {
                    0.0
                };

                let transfer_id = id.clone();
                let filename = state.filename.clone();
                let is_receiving = state.direction != "sending";

                // Context menu button
                let menu_open = self.context_menu_transfer.as_ref() == Some(id);
                let menu_message = if menu_open {
                    Message::CloseTransferContextMenu
                } else {
                    Message::ShowTransferContextMenu(transfer_id.clone())
                };

                let menu_button = button::icon(icon::from_name("view-more-symbolic").size(ICON_S))
                    .padding(SPACE_XXS)
                    .class(cosmic::theme::Button::Transparent)
                    .on_press(menu_message);

                // File metadata
                let file_icon = Self::file_type_icon(&state.filename);
                let file_size = Self::format_file_size(state.total);
                let bytes_transferred = Self::format_file_size(state.current);

                // Build status text with size and time info
                let mut status_text = if state.direction == "sending" {
                    format!("Sending: {} / {}", bytes_transferred, file_size)
                } else {
                    format!("Receiving: {} / {}", bytes_transferred, file_size)
                };

                // Add time estimate if available
                if let Some(time_left) = Self::estimate_time_remaining(state) {
                    status_text.push_str(&format!(" · {}", time_left));
                }

                let mut transfer_row = row![
                    icon::from_name(file_icon).size(ICON_M),
                    column![
                        text(&state.filename).size(ICON_14),
                        progress_bar(0.0..=100.0, progress).height(Length::Fixed(6.0)),
                        row![
                            text(status_text.clone()).size(ICON_XS),
                            horizontal_space(),
                            text(format!("{:.0}%", progress)).size(ICON_XS),
                        ]
                    ]
                    .spacing(SPACE_XXS)
                    .width(Length::Fill),
                    menu_button,
                ]
                .spacing(SPACE_S)
                .align_y(cosmic::iced::Alignment::Center);

                // Show context menu if this transfer's menu is open
                if self.context_menu_transfer.as_ref() == Some(id) {
                    let menu_items =
                        self.build_transfer_context_menu(&transfer_id, &filename, is_receiving);

                    let context_menu = container(column(menu_items).spacing(SPACE_XXXS))
                        .padding(SPACE_XXS)
                        .class(cosmic::theme::Container::Secondary);

                    transfer_row = transfer_row.push(context_menu);
                }

                transfers_list = transfers_list.push(
                    container(transfer_row)
                        .padding(SPACE_S)
                        .class(cosmic::theme::Container::Card),
                );
            }
        }

        // Build received files history section
        let mut history_section = column![].spacing(SPACE_S);

        if !self.received_files_history.is_empty() {
            history_section = history_section.push(
                row![
                    icon::from_name("folder-recent-symbolic").size(ICON_S),
                    cosmic::widget::text::heading("Recently Received"),
                ]
                .spacing(SPACE_S)
                .align_y(cosmic::iced::Alignment::Center),
            );

            for received in self
                .received_files_history
                .iter()
                .take(MAX_DISPLAYED_HISTORY_ITEMS)
            {
                let status_icon = if received.success {
                    "emblem-ok-symbolic"
                } else {
                    "emblem-error-symbolic"
                };
                let time_str = Self::format_elapsed(received.timestamp.elapsed());

                let open_button: Element<'_, Message> = if received.success {
                    cosmic::widget::tooltip(
                        button::icon(icon::from_name("document-open-symbolic").size(ICON_S))
                            .padding(SPACE_XXS)
                            .class(cosmic::theme::Button::Transparent)
                            .on_press(Message::OpenTransferFile(received.filename.clone())),
                        "Open file",
                        cosmic::widget::tooltip::Position::Bottom,
                    )
                    .into()
                } else {
                    cosmic::iced::widget::Space::new(0, 0).into()
                };

                let file_row = row![
                    icon::from_name("document-save-symbolic").size(ICON_S),
                    column![
                        text(&received.filename).size(ICON_14),
                        row![
                            icon::from_name(status_icon).size(ICON_XS),
                            text(format!("from {} • {}", received.device_name, time_str))
                                .size(ICON_XS),
                        ]
                        .spacing(SPACE_XXS)
                        .align_y(cosmic::iced::Alignment::Center),
                    ]
                    .spacing(SPACE_XXS)
                    .width(Length::Fill),
                    open_button,
                ]
                .spacing(SPACE_S)
                .align_y(cosmic::iced::Alignment::Center);

                history_section = history_section.push(
                    container(file_row)
                        .padding(SPACE_S)
                        .class(cosmic::theme::Container::Card),
                );
            }
        }

        // Combine active transfers and history
        let mut all_content = column![transfers_list].spacing(SPACE_M);

        if !self.received_files_history.is_empty() {
            all_content = all_content.push(history_section);
        }

        column![
            row![
                cosmic::widget::tooltip(
                    button::icon(icon::from_name("go-previous-symbolic").size(ICON_S))
                        .on_press(Message::SetViewMode(ViewMode::Devices))
                        .padding(SPACE_XS),
                    "Back",
                    cosmic::widget::tooltip::Position::Bottom,
                ),
                text("Transfer Queue").size(ICON_M),
                horizontal_space(),
            ]
            .spacing(SPACE_S)
            .align_y(cosmic::iced::Alignment::Center),
            scrollable(all_content).height(Length::Fill)
        ]
        .spacing(SPACE_M)
        .padding(SPACE_M)
        .into()
    }

    fn popup_view(&self) -> Element<'_, Message> {
        let mut content = self.inner_view();

        // Screen share control overlay - shows when actively sharing/receiving
        if let Some(screen_share) = &self.active_screen_share {
            let device_name = self
                .devices
                .iter()
                .find(|d| d.device.id() == screen_share.device_id)
                .map(|d| d.device.name().to_string())
                .unwrap_or_else(|| screen_share.device_id.clone());

            let viewer_count = screen_share.viewer_count;
            let (status_text, status_caption) = if screen_share.is_sender {
                if screen_share.is_paused {
                    (
                        format!("Sharing paused to {}", device_name),
                        "Screen sharing paused".to_string(),
                    )
                } else {
                    let viewer_text = if viewer_count == 1 {
                        "1 viewer".to_string()
                    } else {
                        format!("{} viewers", viewer_count)
                    };
                    (format!("Sharing screen to {}", device_name), viewer_text)
                }
            } else {
                (
                    format!("Receiving screen from {}", device_name),
                    "Screen sharing active".to_string(),
                )
            };

            let device_id = screen_share.device_id.clone();
            let device_id_for_pause = device_id.clone();
            let device_id_for_audio = device_id.clone();
            let is_paused = screen_share.is_paused;
            let is_sender = screen_share.is_sender;
            let include_audio = screen_share.include_audio;

            // Build control buttons
            let mut controls = row![].spacing(SPACE_XS);

            // Audio toggle (only for sender)
            if is_sender {
                let audio_toggle = cosmic::widget::toggler(include_audio)
                    .label("Include Audio")
                    .on_toggle(move |enabled| {
                        Message::ToggleScreenShareAudio(device_id_for_audio.clone(), enabled)
                    });
                controls = controls.push(audio_toggle);
            }

            // Pause/Resume button (only for sender)
            if is_sender {
                let pause_resume_btn = if is_paused {
                    button::standard("Resume")
                        .on_press(Message::ResumeScreenShare(device_id_for_pause))
                        .padding(SPACE_XXS)
                } else {
                    button::standard("Pause")
                        .on_press(Message::PauseScreenShare(device_id_for_pause))
                        .padding(SPACE_XXS)
                };
                controls = controls.push(pause_resume_btn);
            }

            // Stop button
            controls = controls.push(
                button::destructive("Stop")
                    .on_press(Message::StopScreenShare(device_id))
                    .padding(SPACE_XXS),
            );

            let control_row = row![
                icon::from_name("video-display-symbolic").size(ICON_S),
                column![
                    text(status_text),
                    cosmic::widget::text::caption(status_caption),
                ]
                .spacing(SPACE_XXXS),
                horizontal_space(),
                controls,
            ]
            .spacing(SPACE_S)
            .align_y(cosmic::iced::Alignment::Center);

            // Quality settings panel (only for sender)
            let mut overlay_content = vec![container(control_row)
                .width(Length::Fill)
                .padding(SPACE_S)
                .class(cosmic::theme::Container::Primary)
                .into()];

            if is_sender {
                let current_quality = &screen_share.quality;
                let current_fps = screen_share.fps;

                // Quality preset buttons
                let quality_buttons = row![
                    text("Quality:").width(Length::Fixed(60.0)),
                    button::text("Low")
                        .on_press(Message::SetScreenShareQuality("low".to_string()))
                        .padding(SPACE_XXS)
                        .class(if current_quality == "low" {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        }),
                    button::text("Medium")
                        .on_press(Message::SetScreenShareQuality("medium".to_string()))
                        .padding(SPACE_XXS)
                        .class(if current_quality == "medium" {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        }),
                    button::text("High")
                        .on_press(Message::SetScreenShareQuality("high".to_string()))
                        .padding(SPACE_XXS)
                        .class(if current_quality == "high" {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        }),
                ]
                .spacing(SPACE_XS)
                .align_y(cosmic::iced::Alignment::Center);

                // FPS buttons
                let fps_buttons = row![
                    text("FPS:").width(Length::Fixed(60.0)),
                    button::text("15")
                        .on_press(Message::SetScreenShareFps(15))
                        .padding(SPACE_XXS)
                        .class(if current_fps == 15 {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        }),
                    button::text("30")
                        .on_press(Message::SetScreenShareFps(30))
                        .padding(SPACE_XXS)
                        .class(if current_fps == 30 {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        }),
                    button::text("60")
                        .on_press(Message::SetScreenShareFps(60))
                        .padding(SPACE_XXS)
                        .class(if current_fps == 60 {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        }),
                ]
                .spacing(SPACE_XS)
                .align_y(cosmic::iced::Alignment::Center);

                let settings_panel = column![quality_buttons, fps_buttons,].spacing(SPACE_S);

                overlay_content.push(
                    container(settings_panel)
                        .width(Length::Fill)
                        .padding(SPACE_S)
                        .class(cosmic::theme::Container::Secondary)
                        .into(),
                );
            }

            overlay_content.push(content);

            content = column(overlay_content).spacing(SPACE_S).into();
        }

        // Keyboard shortcuts help dialog
        if self.show_keyboard_shortcuts_help {
            let shortcuts_content = column![
                row![
                    cosmic::widget::text::title3("Keyboard Shortcuts").width(Length::Fill),
                    cosmic::widget::tooltip(
                        button::icon(icon::from_name("window-close-symbolic").size(ICON_14))
                            .on_press(Message::ToggleKeyboardShortcutsHelp)
                            .padding(SPACE_XXS),
                        "Close",
                        cosmic::widget::tooltip::Position::Bottom,
                    )
                ]
                .align_y(cosmic::iced::Alignment::Center),
                divider::horizontal::default(),
                column![
                    row![
                        cosmic::widget::text::body("Escape").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Close dialogs/overlays")
                            .width(Length::FillPortion(3)),
                    ]
                    .spacing(SPACE_S),
                    row![
                        cosmic::widget::text::body("Ctrl+R").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Refresh devices").width(Length::FillPortion(3)),
                    ]
                    .spacing(SPACE_S),
                    row![
                        cosmic::widget::text::body("Ctrl+F").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Focus search").width(Length::FillPortion(3)),
                    ]
                    .spacing(SPACE_S),
                    row![
                        cosmic::widget::text::body("Ctrl+,").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Toggle device settings")
                            .width(Length::FillPortion(3)),
                    ]
                    .spacing(SPACE_S),
                    row![
                        cosmic::widget::text::body("Ctrl+M").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Open Manager").width(Length::FillPortion(3)),
                    ]
                    .spacing(SPACE_S),
                    divider::horizontal::light(),
                    cosmic::widget::text::title4("Navigation"),
                    row![
                        cosmic::widget::text::body("Tab / Shift+Tab").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Next/Previous element")
                            .width(Length::FillPortion(3)),
                    ]
                    .spacing(SPACE_S),
                    row![
                        cosmic::widget::text::body("Arrow Keys").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Navigate elements")
                            .width(Length::FillPortion(3)),
                    ]
                    .spacing(SPACE_S),
                    row![
                        cosmic::widget::text::body("Enter / Space").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Activate focused element")
                            .width(Length::FillPortion(3)),
                    ]
                    .spacing(SPACE_S),
                ]
                .spacing(SPACE_XS),
            ]
            .spacing(SPACE_S);

            content = column![
                container(shortcuts_content)
                    .padding(SPACE_M)
                    .class(cosmic::theme::Container::Card),
                content
            ]
            .spacing(SPACE_S)
            .into();
        }

        if let Some(notification) = &self.notification {
            let icon_name = match notification.kind {
                NotificationType::Error => "dialog-error-symbolic",
                NotificationType::Success => "emblem-ok-symbolic",
                NotificationType::Info => "dialog-information-symbolic",
            };

            // Hack: Card works, but we ideally want Success/Danger.
            // Since Container::Success/Danger might not be exposed or mapped correctly in styling yet,
            // we will stick to Card for now but use icon/text to differentiate, or try to use proper classes if available.
            // Actually, cosmic::theme::Container has variants. Let's try to map them if possible, otherwise Card + Icon is safe.
            // For now, let's use Card for everything to be safe on compilation, but maybe colored text?
            // Wait, I can't easily change container background without correct theme support.
            // Let's stick to Card and just change the icon.

            let mut notification_row = row![
                icon::from_name(icon_name),
                text(notification.message.clone()).width(Length::Fill),
            ]
            .spacing(SPACE_XS)
            .align_y(cosmic::iced::Alignment::Center);

            if let Some((label, msg)) = &notification.action {
                notification_row = notification_row.push(
                    button::text(label)
                        .on_press(Message::Loop(msg.clone()))
                        .padding(SPACE_XXS),
                );
            }

            column![
                container(
                    container(notification_row)
                        .padding(SPACE_S)
                        .class(cosmic::theme::Container::Card)
                )
                .height(Length::Fixed(self.notification_progress * 50.0))
                .clip(true),
                content
            ]
            .spacing(if self.notification_progress > 0.0 {
                SPACE_S
            } else {
                0.0
            })
            .into()
        } else if !self.daemon_connected {
            column![
                container(
                    row![
                        icon::from_name("dialog-warning-symbolic").size(ICON_XS),
                        text("Disconnected from background daemon").size(ICON_XS),
                    ]
                    .spacing(SPACE_XS)
                    .align_y(cosmic::iced::Alignment::Center)
                )
                .width(Length::Fill)
                .padding(SPACE_XXS)
                .class(cosmic::theme::Container::Card),
                content
            ]
            .spacing(SPACE_XS)
            .into()
        } else {
            content
        }
    }

    fn inner_view(&self) -> Element<'_, Message> {
        // App Continuity dialog (Open on Phone)
        if let Some(device_id) = &self.open_url_dialog_device {
            return self.open_url_dialog_view(device_id);
        }
        // SMS dialog
        if let Some(device_id) = &self.sms_dialog_device {
            return self.sms_dialog_view(device_id);
        }

        // Settings overrides
        if let Some(device_id) = &self.remotedesktop_settings_device {
            if let Some(settings) = self.remotedesktop_settings.get(device_id) {
                return self.remotedesktop_settings_view(device_id, settings);
            }
        }
        if let Some(device_id) = &self.file_sync_settings_device {
            return self.file_sync_settings_view(device_id);
        }
        if let Some(device_id) = &self.run_command_settings_device {
            return self.run_command_settings_view(device_id);
        }

        if let ViewMode::DeviceDetails(device_id) = &self.view_mode {
            return self.device_details_view(device_id);
        }
        if self.view_mode == ViewMode::TransferQueue {
            return self.transfer_queue_view();
        }

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

        let search_input = cosmic::widget::tooltip(
            cosmic::widget::text_input("Search devices...", &self.search_query)
                .on_input(Message::SearchChanged)
                .width(Length::Fill),
            "Search devices (Ctrl+F)",
            cosmic::widget::tooltip::Position::Bottom,
        );

        let header = row![view_switcher,]
            .spacing(SPACE_S)
            .align_y(cosmic::iced::Alignment::Center)
            .width(Length::Fill);

        let controls = if self.scanning {
            row![
                search_input,
                container(
                    row![
                        icon::from_name("process-working-symbolic").size(ICON_S),
                        cosmic::widget::text::caption("Scanning..."),
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
                cosmic::widget::tooltip(
                    button::icon(icon::from_name("view-refresh-symbolic"))
                        .on_press(Message::RefreshDevices)
                        .padding(SPACE_XXS),
                    "Refresh devices (Ctrl+R)",
                    cosmic::widget::tooltip::Position::Bottom,
                ),
                cosmic::widget::tooltip(
                    button::icon(icon::from_name("help-about-symbolic").size(ICON_S))
                        .on_press(Message::ToggleKeyboardShortcutsHelp)
                        .padding(SPACE_XXS),
                    "Keyboard shortcuts",
                    cosmic::widget::tooltip::Position::Bottom,
                ),
                cosmic::widget::tooltip(
                    button::icon(icon::from_name("preferences-desktop-apps-symbolic").size(ICON_S))
                        .on_press(Message::OpenManager)
                        .padding(SPACE_XXS),
                    "Open Manager (Ctrl+M)",
                    cosmic::widget::tooltip::Position::Bottom,
                )
            ]
            .spacing(SPACE_S)
        };

        // MPRIS media controls section
        let mpris_section = self.mpris_controls_view();

        // Camera streaming controls section
        let camera_section = self.camera_controls_view();

        let content: Element<'_, Message> = if self.devices.is_empty() {
            container(
                column![
                    container(icon::from_name("phone-disconnected-symbolic").size(ICON_XL))
                        .padding(Padding::new(0.0).bottom(SPACE_M)),
                    cosmic::widget::text::heading("No Devices Connected"),
                    column![
                        cosmic::widget::text::body("Make sure your devices are:"),
                        cosmic::widget::text::caption("• On the same network"),
                        cosmic::widget::text::caption("• Running the CConnect app"),
                    ]
                    .spacing(SPACE_XS)
                    .align_x(Horizontal::Center),
                    container(
                        button::text("Refresh Devices")
                            .on_press(Message::RefreshDevices)
                            .padding(SPACE_S)
                    )
                    .padding(Padding::new(0.0).top(SPACE_M)),
                ]
                .spacing(SPACE_S)
                .align_x(Horizontal::Center),
            )
            .padding(SPACE_XXL)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Center)
            .align_y(cosmic::iced::Alignment::Center)
            .into()
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

            // Sort each category: pinned devices first
            let sort_by_pinned = |a: &&DeviceState, b: &&DeviceState| {
                let a_pinned = self
                    .pinned_devices_config
                    .is_pinned(&a.device.info.device_id);
                let b_pinned = self
                    .pinned_devices_config
                    .is_pinned(&b.device.info.device_id);
                match (a_pinned, b_pinned) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                }
            };

            connected.sort_by(sort_by_pinned);
            available.sort_by(sort_by_pinned);
            offline.sort_by(sort_by_pinned);

            let mut device_groups = column![].spacing(SPACE_XXS).width(Length::Fill);
            // Track device index for focus navigation (matches filtered_devices() order)
            let mut device_index = 0usize;

            // Connected devices section
            if !connected.is_empty() {
                device_groups = device_groups.push(
                    container(cosmic::widget::text::caption("Connected"))
                        .padding(Padding::from([SPACE_S, SPACE_M, SPACE_XXS, SPACE_M]))
                        .width(Length::Fill),
                );
                for device_state in &connected {
                    device_groups = device_groups.push(self.device_row(device_state, device_index));
                    device_index += 1;
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
                    device_groups = device_groups.push(self.device_row(device_state, device_index));
                    device_index += 1;
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
                    device_groups = device_groups.push(self.device_row(device_state, device_index));
                    device_index += 1;
                }
            }

            device_groups.into()
        };

        let content = content;

        let popup_content = column![
            container(header)
                .padding(Padding::from([SPACE_S, SPACE_M]))
                .width(Length::Fill),
            container(controls)
                .padding(Padding::from([0.0, SPACE_M, SPACE_S, SPACE_M]))
                .width(Length::Fill),
            divider::horizontal::default(),
            mpris_section,
            camera_section,
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

        // Player name/label with context menu button
        let menu_message = if self.context_menu_mpris {
            Message::CloseMprisContextMenu
        } else {
            Message::ShowMprisContextMenu
        };

        let menu_button = button::icon(icon::from_name("view-more-symbolic").size(ICON_S))
            .padding(SPACE_XXS)
            .class(cosmic::theme::Button::Transparent)
            .on_press(menu_message);

        let player_name = row![
            icon::from_name("multimedia-player-symbolic").size(ICON_S),
            cosmic::widget::text::body(selected_player),
            horizontal_space(),
            menu_button,
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

        let (play_icon, play_action) = match status {
            dbus_client::PlaybackStatus::Playing => ("media-playback-pause-symbolic", "Pause"),
            _ => ("media-playback-start-symbolic", "Play"),
        };

        let controls = row![
            cosmic::widget::tooltip(
                button::icon(icon::from_name("media-skip-backward-symbolic").size(ICON_S))
                    .on_press(Message::MprisControl(
                        selected_player.clone(),
                        "Previous".to_string(),
                    ))
                    .padding(SPACE_XS),
                "Previous",
                cosmic::widget::tooltip::Position::Bottom,
            ),
            cosmic::widget::tooltip(
                button::icon(icon::from_name(play_icon).size(ICON_S))
                    .on_press(Message::MprisControl(
                        selected_player.clone(),
                        play_action.to_string()
                    ))
                    .padding(SPACE_XS),
                play_action,
                cosmic::widget::tooltip::Position::Bottom,
            ),
            cosmic::widget::tooltip(
                button::icon(icon::from_name("media-playback-stop-symbolic").size(ICON_S))
                    .on_press(Message::MprisControl(
                        selected_player.clone(),
                        "Stop".to_string(),
                    ))
                    .padding(SPACE_XS),
                "Stop",
                cosmic::widget::tooltip::Position::Bottom,
            ),
            cosmic::widget::tooltip(
                button::icon(icon::from_name("media-skip-forward-symbolic").size(ICON_S))
                    .on_press(Message::MprisControl(
                        selected_player.clone(),
                        "Next".to_string(),
                    ))
                    .padding(SPACE_XS),
                "Next",
                cosmic::widget::tooltip::Position::Bottom,
            ),
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
                container(icon::from_name("audio-x-generic-symbolic").size(ICON_L))
                    .width(Length::Fixed(50.0))
                    .align_x(cosmic::iced::Alignment::Center),
                metadata_col
            ]
            .spacing(SPACE_M)
            .align_y(cosmic::iced::Alignment::Center)
        };

        let mut content = column![player_name, info_row, controls]
            .spacing(SPACE_S)
            .padding(Padding::from([SPACE_S, SPACE_M]));

        // Show context menu if open
        if self.context_menu_mpris {
            let menu_item = |icon_name: &'static str,
                             label: &'static str,
                             message: Message|
             -> Element<'_, Message> {
                button::custom(
                    row![
                        icon::from_name(icon_name).size(ICON_S),
                        text(label).size(ICON_14),
                    ]
                    .spacing(SPACE_S)
                    .align_y(cosmic::iced::Alignment::Center),
                )
                .width(Length::Fill)
                .padding([SPACE_XXS, SPACE_S])
                .class(cosmic::theme::Button::MenuItem)
                .on_press(message)
                .into()
            };

            let menu_items = vec![
                menu_item(
                    "dialog-information-symbolic",
                    "Show track info",
                    Message::ShowMprisTrackInfo,
                ),
                menu_item(
                    "window-maximize-symbolic",
                    "Open player",
                    Message::RaiseMprisPlayer,
                ),
            ];

            let context_menu = container(column(menu_items).spacing(SPACE_XXXS))
                .padding(SPACE_XXS)
                .class(cosmic::theme::Container::Secondary);

            content = content.push(context_menu);
        }

        container(content)
            .width(Length::Fill)
            .class(cosmic::theme::Container::Card)
            .into()
    }

    /// Camera streaming controls view
    fn camera_controls_view(&self) -> Element<'_, Message> {
        // Check if v4l2loopback is available
        if !self.v4l2loopback_available {
            return Element::from(cosmic::iced::widget::Space::new(0, 0));
        }

        // Find devices with camera capability
        let camera_devices: Vec<_> = self
            .devices
            .iter()
            .filter(|d| {
                d.device
                    .info
                    .incoming_capabilities
                    .iter()
                    .any(|cap| cap == "cconnect.camera")
            })
            .collect();

        if camera_devices.is_empty() {
            return Element::from(cosmic::iced::widget::Space::new(0, 0));
        }

        // For now, show UI for the first camera-capable device
        // TODO: Support multiple devices or device selection
        let device_state = camera_devices[0];
        let device_id = &device_state.device.info.device_id;
        let device_name = &device_state.device.info.device_name;

        // Get camera stats if available
        let stats = self.camera_stats.get(device_id);
        let is_streaming = stats.map_or(false, |s| s.is_streaming);

        // Camera header with toggle
        let camera_header = row![
            icon::from_name("camera-web-symbolic").size(ICON_S),
            cosmic::widget::text::body(format!("Camera: {}", device_name)),
            horizontal_space(),
            cosmic::widget::toggler(is_streaming)
                .on_toggle(move |_| Message::ToggleCameraStreaming(device_id.clone()))
        ]
        .spacing(SPACE_XS)
        .align_y(cosmic::iced::Alignment::Center);

        let mut content_col = column![camera_header].spacing(SPACE_S);

        // Show controls only when streaming
        if is_streaming {
            if let Some(stats) = stats {
                // Camera selection dropdown (mock for now)
                let camera_label = row![
                    cosmic::widget::text::caption("Camera:"),
                    horizontal_space(),
                    cosmic::widget::text::caption(if stats.camera_id == 0 {
                        "Back"
                    } else {
                        "Front"
                    }),
                ]
                .spacing(SPACE_XS);

                // Resolution display
                let resolution_label = row![
                    cosmic::widget::text::caption("Resolution:"),
                    horizontal_space(),
                    cosmic::widget::text::caption(&stats.resolution),
                ]
                .spacing(SPACE_XS);

                // Statistics
                let stats_row = row![
                    column![
                        cosmic::widget::text::caption("FPS:"),
                        cosmic::widget::text::body(format!("{}", stats.fps)),
                    ]
                    .spacing(SPACE_XXS),
                    horizontal_space(),
                    column![
                        cosmic::widget::text::caption("Bitrate:"),
                        cosmic::widget::text::body(format!("{} kbps", stats.bitrate)),
                    ]
                    .spacing(SPACE_XXS),
                ]
                .spacing(SPACE_M);

                content_col = content_col.push(divider::horizontal::default());
                content_col = content_col.push(camera_label);
                content_col = content_col.push(resolution_label);
                content_col = content_col.push(stats_row);
            }
        } else {
            // Show helper text when not streaming
            let helper_text = cosmic::widget::text::caption(
                "Toggle to use phone camera as webcam (/dev/video10)",
            );
            content_col = content_col.push(helper_text);
        }

        container(content_col.padding(Padding::from([SPACE_S, SPACE_M])))
            .width(Length::Fill)
            .class(cosmic::theme::Container::Card)
            .into()
    }

    fn device_details_view(&self, device_id: &str) -> Element<'_, Message> {
        let device_state = self
            .devices
            .iter()
            .find(|d| d.device.info.device_id == device_id);

        let Some(device_state) = device_state else {
            return container(text("Device not found")).padding(SPACE_M).into();
        };

        let device = &device_state.device;
        let _config = self.device_configs.get(device_id);

        let header = row![
            cosmic::widget::tooltip(
                button::icon(icon::from_name("go-previous-symbolic").size(ICON_S))
                    .on_press(Message::CloseDeviceDetails)
                    .padding(SPACE_XS),
                "Back",
                cosmic::widget::tooltip::Position::Bottom,
            ),
            text(&device.info.device_name).size(ICON_M),
            horizontal_space(),
        ]
        .spacing(SPACE_S)
        .align_y(cosmic::iced::Alignment::Center);

        // Basic Info
        let info_card = column![
            row![
                text("Status:").width(Length::Fixed(100.0)),
                connection_status_styled_text(device.connection_state, device.pairing_status)
            ]
            .spacing(SPACE_XS),
            row![
                text("Type:").width(Length::Fixed(100.0)),
                text(format!("{:?}", device.info.device_type))
            ]
            .spacing(SPACE_XS),
            row![
                text("IP Address:").width(Length::Fixed(100.0)),
                text(device.host.as_deref().unwrap_or("Unknown"))
            ]
            .spacing(SPACE_XS),
            row![
                text("ID:").width(Length::Fixed(100.0)),
                text(if device_id.len() > 20 {
                    format!("{}...", &device_id[..20])
                } else {
                    device_id.to_string()
                })
            ]
            .spacing(SPACE_XS),
        ]
        .spacing(SPACE_S);

        let content = column![
            header,
            container(info_card)
                .padding(SPACE_M)
                .width(Length::Fill)
                .class(cosmic::theme::Container::Card),
        ]
        .spacing(SPACE_M)
        .padding(SPACE_M);

        content.into()
    }

    fn device_row<'a>(
        &'a self,
        device_state: &'a DeviceState,
        device_index: usize,
    ) -> Element<'a, Message> {
        let device = &device_state.device;
        let device_id = &device.info.device_id;

        // Check if this device is focused for keyboard navigation
        let is_focused = matches!(&self.focus_target,
            FocusTarget::Device(idx) |
            FocusTarget::DeviceAction(idx, _)
            if *idx == device_index
        );

        let device_icon = device_type_icon(device.info.device_type);

        // Device name
        let nickname = self
            .device_configs
            .get(device_id)
            .and_then(|c| c.nickname.as_deref());

        let display_name = nickname.unwrap_or(&device.info.device_name);

        // Metadata row: Status • Battery • Last Seen
        let mut metadata_row = row![connection_status_styled_text(
            device.connection_state,
            device.pairing_status
        )]
        .spacing(SPACE_XS)
        .align_y(cosmic::iced::Alignment::Center);

        // Add battery if available
        if let Some(level) = device_state.battery_level {
            metadata_row = metadata_row.push(
                text("•")
                    .size(ICON_XS)
                    .class(theme::Text::Color(theme_muted_color())),
            );

            let battery_icon = battery_icon_name(level, device_state.is_charging);
            metadata_row = metadata_row.push(
                row![
                    icon::from_name(battery_icon).size(ICON_XS),
                    cosmic::widget::text::caption(format!("{}%", level)),
                ]
                .spacing(SPACE_XXXS)
                .align_y(cosmic::iced::Alignment::Center),
            );
        } else if self.loading_battery && device.is_connected() {
            metadata_row = metadata_row.push(
                text("•")
                    .size(ICON_XS)
                    .class(theme::Text::Color(theme_muted_color())),
            );
            metadata_row =
                metadata_row.push(icon::from_name("process-working-symbolic").size(ICON_XS));
        }

        // Add last seen if disconnected
        if !device.is_connected() && device.last_seen > 0 {
            let last_seen_text = format_last_seen(device.last_seen);
            metadata_row = metadata_row.push(
                text("•")
                    .size(ICON_XS)
                    .class(theme::Text::Color(theme_muted_color())),
            );
            metadata_row = metadata_row.push(cosmic::widget::text::caption(last_seen_text));
        }

        // Combine Name + Metadata
        let info_col = column![cosmic::widget::text::heading(display_name), metadata_row]
            .spacing(SPACE_XXXS)
            .width(Length::Fill);

        // Pin/favorite button
        let is_pinned = self.pinned_devices_config.is_pinned(device_id);
        let star_icon = if is_pinned {
            "starred-symbolic"
        } else {
            "non-starred-symbolic"
        };
        let star_button = cosmic::widget::tooltip(
            button::icon(icon::from_name(star_icon).size(ICON_S))
                .on_press(Message::ToggleDevicePin(device_id.to_string()))
                .padding(SPACE_XXS)
                .class(cosmic::theme::Button::Icon),
            if is_pinned {
                "Unpin device"
            } else {
                "Pin device"
            },
            cosmic::widget::tooltip::Position::Bottom,
        );

        // Build actions
        let actions_row = self.build_device_actions(device, device_id);

        // Main device row layout
        let mut content = column![
            row![
                container(icon::from_name(device_icon).size(ICON_L))
                    .width(Length::Fixed(48.0))
                    .align_x(Horizontal::Center)
                    .padding(Padding::new(SPACE_S)),
                info_col,
                star_button,
            ]
            .spacing(SPACE_S)
            .align_y(cosmic::iced::Alignment::Center)
            .width(Length::Fill),
            // Actions row below
            container(actions_row)
                .width(Length::Fill)
                .padding(Padding::new(0.0).left(48.0 + SPACE_S)) // Indent to align with text
                .align_x(Horizontal::Left),
        ]
        .spacing(SPACE_S)
        .padding(SPACE_M)
        .width(Length::Fill);

        // Add RemoteDesktop settings panel if active
        if self.remotedesktop_settings_device.as_ref() == Some(device_id) {
            if let Some(settings) = self.remotedesktop_settings.get(device_id) {
                content = content.push(
                    container(self.remotedesktop_settings_view(device_id, settings))
                        .padding(Padding::from([0.0, 0.0, 0.0, 48.0 + SPACE_S])),
                );
            }
        }

        // Add FileSync settings panel if active
        if self.file_sync_settings_device.as_ref() == Some(device_id) {
            content = content.push(
                container(self.file_sync_settings_view(device_id)).padding(Padding::from([
                    0.0,
                    0.0,
                    0.0,
                    48.0 + SPACE_S,
                ])),
            );
        }

        // Add RunCommand settings panel if active
        if self.run_command_settings_device.as_ref() == Some(device_id) {
            content = content.push(
                container(self.run_command_settings_view(device_id)).padding(Padding::from([
                    0.0,
                    0.0,
                    0.0,
                    48.0 + SPACE_S,
                ])),
            );
        }

        // Add Camera settings panel if active
        if self.camera_settings_device.as_ref() == Some(device_id) {
            content = content.push(
                container(self.camera_settings_view(device_id)).padding(Padding::from([
                    0.0,
                    0.0,
                    0.0,
                    48.0 + SPACE_S,
                ])),
            );
        }

        // Add context menu if open for this device
        if self.context_menu_device.as_ref() == Some(device_id) {
            content = content.push(
                container(self.device_context_menu_view(device_id, device))
                    .padding(Padding::from([0.0, 0.0, 0.0, 48.0 + SPACE_S])),
            );
        }

        // Check if this device is a valid drop target
        let can_receive_files = device.is_connected()
            && device.is_paired()
            && device.has_incoming_capability("cconnect.share");
        let show_drop_zone = self.dragging_files && can_receive_files;
        let is_drag_target = show_drop_zone && self.drag_hover_device.as_ref() == Some(device_id);

        // Add drop zone indicator when dragging files
        if show_drop_zone {
            content = content.push(
                container(
                    row![
                        icon::from_name("document-save-symbolic").size(ICON_S),
                        cosmic::widget::text::body("Drop file here"),
                    ]
                    .spacing(SPACE_S)
                    .align_y(cosmic::iced::Alignment::Center),
                )
                .padding(SPACE_S)
                .width(Length::Fill)
                .align_x(Horizontal::Center)
                .class(cosmic::theme::Container::Secondary),
            );
        }

        // Apply focus/drag indicator styling
        let container_class = if is_drag_target || is_focused {
            cosmic::theme::Container::Primary
        } else {
            cosmic::theme::Container::Card
        };

        // Wrap in button for click-to-select as drop target when dragging
        if show_drop_zone {
            button::custom(
                container(content)
                    .width(Length::Fill)
                    .class(container_class),
            )
            .on_press(Message::SetDragHoverDevice(Some(device_id.to_string())))
            .padding(0)
            .class(cosmic::theme::Button::Transparent)
            .width(Length::Fill)
            .into()
        } else {
            container(content)
                .width(Length::Fill)
                .class(container_class)
                .into()
        }
    }

    fn build_device_actions<'a>(
        &self,
        device: &'a Device,
        device_id: &str,
    ) -> cosmic::iced::widget::Row<'a, Message, cosmic::Theme> {
        let mut actions = row![].spacing(SPACE_S);

        // Quick actions for connected & paired devices
        if device.is_connected() && device.is_paired() {
            let is_pinging = self
                .pending_operations
                .contains(&(device_id.to_string(), OperationType::Ping));
            actions = actions.push(action_button_with_tooltip_loading(
                "user-available-symbolic",
                "Send ping",
                Message::SendPing(device_id.to_string()),
                is_pinging,
            ));

            if device.has_incoming_capability("cconnect.share") {
                actions = actions
                    .push(action_button_with_tooltip(
                        "document-send-symbolic",
                        "Send file",
                        Message::SendFile(device_id.to_string()),
                    ))
                    .push(action_button_with_tooltip_loading(
                        "insert-text-symbolic",
                        "Share clipboard text",
                        Message::ShareText(device_id.to_string()),
                        self.pending_operations
                            .contains(&(device_id.to_string(), OperationType::ShareText)),
                    ))
                    .push(action_button_with_tooltip_loading(
                        "send-to-symbolic",
                        "Share URL",
                        Message::ShareUrl(device_id.to_string()),
                        self.pending_operations
                            .contains(&(device_id.to_string(), OperationType::ShareUrl)),
                    ))
                    .push(action_button_with_tooltip(
                        "smartphone-symbolic",
                        "Open on Phone (App Continuity)",
                        Message::ShowOpenUrlDialog(device_id.to_string()),
                    ));
            }

            // Add Find My Phone if supported
            if device.has_incoming_capability("cconnect.findmyphone.request") {
                let is_ringing = self
                    .pending_operations
                    .contains(&(device_id.to_string(), OperationType::FindPhone));
                actions = actions.push(action_button_with_tooltip_loading(
                    "find-location-symbolic",
                    "Ring device",
                    Message::FindPhone(device_id.to_string()),
                    is_ringing,
                ));
            }

            // Lock device button
            if device.has_incoming_capability("cconnect.lock.request") {
                actions = actions.push(action_button_with_tooltip(
                    "system-lock-screen-symbolic",
                    "Lock device",
                    Message::LockDevice(device_id.to_string()),
                ));
            }

            // Power control button (shutdown)
            if device.has_incoming_capability("cconnect.power.request") {
                actions = actions.push(action_button_with_tooltip(
                    "system-shutdown-symbolic",
                    "Shutdown device",
                    Message::PowerAction(device_id.to_string(), "shutdown".to_string()),
                ));
            }

            // Wake-on-LAN button (for offline devices)
            if device.has_incoming_capability("cconnect.wol.request") {
                actions = actions.push(action_button_with_tooltip(
                    "network-wired-symbolic",
                    "Wake device",
                    Message::WakeDevice(device_id.to_string()),
                ));
            }

            // System Volume button
            if device.has_incoming_capability("cconnect.systemvolume.request") {
                actions = actions.push(action_button_with_tooltip(
                    "multimedia-volume-control-symbolic",
                    "Control volume",
                    Message::SetDeviceVolume(device_id.to_string(), 0.5),
                ));
            }

            // System Monitor button
            if device.has_incoming_capability("cconnect.systemmonitor.request") {
                actions = actions.push(action_button_with_tooltip(
                    "utilities-system-monitor-symbolic",
                    "Get system info",
                    Message::RequestSystemInfo(device_id.to_string()),
                ));
            }

            // Screenshot button
            if device.has_incoming_capability("cconnect.screenshot.request") {
                actions = actions.push(action_button_with_tooltip(
                    "camera-photo-symbolic",
                    "Take screenshot",
                    Message::TakeScreenshot(device_id.to_string()),
                ));
            }

            // Audio Stream toggle button
            if device.has_incoming_capability("cconnect.audiostream") {
                let is_streaming = self.audio_streaming_devices.contains(device_id);

                // Telephony - Mute Call button
                if device.has_incoming_capability("cconnect.telephony") {
                    let is_muting = self
                        .pending_operations
                        .contains(&(device_id.to_string(), OperationType::MuteCall));
                    actions = actions.push(action_button_with_tooltip_loading(
                        "audio-volume-muted-symbolic",
                        "Mute incoming call",
                        Message::MuteCall(device_id.to_string()),
                        is_muting,
                    ));
                }

                // SMS button
                if device.has_incoming_capability("cconnect.sms") {
                    actions = actions.push(action_button_with_tooltip(
                        "mail-message-new-symbolic",
                        "Send SMS",
                        Message::ShowSmsDialog(device_id.to_string()),
                    ));
                }
                actions = actions.push(
                    cosmic::widget::button::icon(if is_streaming {
                        cosmic::widget::icon::from_name("audio-volume-high-symbolic").size(16)
                    } else {
                        cosmic::widget::icon::from_name("audio-volume-muted-symbolic").size(16)
                    })
                    .on_press(Message::ToggleAudioStream(device_id.to_string()))
                    .padding(SPACE_XXS)
                    .tooltip(if is_streaming {
                        "Stop audio streaming"
                    } else {
                        "Start audio streaming"
                    }),
                );
            }

            // Presenter mode toggle button
            if device.has_incoming_capability("cconnect.presenter") {
                let is_presenting = self.presenter_mode_devices.contains(device_id);
                actions = actions.push(
                    cosmic::widget::button::icon(if is_presenting {
                        cosmic::widget::icon::from_name("x11-cursor-symbolic").size(16)
                    } else {
                        cosmic::widget::icon::from_name("input-touchpad-symbolic").size(16)
                    })
                    .on_press(Message::TogglePresenterMode(device_id.to_string()))
                    .padding(SPACE_XXS)
                    .tooltip(if is_presenting {
                        "Stop presenter mode"
                    } else {
                        "Start presenter mode"
                    }),
                );
            }
            // Battery refresh button
            let is_refreshing_battery = self
                .pending_operations
                .contains(&(device_id.to_string(), OperationType::Battery));
            actions = actions.push(action_button_with_tooltip_loading(
                "view-refresh-symbolic",
                "Refresh battery status",
                Message::RequestBatteryUpdate(device_id.to_string()),
                is_refreshing_battery,
            ));

            // Screen Mirroring button
            if device.has_outgoing_capability("cconnect.screenshare") {
                actions = actions.push(action_button_with_tooltip(
                    "video-display-symbolic",
                    "Mirror Screen",
                    Message::LaunchScreenMirror(device_id.to_string()),
                ));
            }
        }

        // Device details and manager button (for paired devices)
        if device.is_paired() {
            actions = actions.push(action_button_with_tooltip(
                "document-properties-symbolic",
                "Device Details",
                Message::ShowDeviceDetails(device_id.to_string()),
            ));

            actions = actions.push(action_button_with_tooltip(
                "preferences-system-symbolic",
                "Open Manager",
                Message::LaunchManager(device_id.to_string()),
            ));
        }

        // Pair/Unpair button
        let (label, message, is_loading) = if device.is_paired() {
            (
                "Unpair",
                Message::UnpairDevice(device_id.to_string()),
                self.pending_operations
                    .contains(&(device_id.to_string(), OperationType::Unpair)),
            )
        } else {
            (
                "Pair",
                Message::PairDevice(device_id.to_string()),
                self.pending_operations
                    .contains(&(device_id.to_string(), OperationType::Pair)),
            )
        };

        if is_loading {
            actions = actions.push(cosmic::widget::tooltip(
                button::icon(icon::from_name("process-working-symbolic").size(ICON_S))
                    .padding(SPACE_XS),
                if label == "Pair" {
                    "Pairing..."
                } else {
                    "Unpairing..."
                },
                cosmic::widget::tooltip::Position::Bottom,
            ));
        } else {
            actions = actions.push(button::text(label).on_press(message).padding(SPACE_XS));
        }

        // Context menu button (more options)
        let is_menu_open = self.context_menu_device.as_ref() == Some(&device_id.to_string());
        actions = actions.push(cosmic::widget::tooltip(
            button::icon(
                icon::from_name(if is_menu_open {
                    "go-up-symbolic"
                } else {
                    "view-more-symbolic"
                })
                .size(ICON_S),
            )
            .on_press(if is_menu_open {
                Message::CloseContextMenu
            } else {
                Message::ShowContextMenu(device_id.to_string())
            })
            .padding(SPACE_XS)
            .class(if is_menu_open {
                cosmic::theme::Button::Suggested
            } else {
                cosmic::theme::Button::Standard
            }),
            "More options",
            cosmic::widget::tooltip::Position::Bottom,
        ));

        actions
    }

    /// Build context menu items for a file transfer
    fn build_transfer_context_menu(
        &self,
        transfer_id: &str,
        filename: &str,
        is_receiving: bool,
    ) -> Vec<Element<'_, Message>> {
        let menu_item = |icon_name: &'static str,
                         label: &'static str,
                         message: Message|
         -> Element<'_, Message> {
            button::custom(
                row![
                    icon::from_name(icon_name).size(ICON_S),
                    text(label).size(ICON_14),
                ]
                .spacing(SPACE_S)
                .align_y(cosmic::iced::Alignment::Center),
            )
            .width(Length::Fill)
            .padding([SPACE_XXS, SPACE_S])
            .class(cosmic::theme::Button::MenuItem)
            .on_press(message)
            .into()
        };

        let mut items = vec![menu_item(
            "process-stop-symbolic",
            "Cancel transfer",
            Message::CancelTransfer(transfer_id.to_string()),
        )];

        if is_receiving {
            items.push(divider::horizontal::default().into());
            items.push(menu_item(
                "document-open-symbolic",
                "Open file",
                Message::OpenTransferFile(filename.to_string()),
            ));
            items.push(menu_item(
                "folder-open-symbolic",
                "Reveal in folder",
                Message::RevealTransferFile(filename.to_string()),
            ));
        }

        items
    }

    /// Builds the context menu for a device
    fn device_context_menu_view<'a>(
        &'a self,
        device_id: &str,
        device: &'a Device,
    ) -> Element<'a, Message> {
        // Helper to create consistent menu items
        let menu_item = |icon_name: &'a str,
                         label: &'a str,
                         message: Message,
                         style: cosmic::theme::Button|
         -> Element<'a, Message> {
            button::custom(
                row![icon::from_name(icon_name).size(ICON_S), text(label),]
                    .spacing(SPACE_S)
                    .align_y(cosmic::iced::Alignment::Center),
            )
            .on_press(message)
            .padding(SPACE_S)
            .width(Length::Fill)
            .class(style)
            .into()
        };

        let mut menu_items: Vec<Element<'a, Message>> = Vec::new();

        // Header
        menu_items.push(
            container(cosmic::widget::text::caption("Quick Actions"))
                .padding(Padding::from([SPACE_XS, SPACE_S]))
                .into(),
        );
        menu_items.push(divider::horizontal::default().into());

        // Connected device actions
        if device.is_connected() && device.is_paired() {
            menu_items.push(menu_item(
                "document-edit-symbolic",
                "Rename device",
                Message::StartRenaming(device_id.to_string()),
                cosmic::theme::Button::MenuItem,
            ));

            if device.has_incoming_capability("cconnect.share") {
                menu_items.push(menu_item(
                    "document-send-symbolic",
                    "Send file...",
                    Message::SendFile(device_id.to_string()),
                    cosmic::theme::Button::MenuItem,
                ));
                menu_items.push(menu_item(
                    "folder-documents-symbolic",
                    "Send multiple files...",
                    Message::SendFiles(device_id.to_string()),
                    cosmic::theme::Button::MenuItem,
                ));
            }

            if device.has_incoming_capability("cconnect.findmyphone.request") {
                menu_items.push(menu_item(
                    "find-location-symbolic",
                    "Ring device",
                    Message::FindPhone(device_id.to_string()),
                    cosmic::theme::Button::MenuItem,
                ));
            }

            if device.has_outgoing_capability("cconnect.screenshare") {
                menu_items.push(menu_item(
                    "video-display-symbolic",
                    "Mirror screen",
                    Message::LaunchScreenMirror(device_id.to_string()),
                    cosmic::theme::Button::MenuItem,
                ));
            }

            menu_items.push(divider::horizontal::default().into());
        }

        // Settings section
        if device.is_paired() {
            menu_items.push(menu_item(
                "document-properties-symbolic",
                "Device details",
                Message::ShowDeviceDetails(device_id.to_string()),
                cosmic::theme::Button::MenuItem,
            ));

            menu_items.push(menu_item(
                "preferences-system-symbolic",
                "Open Manager",
                Message::LaunchManager(device_id.to_string()),
                cosmic::theme::Button::MenuItem,
            ));

            menu_items.push(divider::horizontal::default().into());

            menu_items.push(menu_item(
                "edit-delete-symbolic",
                "Unpair device",
                Message::UnpairDevice(device_id.to_string()),
                cosmic::theme::Button::Destructive,
            ));
        } else {
            menu_items.push(menu_item(
                "emblem-default-symbolic",
                "Pair device",
                Message::PairDevice(device_id.to_string()),
                cosmic::theme::Button::Suggested,
            ));
        }

        // Close menu button
        menu_items.push(divider::horizontal::default().into());
        menu_items.push(menu_item(
            "window-close-symbolic",
            "Close menu",
            Message::CloseContextMenu,
            cosmic::theme::Button::MenuItem,
        ));

        container(column(menu_items).spacing(SPACE_XXXS).width(Length::Fill))
            .padding(SPACE_XS)
            .width(Length::Fill)
            .class(cosmic::theme::Container::Secondary)
            .into()
    }

    /// FileSync settings view
    fn file_sync_settings_view(&self, device_id: &str) -> Element<'_, Message> {
        use cosmic::widget::{horizontal_space, text_input};

        // Header with close button
        let header = row![
            cosmic::widget::text::body("File Sync Settings"),
            horizontal_space(),
            cosmic::widget::tooltip(
                button::icon(icon::from_name("window-close-symbolic").size(ICON_14))
                    .on_press(Message::CloseFileSyncSettings)
                    .padding(SPACE_XXS),
                "Close settings",
                cosmic::widget::tooltip::Position::Bottom,
            )
        ]
        .width(Length::Fill)
        .align_y(cosmic::iced::Alignment::Center);

        let mut content = column![header].spacing(SPACE_M);

        // List existing sync folders
        if let Some(folders) = self.sync_folders.get(device_id) {
            if !folders.is_empty() {
                let mut list = column![].spacing(SPACE_S);
                for folder in folders {
                    let strategy_text = match folder.strategy.as_str() {
                        "LastModifiedWins" => "Last Modified",
                        "KeepBoth" => "Keep Both",
                        "Manual" => "Manual",
                        s => s,
                    };

                    let row = row![
                        column![
                            cosmic::widget::text::body(&folder.folder_id),
                            cosmic::widget::text::caption(&folder.path),
                            cosmic::widget::text::caption(format!("Conflict: {}", strategy_text)),
                        ]
                        .spacing(SPACE_XXS),
                        horizontal_space(),
                        cosmic::widget::tooltip(
                            button::icon(icon::from_name("user-trash-symbolic").size(ICON_S))
                                .on_press(Message::RemoveSyncFolder(
                                    device_id.to_string(),
                                    folder.folder_id.clone(),
                                ))
                                .padding(SPACE_XS),
                            "Remove sync folder",
                            cosmic::widget::tooltip::Position::Bottom,
                        )
                    ]
                    .align_y(cosmic::iced::Alignment::Center)
                    .width(Length::Fill);

                    list = list.push(
                        container(row)
                            .padding(SPACE_S)
                            .class(cosmic::theme::Container::Card),
                    );
                }
                content = content.push(list);
            } else {
                content = content.push(
                    container(cosmic::widget::text::caption("No sync folders configured"))
                        .padding(SPACE_S)
                        .width(Length::Fill)
                        .align_x(Horizontal::Center),
                );
            }
        } else {
            content = content.push(
                container(
                    row![
                        icon::from_name("process-working-symbolic").size(ICON_S),
                        cosmic::widget::text::caption("Loading..."),
                    ]
                    .spacing(SPACE_S)
                    .align_y(cosmic::iced::Alignment::Center),
                )
                .padding(SPACE_S)
                .width(Length::Fill)
                .align_x(Horizontal::Center),
            );
        }

        content = content.push(divider::horizontal::default());

        // Add New Folder Form
        if self.add_sync_folder_device.as_deref() == Some(device_id) {
            let strategy_idx = match self.add_sync_folder_strategy.as_str() {
                "last_modified_wins" => 0,
                "keep_both" => 1,
                "manual" => 2,
                _ => 0,
            };

            let form = column![
                cosmic::widget::text::title3("Add Sync Folder"),
                text_input("Local Path", &self.add_sync_folder_path)
                    .on_input(Message::UpdateSyncFolderPathInput),
                text_input("Folder ID", &self.add_sync_folder_id)
                    .on_input(Message::UpdateSyncFolderIdInput),
                row![
                    cosmic::widget::text::body("Conflict Strategy:"),
                    cosmic::widget::dropdown(
                        &["Last Modified", "Keep Both", "Manual"],
                        Some(strategy_idx),
                        |idx| {
                            let s = match idx {
                                0 => "last_modified_wins",
                                1 => "keep_both",
                                2 => "manual",
                                _ => "last_modified_wins",
                            }
                            .to_string();
                            Message::UpdateSyncFolderStrategy(s)
                        }
                    )
                ]
                .spacing(SPACE_S)
                .align_y(cosmic::iced::Alignment::Center),
                row![
                    button::text("Cancel").on_press(Message::CancelAddSyncFolder),
                    horizontal_space(),
                    if self
                        .pending_operations
                        .contains(&(device_id.to_string(), OperationType::AddSyncFolder))
                    {
                        button::text("Adding...")
                    } else {
                        button::text("Add Folder")
                            .on_press(Message::AddSyncFolder(device_id.to_string()))
                    }
                ]
                .spacing(SPACE_M)
                .spacing(SPACE_M)
            ]
            .spacing(SPACE_S); // Distinct background for form

            content = content.push(
                container(form)
                    .padding(SPACE_S)
                    .class(cosmic::theme::Container::Card),
            );
        } else {
            content = content.push(
                button::text("Add Synced Folder")
                    .on_press(Message::StartAddSyncFolder(device_id.to_string()))
                    .width(Length::Fill),
            );
        }

        container(content)
            .class(cosmic::theme::Container::Card)
            .padding(SPACE_M)
            .into()
    }

    fn run_command_settings_view(&self, device_id: &str) -> Element<'_, Message> {
        use cosmic::iced::Alignment;
        use cosmic::widget::{button, container, icon, text, text_input};

        let mut content = column![row![
            text::title3("Run Commands").width(Length::Fill),
            cosmic::widget::tooltip(
                button::icon(icon::from_name("window-close-symbolic").size(ICON_S))
                    .on_press(Message::CloseRunCommandSettings)
                    .padding(SPACE_XS),
                "Close settings",
                cosmic::widget::tooltip::Position::Bottom,
            )
        ]
        .align_y(Alignment::Center)]
        .spacing(SPACE_M);

        // List existing commands
        if let Some(commands) = self.run_commands.get(device_id) {
            if !commands.is_empty() {
                let mut list = column![].spacing(SPACE_S);
                // Sort by name
                let mut sorted_cmds: Vec<_> = commands.into_iter().collect();
                sorted_cmds.sort_by(|a, b| a.1.name.cmp(&b.1.name));

                for (cmd_id, cmd) in sorted_cmds {
                    let row = row![
                        column![
                            text::body(&cmd.name),
                            text::caption(&cmd.command)
                                .class(cosmic::theme::Text::Color(theme_muted_color(),)),
                        ]
                        .width(Length::Fill),
                        cosmic::widget::tooltip(
                            button::icon(icon::from_name("user-trash-symbolic").size(ICON_S))
                                .on_press(Message::RemoveRunCommand(
                                    device_id.to_string(),
                                    cmd_id.clone(),
                                ))
                                .padding(SPACE_XS)
                                .class(cosmic::theme::Button::Destructive),
                            "Remove command",
                            cosmic::widget::tooltip::Position::Bottom,
                        )
                    ]
                    .align_y(Alignment::Center)
                    .width(Length::Fill);

                    list = list.push(
                        container(row)
                            .padding(SPACE_S)
                            .class(cosmic::theme::Container::Card),
                    );
                }
                content = content.push(list);
            } else {
                content = content.push(
                    container(text::caption("No run commands configured"))
                        .padding(SPACE_S)
                        .width(Length::Fill)
                        .align_x(Horizontal::Center),
                );
            }
        } else {
            content = content.push(
                container(text::caption("Loading..."))
                    .padding(SPACE_S)
                    .width(Length::Fill)
                    .align_x(Horizontal::Center),
            );
        }

        content = content.push(divider::horizontal::default());

        // Add New Command Form
        if self.add_run_command_device.as_deref() == Some(device_id) {
            let form = column![
                text::title3("Add New Command"),
                text_input("Name (e.g. Lock Screen)", &self.add_run_command_name)
                    .on_input(Message::UpdateRunCommandNameInput),
                text_input(
                    "Command (e.g. loginctl lock-session)",
                    &self.add_run_command_cmd
                )
                .on_input(Message::UpdateRunCommandCmdInput)
                .on_submit({
                    let id = device_id.to_string();
                    move |_| Message::AddRunCommand(id.clone())
                }),
                row![
                    button::text("Cancel")
                        .on_press(Message::CancelAddRunCommand)
                        .width(Length::Fill),
                    if self
                        .pending_operations
                        .contains(&(device_id.to_string(), OperationType::AddRunCommand))
                    {
                        button::text("Adding...")
                            .class(cosmic::theme::Button::Suggested)
                            .width(Length::Fill)
                    } else {
                        button::text("Add Command")
                            .on_press(Message::AddRunCommand(device_id.to_string()))
                            .class(cosmic::theme::Button::Suggested)
                            .width(Length::Fill)
                    }
                ]
                .spacing(SPACE_S)
            ]
            .spacing(SPACE_S);

            content = content.push(
                container(form)
                    .padding(SPACE_S)
                    .class(cosmic::theme::Container::Card),
            );
        } else {
            content = content.push(
                button::text("Add Command")
                    .on_press(Message::StartAddRunCommand(device_id.to_string()))
                    .width(Length::Fill),
            );
        }

        container(content)
            .class(cosmic::theme::Container::Card)
            .padding(SPACE_M)
            .into()
    }

    /// Open URL dialog view for App Continuity
    fn open_url_dialog_view(&self, device_id: &str) -> Element<'_, Message> {
        use cosmic::iced::Alignment;
        use cosmic::widget::{button, container, icon, text, text_input};

        // Get device info
        let device = self.devices.iter().find(|d| d.device.id() == device_id);
        let device_name = device.map(|d| d.device.name()).unwrap_or("Unknown Device");

        let content = column![
            row![
                text::title3("Open on Phone").width(Length::Fill),
                cosmic::widget::tooltip(
                    button::icon(icon::from_name("window-close-symbolic").size(ICON_S))
                        .on_press(Message::CancelOpenUrlDialog)
                        .padding(SPACE_XS),
                    "Close",
                    cosmic::widget::tooltip::Position::Bottom,
                )
            ]
            .align_y(Alignment::Center),
            divider::horizontal::default(),
            text::caption(format!("Send a URL to open on {}", device_name)),
            text_input(
                "Enter URL (http://, https://, tel:, mailto:, etc.)",
                &self.open_url_input
            )
            .on_input(Message::OpenUrlInput)
            .on_submit({
                let url = self.open_url_input.clone();
                let id = device_id.to_string();
                move |_| Message::OpenOnPhone(id.clone(), url.clone())
            }),
            row![
                button::text("Cancel")
                    .on_press(Message::CancelOpenUrlDialog)
                    .width(Length::Fill),
                if !self.open_url_input.is_empty() {
                    button::text("Open on Phone")
                        .on_press({
                            let url = self.open_url_input.clone();
                            Message::OpenOnPhone(device_id.to_string(), url)
                        })
                        .class(cosmic::theme::Button::Suggested)
                        .width(Length::Fill)
                } else {
                    button::text("Open on Phone")
                        .class(cosmic::theme::Button::Suggested)
                        .width(Length::Fill)
                },
            ]
            .spacing(SPACE_S),
        ]
        .spacing(SPACE_M);

        container(content)
            .class(cosmic::theme::Container::Card)
            .padding(SPACE_M)
            .into()
    }

    /// RemoteDesktop settings view with quality, FPS, and resolution controls

    /// SMS dialog view for sending SMS messages via device
    fn sms_dialog_view(&self, device_id: &str) -> Element<'_, Message> {
        use cosmic::iced::Alignment;
        use cosmic::widget::{button, container, icon, text, text_input};

        // Get device info
        let device = self.devices.iter().find(|d| d.device.id() == device_id);
        let device_name = device.map(|d| d.device.name()).unwrap_or("Unknown Device");

        let content = column![
            row![
                text::title3("Send SMS").width(Length::Fill),
                cosmic::widget::tooltip(
                    button::icon(icon::from_name("window-close-symbolic").size(ICON_S))
                        .on_press(Message::CancelSmsDialog)
                        .padding(SPACE_XS),
                    "Close",
                    cosmic::widget::tooltip::Position::Bottom,
                )
            ]
            .align_y(Alignment::Center),
            divider::horizontal::default(),
            text::caption(format!("Send SMS via {}", device_name)),
            text_input("Phone number", &self.sms_phone_number_input)
                .on_input(Message::UpdateSmsPhoneNumberInput),
            text_input("Message", &self.sms_message_input)
                .on_input(Message::UpdateSmsMessageInput)
                .on_submit({
                    let phone = self.sms_phone_number_input.clone();
                    let msg = self.sms_message_input.clone();
                    let id = device_id.to_string();
                    move |_| Message::SendSms(id.clone(), phone.clone(), msg.clone())
                }),
            row![
                button::text("Cancel")
                    .on_press(Message::CancelSmsDialog)
                    .width(Length::Fill),
                if !self.sms_phone_number_input.is_empty() && !self.sms_message_input.is_empty() {
                    button::text("Send SMS")
                        .on_press({
                            let phone = self.sms_phone_number_input.clone();
                            let msg = self.sms_message_input.clone();
                            Message::SendSms(device_id.to_string(), phone, msg)
                        })
                        .class(cosmic::theme::Button::Suggested)
                        .width(Length::Fill)
                } else {
                    button::text("Send SMS")
                        .class(cosmic::theme::Button::Suggested)
                        .width(Length::Fill)
                },
            ]
            .spacing(SPACE_S),
        ]
        .spacing(SPACE_M);

        container(content)
            .class(cosmic::theme::Container::Card)
            .padding(SPACE_M)
            .into()
    }
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
            cosmic::widget::tooltip(
                button::icon(icon::from_name("window-close-symbolic").size(ICON_14))
                    .on_press(Message::CloseRemoteDesktopSettings)
                    .padding(SPACE_XXS),
                "Close settings",
                cosmic::widget::tooltip::Position::Bottom,
            )
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
                    .size(ICON_XS)
                    .class(theme::Text::Color(theme_destructive_color())),
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

    /// Camera settings view with camera selection, resolution, and streaming controls
    fn camera_settings_view(&self, device_id: &str) -> Element<'_, Message> {
        use cosmic::widget::horizontal_space;

        // Header with close button
        let header = row![
            cosmic::widget::text::body("Camera Settings"),
            horizontal_space(),
            cosmic::widget::tooltip(
                button::icon(icon::from_name("window-close-symbolic").size(ICON_14))
                    .on_press(Message::CloseCameraSettings)
                    .padding(SPACE_XXS),
                "Close settings",
                cosmic::widget::tooltip::Position::Bottom,
            )
        ]
        .width(Length::Fill)
        .align_y(cosmic::iced::Alignment::Center);

        // Get camera stats if available
        let stats = self.camera_stats.get(device_id);
        let is_streaming = stats.map_or(false, |s| s.is_streaming);

        // Camera selection dropdown
        let camera_idx = stats.map_or(0, |s| if s.camera_id == 0 { 0 } else { 1 });
        let camera_row = row![
            text("Camera:").width(Length::Fixed(120.0)),
            cosmic::widget::dropdown(&["Back Camera", "Front Camera"], Some(camera_idx), {
                let device_id = device_id.to_string();
                move |idx| {
                    let camera_id = if idx == 0 { 0 } else { 1 };
                    Message::SelectCamera(device_id.clone(), camera_id)
                }
            })
        ]
        .spacing(SPACE_S)
        .align_y(cosmic::iced::Alignment::Center);

        // Resolution dropdown
        let resolution_idx = stats.map_or(1, |s| match s.resolution.as_str() {
            "480p" => 0,
            "720p" => 1,
            "1080p" => 2,
            _ => 1,
        });

        let resolution_row = row![
            text("Resolution:").width(Length::Fixed(120.0)),
            cosmic::widget::dropdown(&["480p", "720p", "1080p"], Some(resolution_idx), {
                let device_id = device_id.to_string();
                move |idx| {
                    let resolution = match idx {
                        0 => "480p",
                        1 => "720p",
                        2 => "1080p",
                        _ => "720p",
                    }
                    .to_string();
                    Message::SelectCameraResolution(device_id.clone(), resolution)
                }
            })
        ]
        .spacing(SPACE_S)
        .align_y(cosmic::iced::Alignment::Center);

        // Build content
        let mut content = column![
            header,
            divider::horizontal::default(),
            camera_row,
            resolution_row
        ]
        .spacing(SPACE_M);

        // Statistics section (only show when streaming)
        if is_streaming {
            if let Some(stats) = stats {
                content = content.push(divider::horizontal::default());

                let stats_section = column![
                    cosmic::widget::text::caption("Stream Statistics:"),
                    row![
                        column![
                            cosmic::widget::text::caption("FPS:"),
                            text(format!("{}", stats.fps)),
                        ]
                        .spacing(SPACE_XXS)
                        .width(Length::FillPortion(1)),
                        column![
                            cosmic::widget::text::caption("Bitrate:"),
                            text(format!("{} kbps", stats.bitrate)),
                        ]
                        .spacing(SPACE_XXS)
                        .width(Length::FillPortion(1)),
                        column![
                            cosmic::widget::text::caption("Current:"),
                            text(&stats.resolution),
                        ]
                        .spacing(SPACE_XXS)
                        .width(Length::FillPortion(1)),
                    ]
                    .spacing(SPACE_M),
                ]
                .spacing(SPACE_S);

                content = content.push(stats_section);
            }
        }

        content = content.push(divider::horizontal::default());

        // Start/Stop streaming button
        let streaming_button = if is_streaming {
            button::text("Stop Streaming")
                .on_press(Message::ToggleCameraStreaming(device_id.to_string()))
                .padding(SPACE_S)
                .class(cosmic::theme::Button::Destructive)
        } else {
            button::text("Start Streaming")
                .on_press(Message::ToggleCameraStreaming(device_id.to_string()))
                .padding(SPACE_S)
                .class(cosmic::theme::Button::Suggested)
        };

        content = content.push(streaming_button);

        // Helper text
        let helper_text = if is_streaming {
            cosmic::widget::text::caption("Camera is available at /dev/video10")
        } else {
            cosmic::widget::text::caption("Start streaming to use phone camera as webcam")
        };
        content = content.push(helper_text);

        container(content).padding(SPACE_M).into()
    }

    fn transfers_view(&self) -> Element<'_, Message> {
        if self.active_transfers.is_empty() {
            return Element::from(cosmic::widget::Space::new(0, 0));
        }

        let header = row![
            text("Active Transfers")
                .size(ICON_14)
                .class(theme::Text::Color(theme_accent_color()))
                .width(Length::Fill),
            cosmic::widget::tooltip(
                button::icon(icon::from_name("go-next-symbolic").size(ICON_S))
                    .on_press(Message::ShowTransferQueue)
                    .padding(SPACE_XS),
                "View Transfer Queue",
                cosmic::widget::tooltip::Position::Bottom,
            )
        ]
        .align_y(cosmic::iced::Alignment::Center);

        let mut transfers_col = column![header].spacing(SPACE_S);

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

/// Creates a small icon button with tooltip for device quick actions
fn action_button_with_tooltip(
    icon_name: &str,
    tooltip_text: &'static str,
    message: Message,
) -> Element<'static, Message> {
    cosmic::widget::tooltip(
        button::icon(icon::from_name(icon_name).size(ICON_S))
            .on_press(message)
            .padding(SPACE_XS),
        tooltip_text,
        cosmic::widget::tooltip::Position::Bottom,
    )
    .into()
}

/// Creates a small icon button with tooltip that supports a loading state
fn action_button_with_tooltip_loading(
    icon_name: &str,
    tooltip_text: &'static str,
    message: Message,
    is_loading: bool,
) -> Element<'static, Message> {
    if is_loading {
        cosmic::widget::tooltip(
            button::icon(icon::from_name("process-working-symbolic").size(ICON_S))
                .padding(SPACE_XS),
            "Working...",
            cosmic::widget::tooltip::Position::Bottom,
        )
        .into()
    } else {
        action_button_with_tooltip(icon_name, tooltip_text, message)
    }
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
        ConnectionState::Connected => theme_success_color(),
        ConnectionState::Failed => theme_destructive_color(),
        ConnectionState::Connecting => theme_warning_color(),
        ConnectionState::Disconnected => theme_muted_color(),
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
