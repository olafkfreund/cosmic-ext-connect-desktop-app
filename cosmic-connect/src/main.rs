mod dbus_client;
mod notifications;
mod settings;

use cosmic::app::{Core, Settings, Task};
use cosmic::iced::{
    widget::{column, row},
    Alignment, Color, Length,
};
use cosmic::widget::{self, nav_bar};
use cosmic::{theme, Application, Element};
use std::collections::HashMap;

use dbus_client::{DaemonEvent, DbusClient};

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt::init();
    cosmic::app::run::<CConnectApp>(Settings::default(), ())
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
struct CConnectApp {
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

    // Settings state
    daemon_config: Option<settings::DaemonConfig>,
    settings_loading: bool,
    settings_error: Option<String>,
    pending_device_name: String,
    show_restart_required: bool,
}

#[derive(Debug, Clone)]
enum Message {
    PageSelected(widget::segmented_button::Entity),
    SetPage(Page), // Direct page navigation (for keyboard shortcuts)
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
    TextInputOpened(String),       // device_id for text sharing
    TextSubmitted(String, String), // device_id, text
    // Quick actions
    QuickSendFile(String),      // device_id
    QuickNotification(String),  // device_id
    QuickScreenshot(String),    // device_id (Desktop-to-Desktop plugin)
    QuickSystemMonitor(String), // device_id (Desktop-to-Desktop plugin)
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
    CancelTransfer(String), // transfer_id
    // Settings messages
    SettingsLoaded(Result<settings::DaemonConfig, String>),
    RefreshSettings,
    DeviceNameChanged(String), // Text input change
    SetDeviceName(String),
    SetDeviceType(String),
    SetGlobalPluginEnabled(String, bool), // plugin_name, enabled
    SetTcpEnabled(bool),
    SetBluetoothEnabled(bool),
    SetTransportPreference(String),
    SetAutoFallback(bool),
    SetDiscoveryInterval(u64),
    SetDeviceTimeout(u64),
    ResetConfigToDefaults,
    RestartDaemon,
    SettingsUpdateResult(Result<(), String>),
}

impl Application for CConnectApp {
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

            // Settings state
            daemon_config: None,
            settings_loading: false,
            settings_error: None,
            pending_device_name: String::new(),
            show_restart_required: false,
        };

        // Load devices, MPRIS players, and settings on startup
        (
            app,
            Task::batch(vec![
                Task::perform(fetch_devices(), |devices| {
                    cosmic::Action::App(Message::DevicesLoaded(devices))
                }),
                Task::perform(fetch_mpris_players(), |players| {
                    cosmic::Action::App(Message::MprisPlayersUpdated(players))
                }),
                Task::perform(fetch_daemon_config(), |result| {
                    cosmic::Action::App(Message::SettingsLoaded(result))
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
            Message::SetPage(page) => {
                self.current_page = page;
                // Also update the nav model to reflect the change
                self.nav_model.activate_position(page as u16);
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
                    Task::perform(fetch_battery_statuses(connected_device_ids), |statuses| {
                        cosmic::Action::App(Message::BatteryStatusesUpdated(statuses))
                    })
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
                Task::perform(open_file_picker(device_id), |result| match result {
                    Some((device_id, path)) => {
                        cosmic::Action::App(Message::FileSelected(device_id, path))
                    }
                    None => {
                        tracing::debug!("File picker cancelled");
                        cosmic::Action::App(Message::RefreshDevices)
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
            // Quick actions handlers
            Message::QuickSendFile(device_id) => {
                tracing::info!("Quick send file to device: {}", device_id);
                Task::perform(open_file_picker(device_id), |result| match result {
                    Some((device_id, path)) => {
                        cosmic::Action::App(Message::FileSelected(device_id, path))
                    }
                    None => {
                        tracing::debug!("File picker cancelled");
                        cosmic::Action::App(Message::RefreshDevices)
                    }
                })
            }
            Message::QuickNotification(device_id) => {
                tracing::info!("Quick send notification to device: {}", device_id);
                // Send a simple test notification
                Task::perform(
                    async move {
                        send_notification(
                            device_id,
                            "Quick Message".to_string(),
                            "Sent from COSMIC Connect".to_string(),
                        )
                        .await
                    },
                    |result| {
                        if let Err(e) = result {
                            tracing::error!("Failed to send notification: {}", e);
                        }
                        cosmic::Action::App(Message::RefreshDevices)
                    },
                )
            }
            Message::QuickScreenshot(device_id) => {
                tracing::info!("Quick screenshot request for device: {}", device_id);
                // TODO: Implement screenshot plugin when Desktop-to-Desktop plugins are ready (Issue #67)
                tracing::warn!("Screenshot plugin not yet implemented");
                Task::none()
            }
            Message::QuickSystemMonitor(device_id) => {
                tracing::info!("Quick system monitor request for device: {}", device_id);
                // TODO: Implement system monitor plugin when Desktop-to-Desktop plugins are ready (Issue #66)
                tracing::warn!("System monitor plugin not yet implemented");
                Task::none()
            }
            Message::CancelTransfer(transfer_id) => {
                tracing::info!("Cancelling transfer: {}", transfer_id);

                // Update local state immediately for UI responsiveness
                if let Some(transfer) = self.transfers.get_mut(&transfer_id) {
                    transfer.status = TransferStatus::Cancelled;
                    transfer.error_message = Some("Cancelled by user".to_string());
                }

                // Call daemon to cancel the transfer
                Task::perform(
                    async move {
                        if let Ok((client, _)) = crate::dbus_client::DbusClient::connect().await {
                            client.cancel_transfer(&transfer_id).await
                        } else {
                            Err(anyhow::anyhow!("Failed to connect to daemon"))
                        }
                    },
                    |result| {
                        if let Err(e) = result {
                            tracing::error!("Failed to cancel transfer: {}", e);
                        }
                        cosmic::Action::None
                    },
                )
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
            // Settings messages
            Message::SettingsLoaded(result) => {
                self.settings_loading = false;
                match result {
                    Ok(config) => {
                        tracing::info!("Settings loaded successfully");
                        self.pending_device_name = config.device.name.clone();
                        self.daemon_config = Some(config);
                        self.settings_error = None;
                    }
                    Err(e) => {
                        tracing::error!("Failed to load settings: {}", e);
                        self.settings_error = Some(e);
                    }
                }
                Task::none()
            }
            Message::RefreshSettings => {
                tracing::info!("Refreshing settings");
                self.settings_loading = true;
                Task::perform(fetch_daemon_config(), |result| {
                    cosmic::Action::App(Message::SettingsLoaded(result))
                })
            }
            Message::DeviceNameChanged(name) => {
                self.pending_device_name = name;
                Task::none()
            }
            Message::SetDeviceName(name) => {
                tracing::info!("Setting device name to: {}", name);
                Task::perform(set_device_name(name), |result| {
                    cosmic::Action::App(Message::SettingsUpdateResult(result))
                })
            }
            Message::SetDeviceType(device_type) => {
                tracing::info!("Setting device type to: {}", device_type);
                Task::perform(set_device_type(device_type), |result| {
                    cosmic::Action::App(Message::SettingsUpdateResult(result))
                })
            }
            Message::SetGlobalPluginEnabled(plugin, enabled) => {
                tracing::info!(
                    "Setting plugin {} to {}",
                    plugin,
                    if enabled { "enabled" } else { "disabled" }
                );

                // Update local state immediately for responsiveness
                if let Some(config) = &mut self.daemon_config {
                    config.plugins.set(&plugin, enabled);
                }

                Task::perform(set_global_plugin_enabled(plugin, enabled), |result| {
                    cosmic::Action::App(Message::SettingsUpdateResult(result))
                })
            }
            Message::SetTcpEnabled(enabled) => {
                tracing::info!(
                    "Setting TCP transport to {}",
                    if enabled { "enabled" } else { "disabled" }
                );

                // Update local state and show restart banner
                if let Some(config) = &mut self.daemon_config {
                    config.transport.enable_tcp = enabled;
                }
                self.show_restart_required = true;

                Task::perform(set_tcp_enabled(enabled), |result| {
                    cosmic::Action::App(Message::SettingsUpdateResult(result))
                })
            }
            Message::SetBluetoothEnabled(enabled) => {
                tracing::info!(
                    "Setting Bluetooth transport to {}",
                    if enabled { "enabled" } else { "disabled" }
                );

                // Update local state and show restart banner
                if let Some(config) = &mut self.daemon_config {
                    config.transport.enable_bluetooth = enabled;
                }
                self.show_restart_required = true;

                Task::perform(set_bluetooth_enabled(enabled), |result| {
                    cosmic::Action::App(Message::SettingsUpdateResult(result))
                })
            }
            Message::SetTransportPreference(preference) => {
                tracing::info!("Setting transport preference to: {}", preference);

                // Update local state and show restart banner
                if let Some(config) = &mut self.daemon_config {
                    if let Some(pref) = settings::TransportPreference::from_str(&preference) {
                        config.transport.preference = pref;
                    }
                }
                self.show_restart_required = true;

                Task::perform(set_transport_preference(preference), |result| {
                    cosmic::Action::App(Message::SettingsUpdateResult(result))
                })
            }
            Message::SetAutoFallback(enabled) => {
                tracing::info!(
                    "Setting auto fallback to {}",
                    if enabled { "enabled" } else { "disabled" }
                );

                // Update local state and show restart banner
                if let Some(config) = &mut self.daemon_config {
                    config.transport.auto_fallback = enabled;
                }
                self.show_restart_required = true;

                Task::perform(set_auto_fallback(enabled), |result| {
                    cosmic::Action::App(Message::SettingsUpdateResult(result))
                })
            }
            Message::SetDiscoveryInterval(interval_secs) => {
                tracing::info!("Setting discovery interval to {} seconds", interval_secs);

                // Update local state and show restart banner
                if let Some(config) = &mut self.daemon_config {
                    config.discovery.broadcast_interval_secs = interval_secs;
                }
                self.show_restart_required = true;

                Task::perform(set_discovery_interval(interval_secs), |result| {
                    cosmic::Action::App(Message::SettingsUpdateResult(result))
                })
            }
            Message::SetDeviceTimeout(timeout_secs) => {
                tracing::info!("Setting device timeout to {} seconds", timeout_secs);

                // Update local state and show restart banner
                if let Some(config) = &mut self.daemon_config {
                    config.discovery.device_timeout_secs = timeout_secs;
                }
                self.show_restart_required = true;

                Task::perform(set_device_timeout(timeout_secs), |result| {
                    cosmic::Action::App(Message::SettingsUpdateResult(result))
                })
            }
            Message::ResetConfigToDefaults => {
                tracing::warn!("Resetting configuration to defaults");
                self.show_restart_required = true;

                Task::perform(reset_config_to_defaults(), |result| {
                    cosmic::Action::App(Message::SettingsUpdateResult(result))
                })
            }
            Message::RestartDaemon => {
                tracing::warn!("Restarting daemon");
                self.show_restart_required = false;

                Task::perform(restart_daemon(), |result| {
                    cosmic::Action::App(Message::SettingsUpdateResult(result))
                })
            }
            Message::SettingsUpdateResult(result) => {
                match result {
                    Ok(()) => {
                        tracing::info!("Settings updated successfully");
                        // Refresh settings to get updated values from daemon
                        Task::perform(fetch_daemon_config(), |result| {
                            cosmic::Action::App(Message::SettingsLoaded(result))
                        })
                    }
                    Err(e) => {
                        tracing::error!("Failed to update settings: {}", e);
                        self.settings_error = Some(e);
                        Task::none()
                    }
                }
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
                        filename,
                        success,
                        error_message,
                        ..
                    } => {
                        // Show notification
                        let error = if error_message.is_empty() {
                            None
                        } else {
                            Some(error_message.as_str())
                        };
                        notifications::notify_transfer_complete(&filename, success, error);

                        // Update transfer state
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
                    DaemonEvent::DeviceAdded {
                        device_id,
                        device_info,
                    } => {
                        tracing::info!("Device added: {}", device_id);

                        // Show notification
                        notifications::notify_device_discovered(&device_info.name);

                        self.devices.insert(device_id, device_info);
                    }
                    DaemonEvent::DeviceRemoved { device_id } => {
                        tracing::info!("Device removed: {}", device_id);

                        // Show notification
                        if let Some(device) = self.devices.get(&device_id) {
                            notifications::notify_device_disconnected(&device.name);
                        }

                        self.devices.remove(&device_id);
                    }
                    DaemonEvent::DeviceStateChanged { device_id, state } => {
                        tracing::info!("Device {} state changed to: {}", device_id, state);
                        // Refresh devices to get updated state
                        return Task::perform(fetch_devices(), |devices| {
                            cosmic::Action::App(Message::DevicesLoaded(devices))
                        });
                    }
                    DaemonEvent::PairingRequest { device_id } => {
                        tracing::info!("Pairing request from device: {}", device_id);

                        // Show notification
                        if let Some(device) = self.devices.get(&device_id) {
                            notifications::notify_pairing_request(&device.name);
                        } else {
                            notifications::notify_pairing_request("Unknown Device");
                        }
                    }
                    DaemonEvent::PairingStatusChanged { device_id, status } => {
                        tracing::info!(
                            "Device {} pairing status changed to: {}",
                            device_id,
                            status
                        );

                        // Show notification for successful pairing
                        if status.to_lowercase().contains("paired")
                            || status.to_lowercase().contains("success")
                        {
                            if let Some(device) = self.devices.get(&device_id) {
                                notifications::notify_pairing_success(&device.name);
                            }
                        } else if status.to_lowercase().contains("failed")
                            || status.to_lowercase().contains("rejected")
                        {
                            let device_name = self
                                .devices
                                .get(&device_id)
                                .map(|d| d.name.as_str())
                                .unwrap_or("Unknown Device");
                            notifications::notify_pairing_failed(device_name, Some(&status));
                        }

                        // Refresh devices to get updated pairing state
                        return Task::perform(fetch_devices(), |devices| {
                            cosmic::Action::App(Message::DevicesLoaded(devices))
                        });
                    }
                    DaemonEvent::DaemonReconnected => {
                        tracing::info!("Daemon reconnected");
                        notifications::notify_daemon_reconnected();

                        // Refresh devices after reconnection
                        return Task::perform(fetch_devices(), |devices| {
                            cosmic::Action::App(Message::DevicesLoaded(devices))
                        });
                    }
                    DaemonEvent::PluginEvent {
                        device_id,
                        plugin,
                        data,
                    } => {
                        tracing::debug!("Plugin event from {}: {} - {}", device_id, plugin, data);

                        // Show notification for ping events
                        if plugin == "ping" {
                            if let Some(device) = self.devices.get(&device_id) {
                                // Try to parse message from data
                                let message = if let Ok(json) =
                                    serde_json::from_str::<serde_json::Value>(&data)
                                {
                                    json.get("message")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string())
                                } else {
                                    None
                                };
                                notifications::notify_ping_received(
                                    &device.name,
                                    message.as_deref(),
                                );
                            }
                        }
                    }
                    DaemonEvent::DaemonDisconnected => {
                        tracing::warn!("Daemon disconnected");
                        // Don't show notification for disconnect as it might be noisy during restarts
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

    fn subscription(&self) -> cosmic::iced::Subscription<Self::Message> {
        use cosmic::iced::event;
        use cosmic::iced::keyboard::{self, Key};

        let keyboard_sub = event::listen_with(|event, _status, _id| {
            match event {
                event::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                    // Navigation shortcuts (Alt+1, Alt+2, Alt+3)
                    if modifiers.alt() && !modifiers.control() && !modifiers.shift() {
                        match key.as_ref() {
                            Key::Character("1") => return Some(Message::SetPage(Page::Devices)),
                            Key::Character("2") => return Some(Message::SetPage(Page::Transfers)),
                            Key::Character("3") => return Some(Message::SetPage(Page::Settings)),
                            _ => {}
                        }
                    }

                    // Action shortcuts (Ctrl+...)
                    if modifiers.control() && !modifiers.alt() && !modifiers.shift() {
                        match key.as_ref() {
                            Key::Character("r") => return Some(Message::RefreshDevices),
                            Key::Character(",") => return Some(Message::SetPage(Page::Settings)),
                            _ => {}
                        }
                    }

                    None
                }
                _ => None,
            }
        });

        keyboard_sub
    }
}

impl CConnectApp {
    /// View for the Devices page
    fn devices_view(&self) -> Element<'_, Message> {
        // If a device is selected, show details view instead
        if let Some(device_id) = &self.selected_device_id {
            if let Some(device) = self.devices.get(device_id) {
                return self.device_details_view(device);
            }
        }

        // Otherwise show device list
        let theme = theme::active();
        let spacing = theme.cosmic().spacing;

        let header = row![
            widget::text::title3("Devices"),
            widget::horizontal_space(),
            widget::button::standard("Refresh").on_press(Message::RefreshDevices)
        ]
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center)
        .padding(spacing.space_l);

        let devices_list: Element<Message> = if self.devices.is_empty() {
            column![
                widget::text("No devices found"),
                widget::text("Make sure COSMIC Connect is installed on your devices").size(14),
            ]
            .spacing(spacing.space_xxs)
            .padding(spacing.space_l)
            .into()
        } else {
            let mut col = widget::column()
                .spacing(spacing.space_xs)
                .padding(spacing.space_l);
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

        let theme = theme::active();
        let spacing = theme.cosmic().spacing;
        let icon_style = device_type_style(&device.device_type, &theme);
        let device_id_for_click = device.id.clone();

        // Build name and status column with optional battery indicator
        let mut info_column = column![
            widget::text::body(&device.name),
            widget::text::caption(status),
        ]
        .spacing(spacing.space_xxs);

        // Add battery info if available
        if let Some(battery) = self.battery_statuses.get(&device.id) {
            let battery_icon = battery_icon_name(battery.level, battery.is_charging);
            let battery_row = row![
                widget::icon::from_name(battery_icon).size(14),
                widget::text::caption(format!("{}%", battery.level)),
            ]
            .spacing(spacing.space_xxs)
            .align_y(Alignment::Center);
            info_column = info_column.push(battery_row);
        }

        // Create styled device icon
        let icon = styled_device_icon(icon_style.icon_name, icon_style.color, 24, 8);

        // Quick actions (only for connected devices)
        let quick_actions: Element<Message> = if device.is_connected && device.is_paired {
            let id1 = device.id.clone();
            let id2 = device.id.clone();
            let id3 = device.id.clone();
            let id4 = device.id.clone();

            row![
                widget::button::icon(widget::icon::from_name("document-send-symbolic").size(16))
                    .on_press(Message::QuickSendFile(id1))
                    .tooltip("Send File"),
                widget::button::icon(widget::icon::from_name("mail-send-symbolic").size(16))
                    .on_press(Message::QuickNotification(id2))
                    .tooltip("Send Notification"),
                widget::button::icon(widget::icon::from_name("camera-photo-symbolic").size(16))
                    .on_press(Message::QuickScreenshot(id3))
                    .tooltip("Request Screenshot"),
                widget::button::icon(
                    widget::icon::from_name("utilities-system-monitor-symbolic").size(16)
                )
                .on_press(Message::QuickSystemMonitor(id4))
                .tooltip("View System Monitor"),
            ]
            .spacing(spacing.space_xxs)
            .align_y(Alignment::Center)
            .into()
        } else {
            widget::horizontal_space().into()
        };

        widget::button::custom(
            widget::container(
                column![row![
                    icon,
                    info_column,
                    widget::horizontal_space(),
                    quick_actions,
                    pair_button,
                ]
                .spacing(spacing.space_xs)
                .align_y(Alignment::Center),]
                .padding(spacing.space_s),
            )
            .style(card_container_style)
            .width(Length::Fill),
        )
        .on_press(Message::SelectDevice(device_id_for_click))
        .width(Length::Fill)
        .into()
    }

    /// Detailed view for a selected device
    fn device_details_view<'a>(&self, device: &'a dbus_client::DeviceInfo) -> Element<'a, Message> {
        let theme = theme::active();
        let status: (&str, Color) = if device.is_connected {
            ("Connected", theme.cosmic().palette.bright_green.into())
        } else if device.is_paired {
            (
                "Paired (Disconnected)",
                theme.cosmic().palette.neutral_6.into(),
            )
        } else {
            ("Available", theme.cosmic().palette.bright_orange.into())
        };

        let icon_style = device_type_style(&device.device_type, &theme);

        // Header with back button
        let spacing = theme.cosmic().spacing;
        let header = row![
            widget::button::icon(widget::icon::from_name("go-previous-symbolic"))
                .on_press(Message::BackToDeviceList),
            widget::horizontal_space(),
            widget::button::standard("Refresh").on_press(Message::RefreshDevices)
        ]
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center)
        .padding(spacing.space_l);

        // Styled device icon (larger for details view)
        let icon = styled_device_icon(icon_style.icon_name, icon_style.color, 48, 16);

        // Device info section
        let device_info = widget::container(
            column![
                row![icon, widget::horizontal_space(),]
                    .spacing(spacing.space_s)
                    .align_y(Alignment::Center),
                widget::text::title2(&device.name),
                widget::text::body(status.0),
            ]
            .spacing(spacing.space_xs)
            .padding(spacing.space_l),
        )
        .style(card_container_style);

        // Device details section
        let mut details_col = column![
            widget::text::title3("Device Information"),
            widget::divider::horizontal::default(),
            detail_row("Type:", &device.device_type),
            detail_row("ID:", &device.id),
            detail_row(
                "Status:",
                if device.is_connected {
                    "Online"
                } else {
                    "Offline"
                }
            ),
            detail_row("Paired:", if device.is_paired { "Yes" } else { "No" }),
            detail_row("Reachable:", if device.is_reachable { "Yes" } else { "No" }),
        ]
        .spacing(spacing.space_xxs);

        // Add battery information if available
        if let Some(battery) = self.battery_statuses.get(&device.id) {
            let battery_icon = battery_icon_name(battery.level, battery.is_charging);
            details_col = details_col.push(
                row![
                    widget::text::body("Battery:"),
                    widget::horizontal_space(),
                    row![
                        widget::icon::from_name(battery_icon).size(14),
                        widget::text::body(format!(
                            "{}%{}",
                            battery.level,
                            if battery.is_charging {
                                " (Charging)"
                            } else {
                                ""
                            }
                        )),
                    ]
                    .spacing(spacing.space_xxs)
                    .align_y(Alignment::Center),
                ]
                .spacing(spacing.space_xxs),
            );
        }

        let details =
            widget::container(details_col.padding(spacing.space_s)).style(card_container_style);

        // Actions section (if device is paired and connected)
        let device_id_for_actions = device.id.clone();
        let actions: Element<Message> = if device.is_paired && device.is_connected {
            let id1 = device_id_for_actions.clone();
            let id2 = device_id_for_actions.clone();
            let id3 = device_id_for_actions.clone();
            let id4 = device_id_for_actions.clone();

            widget::container(
                column![
                    widget::text::title3("Actions"),
                    widget::divider::horizontal::default(),
                    row![
                        widget::button::standard("Send Ping").on_press(Message::SendPing(id1)),
                        widget::button::standard("Send File").on_press(Message::SendFile(id2)),
                    ]
                    .spacing(spacing.space_xxs),
                    row![
                        widget::button::standard("Find Phone").on_press(Message::FindPhone(id3)),
                        widget::button::standard("Share Text").on_press(Message::ShareText(id4)),
                    ]
                    .spacing(spacing.space_xxs),
                ]
                .spacing(spacing.space_xs)
                .padding(spacing.space_s),
            )
            .style(card_container_style)
            .into()
        } else {
            widget::container(
                column![
                    widget::text::body("Actions unavailable"),
                    widget::text::caption("Device must be paired and connected"),
                ]
                .spacing(spacing.space_xxs)
                .padding(spacing.space_s),
            )
            .into()
        };

        // Main content
        let content = widget::scrollable(
            column![device_info, details, actions]
                .spacing(spacing.space_s)
                .padding(spacing.space_l),
        );

        column![header, widget::divider::horizontal::default(), content]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// View for the Transfers page
    fn transfers_view(&self) -> Element<'_, Message> {
        let theme = theme::active();
        let spacing = theme.cosmic().spacing;

        let header = row![
            widget::text::title3("File Transfers"),
            widget::horizontal_space(),
        ]
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center)
        .padding(spacing.space_l);

        let transfers_list: Element<Message> = if self.transfers.is_empty() {
            column![
                widget::text("No active transfers"),
                widget::text("File transfers will appear here when you send or receive files")
                    .size(14),
            ]
            .spacing(spacing.space_xxs)
            .padding(spacing.space_l)
            .into()
        } else {
            let mut col = widget::column()
                .spacing(spacing.space_xs)
                .padding(spacing.space_l);

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
                col = col.push(widget::text::title4("Active Transfers"));
                for transfer in active_transfers {
                    col = col.push(self.transfer_card(transfer));
                }
            }

            // Show completed transfers
            if !completed_transfers.is_empty() {
                col = col.push(widget::text::title4("Recent Transfers"));
                for transfer in completed_transfers {
                    col = col.push(self.transfer_card(transfer));
                }
            }

            col.into()
        };

        column![
            header,
            widget::divider::horizontal::default(),
            transfers_list
        ]
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

        let theme = theme::active();
        let status_text = match transfer.status {
            TransferStatus::Active => format!("{}%", progress_percentage),
            TransferStatus::Completed => "Completed".to_string(),
            TransferStatus::Failed => "Failed".to_string(),
            TransferStatus::Cancelled => "Cancelled".to_string(),
        };

        let status_color = match transfer.status {
            TransferStatus::Active => theme.cosmic().palette.accent_blue.into(),
            TransferStatus::Completed => theme.cosmic().palette.bright_green.into(),
            TransferStatus::Failed => theme.cosmic().palette.bright_red.into(),
            TransferStatus::Cancelled => theme.cosmic().palette.neutral_6.into(),
        };

        let spacing = theme.cosmic().spacing;

        // Cancel button for active transfers
        let cancel_button: Element<Message> = if transfer.status == TransferStatus::Active {
            widget::button::text("Cancel")
                .on_press(Message::CancelTransfer(transfer.id.clone()))
                .into()
        } else {
            widget::horizontal_space().into()
        };

        let mut content_col = column![row![
            widget::icon::from_name(direction_icon).size(24),
            column![
                widget::text::body(&transfer.filename),
                widget::text::caption(format!(
                    "{} {} {}",
                    if transfer.direction == "sending" {
                        "Sending to"
                    } else {
                        "Receiving from"
                    },
                    &transfer.device_name,
                    format_bytes(transfer.bytes_transferred)
                )),
            ]
            .spacing(spacing.space_xxs),
            widget::horizontal_space(),
            widget::text::body(status_text),
            cancel_button,
        ]
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center),]
        .spacing(spacing.space_xxs);

        // Add progress bar for active transfers
        if transfer.status == TransferStatus::Active && transfer.total_bytes > 0 {
            content_col = content_col.push(
                widget::progress_bar(0.0..=100.0, progress_percentage as f32).width(Length::Fill),
            );

            // Show transfer speed and ETA
            let speed_text = format!(
                "{} / {} ({})",
                format_bytes(transfer.bytes_transferred),
                format_bytes(transfer.total_bytes),
                format_bytes(transfer.total_bytes - transfer.bytes_transferred)
            );
            content_col = content_col.push(widget::text::caption(speed_text));
        }

        // Show error message if failed
        if let Some(error) = &transfer.error_message {
            content_col = content_col.push(widget::text::caption(format!("Error: {}", error)));
        }

        widget::container(content_col.padding(spacing.space_s))
            .style(
                move |theme: &cosmic::Theme| cosmic::iced::widget::container::Style {
                    background: Some(cosmic::iced::Background::Color(
                        theme.cosmic().palette.neutral_2.into(),
                    )),
                    border: cosmic::iced::Border {
                        color: status_color,
                        width: 2.0,
                        radius: theme.cosmic().corner_radii.radius_s.into(),
                    },
                    ..Default::default()
                },
            )
            .width(Length::Fill)
            .into()
    }

    /// View for the Settings page
    fn settings_view(&self) -> Element<'_, Message> {
        let theme = theme::active();
        let spacing = theme.cosmic().spacing;

        let header = row![widget::text::title3("Settings"), widget::horizontal_space(),]
            .spacing(spacing.space_xs)
            .align_y(Alignment::Center)
            .padding(spacing.space_l);

        // Show loading state
        if self.settings_loading {
            return column![
                header,
                widget::divider::horizontal::default(),
                widget::container(widget::text::body("Loading settings..."))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::Center)
                    .align_y(Alignment::Center)
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        }

        // Show error state
        if let Some(error) = &self.settings_error {
            return column![
                header,
                widget::divider::horizontal::default(),
                widget::container(
                    column![
                        widget::text::body(format!("Error loading settings: {}", error)),
                        widget::button::standard("Retry").on_press(Message::RefreshSettings),
                    ]
                    .spacing(spacing.space_xs)
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        }

        // Build settings sections
        let mut content_col = column![].spacing(spacing.space_s);

        // Restart required banner
        if self.show_restart_required {
            content_col = content_col.push(
                widget::container(
                    row![
                        widget::icon::from_name("dialog-warning-symbolic").size(24),
                        widget::text::body(
                            "Restart required for transport/discovery changes to take effect"
                        ),
                        widget::horizontal_space(),
                        widget::button::suggested("Restart Daemon")
                            .on_press(Message::RestartDaemon),
                    ]
                    .spacing(spacing.space_xs)
                    .align_y(Alignment::Center)
                    .padding(spacing.space_xs),
                )
                .style(warning_container_style)
                .width(Length::Fill),
            );
        }

        if let Some(config) = &self.daemon_config {
            content_col = content_col
                .push(self.general_settings_section(&theme, config))
                .push(self.connectivity_settings_section(&theme, config))
                .push(self.plugins_settings_section(&theme, config))
                .push(self.discovery_settings_section(&theme, config))
                .push(self.advanced_settings_section(&theme, config))
                .push(self.mpris_controls_section(&theme));
        }

        // Always show About section
        content_col = content_col.push(self.about_section(&theme));

        let content = widget::scrollable(content_col.padding(spacing.space_l));

        column![header, widget::divider::horizontal::default(), content]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// General settings section
    fn general_settings_section<'a>(
        &'a self,
        theme: &cosmic::Theme,
        config: &'a settings::DaemonConfig,
    ) -> Element<'a, Message> {
        let spacing = theme.cosmic().spacing;

        let mut content_col = column![
            widget::text::title4("General"),
            widget::divider::horizontal::default(),
        ]
        .spacing(spacing.space_xs);

        // Device Name
        content_col = content_col.push(
            row![
                widget::text::body("Device Name:").width(Length::Fixed(150.0)),
                widget::text_input("My Computer", &self.pending_device_name)
                    .on_input(Message::DeviceNameChanged)
                    .width(Length::Fixed(300.0)),
                widget::button::standard("Apply")
                    .on_press(Message::SetDeviceName(self.pending_device_name.clone())),
            ]
            .spacing(spacing.space_xs)
            .align_y(Alignment::Center),
        );

        // Device Type
        content_col =
            content_col.push(widget::text::body("Device Type:").width(Length::Fixed(150.0)));

        // Device type selector buttons
        let device_types = ["desktop", "laptop", "tablet"];
        let current_type = &config.device.device_type;

        let type_buttons: Vec<Element<'_, Message>> = device_types
            .iter()
            .map(|&dt| {
                let button = if dt == current_type {
                    widget::button::suggested(settings::device_type_name(dt))
                } else {
                    widget::button::standard(settings::device_type_name(dt))
                        .on_press(Message::SetDeviceType(dt.to_string()))
                };
                button.into()
            })
            .collect();

        let mut type_row = row![].spacing(spacing.space_xs);
        for button in type_buttons {
            type_row = type_row.push(button);
        }
        content_col = content_col.push(type_row);

        // Device ID (read-only)
        if let Some(device_id) = &config.device.device_id {
            content_col = content_col.push(
                row![
                    widget::text::body("Device ID:").width(Length::Fixed(150.0)),
                    widget::text::caption(device_id),
                ]
                .spacing(spacing.space_xs)
                .align_y(Alignment::Center),
            );
        }

        widget::container(content_col.padding(spacing.space_s))
            .style(card_container_style)
            .width(Length::Fill)
            .into()
    }

    /// Connectivity settings section
    fn connectivity_settings_section<'a>(
        &'a self,
        theme: &cosmic::Theme,
        config: &'a settings::DaemonConfig,
    ) -> Element<'a, Message> {
        let spacing = theme.cosmic().spacing;

        let mut content_col = column![
            widget::text::title4("Connectivity"),
            widget::divider::horizontal::default(),
        ]
        .spacing(spacing.space_xs);

        // TCP Transport
        content_col = content_col.push(
            row![
                widget::toggler(config.transport.enable_tcp)
                    .label("Enable TCP/TLS")
                    .on_toggle(Message::SetTcpEnabled)
                    .width(Length::Fixed(200.0)),
                widget::horizontal_space(),
                widget::text::caption(format!("Timeout: {}s", config.transport.tcp_timeout_secs)),
            ]
            .spacing(spacing.space_xs)
            .align_y(Alignment::Center),
        );

        // Bluetooth Transport
        content_col = content_col.push(
            row![
                widget::toggler(config.transport.enable_bluetooth)
                    .label("Enable Bluetooth")
                    .on_toggle(Message::SetBluetoothEnabled)
                    .width(Length::Fixed(200.0)),
                widget::horizontal_space(),
                widget::text::caption(format!(
                    "Timeout: {}s",
                    config.transport.bluetooth_timeout_secs
                )),
            ]
            .spacing(spacing.space_xs)
            .align_y(Alignment::Center),
        );

        // Transport Preference
        content_col = content_col.push(widget::text::body("Transport Preference:"));
        content_col = content_col.push(widget::text::body(
            config.transport.preference.display_name(),
        ));

        // Auto Fallback
        content_col = content_col.push(
            widget::toggler(config.transport.auto_fallback)
                .label("Auto Fallback (try alternative transport if connection fails)")
                .on_toggle(Message::SetAutoFallback),
        );

        content_col = content_col.push(widget::text::caption(
            "⚠ Changes to transport settings require daemon restart",
        ));

        widget::container(content_col.padding(spacing.space_s))
            .style(card_container_style)
            .width(Length::Fill)
            .into()
    }

    /// Plugins settings section
    fn plugins_settings_section<'a>(
        &'a self,
        theme: &cosmic::Theme,
        config: &'a settings::DaemonConfig,
    ) -> Element<'a, Message> {
        let spacing = theme.cosmic().spacing;

        let mut content_col = column![
            widget::text::title4("Plugins"),
            widget::divider::horizontal::default(),
            widget::text::caption(
                "Enable or disable plugins globally (can be overridden per-device)"
            ),
        ]
        .spacing(spacing.space_xs);

        // Group plugins by category
        let grouped_plugins = settings::plugins_by_category();

        for (category, plugins) in grouped_plugins {
            content_col = content_col.push(widget::text::body(category));

            for plugin in plugins {
                if let Some(enabled) = config.plugins.get(plugin) {
                    let description = settings::plugin_description(plugin);
                    content_col = content_col.push(
                        widget::toggler(enabled)
                            .label(format!("{} - {}", plugin, description))
                            .on_toggle(move |e| {
                                Message::SetGlobalPluginEnabled(plugin.to_string(), e)
                            })
                            .width(Length::Fill),
                    );
                }
            }
        }

        widget::container(content_col.padding(spacing.space_s))
            .style(card_container_style)
            .width(Length::Fill)
            .into()
    }

    /// Discovery settings section
    fn discovery_settings_section<'a>(
        &'a self,
        theme: &cosmic::Theme,
        config: &'a settings::DaemonConfig,
    ) -> Element<'a, Message> {
        let spacing = theme.cosmic().spacing;

        let mut content_col = column![
            widget::text::title4("Discovery"),
            widget::divider::horizontal::default(),
            widget::text::caption("Configure device discovery and timeout settings"),
        ]
        .spacing(spacing.space_xs);

        // Discovery Interval
        content_col = content_col.push(
            row![
                widget::text::body("Broadcast Interval:").width(Length::Fixed(180.0)),
                widget::text::body(format!(
                    "{} seconds",
                    config.discovery.broadcast_interval_secs
                ))
                .width(Length::Fixed(100.0)),
                widget::text::caption("(How often to announce presence)"),
            ]
            .spacing(spacing.space_xs)
            .align_y(Alignment::Center),
        );

        // Device Timeout
        content_col = content_col.push(
            row![
                widget::text::body("Device Timeout:").width(Length::Fixed(180.0)),
                widget::text::body(format!("{} seconds", config.discovery.device_timeout_secs))
                    .width(Length::Fixed(100.0)),
                widget::text::caption("(How long before device is offline)"),
            ]
            .spacing(spacing.space_xs)
            .align_y(Alignment::Center),
        );

        content_col = content_col.push(widget::text::caption(
            "⚠ Changes to discovery settings require daemon restart",
        ));

        widget::container(content_col.padding(spacing.space_s))
            .style(card_container_style)
            .width(Length::Fill)
            .into()
    }

    /// Advanced settings section
    fn advanced_settings_section(
        &self,
        theme: &cosmic::Theme,
        _config: &settings::DaemonConfig,
    ) -> Element<'_, Message> {
        let spacing = theme.cosmic().spacing;

        let mut content_col = column![
            widget::text::title4("Advanced"),
            widget::divider::horizontal::default(),
            widget::text::caption("Advanced configuration and maintenance options"),
        ]
        .spacing(spacing.space_xs);

        // Reset to Defaults button
        content_col = content_col.push(
            column![
                widget::text::body("Reset Configuration"),
                widget::text::caption(
                    "Restore all settings to default values (preserves device ID)"
                ),
                widget::button::destructive("Reset to Defaults")
                    .on_press(Message::ResetConfigToDefaults),
            ]
            .spacing(spacing.space_xs),
        );

        content_col = content_col.push(widget::divider::horizontal::default());

        // Restart Daemon button
        content_col = content_col.push(
            column![
                widget::text::body("Restart Daemon"),
                widget::text::caption("Restart the background service to apply changes"),
                widget::button::suggested("Restart Daemon").on_press(Message::RestartDaemon),
            ]
            .spacing(spacing.space_xs),
        );

        widget::container(content_col.padding(spacing.space_s))
            .style(card_container_style)
            .width(Length::Fill)
            .into()
    }

    /// About section
    fn about_section(&self, theme: &cosmic::Theme) -> Element<'_, Message> {
        let spacing = theme.cosmic().spacing;

        widget::container(
            column![
                widget::text::title4("About"),
                widget::divider::horizontal::default(),

                // Application Info
                row![
                    widget::text::body("Application:"),
                    widget::horizontal_space(),
                    widget::text::body("COSMIC Connect"),
                ]
                .spacing(spacing.space_xxs),
                row![
                    widget::text::body("Version:"),
                    widget::horizontal_space(),
                    widget::text::body(env!("CARGO_PKG_VERSION")),
                ]
                .spacing(spacing.space_xxs),
                row![
                    widget::text::body("Protocol:"),
                    widget::horizontal_space(),
                    widget::text::body("CConnect v7/8 (port 1816)"),
                ]
                .spacing(spacing.space_xxs),

                widget::divider::horizontal::default(),

                // Description
                widget::text::body("Description"),
                widget::text::caption("Connect and sync your devices seamlessly with COSMIC Connect. Share files, sync clipboards, mirror notifications, and control media across all your devices."),

                widget::divider::horizontal::default(),

                // Features
                widget::text::body("Features"),
                column![
                    widget::text::caption("• File sharing and transfer"),
                    widget::text::caption("• Clipboard synchronization"),
                    widget::text::caption("• Notification mirroring"),
                    widget::text::caption("• Battery status monitoring"),
                    widget::text::caption("• Media player control (MPRIS)"),
                    widget::text::caption("• Remote input and commands"),
                    widget::text::caption("• Find My Phone"),
                    widget::text::caption("• Desktop-to-desktop features"),
                ]
                .spacing(2),

                widget::divider::horizontal::default(),

                // System Status
                widget::text::body("System Status"),
                row![
                    widget::text::caption("Daemon:"),
                    widget::horizontal_space(),
                    widget::text::caption(if self.dbus_client.is_some() { "Running" } else { "Disconnected" }),
                ]
                .spacing(spacing.space_xxs),
                row![
                    widget::text::caption("Connected Devices:"),
                    widget::horizontal_space(),
                    widget::text::caption(format!("{}", self.devices.len())),
                ]
                .spacing(spacing.space_xxs),
                row![
                    widget::text::caption("Active Transfers:"),
                    widget::horizontal_space(),
                    widget::text::caption(format!("{}",
                        self.transfers.values().filter(|t| t.status == TransferStatus::Active).count()
                    )),
                ]
                .spacing(spacing.space_xxs),

                widget::divider::horizontal::default(),

                // Links
                widget::text::body("Links"),
                widget::text::caption("GitHub: https://github.com/olafkfreund/cosmic-connect-desktop-app"),
                widget::text::caption("Report Issue: https://github.com/olafkfreund/cosmic-connect-desktop-app/issues"),

                widget::divider::horizontal::default(),

                // License & Credits
                widget::text::body("License"),
                widget::text::caption("Licensed under GPL-3.0-or-later"),
                widget::text::caption(""),
                widget::text::body("Built With"),
                widget::text::caption("• COSMIC Toolkit by System76"),
                widget::text::caption("• KDE Connect Protocol"),
                widget::text::caption("• Rust and libcosmic"),
                widget::text::caption(""),
                widget::text::caption("COSMIC Connect is compatible with KDE Connect v7/8 protocol"),
            ]
            .spacing(spacing.space_xs)
            .padding(spacing.space_s)
        )
        .style(card_container_style)
        .width(Length::Fill)
        .into()
    }

    /// MPRIS media controls section for Settings page
    fn mpris_controls_section(&self, theme: &cosmic::Theme) -> Element<'_, Message> {
        let spacing = theme.cosmic().spacing;

        let mut content_col = column![
            widget::text::title4("Media Player Controls"),
            widget::divider::horizontal::default(),
        ]
        .spacing(spacing.space_s);

        if self.mpris_players.is_empty() {
            content_col = content_col.push(
                column![
                    widget::text::body("No media players found"),
                    widget::text::caption("Make sure a media player is running"),
                    widget::button::standard("Refresh Players")
                        .on_press(Message::RefreshMprisPlayers),
                ]
                .spacing(spacing.space_xs),
            );
        } else {
            // Player selector
            if let Some(selected) = &self.selected_mpris_player {
                content_col = content_col.push(
                    row![
                        widget::text::body("Selected Player:"),
                        widget::horizontal_space(),
                        widget::text::body(selected),
                    ]
                    .spacing(spacing.space_xs),
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
                    .padding(spacing.space_s),
                    widget::button::icon(
                        widget::icon::from_name("media-playback-start-symbolic").size(24)
                    )
                    .on_press(Message::MprisControl(
                        selected.clone(),
                        "PlayPause".to_string()
                    ))
                    .padding(spacing.space_s),
                    widget::button::icon(
                        widget::icon::from_name("media-playback-stop-symbolic").size(20)
                    )
                    .on_press(Message::MprisControl(selected.clone(), "Stop".to_string()))
                    .padding(spacing.space_s),
                    widget::button::icon(
                        widget::icon::from_name("media-skip-forward-symbolic").size(20)
                    )
                    .on_press(Message::MprisControl(selected.clone(), "Next".to_string()))
                    .padding(spacing.space_s),
                ]
                .spacing(spacing.space_xs)
                .align_y(Alignment::Center);

                content_col = content_col.push(controls);
            }

            // Refresh button
            content_col = content_col.push(
                widget::button::standard("Refresh Players").on_press(Message::RefreshMprisPlayers),
            );

            // List all available players
            content_col = content_col.push(widget::text::body("Available Players:"));
            for player in &self.mpris_players {
                content_col = content_col.push(
                    widget::button::text(player)
                        .on_press(Message::MprisPlayerSelected(player.clone()))
                        .width(Length::Fill),
                );
            }
        }

        widget::container(content_col.padding(spacing.space_m))
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

/// Fetch daemon configuration
async fn fetch_daemon_config() -> Result<settings::DaemonConfig, String> {
    match DbusClient::connect().await {
        Ok((client, _)) => match client.get_daemon_config().await {
            Ok(json) => {
                tracing::info!("Fetched daemon configuration");
                settings::DaemonConfig::from_json(&json)
                    .map_err(|e| format!("Failed to parse config: {}", e))
            }
            Err(e) => {
                tracing::error!("Failed to get daemon config: {}", e);
                Err(format!("Failed to get daemon config: {}", e))
            }
        },
        Err(e) => {
            tracing::warn!("Failed to connect to daemon: {}", e);
            Err(format!("Failed to connect to daemon: {}", e))
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

/// Send a notification to a device
async fn send_notification(device_id: String, title: String, body: String) -> anyhow::Result<()> {
    let (client, _) = DbusClient::connect().await?;
    client.send_notification(&device_id, &title, &body).await?;
    tracing::info!("Sent notification to device: {}", device_id);
    Ok(())
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

// ===== Settings Helper Functions =====

/// Set device name
async fn set_device_name(name: String) -> Result<(), String> {
    match DbusClient::connect().await {
        Ok((client, _)) => client
            .set_device_name(&name)
            .await
            .map_err(|e| format!("Failed to set device name: {}", e)),
        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
    }
}

/// Set device type
async fn set_device_type(device_type: String) -> Result<(), String> {
    match DbusClient::connect().await {
        Ok((client, _)) => client
            .set_device_type(&device_type)
            .await
            .map_err(|e| format!("Failed to set device type: {}", e)),
        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
    }
}

/// Set global plugin enabled state
async fn set_global_plugin_enabled(plugin: String, enabled: bool) -> Result<(), String> {
    match DbusClient::connect().await {
        Ok((client, _)) => client
            .set_global_plugin_enabled(&plugin, enabled)
            .await
            .map_err(|e| format!("Failed to set plugin enabled: {}", e)),
        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
    }
}

/// Set TCP transport enabled
async fn set_tcp_enabled(enabled: bool) -> Result<(), String> {
    match DbusClient::connect().await {
        Ok((client, _)) => client
            .set_tcp_enabled(enabled)
            .await
            .map_err(|e| format!("Failed to set TCP enabled: {}", e)),
        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
    }
}

/// Set Bluetooth transport enabled
async fn set_bluetooth_enabled(enabled: bool) -> Result<(), String> {
    match DbusClient::connect().await {
        Ok((client, _)) => client
            .set_bluetooth_enabled(enabled)
            .await
            .map_err(|e| format!("Failed to set Bluetooth enabled: {}", e)),
        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
    }
}

/// Set transport preference
async fn set_transport_preference(preference: String) -> Result<(), String> {
    match DbusClient::connect().await {
        Ok((client, _)) => client
            .set_transport_preference(&preference)
            .await
            .map_err(|e| format!("Failed to set transport preference: {}", e)),
        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
    }
}

/// Set auto fallback enabled
async fn set_auto_fallback(enabled: bool) -> Result<(), String> {
    match DbusClient::connect().await {
        Ok((client, _)) => client
            .set_auto_fallback(enabled)
            .await
            .map_err(|e| format!("Failed to set auto fallback: {}", e)),
        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
    }
}

/// Set discovery interval in seconds
async fn set_discovery_interval(interval_secs: u64) -> Result<(), String> {
    match DbusClient::connect().await {
        Ok((client, _)) => client
            .set_discovery_interval(interval_secs)
            .await
            .map_err(|e| format!("Failed to set discovery interval: {}", e)),
        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
    }
}

/// Set device timeout in seconds
async fn set_device_timeout(timeout_secs: u64) -> Result<(), String> {
    match DbusClient::connect().await {
        Ok((client, _)) => client
            .set_device_timeout(timeout_secs)
            .await
            .map_err(|e| format!("Failed to set device timeout: {}", e)),
        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
    }
}

/// Reset configuration to defaults
async fn reset_config_to_defaults() -> Result<(), String> {
    match DbusClient::connect().await {
        Ok((client, _)) => client
            .reset_config_to_defaults()
            .await
            .map_err(|e| format!("Failed to reset config to defaults: {}", e)),
        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
    }
}

/// Restart the daemon
async fn restart_daemon() -> Result<(), String> {
    match DbusClient::connect().await {
        Ok((client, _)) => client
            .restart_daemon()
            .await
            .map_err(|e| format!("Failed to restart daemon: {}", e)),
        Err(e) => Err(format!("Failed to connect to daemon: {}", e)),
    }
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
fn device_type_style(device_type: &str, theme: &cosmic::Theme) -> DeviceIconStyle {
    let palette = &theme.cosmic().palette;
    match device_type.to_lowercase().as_str() {
        "phone" => DeviceIconStyle {
            icon_name: "phone-symbolic",
            color: palette.accent_blue.into(),
        },
        "tablet" => DeviceIconStyle {
            icon_name: "tablet-symbolic",
            color: palette.accent_purple.into(),
        },
        "desktop" => DeviceIconStyle {
            icon_name: "computer-symbolic",
            color: palette.bright_green.into(),
        },
        "laptop" => DeviceIconStyle {
            icon_name: "laptop-symbolic",
            color: palette.bright_orange.into(),
        },
        "tv" => DeviceIconStyle {
            icon_name: "tv-symbolic",
            color: palette.bright_red.into(),
        },
        _ => DeviceIconStyle {
            icon_name: "computer-symbolic",
            color: palette.neutral_6.into(),
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
fn card_container_style(theme: &cosmic::Theme) -> cosmic::iced::widget::container::Style {
    let palette = &theme.cosmic().palette;
    let corner_radii = &theme.cosmic().corner_radii;

    cosmic::iced::widget::container::Style {
        background: Some(cosmic::iced::Background::Color(palette.neutral_2.into())),
        border: cosmic::iced::Border {
            radius: corner_radii.radius_s.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Returns the warning container style (yellow/orange background)
///
/// Contrast Verification (WCAG AA Compliance):
/// - Background: bright_orange (warm amber, ~#FFA500)
/// - Text: gray_1 (near black, ~#1A1A1A)
/// - Estimated contrast ratio: ~8.5:1 (exceeds WCAG AAA standard of 7:1)
/// - Meets WCAG AA requirement of 4.5:1 for normal text
/// - COSMIC palette colors are specifically designed for accessibility compliance
fn warning_container_style(theme: &cosmic::Theme) -> cosmic::iced::widget::container::Style {
    let palette = &theme.cosmic().palette;
    let corner_radii = &theme.cosmic().corner_radii;

    cosmic::iced::widget::container::Style {
        background: Some(cosmic::iced::Background::Color(
            palette.bright_orange.into(),
        )),
        border: cosmic::iced::Border {
            radius: corner_radii.radius_s.into(),
            ..Default::default()
        },
        text_color: Some(palette.gray_1.into()),
        ..Default::default()
    }
}

/// Creates a detail row with label and value
fn detail_row<'a>(label: &'a str, value: impl ToString) -> Element<'a, Message> {
    row![
        widget::text::body(label),
        widget::horizontal_space(),
        widget::text::body(value.to_string()),
    ]
    .spacing(8)
    .into()
}
