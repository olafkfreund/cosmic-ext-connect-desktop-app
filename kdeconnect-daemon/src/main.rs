mod config;

use anyhow::{Context, Result};
use kdeconnect_protocol::{
    discovery::{DiscoveryConfig, DiscoveryEvent, DiscoveryService},
    pairing::{PairingConfig, PairingEvent, PairingService, PairingStatus},
    plugins::{
        battery::BatteryPlugin, clipboard::ClipboardPlugin, mpris::MprisPlugin,
        notification::NotificationPlugin, ping::PingPlugin, share::SharePlugin, PluginManager,
    },
    CertificateInfo, Device, DeviceInfo, DeviceManager, DeviceType,
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

    /// Discovery service
    discovery_service: Option<DiscoveryService>,

    /// Pairing service
    pairing_service: Option<PairingService>,
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

        Ok(Self {
            config,
            certificate,
            device_info,
            plugin_manager,
            device_manager,
            discovery_service: None,
            pairing_service: None,
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

    /// Initialize plugins
    async fn initialize_plugins(&self) -> Result<()> {
        let mut manager = self.plugin_manager.write().await;

        info!("Initializing plugins...");

        // Register enabled plugins
        if self.config.plugins.enable_ping {
            info!("Registering ping plugin");
            manager
                .register(Box::new(PingPlugin::new()))
                .context("Failed to register ping plugin")?;
        }

        if self.config.plugins.enable_battery {
            info!("Registering battery plugin");
            manager
                .register(Box::new(BatteryPlugin::new()))
                .context("Failed to register battery plugin")?;
        }

        if self.config.plugins.enable_notification {
            info!("Registering notification plugin");
            manager
                .register(Box::new(NotificationPlugin::new()))
                .context("Failed to register notification plugin")?;
        }

        if self.config.plugins.enable_share {
            info!("Registering share plugin");
            manager
                .register(Box::new(SharePlugin::new()))
                .context("Failed to register share plugin")?;
        }

        if self.config.plugins.enable_clipboard {
            info!("Registering clipboard plugin");
            manager
                .register(Box::new(ClipboardPlugin::new()))
                .context("Failed to register clipboard plugin")?;
        }

        if self.config.plugins.enable_mpris {
            info!("Registering MPRIS plugin");
            manager
                .register(Box::new(MprisPlugin::new()))
                .context("Failed to register MPRIS plugin")?;
        }

        // Create a temporary device for initialization
        let device = Device::from_discovery(self.device_info.clone());

        // Initialize all plugins
        manager
            .init_all(&device)
            .await
            .context("Failed to initialize plugins")?;

        // Start all plugins
        manager
            .start_all()
            .await
            .context("Failed to start plugins")?;

        info!("All plugins initialized and started");

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
        let mut discovery_service =
            DiscoveryService::new(device_info, discovery_config).context("Failed to create discovery service")?;

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
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if let Err(e) = Self::handle_discovery_event(event, &device_manager).await {
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

        let pairing_service = PairingService::new(
            self.device_info.device_id.clone(),
            pairing_config,
        )
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
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if let Err(e) = Self::handle_pairing_event(event, &device_manager).await {
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
                // TODO: Notify UI to show pairing request and fingerprint
            }
            PairingEvent::PairingAccepted {
                device_id,
                device_name,
            } => {
                info!("Pairing accepted with {} ({})", device_name, device_id);
                // Device is now paired - DeviceManager will be updated separately
                // TODO: Notify UI of successful pairing
            }
            PairingEvent::PairingRejected { device_id, reason } => {
                info!(
                    "Pairing rejected with device {} (reason: {:?})",
                    device_id, reason
                );
                // TODO: Notify UI of rejected pairing
            }
            PairingEvent::StatusChanged { device_id, status } => {
                debug!("Pairing status changed for {}: {:?}", device_id, status);
            }
            PairingEvent::DeviceUnpaired { device_id } => {
                info!("Device unpaired: {}", device_id);
                let mut manager = device_manager.write().await;
                if let Err(e) = manager.update_pairing_status(&device_id, PairingStatus::Unpaired)
                {
                    warn!("Failed to update device {} pairing status: {}", device_id, e);
                } else if let Err(e) = manager.save_registry() {
                    warn!("Failed to save device registry: {}", e);
                }
            }
            PairingEvent::PairingTimeout { device_id } => {
                warn!("Pairing request timed out for device {}", device_id);
                // TODO: Notify UI of timeout
            }
            PairingEvent::Error { device_id, message } => {
                error!(
                    "Pairing error for device {:?}: {}",
                    device_id, message
                );
                // TODO: Notify UI of error
            }
        }
        Ok(())
    }

    /// Handle a discovery event
    async fn handle_discovery_event(
        event: DiscoveryEvent,
        device_manager: &Arc<RwLock<DeviceManager>>,
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
                manager.update_from_discovery(info);
                if let Err(e) = manager.save_registry() {
                    warn!("Failed to save device registry: {}", e);
                }
            }
            DiscoveryEvent::DeviceUpdated { info, address } => {
                debug!(
                    "Device updated: {} at {}",
                    info.device_name, address
                );
                let mut manager = device_manager.write().await;
                manager.update_from_discovery(info);
            }
            DiscoveryEvent::DeviceTimeout { device_id } => {
                info!("Device timed out: {}", device_id);
                let mut manager = device_manager.write().await;
                if let Err(e) = manager.mark_disconnected(&device_id) {
                    debug!("Failed to mark device {} as disconnected: {}", device_id, e);
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
        info!("Device registry: {} devices loaded", device_manager.device_count());
        info!("  - Paired devices: {}", device_manager.paired_count());
        info!("  - Connected devices: {}", device_manager.connected_count());
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

        // Save device registry
        let device_manager = self.device_manager.read().await;
        if let Err(e) = device_manager.save_registry() {
            error!("Error saving device registry: {}", e);
        }
        drop(device_manager);

        // Stop all plugins
        let mut manager = self.plugin_manager.write().await;
        if let Err(e) = manager.stop_all().await {
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

    // Run daemon
    let result = daemon.run().await;

    // Shutdown
    daemon.shutdown().await?;

    result
}
