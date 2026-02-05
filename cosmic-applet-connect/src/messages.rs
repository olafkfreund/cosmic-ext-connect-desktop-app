use std::collections::HashMap;
use std::path::PathBuf;

use cosmic::iced::{keyboard, window};

use crate::{
    dbus_client,
    state::{CameraStats, SystemInfo, ViewMode, FocusTarget},
};

/// Application notification type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationType {
    Error,
    Success,
    #[allow(dead_code)]
    Info,
}

/// Application operation types for tracking loading state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationType {
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

/// Main application message type
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Message {
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
    LaunchScreenMirror(String), // device_id - View remote's screen
    ShareScreenTo(String),      // device_id - Share our screen to remote

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
    KeyPress(keyboard::Key, keyboard::Modifiers),
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
    FileDropped(PathBuf),
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
