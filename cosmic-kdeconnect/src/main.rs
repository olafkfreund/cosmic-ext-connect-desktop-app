mod dbus_client;

use cosmic::app::{Core, Settings, Task};
use cosmic::iced::{
    widget::{column, row},
    Alignment, Color, Length,
};
use cosmic::widget::{self, nav_bar};
use cosmic::{Application, Element};
use std::collections::HashMap;

use dbus_client::DbusClient;

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
}

#[derive(Debug, Clone)]
enum Message {
    PageSelected(widget::segmented_button::Entity),
    DevicesLoaded(HashMap<String, dbus_client::DeviceInfo>),
    RefreshDevices,
    PairDevice(String),
    UnpairDevice(String),
    SelectDevice(String),
    BackToDeviceList,
    SendPing(String),
    FindPhone(String),
    SendFile(String),
    ShareText(String),
}

impl Application for KdeConnectApp {
    type Message = Message;
    type Executor = cosmic::executor::multi::Executor;
    type Flags = ();
    const APP_ID: &'static str = "com.system76.CosmicKdeConnect";

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
        }
    }

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
        let status = if device.is_connected {
            "Connected"
        } else if device.is_paired {
            "Paired"
        } else {
            "Available"
        };

        let pair_button = if device.is_paired {
            widget::button::standard("Unpair")
                .on_press(Message::UnpairDevice(device.id.clone()))
        } else {
            widget::button::suggested("Pair")
                .on_press(Message::PairDevice(device.id.clone()))
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
        widget::container(
            column![
                widget::text::title3("File Transfers"),
                widget::text("Transfer progress tracking will be displayed here"),
            ]
            .spacing(12)
            .padding(24)
        )
        .width(Length::Fill)
        .height(Length::Fill)
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
