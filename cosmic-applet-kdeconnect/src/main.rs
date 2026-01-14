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
use kdeconnect_protocol::{ConnectionState, Device, DeviceType, PairingStatus};

use dbus_client::DbusClient;

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt::init();
    cosmic::applet::run::<KdeConnectApplet>(())
}

#[derive(Debug, Clone)]
struct DeviceState {
    device: Device,
    battery_level: Option<u8>,
    is_charging: bool,
}

struct KdeConnectApplet {
    core: Core,
    popup: Option<window::Id>,
    devices: Vec<DeviceState>,
    dbus_client: Option<DbusClient>,
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
    FindPhone(String),
    Surface(cosmic::surface::Action),
    // Daemon responses
    DeviceListUpdated(HashMap<String, dbus_client::DeviceInfo>),
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

impl cosmic::Application for KdeConnectApplet {
    type Message = Message;
    type Executor = cosmic::executor::multi::Executor;
    type Flags = ();
    const APP_ID: &'static str = "com.system76.CosmicAppletKdeConnect";

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        let app = Self {
            core,
            popup: None,
            devices: Vec::new(),
            dbus_client: None,
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
                tracing::info!("Popup opened, fetching devices");
                fetch_devices_task()
            }
            Message::DeviceListUpdated(devices) => {
                tracing::info!("Device list updated: {} devices", devices.len());
                // TODO: Convert dbus_client::DeviceInfo to our DeviceState
                self.devices.clear();
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
                // TODO: Open file picker and send file via daemon
                tracing::warn!("File sending not yet implemented");
                Task::none()
            }
            Message::FindPhone(device_id) => {
                tracing::info!("Finding phone: {}", device_id);
                // TODO: Implement find phone via daemon
                tracing::warn!("Find phone not yet implemented in daemon");
                Task::none()
            }
            Message::Surface(action) => {
                cosmic::task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(action)))
            }
        }
    }

    fn view(&self) -> Element<Self::Message> {
        let have_popup = self.popup;

        let btn = self
            .core
            .applet
            .icon_button("phone-symbolic")
            .on_press_with_rectangle(move |offset, bounds| {
                if let Some(id) = have_popup {
                    Message::Surface(destroy_popup(id))
                } else {
                    Message::Surface(app_popup::<KdeConnectApplet>(
                        move |state: &mut KdeConnectApplet| {
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
                        Some(Box::new(|state: &KdeConnectApplet| {
                            let content = state.popup_view();
                            Element::from(state.core.applet.popup_container(content))
                                .map(cosmic::Action::App)
                        })),
                    ))
                }
            });

        Element::from(self.core.applet.applet_tooltip::<Message>(
            btn,
            "KDE Connect",
            self.popup.is_some(),
            |a| Message::Surface(a),
            None,
        ))
    }

    fn view_window(&self, _id: window::Id) -> Element<Self::Message> {
        text("KDE Connect").into()
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}

impl KdeConnectApplet {
    fn popup_view(&self) -> Element<'_, Message> {
        let header = row![
            text("KDE Connect").size(18),
            button::icon(icon::from_name("view-refresh-symbolic"))
                .on_press(Message::RefreshDevices)
                .padding(4)
        ]
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Center)
        .width(Length::Fill);

        let content = if self.devices.is_empty() {
            column![
                text("No devices found").size(14),
                text("Make sure KDE Connect is installed on your devices").size(12),
            ]
            .spacing(4)
            .padding(16)
            .width(Length::Fill)
        } else {
            let device_list: Vec<Element<'_, Message>> = self
                .devices
                .iter()
                .map(|device_state| self.device_row(device_state))
                .collect();

            column(device_list).spacing(0).width(Length::Fill)
        };

        let popup_content = column![
            container(header)
                .padding(Padding::from([8.0, 12.0]))
                .width(Length::Fill),
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

    fn device_row<'a>(&self, device_state: &'a DeviceState) -> Element<'a, Message> {
        let device = &device_state.device;
        let device_id = &device.info.device_id;

        let device_icon = device_type_icon(device.info.device_type);
        let status_icon = connection_status_icon(device.connection_state, device.pairing_status);
        let status_text = connection_status_text(device.connection_state, device.pairing_status);

        // Device name and status column
        let name_status_col = column![
            text(&device.info.device_name).size(14),
            text(status_text).size(11),
        ]
        .spacing(2);

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

        // Main device row layout
        let content = column![
            row![
                container(icon::from_name(device_icon).size(28))
                    .width(Length::Fixed(44.0))
                    .padding(8),
                container(icon::from_name(status_icon).size(14))
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
                ));

            if matches!(device.info.device_type, DeviceType::Phone) {
                actions = actions.push(action_button(
                    "find-location-symbolic",
                    Message::FindPhone(device_id.to_string()),
                ));
            }
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
