//! WebView Management Module
//!
//! Manages WebView instances for different messaging services.
//! Each messenger gets its own WebView with persistent session storage.

use crate::config::Config;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info};

/// WebView context for persisting sessions
#[derive(Debug)]
#[allow(dead_code)]
pub struct WebViewContext {
    /// Messenger identifier
    pub messenger_id: String,
    /// Web URL
    pub url: String,
    /// Data directory for cookies/storage
    pub data_dir: PathBuf,
    /// Whether the WebView is currently loaded
    pub is_loaded: bool,
    /// Last load timestamp
    pub last_loaded: Option<chrono::DateTime<chrono::Utc>>,
}

impl WebViewContext {
    /// Create a new WebView context
    pub fn new(messenger_id: &str, url: &str) -> Self {
        let data_dir = Config::data_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp/cosmic-messages-popup"))
            .join(messenger_id);

        Self {
            messenger_id: messenger_id.to_string(),
            url: url.to_string(),
            data_dir,
            is_loaded: false,
            last_loaded: None,
        }
    }

    /// Ensure the data directory exists
    pub fn ensure_data_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.data_dir)
            .context("Failed to create WebView data directory")?;
        Ok(())
    }
}

/// Manager for multiple WebView instances
pub struct WebViewManager {
    /// Contexts for each messenger
    contexts: HashMap<String, WebViewContext>,
    /// Currently active messenger
    current: Option<String>,
    /// Configuration
    config: Config,
}

#[allow(dead_code)]
impl WebViewManager {
    /// Create a new WebView manager
    pub fn new(config: Config) -> Self {
        Self {
            contexts: HashMap::new(),
            current: None,
            config,
        }
    }

    /// Update configuration
    pub fn update_config(&mut self, config: Config) {
        self.config = config;
    }

    /// Get or create a WebView context for a messenger
    pub fn get_or_create_context(&mut self, messenger_id: &str) -> Result<&mut WebViewContext> {
        if !self.contexts.contains_key(messenger_id) {
            let url = self
                .config
                .enabled_messengers
                .iter()
                .find(|m| m.id == messenger_id)
                .map(|m| m.web_url.clone())
                .unwrap_or_default();

            if url.is_empty() {
                anyhow::bail!("Unknown messenger: {}", messenger_id);
            }

            let context = WebViewContext::new(messenger_id, &url);
            context.ensure_data_dir()?;

            info!("Created WebView context for {} at {}", messenger_id, url);
            self.contexts.insert(messenger_id.to_string(), context);
        }

        Ok(self.contexts.get_mut(messenger_id).unwrap())
    }

    /// Set the currently active messenger
    pub fn set_current(&mut self, messenger_id: &str) -> Result<()> {
        // Ensure context exists
        self.get_or_create_context(messenger_id)?;
        self.current = Some(messenger_id.to_string());
        debug!("Set current messenger to {}", messenger_id);
        Ok(())
    }

    /// Get the current messenger ID
    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// Get the current context
    pub fn current_context(&self) -> Option<&WebViewContext> {
        self.current.as_ref().and_then(|id| self.contexts.get(id))
    }

    /// Get the current context mutably
    pub fn current_context_mut(&mut self) -> Option<&mut WebViewContext> {
        self.current
            .as_ref()
            .and_then(|id| self.contexts.get_mut(id))
    }

    /// Get the URL for the current messenger
    pub fn current_url(&self) -> Option<&str> {
        self.current_context().map(|ctx| ctx.url.as_str())
    }

    /// Mark the current WebView as loaded
    pub fn mark_loaded(&mut self) {
        if let Some(ctx) = self.current_context_mut() {
            ctx.is_loaded = true;
            ctx.last_loaded = Some(chrono::Utc::now());
        }
    }

    /// Check if a messenger WebView is loaded
    pub fn is_loaded(&self, messenger_id: &str) -> bool {
        self.contexts
            .get(messenger_id)
            .map(|ctx| ctx.is_loaded)
            .unwrap_or(false)
    }

    /// Get list of loaded messengers
    pub fn loaded_messengers(&self) -> Vec<&str> {
        self.contexts
            .iter()
            .filter(|(_, ctx)| ctx.is_loaded)
            .map(|(id, _)| id.as_str())
            .collect()
    }

    /// Clear all WebView data for a messenger
    pub fn clear_data(&mut self, messenger_id: &str) -> Result<()> {
        if let Some(ctx) = self.contexts.get(messenger_id) {
            if ctx.data_dir.exists() {
                std::fs::remove_dir_all(&ctx.data_dir).context("Failed to clear WebView data")?;
                info!("Cleared WebView data for {}", messenger_id);
            }
        }

        // Reset the context
        self.contexts.remove(messenger_id);

        Ok(())
    }

    /// Get messenger display name
    pub fn get_display_name(&self, messenger_id: &str) -> String {
        self.config
            .enabled_messengers
            .iter()
            .find(|m| m.id == messenger_id)
            .map(|m| m.name.clone())
            .unwrap_or_else(|| messenger_id.to_string())
    }

    /// Get all available messenger IDs
    pub fn available_messengers(&self) -> Vec<&str> {
        self.config
            .enabled_messengers
            .iter()
            .filter(|m| m.enabled)
            .map(|m| m.id.as_str())
            .collect()
    }

    /// Navigate to a different URL in the current WebView
    pub fn navigate(&mut self, url: &str) {
        if let Some(ctx) = self.current_context_mut() {
            ctx.url = url.to_string();
            ctx.is_loaded = false;
            debug!("WebView navigating to {}", url);
        }
    }
}

/// Information about a WebView for display purposes
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WebViewInfo {
    pub messenger_id: String,
    pub display_name: String,
    pub url: String,
    pub is_loaded: bool,
    pub is_current: bool,
}

impl WebViewManager {
    /// Get info about all WebViews
    pub fn get_all_info(&self) -> Vec<WebViewInfo> {
        self.config
            .enabled_messengers
            .iter()
            .filter(|m| m.enabled)
            .map(|m| WebViewInfo {
                messenger_id: m.id.clone(),
                display_name: m.name.clone(),
                url: m.web_url.clone(),
                is_loaded: self.contexts.get(&m.id).is_some_and(|c| c.is_loaded),
                is_current: self.current.as_ref() == Some(&m.id),
            })
            .collect()
    }
}

/// JavaScript injection for common operations
#[allow(dead_code)]
pub mod js {
    /// Clear all local storage
    pub const CLEAR_STORAGE: &str = "localStorage.clear(); sessionStorage.clear();";

    /// Get current URL
    pub const GET_URL: &str = "window.location.href";

    /// Check if page is loaded
    pub const IS_LOADED: &str = "document.readyState === 'complete'";

    /// Focus the message input (Google Messages)
    pub const FOCUS_INPUT_GOOGLE: &str =
        "document.querySelector('mws-message-compose textarea')?.focus()";

    /// Focus the message input (WhatsApp)
    pub const FOCUS_INPUT_WHATSAPP: &str = r#"
        document.querySelector('[data-tab="10"]')?.focus()
    "#;

    /// Focus the message input (Telegram)
    pub const FOCUS_INPUT_TELEGRAM: &str = r#"
        document.querySelector('.composer_rich_textarea')?.focus()
    "#;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webview_context_new() {
        let ctx = WebViewContext::new("google-messages", "https://messages.google.com/web");
        assert_eq!(ctx.messenger_id, "google-messages");
        assert!(!ctx.is_loaded);
        assert!(ctx.data_dir.to_string_lossy().contains("google-messages"));
    }

    #[test]
    fn test_webview_manager() {
        let config = Config::default();
        let mut manager = WebViewManager::new(config);

        assert!(manager.current().is_none());

        let result = manager.set_current("google-messages");
        assert!(result.is_ok());
        assert_eq!(manager.current(), Some("google-messages"));

        let url = manager.current_url();
        assert!(url.is_some());
        assert!(url.unwrap().contains("messages.google.com"));
    }

    #[test]
    fn test_available_messengers() {
        let config = Config::default();
        let manager = WebViewManager::new(config);

        let available = manager.available_messengers();
        assert!(available.contains(&"google-messages"));
        assert!(available.contains(&"whatsapp"));
        assert!(available.contains(&"telegram"));
    }

    #[test]
    fn test_mark_loaded() {
        let config = Config::default();
        let mut manager = WebViewManager::new(config);

        manager.set_current("google-messages").unwrap();
        assert!(!manager.is_loaded("google-messages"));

        manager.mark_loaded();
        assert!(manager.is_loaded("google-messages"));
    }
}
