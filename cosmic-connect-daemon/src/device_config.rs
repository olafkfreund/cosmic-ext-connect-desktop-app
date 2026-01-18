//! Per-Device Configuration
//!
//! Manages configuration settings specific to individual devices,
//! including per-device plugin enable/disable settings.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Per-device configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// Device ID
    pub device_id: String,

    /// Device nickname (user-customizable)
    pub nickname: Option<String>,

    /// Per-device plugin settings (overrides global config)
    pub plugins: DevicePluginConfig,

    /// Auto-accept pairing requests from this device
    #[serde(default)]
    pub auto_accept_pairing: bool,

    /// Automatically connect when device is discovered
    #[serde(default = "default_true")]
    pub auto_connect: bool,

    /// Show notifications for this device
    #[serde(default = "default_true")]
    pub show_notifications: bool,

    /// MAC address for Wake-on-LAN
    #[serde(default)]
    pub mac_address: Option<String>,

    /// RemoteDesktop plugin-specific settings
    #[serde(default)]
    pub remotedesktop_settings: Option<RemoteDesktopSettings>,
}

/// Per-device plugin configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DevicePluginConfig {
    /// Enable ping plugin for this device (None = use global config)
    #[serde(default)]
    pub enable_ping: Option<bool>,

    /// Enable battery plugin for this device
    #[serde(default)]
    pub enable_battery: Option<bool>,

    /// Enable notification plugin for this device
    #[serde(default)]
    pub enable_notification: Option<bool>,

    /// Enable share plugin for this device
    #[serde(default)]
    pub enable_share: Option<bool>,

    /// Enable clipboard plugin for this device
    #[serde(default)]
    pub enable_clipboard: Option<bool>,

    /// Enable MPRIS plugin for this device
    #[serde(default)]
    pub enable_mpris: Option<bool>,

    /// Enable RemoteDesktop plugin for this device
    #[serde(default)]
    pub enable_remotedesktop: Option<bool>,

    /// Enable FindMyPhone plugin for this device
    #[serde(default)]
    pub enable_findmyphone: Option<bool>,

    /// Enable Lock plugin for this device
    #[serde(default)]
    pub enable_lock: Option<bool>,
}

/// RemoteDesktop plugin-specific settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteDesktopSettings {
    /// Quality preset: "low", "medium", "high"
    #[serde(default = "default_quality")]
    pub quality: String,

    /// Frames per second: 15, 30, or 60
    #[serde(default = "default_fps")]
    pub fps: u8,

    /// Resolution mode: "native" or "custom"
    #[serde(default = "default_resolution_mode")]
    pub resolution_mode: String,

    /// Custom width (only used if resolution_mode = "custom")
    #[serde(default)]
    pub custom_width: Option<u32>,

    /// Custom height (only used if resolution_mode = "custom")
    #[serde(default)]
    pub custom_height: Option<u32>,
}

fn default_quality() -> String {
    "medium".to_string()
}

fn default_fps() -> u8 {
    30
}

fn default_resolution_mode() -> String {
    "native".to_string()
}

impl Default for RemoteDesktopSettings {
    fn default() -> Self {
        Self {
            quality: default_quality(),
            fps: default_fps(),
            resolution_mode: default_resolution_mode(),
            custom_width: None,
            custom_height: None,
        }
    }
}

fn default_true() -> bool {
    true
}

impl DeviceConfig {
    /// Create a new device configuration with defaults
    pub fn new(device_id: String) -> Self {
        Self {
            device_id,
            nickname: None,
            plugins: DevicePluginConfig::default(),
            auto_accept_pairing: false,
            auto_connect: true,
            show_notifications: true,
            mac_address: None,
            remotedesktop_settings: None,
        }
    }

    /// Check if a specific plugin is enabled for this device
    ///
    /// Returns the device-specific setting if set, otherwise falls back to global config.
    pub fn is_plugin_enabled(
        &self,
        plugin_name: &str,
        global_config: &crate::config::PluginConfig,
    ) -> bool {
        match plugin_name {
            "ping" => self
                .plugins
                .enable_ping
                .unwrap_or(global_config.enable_ping),
            "battery" => self
                .plugins
                .enable_battery
                .unwrap_or(global_config.enable_battery),
            "notification" => self
                .plugins
                .enable_notification
                .unwrap_or(global_config.enable_notification),
            "share" => self
                .plugins
                .enable_share
                .unwrap_or(global_config.enable_share),
            "clipboard" => self
                .plugins
                .enable_clipboard
                .unwrap_or(global_config.enable_clipboard),
            "mpris" => self
                .plugins
                .enable_mpris
                .unwrap_or(global_config.enable_mpris),
            "remotedesktop" => self.plugins.enable_remotedesktop.unwrap_or(false), // Default to false for security
            "findmyphone" => self
                .plugins
                .enable_findmyphone
                .unwrap_or(global_config.enable_findmyphone),
            "lock" => self
                .plugins
                .enable_lock
                .unwrap_or(global_config.enable_lock),
            _ => {
                warn!("Unknown plugin name: {}", plugin_name);
                false
            }
        }
    }

    /// Set plugin enabled state for this device
    pub fn set_plugin_enabled(&mut self, plugin_name: &str, enabled: bool) {
        match plugin_name {
            "ping" => self.plugins.enable_ping = Some(enabled),
            "battery" => self.plugins.enable_battery = Some(enabled),
            "notification" => self.plugins.enable_notification = Some(enabled),
            "share" => self.plugins.enable_share = Some(enabled),
            "clipboard" => self.plugins.enable_clipboard = Some(enabled),
            "mpris" => self.plugins.enable_mpris = Some(enabled),
            "remotedesktop" => self.plugins.enable_remotedesktop = Some(enabled),
            "findmyphone" => self.plugins.enable_findmyphone = Some(enabled),
            "lock" => self.plugins.enable_lock = Some(enabled),
            _ => warn!("Unknown plugin name: {}", plugin_name),
        }
    }

    /// Clear device-specific plugin override (use global config)
    pub fn clear_plugin_override(&mut self, plugin_name: &str) {
        match plugin_name {
            "ping" => self.plugins.enable_ping = None,
            "battery" => self.plugins.enable_battery = None,
            "notification" => self.plugins.enable_notification = None,
            "share" => self.plugins.enable_share = None,
            "clipboard" => self.plugins.enable_clipboard = None,
            "mpris" => self.plugins.enable_mpris = None,
            "remotedesktop" => self.plugins.enable_remotedesktop = None,
            "findmyphone" => self.plugins.enable_findmyphone = None,
            "lock" => self.plugins.enable_lock = None,
            _ => warn!("Unknown plugin name: {}", plugin_name),
        }
    }

    /// Get RemoteDesktop settings for this device
    pub fn get_remotedesktop_settings(&self) -> RemoteDesktopSettings {
        self.remotedesktop_settings.clone().unwrap_or_default()
    }

    /// Set RemoteDesktop settings for this device
    pub fn set_remotedesktop_settings(&mut self, settings: RemoteDesktopSettings) {
        self.remotedesktop_settings = Some(settings);
    }

    /// Clear RemoteDesktop settings (use defaults)
    pub fn clear_remotedesktop_settings(&mut self) {
        self.remotedesktop_settings = None;
    }

    /// Get MAC address for Wake-on-LAN
    pub fn get_mac_address(&self) -> Option<String> {
        self.mac_address.clone()
    }

    /// Set MAC address for Wake-on-LAN
    ///
    /// Validates MAC address format before storing.
    /// Accepts formats: XX:XX:XX:XX:XX:XX or XX-XX-XX-XX-XX-XX
    pub fn set_mac_address(&mut self, mac: String) -> Result<()> {
        // Validate MAC address format
        let normalized = mac.replace('-', ":");
        let parts: Vec<&str> = normalized.split(':').collect();

        if parts.len() != 6 {
            return Err(anyhow::anyhow!(
                "Invalid MAC address format: expected 6 octets, got {}",
                parts.len()
            ));
        }

        for part in &parts {
            if part.len() != 2 {
                return Err(anyhow::anyhow!(
                    "Invalid MAC address format: each octet must be 2 hex digits"
                ));
            }
            if !part.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(anyhow::anyhow!(
                    "Invalid MAC address format: octets must be hexadecimal"
                ));
            }
        }

        self.mac_address = Some(normalized);
        Ok(())
    }

    /// Clear MAC address
    pub fn clear_mac_address(&mut self) {
        self.mac_address = None;
    }
}

/// Device configuration registry
///
/// Manages per-device configurations with persistence.
pub struct DeviceConfigRegistry {
    /// Device configurations indexed by device ID
    configs: HashMap<String, DeviceConfig>,

    /// Path to configuration file
    config_path: PathBuf,
}

impl DeviceConfigRegistry {
    /// Create a new device configuration registry
    pub fn new(config_dir: &PathBuf) -> Self {
        let config_path = config_dir.join("device_configs.json");
        Self {
            configs: HashMap::new(),
            config_path,
        }
    }

    /// Load device configurations from disk
    pub fn load(&mut self) -> Result<()> {
        if !self.config_path.exists() {
            debug!("Device config file not found, starting with empty registry");
            return Ok(());
        }

        let contents =
            fs::read_to_string(&self.config_path).context("Failed to read device configs file")?;

        let configs: HashMap<String, DeviceConfig> =
            serde_json::from_str(&contents).context("Failed to parse device configs")?;

        self.configs = configs;
        info!("Loaded {} device configurations", self.configs.len());

        Ok(())
    }

    /// Save device configurations to disk
    pub fn save(&self) -> Result<()> {
        let contents = serde_json::to_string_pretty(&self.configs)
            .context("Failed to serialize device configs")?;

        fs::write(&self.config_path, contents).context("Failed to write device configs file")?;

        debug!("Saved {} device configurations", self.configs.len());

        Ok(())
    }

    /// Get device configuration, creating default if not found
    pub fn get_or_create(&mut self, device_id: &str) -> &mut DeviceConfig {
        self.configs
            .entry(device_id.to_string())
            .or_insert_with(|| DeviceConfig::new(device_id.to_string()))
    }

    /// Get device configuration (read-only)
    pub fn get(&self, device_id: &str) -> Option<&DeviceConfig> {
        self.configs.get(device_id)
    }

    /// Update device configuration
    pub fn update(&mut self, device_id: &str, config: DeviceConfig) {
        self.configs.insert(device_id.to_string(), config);
    }

    /// Remove device configuration
    pub fn remove(&mut self, device_id: &str) -> Option<DeviceConfig> {
        self.configs.remove(device_id)
    }

    /// Get all device IDs with custom configurations
    pub fn device_ids(&self) -> Vec<String> {
        self.configs.keys().cloned().collect()
    }

    /// Check if device has custom configuration
    pub fn has_config(&self, device_id: &str) -> bool {
        self.configs.contains_key(device_id)
    }

    /// Get number of configured devices
    pub fn len(&self) -> usize {
        self.configs.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.configs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_config_creation() {
        let config = DeviceConfig::new("test-device-123".to_string());
        assert_eq!(config.device_id, "test-device-123");
        assert_eq!(config.nickname, None);
        assert!(config.auto_connect);
        assert!(config.show_notifications);
    }

    #[test]
    fn test_plugin_enable_override() {
        let mut config = DeviceConfig::new("test-device".to_string());
        let global_config = crate::config::PluginConfig::default();

        // Initially uses global config (all enabled by default)
        assert!(config.is_plugin_enabled("ping", &global_config));

        // Set device-specific override
        config.set_plugin_enabled("ping", false);
        assert!(!config.is_plugin_enabled("ping", &global_config));

        // Clear override, back to global
        config.clear_plugin_override("ping");
        assert!(config.is_plugin_enabled("ping", &global_config));
    }

    #[test]
    fn test_device_config_serialization() {
        let mut config = DeviceConfig::new("test-device".to_string());
        config.nickname = Some("My Phone".to_string());
        config.set_plugin_enabled("battery", false);

        let json = serde_json::to_string(&config).unwrap();
        let parsed: DeviceConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.device_id, config.device_id);
        assert_eq!(parsed.nickname, config.nickname);
        assert_eq!(parsed.plugins.enable_battery, Some(false));
    }

    #[test]
    fn test_device_registry() {
        let temp_dir = std::env::temp_dir().join("cconnect-test");
        fs::create_dir_all(&temp_dir).unwrap();

        let mut registry = DeviceConfigRegistry::new(&temp_dir);

        // Create and update config
        let config = registry.get_or_create("device-1");
        config.nickname = Some("Test Device".to_string());

        assert_eq!(registry.len(), 1);
        assert!(registry.has_config("device-1"));

        // Save and reload
        registry.save().unwrap();

        let mut registry2 = DeviceConfigRegistry::new(&temp_dir);
        registry2.load().unwrap();

        assert_eq!(registry2.len(), 1);
        let loaded = registry2.get("device-1").unwrap();
        assert_eq!(loaded.nickname, Some("Test Device".to_string()));

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }
}
