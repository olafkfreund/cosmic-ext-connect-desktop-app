mod dbus_client;

use cosmic::app::{Core, Settings, Task};
use cosmic::iced::{
    widget::{column, row},
    Alignment, Color, Length,
};
use cosmic::widget::{self, nav_bar};
use cosmic::{Application, Element};
use std::collections::HashMap;

use dbus_client::{DaemonEvent, DbusClient};

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt::init();
    cosmic::app::run::<KdeConnectApp>(Settings::default(), ())
}

/// Application pages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Devices,
    Transfers,
    Settings,
}

/// Transfer status
#[derive(Debug, Clone, PartialEq)]
enum TransferStatus {
    Active,
    Completed,
    Failed,
    Cancelled,
}

/// File transfer tracking
#[derive(Debug, Clone)]
struct Transfer {
    id: String,
    device_id: String,
    device_name: String,
    filename: String,
    bytes_transferred: u64,
    total_bytes: u64,
    direction: String, // "sending" or "receiving"
    status: TransferStatus,
    error_message: Option<String>,
}

impl Page {
    fn title(&self) -> &str {
        match self {
            Page::Devices => "Devices",
            Page::Transfers => "Transfers",
            Page::Settings => "Settings",
        }
    }

    fn icon(&self) -> &str {
        match self {
            Page::Devices => "phone-symbolic",
            Page::Transfers => "folder-download-symbolic",
            Page::Settings => "preferences-system-symbolic",
        }
    }
}

/// Main application state
struct KdeConnectApp {
    core: Core,
    nav_model: widget::segmented_button::SingleSelectModel,
    current_page: Page,
    devices: HashMap<String, dbus_client::DeviceInfo>,
    battery_statuses: HashMap<String, dbus_client::BatteryStatus>,
    dbus_client: Option<DbusClient>,
    selected_device_id: Option<String>,
    transfers: HashMap<String, Transfer>,
    mpris_players: Vec<String>,
    selected_mpris_player: Option<String>,
}

#[derive(Debug, Clone)]
enum Message {
    PageSelected(widget::segmented_button::Entity),
    DevicesLoaded(HashMap<String, dbus_client::DeviceInfo>),
    BatteryStatusesUpdated(HashMap<String, dbus_client::BatteryStatus>),
    RefreshDevices,
    PairDevice(String),
    UnpairDevice(String),
    AcceptPairing(String),
    RejectPairing(String),
    SelectDevice(String),
    BackToDeviceList,
    SendPing(String),
    FindPhone(String),
    SendFile(String),
    FileSelected(String, String), // device_id, file_path
    ShareText(String),
    TextInputOpened(String), // device_id for text sharing
    TextSubmitted(String, String), // device_id, text
    // MPRIS controls
    MprisPlayersUpdated(Vec<String>),
    MprisPlayerSelected(String),
    MprisControl(String, String), // player, action
    RefreshMprisPlayers,
    // DBus event
    DaemonEvent(DaemonEvent),
    // Transfer events
    TransferStarted(Transfer),
    TransferProgress {
        transfer_id: String,
        bytes_transferred: u64,
        total_bytes: u64,
    },
    TransferComplete {
        transfer_id: String,
        success: bool,
        error_message: String,
    },
}

impl Application for KdeConnectApp {
    type Message = Message;
    type Executor = cosmic::executor::multi::Executor;
    type Flags = ();
    const APP_ID: &'static str = "com.system76.CosmicConnect";

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        let mut nav_model = widget::segmented_button::ModelBuilder::default();

        // Add navigation items
        nav_model = nav_model.insert(|b| {
            b.text(Page::Devices.title())
                .icon(widget::icon::from_name(Page::Devices.icon()))
        });
        nav_model = nav_model.insert(|b| {
            b.text(Page::Transfers.title())
                .icon(widget::icon::from_name(Page::Transfers.icon()))
        });
        nav_model = nav_model.insert(|b| {
            b.text(Page::Settings.title())
                .icon(widget::icon::from_name(Page::Settings.icon()))
        });

        let nav_model = nav_model.build();
        let current_page = Page::Devices;

        let app = Self {
            core,
            nav_model,
            current_page,
            devices: HashMap::new(),
            battery_statuses: HashMap::new(),
            dbus_client: None,
            selected_device_id: None,
            transfers: HashMap::new(),
            mpris_players: Vec::new(),
            selected_mpris_player: None,
        };

        // Load devices and MPRIS players on startup
        (
            app,
            Task::batch(vec![
                Task::perform(fetch_devices(), |devices| {
                    cosmic::Action::App(Message::DevicesLoaded(devices))
                }),
                Task::perform(fetch_mpris_players(), |players| {
                    cosmic::Action::App(Message::MprisPlayersUpdated(players))
                }),
            ]),
        )
    }

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::PageSelected(entity) => {
                if let Some(page_idx) = self.nav_model.data::<Page>(entity) {
                    self.current_page = *page_idx;
                }
                Task::none()
            }
            Message::DevicesLoaded(devices) => {
                tracing::info!("Loaded {} devices", devices.len());
                self.devices = devices;

                // Fetch battery statuses for connected devices
                let connected_device_ids: Vec<String> = self
                    .devices
                    .iter()
                    .filter(|(_, d)| d.is_connected)
                    .map(|(id, _)| id.clone())
                    .collect();

                if !connected_device_ids.is_empty() {
                    Task::perform(
                        fetch_battery_statuses(connected_device_ids),
                        |statuses| cosmic::Action::App(Message::BatteryStatusesUpdated(statuses)),
                    )
                } else {
                    Task::none()
                }
            }
            Message::BatteryStatusesUpdated(statuses) => {
                tracing::debug!("Updated battery statuses for {} devices", statuses.len());
                self.battery_statuses = statuses;
                Task::none()
            }
            Message::RefreshDevices => {
                tracing::info!("Refreshing device list");
                Task::perform(fetch_devices(), |devices| {
                    cosmic::Action::App(Message::DevicesLoaded(devices))
                })
            }
            Message::PairDevice(device_id) => {
                tracing::info!("Pairing device: {}", device_id);
                Task::perform(pair_device(device_id), |_| {
                    cosmic::Action::App(Message::RefreshDevices)
                })
            }
            Message::UnpairDevice(device_id) => {
                tracing::info!("Unpairing device: {}", device_id);
                Task::perform(unpair_device(device_id), |_| {
                    cosmic::Action::App(Message::RefreshDevices)
                })
            }
            Message::AcceptPairing(device_id) => {
                tracing::info!("Accepting pairing request from: {}", device_id);
                Task::perform(accept_pairing(device_id), |_| {
                    cosmic::Action::App(Message::RefreshDevices)
                })
            }
            Message::RejectPairing(device_id) => {
                tracing::info!("Rejecting pairing request from: {}", device_id);
                Task::perform(reject_pairing(device_id), |_| {
                    cosmic::Action::App(Message::RefreshDevices)
                })
            }
            Message::SelectDevice(device_id) => {
                tracing::info!("Selected device: {}", device_id);
                self.selected_device_id = Some(device_id);
                Task::none()
            }
            Message::BackToDeviceList => {
                tracing::info!("Returning to device list");
                self.selected_device_id = None;
                Task::none()
            }
            Message::SendPing(device_id) => {
                tracing::info!("Sending ping to device: {}", device_id);
                Task::perform(send_ping(device_id), |result| {
                    if let Err(e) = result {
                        tracing::error!("Failed to send ping: {}", e);
                    }
                    cosmic::Action::App(Message::RefreshDevices)
                })
            }
            Message::FindPhone(device_id) => {
                tracing::info!("Finding phone: {}", device_id);
                Task::perform(find_phone(device_id), |result| {
                    if let Err(e) = result {
                        tracing::error!("Failed to find phone: {}", e);
                    }
                    cosmic::Action::App(Message::RefreshDevices)
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
                            tracing::debug!("File picker cancelled");
                            cosmic::Action::App(Message::RefreshDevices)
                        }
                    }
                })
            }
            Message::FileSelected(device_id, file_path) => {
                tracing::info!("Sending file {} to device: {}", file_path, device_id);
                Task::perform(share_file(device_id, file_path), |result| {
                    if let Err(e) = result {
                        tracing::error!("Failed to share file: {}", e);
                    }
                    cosmic::Action::App(Message::RefreshDevices)
                })
            }
            Message::ShareText(device_id) => {
                tracing::info!("Share text requested for device: {}", device_id);
                // For now, share clipboard content
                // TODO: Add text input dialog in future enhancement
                Task::perform(share_clipboard(device_id), |result| {
                    if let Err(e) = result {
                        tracing::error!("Failed to share text: {}", e);
                    }
                    cosmic::Action::App(Message::RefreshDevices)
                })
            }
            Message::TextInputOpened(_device_id) => {
                // TODO: Implement text input dialog
                Task::none()
            }
            Message::TextSubmitted(device_id, text) => {
                tracing::info!("Sharing text to device: {}", device_id);
                Task::perform(share_text(device_id, text), |result| {
                    if let Err(e) = result {
                        tracing::error!("Failed to share text: {}", e);
                    }
                    cosmic::Action::App(Message::RefreshDevices)
                })
            }
            Message::MprisPlayersUpdated(players) => {
                tracing::info!("MPRIS players updated: {} players", players.len());
                self.mpris_players = players;
                // Auto-select first player if none selected
                if self.selected_mpris_player.is_none() && !self.mpris_players.is_empty() {
                    self.selected_mpris_player = Some(self.mpris_players[0].clone());
                }
                Task::none()
            }
            Message::MprisPlayerSelected(player) => {
                tracing::info!("MPRIS player selected: {}", player);
                self.selected_mpris_player = Some(player);
                Task::none()
            }
            Message::MprisControl(player, action) => {
                tracing::info!("MPRIS control: {} on {}", action, player);
                Task::perform(mpris_control(player, action), |result| {
                    if let Err(e) = result {
                        tracing::error!("Failed to control MPRIS player: {}", e);
                    }
                    cosmic::Action::None
                })
            }
            Message::RefreshMprisPlayers => {
                tracing::info!("Refreshing MPRIS players");
                Task::perform(fetch_mpris_players(), |players| {
                    cosmic::Action::App(Message::MprisPlayersUpdated(players))
                })
            }
            Message::DaemonEvent(event) => {
                match event {
                    DaemonEvent::TransferProgress {
                        transfer_id,
                        device_id,
                        filename,
                        bytes_transferred,
                        total_bytes,
                        direction,
                    } => {
                        // Get or create transfer
                        if !self.transfers.contains_key(&transfer_id) {
                            // Create new transfer
                            let device_name = self
                                .devices
                                .get(&device_id)
                                .map(|d| d.name.clone())
                                .unwrap_or_else(|| "Unknown Device".to_string());

                            let transfer = Transfer {
                                id: transfer_id.clone(),
                                device_id: device_id.clone(),
                                device_name,
                                filename: filename.clone(),
                                bytes_transferred,
                                total_bytes,
                                direction: direction.clone(),
                                status: TransferStatus::Active,
                                error_message: None,
                            };
                            self.transfers.insert(transfer_id.clone(), transfer);
                        } else {
                            // Update existing transfer
                            if let Some(transfer) = self.transfers.get_mut(&transfer_id) {
                                transfer.bytes_transferred = bytes_transferred;
                                transfer.total_bytes = total_bytes;
                            }
                        }
                    }
                    DaemonEvent::TransferComplete {
                        transfer_id,
                        success,
                        error_message,
                        ..
                    } => {
                        if let Some(transfer) = self.transfers.get_mut(&transfer_id) {
                            transfer.status = if success {
                                TransferStatus::Completed
                            } else {
                                TransferStatus::Failed
                            };
                            if !error_message.is_empty() {
                                transfer.error_message = Some(error_message);
                            }
                        }
                    }
                    DaemonEvent::DeviceAdded { device_id, device_info } => {
                        tracing::info!("Device added: {}", device_id);
                        self.devices.insert(device_id, device_info);
                    }
                    DaemonEvent::DeviceRemoved { device_id } => {
                        tracing::info!("Device removed: {}", device_id);
                        self.devices.remove(&device_id);
                    }
                    DaemonEvent::DeviceStateChanged { device_id, state } => {
                        tracing::info!("Device {} state changed to: {}", device_id, state);
                        // Refresh devices to get updated state
                        return Task::perform(fetch_devices(), |devices| {
                            cosmic::Action::App(Message::DevicesLoaded(devices))
                        });
                    }
                    _ => {
                        // Other events not handled yet
                        tracing::debug!("Unhandled daemon event: {:?}", event);
                    }
                }
                Task::none()
            }
            Message::TransferStarted(transfer) => {
                tracing::info!("Transfer started: {} - {}", transfer.id, transfer.filename);
                self.transfers.insert(transfer.id.clone(), transfer);
                Task::none()
            }
            Message::TransferProgress {
                transfer_id,
                bytes_transferred,
                total_bytes,
            } => {
                if let Some(transfer) = self.transfers.get_mut(&transfer_id) {
                    transfer.bytes_transferred = bytes_transferred;
                    transfer.total_bytes = total_bytes;
                }
                Task::none()
            }
            Message::TransferComplete {
                transfer_id,
                success,
                error_message,
            } => {
                if let Some(transfer) = self.transfers.get_mut(&transfer_id) {
                    transfer.status = if success {
                        TransferStatus::Completed
                    } else {
                        TransferStatus::Failed
                    };
                    if !error_message.is_empty() {
                        transfer.error_message = Some(error_message);
                    }
                }
                Task::none()
            }
        }
    }

    // TODO: Implement subscription for DBus events to get live transfer progress
    // For now, transfer progress will be displayed when manually triggered

    fn view(&self) -> Element<'_, Self::Message> {
        let nav = nav_bar(&self.nav_model, Message::PageSelected);

        let content = match self.current_page {
            Page::Devices => self.devices_view(),
            Page::Transfers => self.transfers_view(),
            Page::Settings => self.settings_view(),
        };

        widget::container(row![nav, content].spacing(0).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

impl KdeConnectApp {
    /// View for the Devices page
    fn devices_view(&self) -> Element<'_, Message> {
        // If a device is selected, show details view instead
        if let Some(device_id) = &self.selected_device_id {
            if let Some(device) = self.devices.get(device_id) {
                return self.device_details_view(device);
            }
        }

        // Otherwise show device list
        let header = row![
            widget::text::title3("Devices"),
            widget::horizontal_space(),
            widget::button::standard("Refresh")
                .on_press(Message::RefreshDevices)
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding(24);

        let devices_list: Element<Message> = if self.devices.is_empty() {
            column![
                widget::text("No devices found"),
                widget::text("Make sure KDE Connect is installed on your devices")
                    .size(14),
            ]
            .spacing(8)
            .padding(24)
            .into()
        } else {
            let mut col = widget::column().spacing(12).padding(24);
            for device in self.devices.values() {
                col = col.push(self.device_card(device));
            }
            col.into()
        };

        column![header, widget::divider::horizontal::default(), devices_list]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Card for individual device
    fn device_card<'a>(&self, device: &'a dbus_client::DeviceInfo) -> Element<'a, Message> {
        let status = if device.has_pairing_request {
            "Pairing Request!"
        } else if device.is_connected {
            "Connected"
        } else if device.is_paired {
            "Paired"
        } else {
            "Available"
        };

        let pair_button: Element<_> = if device.has_pairing_request {
            // Show Accept/Reject buttons for pending pairing requests
            row![
                widget::button::suggested("Accept")
                    .on_press(Message::AcceptPairing(device.id.clone())),
                widget::button::destructive("Reject")
                    .on_press(Message::RejectPairing(device.id.clone())),
            ]
            .spacing(8)
            .into()
        } else if device.is_paired {
            widget::button::standard("Unpair")
                .on_press(Message::UnpairDevice(device.id.clone()))
                .into()
        } else {
            widget::button::suggested("Pair")
                .on_press(Message::PairDevice(device.id.clone()))
                .into()
        };

        let icon_style = device_type_style(&device.device_type);
        let device_id_for_click = device.id.clone();

        // Build name and status column with optional battery indicator
        let mut info_column = column![
            widget::text(&device.name).size(16),
            widget::text(status).size(12),
        ]
        .spacing(4);

        // Add battery info if available
        if let Some(battery) = self.battery_statuses.get(&device.id) {
            let battery_icon = battery_icon_name(battery.level, battery.is_charging);
            let battery_row = row![
                widget::icon::from_name(battery_icon).size(14),
                widget::text(format!("{}%", battery.level)).size(12),
            ]
            .spacing(4)
            .align_y(Alignment::Center);
            info_column = info_column.push(battery_row);
        }

        // Create styled device icon
        let icon = styled_device_icon(icon_style.icon_name, icon_style.color, 24, 8);

        widget::button::custom(
            widget::container(
                column![row![
                    icon,
                    info_column,
                    widget::horizontal_space(),
                    pair_button,
                ]
                .spacing(12)
                .align_y(Alignment::Center),]
                .padding(16)
            )
            .style(card_container_style)
            .width(Length::Fill)
        )
        .on_press(Message::SelectDevice(device_id_for_click))
        .width(Length::Fill)
        .into()
    }

    /// Detailed view for a selected device
    fn device_details_view<'a>(&self, device: &'a dbus_client::DeviceInfo) -> Element<'a, Message> {
        let status = if device.is_connected {
            ("Connected", Color::from_rgb(0.2, 0.8, 0.4))
        } else if device.is_paired {
            ("Paired (Disconnected)", Color::from_rgb(0.5, 0.5, 0.5))
        } else {
            ("Available", Color::from_rgb(0.8, 0.6, 0.2))
        };

        let icon_style = device_type_style(&device.device_type);

        // Header with back button
        let header = row![
            widget::button::icon(widget::icon::from_name("go-previous-symbolic"))
                .on_press(Message::BackToDeviceList),
            widget::horizontal_space(),
            widget::button::standard("Refresh")
                .on_press(Message::RefreshDevices)
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding(24);

        // Styled device icon (larger for details view)
        let icon = styled_device_icon(icon_style.icon_name, icon_style.color, 48, 16);

        // Device info section
        let device_info = widget::container(
            column![
                row![
                    icon,
                    widget::horizontal_space(),
                ]
                .spacing(16)
                .align_y(Alignment::Center),
                widget::text(&device.name).size(24),
                widget::text(status.0).size(14),
            ]
            .spacing(12)
            .padding(24)
        )
        .style(card_container_style);

        // Device details section
        let mut details_col = column![
            widget::text("Device Information").size(18),
            widget::divider::horizontal::default(),
            detail_row("Type:", &device.device_type),
            detail_row("ID:", &device.id),
            detail_row("Status:", if device.is_connected { "Online" } else { "Offline" }),
            detail_row("Paired:", if device.is_paired { "Yes" } else { "No" }),
            detail_row("Reachable:", if device.is_reachable { "Yes" } else { "No" }),
        ]
        .spacing(8);

        // Add battery information if available
        if let Some(battery) = self.battery_statuses.get(&device.id) {
            let battery_icon = battery_icon_name(battery.level, battery.is_charging);
            details_col = details_col.push(
                row![
                    widget::text("Battery:").size(14),
                    widget::horizontal_space(),
                    row![
                        widget::icon::from_name(battery_icon).size(14),
                        widget::text(format!(
                            "{}%{}",
                            battery.level,
                            if battery.is_charging {
                                " (Charging)"
                            } else {
                                ""
                            }
                        ))
                        .size(14),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                ]
                .spacing(8),
            );
        }

        let details = widget::container(details_col.padding(16))
            .style(card_container_style);

        // Actions section (if device is paired and connected)
        let device_id_for_actions = device.id.clone();
        let actions: Element<Message> = if device.is_paired && device.is_connected {
            let id1 = device_id_for_actions.clone();
            let id2 = device_id_for_actions.clone();
            let id3 = device_id_for_actions.clone();
            let id4 = device_id_for_actions.clone();

            widget::container(
                column![
                    widget::text("Actions").size(18),
                    widget::divider::horizontal::default(),
                    row![
                        widget::button::standard("Send Ping")
                            .on_press(Message::SendPing(id1)),
                        widget::button::standard("Send File")
                            .on_press(Message::SendFile(id2)),
                    ]
                    .spacing(8),
                    row![
                        widget::button::standard("Find Phone")
                            .on_press(Message::FindPhone(id3)),
                        widget::button::standard("Share Text")
                            .on_press(Message::ShareText(id4)),
                    ]
                    .spacing(8),
                ]
                .spacing(12)
                .padding(16)
            )
            .style(card_container_style)
            .into()
        } else {
            widget::container(
                column![
                    widget::text("Actions unavailable").size(14),
                    widget::text("Device must be paired and connected").size(12),
                ]
                .spacing(4)
                .padding(16)
            )
            .into()
        };

        // Main content
        let content = widget::scrollable(
            column![device_info, details, actions]
                .spacing(16)
                .padding(24)
        );

        column![header, widget::divider::horizontal::default(), content]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// View for the Transfers page
    fn transfers_view(&self) -> Element<'_, Message> {
        let header = row![
            widget::text::title3("File Transfers"),
            widget::horizontal_space(),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding(24);

        let transfers_list: Element<Message> = if self.transfers.is_empty() {
            column![
                widget::text("No active transfers"),
                widget::text("File transfers will appear here when you send or receive files")
                    .size(14),
            ]
            .spacing(8)
            .padding(24)
            .into()
        } else {
            let mut col = widget::column().spacing(12).padding(24);

            // Separate active and completed transfers
            let mut active_transfers: Vec<_> = self
                .transfers
                .values()
                .filter(|t| t.status == TransferStatus::Active)
                .collect();
            let mut completed_transfers: Vec<_> = self
                .transfers
                .values()
                .filter(|t| t.status != TransferStatus::Active)
                .collect();

            // Sort by ID (which includes timestamp)
            active_transfers.sort_by(|a, b| b.id.cmp(&a.id));
            completed_transfers.sort_by(|a, b| b.id.cmp(&a.id));

            // Show active transfers first
            if !active_transfers.is_empty() {
                col = col.push(widget::text("Active Transfers").size(16));
                for transfer in active_transfers {
                    col = col.push(self.transfer_card(transfer));
                }
            }

            // Show completed transfers
            if !completed_transfers.is_empty() {
                col = col.push(widget::text("Recent Transfers").size(16));
                for transfer in completed_transfers {
                    col = col.push(self.transfer_card(transfer));
                }
            }

            col.into()
        };

        column![header, widget::divider::horizontal::default(), transfers_list]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Card for individual transfer
    fn transfer_card<'a>(&self, transfer: &'a Transfer) -> Element<'a, Message> {
        let direction_icon = if transfer.direction == "sending" {
            "go-up-symbolic"
        } else {
            "go-down-symbolic"
        };

        let progress_percentage = if transfer.total_bytes > 0 {
            (transfer.bytes_transferred as f64 / transfer.total_bytes as f64 * 100.0) as u32
        } else {
            0
        };

        let status_text = match transfer.status {
            TransferStatus::Active => format!("{}%", progress_percentage),
            TransferStatus::Completed => "Completed".to_string(),
            TransferStatus::Failed => "Failed".to_string(),
            TransferStatus::Cancelled => "Cancelled".to_string(),
        };

        let status_color = match transfer.status {
            TransferStatus::Active => Color::from_rgb(0.4, 0.6, 0.8),
            TransferStatus::Completed => Color::from_rgb(0.2, 0.8, 0.4),
            TransferStatus::Failed => Color::from_rgb(0.8, 0.2, 0.2),
            TransferStatus::Cancelled => Color::from_rgb(0.6, 0.6, 0.6),
        };

        let mut content_col = column![
            row![
                widget::icon::from_name(direction_icon).size(24),
                column![
                    widget::text(&transfer.filename).size(14),
                    widget::text(format!(
                        "{} {} {}",
                        if transfer.direction == "sending" {
                            "Sending to"
                        } else {
                            "Receiving from"
                        },
                        &transfer.device_name,
                        format_bytes(transfer.bytes_transferred)
                    ))
                    .size(12),
                ]
                .spacing(4),
                widget::horizontal_space(),
                widget::text(status_text).size(14),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ]
        .spacing(8);

        // Add progress bar for active transfers
        if transfer.status == TransferStatus::Active && transfer.total_bytes > 0 {
            content_col = content_col.push(
                widget::progress_bar(0.0..=100.0, progress_percentage as f32)
                    .width(Length::Fill),
            );

            // Show transfer speed and ETA
            let speed_text = format!(
                "{} / {} ({})",
                format_bytes(transfer.bytes_transferred),
                format_bytes(transfer.total_bytes),
                format_bytes(transfer.total_bytes - transfer.bytes_transferred)
            );
            content_col = content_col.push(widget::text(speed_text).size(12));
        }

        // Show error message if failed
        if let Some(error) = &transfer.error_message {
            content_col = content_col.push(widget::text(format!("Error: {}", error)).size(12));
        }

        widget::container(content_col.padding(16))
            .style(move |_theme| cosmic::iced::widget::container::Style {
                background: Some(cosmic::iced::Background::Color(Color::from_rgb(
                    0.1, 0.1, 0.1,
                ))),
                border: cosmic::iced::Border {
                    color: status_color,
                    width: 2.0,
                    radius: 8.0.into(),
                },
                ..Default::default()
            })
            .width(Length::Fill)
            .into()
    }

    /// View for the Settings page
    fn settings_view(&self) -> Element<'_, Message> {
        let header = row![
            widget::text::title3("Settings"),
            widget::horizontal_space(),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding(24);

        // MPRIS Media Controls Section
        let mpris_section = self.mpris_controls_section();

        // About Section
        let about_section = widget::container(
            column![
                widget::text::title4("About"),
                widget::divider::horizontal::default(),
                row![
                    widget::text("Application:").size(14),
                    widget::horizontal_space(),
                    widget::text("COSMIC Connect").size(14),
                ]
                .spacing(8),
                row![
                    widget::text("Version:").size(14),
                    widget::horizontal_space(),
                    widget::text(env!("CARGO_PKG_VERSION")).size(14),
                ]
                .spacing(8),
                row![
                    widget::text("Protocol:").size(14),
                    widget::horizontal_space(),
                    widget::text("KDE Connect v7/8").size(14),
                ]
                .spacing(8),
            ]
            .spacing(12)
            .padding(16)
        )
        .style(card_container_style)
        .width(Length::Fill);

        let content = widget::scrollable(
            column![mpris_section, about_section]
                .spacing(16)
                .padding(24)
        );

        column![header, widget::divider::horizontal::default(), content]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// MPRIS media controls section for Settings page
    fn mpris_controls_section(&self) -> Element<'_, Message> {
        let mut content_col = column![
            widget::text::title4("Media Player Controls"),
            widget::divider::horizontal::default(),
        ]
        .spacing(12);

        if self.mpris_players.is_empty() {
            content_col = content_col.push(
                column![
                    widget::text("No media players found").size(14),
                    widget::text("Make sure a media player is running").size(12),
                    widget::button::standard("Refresh Players")
                        .on_press(Message::RefreshMprisPlayers),
                ]
                .spacing(8)
            );
        } else {
            // Player selector
            if let Some(selected) = &self.selected_mpris_player {
                content_col = content_col.push(
                    row![
                        widget::text("Selected Player:").size(14),
                        widget::horizontal_space(),
                        widget::text(selected).size(14),
                    ]
                    .spacing(8)
                );

                // Playback controls
                let controls = row![
                    widget::button::icon(
                        widget::icon::from_name("media-skip-backward-symbolic").size(20)
                    )
                    .on_press(Message::MprisControl(
                        selected.clone(),
                        "Previous".to_string()
                    ))
                    .padding(12),
                    widget::button::icon(
                        widget::icon::from_name("media-playback-start-symbolic").size(24)
                    )
                    .on_press(Message::MprisControl(
                        selected.clone(),
                        "PlayPause".to_string()
                    ))
                    .padding(12),
                    widget::button::icon(
                        widget::icon::from_name("media-playback-stop-symbolic").size(20)
                    )
                    .on_press(Message::MprisControl(
                        selected.clone(),
                        "Stop".to_string()
                    ))
                    .padding(12),
                    widget::button::icon(
                        widget::icon::from_name("media-skip-forward-symbolic").size(20)
                    )
                    .on_press(Message::MprisControl(
                        selected.clone(),
                        "Next".to_string()
                    ))
                    .padding(12),
                ]
                .spacing(8)
                .align_y(Alignment::Center);

                content_col = content_col.push(controls);
            }

            // Refresh button
            content_col = content_col.push(
                widget::button::standard("Refresh Players")
                    .on_press(Message::RefreshMprisPlayers)
            );

            // List all available players
            content_col = content_col.push(widget::text("Available Players:").size(14));
            for player in &self.mpris_players {
                content_col = content_col.push(
                    widget::button::text(player)
                        .on_press(Message::MprisPlayerSelected(player.clone()))
                        .width(Length::Fill)
                );
            }
        }

        widget::container(content_col.padding(16))
            .style(card_container_style)
            .width(Length::Fill)
            .into()
    }
}

/// Fetch devices from daemon
async fn fetch_devices() -> HashMap<String, dbus_client::DeviceInfo> {
    match DbusClient::connect().await {
        Ok((client, _)) => match client.list_devices().await {
            Ok(devices) => {
                tracing::info!("Fetched {} devices", devices.len());
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

/// Fetch battery statuses for connected devices
async fn fetch_battery_statuses(
    device_ids: Vec<String>,
) -> HashMap<String, dbus_client::BatteryStatus> {
    let mut battery_statuses = HashMap::new();

    if device_ids.is_empty() {
        return battery_statuses;
    }

    match DbusClient::connect().await {
        Ok((client, _)) => {
            for device_id in device_ids {
                match client.get_battery_status(&device_id).await {
                    Ok(status) => {
                        battery_statuses.insert(device_id, status);
                    }
                    Err(e) => {
                        tracing::debug!("Failed to get battery status for {}: {}", device_id, e);
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to connect to daemon for battery statuses: {}", e);
        }
    }

    battery_statuses
}

/// Pair a device
async fn pair_device(device_id: String) {
    if let Ok((client, _)) = DbusClient::connect().await {
        if let Err(e) = client.pair_device(&device_id).await {
            tracing::error!("Failed to pair device {}: {}", device_id, e);
        }
    }
}

/// Unpair a device
async fn unpair_device(device_id: String) {
    if let Ok((client, _)) = DbusClient::connect().await {
        if let Err(e) = client.unpair_device(&device_id).await {
            tracing::error!("Failed to unpair device {}: {}", device_id, e);
        }
    }
}

/// Accept an incoming pairing request
async fn accept_pairing(device_id: String) {
    if let Ok((client, _)) = DbusClient::connect().await {
        if let Err(e) = client.accept_pairing(&device_id).await {
            tracing::error!("Failed to accept pairing from {}: {}", device_id, e);
        }
    }
}

/// Reject an incoming pairing request
async fn reject_pairing(device_id: String) {
    if let Ok((client, _)) = DbusClient::connect().await {
        if let Err(e) = client.reject_pairing(&device_id).await {
            tracing::error!("Failed to reject pairing from {}: {}", device_id, e);
        }
    }
}

/// Send ping to device
async fn send_ping(device_id: String) -> anyhow::Result<()> {
    let (client, _) = DbusClient::connect().await?;
    client.send_ping(&device_id, "Hello from COSMIC!").await?;
    tracing::info!("Ping sent to device {}", device_id);
    Ok(())
}

/// Find phone (ring it)
async fn find_phone(device_id: String) -> anyhow::Result<()> {
    let (client, _) = DbusClient::connect().await?;
    client.find_phone(&device_id).await?;
    tracing::info!("Find phone triggered for device {}", device_id);
    Ok(())
}

/// Open file picker dialog
async fn open_file_picker(device_id: String) -> Option<(String, String)> {
    use ashpd::desktop::file_chooser::OpenFileRequest;

    let request = OpenFileRequest::default()
        .title("Select file to send")
        .modal(true)
        .multiple(false);

    match request.send().await {
        Ok(request) => match request.response() {
            Ok(response) => {
                if let Some(uri) = response.uris().first() {
                    let path = uri.path().to_string();
                    tracing::info!("File selected: {}", path);
                    Some((device_id, path))
                } else {
                    None
                }
            }
            Err(e) => {
                tracing::error!("Failed to get file picker response: {}", e);
                None
            }
        },
        Err(e) => {
            tracing::error!("Failed to open file picker: {}", e);
            None
        }
    }
}

/// Share a file with a device
async fn share_file(device_id: String, file_path: String) -> anyhow::Result<()> {
    let (client, _) = DbusClient::connect().await?;
    client.share_file(&device_id, &file_path).await?;
    tracing::info!("File {} shared with device {}", file_path, device_id);
    Ok(())
}

/// Share text with a device
async fn share_text(device_id: String, text: String) -> anyhow::Result<()> {
    let (client, _) = DbusClient::connect().await?;
    client.share_text(&device_id, &text).await?;
    tracing::info!("Text shared with device {}", device_id);
    Ok(())
}

/// Share clipboard content with a device
async fn share_clipboard(device_id: String) -> anyhow::Result<()> {
    // TODO: Get actual clipboard content
    // For now, just share a placeholder message
    let text = "Shared from COSMIC Connect".to_string();
    share_text(device_id, text).await
}

/// Fetch available MPRIS media players
async fn fetch_mpris_players() -> Vec<String> {
    match DbusClient::connect().await {
        Ok((client, _)) => match client.get_mpris_players().await {
            Ok(players) => {
                tracing::info!("Fetched {} MPRIS players", players.len());
                players
            }
            Err(e) => {
                tracing::error!("Failed to get MPRIS players: {}", e);
                Vec::new()
            }
        },
        Err(e) => {
            tracing::warn!("Failed to connect to daemon for MPRIS: {}", e);
            Vec::new()
        }
    }
}

/// Control an MPRIS media player
async fn mpris_control(player: String, action: String) -> anyhow::Result<()> {
    let (client, _) = DbusClient::connect().await?;
    client.mpris_control(&player, &action).await?;
    tracing::info!("MPRIS control {} executed on {}", action, player);
    Ok(())
}

/// Format bytes as human-readable string
fn format_bytes(bytes: u64) -> String {
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

/// Get battery icon name based on level and charging status
fn battery_icon_name(level: i32, is_charging: bool) -> &'static str {
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

/// Device icon styling information
struct DeviceIconStyle {
    icon_name: &'static str,
    color: Color,
}

/// Get device type icon style (name and color)
fn device_type_style(device_type: &str) -> DeviceIconStyle {
    match device_type.to_lowercase().as_str() {
        "phone" => DeviceIconStyle {
            icon_name: "phone-symbolic",
            color: Color::from_rgb(0.3, 0.6, 0.9), // Blue
        },
        "tablet" => DeviceIconStyle {
            icon_name: "tablet-symbolic",
            color: Color::from_rgb(0.6, 0.4, 0.9), // Purple
        },
        "desktop" => DeviceIconStyle {
            icon_name: "computer-symbolic",
            color: Color::from_rgb(0.5, 0.7, 0.5), // Green
        },
        "laptop" => DeviceIconStyle {
            icon_name: "laptop-symbolic",
            color: Color::from_rgb(0.9, 0.6, 0.3), // Orange
        },
        "tv" => DeviceIconStyle {
            icon_name: "tv-symbolic",
            color: Color::from_rgb(0.9, 0.4, 0.5), // Pink
        },
        _ => DeviceIconStyle {
            icon_name: "computer-symbolic",
            color: Color::from_rgb(0.6, 0.6, 0.6), // Gray (default)
        },
    }
}

/// Creates a styled device icon with circular colored background
fn styled_device_icon<'a>(
    icon_name: &'static str,
    color: Color,
    icon_size: u16,
    padding: u16,
) -> Element<'a, Message> {
    let radius = (icon_size + padding * 2) as f32 / 2.0;
    widget::container(widget::icon::from_name(icon_name).size(icon_size))
        .padding(padding)
        .style(move |_theme| cosmic::iced::widget::container::Style {
            background: Some(cosmic::iced::Background::Color(color)),
            border: cosmic::iced::Border {
                radius: radius.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// Returns the standard card container style
fn card_container_style(_theme: &cosmic::Theme) -> cosmic::iced::widget::container::Style {
    cosmic::iced::widget::container::Style {
        background: Some(cosmic::iced::Background::Color(Color::from_rgb(0.1, 0.1, 0.1))),
        border: cosmic::iced::Border {
            radius: 8.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Creates a detail row with label and value
fn detail_row<'a>(label: &'a str, value: impl ToString) -> Element<'a, Message> {
    row![
        widget::text(label).size(14),
        widget::horizontal_space(),
        widget::text(value.to_string()).size(14),
    ]
    .spacing(8)
    .into()
}
