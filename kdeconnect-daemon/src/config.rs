//! Daemon Configuration
//!
//! Configuration management for the KDE Connect daemon.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Daemon configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Device configuration
    pub device: DeviceConfig,

    /// Network configuration
    pub network: NetworkConfig,

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

fn default_true() -> bool {
    true
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            discovery_port: default_discovery_port(),
            transfer_port_start: default_transfer_port_start(),
            transfer_port_end: default_transfer_port_end(),
            discovery_interval: default_discovery_interval(),
        }
    }
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            enable_ping: true,
            enable_battery: true,
            enable_notification: true,
            enable_share: true,
            enable_clipboard: true,
            enable_mpris: true,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("kdeconnect");

        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from(".local/share"))
            .join("kdeconnect");

        let cert_dir = config_dir.join("certs");

        Self {
            device: DeviceConfig {
                name: hostname::get()
                    .ok()
                    .and_then(|h| h.into_string().ok())
                    .unwrap_or_else(|| "Unknown Device".to_string()),
                device_type: "desktop".to_string(),
                device_id: None,
            },
            network: NetworkConfig::default(),
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
            .join("kdeconnect");

        let config_path = config_dir.join("daemon.toml");

        if config_path.exists() {
            let contents = fs::read_to_string(&config_path)
                .context("Failed to read config file")?;
            let config: Config = toml::from_str(&contents)
                .context("Failed to parse config file")?;
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
        fs::create_dir_all(&self.paths.config_dir)
            .context("Failed to create config directory")?;

        let config_path = self.paths.config_dir.join("daemon.toml");
        let contents = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;

        fs::write(&config_path, contents)
            .context("Failed to write config file")?;

        Ok(())
    }

    /// Ensure all required directories exist
    pub fn ensure_directories(&self) -> Result<()> {
        fs::create_dir_all(&self.paths.config_dir)
            .context("Failed to create config directory")?;
        fs::create_dir_all(&self.paths.data_dir)
            .context("Failed to create data directory")?;
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
}
