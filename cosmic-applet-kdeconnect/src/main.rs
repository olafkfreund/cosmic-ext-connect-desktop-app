mod dbus_client;

use cosmic::iced::widget::{column, container, row, scrollable, text};
use cosmic::{
    app::{Core, Task},
    iced::{alignment::Horizontal, window, Length, Padding, Rectangle},
    iced_runtime::core::layout::Limits,
    surface::action::{app_popup, destroy_popup},
    widget::{button, divider, icon},
    Element,
};
use kdeconnect_protocol::{ConnectionState, Device, DeviceInfo, DeviceType, PairingStatus};

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt::init();
    cosmic::applet::run::<KdeConnectApplet>(())
}

#[derive(Debug, Clone)]
struct DeviceState {
    device: Device,
    battery_level: Option<u8>,
}

struct KdeConnectApplet {
    core: Core,
    popup: Option<window::Id>,
    devices: Vec<DeviceState>,
}

#[derive(Debug, Clone)]
enum Message {
    PopupClosed(window::Id),
    PairDevice(String),
    UnpairDevice(String),
    RefreshDevices,
    Surface(cosmic::surface::Action),
}

impl cosmic::Application for KdeConnectApplet {
    type Message = Message;
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    const APP_ID: &'static str = "com.system76.CosmicAppletKdeConnect";

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        // Mock devices for UI development
        let mock_devices = vec![
            DeviceState {
                device: Device {
                    info: DeviceInfo::new("My Phone", DeviceType::Phone, 1716),
                    connection_state: ConnectionState::Connected,
                    pairing_status: PairingStatus::Paired,
                    is_trusted: true,
                    last_seen: 0,
                    last_connected: Some(0),
                    host: Some("192.168.1.100".to_string()),
                    port: Some(1716),
                    certificate_fingerprint: None,
                    certificate_data: None,
                },
                battery_level: Some(85),
            },
            DeviceState {
                device: Device {
                    info: DeviceInfo::new("My Tablet", DeviceType::Tablet, 1716),
                    connection_state: ConnectionState::Disconnected,
                    pairing_status: PairingStatus::Paired,
                    is_trusted: true,
                    last_seen: 0,
                    last_connected: None,
                    host: None,
                    port: None,
                    certificate_fingerprint: None,
                    certificate_data: None,
                },
                battery_level: None,
            },
            DeviceState {
                device: Device {
                    info: DeviceInfo::new("Unknown Device", DeviceType::Desktop, 1716),
                    connection_state: ConnectionState::Disconnected,
                    pairing_status: PairingStatus::Unpaired,
                    is_trusted: false,
                    last_seen: 0,
                    last_connected: None,
                    host: None,
                    port: None,
                    certificate_fingerprint: None,
                    certificate_data: None,
                },
                battery_level: None,
            },
        ];

        (
            Self {
                core,
                popup: None,
                devices: mock_devices,
            },
            Task::none(),
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
            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                }
            }
            Message::PairDevice(device_id) => {
                tracing::info!("Pairing device: {}", device_id);
                // TODO: Connect to daemon and initiate pairing
                if let Some(device_state) = self
                    .devices
                    .iter_mut()
                    .find(|d| d.device.info.device_id == device_id)
                {
                    device_state.device.pairing_status = PairingStatus::RequestedByPeer;
                }
            }
            Message::UnpairDevice(device_id) => {
                tracing::info!("Unpairing device: {}", device_id);
                // TODO: Connect to daemon and unpair
                if let Some(device_state) = self
                    .devices
                    .iter_mut()
                    .find(|d| d.device.info.device_id == device_id)
                {
                    device_state.device.pairing_status = PairingStatus::Unpaired;
                    device_state.device.is_trusted = false;
                }
            }
            Message::RefreshDevices => {
                tracing::info!("Refreshing device list");
                // TODO: Connect to daemon and fetch device list
            }
            Message::Surface(action) => {
                return cosmic::task::message(cosmic::Action::Cosmic(
                    cosmic::app::Action::Surface(action),
                ));
            }
        }

        Task::none()
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
                .padding(Padding {
                    top: 8.0,
                    bottom: 8.0,
                    left: 12.0,
                    right: 12.0,
                })
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

        // Device icon based on type
        let device_icon = match device.info.device_type {
            DeviceType::Phone => "phone-symbolic",
            DeviceType::Tablet => "tablet-symbolic",
            DeviceType::Desktop => "computer-symbolic",
            DeviceType::Laptop => "laptop-symbolic",
            DeviceType::Tv => "tv-symbolic",
            _ => "question-symbolic",
        };

        // Status indicator icon
        let status_icon = match (device.connection_state, device.pairing_status) {
            (ConnectionState::Connected, PairingStatus::Paired) => "emblem-ok-symbolic",
            (_, PairingStatus::Paired) => "emblem-default-symbolic",
            (_, PairingStatus::RequestedByPeer | PairingStatus::Requested) => {
                "emblem-synchronizing-symbolic"
            }
            _ => "dialog-question-symbolic",
        };

        // Device name and status text
        let mut info_col = column![text(&device.info.device_name).size(14),].spacing(2);

        // Add connection status
        let status_text = match device.connection_state {
            ConnectionState::Connected => "Connected",
            ConnectionState::Connecting => "Connecting...",
            ConnectionState::Disconnected => {
                if device.pairing_status == PairingStatus::Paired {
                    "Disconnected"
                } else {
                    "Not paired"
                }
            }
            ConnectionState::Failed => "Connection failed",
        };
        info_col = info_col.push(text(status_text).size(11));

        // Add battery level if available
        if let Some(battery) = device_state.battery_level {
            let battery_text = format!("Battery: {}%", battery);
            info_col = info_col.push(text(battery_text).size(11));
        }

        // Action button
        let action_button = if device.pairing_status == PairingStatus::Paired {
            button::text("Unpair")
                .on_press(Message::UnpairDevice(device.info.device_id.clone()))
                .padding(4)
        } else {
            button::text("Pair")
                .on_press(Message::PairDevice(device.info.device_id.clone()))
                .padding(4)
        };

        let content = row![
            container(icon::from_name(device_icon).size(24))
                .width(Length::Fixed(40.0))
                .padding(8),
            container(icon::from_name(status_icon).size(12)).width(Length::Fixed(20.0)),
            container(info_col)
                .width(Length::Fill)
                .align_x(Horizontal::Left),
            action_button,
        ]
        .spacing(8)
        .padding(Padding {
            top: 8.0,
            bottom: 8.0,
            left: 12.0,
            right: 12.0,
        })
        .align_y(cosmic::iced::Alignment::Center)
        .width(Length::Fill);

        container(content).width(Length::Fill).into()
    }
}
