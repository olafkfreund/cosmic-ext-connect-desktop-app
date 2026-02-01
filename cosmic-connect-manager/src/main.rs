mod dbus_client;

use clap::Parser;
use cosmic::{
    app::{Core, Task},
    iced::{Alignment, Length, Size},
    theme,
    widget::{button, column, container, icon, row, scrollable, text, toggler, vertical_space},
    Application, Element,
};

/// Navigation pages in the manager
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Devices,
    MediaPlayers,
    Transfers,
    History,
    Settings,
}

impl Page {
    fn title(&self) -> &'static str {
        match self {
            Page::Devices => "Devices",
            Page::MediaPlayers => "Media",
            Page::Transfers => "Transfers",
            Page::History => "History",
            Page::Settings => "Settings",
        }
    }

    fn icon_name(&self) -> &'static str {
        match self {
            Page::Devices => "computer-symbolic",
            Page::MediaPlayers => "multimedia-player-symbolic",
            Page::Transfers => "folder-download-symbolic",
            Page::History => "document-open-recent-symbolic",
            Page::Settings => "preferences-system-symbolic",
        }
    }
}

use dbus_client::{DbusClient, DaemonEvent, DeviceConfig, DeviceInfo};
use std::collections::HashMap;

const APP_ID: &str = "com.system76.CosmicConnectManager";

#[derive(Parser, Debug, Clone)]
#[command(name = "cosmic-connect-manager")]
#[command(about = "COSMIC Connect Device Manager")]
pub struct Args {
    #[arg(long)]
    pub device: Option<String>,
    #[arg(long)]
    pub action: Option<String>,
}

/// Device type category for filtering actions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceCategory {
    /// Phone or tablet (Android)
    Mobile,
    /// Desktop or laptop computer
    Desktop,
    /// Unknown device type
    Unknown,
}

impl DeviceCategory {
    fn from_device_type(device_type: &str) -> Self {
        match device_type {
            "phone" | "tablet" => DeviceCategory::Mobile,
            "desktop" | "laptop" => DeviceCategory::Desktop,
            _ => DeviceCategory::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceAction {
    // Universal actions (all device types)
    Ping,
    SendFile,
    Clipboard,
    RemoteInput,
    Screenshot,
    SystemInfo,
    Settings,

    // Mobile-only actions (phone/tablet)
    Find,
    Sms,
    MuteCall,
    Camera,
    RefreshBattery,
    Contacts,

    // Desktop-only actions (desktop/laptop)
    ScreenShare,
    Lock,
    Power,
    Wake,
    RunCommand,
    Presenter,

    // Media control (primarily desktop but could work on both)
    MediaControl,
}

impl DeviceAction {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "ping" => Some(DeviceAction::Ping),
            "send-file" => Some(DeviceAction::SendFile),
            "clipboard" => Some(DeviceAction::Clipboard),
            "remote-input" => Some(DeviceAction::RemoteInput),
            "screenshot" => Some(DeviceAction::Screenshot),
            "system-info" => Some(DeviceAction::SystemInfo),
            "settings" => Some(DeviceAction::Settings),
            "find" => Some(DeviceAction::Find),
            "sms" => Some(DeviceAction::Sms),
            "mute-call" => Some(DeviceAction::MuteCall),
            "camera" => Some(DeviceAction::Camera),
            "refresh-battery" => Some(DeviceAction::RefreshBattery),
            "contacts" => Some(DeviceAction::Contacts),
            "screen-share" => Some(DeviceAction::ScreenShare),
            "lock" => Some(DeviceAction::Lock),
            "power" => Some(DeviceAction::Power),
            "wake" => Some(DeviceAction::Wake),
            "run-command" => Some(DeviceAction::RunCommand),
            "presenter" => Some(DeviceAction::Presenter),
            "media-control" => Some(DeviceAction::MediaControl),
            _ => None,
        }
    }

    /// Check if this action is available for the given device category
    fn is_available_for(&self, category: DeviceCategory) -> bool {
        match self {
            // Universal actions
            DeviceAction::Ping
            | DeviceAction::SendFile
            | DeviceAction::Clipboard
            | DeviceAction::RemoteInput
            | DeviceAction::Screenshot
            | DeviceAction::SystemInfo
            | DeviceAction::Settings => true,

            // Mobile-only actions
            DeviceAction::Find
            | DeviceAction::Sms
            | DeviceAction::MuteCall
            | DeviceAction::Camera
            | DeviceAction::RefreshBattery
            | DeviceAction::Contacts => matches!(category, DeviceCategory::Mobile | DeviceCategory::Unknown),

            // Desktop-only actions
            DeviceAction::ScreenShare
            | DeviceAction::Lock
            | DeviceAction::Power
            | DeviceAction::Wake
            | DeviceAction::RunCommand
            | DeviceAction::Presenter => matches!(category, DeviceCategory::Desktop | DeviceCategory::Unknown),

            // Media control works on both but primarily desktop
            DeviceAction::MediaControl => true,
        }
    }

    /// Get the icon name for this action
    fn icon_name(&self) -> &'static str {
        match self {
            DeviceAction::Ping => "network-transmit-receive-symbolic",
            DeviceAction::SendFile => "document-send-symbolic",
            DeviceAction::Clipboard => "edit-paste-symbolic",
            DeviceAction::RemoteInput => "input-keyboard-symbolic",
            DeviceAction::Screenshot => "applets-screenshooter-symbolic",
            DeviceAction::SystemInfo => "computer-symbolic",
            DeviceAction::Settings => "preferences-system-symbolic",
            DeviceAction::Find => "find-location-symbolic",
            DeviceAction::Sms => "mail-message-new-symbolic",
            DeviceAction::MuteCall => "audio-volume-muted-symbolic",
            DeviceAction::Camera => "camera-web-symbolic",
            DeviceAction::RefreshBattery => "battery-symbolic",
            DeviceAction::Contacts => "contact-new-symbolic",
            DeviceAction::ScreenShare => "video-display-symbolic",
            DeviceAction::Lock => "system-lock-screen-symbolic",
            DeviceAction::Power => "system-shutdown-symbolic",
            DeviceAction::Wake => "system-reboot-symbolic",
            DeviceAction::RunCommand => "utilities-terminal-symbolic",
            DeviceAction::Presenter => "x-office-presentation-symbolic",
            DeviceAction::MediaControl => "multimedia-player-symbolic",
        }
    }

    /// Get the tooltip text for this action
    fn tooltip(&self) -> &'static str {
        match self {
            DeviceAction::Ping => "Send ping",
            DeviceAction::SendFile => "Send file",
            DeviceAction::Clipboard => "Sync clipboard",
            DeviceAction::RemoteInput => "Remote keyboard/mouse",
            DeviceAction::Screenshot => "Take screenshot",
            DeviceAction::SystemInfo => "System information",
            DeviceAction::Settings => "Device settings",
            DeviceAction::Find => "Find device",
            DeviceAction::Sms => "Send SMS",
            DeviceAction::MuteCall => "Mute call",
            DeviceAction::Camera => "Use as webcam",
            DeviceAction::RefreshBattery => "Refresh battery",
            DeviceAction::Contacts => "Browse contacts",
            DeviceAction::ScreenShare => "Screen share",
            DeviceAction::Lock => "Lock device",
            DeviceAction::Power => "Power options",
            DeviceAction::Wake => "Wake device",
            DeviceAction::RunCommand => "Run command",
            DeviceAction::Presenter => "Presenter mode",
            DeviceAction::MediaControl => "Media control",
        }
    }
}

/// Get available actions for a device based on its type and capabilities
fn get_available_actions(device: &DeviceInfo) -> Vec<DeviceAction> {
    let category = DeviceCategory::from_device_type(&device.device_type);

    // All possible actions in display order
    let all_actions = [
        // Primary row - common actions
        DeviceAction::Ping,
        DeviceAction::SendFile,
        DeviceAction::Clipboard,
        DeviceAction::Screenshot,
        // Mobile-specific
        DeviceAction::Find,
        DeviceAction::RefreshBattery,
        DeviceAction::Sms,
        DeviceAction::MuteCall,
        DeviceAction::Camera,
        DeviceAction::Contacts,
        // Desktop-specific
        DeviceAction::ScreenShare,
        DeviceAction::Lock,
        DeviceAction::Power,
        DeviceAction::Wake,
        DeviceAction::RunCommand,
        DeviceAction::Presenter,
        // Advanced
        DeviceAction::RemoteInput,
        DeviceAction::MediaControl,
        DeviceAction::SystemInfo,
    ];

    all_actions
        .into_iter()
        .filter(|action| action.is_available_for(category))
        .collect()
}

fn device_icon_name(device_type: &str) -> &'static str {
    match device_type {
        "phone" => "phone-symbolic",
        "tablet" => "tablet-symbolic",
        "desktop" | "laptop" => "computer-symbolic",
        _ => "network-wireless-symbolic",
    }
}

fn connection_status(device: &DeviceInfo) -> &'static str {
    if device.is_connected {
        "Connected"
    } else if device.is_reachable {
        "Available"
    } else {
        "Offline"
    }
}

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let settings = cosmic::app::Settings::default().size(Size::new(900.0, 700.0));
    cosmic::app::run::<CosmicConnectManager>(settings, args)
}

#[derive(Debug, Clone)]
pub struct TransferInfo {
    pub transfer_id: String,
    pub device_id: String,
    pub filename: String,
    pub current: u64,
    pub total: u64,
    pub direction: String,
}

#[derive(Debug, Clone)]
pub struct CompletedTransfer {
    pub filename: String,
    pub size: u64,
    pub timestamp: chrono::DateTime<chrono::Local>,
    pub success: bool,
}

#[derive(Debug, Clone)]
pub struct HistoryEvent {
    pub icon_name: String,
    pub event_type: String,
    pub description: String,
    pub timestamp: chrono::DateTime<chrono::Local>,
}

#[derive(Debug, Clone)]
pub enum Message {
    NavigateTo(Page),
    SelectDevice(String),
    DevicesUpdated(HashMap<String, DeviceInfo>),
    DeviceConfigLoaded(String, DeviceConfig),
    ExecuteAction(String, DeviceAction),
    DbusReady(DbusClient),
    MediaPlayPause(String),
    MediaNext(String),
    MediaPrevious(String),
    CancelTransfer(String),
    ClearHistory,
    ToggleAutoStart(bool),
    ToggleNotifications(bool),
    TogglePlugin(String, bool),
    DbusConnected(DbusClient),
    DbusError(String),
    DeviceAdded(String, DeviceInfo),
    DeviceRemoved(String),
    DeviceStateChanged(String, String),
    MprisPlayersLoaded(Vec<String>),
    MprisPlayerStateLoaded(String, dbus_client::PlayerState),
    TransferProgressUpdate(TransferInfo),
    TransferCompleted(String, String, String, bool, String),
    AddHistoryEvent(HistoryEvent),
    RefreshDevices,
    RefreshMprisPlayers,
    BatteryStatusLoaded(String, dbus_client::BatteryStatus),
    DaemonEventReceived(DaemonEvent),
    None,
}

pub struct CosmicConnectManager {
    core: Core,
    active_page: Page,
    dbus_client: Option<DbusClient>,
    devices: HashMap<String, DeviceInfo>,
    device_configs: HashMap<String, DeviceConfig>,
    battery_status: HashMap<String, dbus_client::BatteryStatus>,
    selected_device: Option<String>,
    _initial_device: Option<String>,
    _initial_action: Option<DeviceAction>,
    dbus_ready: bool,
    auto_start_enabled: bool,
    show_notifications: bool,
    plugin_states: HashMap<String, bool>,
    mpris_players: Vec<(String, Option<dbus_client::PlayerState>)>,
    active_transfers: HashMap<String, TransferInfo>,
    completed_transfers: Vec<CompletedTransfer>,
    history_events: Vec<HistoryEvent>,
    _event_rx: Option<tokio::sync::mpsc::UnboundedReceiver<DaemonEvent>>,
}

impl CosmicConnectManager {
    fn sidebar_view(&self) -> Element<'_, Message> {
        let pages = [
            Page::Devices,
            Page::MediaPlayers,
            Page::Transfers,
            Page::History,
            Page::Settings,
        ];

        let mut nav_items = column::with_capacity(pages.len() + 1)
            .spacing(theme::active().cosmic().space_xxs());

        for page in pages {
            let is_active = self.active_page == page;
            let item_icon = icon::from_name(page.icon_name()).size(20);
            let item_label = text(page.title()).size(14);

            let item_content = row::with_capacity(2)
                .spacing(theme::active().cosmic().space_s())
                .align_y(Alignment::Center)
                .push(item_icon)
                .push(item_label);

            let item_container = container(item_content)
                .padding(theme::active().cosmic().space_s())
                .width(Length::Fill);

            let nav_button = if is_active {
                button::custom(item_container)
                    .class(theme::Button::Suggested)
            } else {
                button::custom(item_container)
                    .class(theme::Button::Text)
            };

            nav_items = nav_items.push(
                nav_button
                    .on_press(Message::NavigateTo(page))
                    .padding(0)
                    .width(Length::Fill)
            );
        }

        container(
            column::with_capacity(2)
                .push(
                    text("COSMIC Connect")
                        .size(18)
                )
                .push(vertical_space().height(theme::active().cosmic().space_m()))
                .push(nav_items)
        )
        .padding(theme::active().cosmic().space_m())
        .width(Length::Fixed(200.0))
        .height(Length::Fill)
        .into()
    }

    fn content_view(&self) -> Element<'_, Message> {
        match self.active_page {
            Page::Devices => self.device_list_view(),
            Page::MediaPlayers => self.media_players_view(),
            Page::Transfers => self.transfers_view(),
            Page::History => self.history_view(),
            Page::Settings => self.settings_view(),
        }
    }

    #[allow(dead_code)]
    fn placeholder_view(&self, title: &'static str, icon_name: &'static str) -> Element<'static, Message> {
        container(
            column::with_capacity(2)
                .spacing(theme::active().cosmic().space_s())
                .align_x(Alignment::Center)
                .push(icon::from_name(icon_name).size(64))
                .push(text(title).size(24))
                .push(text("Coming soon").size(14))
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn device_list_view(&self) -> Element<'_, Message> {
        let mut connected_devices = Vec::new();
        let mut available_devices = Vec::new();
        let mut offline_devices = Vec::new();

        for (device_id, device) in &self.devices {
            let config = self.device_configs.get(device_id);
            let is_selected = self.selected_device.as_ref() == Some(device_id);
            let card = self.device_card(device_id, device, config, is_selected);

            if device.is_connected {
                connected_devices.push(card);
            } else if device.is_reachable {
                available_devices.push(card);
            } else {
                offline_devices.push(card);
            }
        }

        let mut sections = column::with_capacity(6)
            .spacing(theme::active().cosmic().space_m())
            .padding(theme::active().cosmic().space_m());

        if !connected_devices.is_empty() {
            sections = sections.push(text("Connected").size(14));
            for device in connected_devices {
                sections = sections.push(device);
            }
        }

        if !available_devices.is_empty() {
            sections = sections.push(text("Available").size(14));
            for device in available_devices {
                sections = sections.push(device);
            }
        }

        if !offline_devices.is_empty() {
            sections = sections.push(text("Offline").size(14));
            for device in offline_devices {
                sections = sections.push(device);
            }
        }

        if self.devices.is_empty() {
            sections = sections.push(
                container(
                    column::with_capacity(3)
                        .spacing(theme::active().cosmic().space_s())
                        .align_x(Alignment::Center)
                        .push(icon::from_name("network-wireless-offline-symbolic").size(64))
                        .push(text("No devices found").size(18))
                        .push(text("Make sure devices are on the same network").size(14))
                )
                .center_x(Length::Fill)
                .center_y(Length::Fill)
            );
        }

        container(sections)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn media_players_view(&self) -> Element<'_, Message> {
        let mut sections = column::with_capacity(4)
            .spacing(theme::active().cosmic().space_m())
            .padding(theme::active().cosmic().space_m());

        sections = sections.push(text("Media Players").size(18));

        if self.mpris_players.is_empty() {
            sections = sections.push(
                container(
                    column::with_capacity(3)
                        .spacing(theme::active().cosmic().space_s())
                        .align_x(Alignment::Center)
                        .push(icon::from_name("multimedia-player-symbolic").size(64))
                        .push(text("No media players found").size(18))
                        .push(text("Play media on connected devices to see players").size(14))
                )
                .center_x(Length::Fill)
                .center_y(Length::Fill)
            );
        } else {
            for (player_id, state) in &self.mpris_players {
                sections = sections.push(self.media_player_card_with_state(player_id, state.as_ref()));
            }
        }

        container(sections)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn media_player_card_with_state(&self, player_id: &str, state: Option<&dbus_client::PlayerState>) -> Element<'_, Message> {
        let player_icon = icon::from_name("multimedia-player-symbolic").size(48);

        let (player_name, track_info_text, play_pause_icon) = if let Some(state) = state {
            let track = if let Some(title) = &state.metadata.title {
                if let Some(artist) = &state.metadata.artist {
                    format!("{} - {}", artist, title)
                } else {
                    title.clone()
                }
            } else {
                "No track playing".to_string()
            };

            let icon_name = match state.playback_status {
                dbus_client::PlaybackStatus::Playing => "media-playback-pause-symbolic",
                _ => "media-playback-start-symbolic",
            };

            (state.identity.clone(), track, icon_name)
        } else {
            (player_id.to_string(), "Loading...".to_string(), "media-playback-start-symbolic")
        };

        let name_text = text(player_name).size(16);
        let track_info = text(track_info_text).size(12);

        let info_column = column::with_capacity(2)
            .spacing(theme::active().cosmic().space_xxs())
            .push(name_text)
            .push(track_info);

        let header_row = row::with_capacity(2)
            .spacing(theme::active().cosmic().space_s())
            .align_y(Alignment::Center)
            .push(player_icon)
            .push(info_column);

        let prev_button = button::icon(icon::from_name("media-skip-backward-symbolic").size(16))
            .on_press(Message::MediaPrevious(player_id.to_string()))
            .padding(theme::active().cosmic().space_xxs());

        let play_pause_button = button::icon(icon::from_name(play_pause_icon).size(16))
            .on_press(Message::MediaPlayPause(player_id.to_string()))
            .padding(theme::active().cosmic().space_xxs());

        let next_button = button::icon(icon::from_name("media-skip-forward-symbolic").size(16))
            .on_press(Message::MediaNext(player_id.to_string()))
            .padding(theme::active().cosmic().space_xxs());

        let controls_row = row::with_capacity(3)
            .spacing(theme::active().cosmic().space_xs())
            .push(prev_button)
            .push(play_pause_button)
            .push(next_button);

        let card_content = column::with_capacity(2)
            .spacing(theme::active().cosmic().space_s())
            .push(header_row)
            .push(controls_row);

        container(card_content)
            .padding(theme::active().cosmic().space_s())
            .width(Length::Fill)
            .into()
    }

    #[allow(dead_code)]
    fn media_player_card(&self, player_id: &str, player_name: &str) -> Element<'static, Message> {
        let player_icon = icon::from_name("multimedia-player-symbolic").size(48);
        let name_text = text(player_name.to_string()).size(16);
        let track_info = text("No track playing").size(12);

        let info_column = column::with_capacity(2)
            .spacing(theme::active().cosmic().space_xxs())
            .push(name_text)
            .push(track_info);

        let header_row = row::with_capacity(2)
            .spacing(theme::active().cosmic().space_s())
            .align_y(Alignment::Center)
            .push(player_icon)
            .push(info_column);

        let prev_button = button::icon(icon::from_name("media-skip-backward-symbolic").size(16))
            .on_press(Message::MediaPrevious(player_id.to_string()))
            .padding(theme::active().cosmic().space_xxs());

        let play_pause_button = button::icon(icon::from_name("media-playback-start-symbolic").size(16))
            .on_press(Message::MediaPlayPause(player_id.to_string()))
            .padding(theme::active().cosmic().space_xxs());

        let next_button = button::icon(icon::from_name("media-skip-forward-symbolic").size(16))
            .on_press(Message::MediaNext(player_id.to_string()))
            .padding(theme::active().cosmic().space_xxs());

        let controls_row = row::with_capacity(3)
            .spacing(theme::active().cosmic().space_xs())
            .push(prev_button)
            .push(play_pause_button)
            .push(next_button);

        let card_content = column::with_capacity(2)
            .spacing(theme::active().cosmic().space_s())
            .push(header_row)
            .push(controls_row);

        container(card_content)
            .padding(theme::active().cosmic().space_s())
            .width(Length::Fill)
            .into()
    }

    fn transfers_view(&self) -> Element<'_, Message> {
        let mut content = column::with_capacity(4)
            .spacing(theme::active().cosmic().space_m())
            .padding(theme::active().cosmic().space_m());

        let active_count = self.active_transfers.len();
        content = content.push(text(format!("Active Transfers ({})", active_count)).size(16));

        if !self.active_transfers.is_empty() {
            for (transfer_id, info) in &self.active_transfers {
                let progress = if info.total > 0 {
                    ((info.current as f64 / info.total as f64) * 100.0) as u8
                } else {
                    0
                };

                let speed = if info.current > 0 {
                    format!("{:.1} MB/s", info.current as f64 / 1_000_000.0)
                } else {
                    "Calculating...".to_string()
                };

                let icon_name = if info.filename.ends_with(".pdf") || info.filename.ends_with(".txt") {
                    "text-x-generic-symbolic"
                } else if info.filename.ends_with(".jpg") || info.filename.ends_with(".png") {
                    "image-x-generic-symbolic"
                } else {
                    "text-x-generic-symbolic"
                };

                content = content.push(self.transfer_card(
                    transfer_id,
                    &info.filename,
                    icon_name,
                    progress,
                    &speed,
                    true,
                ));
            }
        } else {
            content = content.push(text("No active transfers").size(14));
        }

        if !self.completed_transfers.is_empty() {
            content = content.push(vertical_space().height(theme::active().cosmic().space_m()));
            content = content.push(text(format!("Completed ({})", self.completed_transfers.len())).size(16));

            let mut completed_col = column::with_capacity(self.completed_transfers.len())
                .spacing(theme::active().cosmic().space_xs());

            for transfer in &self.completed_transfers {
                let icon_name = if transfer.filename.ends_with(".pdf") || transfer.filename.ends_with(".txt") {
                    "text-x-generic-symbolic"
                } else if transfer.filename.ends_with(".jpg") || transfer.filename.ends_with(".png") {
                    "image-x-generic-symbolic"
                } else {
                    "text-x-generic-symbolic"
                };

                let size_str = if transfer.size > 0 {
                    format!("{:.1} MB", transfer.size as f64 / 1_000_000.0)
                } else {
                    "Unknown".to_string()
                };

                let time_str = transfer.timestamp.format("%H:%M").to_string();

                completed_col = completed_col.push(self.completed_transfer_item(
                    &transfer.filename,
                    icon_name,
                    &size_str,
                    &time_str,
                ));
            }

            content = content.push(completed_col);
        }

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn transfer_card(
        &self,
        transfer_id: &str,
        filename: &str,
        icon_name: &str,
        progress: u8,
        speed: &str,
        is_active: bool,
    ) -> Element<'_, Message> {
        let file_icon = icon::from_name(icon_name).size(24);
        let filename_text = text(filename.to_string()).size(14);

        let header_row = row::with_capacity(2)
            .spacing(theme::active().cosmic().space_s())
            .align_y(Alignment::Center)
            .push(file_icon)
            .push(filename_text);

        let progress_text = format!("{}%", progress);
        let progress_label = text(progress_text).size(12);
        let speed_label = text(speed.to_string()).size(12);

        let mut info_row = row::with_capacity(3)
            .spacing(theme::active().cosmic().space_s())
            .align_y(Alignment::Center)
            .push(progress_label)
            .push(text("·").size(12))
            .push(speed_label);

        if is_active {
            let cancel_button = button::text("Cancel")
                .on_press(Message::CancelTransfer(transfer_id.to_string()))
                .class(theme::Button::Destructive)
                .padding(theme::active().cosmic().space_xxs());

            info_row = info_row.push(cancel_button);
        }

        let card_content = column::with_capacity(2)
            .spacing(theme::active().cosmic().space_xs())
            .push(header_row)
            .push(info_row);

        container(card_content)
            .padding(theme::active().cosmic().space_s())
            .width(Length::Fill)
            .into()
    }

    fn completed_transfer_item(
        &self,
        filename: &str,
        icon_name: &str,
        size: &str,
        time: &str,
    ) -> Element<'_, Message> {
        let file_icon = icon::from_name(icon_name).size(20);
        let filename_text = text(filename.to_string()).size(14);
        let size_text = text(size.to_string()).size(12);
        let time_text = text(time.to_string()).size(12);

        let item_row = row::with_capacity(5)
            .spacing(theme::active().cosmic().space_s())
            .align_y(Alignment::Center)
            .push(file_icon)
            .push(filename_text)
            .push(text("-").size(12))
            .push(size_text)
            .push(text("-").size(12))
            .push(time_text);

        container(item_row)
            .padding(theme::active().cosmic().space_xs())
            .width(Length::Fill)
            .into()
    }

    fn history_view(&self) -> Element<'_, Message> {
        let mut content = column::with_capacity(2)
            .spacing(theme::active().cosmic().space_m())
            .padding(theme::active().cosmic().space_m());

        let header = row::with_capacity(2)
            .spacing(theme::active().cosmic().space_s())
            .align_y(Alignment::Center)
            .push(text("Event History").size(18))
            .push(
                button::text("Clear")
                    .on_press(Message::ClearHistory)
                    .class(theme::Button::Destructive)
                    .padding(theme::active().cosmic().space_xxs())
            );

        content = content.push(header);

        if self.history_events.is_empty() {
            content = content.push(
                container(
                    column::with_capacity(3)
                        .spacing(theme::active().cosmic().space_s())
                        .align_x(Alignment::Center)
                        .push(icon::from_name("document-open-recent-symbolic").size(64))
                        .push(text("No events yet").size(18))
                        .push(text("Device activity will appear here").size(14))
                )
                .center_x(Length::Fill)
                .center_y(Length::Fill)
            );
        } else {
            let mut events_list = column::with_capacity(self.history_events.len())
                .spacing(theme::active().cosmic().space_xs());

            for event in &self.history_events {
                let timestamp_str = {
                    let now = chrono::Local::now();
                    let diff = now.signed_duration_since(event.timestamp);

                    if diff.num_minutes() < 1 {
                        "Just now".to_string()
                    } else if diff.num_minutes() < 60 {
                        format!("{} minutes ago", diff.num_minutes())
                    } else if diff.num_hours() < 24 {
                        format!("{} hours ago", diff.num_hours())
                    } else {
                        event.timestamp.format("%b %d, %H:%M").to_string()
                    }
                };

                events_list = events_list.push(self.history_event_item(
                    &event.icon_name,
                    &event.event_type,
                    &event.description,
                    &timestamp_str,
                ));
            }

            content = content.push(events_list);
        }

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn history_event_item(
        &self,
        icon_name: &str,
        event_type: &str,
        description: &str,
        timestamp: &str,
    ) -> Element<'_, Message> {
        let event_icon = icon::from_name(icon_name).size(20);

        let event_info = column::with_capacity(2)
            .spacing(theme::active().cosmic().space_xxs())
            .push(text(event_type.to_string()).size(14))
            .push(
                row::with_capacity(3)
                    .spacing(theme::active().cosmic().space_xxs())
                    .align_y(Alignment::Center)
                    .push(text(description.to_string()).size(12))
                    .push(text("·").size(12))
                    .push(text(timestamp.to_string()).size(12))
            );

        let item_content = row::with_capacity(2)
            .spacing(theme::active().cosmic().space_s())
            .align_y(Alignment::Center)
            .push(event_icon)
            .push(event_info);

        container(item_content)
            .padding(theme::active().cosmic().space_s())
            .width(Length::Fill)
            .into()
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let mut content = column::with_capacity(6)
            .spacing(theme::active().cosmic().space_m())
            .padding(theme::active().cosmic().space_m());

        content = content.push(text("General Settings").size(18));

        let general_section = column::with_capacity(3)
            .spacing(theme::active().cosmic().space_s())
            .push(
                row::with_capacity(2)
                    .spacing(theme::active().cosmic().space_s())
                    .align_y(Alignment::Center)
                    .push(text("Auto-start daemon").size(14))
                    .push(
                        toggler(self.auto_start_enabled)
                            .on_toggle(Message::ToggleAutoStart)
                    )
            )
            .push(
                row::with_capacity(2)
                    .spacing(theme::active().cosmic().space_s())
                    .align_y(Alignment::Center)
                    .push(text("Show notifications").size(14))
                    .push(
                        toggler(self.show_notifications)
                            .on_toggle(Message::ToggleNotifications)
                    )
            );

        content = content.push(
            container(general_section)
                .padding(theme::active().cosmic().space_s())
                .width(Length::Fill)
        );

        content = content.push(vertical_space().height(theme::active().cosmic().space_m()));
        content = content.push(text("Plugin Settings").size(18));

        let plugins = vec![
            ("battery", "Battery", "Monitor battery level"),
            ("clipboard", "Clipboard Sync", "Synchronize clipboard content"),
            ("notification", "Notifications", "Sync notifications"),
            ("share", "File Sharing", "Send and receive files"),
            ("mpris", "Media Controls", "Control media playback"),
            ("findmyphone", "Find My Phone", "Locate device"),
            ("ping", "Ping", "Check connection"),
            ("runcommand", "Run Commands", "Execute remote commands"),
            ("remotedesktop", "Remote Desktop", "Screen sharing"),
            ("camera", "Camera", "Webcam streaming"),
        ];

        let mut plugin_section = column::with_capacity(plugins.len())
            .spacing(theme::active().cosmic().space_xs());

        for (plugin_id, plugin_name, plugin_desc) in plugins {
            let is_enabled = self.plugin_states.get(plugin_id).copied().unwrap_or(false);

            let plugin_info = column::with_capacity(2)
                .spacing(theme::active().cosmic().space_xxs())
                .push(text(plugin_name).size(14))
                .push(text(plugin_desc).size(12));

            let plugin_row = row::with_capacity(2)
                .spacing(theme::active().cosmic().space_s())
                .align_y(Alignment::Center)
                .push(plugin_info)
                .push(
                    toggler(is_enabled)
                        .on_toggle(move |enabled| Message::TogglePlugin(plugin_id.to_string(), enabled))
                );

            plugin_section = plugin_section.push(
                container(plugin_row)
                    .padding(theme::active().cosmic().space_s())
                    .width(Length::Fill)
            );
        }

        content = content.push(plugin_section);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn device_card<'a>(
        &self,
        device_id: &'a str,
        device: &'a DeviceInfo,
        config: Option<&'a DeviceConfig>,
        is_selected: bool,
    ) -> Element<'a, Message> {
        let device_icon = icon::from_name(device_icon_name(&device.device_type)).size(48);
        let display_name = config
            .and_then(|c| c.nickname.as_deref())
            .unwrap_or(&device.name);
        let name_text = text(display_name).size(16);
        let status_text = connection_status(device);
        let status_badge = text(status_text).size(12);

        let mut info_column = column::with_capacity(2)
            .spacing(theme::active().cosmic().space_xxs())
            .push(name_text)
            .push(status_badge);

        if let Some(battery) = self.battery_status.get(device_id) {
            let battery_icon = if battery.is_charging {
                "battery-charging-symbolic"
            } else if battery.level > 80 {
                "battery-full-symbolic"
            } else if battery.level > 50 {
                "battery-good-symbolic"
            } else if battery.level > 20 {
                "battery-low-symbolic"
            } else {
                "battery-caution-symbolic"
            };

            let battery_row = row::with_capacity(2)
                .spacing(theme::active().cosmic().space_xxs())
                .align_y(Alignment::Center)
                .push(icon::from_name(battery_icon).size(16))
                .push(text(format!("{}%", battery.level)).size(12));

            info_column = info_column.push(battery_row);
        }

        let info_row = row::with_capacity(2)
            .spacing(theme::active().cosmic().space_s())
            .align_y(Alignment::Center)
            .push(device_icon)
            .push(info_column);

        let mut card_content = column::with_capacity(2)
            .spacing(theme::active().cosmic().space_s())
            .push(info_row);

        if device.is_connected {
            // Get device-appropriate actions based on device type
            let available_actions = get_available_actions(device);

            // Split actions into rows of 5 for better layout
            let actions_per_row = 5;
            let mut action_rows: Vec<Element<'_, Message>> = Vec::new();
            let mut current_row = row::with_capacity(actions_per_row)
                .spacing(theme::active().cosmic().space_xs());
            let mut count = 0;

            for action in available_actions {
                // Skip Settings action from the button grid (it's accessed differently)
                if matches!(action, DeviceAction::Settings) {
                    continue;
                }

                let action_button = cosmic::widget::tooltip(
                    button::icon(icon::from_name(action.icon_name()).size(20))
                        .on_press(Message::ExecuteAction(device_id.to_string(), action))
                        .padding(theme::active().cosmic().space_xxs())
                        .class(theme::Button::Icon),
                    action.tooltip(),
                    cosmic::widget::tooltip::Position::Bottom,
                );

                current_row = current_row.push(action_button);
                count += 1;

                if count >= actions_per_row {
                    action_rows.push(current_row.into());
                    current_row = row::with_capacity(actions_per_row)
                        .spacing(theme::active().cosmic().space_xs());
                    count = 0;
                }
            }

            // Push remaining buttons if any
            if count > 0 {
                action_rows.push(current_row.into());
            }

            // Build action container
            let mut all_actions = column::with_capacity(action_rows.len())
                .spacing(theme::active().cosmic().space_xs());

            for action_row in action_rows {
                all_actions = all_actions.push(action_row);
            }

            card_content = card_content.push(all_actions);
        }

        let card_container = container(card_content)
            .padding(theme::active().cosmic().space_m())
            .width(Length::Fill)
            .class(theme::Container::Card);

        let card_button = if is_selected {
            button::custom(card_container)
                .class(theme::Button::Suggested)
        } else {
            button::custom(card_container)
        };

        card_button
            .on_press(Message::SelectDevice(device_id.to_string()))
            .padding(0)
            .width(Length::Fill)
            .into()
    }
}

impl Application for CosmicConnectManager {
    type Executor = cosmic::executor::Default;
    type Flags = Args;
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let initial_device = flags.device.clone();
        let initial_action = flags.action.as_deref().and_then(DeviceAction::from_str);

        let mut plugin_states = HashMap::new();
        plugin_states.insert("battery".to_string(), true);
        plugin_states.insert("clipboard".to_string(), true);
        plugin_states.insert("notification".to_string(), true);
        plugin_states.insert("share".to_string(), true);
        plugin_states.insert("mpris".to_string(), true);
        plugin_states.insert("findmyphone".to_string(), true);
        plugin_states.insert("ping".to_string(), true);
        plugin_states.insert("runcommand".to_string(), false);
        plugin_states.insert("remotedesktop".to_string(), false);
        plugin_states.insert("camera".to_string(), false);

        let connect_task = cosmic::task::future(async move {
            match DbusClient::connect().await {
                Ok((client, _event_rx)) => {
                    if let Err(e) = client.start_signal_listener().await {
                        tracing::warn!("Failed to start signal listener: {}", e);
                        return Message::DbusError(format!("Failed to start signal listener: {}", e));
                    }

                    Message::DbusConnected(client)
                }
                Err(e) => {
                    tracing::warn!("Failed to connect to DBus: {}", e);
                    Message::DbusError(format!("Failed to connect to daemon: {}", e))
                }
            }
        });

        (
            CosmicConnectManager {
                core,
                active_page: Page::Devices,
                dbus_client: None,
                devices: HashMap::new(),
                device_configs: HashMap::new(),
                battery_status: HashMap::new(),
                selected_device: initial_device.clone(),
                _initial_device: initial_device,
                _initial_action: initial_action,
                dbus_ready: false,
                auto_start_enabled: true,
                show_notifications: true,
                plugin_states,
                mpris_players: Vec::new(),
                active_transfers: HashMap::new(),
                completed_transfers: Vec::new(),
                history_events: Vec::new(),
                _event_rx: None,
            },
            connect_task,
        )
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Self::Message> {
        cosmic::iced::Subscription::none()
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        vec![]
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let sidebar = self.sidebar_view();
        let content = scrollable(self.content_view())
            .width(Length::Fill)
            .height(Length::Fill);

        row::with_capacity(2)
            .push(sidebar)
            .push(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::NavigateTo(page) => {
                self.active_page = page;
                Task::none()
            }
            Message::SelectDevice(device_id) => {
                self.selected_device = Some(device_id);
                Task::none()
            }
            Message::DevicesUpdated(devices) => {
                if let Some(client) = &self.dbus_client {
                    let client_clone = client.clone();
                    let connected_device_ids: Vec<String> = devices
                        .iter()
                        .filter(|(_, device)| device.is_connected)
                        .map(|(id, _)| id.clone())
                        .collect();

                    self.devices = devices;

                    let battery_tasks: Vec<_> = connected_device_ids
                        .into_iter()
                        .map(|device_id| {
                            let client = client_clone.clone();
                            cosmic::task::future(async move {
                                match client.get_battery_status(&device_id).await {
                                    Ok(status) => Message::BatteryStatusLoaded(device_id, status),
                                    Err(_) => Message::None,
                                }
                            })
                        })
                        .collect();

                    Task::batch(battery_tasks)
                } else {
                    self.devices = devices;
                    Task::none()
                }
            }
            Message::DeviceConfigLoaded(device_id, config) => {
                self.device_configs.insert(device_id, config);
                Task::none()
            }
            Message::DbusConnected(client) => {
                self.dbus_client = Some(client);
                self.dbus_ready = true;

                Task::batch(vec![
                    cosmic::task::future(async { Message::RefreshDevices }),
                    cosmic::task::future(async { Message::RefreshMprisPlayers }),
                ])
            }
            Message::DbusError(err) => {
                tracing::error!("DBus error: {}", err);
                self.dbus_ready = false;
                Task::none()
            }
            Message::RefreshDevices => {
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    cosmic::task::future(async move {
                        match client.list_devices().await {
                            Ok(devices) => Message::DevicesUpdated(devices),
                            Err(e) => {
                                tracing::warn!("Failed to list devices: {}", e);
                                Message::None
                            }
                        }
                    })
                } else {
                    Task::none()
                }
            }
            Message::RefreshMprisPlayers => {
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    cosmic::task::future(async move {
                        match client.get_mpris_players().await {
                            Ok(players) => Message::MprisPlayersLoaded(players),
                            Err(e) => {
                                tracing::warn!("Failed to get MPRIS players: {}", e);
                                Message::None
                            }
                        }
                    })
                } else {
                    Task::none()
                }
            }
            Message::MprisPlayersLoaded(players) => {
                let client = self.dbus_client.clone();
                let tasks: Vec<_> = players.iter().map(|player| {
                    let player_clone = player.clone();
                    let client_clone = client.clone();
                    cosmic::task::future(async move {
                        if let Some(client) = client_clone {
                            match client.get_player_state(&player_clone).await {
                                Ok(state) => Message::MprisPlayerStateLoaded(player_clone, state),
                                Err(_) => Message::MprisPlayerStateLoaded(player_clone.clone(), dbus_client::PlayerState {
                                    name: player_clone.clone(),
                                    identity: player_clone,
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
                                    metadata: dbus_client::PlayerMetadata::default(),
                                }),
                            }
                        } else {
                            Message::None
                        }
                    })
                }).collect();

                self.mpris_players = players.into_iter().map(|p| (p, None)).collect();
                Task::batch(tasks)
            }
            Message::MprisPlayerStateLoaded(player, state) => {
                if let Some(entry) = self.mpris_players.iter_mut().find(|(p, _)| p == &player) {
                    entry.1 = Some(state);
                }
                Task::none()
            }
            Message::ExecuteAction(device_id, action) => {
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    match action {
                        // Universal actions
                        DeviceAction::Ping => {
                            cosmic::task::future(async move {
                                if let Err(e) = client.send_ping(&device_id, "Ping from manager").await {
                                    tracing::error!("Failed to send ping: {}", e);
                                }
                                Message::None
                            })
                        }
                        DeviceAction::SendFile => {
                            // TODO: Open file picker dialog
                            tracing::info!("Send file to {}", device_id);
                            Task::none()
                        }
                        DeviceAction::Clipboard => {
                            // Toggle clipboard sync for this device
                            tracing::info!("Clipboard sync toggled for {}", device_id);
                            Task::none()
                        }
                        DeviceAction::RemoteInput => {
                            // TODO: Open remote input dialog/window
                            tracing::info!("Remote input requested for {}", device_id);
                            Task::none()
                        }
                        DeviceAction::Screenshot => {
                            cosmic::task::future(async move {
                                if let Err(e) = client.take_screenshot(&device_id).await {
                                    tracing::error!("Failed to take screenshot: {}", e);
                                }
                                Message::None
                            })
                        }
                        DeviceAction::SystemInfo => {
                            cosmic::task::future(async move {
                                if let Err(e) = client.request_system_info(&device_id).await {
                                    tracing::error!("Failed to request system info: {}", e);
                                }
                                Message::None
                            })
                        }
                        DeviceAction::Settings => {
                            // TODO: Open device settings dialog
                            Task::none()
                        }

                        // Mobile-only actions
                        DeviceAction::Find => {
                            cosmic::task::future(async move {
                                if let Err(e) = client.find_phone(&device_id).await {
                                    tracing::error!("Failed to find phone: {}", e);
                                }
                                Message::None
                            })
                        }
                        DeviceAction::Sms => {
                            // TODO: Open SMS compose dialog
                            tracing::info!("SMS compose requested for {}", device_id);
                            Task::none()
                        }
                        DeviceAction::MuteCall => {
                            cosmic::task::future(async move {
                                if let Err(e) = client.mute_call(&device_id).await {
                                    tracing::error!("Failed to mute call: {}", e);
                                }
                                Message::None
                            })
                        }
                        DeviceAction::Camera => {
                            // TODO: Start camera as webcam
                            tracing::info!("Camera webcam requested for {}", device_id);
                            Task::none()
                        }
                        DeviceAction::RefreshBattery => {
                            let device_id_clone = device_id.clone();
                            cosmic::task::future(async move {
                                match client.request_battery_update(&device_id).await {
                                    Ok(_) => {
                                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                                        match client.get_battery_status(&device_id).await {
                                            Ok(status) => Message::BatteryStatusLoaded(device_id_clone, status),
                                            Err(e) => {
                                                tracing::error!("Failed to get battery status: {}", e);
                                                Message::None
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to request battery update: {}", e);
                                        Message::None
                                    }
                                }
                            })
                        }
                        DeviceAction::Contacts => {
                            // TODO: Open contacts browser
                            tracing::info!("Contacts browser requested for {}", device_id);
                            Task::none()
                        }

                        // Desktop-only actions
                        DeviceAction::ScreenShare => {
                            cosmic::task::future(async move {
                                if let Err(e) = client.start_screen_share(&device_id, 5000).await {
                                    tracing::error!("Failed to start screen share: {}", e);
                                }
                                Message::None
                            })
                        }
                        DeviceAction::Lock => {
                            cosmic::task::future(async move {
                                if let Err(e) = client.lock_device(&device_id).await {
                                    tracing::error!("Failed to lock device: {}", e);
                                }
                                Message::None
                            })
                        }
                        DeviceAction::Power => {
                            // TODO: Show power options menu (shutdown, suspend, hibernate)
                            cosmic::task::future(async move {
                                if let Err(e) = client.power_action(&device_id, "suspend").await {
                                    tracing::error!("Failed to send power action: {}", e);
                                }
                                Message::None
                            })
                        }
                        DeviceAction::Wake => {
                            cosmic::task::future(async move {
                                if let Err(e) = client.wake_device(&device_id).await {
                                    tracing::error!("Failed to wake device: {}", e);
                                }
                                Message::None
                            })
                        }
                        DeviceAction::RunCommand => {
                            // TODO: Open run command dialog
                            tracing::info!("Run command requested for {}", device_id);
                            Task::none()
                        }
                        DeviceAction::Presenter => {
                            cosmic::task::future(async move {
                                if let Err(e) = client.start_presenter(&device_id).await {
                                    tracing::error!("Failed to start presenter mode: {}", e);
                                }
                                Message::None
                            })
                        }

                        // Media control
                        DeviceAction::MediaControl => {
                            // Switch to media page for this device
                            self.active_page = Page::MediaPlayers;
                            Task::none()
                        }
                    }
                } else {
                    Task::none()
                }
            }
            Message::MediaPlayPause(player) => {
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    cosmic::task::future(async move {
                        if let Err(e) = client.mpris_control(&player, "PlayPause").await {
                            tracing::error!("Failed to control media player: {}", e);
                        }
                        Message::None
                    })
                } else {
                    Task::none()
                }
            }
            Message::MediaNext(player) => {
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    cosmic::task::future(async move {
                        if let Err(e) = client.mpris_control(&player, "Next").await {
                            tracing::error!("Failed to control media player: {}", e);
                        }
                        Message::None
                    })
                } else {
                    Task::none()
                }
            }
            Message::MediaPrevious(player) => {
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    cosmic::task::future(async move {
                        if let Err(e) = client.mpris_control(&player, "Previous").await {
                            tracing::error!("Failed to control media player: {}", e);
                        }
                        Message::None
                    })
                } else {
                    Task::none()
                }
            }
            Message::CancelTransfer(transfer_id) => {
                if let Some(client) = &self.dbus_client {
                    let client = client.clone();
                    cosmic::task::future(async move {
                        if let Err(e) = client.cancel_transfer(&transfer_id).await {
                            tracing::error!("Failed to cancel transfer: {}", e);
                        }
                        Message::None
                    })
                } else {
                    Task::none()
                }
            }
            Message::TransferProgressUpdate(info) => {
                self.active_transfers.insert(info.transfer_id.clone(), info);
                Task::none()
            }
            Message::TransferCompleted(transfer_id, _device_id, filename, success, _error) => {
                self.active_transfers.remove(&transfer_id);

                let completed = CompletedTransfer {
                    filename: filename.clone(),
                    size: 0,
                    timestamp: chrono::Local::now(),
                    success,
                };
                self.completed_transfers.push(completed);

                let event = HistoryEvent {
                    icon_name: if success { "document-save-symbolic".to_string() } else { "dialog-error-symbolic".to_string() },
                    event_type: if success { "File received".to_string() } else { "Transfer failed".to_string() },
                    description: filename,
                    timestamp: chrono::Local::now(),
                };
                self.history_events.push(event);

                Task::none()
            }
            Message::DeviceAdded(device_id, device_info) => {
                self.devices.insert(device_id.clone(), device_info.clone());

                let event = HistoryEvent {
                    icon_name: "network-wireless-signal-excellent-symbolic".to_string(),
                    event_type: "Device discovered".to_string(),
                    description: device_info.name,
                    timestamp: chrono::Local::now(),
                };
                self.history_events.push(event);

                Task::none()
            }
            Message::DeviceRemoved(device_id) => {
                if let Some(device) = self.devices.remove(&device_id) {
                    let event = HistoryEvent {
                        icon_name: "network-wireless-offline-symbolic".to_string(),
                        event_type: "Device disconnected".to_string(),
                        description: device.name,
                        timestamp: chrono::Local::now(),
                    };
                    self.history_events.push(event);
                }
                Task::none()
            }
            Message::DeviceStateChanged(device_id, state) => {
                if let Some(device) = self.devices.get_mut(&device_id) {
                    match state.as_str() {
                        "connected" => device.is_connected = true,
                        "disconnected" => device.is_connected = false,
                        _ => {}
                    }
                }
                Task::none()
            }
            Message::AddHistoryEvent(event) => {
                self.history_events.push(event);
                Task::none()
            }
            Message::ClearHistory => {
                self.history_events.clear();
                Task::none()
            }
            Message::ToggleAutoStart(enabled) => {
                self.auto_start_enabled = enabled;
                Task::none()
            }
            Message::ToggleNotifications(enabled) => {
                self.show_notifications = enabled;
                Task::none()
            }
            Message::TogglePlugin(plugin_id, enabled) => {
                self.plugin_states.insert(plugin_id, enabled);
                Task::none()
            }
            Message::BatteryStatusLoaded(device_id, status) => {
                self.battery_status.insert(device_id, status);
                Task::none()
            }
            Message::DaemonEventReceived(event) => {
                match event {
                    DaemonEvent::DeviceAdded { device_id, device_info } => {
                        cosmic::task::future(async move { Message::DeviceAdded(device_id, device_info) })
                    }
                    DaemonEvent::DeviceRemoved { device_id } => {
                        cosmic::task::future(async move { Message::DeviceRemoved(device_id) })
                    }
                    DaemonEvent::DeviceStateChanged { device_id, state } => {
                        cosmic::task::future(async move { Message::DeviceStateChanged(device_id, state) })
                    }
                    DaemonEvent::TransferProgress { transfer_id, device_id, filename, current, total, direction } => {
                        let info = TransferInfo {
                            transfer_id,
                            device_id,
                            filename,
                            current,
                            total,
                            direction,
                        };
                        cosmic::task::future(async move { Message::TransferProgressUpdate(info) })
                    }
                    DaemonEvent::TransferComplete { transfer_id, device_id, filename, success, error } => {
                        cosmic::task::future(async move { Message::TransferCompleted(transfer_id, device_id, filename, success, error) })
                    }
                    _ => Task::none(),
                }
            }
            Message::DbusReady(client) => {
                self.dbus_client = Some(client);
                self.dbus_ready = true;
                Task::none()
            }
            Message::None => Task::none(),
        }
    }
}
