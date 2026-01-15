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
    dbus_client: Option<DbusClient>,
    selected_device_id: Option<String>,
    transfers: HashMap<String, Transfer>,
}

#[derive(Debug, Clone)]
enum Message {
    PageSelected(widget::segmented_button::Entity),
    DevicesLoaded(HashMap<String, dbus_client::DeviceInfo>),
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
    ShareText(String),
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
            dbus_client: None,
            selected_device_id: None,
            transfers: HashMap::new(),
        };

        // Load devices on startup
        (
            app,
            Task::perform(fetch_devices(), |devices| {
                cosmic::Action::App(Message::DevicesLoaded(devices))
            }),
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
                tracing::info!("Send file requested for device: {}", device_id);
                // TODO: Open file picker dialog
                Task::none()
            }
            Message::ShareText(device_id) => {
                tracing::info!("Share text requested for device: {}", device_id);
                // TODO: Open text input dialog
                Task::none()
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

    fn view(&self) -> Element<Self::Message> {
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
    fn devices_view(&self) -> Element<Message> {
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

        let device_icon = format!("{}-symbolic", device.device_type);
        let device_id_for_click = device.id.clone();

        widget::button::custom(
            widget::container(
                column![row![
                    widget::icon::from_name(device_icon.as_str()).size(32),
                    column![
                        widget::text(&device.name).size(16),
                        widget::text(status).size(12),
                    ]
                    .spacing(4),
                    widget::horizontal_space(),
                    pair_button,
                ]
                .spacing(12)
                .align_y(Alignment::Center),]
                .padding(16)
            )
            .style(|_theme| cosmic::iced::widget::container::Style {
                background: Some(cosmic::iced::Background::Color(Color::from_rgb(
                    0.1, 0.1, 0.1
                ))),
                border: cosmic::iced::Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
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

        let device_icon = format!("{}-symbolic", device.device_type);

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

        // Device info section
        let device_info = widget::container(
            column![
                row![
                    widget::icon::from_name(device_icon.as_str()).size(64),
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
        .style(|_theme| cosmic::iced::widget::container::Style {
            background: Some(cosmic::iced::Background::Color(Color::from_rgb(
                0.1, 0.1, 0.1
            ))),
            border: cosmic::iced::Border {
                radius: 8.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

        // Device details section
        let mut details_col = column![
            widget::text("Device Information").size(18),
            widget::divider::horizontal::default(),
        ]
        .spacing(8);

        details_col = details_col.push(
            row![
                widget::text("Type:").size(14),
                widget::horizontal_space(),
                widget::text(&device.device_type).size(14),
            ]
            .spacing(8),
        );

        details_col = details_col.push(
            row![
                widget::text("ID:").size(14),
                widget::horizontal_space(),
                widget::text(&device.id).size(12),
            ]
            .spacing(8),
        );

        details_col = details_col.push(
            row![
                widget::text("Status:").size(14),
                widget::horizontal_space(),
                widget::text(if device.is_connected {
                    "Online"
                } else {
                    "Offline"
                })
                .size(14),
            ]
            .spacing(8),
        );

        details_col = details_col.push(
            row![
                widget::text("Paired:").size(14),
                widget::horizontal_space(),
                widget::text(if device.is_paired { "Yes" } else { "No" }).size(14),
            ]
            .spacing(8),
        );

        details_col = details_col.push(
            row![
                widget::text("Reachable:").size(14),
                widget::horizontal_space(),
                widget::text(if device.is_reachable { "Yes" } else { "No" }).size(14),
            ]
            .spacing(8),
        );

        let details = widget::container(details_col.padding(16))
            .style(|_theme| cosmic::iced::widget::container::Style {
                background: Some(cosmic::iced::Background::Color(Color::from_rgb(
                    0.1, 0.1, 0.1
                ))),
                border: cosmic::iced::Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });

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
            .style(|_theme| cosmic::iced::widget::container::Style {
                background: Some(cosmic::iced::Background::Color(Color::from_rgb(
                    0.1, 0.1, 0.1
                ))),
                border: cosmic::iced::Border {
                    radius: 8.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
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
    fn transfers_view(&self) -> Element<Message> {
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
    fn settings_view(&self) -> Element<Message> {
        widget::container(
            column![
                widget::text::title3("Settings"),
                widget::text("Global settings and preferences"),
            ]
            .spacing(12)
            .padding(24)
        )
        .width(Length::Fill)
        .height(Length::Fill)
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
