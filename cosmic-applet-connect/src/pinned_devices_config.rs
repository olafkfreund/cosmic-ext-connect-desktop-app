use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

/// Configuration for pinned/favorited devices
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinnedDevicesConfig {
    /// Set of device IDs that are pinned/favorited
    #[serde(default)]
    pub pinned_devices: HashSet<String>,
}

impl Default for PinnedDevicesConfig {
    fn default() -> Self {
        Self {
            pinned_devices: HashSet::new(),
        }
    }
}

impl PinnedDevicesConfig {
    /// Get the config file path
    fn config_path() -> PathBuf {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("cosmic")
            .join("com.system76.CosmicAppletConnect");

        config_dir.join("pinned_devices.toml")
    }

    /// Load configuration from file, creating default if not found
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();

        if config_path.exists() {
            let contents = fs::read_to_string(&config_path)
                .context("Failed to read pinned devices config file")?;
            let config: PinnedDevicesConfig =
                toml::from_str(&contents).context("Failed to parse pinned devices config file")?;
            Ok(config)
        } else {
            Ok(PinnedDevicesConfig::default())
        }
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path();

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).context("Failed to create config directory")?;
        }

        let contents =
            toml::to_string_pretty(self).context("Failed to serialize pinned devices config")?;

        fs::write(&config_path, contents).context("Failed to write pinned devices config file")?;

        tracing::debug!("Saved pinned devices config to {}", config_path.display());
        Ok(())
    }

    /// Check if a device is pinned
    pub fn is_pinned(&self, device_id: &str) -> bool {
        self.pinned_devices.contains(device_id)
    }

    /// Toggle pin state for a device
    pub fn toggle_pin(&mut self, device_id: String) -> bool {
        if self.pinned_devices.contains(&device_id) {
            self.pinned_devices.remove(&device_id);
            false
        } else {
            self.pinned_devices.insert(device_id);
            true
        }
    }

    /// Add a device to pinned list
    #[allow(dead_code)]
    pub fn pin_device(&mut self, device_id: String) {
        self.pinned_devices.insert(device_id);
    }

    /// Remove a device from pinned list
    #[allow(dead_code)]
    pub fn unpin_device(&mut self, device_id: &str) {
        self.pinned_devices.remove(device_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PinnedDevicesConfig::default();
        assert!(config.pinned_devices.is_empty());
    }

    #[test]
    fn test_is_pinned() {
        let mut config = PinnedDevicesConfig::default();
        config.pinned_devices.insert("device1".to_string());
        assert!(config.is_pinned("device1"));
        assert!(!config.is_pinned("device2"));
    }

    #[test]
    fn test_toggle_pin() {
        let mut config = PinnedDevicesConfig::default();

        // Pin device
        let is_pinned = config.toggle_pin("device1".to_string());
        assert!(is_pinned);
        assert!(config.is_pinned("device1"));

        // Unpin device
        let is_pinned = config.toggle_pin("device1".to_string());
        assert!(!is_pinned);
        assert!(!config.is_pinned("device1"));
    }

    #[test]
    fn test_config_serialization() {
        let mut config = PinnedDevicesConfig::default();
        config.pinned_devices.insert("device1".to_string());
        config.pinned_devices.insert("device2".to_string());

        let toml_str = toml::to_string(&config).unwrap();
        let parsed: PinnedDevicesConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.pinned_devices.len(), 2);
        assert!(parsed.is_pinned("device1"));
        assert!(parsed.is_pinned("device2"));
    }
}
