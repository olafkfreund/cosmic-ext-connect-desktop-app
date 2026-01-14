mod config;
mod cosmic_notifications;
mod dbus;
mod device_config;

use anyhow::{Context, Result};
use dbus::DbusServer;
use kdeconnect_protocol::{
    connection::{ConnectionConfig, ConnectionEvent, ConnectionManager},
    discovery::{DiscoveryConfig, DiscoveryEvent, DiscoveryService},
    pairing::{PairingConfig, PairingEvent, PairingService, PairingStatus},
    plugins::{
        battery::BatteryPluginFactory, clipboard::ClipboardPluginFactory,
        mpris::MprisPluginFactory, notification::NotificationPluginFactory,
        ping::PingPluginFactory, share::SharePluginFactory, PluginManager,
    },
    CertificateInfo, DeviceInfo, DeviceManager, DeviceType,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use config::Config;

/// Main daemon state
struct Daemon {
    /// Configuration
    config: Config,

    /// Device certificate (for future TLS support)
    #[allow(dead_code)]
    certificate: CertificateInfo,

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

    /// Pairing service
    pairing_service: Option<PairingService>,

    /// Connection manager (wrapped for shared access)
    connection_manager: Arc<RwLock<ConnectionManager>>,

    /// COSMIC notifications client
    cosmic_notifier: Option<Arc<cosmic_notifications::CosmicNotifier>>,

    /// DBus server
    dbus_server: Option<Arc<DbusServer>>,
}

impl Daemon {
    /// Create a new daemon
    async fn new(config: Config) -> Result<Self> {
        // Ensure directories exist
        config
            .ensure_directories()
            .context("Failed to create directories")?;

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

        let device_info = if let Some(device_id) = &config.device.device_id {
            // Use configured device ID
            DeviceInfo::with_id(
                device_id,
                &config.device.name,
                device_type,
                config.network.discovery_port,
            )
        } else {
            // Generate new device ID
            DeviceInfo::new(
                &config.device.name,
                device_type,
                config.network.discovery_port,
            )
        };

        // Create plugin manager
        let plugin_manager = Arc::new(RwLock::new(PluginManager::new()));

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

        // Create connection config
        let connection_config = ConnectionConfig {
            listen_addr: format!("0.0.0.0:{}", config.network.discovery_port)
                .parse()
                .context("Invalid listen address")?,
            keep_alive_interval: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(60),
        };

        // Create connection manager (not started yet)
        let connection_manager = Arc::new(RwLock::new(ConnectionManager::new(
            certificate.clone(),
            device_manager.clone(),
            connection_config,
        )));

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

        Ok(Self {
            config,
            certificate,
            device_info,
            plugin_manager,
            device_manager,
            device_config_registry,
            discovery_service: None,
            pairing_service: None,
            connection_manager,
            cosmic_notifier,
            dbus_server: None,
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
                .unwrap_or("kdeconnect-device");

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

        info!("Registering plugin factories...");

        // Register enabled plugin factories
        if self.config.plugins.enable_ping {
            info!("Registering ping plugin factory");
            manager
                .register_factory(Arc::new(PingPluginFactory))
                .context("Failed to register ping plugin factory")?;
        }

        if self.config.plugins.enable_battery {
            info!("Registering battery plugin factory");
            manager
                .register_factory(Arc::new(BatteryPluginFactory))
                .context("Failed to register battery plugin factory")?;
        }

        if self.config.plugins.enable_notification {
            info!("Registering notification plugin factory");
            manager
                .register_factory(Arc::new(NotificationPluginFactory))
                .context("Failed to register notification plugin factory")?;
        }

        if self.config.plugins.enable_share {
            info!("Registering share plugin factory");
            manager
                .register_factory(Arc::new(SharePluginFactory))
                .context("Failed to register share plugin factory")?;
        }

        if self.config.plugins.enable_clipboard {
            info!("Registering clipboard plugin factory");
            manager
                .register_factory(Arc::new(ClipboardPluginFactory))
                .context("Failed to register clipboard plugin factory")?;
        }

        if self.config.plugins.enable_mpris {
            info!("Registering MPRIS plugin factory");
            manager
                .register_factory(Arc::new(MprisPluginFactory))
                .context("Failed to register MPRIS plugin factory")?;
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

        // Get capabilities from plugin manager
        let manager = self.plugin_manager.read().await;
        let incoming = manager.get_all_incoming_capabilities();
        let outgoing = manager.get_all_outgoing_capabilities();
        drop(manager);

        // Create device info with capabilities
        let mut device_info = self.device_info.clone();
        device_info.incoming_capabilities = incoming;
        device_info.outgoing_capabilities = outgoing;

        // Create discovery config
        let discovery_config = DiscoveryConfig {
            broadcast_interval: Duration::from_secs(self.config.network.discovery_interval),
            device_timeout: Duration::from_secs(self.config.network.device_timeout),
            enable_timeout_check: true,
        };

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
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if let Err(e) =
                    Self::handle_discovery_event(event, &device_manager, &dbus_server).await
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

        // Create pairing service with certificate directory from config
        let pairing_config = PairingConfig {
            cert_dir: self.config.paths.cert_dir.clone(),
            timeout: Duration::from_secs(30),
        };

        let pairing_service =
            PairingService::new(self.device_info.device_id.clone(), pairing_config)
                .context("Failed to create pairing service")?;

        info!(
            "Pairing service created (fingerprint: {})",
            pairing_service.fingerprint()
        );

        // Subscribe to pairing events
        let mut event_rx = pairing_service.subscribe().await;

        // Store pairing service
        self.pairing_service = Some(pairing_service);

        // Spawn task to handle pairing events
        let device_manager = self.device_manager.clone();
        let dbus_server = self.dbus_server.clone();
        let cosmic_notifier = self.cosmic_notifier.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if let Err(e) =
                    Self::handle_pairing_event(event, &device_manager, &dbus_server, &cosmic_notifier).await
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
    async fn handle_pairing_event(
        event: PairingEvent,
        device_manager: &Arc<RwLock<DeviceManager>>,
        dbus_server: &Option<Arc<DbusServer>>,
        cosmic_notifier: &Option<Arc<cosmic_notifications::CosmicNotifier>>,
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

                // Emit DBus signal for pairing request
                if let Some(dbus) = dbus_server {
                    if let Err(e) = dbus.emit_pairing_request(&device_id).await {
                        warn!("Failed to emit PairingRequest signal: {}", e);
                    }
                }

                // Show COSMIC notification for pairing request
                if let Some(notifier) = cosmic_notifier {
                    if let Err(e) = notifier.notify_pairing_request(&device_name).await {
                        warn!("Failed to send pairing request notification: {}", e);
                    }
                }
            }
            PairingEvent::PairingAccepted {
                device_id,
                device_name,
            } => {
                info!("Pairing accepted with {} ({})", device_name, device_id);

                // Emit DBus signal for pairing status changed
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

                // Emit DBus signal for pairing status changed
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
            }
            PairingEvent::PairingTimeout { device_id } => {
                warn!("Pairing request timed out for device {}", device_id);
                // TODO: Notify UI of timeout
            }
            PairingEvent::Error { device_id, message } => {
                error!("Pairing error for device {:?}: {}", device_id, message);
                // TODO: Notify UI of error
            }
        }
        Ok(())
    }

    /// Start connection manager
    async fn start_connections(&mut self) -> Result<()> {
        info!("Starting connection manager...");

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
        let dbus_server = self.dbus_server.clone();
        let cosmic_notifier = self.cosmic_notifier.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if let Err(e) = Self::handle_connection_event(
                    event,
                    &device_manager,
                    &plugin_manager,
                    &dbus_server,
                    &cosmic_notifier,
                )
                .await
                {
                    error!("Error handling connection event: {}", e);
                }
            }
            info!("Connection event handler stopped");
        });

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
        )
        .await
        .context("Failed to start DBus server")?;

        info!("DBus server started on {}", dbus::SERVICE_NAME);

        self.dbus_server = Some(Arc::new(dbus_server));

        Ok(())
    }

    /// Handle a connection event
    async fn handle_connection_event(
        event: ConnectionEvent,
        device_manager: &Arc<RwLock<DeviceManager>>,
        plugin_manager: &Arc<RwLock<PluginManager>>,
        dbus_server: &Option<Arc<DbusServer>>,
        cosmic_notifier: &Option<Arc<cosmic_notifications::CosmicNotifier>>,
    ) -> Result<()> {
        match event {
            ConnectionEvent::Connected {
                device_id,
                remote_addr,
            } => {
                info!("Device {} connected from {}", device_id, remote_addr);

                // Get device name for notifications
                let device_name = {
                    let dev_manager = device_manager.read().await;
                    dev_manager.get_device(&device_id).map(|d| d.name().to_string())
                };

                // Initialize per-device plugins
                {
                    let dev_manager = device_manager.read().await;
                    if let Some(device) = dev_manager.get_device(&device_id) {
                        let mut plug_manager = plugin_manager.write().await;
                        if let Err(e) = plug_manager.init_device_plugins(&device_id, device).await {
                            error!(
                                "Failed to initialize plugins for device {}: {}",
                                device_id, e
                            );
                        } else {
                            info!("Initialized plugins for device {}", device_id);
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
                if let Some(notifier) = cosmic_notifier {
                    if let Some(name) = device_name {
                        if let Err(e) = notifier.notify_device_connected(&name).await {
                            warn!("Failed to send device connected notification: {}", e);
                        }
                    }
                }
            }
            ConnectionEvent::Disconnected { device_id, reason } => {
                info!("Device {} disconnected (reason: {:?})", device_id, reason);

                // Get device name for notifications
                let device_name = {
                    let dev_manager = device_manager.read().await;
                    dev_manager.get_device(&device_id).map(|d| d.name().to_string())
                };

                // Cleanup per-device plugins
                {
                    let mut plug_manager = plugin_manager.write().await;
                    if let Err(e) = plug_manager.cleanup_device_plugins(&device_id).await {
                        error!("Failed to cleanup plugins for device {}: {}", device_id, e);
                    } else {
                        info!("Cleaned up plugins for device {}", device_id);
                    }
                }

                // Emit DBus signal for device state changed
                if let Some(dbus) = dbus_server {
                    if let Err(e) = dbus.emit_device_state_changed(&device_id, "paired").await {
                        warn!("Failed to emit DeviceStateChanged signal: {}", e);
                    }
                }

                // Show COSMIC notification for device disconnection
                if let Some(notifier) = cosmic_notifier {
                    if let Some(name) = device_name {
                        if let Err(e) = notifier.notify_device_disconnected(&name).await {
                            warn!("Failed to send device disconnected notification: {}", e);
                        }
                    }
                }
            }
            ConnectionEvent::PacketReceived { device_id, packet } => {
                debug!(
                    "Received packet '{}' from device {}",
                    packet.packet_type, device_id
                );

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
                    }
                    drop(plug_manager);
                    drop(dev_manager);

                    // Send COSMIC notifications for specific packet types
                    if let Some(notifier) = &cosmic_notifier {
                        match packet.packet_type.as_str() {
                            "kdeconnect.ping" => {
                                // Show notification for ping
                                let message = packet
                                    .body
                                    .get("message")
                                    .and_then(|v| v.as_str());

                                if let Err(e) = notifier.notify_ping(&device_name, message).await {
                                    warn!("Failed to send ping notification: {}", e);
                                }
                            }
                            "kdeconnect.notification" => {
                                // Check if it's a cancel notification
                                let is_cancel = packet.body.get("isCancel")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);

                                if !is_cancel {
                                    // Check if notification is silent (preexisting)
                                    let is_silent = packet.body.get("silent")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s == "true")
                                        .unwrap_or(false);

                                    // Only show COSMIC notification for new notifications
                                    if !is_silent {
                                        let app_name = packet.body.get("appName")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        let title = packet.body.get("title")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("Notification");
                                        let text = packet.body.get("text")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");

                                        if let Err(e) = notifier.notify_from_device(
                                            &device_name,
                                            app_name,
                                            title,
                                            text
                                        ).await {
                                            warn!("Failed to send device notification: {}", e);
                                        }
                                    }
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

    /// Handle a discovery event
    async fn handle_discovery_event(
        event: DiscoveryEvent,
        device_manager: &Arc<RwLock<DeviceManager>>,
        dbus_server: &Option<Arc<DbusServer>>,
    ) -> Result<()> {
        match event {
            DiscoveryEvent::DeviceDiscovered { info, address } => {
                info!(
                    "Device discovered: {} ({}) at {}",
                    info.device_name,
                    info.device_type.as_str(),
                    address
                );
                let mut manager = device_manager.write().await;
                manager.update_from_discovery(info.clone());
                if let Err(e) = manager.save_registry() {
                    warn!("Failed to save device registry: {}", e);
                }

                // Emit DBus signal for device added
                if let Some(dbus) = dbus_server {
                    if let Some(device) = manager.get_device(&info.device_id) {
                        if let Err(e) = dbus.emit_device_added(device).await {
                            warn!("Failed to emit DeviceAdded signal: {}", e);
                        }
                    }
                }
            }
            DiscoveryEvent::DeviceUpdated { info, address } => {
                debug!("Device updated: {} at {}", info.device_name, address);
                let mut manager = device_manager.write().await;
                manager.update_from_discovery(info);
            }
            DiscoveryEvent::DeviceTimeout { device_id } => {
                info!("Device timed out: {}", device_id);
                let mut manager = device_manager.write().await;
                if let Err(e) = manager.mark_disconnected(&device_id) {
                    debug!("Failed to mark device {} as disconnected: {}", device_id, e);
                }

                // Emit DBus signal for device removed
                if let Some(dbus) = dbus_server {
                    if let Err(e) = dbus.emit_device_removed(&device_id).await {
                        warn!("Failed to emit DeviceRemoved signal: {}", e);
                    }
                }
            }
            DiscoveryEvent::ServiceStarted { port } => {
                info!("Discovery service started successfully on port {}", port);
            }
            DiscoveryEvent::ServiceStopped => {
                info!("Discovery service stopped");
            }
            DiscoveryEvent::Error { message } => {
                error!("Discovery error: {}", message);
            }
        }
        Ok(())
    }

    /// Run the daemon
    async fn run(&self) -> Result<()> {
        info!("KDE Connect daemon running");
        info!(
            "Device: {} ({})",
            self.device_info.device_name, self.device_info.device_id
        );
        info!("Type: {:?}", self.device_info.device_type);
        info!("Protocol version: {}", self.device_info.protocol_version);

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
        drop(device_manager);

        info!("Daemon initialized successfully");
        info!("Press Ctrl+C to stop");

        // Wait for shutdown signal
        tokio::signal::ctrl_c().await?;

        info!("Received shutdown signal");

        Ok(())
    }

    /// Shutdown the daemon
    async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down daemon...");

        // Stop discovery service
        if let Some(mut discovery) = self.discovery_service.take() {
            discovery.stop().await;
        }

        // Stop connection manager
        {
            let mut connection_manager = self.connection_manager.write().await;
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

        info!("Daemon shutdown complete");
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("Starting KDE Connect daemon...");

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

    // Initialize plugins
    daemon
        .initialize_plugins()
        .await
        .context("Failed to initialize plugins")?;

    // Start DBus server (before other services so they can emit signals)
    daemon
        .start_dbus()
        .await
        .context("Failed to start DBus server")?;

    // Start discovery
    daemon
        .start_discovery()
        .await
        .context("Failed to start discovery")?;

    // Start pairing
    daemon
        .start_pairing()
        .await
        .context("Failed to start pairing")?;

    // Start connection manager
    daemon
        .start_connections()
        .await
        .context("Failed to start connection manager")?;

    // Run daemon
    let result = daemon.run().await;

    // Shutdown
    daemon.shutdown().await?;

    result
}
