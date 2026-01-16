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
use cosmic_connect_protocol::{ConnectionState, Device, DeviceInfo as ProtocolDeviceInfo, DeviceType, PairingStatus};

use dbus_client::DbusClient;

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
    mpris_players: Vec<String>,
    selected_player: Option<String>,
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
    FileSelected(String, String), // device_id, file_path
    FindPhone(String),
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

/// Fetches battery status for a list of device IDs
async fn fetch_battery_statuses(device_ids: Vec<String>) -> HashMap<String, dbus_client::BatteryStatus> {
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
        last_connected: if info.is_connected { Some(info.last_seen as u64) } else { None },
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

impl cosmic::Application for KdeConnectApplet {
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
            Message::DeviceListUpdated(devices) => {
                tracing::info!("Device list updated: {} devices", devices.len());

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

                tracing::debug!("Fetching battery status for {} connected devices", connected_ids.len());
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
                Task::perform(open_file_picker(device_id), |result| {
                    match result {
                        Some((device_id, path)) => {
                            cosmic::Action::App(Message::FileSelected(device_id, path))
                        }
                        None => {
                            tracing::debug!("File picker cancelled or no file selected");
                            cosmic::Action::App(Message::RefreshDevices)
                        }
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
                self.selected_player = Some(player);
                Task::none()
            }
            Message::MprisControl(player, action) => {
                tracing::info!("MPRIS control: {} on {}", action, player);
                Task::perform(
                    async move {
                        if let Ok((client, _)) = DbusClient::connect().await {
                            if let Err(e) = client.mpris_control(&player, &action).await {
                                tracing::error!("Failed to control MPRIS player: {}", e);
                            }
                        }
                    },
                    |_| cosmic::Action::None,
                )
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
            Message::Surface(action) => {
                cosmic::task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(action)))
            }
        }
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

    fn view_window(&self, _id: window::Id) -> Element<'_, Self::Message> {
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

        // MPRIS media controls section
        let mpris_section = self.mpris_controls_view();

        let content = if self.devices.is_empty() {
            column![
                text("No devices found").size(14),
                text("Make sure KDE Connect is installed on your devices").size(12),
            ]
            .spacing(4)
            .padding(16)
            .width(Length::Fill)
        } else {
            // Group devices by category
            let mut connected = Vec::new();
            let mut available = Vec::new();
            let mut offline = Vec::new();

            for device_state in &self.devices {
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
                    container(text("Connected").size(12))
                        .padding(Padding::from([8.0, 12.0, 4.0, 12.0]))
                        .width(Length::Fill)
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
                    container(text("Available").size(12))
                        .padding(Padding::from([8.0, 12.0, 4.0, 12.0]))
                        .width(Length::Fill)
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
                    container(text("Offline").size(12))
                        .padding(Padding::from([8.0, 12.0, 4.0, 12.0]))
                        .width(Length::Fill)
                );
                for device_state in &offline {
                    device_groups = device_groups.push(self.device_row(device_state));
                }
            }

            device_groups
        };

        let popup_content = column![
            container(header)
                .padding(Padding::from([8.0, 12.0]))
                .width(Length::Fill),
            divider::horizontal::default(),
            mpris_section,
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
            text(selected_player).size(13),
        ]
        .spacing(6)
        .align_y(cosmic::iced::Alignment::Center);

        // Playback controls
        let controls = row![
            button::icon(icon::from_name("media-skip-backward-symbolic").size(16))
                .on_press(Message::MprisControl(
                    selected_player.clone(),
                    "Previous".to_string()
                ))
                .padding(6),
            button::icon(icon::from_name("media-playback-start-symbolic").size(16))
                .on_press(Message::MprisControl(
                    selected_player.clone(),
                    "PlayPause".to_string()
                ))
                .padding(6),
            button::icon(icon::from_name("media-playback-stop-symbolic").size(16))
                .on_press(Message::MprisControl(selected_player.clone(), "Stop".to_string()))
                .padding(6),
            button::icon(icon::from_name("media-skip-forward-symbolic").size(16))
                .on_press(Message::MprisControl(selected_player.clone(), "Next".to_string()))
                .padding(6),
        ]
        .spacing(4)
        .align_y(cosmic::iced::Alignment::Center);

        let content = column![player_name, controls]
            .spacing(8)
            .padding(Padding::from([8.0, 12.0]));

        container(content).width(Length::Fill).into()
    }

    fn device_row<'a>(&self, device_state: &'a DeviceState) -> Element<'a, Message> {
        let device = &device_state.device;
        let device_id = &device.info.device_id;

        let device_icon = device_type_icon(device.info.device_type);
        let status_icon = connection_status_icon(device.connection_state, device.pairing_status);
        let status_text = connection_status_text(device.connection_state, device.pairing_status);
        let quality_icon = connection_quality_icon(device.connection_state);

        // Device name and status column with last seen for disconnected devices
        let mut name_status_col = column![
            text(&device.info.device_name).size(14),
            text(status_text).size(11),
        ]
        .spacing(2);

        // Add last seen timestamp for disconnected devices
        if !device.is_connected() && device.last_seen > 0 {
            let last_seen_text = format_last_seen(device.last_seen);
            name_status_col = name_status_col.push(
                text(format!("Last seen: {}", last_seen_text))
                    .size(10)
            );
        }

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

        // Main device row layout with connection quality indicator
        let content = column![
            row![
                container(icon::from_name(device_icon).size(28))
                    .width(Length::Fixed(44.0))
                    .padding(8),
                container(
                    column![
                        icon::from_name(status_icon).size(14),
                        icon::from_name(quality_icon).size(12),
                    ]
                    .spacing(2)
                    .align_x(Horizontal::Center)
                )
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
    if count == 1 { "" } else { "s" }
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
