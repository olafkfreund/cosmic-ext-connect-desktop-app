//! Daemon Configuration
//!
//! Configuration management for the CConnect daemon.

use anyhow::{Context, Result};
use cosmic_connect_protocol::TransportPreference;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

/// Daemon configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Device configuration
    pub device: DeviceConfig,

    /// Network configuration
    pub network: NetworkConfig,

    /// Transport configuration
    #[serde(default)]
    pub transport: TransportConfig,

    /// Plugin configuration
    pub plugins: PluginConfig,

    /// Storage paths
    pub paths: PathConfig,
}

/// Device configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// Device name
    pub name: String,

    /// Device type (desktop, laptop, phone, tablet)
    pub device_type: String,

    /// Device ID (auto-generated if not set)
    #[serde(default)]
    pub device_id: Option<String>,
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// UDP discovery port
    #[serde(default = "default_discovery_port")]
    pub discovery_port: u16,

    /// TCP transfer port range start
    #[serde(default = "default_transfer_port_start")]
    pub transfer_port_start: u16,

    /// TCP transfer port range end
    #[serde(default = "default_transfer_port_end")]
    pub transfer_port_end: u16,

    /// Discovery broadcast interval in seconds
    #[serde(default = "default_discovery_interval")]
    pub discovery_interval: u64,

    /// Device timeout in seconds (how long before a device is considered offline)
    #[serde(default = "default_device_timeout")]
    pub device_timeout: u64,
}

/// Transport configuration
///
/// Configure which network transports are available and how they should be used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    /// Enable TCP/IP transport (WiFi, Ethernet)
    #[serde(default = "default_true")]
    pub enable_tcp: bool,

    /// Enable Bluetooth transport
    #[serde(default = "default_false")]
    pub enable_bluetooth: bool,

    /// Transport preference for new connections
    #[serde(default)]
    pub preference: TransportPreferenceConfig,

    /// TCP operation timeout in seconds
    #[serde(default = "default_tcp_timeout")]
    pub tcp_timeout_secs: u64,

    /// Bluetooth operation timeout in seconds
    #[serde(default = "default_bluetooth_timeout")]
    pub bluetooth_timeout_secs: u64,

    /// Automatically fallback to alternative transport if primary fails
    #[serde(default = "default_true")]
    pub auto_fallback: bool,

    /// Bluetooth device filtering (empty = no filter, accepts all)
    #[serde(default)]
    pub bluetooth_device_filter: Vec<String>,
}

/// Transport preference configuration (serialization wrapper)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum TransportPreferenceConfig {
    /// Prefer TCP if available
    #[default]
    PreferTcp,
    /// Prefer Bluetooth if available
    PreferBluetooth,
    /// Try TCP first, fall back to Bluetooth
    TcpFirst,
    /// Try Bluetooth first, fall back to TCP
    BluetoothFirst,
    /// Only use TCP
    OnlyTcp,
    /// Only use Bluetooth
    OnlyBluetooth,
}


impl From<TransportPreferenceConfig> for TransportPreference {
    fn from(config: TransportPreferenceConfig) -> Self {
        use cosmic_connect_protocol::TransportType;
        match config {
            TransportPreferenceConfig::PreferTcp => TransportPreference::PreferTcp,
            TransportPreferenceConfig::PreferBluetooth => TransportPreference::PreferBluetooth,
            TransportPreferenceConfig::TcpFirst => TransportPreference::TcpFirst,
            TransportPreferenceConfig::BluetoothFirst => TransportPreference::BluetoothFirst,
            TransportPreferenceConfig::OnlyTcp => TransportPreference::Only(TransportType::Tcp),
            TransportPreferenceConfig::OnlyBluetooth => {
                TransportPreference::Only(TransportType::Bluetooth)
            }
        }
    }
}

/// Plugin configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    /// Enable ping plugin
    #[serde(default = "default_true")]
    pub enable_ping: bool,

    /// Enable battery plugin
    #[serde(default = "default_true")]
    pub enable_battery: bool,

    /// Enable notification plugin
    #[serde(default = "default_true")]
    pub enable_notification: bool,

    /// Enable share plugin
    #[serde(default = "default_true")]
    pub enable_share: bool,

    /// Enable clipboard plugin
    #[serde(default = "default_true")]
    pub enable_clipboard: bool,

    /// Enable MPRIS plugin
    #[serde(default = "default_true")]
    pub enable_mpris: bool,

    /// Enable RunCommand plugin
    #[serde(default = "default_true")]
    pub enable_runcommand: bool,

    /// Enable Remote Input plugin
    #[serde(default = "default_true")]
    pub enable_remoteinput: bool,

    /// Enable Find My Phone plugin
    #[serde(default = "default_true")]
    pub enable_findmyphone: bool,

    /// Enable Lock plugin
    #[serde(default = "default_true")]
    pub enable_lock: bool,

    /// Enable Telephony/SMS plugin
    #[serde(default = "default_true")]
    pub enable_telephony: bool,

    /// Enable Presenter plugin
    #[serde(default = "default_true")]
    pub enable_presenter: bool,

    /// Enable Contacts plugin
    #[serde(default = "default_true")]
    pub enable_contacts: bool,

    /// Enable SystemMonitor plugin
    #[serde(default = "default_true")]
    pub enable_systemmonitor: bool,

    /// Enable Wake-on-LAN plugin
    #[serde(default = "default_true")]
    pub enable_wol: bool,

    /// Enable Screenshot plugin
    #[serde(default = "default_true")]
    pub enable_screenshot: bool,

    /// Enable RemoteDesktop plugin (VNC-based remote desktop)
    #[serde(default = "default_false")]
    pub enable_remotedesktop: bool,

    /// Enable Power plugin (remote power management)
    #[serde(default = "default_false")]
    pub enable_power: bool,

    /// Enable ClipboardHistory plugin (persistent clipboard history)
    #[serde(default = "default_true")]
    pub enable_clipboardhistory: bool,

    /// Enable Macro plugin (automation scripts)
    #[serde(default = "default_false")]
    pub enable_macro: bool,

    /// Enable Chat plugin (instant messaging)
    #[serde(default = "default_true")]
    pub enable_chat: bool,

    /// Enable AudioStream plugin (audio streaming between desktops)
    #[serde(default = "default_false")]
    pub enable_audiostream: bool,
}

/// Storage paths configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathConfig {
    /// Configuration directory
    pub config_dir: PathBuf,

    /// Data directory (for received files, etc.)
    pub data_dir: PathBuf,

    /// Certificate directory
    pub cert_dir: PathBuf,
}

fn default_discovery_port() -> u16 {
    1716
}

fn default_transfer_port_start() -> u16 {
    1739
}

fn default_transfer_port_end() -> u16 {
    1764
}

fn default_discovery_interval() -> u64 {
    5
}

fn default_device_timeout() -> u64 {
    30
}

fn default_tcp_timeout() -> u64 {
    10
}

fn default_bluetooth_timeout() -> u64 {
    15
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            discovery_port: default_discovery_port(),
            transfer_port_start: default_transfer_port_start(),
            transfer_port_end: default_transfer_port_end(),
            discovery_interval: default_discovery_interval(),
            device_timeout: default_device_timeout(),
        }
    }
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            // TCP enabled by default (existing behavior)
            enable_tcp: true,
            // Bluetooth disabled by default (opt-in)
            enable_bluetooth: false,
            // Prefer TCP by default (faster, more reliable on local network)
            preference: TransportPreferenceConfig::PreferTcp,
            // TCP timeout: 10 seconds
            tcp_timeout_secs: default_tcp_timeout(),
            // Bluetooth timeout: 15 seconds (BLE has higher latency)
            bluetooth_timeout_secs: default_bluetooth_timeout(),
            // Auto fallback enabled by default
            auto_fallback: true,
            // No device filter by default (accept all)
            bluetooth_device_filter: Vec::new(),
        }
    }
}

impl TransportConfig {
    /// Get TCP timeout as Duration
    pub fn tcp_timeout(&self) -> Duration {
        Duration::from_secs(self.tcp_timeout_secs)
    }

    /// Get Bluetooth timeout as Duration
    pub fn bluetooth_timeout(&self) -> Duration {
        Duration::from_secs(self.bluetooth_timeout_secs)
    }

    /// Check if a Bluetooth device address should be accepted
    pub fn should_accept_bluetooth_device(&self, address: &str) -> bool {
        // If no filter, accept all devices
        if self.bluetooth_device_filter.is_empty() {
            return true;
        }

        // Check if address matches any pattern in filter
        self.bluetooth_device_filter
            .iter()
            .any(|pattern| address.contains(pattern))
    }
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            // Core plugins - always enabled
            enable_ping: true,
            enable_battery: true,
            enable_notification: true,
            enable_share: true,
            enable_clipboard: true,
            enable_mpris: true,

            // Advanced plugins - disabled by default to reduce discovery packet size
            // These can be enabled in config if needed
            enable_runcommand: false,     // Security: command execution should be opt-in
            enable_remoteinput: true,     // Keep enabled - useful for presentations
            enable_findmyphone: true,     // Keep enabled - emergency feature
            enable_lock: true,            // Desktop lock/unlock control
            enable_telephony: true,       // Enabled - SMS and call notifications
            enable_presenter: false,      // Specialized use case - reduce packet size
            enable_contacts: false,       // Reduce packet size - can be enabled if needed
            enable_systemmonitor: true,   // Desktop-to-desktop system monitoring
            enable_wol: true,             // Desktop-to-desktop Wake-on-LAN
            enable_screenshot: true,      // Desktop-to-desktop screenshot capture
            enable_remotedesktop: false,  // Security: disabled by default, requires explicit opt-in
            enable_power: false,          // Security: power control disabled by default
            enable_clipboardhistory: true, // Clipboard history with sync
            enable_macro: false,          // Security: automation disabled by default
            enable_chat: true,            // Instant messaging enabled by default
            enable_audiostream: false,    // Audio streaming disabled by default (requires audio backend)
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("cosmic")
            .join("cosmic-connect");

        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from(".local/share"))
            .join("cosmic")
            .join("cosmic-connect");

        let cert_dir = config_dir.join("certs");

        Self {
            device: DeviceConfig {
                name: format!(
                    "CD-{}",
                    hostname::get()
                        .ok()
                        .and_then(|h| h.into_string().ok())
                        .unwrap_or_else(|| "Unknown Device".to_string())
                ),
                device_type: "desktop".to_string(),
                device_id: None,
            },
            network: NetworkConfig::default(),
            transport: TransportConfig::default(),
            plugins: PluginConfig::default(),
            paths: PathConfig {
                config_dir,
                data_dir,
                cert_dir,
            },
        }
    }
}

impl Config {
    /// Load configuration from file, creating default if not found
    pub fn load() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("cosmic")
            .join("cosmic-connect");

        let config_path = config_dir.join("daemon.toml");

        if config_path.exists() {
            let contents =
                fs::read_to_string(&config_path).context("Failed to read config file")?;
            let config: Config =
                toml::from_str(&contents).context("Failed to parse config file")?;
            Ok(config)
        } else {
            // Create default config
            let config = Config::default();
            config.save()?;
            Ok(config)
        }
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        // Ensure config directory exists
        fs::create_dir_all(&self.paths.config_dir).context("Failed to create config directory")?;

        let config_path = self.paths.config_dir.join("daemon.toml");
        let contents = toml::to_string_pretty(self).context("Failed to serialize config")?;

        fs::write(&config_path, contents).context("Failed to write config file")?;

        Ok(())
    }

    /// Ensure all required directories exist
    pub fn ensure_directories(&self) -> Result<()> {
        fs::create_dir_all(&self.paths.config_dir).context("Failed to create config directory")?;
        fs::create_dir_all(&self.paths.data_dir).context("Failed to create data directory")?;
        fs::create_dir_all(&self.paths.cert_dir)
            .context("Failed to create certificate directory")?;
        Ok(())
    }

    /// Get the certificate path for this device
    pub fn certificate_path(&self) -> PathBuf {
        self.paths.cert_dir.join("device.crt")
    }

    /// Get the private key path for this device
    pub fn private_key_path(&self) -> PathBuf {
        self.paths.cert_dir.join("device.key")
    }

    /// Get the device registry path
    pub fn device_registry_path(&self) -> PathBuf {
        self.paths.data_dir.join("devices.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.network.discovery_port, 1716);
        assert_eq!(config.network.transfer_port_start, 1739);
        assert!(config.plugins.enable_ping);
        assert!(config.plugins.enable_battery);
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.network.discovery_port, config.network.discovery_port);
    }

    #[test]
    fn test_transport_config_defaults() {
        let transport = TransportConfig::default();
        assert!(transport.enable_tcp);
        assert!(!transport.enable_bluetooth);
        assert!(transport.auto_fallback);
        assert_eq!(transport.tcp_timeout_secs, 10);
        assert_eq!(transport.bluetooth_timeout_secs, 15);
    }

    #[test]
    fn test_transport_timeout_conversion() {
        let transport = TransportConfig::default();
        assert_eq!(transport.tcp_timeout(), Duration::from_secs(10));
        assert_eq!(transport.bluetooth_timeout(), Duration::from_secs(15));
    }

    #[test]
    fn test_bluetooth_device_filter() {
        let mut transport = TransportConfig::default();

        // No filter = accept all
        assert!(transport.should_accept_bluetooth_device("00:11:22:33:44:55"));
        assert!(transport.should_accept_bluetooth_device("AA:BB:CC:DD:EE:FF"));

        // With filter
        transport.bluetooth_device_filter = vec!["00:11:22".to_string()];
        assert!(transport.should_accept_bluetooth_device("00:11:22:33:44:55"));
        assert!(!transport.should_accept_bluetooth_device("AA:BB:CC:DD:EE:FF"));
    }

    #[test]
    fn test_transport_preference_conversion() {
        let pref: TransportPreference = TransportPreferenceConfig::PreferTcp.into();
        assert_eq!(pref, TransportPreference::PreferTcp);

        let pref: TransportPreference = TransportPreferenceConfig::BluetoothFirst.into();
        assert_eq!(pref, TransportPreference::BluetoothFirst);
    }
}
