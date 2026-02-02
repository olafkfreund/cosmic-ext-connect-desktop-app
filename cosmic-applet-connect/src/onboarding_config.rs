use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Applet configuration for persistent settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct AppletConfig {
    /// Whether the user has completed the first-run onboarding
    #[serde(default)]
    pub onboarding_complete: bool,
}


impl AppletConfig {
    /// Get the config file path
    fn config_path() -> PathBuf {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("cosmic")
            .join("com.system76.CosmicAppletConnect");

        config_dir.join("applet.toml")
    }

    /// Load configuration from file, creating default if not found
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();

        if config_path.exists() {
            let contents =
                fs::read_to_string(&config_path).context("Failed to read applet config file")?;
            let config: AppletConfig =
                toml::from_str(&contents).context("Failed to parse applet config file")?;
            Ok(config)
        } else {
            Ok(AppletConfig::default())
        }
    }

    /// Save configuration to file
    #[allow(dead_code)]
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path();

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).context("Failed to create config directory")?;
        }

        let contents = toml::to_string_pretty(self).context("Failed to serialize applet config")?;

        fs::write(&config_path, contents).context("Failed to write applet config file")?;

        tracing::debug!("Saved applet config to {}", config_path.display());
        Ok(())
    }

    /// Mark onboarding as complete and save
    #[allow(dead_code)]
    pub fn complete_onboarding(&mut self) -> Result<()> {
        self.onboarding_complete = true;
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppletConfig::default();
        assert!(!config.onboarding_complete);
    }

    #[test]
    fn test_config_serialization() {
        let config = AppletConfig {
            onboarding_complete: true,
        };
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: AppletConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.onboarding_complete, config.onboarding_complete);
    }
}
