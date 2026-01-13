mod config;

use anyhow::{Context, Result};
use kdeconnect_protocol::{
    CertificateInfo, Device, DeviceInfo, DeviceType,
    plugins::{
        PluginManager,
        battery::BatteryPlugin,
        clipboard::ClipboardPlugin,
        mpris::MprisPlugin,
        notification::NotificationPlugin,
        ping::PingPlugin,
        share::SharePlugin,
    },
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

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
}

impl Daemon {
    /// Create a new daemon
    async fn new(config: Config) -> Result<Self> {
        // Ensure directories exist
        config.ensure_directories()
            .context("Failed to create directories")?;

        // Load or generate certificate
        let certificate = Self::load_or_generate_certificate(&config)
            .context("Failed to load certificate")?;

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

        Ok(Self {
            config,
            certificate,
            device_info,
            plugin_manager,
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
            let device_id = config.device.device_id
                .as_deref()
                .unwrap_or("kdeconnect-device");

            let cert = CertificateInfo::generate(device_id)
                .context("Failed to generate certificate")?;

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
            manager.register(Box::new(PingPlugin::new()))
                .context("Failed to register ping plugin")?;
        }

        if self.config.plugins.enable_battery {
            info!("Registering battery plugin");
            manager.register(Box::new(BatteryPlugin::new()))
                .context("Failed to register battery plugin")?;
        }

        if self.config.plugins.enable_notification {
            info!("Registering notification plugin");
            manager.register(Box::new(NotificationPlugin::new()))
                .context("Failed to register notification plugin")?;
        }

        if self.config.plugins.enable_share {
            info!("Registering share plugin");
            manager.register(Box::new(SharePlugin::new()))
                .context("Failed to register share plugin")?;
        }

        if self.config.plugins.enable_clipboard {
            info!("Registering clipboard plugin");
            manager.register(Box::new(ClipboardPlugin::new()))
                .context("Failed to register clipboard plugin")?;
        }

        if self.config.plugins.enable_mpris {
            info!("Registering MPRIS plugin");
            manager.register(Box::new(MprisPlugin::new()))
                .context("Failed to register MPRIS plugin")?;
        }

        // Create a temporary device for initialization
        let device = Device::from_discovery(self.device_info.clone());

        // Initialize all plugins
        manager.init_all(&device).await
            .context("Failed to initialize plugins")?;

        // Start all plugins
        manager.start_all().await
            .context("Failed to start plugins")?;

        info!("All plugins initialized and started");

        Ok(())
    }

    /// Run the daemon
    async fn run(&self) -> Result<()> {
        info!("KDE Connect daemon running");
        info!("Device: {} ({})", self.device_info.device_name, self.device_info.device_id);
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

        info!("Daemon initialized successfully");
        info!("Press Ctrl+C to stop");

        // Wait for shutdown signal
        tokio::signal::ctrl_c().await?;

        info!("Received shutdown signal");

        Ok(())
    }

    /// Shutdown the daemon
    async fn shutdown(&self) -> Result<()> {
        info!("Shutting down daemon...");

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
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .init();

    info!("Starting KDE Connect daemon...");

    // Load configuration
    let config = Config::load()
        .context("Failed to load configuration")?;

    info!("Configuration loaded");
    info!("Device name: {}", config.device.name);
    info!("Device type: {}", config.device.device_type);
    info!("Discovery port: {}", config.network.discovery_port);

    // Create daemon
    let daemon = Daemon::new(config).await
        .context("Failed to create daemon")?;

    // Initialize plugins
    daemon.initialize_plugins().await
        .context("Failed to initialize plugins")?;

    // Run daemon
    let result = daemon.run().await;

    // Shutdown
    daemon.shutdown().await?;

    result
}
