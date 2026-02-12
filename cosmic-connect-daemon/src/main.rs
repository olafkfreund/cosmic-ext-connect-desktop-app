mod config;
mod cosmic_notifications;
mod dbus;
mod desktop_icons;
mod device_config;
mod diagnostics;
mod error_handler;
mod mpris_manager;
mod notification_image;
mod notification_listener;

use anyhow::{Context, Result};
use clap::Parser;
use cosmic_connect_protocol::{
    connection::{ConnectionConfig, ConnectionEvent, ConnectionManager},
    discovery::{
        default_additional_broadcast_addrs, DiscoveryConfig, DiscoveryEvent, DiscoveryService,
    },
    pairing::{PairingConfig, PairingEvent, PairingService, PairingStatus},
    plugins::{
        audiostream::AudioStreamPluginFactory,
        battery::BatteryPluginFactory,
        camera::CameraPluginFactory,
        chat::ChatPluginFactory,
        clipboard::ClipboardPluginFactory,
        clipboardhistory::ClipboardHistoryPluginFactory,
        connectivity_report::ConnectivityReportPluginFactory,
        contacts::{ContactsPlugin, ContactsPluginFactory},
        filesync::FileSyncPluginFactory,
        findmyphone::FindMyPhonePluginFactory,
        lock::LockPluginFactory,
        mousekeyboardshare::MouseKeyboardSharePluginFactory,
        mpris::MprisPluginFactory,
        networkshare::NetworkSharePluginFactory,
        notification::NotificationPluginFactory,
        ping::PingPluginFactory,
        power::PowerPluginFactory,
        presenter::PresenterPluginFactory,
        r#macro::MacroPluginFactory,
        remoteinput::RemoteInputPluginFactory,
        runcommand::RunCommandPluginFactory,
        screenshare::ScreenSharePluginFactory,
        screenshot::ScreenshotPluginFactory,
        share::SharePluginFactory,
        systemmonitor::SystemMonitorPluginFactory,
        systemvolume::SystemVolumePluginFactory,
        telephony::TelephonyPluginFactory,
        wol::WolPluginFactory,
        PluginManager,
    },
    CertificateInfo, DeviceInfo, DeviceManager, DeviceType, Packet, TransportManager,
    TransportManagerConfig, TransportManagerEvent,
};
use dbus::DbusServer;
use diagnostics::{BuildInfo, Cli, DiagnosticCommand, Metrics};
use tokio::sync::mpsc::{channel, Receiver, Sender};

use base64::{engine::general_purpose, Engine as _};
#[cfg(feature = "extendeddisplay")]
use cosmic_connect_protocol::plugins::extendeddisplay::ExtendedDisplayPluginFactory;
use cosmic_connect_protocol::plugins::remotedesktop::RemoteDesktopPluginFactory;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Placeholder address for Bluetooth connections that lack a real SocketAddr
const BT_PLACEHOLDER_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, trace, warn};

use config::Config;

use error_handler::ErrorHandler;

use notification_listener::{CapturedNotification, NotificationListener};

/// Main daemon state
#[allow(clippy::type_complexity)] // Complex types needed for async shared state
struct Daemon {
    /// Configuration (wrapped for shared access with DBus)
    config: Arc<RwLock<Config>>,

    /// Error handler for user notifications
    error_handler: Arc<ErrorHandler>,

    /// Device certificate (for TLS support)
    #[allow(dead_code)]
    certificate: CertificateInfo,

    /// TLS configuration for payload transfers
    tls_config: Arc<cosmic_connect_protocol::TlsConfig>,

    /// This device info
    device_info: DeviceInfo,

    /// Plugin manager
    plugin_manager: Arc<RwLock<PluginManager>>,

    /// Device manager (tracks discovered/paired devices)
    device_manager: Arc<RwLock<DeviceManager>>,

    /// Per-device configuration registry
    device_config_registry: Arc<RwLock<device_config::DeviceConfigRegistry>>,

    /// Discovery service
    discovery_service: Option<DiscoveryService>,

    /// Pairing service (wrapped for shared access with DBus)
    pairing_service: Option<Arc<RwLock<PairingService>>>,

    /// Connection manager (wrapped for shared access)
    connection_manager: Arc<RwLock<ConnectionManager>>,

    /// Transport manager (optional, used when Bluetooth is enabled)
    transport_manager: Option<Arc<TransportManager>>,

    /// COSMIC notifications client
    cosmic_notifier: Option<Arc<cosmic_notifications::CosmicNotifier>>,

    /// DBus server
    dbus_server: Option<Arc<DbusServer>>,

    /// MPRIS manager for local media player control
    mpris_manager: Option<Arc<mpris_manager::MprisManager>>,

    /// Map of notification IDs to device IDs for pairing notifications
    pairing_notifications: Arc<RwLock<std::collections::HashMap<u32, String>>>,

    /// Map of device IDs to pending pairing request status
    pending_pairing_requests: Arc<RwLock<std::collections::HashMap<String, bool>>>,

    /// Performance metrics (if enabled)
    metrics: Option<Arc<RwLock<Metrics>>>,

    /// Enable packet dumping (debug mode)
    dump_packets: bool,

    /// Packet sender for plugins
    packet_sender: Sender<(String, Packet)>,

    /// Packet receiver for plugins (wrapped in Mutex to allow extraction)
    packet_receiver: Arc<tokio::sync::Mutex<Option<Receiver<(String, Packet)>>>>,

    /// Track connection attempts for exponential backoff (device_id -> (last_attempt, failure_count))
    connection_attempts: Arc<RwLock<std::collections::HashMap<String, (std::time::Instant, u32)>>>,

    /// Receiver for captured notifications from the notification listener
    notification_receiver:
        Arc<tokio::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<CapturedNotification>>>>,
}

impl Daemon {
    /// Create a new daemon
    async fn new(config: Config) -> Result<Self> {
        // Ensure directories exist
        config
            .ensure_directories()
            .context("Failed to create directories")?;

        // Initialize error handler
        let error_handler = Arc::new(ErrorHandler::new());
        if let Err(e) = error_handler.init().await {
            warn!("Failed to initialize error handler: {}", e);
        }

        // Load or generate certificate
        let certificate =
            Self::load_or_generate_certificate(&config).context("Failed to load certificate")?;
        // Create device info
        let device_type = match config.device.device_type.as_str() {
            "laptop" => DeviceType::Laptop,
            "phone" => DeviceType::Phone,
            "tablet" => DeviceType::Tablet,
            "tv" => DeviceType::Tv,
            _ => DeviceType::Desktop,
        };

        let device_info = if let Some(device_id) = config.load_device_id() {
            // Use saved or configured device ID
            info!("Using existing device ID: {}", device_id);
            DeviceInfo::with_id(
                &device_id,
                &config.device.name,
                device_type,
                config.network.discovery_port,
            )
        } else {
            // Generate new device ID and save it
            let info = DeviceInfo::new(
                &config.device.name,
                device_type,
                config.network.discovery_port,
            );
            info!("Generated new device ID: {}", info.device_id);
            if let Err(e) = config.save_device_id(&info.device_id) {
                warn!(
                    "Failed to save device ID: {}. ID will change on next restart.",
                    e
                );
            }
            info
        };

        // Create plugin manager
        let plugin_manager = Arc::new(RwLock::new(PluginManager::new()));

        // Create packet channel for plugins
        let (packet_sender, packet_receiver) = channel(100);
        let packet_receiver = Arc::new(tokio::sync::Mutex::new(Some(packet_receiver)));

        // Create device manager
        let device_manager = Arc::new(RwLock::new(
            DeviceManager::new(config.device_registry_path())
                .context("Failed to create device manager")?,
        ));

        // Create and load device configuration registry
        let mut device_config_registry =
            device_config::DeviceConfigRegistry::new(&config.paths.config_dir);
        device_config_registry
            .load()
            .context("Failed to load device configurations")?;
        let device_config_registry = Arc::new(RwLock::new(device_config_registry));

        // Create TLS configuration for payload transfers
        let tls_config = Arc::new(
            cosmic_connect_protocol::TlsConfig::new(&certificate)
                .context("Failed to create TLS configuration")?,
        );

        // Create connection config
        let connection_config = ConnectionConfig {
            listen_addr: format!("[::]:{}", config.network.discovery_port)
                .parse()
                .context("Invalid listen address")?,
            keep_alive_interval: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(60),
        };

        // Create connection manager (not started yet)
        let connection_manager = Arc::new(RwLock::new(ConnectionManager::new(
            certificate.clone(),
            device_info.clone(),
            device_manager.clone(),
            connection_config,
        )?));

        // Create transport manager if Bluetooth is enabled
        let transport_manager = if config.transport.enable_bluetooth {
            info!("Bluetooth transport enabled - creating TransportManager");

            // Convert daemon TransportConfig to TransportManagerConfig
            let transport_config = TransportManagerConfig {
                enable_tcp: config.transport.enable_tcp,
                enable_bluetooth: config.transport.enable_bluetooth,
                preference: config.transport.preference.clone().into(),
                tcp_timeout: config.transport.tcp_timeout(),
                bluetooth_timeout: config.transport.bluetooth_timeout(),
                auto_fallback: config.transport.auto_fallback,
                bluetooth_device_filter: config.transport.bluetooth_device_filter.clone(),
            };

            match TransportManager::new(connection_manager.clone(), transport_config) {
                Ok(tm) => {
                    info!("TransportManager created successfully");
                    Some(Arc::new(tm))
                }
                Err(e) => {
                    warn!("Failed to create TransportManager: {}", e);
                    warn!("Falling back to TCP-only mode");
                    None
                }
            }
        } else {
            debug!("Bluetooth transport disabled - using ConnectionManager directly");
            None
        };

        // Initialize COSMIC notifications client
        let cosmic_notifier = match cosmic_notifications::CosmicNotifier::new().await {
            Ok(notifier) => {
                info!("COSMIC notifications client initialized");
                Some(Arc::new(notifier))
            }
            Err(e) => {
                warn!("Failed to initialize COSMIC notifications: {}", e);
                warn!("Notifications will be disabled");
                None
            }
        };

        // Initialize MPRIS manager if enabled
        let mpris_manager = if config.plugins.enable_mpris {
            mpris_manager::MprisManager::new()
                .await
                .map(|manager| {
                    info!("MPRIS manager initialized");
                    Arc::new(manager)
                })
                .map_err(|e| {
                    warn!("Failed to initialize MPRIS manager: {}", e);
                    warn!("MPRIS functionality will be disabled");
                })
                .ok()
        } else {
            None
        };

        // Wrap config in Arc<RwLock<>> for shared access with DBus
        let config = Arc::new(RwLock::new(config));

        Ok(Self {
            config,
            error_handler,
            certificate,
            tls_config,
            device_info,
            plugin_manager,
            device_manager,
            device_config_registry,
            discovery_service: None,
            pairing_service: None,
            connection_manager,
            transport_manager,
            cosmic_notifier,
            dbus_server: None,
            mpris_manager,
            pairing_notifications: Arc::new(RwLock::new(std::collections::HashMap::new())),
            pending_pairing_requests: Arc::new(RwLock::new(std::collections::HashMap::new())),
            metrics: None,
            dump_packets: false,
            packet_sender,
            packet_receiver,
            connection_attempts: Arc::new(RwLock::new(std::collections::HashMap::new())),
            notification_receiver: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    /// Load or generate device certificate
    fn load_or_generate_certificate(config: &Config) -> Result<CertificateInfo> {
        let cert_path = config.certificate_path();
        let key_path = config.private_key_path();

        if cert_path.exists() && key_path.exists() {
            info!("Loading existing certificate from {:?}", cert_path);
            CertificateInfo::load_from_files(&cert_path, &key_path)
                .context("Failed to load certificate")
        } else {
            info!("Generating new device certificate");
            let device_id = config
                .device
                .device_id
                .as_deref()
                .unwrap_or("cconnect-device");

            let cert =
                CertificateInfo::generate(device_id).context("Failed to generate certificate")?;

            // Save certificate
            cert.save_to_files(&cert_path, &key_path)
                .context("Failed to save certificate")?;

            info!("Certificate saved to {:?}", cert_path);
            Ok(cert)
        }
    }

    /// Initialize plugin factories
    async fn initialize_plugins(&self) -> Result<()> {
        let mut manager = self.plugin_manager.write().await;
        let config = self.config.read().await;

        info!("Registering plugin factories...");

        // Register enabled plugin factories
        if config.plugins.enable_ping {
            info!("Registering ping plugin factory");
            manager
                .register_factory(Arc::new(PingPluginFactory))
                .context("Failed to register ping plugin factory")?;
        }

        if config.plugins.enable_battery {
            info!("Registering battery plugin factory");
            manager
                .register_factory(Arc::new(BatteryPluginFactory))
                .context("Failed to register battery plugin factory")?;
        }

        if config.plugins.enable_notification {
            info!("Registering notification plugin factory");
            manager
                .register_factory(Arc::new(NotificationPluginFactory))
                .context("Failed to register notification plugin factory")?;
        }

        if config.plugins.enable_share {
            info!("Registering share plugin factory");
            manager
                .register_factory(Arc::new(SharePluginFactory))
                .context("Failed to register share plugin factory")?;
        }

        if config.plugins.enable_clipboard {
            info!("Registering clipboard plugin factory");
            manager
                .register_factory(Arc::new(ClipboardPluginFactory))
                .context("Failed to register clipboard plugin factory")?;
        }

        if config.plugins.enable_mpris {
            info!("Registering MPRIS plugin factory");
            manager
                .register_factory(Arc::new(MprisPluginFactory))
                .context("Failed to register MPRIS plugin factory")?;
        }

        if config.plugins.enable_runcommand {
            info!("Registering RunCommand plugin factory");
            manager
                .register_factory(Arc::new(RunCommandPluginFactory))
                .context("Failed to register RunCommand plugin factory")?;
        }

        if config.plugins.enable_remoteinput {
            info!("Registering Remote Input plugin factory");
            manager
                .register_factory(Arc::new(RemoteInputPluginFactory))
                .context("Failed to register Remote Input plugin factory")?;
        }

        if config.plugins.enable_findmyphone {
            info!("Registering Find My Phone plugin factory");
            manager
                .register_factory(Arc::new(FindMyPhonePluginFactory))
                .context("Failed to register Find My Phone plugin factory")?;
        }

        if config.plugins.enable_lock {
            info!("Registering Lock plugin factory");
            manager
                .register_factory(Arc::new(LockPluginFactory))
                .context("Failed to register Lock plugin factory")?;
        }

        if config.plugins.enable_telephony {
            info!("Registering Telephony/SMS plugin factory");
            manager
                .register_factory(Arc::new(TelephonyPluginFactory))
                .context("Failed to register Telephony plugin factory")?;
        }

        if config.plugins.enable_presenter {
            info!("Registering Presenter plugin factory");
            manager
                .register_factory(Arc::new(PresenterPluginFactory))
                .context("Failed to register Presenter plugin factory")?;
        }

        if config.plugins.enable_contacts {
            info!("Registering Contacts plugin factory");
            manager
                .register_factory(Arc::new(ContactsPluginFactory))
                .context("Failed to register Contacts plugin factory")?;
        }

        if config.plugins.enable_systemmonitor {
            info!("Registering SystemMonitor plugin factory");
            manager
                .register_factory(Arc::new(SystemMonitorPluginFactory))
                .context("Failed to register SystemMonitor plugin factory")?;
        }

        if config.plugins.enable_wol {
            info!("Registering Wake-on-LAN plugin factory");
            manager
                .register_factory(Arc::new(WolPluginFactory))
                .context("Failed to register WOL plugin factory")?;
        }

        if config.plugins.enable_screenshot {
            info!("Registering Screenshot plugin factory");
            manager
                .register_factory(Arc::new(ScreenshotPluginFactory))
                .context("Failed to register Screenshot plugin factory")?;
        }

        if config.plugins.enable_remotedesktop {
            info!("Registering RemoteDesktop plugin factory");
            manager
                .register_factory(Arc::new(RemoteDesktopPluginFactory))
                .context("Failed to register RemoteDesktop plugin factory")?;
        }

        if config.plugins.enable_power {
            info!("Registering Power plugin factory");
            manager
                .register_factory(Arc::new(PowerPluginFactory))
                .context("Failed to register Power plugin factory")?;
        }

        if config.plugins.enable_clipboardhistory {
            info!("Registering ClipboardHistory plugin factory");
            manager
                .register_factory(Arc::new(ClipboardHistoryPluginFactory))
                .context("Failed to register ClipboardHistory plugin factory")?;
        }

        if config.plugins.enable_macro {
            info!("Registering Macro plugin factory");
            manager
                .register_factory(Arc::new(MacroPluginFactory))
                .context("Failed to register Macro plugin factory")?;
        }

        if config.plugins.enable_chat {
            info!("Registering Chat plugin factory");
            manager
                .register_factory(Arc::new(ChatPluginFactory))
                .context("Failed to register Chat plugin factory")?;
        }

        if config.plugins.enable_audiostream {
            info!("Registering AudioStream plugin factory");
            manager
                .register_factory(Arc::new(AudioStreamPluginFactory))
                .context("Failed to register AudioStream plugin factory")?;
        }

        if config.plugins.enable_filesync {
            info!("Registering FileSync plugin factory");
            manager
                .register_factory(Arc::new(FileSyncPluginFactory))
                .context("Failed to register FileSync plugin factory")?;
        }

        if config.plugins.enable_screenshare {
            info!("Registering ScreenShare plugin factory");
            manager
                .register_factory(Arc::new(ScreenSharePluginFactory::with_restore_session(
                    config.plugins.screenshare_restore_session,
                )))
                .context("Failed to register ScreenShare plugin factory")?;
        }

        if config.plugins.enable_mousekeyboardshare {
            info!("Registering MouseKeyboardShare plugin factory");
            manager
                .register_factory(Arc::new(MouseKeyboardSharePluginFactory))
                .context("Failed to register MouseKeyboardShare plugin factory")?;
        }

        if config.plugins.enable_networkshare {
            info!("Registering NetworkShare plugin factory");
            manager
                .register_factory(Arc::new(NetworkSharePluginFactory))
                .context("Failed to register NetworkShare plugin factory")?;
        }

        if config.plugins.enable_systemvolume {
            info!("Registering SystemVolume plugin factory");
            manager
                .register_factory(Arc::new(SystemVolumePluginFactory))
                .context("Failed to register SystemVolume plugin factory")?;
        }

        if config.plugins.enable_connectivityreport {
            info!("Registering ConnectivityReport plugin factory");
            manager
                .register_factory(Arc::new(ConnectivityReportPluginFactory))
                .context("Failed to register ConnectivityReport plugin factory")?;
        }

        if config.plugins.enable_camera {
            info!("Registering Camera plugin factory");
            manager
                .register_factory(Arc::new(CameraPluginFactory))
                .context("Failed to register Camera plugin factory")?;
        }

        #[cfg(feature = "extendeddisplay")]
        if config.plugins.enable_extendeddisplay {
            info!("Registering ExtendedDisplay plugin factory");
            manager
                .register_factory(Arc::new(ExtendedDisplayPluginFactory::new()))
                .context("Failed to register ExtendedDisplay plugin factory")?;
        }

        info!(
            "All plugin factories registered ({} total)",
            manager.factory_count()
        );

        Ok(())
    }

    /// Start discovery service
    async fn start_discovery(&mut self) -> Result<()> {
        info!("Starting device discovery...");

        let config = self.config.read().await;

        // Get capabilities from plugin manager
        let manager = self.plugin_manager.read().await;
        let incoming = manager.get_all_incoming_capabilities();
        let outgoing = manager.get_all_outgoing_capabilities();
        drop(manager);

        // Update self.device_info with capabilities
        self.device_info.incoming_capabilities = incoming;
        self.device_info.outgoing_capabilities = outgoing;

        // Update connection manager with new capabilities
        self.connection_manager
            .write()
            .await
            .update_device_info(self.device_info.clone());

        let device_info = self.device_info.clone();
        let discovery_config = DiscoveryConfig {
            broadcast_interval: Duration::from_secs(config.network.discovery_interval),
            device_timeout: Duration::from_secs(config.network.device_timeout),
            enable_timeout_check: true,
            additional_broadcast_addrs: default_additional_broadcast_addrs(),
        };
        drop(config);

        // Create discovery service
        let mut discovery_service = DiscoveryService::new(device_info, discovery_config)
            .context("Failed to create discovery service")?;

        // Subscribe to discovery events
        let mut event_rx = discovery_service.subscribe().await;

        // Start discovery service
        discovery_service
            .start()
            .await
            .context("Failed to start discovery service")?;

        info!(
            "Discovery service started on port {}",
            discovery_service.local_port()?
        );

        // Store discovery service
        self.discovery_service = Some(discovery_service);

        // Spawn task to handle discovery events
        let device_manager = self.device_manager.clone();
        let dbus_server = self.dbus_server.clone();
        let error_handler = self.error_handler.clone();
        let connection_manager = self.connection_manager.clone();
        let connection_attempts = self.connection_attempts.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if let Err(e) = Self::handle_discovery_event(
                    event,
                    &device_manager,
                    &dbus_server,
                    &error_handler,
                    &connection_manager,
                    &connection_attempts,
                )
                .await
                {
                    error!("Error handling discovery event: {}", e);
                }
            }
            info!("Discovery event handler stopped");
        });

        Ok(())
    }

    /// Start pairing service
    async fn start_pairing(&mut self) -> Result<()> {
        info!("Starting pairing service...");

        let config = self.config.read().await;

        // Create pairing service with certificate directory from config
        let pairing_config = PairingConfig {
            cert_dir: config.paths.cert_dir.clone(),
            timeout: Duration::from_secs(30),
        };

        let pairing_service =
            PairingService::new(self.device_info.device_id.clone(), pairing_config)
                .context("Failed to create pairing service")?;

        info!(
            "Pairing service created (fingerprint: {})",
            pairing_service.fingerprint()
        );

        // Wrap in Arc<RwLock> for shared access
        let pairing_service = Arc::new(RwLock::new(pairing_service));

        // Set connection manager for Protocol v8 (pairing over TLS)
        {
            let mut service = pairing_service.write().await;
            service.set_connection_manager(self.connection_manager.clone());
        }

        // Subscribe to pairing events
        let mut event_rx = {
            let service = pairing_service.read().await;
            service.subscribe().await
        };

        // Store pairing service
        self.pairing_service = Some(pairing_service.clone());

        // Spawn task to handle pairing events
        let device_manager = self.device_manager.clone();
        let dbus_server = self.dbus_server.clone();
        let cosmic_notifier = self.cosmic_notifier.clone();
        let pairing_notifications = self.pairing_notifications.clone();
        let pending_pairing_requests = self.pending_pairing_requests.clone();
        let error_handler = self.error_handler.clone();
        let plugin_manager = self.plugin_manager.clone();
        let packet_sender = self.packet_sender.clone();
        let tls_config = self.tls_config.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if let Err(e) = Self::handle_pairing_event(
                    event,
                    &device_manager,
                    &dbus_server,
                    &cosmic_notifier,
                    &pairing_notifications,
                    &pending_pairing_requests,
                    &error_handler,
                    &plugin_manager,
                    &packet_sender,
                    &tls_config,
                )
                .await
                {
                    error!("Error handling pairing event: {}", e);
                }
            }
            info!("Pairing event handler stopped");
        });

        info!("Pairing service started");

        Ok(())
    }

    /// Handle a pairing event
    #[allow(clippy::too_many_arguments)]
    async fn handle_pairing_event(
        event: PairingEvent,
        device_manager: &Arc<RwLock<DeviceManager>>,
        dbus_server: &Option<Arc<DbusServer>>,
        cosmic_notifier: &Option<Arc<cosmic_notifications::CosmicNotifier>>,
        pairing_notifications: &Arc<RwLock<std::collections::HashMap<u32, String>>>,
        pending_pairing_requests: &Arc<RwLock<std::collections::HashMap<String, bool>>>,
        error_handler: &ErrorHandler,
        plugin_manager: &Arc<RwLock<PluginManager>>,
        packet_sender: &Sender<(String, Packet)>,
        tls_config: &Arc<cosmic_connect_protocol::TlsConfig>,
    ) -> Result<()> {
        match event {
            PairingEvent::RequestSent {
                device_id,
                our_fingerprint,
            } => {
                info!(
                    "Pairing request sent to device {} (our fingerprint: {})",
                    device_id, our_fingerprint
                );
            }
            PairingEvent::RequestReceived {
                device_id,
                device_name,
                their_fingerprint,
            } => {
                info!(
                    "Pairing request received from {} ({}) - fingerprint: {}",
                    device_name, device_id, their_fingerprint
                );
                info!("User should verify fingerprints match on both devices");

                // Track pending pairing request
                pending_pairing_requests
                    .write()
                    .await
                    .insert(device_id.clone(), true);
                info!("Added {} to pending pairing requests", device_id);

                // Emit DBus signal for pairing request
                if let Some(dbus) = dbus_server {
                    if let Err(e) = dbus.emit_pairing_request(&device_id).await {
                        warn!("Failed to emit PairingRequest signal: {}", e);
                    }
                }

                // Show COSMIC notification for pairing request
                if let Some(notifier) = cosmic_notifier {
                    info!("Sending pairing request notification for {}", device_name);
                    match notifier.notify_pairing_request(&device_name).await {
                        Ok(notification_id) => {
                            info!(
                                "Pairing request notification sent successfully (ID: {})",
                                notification_id
                            );
                            // Store notification ID so we can handle clicks
                            let mut notifications = pairing_notifications.write().await;
                            notifications.insert(notification_id, device_id.clone());
                        }
                        Err(e) => {
                            warn!("Failed to send pairing request notification: {}", e);
                        }
                    }
                } else {
                    warn!("COSMIC notifier not available for pairing request");
                }
            }
            PairingEvent::PairingAccepted {
                device_id,
                device_name,
                certificate_fingerprint,
            } => {
                info!("Pairing accepted with {} ({})", device_name, device_id);
                Self::clear_pending_pairing_request(pending_pairing_requests, &device_id).await;

                // Mark device as paired and save to disk
                {
                    let mut manager = device_manager.write().await;
                    if let Err(e) = manager
                        .mark_paired(&device_id, certificate_fingerprint.clone())
                        .and_then(|()| manager.save_registry())
                    {
                        error!("Failed to persist pairing for device {}: {}", device_id, e);
                    } else {
                        info!(
                            "Device {} paired with fingerprint: {}",
                            device_id, certificate_fingerprint
                        );
                    }
                }

                // Initialize plugins for newly paired device
                // This handles the case where device connected first, then paired later
                {
                    let dev_manager = device_manager.read().await;
                    if let Some(device) = dev_manager.get_device(&device_id) {
                        let mut plug_manager = plugin_manager.write().await;
                        if let Err(e) = plug_manager
                            .init_device_plugins(&device_id, device, packet_sender.clone())
                            .await
                        {
                            error!(
                                "Failed to initialize plugins for device {}: {}",
                                device_id, e
                            );
                        } else {
                            info!("Initialized plugins for device {} after pairing", device_id);

                            // Set TLS config on SharePlugin for secure file transfers
                            if let Some(plugin) =
                                plug_manager.get_device_plugin_mut(&device_id, "share")
                            {
                                use cosmic_connect_protocol::plugins::share::SharePlugin;
                                if let Some(share_plugin) =
                                    plugin.as_any_mut().downcast_mut::<SharePlugin>()
                                {
                                    share_plugin.set_tls_config(tls_config.clone());
                                    debug!(
                                        "Set TLS config on SharePlugin for device {}",
                                        device_id
                                    );
                                }
                            }
                        }
                    } else {
                        warn!("Device {} not found in manager after pairing", device_id);
                    }
                }

                // Create desktop icon for paired device
                {
                    let dev_manager = device_manager.read().await;
                    if let Some(device) = dev_manager.get_device(&device_id) {
                        if let Err(e) = desktop_icons::sync_desktop_icon(device, None) {
                            warn!(
                                "Failed to create desktop icon for device {}: {}",
                                device_id, e
                            );
                        } else {
                            info!("Created desktop icon for device {}", device_id);
                        }
                    }
                }

                if let Some(dbus) = dbus_server {
                    if let Err(e) = dbus.emit_pairing_status_changed(&device_id, "paired").await {
                        warn!("Failed to emit PairingStatusChanged signal: {}", e);
                    }
                }
            }
            PairingEvent::PairingRejected { device_id, reason } => {
                info!(
                    "Pairing rejected with device {} (reason: {:?})",
                    device_id, reason
                );
                Self::clear_pending_pairing_request(pending_pairing_requests, &device_id).await;

                if let Some(dbus) = dbus_server {
                    if let Err(e) = dbus
                        .emit_pairing_status_changed(&device_id, "rejected")
                        .await
                    {
                        warn!("Failed to emit PairingStatusChanged signal: {}", e);
                    }
                }
            }
            PairingEvent::StatusChanged { device_id, status } => {
                debug!("Pairing status changed for {}: {:?}", device_id, status);
            }
            PairingEvent::DeviceUnpaired { device_id } => {
                info!("Device unpaired: {}", device_id);
                let mut manager = device_manager.write().await;
                if let Err(e) = manager.update_pairing_status(&device_id, PairingStatus::Unpaired) {
                    warn!(
                        "Failed to update device {} pairing status: {}",
                        device_id, e
                    );
                } else if let Err(e) = manager.save_registry() {
                    warn!("Failed to save device registry: {}", e);
                }

                // Remove desktop icon for unpaired device
                if let Err(e) = desktop_icons::remove_desktop_icon(&device_id) {
                    warn!(
                        "Failed to remove desktop icon for device {}: {}",
                        device_id, e
                    );
                } else {
                    info!("Removed desktop icon for device {}", device_id);
                }
            }
            PairingEvent::PairingTimeout { device_id } => {
                warn!("Pairing request timed out for device {}", device_id);
                let error = cosmic_connect_protocol::ProtocolError::Timeout(
                    "Pairing request timed out".to_string(),
                );
                error_handler
                    .handle_error(&error, "pairing", Some(&device_id))
                    .await;
            }
            PairingEvent::Error { device_id, message } => {
                error!("Pairing error for device {:?}: {}", device_id, message);
                let error = cosmic_connect_protocol::ProtocolError::NetworkError(message);
                error_handler
                    .handle_error(&error, "pairing", device_id.as_deref())
                    .await;
            }
        }
        Ok(())
    }

    /// Clear a pending pairing request from the tracking map
    async fn clear_pending_pairing_request(
        pending_pairing_requests: &Arc<RwLock<std::collections::HashMap<String, bool>>>,
        device_id: &str,
    ) {
        if pending_pairing_requests
            .write()
            .await
            .remove(device_id)
            .is_some()
        {
            info!("Removed {} from pending pairing requests", device_id);
        }
    }

    /// Start connection manager
    async fn start_connections(&mut self) -> Result<()> {
        info!("Starting connection manager...");

        // If TransportManager is available, use it; otherwise use ConnectionManager directly
        if let Some(transport_mgr) = &self.transport_manager {
            info!("Using TransportManager (Bluetooth enabled)");

            // Start transport manager (starts all enabled transports)
            transport_mgr
                .start()
                .await
                .context("Failed to start transport manager")?;

            info!("TransportManager started successfully");

            // Subscribe to transport manager events
            let mut event_rx = transport_mgr.subscribe().await;

            // Spawn task to handle transport manager events
            let device_manager = self.device_manager.clone();
            let plugin_manager = self.plugin_manager.clone();
            let connection_mgr = self.connection_manager.clone();
            let device_config_registry = self.device_config_registry.clone();
            let pairing_service = self.pairing_service.clone();
            let dbus_server = self.dbus_server.clone();
            let cosmic_notifier = self.cosmic_notifier.clone();
            let mpris_manager = self.mpris_manager.clone();
            let dump_packets = self.dump_packets;
            let packet_sender = self.packet_sender.clone();
            let config = self.config.clone();
            let error_handler = Some(self.error_handler.clone());
            let tls_config = self.tls_config.clone();
            tokio::spawn(async move {
                while let Some(event) = event_rx.recv().await {
                    // Convert TransportManagerEvent to ConnectionEvent
                    let connection_event = match event {
                        TransportManagerEvent::Connected {
                            device_id,
                            transport_type,
                        } => {
                            info!("Device {} connected via {:?}", device_id, transport_type);
                            // We don't have remote_addr for Bluetooth, use placeholder
                            ConnectionEvent::Connected {
                                device_id,
                                remote_addr: BT_PLACEHOLDER_ADDR,
                            }
                        }
                        TransportManagerEvent::Disconnected {
                            device_id,
                            transport_type,
                            reason,
                        } => {
                            info!(
                                "Device {} disconnected from {:?} (reason: {:?})",
                                device_id, transport_type, reason
                            );
                            ConnectionEvent::Disconnected {
                                device_id,
                                reason,
                                reconnect: false, // Transport-level disconnects are not automatic reconnects
                            }
                        }
                        TransportManagerEvent::PacketReceived {
                            device_id,
                            packet,
                            transport_type,
                        } => {
                            debug!(
                                "Received packet from {} via {:?}",
                                device_id, transport_type
                            );
                            ConnectionEvent::PacketReceived {
                                device_id,
                                packet,
                                remote_addr: BT_PLACEHOLDER_ADDR,
                            }
                        }
                        TransportManagerEvent::Started { transport_type } => {
                            info!("Transport {:?} started", transport_type);
                            continue;
                        }
                        TransportManagerEvent::Error {
                            transport_type,
                            message,
                        } => {
                            error!("Transport {:?} error: {}", transport_type, message);
                            continue;
                        }
                    };

                    // Handle the converted event
                    if let Err(e) = Self::handle_connection_event(
                        connection_event,
                        &device_manager,
                        &plugin_manager,
                        &connection_mgr,
                        &device_config_registry,
                        &pairing_service,
                        &dbus_server,
                        &cosmic_notifier,
                        &mpris_manager,
                        dump_packets,
                        packet_sender.clone(),
                        &config,
                        &error_handler,
                        &tls_config,
                    )
                    .await
                    {
                        error!("Error handling connection event: {}", e);
                    }
                }
                info!("Transport event handler stopped");
            });
        } else {
            info!("Using ConnectionManager directly (Bluetooth disabled)");

            // Start the manager (starts TLS server)
            let port = {
                let manager = self.connection_manager.write().await;
                manager
                    .start()
                    .await
                    .context("Failed to start connection manager")?
            };

            info!("Connection manager started on port {}", port);

            // Subscribe to connection events
            let mut event_rx = {
                let manager = self.connection_manager.read().await;
                manager.subscribe().await
            };

            // Spawn task to handle connection events
            let device_manager = self.device_manager.clone();
            let plugin_manager = self.plugin_manager.clone();
            let connection_mgr = self.connection_manager.clone();
            let device_config_registry = self.device_config_registry.clone();
            let pairing_service = self.pairing_service.clone();
            let dbus_server = self.dbus_server.clone();
            let cosmic_notifier = self.cosmic_notifier.clone();
            let mpris_manager = self.mpris_manager.clone();
            let dump_packets = self.dump_packets;
            let packet_sender = self.packet_sender.clone();
            let config = self.config.clone();
            let error_handler = Some(self.error_handler.clone());
            let tls_config = self.tls_config.clone();
            tokio::spawn(async move {
                while let Some(event) = event_rx.recv().await {
                    if let Err(e) = Self::handle_connection_event(
                        event,
                        &device_manager,
                        &plugin_manager,
                        &connection_mgr,
                        &device_config_registry,
                        &pairing_service,
                        &dbus_server,
                        &cosmic_notifier,
                        &mpris_manager,
                        dump_packets,
                        packet_sender.clone(),
                        &config,
                        &error_handler,
                        &tls_config,
                    )
                    .await
                    {
                        error!("Error handling connection event: {}", e);
                    }
                }
                info!("Connection event handler stopped");
            });
        }

        info!("Connection manager started successfully");

        Ok(())
    }

    /// Start DBus server
    async fn start_dbus(&mut self) -> Result<()> {
        info!("Starting DBus server...");

        let dbus_server = DbusServer::start(
            self.device_manager.clone(),
            self.plugin_manager.clone(),
            self.connection_manager.clone(),
            self.device_config_registry.clone(),
            self.pairing_service.clone(),
            self.mpris_manager.clone(),
            self.pending_pairing_requests.clone(),
            self.metrics.clone(),
            self.config.clone(),
        )
        .await
        .context("Failed to start DBus server")?;

        info!("DBus server started on {}", dbus::SERVICE_NAME);

        self.dbus_server = Some(Arc::new(dbus_server));

        Ok(())
    }

    /// Start MPRIS player monitoring
    async fn start_mpris_monitoring(&self) -> Result<()> {
        let Some(mpris_manager) = &self.mpris_manager else {
            return Ok(());
        };

        info!("Starting MPRIS player discovery and monitoring...");

        let players = match mpris_manager.discover_players().await {
            Ok(players) => players,
            Err(e) => {
                warn!("Failed to discover MPRIS players: {}", e);
                return Ok(());
            }
        };

        info!("Discovered {} MPRIS players: {:?}", players.len(), players);

        for player in players {
            if let Err(e) = mpris_manager.start_monitoring(player.clone()).await {
                warn!("Failed to start monitoring player {}: {}", player, e);
            } else {
                info!("Started monitoring player: {}", player);
            }
        }

        Ok(())
    }

    async fn start_clipboard_monitor(&self) -> Result<()> {
        let config = self.config.read().await;
        if !config.plugins.enable_clipboard {
            info!("Clipboard plugin disabled, skipping clipboard monitor");
            return Ok(());
        }

        info!("Starting clipboard monitor...");

        let device_manager = self.device_manager.clone();
        let plugin_manager = self.plugin_manager.clone();
        let connection_manager = self.connection_manager.clone();

        // Spawn background task to monitor clipboard
        tokio::spawn(async move {
            use arboard::Clipboard;
            use std::time::Duration;

            // Initialize clipboard
            let mut clipboard = match Clipboard::new() {
                Ok(cb) => cb,
                Err(e) => {
                    error!("Failed to initialize clipboard: {}", e);
                    return;
                }
            };

            let mut last_content = String::new();
            let poll_interval = Duration::from_millis(500);

            info!(
                "Clipboard monitor started (polling every {:?})",
                poll_interval
            );

            loop {
                tokio::time::sleep(poll_interval).await;

                // Read current clipboard content
                let current_content = match clipboard.get_text() {
                    Ok(text) => text,
                    Err(_) => continue, // Clipboard might be empty or contain non-text
                };

                // Check if clipboard changed
                if current_content != last_content && !current_content.is_empty() {
                    debug!("Clipboard changed: {} chars", current_content.len());

                    // Update local clipboard plugin state for all connected devices
                    let dev_manager = device_manager.read().await;
                    let connected_devices: Vec<String> = dev_manager
                        .devices()
                        .filter(|d| d.is_connected())
                        .map(|d| d.id().to_string())
                        .collect();
                    drop(dev_manager);

                    if !connected_devices.is_empty() {
                        let plug_manager = plugin_manager.read().await;

                        for device_id in &connected_devices {
                            if let Some(plugin) =
                                plug_manager.get_device_plugin(device_id, "clipboard")
                            {
                                // Downcast to ClipboardPlugin
                                use cosmic_connect_protocol::plugins::clipboard::ClipboardPlugin;
                                if let Some(clipboard_plugin) =
                                    plugin.as_any().downcast_ref::<ClipboardPlugin>()
                                {
                                    // Create clipboard packet
                                    let packet = clipboard_plugin
                                        .create_clipboard_packet(current_content.clone())
                                        .await;

                                    // Send packet via connection manager
                                    let conn_manager = connection_manager.read().await;
                                    if let Err(e) =
                                        conn_manager.send_packet(device_id, &packet).await
                                    {
                                        warn!(
                                            "Failed to send clipboard update to {}: {}",
                                            device_id, e
                                        );
                                    } else {
                                        debug!(
                                            "Sent clipboard update to {} ({} chars)",
                                            device_id,
                                            current_content.len()
                                        );
                                    }
                                }
                            }
                        }
                        drop(plug_manager);
                    }

                    last_content = current_content;
                }
            }
        });

        info!("Clipboard monitor started successfully");

        // Start notification action listener if available
        if let Some(notifier) = &self.cosmic_notifier {
            info!("Starting notification action listener...");

            let notifier_clone = notifier.clone();
            let pairing_service = self.pairing_service.clone();
            let pairing_notifications = self.pairing_notifications.clone();
            let _device_manager = self.device_manager.clone();

            tokio::spawn(async move {
                use futures::StreamExt;

                match notifier_clone.subscribe_actions().await {
                    Ok(mut action_stream) => {
                        info!("Notification action listener started");

                        while let Some((notification_id, action_key)) = action_stream.next().await {
                            debug!(
                                "Received notification action: id={}, action={}",
                                notification_id, action_key
                            );

                            // Check if this is a pairing notification
                            let device_id = {
                                let notifications = pairing_notifications.read().await;
                                notifications.get(&notification_id).cloned()
                            };

                            if let Some(device_id) = device_id {
                                info!(
                                    "Handling pairing action '{}' for device {}",
                                    action_key, device_id
                                );

                                if let Some(pairing_svc) = &pairing_service {
                                    let pairing = pairing_svc.read().await;

                                    match action_key.as_str() {
                                        "accept" => {
                                            info!("User accepted pairing for {}", device_id);
                                            if let Err(e) = pairing.accept_pairing(&device_id).await
                                            {
                                                error!("Failed to accept pairing: {}", e);
                                            }
                                        }
                                        "reject" => {
                                            info!("User rejected pairing for {}", device_id);
                                            if let Err(e) = pairing.reject_pairing(&device_id).await
                                            {
                                                error!("Failed to reject pairing: {}", e);
                                            }
                                        }
                                        _ => {
                                            if action_key.starts_with("open_web:") {
                                                let url = action_key
                                                    .trim_start_matches("open_web:")
                                                    .to_string();
                                                info!("Opening web URL from notification: {}", url);
                                                tokio::spawn(async move {
                                                    if let Err(e) =
                                                        tokio::process::Command::new("xdg-open")
                                                            .arg(url)
                                                            .spawn()
                                                    {
                                                        error!("Failed to open web URL: {}", e);
                                                    }
                                                });
                                            } else if action_key == "reply" {
                                                // Quick reply requires a UI dialog to collect user input.
                                                // This should be implemented in cosmic-connect-manager
                                                // via DBus method call that opens a reply dialog.
                                                // FIXME(#future): Implement quick reply dialog in manager
                                                // - Add DBus method: OpenReplyDialog(device_id, notification_id)
                                                // - Manager shows input dialog, sends reply via daemon
                                                // - Send cconnect.notification.reply packet with user text
                                                info!("Quick reply requested - UI dialog not yet implemented");
                                            } else {
                                                warn!(
                                                    "Unknown notification action: {}",
                                                    action_key
                                                );
                                            }
                                        }
                                    }
                                }

                                // Remove notification from tracking
                                let mut notifications = pairing_notifications.write().await;
                                notifications.remove(&notification_id);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to subscribe to notification actions: {}", e);
                    }
                }
            });
        }

        Ok(())
    }

    /// Start notification listener
    async fn start_notification_listener(&mut self) -> Result<()> {
        let config = self.config.read().await;

        if !config.notification_listener.enabled {
            info!("Notification listener is disabled");
            return Ok(());
        }

        info!("Starting notification listener...");

        // Create channel for captured notifications
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        // Initialize notification listener
        let listener_config = notification_listener::NotificationListenerConfig {
            enabled: config.notification_listener.enabled,
            excluded_apps: config.notification_listener.excluded_apps.clone(),
            included_apps: config.notification_listener.included_apps.clone(),
            include_transient: config.notification_listener.include_transient,
            include_low_urgency: config.notification_listener.include_low_urgency,
            max_body_length: config.notification_listener.max_body_length,
        };

        match NotificationListener::new(listener_config, tx).await {
            Ok(listener) => {
                info!("Notification listener initialized successfully");

                // Store receiver for event loop
                let mut receiver_guard = self.notification_receiver.lock().await;
                *receiver_guard = Some(rx);
                drop(receiver_guard);
                drop(config);

                // Spawn DBus monitoring task
                // Note: listener is moved into the task, so we don't store it
                tokio::spawn(async move {
                    if let Err(e) = listener.listen().await {
                        error!("Notification listener error: {}", e);
                    }
                });

                // Spawn notification forwarding task
                let device_manager = self.device_manager.clone();
                let plugin_manager = self.plugin_manager.clone();
                let connection_manager = self.connection_manager.clone();
                let notification_receiver_mutex = self.notification_receiver.clone();

                tokio::spawn(async move {
                    let mut receiver_guard = notification_receiver_mutex.lock().await;
                    let Some(mut receiver) = receiver_guard.take() else {
                        return;
                    };
                    drop(receiver_guard);

                    info!("Notification forwarding task started");

                    while let Some(notification) = receiver.recv().await {
                        debug!(
                            "Captured notification: app={}, summary={}, body={}",
                            notification.app_name,
                            notification.summary,
                            notification.body.chars().take(50).collect::<String>()
                        );

                        // Get list of paired and connected devices
                        let devices = {
                            let dev_manager = device_manager.read().await;
                            dev_manager
                                .devices()
                                .filter(|d| d.is_paired() && d.is_connected())
                                .map(|d| d.id().to_string())
                                .collect::<Vec<_>>()
                        };

                        if devices.is_empty() {
                            trace!("No paired+connected devices to forward notification to");
                            continue;
                        }

                        // Actions are already in the correct format
                        let actions = &notification.actions;

                        // Process notification image if present
                        let image_bytes = Self::process_notification_image(&notification).await;

                        // Create notification packet using NotificationPlugin
                        use cosmic_connect_protocol::plugins::notification::{
                            NotificationPlugin, NotificationUrgency,
                        };

                        // Map urgency from notification hints
                        let urgency = Some(NotificationUrgency::from_byte(notification.urgency()));

                        let packet = NotificationPlugin::create_desktop_notification_packet(
                            &notification.app_name,
                            &notification.summary,
                            &notification.body,
                            notification.timestamp as i64,
                            image_bytes.as_deref(),
                            actions,
                            urgency,
                            notification.category(),
                            None, // app_icon - could be enhanced later
                        );

                        // Forward to each device that supports notifications
                        for device_id in &devices {
                            // Check if device supports notification capability
                            let supports_notifications = {
                                let plug_manager = plugin_manager.read().await;
                                plug_manager
                                    .get_device_plugin(device_id, "cconnect.notification")
                                    .is_some()
                            };

                            if !supports_notifications {
                                trace!(
                                    "Device {} does not support notifications, skipping",
                                    device_id
                                );
                                continue;
                            }

                            // Send packet
                            let conn_manager = connection_manager.read().await;
                            if let Err(e) = conn_manager.send_packet(device_id, &packet).await {
                                error!(
                                    "Failed to forward notification to device {}: {}",
                                    device_id, e
                                );
                            } else {
                                debug!("Forwarded notification to device {}", device_id);
                            }
                        }
                    }

                    info!("Notification forwarding task stopped");
                });

                info!("Notification listener started successfully");
            }
            Err(e) => {
                warn!("Failed to initialize notification listener: {}", e);
                warn!("Desktop notification forwarding will be disabled");
            }
        }

        Ok(())
    }

    /// Process notification image from captured notification
    ///
    /// Attempts to extract and process an image from the notification, trying:
    /// 1. Raw image data from `image-data` hint
    /// 2. Image file path from `image-path` hint
    ///
    /// Returns PNG-encoded image bytes if successful, None otherwise.
    async fn process_notification_image(notification: &CapturedNotification) -> Option<Vec<u8>> {
        use crate::notification_image::NotificationImage;

        // Try raw image data first
        if let Some(image_data) = notification.image_data() {
            match NotificationImage::from_image_data(image_data) {
                Ok(img) => match img.to_png() {
                    Ok(png_bytes) => {
                        debug!(
                            "Processed notification image from image-data: {} bytes",
                            png_bytes.len()
                        );
                        return Some(png_bytes);
                    }
                    Err(e) => {
                        warn!("Failed to convert image to PNG: {}", e);
                    }
                },
                Err(e) => {
                    warn!("Failed to process image-data: {}", e);
                }
            }
        }

        // Try image path as fallback
        if let Some(image_path) = notification.image_path() {
            match NotificationImage::from_path(image_path) {
                Ok(img) => match img.to_png() {
                    Ok(png_bytes) => {
                        debug!(
                            "Processed notification image from path {}: {} bytes",
                            image_path,
                            png_bytes.len()
                        );
                        return Some(png_bytes);
                    }
                    Err(e) => {
                        warn!("Failed to convert image from path to PNG: {}", e);
                    }
                },
                Err(e) => {
                    trace!("Failed to load image from path {}: {}", image_path, e);
                }
            }
        }

        None
    }

    /// Handle a connection event
    #[allow(clippy::too_many_arguments)]
    async fn handle_connection_event(
        event: ConnectionEvent,
        device_manager: &Arc<RwLock<DeviceManager>>,
        plugin_manager: &Arc<RwLock<PluginManager>>,
        connection_mgr: &Arc<RwLock<ConnectionManager>>,
        device_config_registry: &Arc<RwLock<device_config::DeviceConfigRegistry>>,
        pairing_service: &Option<Arc<RwLock<PairingService>>>,
        dbus_server: &Option<Arc<DbusServer>>,
        cosmic_notifier: &Option<Arc<cosmic_notifications::CosmicNotifier>>,
        mpris_manager: &Option<Arc<mpris_manager::MprisManager>>,
        dump_packets: bool,
        packet_sender: Sender<(String, Packet)>,
        config: &Arc<RwLock<Config>>,
        error_handler: &Option<Arc<ErrorHandler>>,
        tls_config: &Arc<cosmic_connect_protocol::TlsConfig>,
    ) -> Result<()> {
        match event {
            ConnectionEvent::Connected {
                device_id,
                remote_addr,
            } => {
                info!("Device {} connected from {}", device_id, remote_addr);

                // Get device name for notifications
                let _device_name = {
                    let dev_manager = device_manager.read().await;
                    dev_manager
                        .get_device(&device_id)
                        .map(|d| d.name().to_string())
                };

                // Initialize per-device plugins (only for paired devices)
                {
                    let dev_manager = device_manager.read().await;
                    if let Some(device) = dev_manager.get_device(&device_id) {
                        // Only initialize plugins for paired/trusted devices
                        if device.is_paired() {
                            let mut plug_manager = plugin_manager.write().await;
                            if let Err(e) = plug_manager
                                .init_device_plugins(&device_id, device, packet_sender.clone())
                                .await
                            {
                                error!(
                                    "Failed to initialize plugins for device {}: {}",
                                    device_id, e
                                );
                            } else {
                                info!("Initialized plugins for device {}", device_id);

                                // Set TLS config on SharePlugin for secure file transfers
                                if let Some(plugin) =
                                    plug_manager.get_device_plugin_mut(&device_id, "share")
                                {
                                    use cosmic_connect_protocol::plugins::share::SharePlugin;
                                    if let Some(share_plugin) =
                                        plugin.as_any_mut().downcast_mut::<SharePlugin>()
                                    {
                                        share_plugin.set_tls_config(tls_config.clone());
                                        debug!(
                                            "Set TLS config on SharePlugin for device {}",
                                            device_id
                                        );
                                    }
                                }

                                // Load MAC address from config and set it on WOL plugin
                                let config_registry = device_config_registry.read().await;
                                if let Some(device_config) = config_registry.get(&device_id) {
                                    if let Some(mac_address) = device_config.get_mac_address() {
                                        use cosmic_connect_protocol::plugins::wol::WolPlugin;
                                        if let Some(wol_plugin) =
                                            plug_manager.get_device_plugin_mut(&device_id, "wol")
                                        {
                                            if let Some(wol) =
                                                wol_plugin.as_any_mut().downcast_mut::<WolPlugin>()
                                            {
                                                info!(
                                                    "Loading saved MAC address {} for device {}",
                                                    mac_address, device_id
                                                );
                                                wol.set_mac_address(mac_address);
                                            }
                                        }
                                    }
                                }

                                // Initialize Contacts plugin database and signals
                                if let Some(contacts_plugin) =
                                    plug_manager.get_device_plugin_mut(&device_id, "contacts")
                                {
                                    if let Some(contacts) = contacts_plugin
                                        .as_any_mut()
                                        .downcast_mut::<ContactsPlugin>()
                                    {
                                        // Get data dir from config
                                        let config = config.read().await;
                                        let db_path = config
                                            .paths
                                            .data_dir
                                            .join(format!("contacts_{}.db", device_id));
                                        drop(config);

                                        if let Some(db_path_str) = db_path.to_str() {
                                            if let Err(e) =
                                                contacts.init_database(db_path_str).await
                                            {
                                                error!(
                                                    "Failed to init contacts DB for device {}: {}",
                                                    device_id, e
                                                );
                                            } else {
                                                info!(
                                                    "Contacts DB initialized for device {}",
                                                    device_id
                                                );
                                            }
                                        }

                                        if let Err(e) = contacts.init_signals().await {
                                            error!(
                                                "Failed to init contacts signals for device {}: {}",
                                                device_id, e
                                            );
                                        } else {
                                            info!(
                                                "Contacts signals initialized for device {}",
                                                device_id
                                            );
                                        }
                                    }
                                }
                            }
                        } else {
                            info!(
                                "Device {} connected but not paired - plugins not initialized",
                                device_id
                            );
                        }
                    } else {
                        warn!(
                            "Cannot initialize plugins - device {} not found in device manager",
                            device_id
                        );
                    }
                }

                // Emit DBus signal for device state changed
                if let Some(dbus) = dbus_server {
                    if let Err(e) = dbus
                        .emit_device_state_changed(&device_id, "connected")
                        .await
                    {
                        warn!("Failed to emit DeviceStateChanged signal: {}", e);
                    }
                }

                // Show COSMIC notification for device connection
                // TEMPORARILY DISABLED TO REDUCE SPAM
                // if let Some(notifier) = cosmic_notifier {
                //     if let Some(name) = device_name {
                //         if let Err(e) = notifier.notify_device_connected(&name).await {
                //             warn!("Failed to send device connected notification: {}", e);
                //         }
                //     }
                // }
            }
            ConnectionEvent::Disconnected {
                device_id,
                reason,
                reconnect,
            } => {
                info!(
                    "Device {} disconnected (reason: {:?}, reconnect: {})",
                    device_id, reason, reconnect
                );

                // Get device name for notifications
                let _device_name = {
                    let dev_manager = device_manager.read().await;
                    dev_manager
                        .get_device(&device_id)
                        .map(|d| d.name().to_string())
                };

                // Cleanup per-device plugins ONLY if not a socket replacement
                if !reconnect {
                    let mut plug_manager = plugin_manager.write().await;
                    if let Err(e) = plug_manager.cleanup_device_plugins(&device_id).await {
                        error!("Failed to cleanup plugins for device {}: {}", device_id, e);
                    } else {
                        info!("Cleaned up plugins for device {}", device_id);
                    }
                } else {
                    info!(
                        "Socket replacement for {} - preserving plugin state",
                        device_id
                    );
                }

                // Emit DBus signal for device state changed
                if let Some(dbus) = dbus_server {
                    if let Err(e) = dbus.emit_device_state_changed(&device_id, "disconnected").await {
                        warn!("Failed to emit DeviceStateChanged signal: {}", e);
                    }
                }

                // Show COSMIC notification for device disconnection
                // TEMPORARILY DISABLED TO REDUCE SPAM
                // if let Some(notifier) = cosmic_notifier {
                //     if let Some(name) = device_name {
                //         if let Err(e) = notifier.notify_device_disconnected(&name).await {
                //             warn!("Failed to send device disconnected notification: {}", e);
                //         }
                //     }
                // }
            }
            ConnectionEvent::PacketReceived {
                device_id,
                packet,
                remote_addr,
            } => {
                debug!(
                    "Received packet '{}' from device {} at {}",
                    packet.packet_type, device_id, remote_addr
                );

                // Dump packet contents if enabled
                if dump_packets {
                    match serde_json::to_string_pretty(&packet) {
                        Ok(json) => {
                            debug!(
                                "📨 PACKET DUMP (RX) - Type: {}, Device: {}, Size: {} bytes\n{}",
                                packet.packet_type,
                                device_id,
                                json.len(),
                                json
                            );
                        }
                        Err(e) => {
                            warn!("Failed to serialize packet for dumping: {}", e);
                        }
                    }
                }

                // Handle special protocol packets BEFORE routing to plugins
                match packet.packet_type.as_str() {
                    "cconnect.identity" => {
                        // Protocol v8: Post-TLS identity exchange - ignore for now
                        // This is the second identity packet sent after TLS encryption
                        // In protocol v8, devices exchange identity packets again after TLS
                        debug!(
                            "Received post-TLS identity packet from {} (protocol v8)",
                            device_id
                        );
                        return Ok(());
                    }
                    "cconnect.pair" => {
                        info!(
                            "Received pairing packet from {} at {}",
                            device_id, remote_addr
                        );

                        let Some(pairing_svc) = pairing_service else {
                            warn!("Received pairing packet but pairing service is not available");
                            return Ok(());
                        };

                        // Get device info and certificate from device manager
                        let (device_info, device_cert) = {
                            let dev_manager = device_manager.read().await;
                            match dev_manager.get_device(&device_id) {
                                Some(device) => (
                                    device.info.clone(),
                                    device.certificate_data.clone().unwrap_or_default(),
                                ),
                                None => {
                                    warn!(
                                        "Cannot handle pairing packet - device {} not found",
                                        device_id
                                    );
                                    return Ok(());
                                }
                            }
                        };

                        // Forward to pairing service and send response if needed
                        let pairing = pairing_svc.read().await;
                        match pairing
                            .handle_pairing_packet(&packet, &device_info, &device_cert, remote_addr)
                            .await
                        {
                            Ok(Some(response_packet)) => {
                                info!(
                                    "Sending pairing response to {} through existing connection",
                                    device_id
                                );
                                let mgr = connection_mgr.read().await;
                                if let Err(e) = mgr.send_packet(&device_id, &response_packet).await
                                {
                                    error!(
                                        "Failed to send pairing response to {}: {}",
                                        device_id, e
                                    );
                                }
                            }
                            Ok(None) => {
                                debug!("No pairing response needed for {}", device_id);
                            }
                            Err(e) => {
                                error!("Failed to handle pairing packet from {}: {}", device_id, e);
                            }
                        }
                        return Ok(());
                    }
                    _ => {
                        // Regular packet - route to plugin manager
                    }
                }

                // Get device from device manager
                let mut dev_manager = device_manager.write().await;
                if let Some(device) = dev_manager.get_device_mut(&device_id) {
                    let device_name = device.name().to_string();

                    // Route packet to plugin manager
                    let mut plug_manager = plugin_manager.write().await;
                    if let Err(e) = plug_manager
                        .handle_packet(&device_id, &packet, device)
                        .await
                    {
                        error!("Error handling packet from device {}: {}", device_id, e);
                        if let Some(handler) = error_handler {
                            handler
                                .handle_error(&e, "plugin_packet", Some(&device_id))
                                .await;
                        }
                    }

                    // Handle camera frame payload reception (Issue #139)
                    // When Android sends a camera frame, it includes payloadTransferInfo with a port
                    // We need to connect to that port with TLS, download the frame data, and pass it to the camera plugin
                    #[cfg(feature = "video")]
                    if packet.packet_type == "cconnect.camera.frame" {
                        if let (Some(payload_info), Some(payload_size)) = (
                            packet.payload_transfer_info.as_ref(),
                            packet.payload_size,
                        ) {
                        use cosmic_connect_protocol::plugins::camera::CameraPlugin;
                        use tokio_rustls::TlsConnector;

                        // Extract port from payloadTransferInfo
                        if let Some(port_value) = payload_info.get("port") {
                            if let Some(port) = port_value.as_u64() {
                                debug!(
                                    "Receiving camera frame payload: {} bytes from {}:{}",
                                    payload_size,
                                    remote_addr.ip(),
                                    port
                                );

                                // Spawn task to receive payload without blocking the event loop
                                let device_id_clone = device_id.clone();
                                let packet_clone = packet.clone();
                                let plugin_manager_clone = plugin_manager.clone();
                                let remote_ip = remote_addr.ip();
                                let tls_config_clone = tls_config.clone();

                                tokio::spawn(async move {
                                    // Connect to payload port on Android device
                                    let payload_addr =
                                        std::net::SocketAddr::new(remote_ip, port as u16);

                                    // Android uses TRADITIONAL TLS roles for payload transfers (not inverted):
                                    // - TCP initiator (us) acts as TLS CLIENT
                                    // - TCP acceptor (Android) acts as TLS SERVER
                                    // This differs from the main connection which uses inverted roles.
                                    match tokio::net::TcpStream::connect(payload_addr).await {
                                        Ok(tcp_stream) => {
                                            debug!(
                                                "TCP connected to payload port {} for camera frame",
                                                payload_addr
                                            );

                                            // Perform TLS handshake as CLIENT (traditional role for payload transfers)
                                            let connector = TlsConnector::from(
                                                tls_config_clone.client_config(),
                                            );
                                            let server_name = tokio_rustls::rustls::pki_types::ServerName::try_from("kdeconnect")
                                                .expect("invalid server name");

                                            match connector.connect(server_name, tcp_stream).await {
                                                Ok(mut tls_stream) => {
                                                    debug!("TLS handshake complete for camera frame payload");

                                                    // Read payload over TLS
                                                    use tokio::io::AsyncReadExt;

                                                    let mut payload =
                                                        vec![0u8; payload_size as usize];
                                                    match tls_stream.read_exact(&mut payload).await
                                                    {
                                                        Ok(_) => {
                                                            debug!("Received complete camera frame payload: {} bytes over TLS", payload_size);

                                                            // Pass payload to camera plugin
                                                            let plug_manager =
                                                                plugin_manager_clone.write().await;
                                                            if let Some(camera_plugin) =
                                                                plug_manager.get_device_plugin(
                                                                    &device_id_clone,
                                                                    "camera",
                                                                )
                                                            {
                                                                if let Some(camera) = camera_plugin
                                                                    .as_any()
                                                                    .downcast_ref::<CameraPlugin>()
                                                                {
                                                                    if let Err(e) = camera.process_camera_frame_payload(&packet_clone, payload).await {
                                                                        error!("Failed to process camera frame payload: {}", e);
                                                                    } else {
                                                                        debug!("Camera frame payload processed successfully");
                                                                    }
                                                                } else {
                                                                    error!("Camera plugin not initialized for device {}", device_id_clone);
                                                                }
                                                            } else {
                                                                error!("Failed to get camera plugin for device {}", device_id_clone);
                                                            }
                                                        }
                                                        Err(e) => {
                                                            error!("Failed to receive camera frame payload ({} bytes) over TLS: {}", payload_size, e);
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    error!("TLS handshake failed for camera frame payload: {}", e);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!(
                                                "Failed to connect to payload port {}: {}",
                                                payload_addr, e
                                            );
                                        }
                                    }
                                });
                            } else {
                                warn!("Camera frame payloadTransferInfo port is not a number");
                            }
                        } else {
                            warn!("Camera frame payloadTransferInfo missing port field");
                        }
                        }
                    }

                    // Save MAC address if WOL config packet was received
                    if packet.packet_type == "cconnect.wol.config" {
                        use cosmic_connect_protocol::plugins::wol::WolPlugin;

                        if let Some(wol_plugin) =
                            plug_manager.get_device_plugin_mut(&device_id, "wol")
                        {
                            if let Some(wol) = wol_plugin.as_any_mut().downcast_mut::<WolPlugin>() {
                                if let Some(mac_address) = wol.get_mac_address() {
                                    info!(
                                        "Persisting MAC address {} for device {}",
                                        mac_address, device_id
                                    );

                                    let mut config_registry = device_config_registry.write().await;
                                    let device_config = config_registry.get_or_create(&device_id);

                                    if let Err(e) = device_config.set_mac_address(mac_address) {
                                        error!("Failed to set MAC address: {}", e);
                                    } else {
                                        // Save config to disk
                                        if let Err(e) = config_registry.save() {
                                            error!("Failed to save device config: {}", e);
                                        } else {
                                            info!("MAC address saved to device config");
                                        }
                                    }
                                }
                            }
                        }
                    }

                    drop(plug_manager);
                    drop(dev_manager);

                    // Check device notification preference
                    let notification_pref = {
                        let config_registry = device_config_registry.read().await;
                        if let Some(config) = config_registry.get(&device_id) {
                            config.get_notification_preference()
                        } else {
                            device_config::NotificationPreference::All
                        }
                    };

                    // Send COSMIC notifications for specific packet types
                    if let Some(notifier) = &cosmic_notifier {
                        match packet.packet_type.as_str() {
                            "cconnect.ping" => {
                                // Skip notification for keepalive pings (silent pings)
                                let is_keepalive = packet
                                    .body
                                    .get("keepalive")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);

                                if !is_keepalive {
                                    // Show notification for regular pings only
                                    let message =
                                        packet.body.get("message").and_then(|v| v.as_str());

                                    if let Err(e) =
                                        notifier.notify_ping(&device_name, message).await
                                    {
                                        warn!("Failed to send ping notification: {}", e);
                                    }
                                } else {
                                    debug!("Received keepalive ping from {} - suppressing notification", device_name);
                                }
                            }
                            "cconnect.notification" => {
                                // Check if it's a cancel notification
                                let is_cancel = packet
                                    .body
                                    .get("isCancel")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);

                                if !is_cancel {
                                    // Check if notification is silent (preexisting)
                                    let is_silent = packet
                                        .body
                                        .get("silent")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s == "true")
                                        .unwrap_or(false);

                                    // Only show COSMIC notification for new notifications
                                    if !is_silent {
                                        let app_name = packet
                                            .body
                                            .get("appName")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        let title = packet
                                            .body
                                            .get("title")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("Notification");

                                        // Extract body text - prefer richBody over plain text
                                        let rich_body =
                                            packet.body.get("richBody").and_then(|v| v.as_str());
                                        let text = packet
                                            .body
                                            .get("text")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");

                                        // Check if it's a messaging app
                                        let is_messaging = packet
                                            .body
                                            .get("isMessagingApp")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false);

                                        // Apply notification filtering based on preference
                                        let should_show = match notification_pref {
                                            device_config::NotificationPreference::All => true,
                                            device_config::NotificationPreference::Important => {
                                                // Important includes messaging apps, calls, alarms
                                                is_messaging
                                                    || app_name.to_lowercase().contains("phone")
                                                    || app_name.to_lowercase().contains("call")
                                                    || app_name.to_lowercase().contains("alarm")
                                                    || app_name.to_lowercase().contains("clock")
                                            }
                                            device_config::NotificationPreference::None => false,
                                        };

                                        if should_show && is_messaging {
                                            let web_url =
                                                packet.body.get("webUrl").and_then(|v| v.as_str());

                                            if let Err(e) = notifier
                                                .notify_messaging(
                                                    &device_name,
                                                    app_name,
                                                    title,
                                                    text,
                                                    rich_body,
                                                    web_url,
                                                )
                                                .await
                                            {
                                                warn!(
                                                    "Failed to send messaging notification: {}",
                                                    e
                                                );
                                            }

                                            // Emit D-Bus signal for cosmic-messages
                                            if let Some(dbus) = &dbus_server {
                                                let conv_id = packet
                                                    .body
                                                    .get("conversationId")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("");
                                                if let Err(e) = dbus
                                                    .emit_messaging_notification(
                                                        app_name, title, text, conv_id,
                                                    )
                                                    .await
                                                {
                                                    warn!(
                                                        "Failed to emit messaging D-Bus signal: {}",
                                                        e
                                                    );
                                                }
                                            }
                                        } else if should_show {
                                            // Extract notification ID for rich notifications
                                            let notification_id = packet
                                                .body
                                                .get("id")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");

                                            // Issue #180: Check multiple image sources from Android
                                            // Priority order:
                                            // 1. imageData - Main notification image/large icon
                                            // 2. senderAvatar - For messaging notifications
                                            // 3. appIcon - Fallback to app icon
                                            debug!(
                                                "Notification image sources: imageData={}, senderAvatar={}, appIcon={}",
                                                packet.body.get("imageData").is_some(),
                                                packet.body.get("senderAvatar").is_some(),
                                                packet.body.get("appIcon").is_some()
                                            );

                                            // Helper closure to decode base64 image field
                                            let try_decode_image_field =
                                                |field: &str| -> Option<(Vec<u8>, i32, i32)> {
                                                    let base64_data =
                                                        packet.body.get(field)?.as_str()?;

                                                    // Decode base64 to bytes
                                                    let bytes = match general_purpose::STANDARD
                                                        .decode(base64_data)
                                                    {
                                                        Ok(b) => b,
                                                        Err(e) => {
                                                            debug!("Failed to decode base64 for {}: {}", field, e);
                                                            return None;
                                                        }
                                                    };

                                                    // Load as image to get dimensions
                                                    match image::load_from_memory(&bytes) {
                                                        Ok(img) => {
                                                            let width = img.width() as i32;
                                                            let height = img.height() as i32;
                                                            // Convert to RGBA8 and get raw bytes
                                                            let rgba = img.to_rgba8();
                                                            debug!(
                                                                "Decoded {} image: {}x{}",
                                                                field, width, height
                                                            );
                                                            Some((rgba.into_raw(), width, height))
                                                        }
                                                        Err(e) => {
                                                            debug!(
                                                                "Failed to decode image for {}: {}",
                                                                field, e
                                                            );
                                                            None
                                                        }
                                                    }
                                                };

                                            // Extract best available image (Issue #180)
                                            let image_bytes = try_decode_image_field("imageData")
                                                .or_else(|| try_decode_image_field("senderAvatar"))
                                                .or_else(|| try_decode_image_field("appIcon"));

                                            if let Some((_, w, h)) = &image_bytes {
                                                debug!("Using notification image: {}x{}", w, h);
                                            }

                                            // Send notification with or without image
                                            if image_bytes.is_some() {
                                                // Use rich notification with image
                                                if let Err(e) = notifier
                                                    .notify_rich_from_device(
                                                        notification_id,
                                                        &device_name,
                                                        app_name,
                                                        title,
                                                        text,
                                                        None, // rich_body
                                                        image_bytes,
                                                        Vec::new(), // links
                                                    )
                                                    .await
                                                {
                                                    warn!(
                                                        "Failed to send rich notification: {}",
                                                        e
                                                    );
                                                }
                                            } else {
                                                // Use simple notification without image
                                                if let Err(e) = notifier
                                                    .notify_from_device(
                                                        &device_name,
                                                        app_name,
                                                        title,
                                                        text,
                                                        None, // rich_body
                                                    )
                                                    .await
                                                {
                                                    warn!(
                                                        "Failed to send device notification: {}",
                                                        e
                                                    );
                                                }
                                            }
                                        } else {
                                            debug!(
                                                "Notification from {} filtered based on preference {:?}",
                                                device_name, notification_pref
                                            );
                                        }
                                    }
                                }
                            }
                            "cconnect.share.request" => {
                                // Handle different share types: file, URL, or text
                                if let Some(filename) =
                                    packet.body.get("filename").and_then(|v| v.as_str())
                                {
                                    if packet.payload_size.is_some() {
                                        // This is a file transfer, show notification
                                        let file_size = packet.payload_size.unwrap_or(0);

                                        // Construct download path
                                        let downloads_dir = std::path::PathBuf::from(
                                            std::env::var("HOME")
                                                .unwrap_or_else(|_| "/tmp".to_string()),
                                        )
                                        .join("Downloads");
                                        let file_path = downloads_dir.join(filename);

                                        if let Err(e) = notifier
                                            .notify_file_received(
                                                &device_name,
                                                filename,
                                                &file_path.to_string_lossy(),
                                            )
                                            .await
                                        {
                                            warn!(
                                                "Failed to send file received notification: {}",
                                                e
                                            );
                                        } else {
                                            info!(
                                                "Sent file received notification for '{}' ({} bytes) from {}",
                                                filename, file_size, device_name
                                            );
                                        }
                                    }
                                } else if let Some(url) =
                                    packet.body.get("url").and_then(|v| v.as_str())
                                {
                                    // URL share - open in default browser
                                    info!("Received URL share from {}: {}", device_name, url);

                                    // Spawn xdg-open to open URL in default browser
                                    let url_clone = url.to_string();
                                    let device_name_clone = device_name.clone();
                                    tokio::spawn(async move {
                                        match tokio::process::Command::new("xdg-open")
                                            .arg(&url_clone)
                                            .spawn()
                                        {
                                            Ok(_) => {
                                                info!(
                                                    "Opened URL from {} in default browser: {}",
                                                    device_name_clone, url_clone
                                                );
                                            }
                                            Err(e) => {
                                                warn!(
                                                    "Failed to open URL from {}: {}",
                                                    device_name_clone, e
                                                );
                                            }
                                        }
                                    });
                                } else if let Some(text) =
                                    packet.body.get("text").and_then(|v| v.as_str())
                                {
                                    // Text share - copy to clipboard
                                    info!(
                                        "Received text share from {} ({} chars)",
                                        device_name,
                                        text.len()
                                    );

                                    use arboard::Clipboard;
                                    match Clipboard::new() {
                                        Ok(mut clipboard) => {
                                            if let Err(e) = clipboard.set_text(text) {
                                                warn!(
                                                    "Failed to copy shared text to clipboard: {}",
                                                    e
                                                );
                                            } else {
                                                info!("Copied shared text from {} to clipboard ({} chars)",
                                                    device_name, text.len());
                                            }
                                        }
                                        Err(e) => {
                                            warn!(
                                                "Failed to initialize clipboard for text share: {}",
                                                e
                                            );
                                        }
                                    }
                                }
                            }
                            "cconnect.clipboard" | "kdeconnect.clipboard.connect" => {
                                // Update system clipboard with received content
                                if let Some(content) =
                                    packet.body.get("content").and_then(|v| v.as_str())
                                {
                                    if !content.is_empty() {
                                        use arboard::Clipboard;
                                        match Clipboard::new() {
                                            Ok(mut clipboard) => {
                                                if let Err(e) = clipboard.set_text(content) {
                                                    warn!(
                                                        "Failed to update system clipboard: {}",
                                                        e
                                                    );
                                                } else {
                                                    info!("Updated system clipboard from {} ({} chars)",
                                                        device_name, content.len());
                                                }
                                            }
                                            Err(e) => {
                                                warn!("Failed to initialize clipboard: {}", e);
                                            }
                                        }
                                    }
                                }
                            }
                            "cconnect.battery" => {
                                // Handle battery status updates - show notification for low battery
                                if let Some(charge) =
                                    packet.body.get("currentCharge").and_then(|v| v.as_i64())
                                {
                                    let is_charging = packet
                                        .body
                                        .get("isCharging")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false);
                                    let threshold_event = packet
                                        .body
                                        .get("thresholdEvent")
                                        .and_then(|v| v.as_i64())
                                        .unwrap_or(0);

                                    // threshold_event == 1 means low battery
                                    if threshold_event == 1 && !is_charging {
                                        info!(
                                            "Low battery detected on {} ({}%)",
                                            device_name, charge
                                        );

                                        if let Err(e) = notifier
                                            .notify_battery_low(
                                                &device_name,
                                                charge.clamp(0, 100) as u8,
                                            )
                                            .await
                                        {
                                            warn!("Failed to send low battery notification: {}", e);
                                        } else {
                                            info!(
                                                "Sent low battery notification for {} ({}%)",
                                                device_name, charge
                                            );
                                        }
                                    } else {
                                        debug!(
                                            "Battery status from {}: {}% (charging: {})",
                                            device_name, charge, is_charging
                                        );
                                    }
                                }
                            }
                            "cconnect.mpris.request" => {
                                if let Some(mpris_mgr) = &mpris_manager {
                                    Self::handle_mpris_request(
                                        &packet.body,
                                        mpris_mgr,
                                        &device_id,
                                        &device_name,
                                        connection_mgr,
                                        plugin_manager,
                                    )
                                    .await;
                                }
                            }
                            _ => {
                                // Other packet types don't trigger notifications
                            }
                        }
                    }
                } else {
                    warn!("Received packet from unknown device: {}", device_id);
                }
            }
            ConnectionEvent::ConnectionError { device_id, message } => {
                error!("Connection error for device {:?}: {}", device_id, message);
                if let Some(handler) = error_handler {
                    let error = cosmic_connect_protocol::ProtocolError::NetworkError(message);
                    handler
                        .handle_error(&error, "connection", device_id.as_deref())
                        .await;
                }
            }
            ConnectionEvent::ManagerStarted { port } => {
                info!("Connection manager started on port {}", port);
            }
            ConnectionEvent::ManagerStopped => {
                info!("Connection manager stopped");
            }
        }
        Ok(())
    }

    /// Convert MprisManager PlayerState to protocol types for CConnect
    fn convert_player_state(
        state: &mpris_manager::PlayerState,
    ) -> (
        cosmic_connect_protocol::plugins::mpris::PlayerStatus,
        cosmic_connect_protocol::plugins::mpris::PlayerMetadata,
    ) {
        use cosmic_connect_protocol::plugins::mpris::{
            LoopStatus, PlayerCapabilities, PlayerMetadata, PlayerStatus,
        };

        // Convert loop status
        let loop_status = match state.loop_status {
            mpris_manager::LoopStatus::None => LoopStatus::None,
            mpris_manager::LoopStatus::Track => LoopStatus::Track,
            mpris_manager::LoopStatus::Playlist => LoopStatus::Playlist,
        };

        // Convert capabilities
        let capabilities = PlayerCapabilities {
            can_play: state.can_play,
            can_pause: state.can_pause,
            can_go_next: state.can_go_next,
            can_go_previous: state.can_go_previous,
            can_seek: state.can_seek,
        };

        // Create status (convert microseconds to milliseconds, volume to 0-100)
        let status = PlayerStatus {
            is_playing: state.playback_status.is_playing(),
            position: state.position / 1000, // microseconds to milliseconds
            length: state.metadata.length / 1000, // microseconds to milliseconds
            volume: (state.volume * 100.0) as i32, // 0.0-1.0 to 0-100
            loop_status,
            shuffle: state.shuffle,
            capabilities,
        };

        // Create metadata
        let metadata = PlayerMetadata {
            artist: state.metadata.artist.clone(),
            title: state.metadata.title.clone(),
            album: state.metadata.album.clone(),
            album_art_url: state.metadata.album_art_url.clone(),
        };

        (status, metadata)
    }

    /// Handle MPRIS control requests from a remote device
    async fn handle_mpris_request(
        body: &serde_json::Value,
        mpris_manager: &Arc<mpris_manager::MprisManager>,
        device_id: &str,
        device_name: &str,
        connection_mgr: &Arc<RwLock<ConnectionManager>>,
        plugin_manager: &Arc<RwLock<PluginManager>>,
    ) {
        use cosmic_connect_protocol::plugins::mpris::MprisPlugin;

        let player = body.get("player").and_then(|v| v.as_str()).unwrap_or("");

        // Helper to send MPRIS packets via the plugin
        let send_mpris_packet = |packet: cosmic_connect_protocol::Packet| async move {
            let conn_manager = connection_mgr.read().await;
            if let Err(e) = conn_manager.send_packet(device_id, &packet).await {
                warn!("Failed to send MPRIS packet to {}: {}", device_name, e);
            }
        };

        // Handle player list request (does not require a player name)
        if body.get("requestPlayerList").is_some() {
            info!("Received player list request from {}", device_name);

            let players = match mpris_manager.discover_players().await {
                Ok(p) => p,
                Err(e) => {
                    warn!("Failed to discover players for {}: {}", device_name, e);
                    return;
                }
            };

            info!("Sending player list to {}: {:?}", device_name, players);

            let plug_manager = plugin_manager.read().await;
            if let Some(mpris_plugin) = plug_manager
                .get_device_plugin(device_id, "mpris")
                .and_then(|p| p.as_any().downcast_ref::<MprisPlugin>())
            {
                let packet = mpris_plugin.create_player_list_packet(players);
                drop(plug_manager);
                send_mpris_packet(packet).await;
                info!("Sent player list to {}", device_name);
            }
            return;
        }

        if player.is_empty() {
            debug!("Received MPRIS request without player name");
            return;
        }

        // Playback control action (Play, Pause, PlayPause, Stop, Next, Previous)
        if let Some(action) = body.get("action").and_then(|v| v.as_str()) {
            match mpris_manager.call_player_method(player, action).await {
                Ok(()) => info!(
                    "Executed MPRIS action '{}' on {} from {}",
                    action, player, device_name
                ),
                Err(e) => warn!("Failed MPRIS action '{}' on {}: {}", action, player, e),
            }
            return;
        }

        // Seek operation (offset in microseconds)
        if let Some(offset) = body.get("Seek").and_then(|v| v.as_i64()) {
            match mpris_manager.seek(player, offset).await {
                Ok(()) => info!("Seeked {}us on {} from {}", offset, player, device_name),
                Err(e) => warn!("Failed to seek {}us on {}: {}", offset, player, e),
            }
            return;
        }

        // Set absolute position (milliseconds from protocol, convert to microseconds)
        if let Some(position_ms) = body.get("SetPosition").and_then(|v| v.as_i64()) {
            let position_us = position_ms * 1000;
            // Note: MPRIS SetPosition requires a track ID, but most players work with NoTrack.
            // To properly support this, we would need to cache mpris:trackid from Metadata.
            // For now, NoTrack works with common players (VLC, Spotify, etc.).
            let track_id = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
            match mpris_manager
                .set_position(player, track_id, position_us)
                .await
            {
                Ok(()) => info!(
                    "Set position to {}ms on {} from {}",
                    position_ms, player, device_name
                ),
                Err(e) => warn!(
                    "Failed to set position to {}ms on {}: {}",
                    position_ms, player, e
                ),
            }
            return;
        }

        // Set volume (0-100 from protocol, convert to 0.0-1.0 for MPRIS)
        if let Some(volume) = body.get("setVolume").and_then(|v| v.as_i64()) {
            let volume_normalized = (volume as f64) / 100.0;
            match mpris_manager.set_volume(player, volume_normalized).await {
                Ok(()) => info!(
                    "Set volume to {}% on {} from {}",
                    volume, player, device_name
                ),
                Err(e) => warn!("Failed to set volume to {}% on {}: {}", volume, player, e),
            }
            return;
        }

        // Set loop status
        if let Some(loop_str) = body.get("setLoopStatus").and_then(|v| v.as_str()) {
            let loop_status = mpris_manager::LoopStatus::from_str(loop_str);
            match mpris_manager.set_loop_status(player, loop_status).await {
                Ok(()) => info!(
                    "Set loop status to {} on {} from {}",
                    loop_str, player, device_name
                ),
                Err(e) => warn!(
                    "Failed to set loop status to {} on {}: {}",
                    loop_str, player, e
                ),
            }
            return;
        }

        // Set shuffle
        if let Some(shuffle) = body.get("setShuffle").and_then(|v| v.as_bool()) {
            match mpris_manager.set_shuffle(player, shuffle).await {
                Ok(()) => info!(
                    "Set shuffle to {} on {} from {}",
                    shuffle, player, device_name
                ),
                Err(e) => warn!("Failed to set shuffle to {} on {}: {}", shuffle, player, e),
            }
            return;
        }

        // Request for now playing state
        if body.get("requestNowPlaying").is_some() {
            info!(
                "Received now playing request for {} from {}",
                player, device_name
            );

            let state = match mpris_manager.query_player_state(player).await {
                Ok(s) => s,
                Err(e) => {
                    warn!("Failed to query player {} state: {}", player, e);
                    return;
                }
            };

            let (status, metadata) = Self::convert_player_state(&state);

            let plug_manager = plugin_manager.read().await;
            if let Some(mpris_plugin) = plug_manager
                .get_device_plugin(device_id, "mpris")
                .and_then(|p| p.as_any().downcast_ref::<MprisPlugin>())
            {
                let packet =
                    mpris_plugin.create_status_packet(player.to_string(), status, metadata);
                drop(plug_manager);
                send_mpris_packet(packet).await;
                info!("Sent player state for {} to {}", player, device_name);
            }
        }
    }

    /// Handle a discovery event
    async fn handle_discovery_event(
        event: DiscoveryEvent,
        device_manager: &Arc<RwLock<DeviceManager>>,
        dbus_server: &Option<Arc<DbusServer>>,
        _error_handler: &ErrorHandler,
        connection_manager: &Arc<RwLock<ConnectionManager>>,
        connection_attempts: &Arc<
            RwLock<std::collections::HashMap<String, (std::time::Instant, u32)>>,
        >,
    ) -> Result<()> {
        match event {
            DiscoveryEvent::DeviceDiscovered {
                ref info,
                ref transport_address,
                ..
            }
            | DiscoveryEvent::DeviceUpdated {
                ref info,
                ref transport_address,
                ..
            } => {
                let device_id = info.device_id.clone();

                // Update registry
                {
                    let mut manager = device_manager.write().await;
                    manager.update_from_discovery(info.clone(), transport_address.clone());
                    if let Err(e) = manager.save_registry() {
                        warn!("Failed to save device registry: {}", e);
                    }
                }

                // Auto-connect if paired
                let should_connect = {
                    let manager = device_manager.read().await;
                    if let Some(device) = manager.get_device(&device_id) {
                        device.is_paired() && !device.is_connected()
                    } else {
                        false
                    }
                };

                if should_connect {
                    // Check backoff
                    let mut attempts = connection_attempts.write().await;
                    let now = std::time::Instant::now();
                    let (last_attempt, count) = attempts
                        .entry(device_id.clone())
                        .or_insert((now - Duration::from_secs(3600), 0));

                    // Exponential backoff: 2^count seconds (cap at 60s)
                    let backoff = Duration::from_secs(2u64.pow((*count).min(6)));

                    if now.duration_since(*last_attempt) >= backoff {
                        info!(
                            "Auto-connecting to trusted device {} (attempt {})",
                            device_id, count
                        );
                        *last_attempt = now;
                        *count += 1;

                        // Try to connect
                        let _mgr = connection_manager.read().await;
                        // Need to extract SocketAddr from TransportAddress if it's TCP
                        // DiscoveryService usually returns TransportAddress::Tcp for UDP discovery results
                        if let cosmic_connect_protocol::transport::TransportAddress::Tcp(addr) =
                            transport_address
                        {
                            let device_id_clone = device_id.clone();
                            let socket_addr = *addr;

                            let mgr_arc = connection_manager.clone();
                            tokio::spawn(async move {
                                let mgr = mgr_arc.read().await;
                                if let Err(e) = mgr.connect(&device_id_clone, socket_addr).await {
                                    warn!("Failed to auto-connect to {}: {}", device_id_clone, e);
                                }
                            });
                        }
                    }
                } else if !should_connect {
                    // Reset attempts if connected or not paired
                    let manager = device_manager.read().await;
                    if let Some(device) = manager.get_device(&device_id) {
                        if device.is_connected() {
                            let mut attempts = connection_attempts.write().await;
                            attempts.remove(&device_id);
                        }
                    }
                }

                // Emit DBus signal for device added (only on discovery, logic handles updated)
                if matches!(event, DiscoveryEvent::DeviceDiscovered { .. }) {
                    if let Some(dbus) = dbus_server {
                        let manager = device_manager.read().await;
                        if let Some(device) = manager.get_device(&device_id) {
                            if let Err(e) = dbus.emit_device_added(device).await {
                                warn!("Failed to emit DeviceAdded signal: {}", e);
                            }
                        }
                    }
                }
            }
            DiscoveryEvent::DeviceTimeout { device_id } => {
                info!("Device timed out: {}", device_id);
                // We don't mark as disconnected here, ConnectionManager handles TCP timeout.
                // But we can emit a signal if needed.
                if let Some(dbus) = dbus_server {
                    if let Err(e) = dbus
                        .emit_device_state_changed(&device_id, "reachable")
                        .await
                    {
                        // Just updating reachability state logic handled in list_devices mostly
                        warn!("Failed to emit state change: {}", e);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Run the daemon
    async fn run(&self) -> Result<()> {
        info!("CConnect daemon running");
        info!(
            "Device: {} ({})",
            self.device_info.device_name, self.device_info.device_id
        );
        info!("Type: {:?}", self.device_info.device_type);
        info!("Protocol version: {}", self.device_info.protocol_version);

        // Spawn proactive packet handler
        let packet_receiver_mutex = self.packet_receiver.clone();
        let connection_manager = self.connection_manager.clone();
        let dbus_server = self.dbus_server.clone();

        tokio::spawn(async move {
            let mut receiver_guard = packet_receiver_mutex.lock().await;
            let Some(mut receiver) = receiver_guard.take() else {
                return;
            };
            drop(receiver_guard); // Release mutex

            info!("Started proactive packet handler");
            while let Some((device_id, packet)) = receiver.recv().await {
                // Handle internal signaling packets for DBus emission
                let handled = if let Some(dbus) = &dbus_server {
                    handle_internal_packet(dbus, &device_id, &packet).await
                } else {
                    false
                };

                // Forward non-internal packets to the connection manager
                if !handled {
                    let manager = connection_manager.read().await;
                    if let Err(e) = manager.send_packet(&device_id, &packet).await {
                        error!("Failed to send proactive packet to {}: {}", device_id, e);
                    }
                }
            }
            info!("Proactive packet handler stopped");
        });

        // Get capabilities from plugin manager
        let manager = self.plugin_manager.read().await;
        let incoming = manager.get_all_incoming_capabilities();
        let outgoing = manager.get_all_outgoing_capabilities();

        info!("Incoming capabilities: {}", incoming.len());
        for cap in &incoming {
            info!("  - {}", cap);
        }

        info!("Outgoing capabilities: {}", outgoing.len());
        for cap in &outgoing {
            info!("  - {}", cap);
        }

        drop(manager);

        // Display device manager status
        let device_manager = self.device_manager.read().await;
        info!(
            "Device registry: {} devices loaded",
            device_manager.device_count()
        );
        info!("  - Paired devices: {}", device_manager.paired_count());
        info!(
            "  - Connected devices: {}",
            device_manager.connected_count()
        );

        // Create desktop icons for all paired devices at startup
        let paired_count = device_manager.paired_count();
        if paired_count > 0 {
            info!("Creating desktop icons for {} paired devices...", paired_count);
            for device in device_manager.paired_devices() {
                if let Err(e) = desktop_icons::sync_desktop_icon(device, None) {
                    warn!(
                        "Failed to create desktop icon for device {}: {}",
                        device.info.device_id, e
                    );
                } else {
                    debug!("Created desktop icon for device {}", device.info.device_id);
                }
            }
            info!("Desktop icons created for paired devices");
        }

        drop(device_manager);

        info!("Daemon initialized successfully");
        info!("Press Ctrl+C to stop");

        // Wait for shutdown signal (SIGINT or SIGTERM)
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = signal(SignalKind::terminate())
            .context("Failed to create SIGTERM handler")?;

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Received SIGINT (Ctrl+C)");
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM");
            }
        }

        Ok(())
    }

    /// Enable performance metrics collection
    fn enable_metrics(&mut self) {
        let metrics = Arc::new(RwLock::new(Metrics::new()));
        info!("Performance metrics enabled");
        self.metrics = Some(metrics);
    }

    /// Enable packet dumping (debug mode)
    fn enable_packet_dumping(&mut self) {
        self.dump_packets = true;
        info!("Packet dumping enabled (debug mode)");
        warn!("Packet dumping generates large log output - use only for debugging");
    }

    /// Shutdown the daemon
    async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down daemon...");

        let shutdown_result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            async {
                // Stop discovery service
                if let Some(mut discovery) = self.discovery_service.take() {
                    let _ = discovery.stop().await;
                }

                // Stop transport manager or connection manager
                if let Some(transport_mgr) = &self.transport_manager {
                    info!("Stopping TransportManager...");
                    transport_mgr.stop().await;
                } else {
                    // Stop connection manager directly if no TransportManager
                    let connection_manager = self.connection_manager.write().await;
                    connection_manager.stop().await;
                }

                // Drop DBus server (connection will be closed automatically)
                if let Some(_dbus) = self.dbus_server.take() {
                    info!("DBus server stopped");
                }

                // Save device registry
                let device_manager = self.device_manager.read().await;
                if let Err(e) = device_manager.save_registry() {
                    error!("Error saving device registry: {}", e);
                }
                drop(device_manager);

                // Stop all plugins
                let mut manager = self.plugin_manager.write().await;
                if let Err(e) = manager.shutdown_all().await {
                    error!("Error stopping plugins: {}", e);
                }
            }
        ).await;

        if shutdown_result.is_err() {
            error!("Shutdown timed out after 30 seconds, forcing exit");
        }

        info!("Daemon shutdown complete");
        Ok(())
    }
}

/// Handle internal signaling packets for DBus emission
///
/// Returns true if the packet was an internal packet and was handled,
/// false if it should be forwarded to the connection manager.
async fn handle_internal_packet(dbus: &dbus::DbusServer, device_id: &str, packet: &Packet) -> bool {
    match packet.packet_type.as_str() {
        "cconnect.internal.screenshare.requested" => {
            if let Err(e) = dbus.emit_screen_share_requested(device_id).await {
                error!("Failed to emit screen_share_requested signal: {}", e);
            }
            true
        }
        "cconnect.internal.screenshare.started" => {
            let is_sender = packet
                .body
                .get("is_sender")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if let Err(e) = dbus.emit_screen_share_started(device_id, is_sender).await {
                error!("Failed to emit screen_share_started signal: {}", e);
            }
            true
        }
        "cconnect.internal.screenshare.stopped" => {
            if let Err(e) = dbus.emit_screen_share_stopped(device_id).await {
                error!("Failed to emit screen_share_stopped signal: {}", e);
            }
            true
        }
        "cconnect.internal.screenshare.cursor" => {
            let x = packet.body.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let y = packet.body.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let visible = packet
                .body
                .get("visible")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if let Err(e) = dbus
                .emit_screen_share_cursor_update(device_id, x, y, visible)
                .await
            {
                error!("Failed to emit screen_share_cursor_update signal: {}", e);
            }
            true
        }
        "cconnect.internal.screenshare.annotation" => {
            let annotation_type = packet
                .body
                .get("annotation_type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let x1 = packet.body.get("x1").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let y1 = packet.body.get("y1").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let x2 = packet.body.get("x2").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let y2 = packet.body.get("y2").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let color = packet
                .body
                .get("color")
                .and_then(|v| v.as_str())
                .unwrap_or("#FF0000");
            let width = packet
                .body
                .get("width")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as u8;
            if let Err(e) = dbus
                .emit_screen_share_annotation(
                    device_id,
                    annotation_type,
                    x1,
                    y1,
                    x2,
                    y2,
                    color,
                    width,
                )
                .await
            {
                error!("Failed to emit screen_share_annotation signal: {}", e);
            }
            true
        }
        "cconnect.internal.screenshare.share_requested" => {
            // Remote device is requesting us to share our screen with them
            let requester_name = packet
                .body
                .get("requester_name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            info!(
                "Screen share request from {} ({})",
                requester_name, device_id
            );
            if let Err(e) = dbus.emit_screen_share_outgoing_request(device_id).await {
                error!("Failed to emit screen_share_outgoing_request signal: {}", e);
            }
            true
        }
        "cconnect.internal.telephony.ringing"
        | "cconnect.internal.telephony.talking"
        | "cconnect.internal.telephony.missed_call" => {
            let phone_number = packet
                .body
                .get("phoneNumber")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let contact_name = packet
                .body
                .get("contactName")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown contact");

            let result = match packet.packet_type.as_str() {
                "cconnect.internal.telephony.ringing" => {
                    dbus.emit_incoming_call(device_id, phone_number, contact_name, "ringing")
                        .await
                }
                "cconnect.internal.telephony.talking" => {
                    dbus.emit_call_state_changed(device_id, "talking", phone_number, contact_name)
                        .await
                }
                _ => {
                    // missed_call
                    dbus.emit_missed_call(device_id, phone_number, contact_name)
                        .await
                }
            };
            if let Err(e) = result {
                error!(
                    "Failed to emit telephony signal for {}: {}",
                    packet.packet_type, e
                );
            }
            true
        }
        "cconnect.internal.sms.received" => {
            let thread_id = packet
                .body
                .get("threadId")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let address = packet
                .body
                .get("address")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let body = packet
                .body
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let timestamp = packet
                .body
                .get("date")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if let Err(e) = dbus
                .emit_sms_received(device_id, thread_id, address, body, timestamp)
                .await
            {
                error!("Failed to emit sms_received signal: {}", e);
            }
            true
        }
        "cconnect.internal.sms.conversations_updated" => {
            let count = packet
                .body
                .get("count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            if let Err(e) = dbus
                .emit_sms_conversations_updated(device_id, count)
                .await
            {
                error!("Failed to emit sms_conversations_updated signal: {}", e);
            }
            true
        }
        "cconnect.internal.extendeddisplay.started" => {
            if let Err(e) = dbus.emit_extended_display_started(device_id).await {
                error!("Failed to emit extended_display_started signal: {}", e);
            }
            true
        }
        "cconnect.internal.extendeddisplay.stopped" => {
            if let Err(e) = dbus.emit_extended_display_stopped(device_id).await {
                error!("Failed to emit extended_display_stopped signal: {}", e);
            }
            true
        }
        "cconnect.internal.extendeddisplay.error" => {
            let error_msg = packet
                .body
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            if let Err(e) = dbus
                .emit_extended_display_error(device_id, error_msg)
                .await
            {
                error!("Failed to emit extended_display_error signal: {}", e);
            }
            true
        }
        _ => false, // Not an internal packet
    }
}

/// Handle diagnostic commands
async fn handle_diagnostic_command(command: &DiagnosticCommand) -> Result<()> {
    match command {
        DiagnosticCommand::Version { verbose } => {
            let build_info = BuildInfo::get();
            build_info.display(*verbose);
            Ok(())
        }
        DiagnosticCommand::ListDevices { verbose } => {
            // Load configuration to get device registry path
            let config = Config::load().context("Failed to load configuration")?;
            let device_manager = DeviceManager::new(config.device_registry_path())
                .context("Failed to load device registry")?;

            println!("\n=== Known Devices ===");
            let device_count = device_manager.device_count();

            if device_count == 0 {
                println!("No devices found.");
            } else {
                for device in device_manager.devices() {
                    print!("{} ({})", device.name(), device.id());

                    if device.is_connected() {
                        print!(" - CONNECTED");
                    } else if device.is_paired() {
                        print!(" - PAIRED");
                    } else {
                        print!(" - AVAILABLE");
                    }

                    println!();

                    if *verbose {
                        println!("  Type: {:?}", device.info.device_type);
                        println!(
                            "  Last seen: {} seconds ago",
                            device.seconds_since_last_seen()
                        );
                        if let Some(host) = &device.host {
                            println!("  Host: {}:{}", host, device.port.unwrap_or(0));
                        }
                        println!();
                    }
                }
                println!("\nTotal: {} devices", device_count);
            }
            Ok(())
        }
        DiagnosticCommand::DeviceInfo { device_id } => {
            let config = Config::load().context("Failed to load configuration")?;
            let device_manager = DeviceManager::new(config.device_registry_path())
                .context("Failed to load device registry")?;

            match device_manager.get_device(device_id) {
                Some(device) => {
                    println!("\n=== Device Information ===");
                    println!("Name: {}", device.name());
                    println!("ID: {}", device.id());
                    println!("Type: {:?}", device.info.device_type);
                    println!("Connection: {:?}", device.connection_state);
                    println!("Pairing: {:?}", device.pairing_status);
                    println!("Trusted: {}", device.is_trusted);
                    println!(
                        "Last seen: {} seconds ago",
                        device.seconds_since_last_seen()
                    );

                    if let Some(host) = &device.host {
                        println!("Host: {}:{}", host, device.port.unwrap_or(0));
                    }

                    if let Some(fingerprint) = &device.certificate_fingerprint {
                        println!("Certificate fingerprint: {}", fingerprint);
                    }

                    println!("\nCapabilities:");
                    println!("  Incoming ({}):", device.info.incoming_capabilities.len());
                    for cap in &device.info.incoming_capabilities {
                        println!("    - {}", cap);
                    }
                    println!("  Outgoing ({}):", device.info.outgoing_capabilities.len());
                    for cap in &device.info.outgoing_capabilities {
                        println!("    - {}", cap);
                    }

                    Ok(())
                }
                None => {
                    eprintln!("Device not found: {}", device_id);
                    std::process::exit(1);
                }
            }
        }
        DiagnosticCommand::TestConnectivity { device_id, timeout } => {
            println!("Testing connectivity to device: {}", device_id);
            println!("Timeout: {} seconds", timeout);
            println!("\nNote: Full connectivity testing requires running daemon.");
            println!("This command currently only checks device registry.");

            let config = Config::load().context("Failed to load configuration")?;
            let device_manager = DeviceManager::new(config.device_registry_path())
                .context("Failed to load device registry")?;

            match device_manager.get_device(device_id) {
                Some(device) => {
                    if device.is_connected() {
                        println!("✓ Device is currently connected");
                    } else if device.seen_recently(60) {
                        println!("⚠ Device was seen recently but not connected");
                    } else {
                        println!(
                            "✗ Device not seen recently (last seen {} seconds ago)",
                            device.seconds_since_last_seen()
                        );
                    }
                    Ok(())
                }
                None => {
                    eprintln!("✗ Device not found: {}", device_id);
                    std::process::exit(1);
                }
            }
        }
        DiagnosticCommand::DumpConfig { show_sensitive } => {
            let config = Config::load().context("Failed to load configuration")?;

            println!("\n=== Daemon Configuration ===");
            println!("\n[Device]");
            println!("Name: {}", config.device.name);
            println!("Type: {}", config.device.device_type);
            if let Some(id) = &config.device.device_id {
                println!("ID: {}", id);
            }

            println!("\n[Network]");
            println!("Discovery port: {}", config.network.discovery_port);
            println!(
                "Transfer port range: {}-{}",
                config.network.transfer_port_start, config.network.transfer_port_end
            );
            println!(
                "Discovery interval: {} seconds",
                config.network.discovery_interval
            );
            println!("Device timeout: {} seconds", config.network.device_timeout);

            println!("\n[Plugins]");
            println!("Ping: {}", config.plugins.enable_ping);
            println!("Battery: {}", config.plugins.enable_battery);
            println!("Notification: {}", config.plugins.enable_notification);
            println!("Share: {}", config.plugins.enable_share);
            println!("Clipboard: {}", config.plugins.enable_clipboard);
            println!("MPRIS: {}", config.plugins.enable_mpris);
            println!("RunCommand: {}", config.plugins.enable_runcommand);
            println!("Remote Input: {}", config.plugins.enable_remoteinput);
            println!("Find My Phone: {}", config.plugins.enable_findmyphone);
            println!("Telephony: {}", config.plugins.enable_telephony);
            println!("Presenter: {}", config.plugins.enable_presenter);
            println!("Contacts: {}", config.plugins.enable_contacts);

            if *show_sensitive {
                println!("\n[Paths]");
                println!("Config: {:?}", config.paths.config_dir);
                println!("Data: {:?}", config.paths.data_dir);
                println!("Certificates: {:?}", config.paths.cert_dir);
                println!("Certificate file: {:?}", config.certificate_path());
                println!("Private key file: {:?}", config.private_key_path());
            }

            Ok(())
        }
        DiagnosticCommand::ExportLogs { output, lines } => {
            println!("Exporting last {} lines of logs to: {}", lines, output);
            println!("\nNote: Log export currently requires manual journal extraction.");
            println!(
                "Run: journalctl -u cconnect-daemon -n {} > {}",
                lines, output
            );
            Ok(())
        }
        DiagnosticCommand::Metrics { interval, count } => {
            println!("Performance metrics display");
            println!("Update interval: {} seconds", interval);
            println!(
                "Updates: {}",
                if *count == 0 {
                    "infinite".to_string()
                } else {
                    count.to_string()
                }
            );
            println!("\nNote: Metrics require running daemon with --metrics flag.");
            println!("Start daemon with: cconnect-daemon --metrics");
            Ok(())
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command-line arguments
    let cli = Cli::parse();

    // Handle diagnostic commands (non-daemon mode)
    if let Some(command) = &cli.command {
        return handle_diagnostic_command(command).await;
    }

    // Initialize logging with CLI configuration
    diagnostics::init_logging(&cli).context("Failed to initialize logging")?;

    info!("Starting CConnect daemon...");

    // Load configuration
    let config = Config::load().context("Failed to load configuration")?;

    info!("Configuration loaded");
    info!("Device name: {}", config.device.name);
    info!("Device type: {}", config.device.device_type);
    info!("Discovery port: {}", config.network.discovery_port);

    // Create daemon
    let mut daemon = Daemon::new(config)
        .await
        .context("Failed to create daemon")?;

    // Enable metrics if requested
    if cli.metrics {
        daemon.enable_metrics();
    }

    // Enable packet dumping if requested
    if cli.dump_packets {
        daemon.enable_packet_dumping();
    }

    // Initialize plugins
    daemon
        .initialize_plugins()
        .await
        .context("Failed to initialize plugins")?;

    // Start pairing service FIRST (needed by DBus)
    daemon
        .start_pairing()
        .await
        .context("Failed to start pairing")?;

    // Start DBus server (after pairing so it can access the pairing service)
    daemon
        .start_dbus()
        .await
        .context("Failed to start DBus server")?;

    // Start discovery
    daemon
        .start_discovery()
        .await
        .context("Failed to start discovery")?;

    // Start connection manager
    daemon
        .start_connections()
        .await
        .context("Failed to start connection manager")?;

    // Start clipboard monitor
    daemon
        .start_clipboard_monitor()
        .await
        .context("Failed to start clipboard monitor")?;

    // Start notification listener
    daemon
        .start_notification_listener()
        .await
        .context("Failed to start notification listener")?;

    // Start MPRIS monitoring
    daemon
        .start_mpris_monitoring()
        .await
        .context("Failed to start MPRIS monitoring")?;

    // Run daemon
    let result = daemon.run().await;

    // Shutdown
    daemon.shutdown().await?;

    result
}
