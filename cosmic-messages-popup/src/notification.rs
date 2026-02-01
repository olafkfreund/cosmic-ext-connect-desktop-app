//! Notification Handling Module
//!
//! Displays COSMIC notifications when messages arrive and handles
//! the "Open" action to show the messaging popup.

use crate::config::Config;
use crate::dbus::NotificationData;
use tracing::{debug, info};

/// Messenger type derived from package name
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessengerType {
    GoogleMessages,
    WhatsApp,
    Telegram,
    Signal,
    Discord,
    Slack,
    Unknown,
}

#[allow(dead_code)]
impl MessengerType {
    /// Detect messenger type from Android package name
    pub fn from_package(package: &str) -> Self {
        match package {
            "com.google.android.apps.messaging" => Self::GoogleMessages,
            "com.whatsapp" | "com.whatsapp.w4b" => Self::WhatsApp,
            "org.telegram.messenger" | "org.telegram.messenger.web" => Self::Telegram,
            "org.thoughtcrime.securesms" => Self::Signal,
            "com.discord" => Self::Discord,
            "com.Slack" => Self::Slack,
            _ => Self::Unknown,
        }
    }

    /// Get the messenger ID used in configuration
    pub fn id(&self) -> &'static str {
        match self {
            Self::GoogleMessages => "google-messages",
            Self::WhatsApp => "whatsapp",
            Self::Telegram => "telegram",
            Self::Signal => "signal",
            Self::Discord => "discord",
            Self::Slack => "slack",
            Self::Unknown => "unknown",
        }
    }

    /// Get the icon name for this messenger
    pub fn icon(&self) -> &'static str {
        match self {
            Self::GoogleMessages => "google-messages-symbolic",
            Self::WhatsApp => "whatsapp-symbolic",
            Self::Telegram => "telegram-symbolic",
            Self::Signal => "signal-symbolic",
            Self::Discord => "discord-symbolic",
            Self::Slack => "slack-symbolic",
            Self::Unknown => "chat-symbolic",
        }
    }

    /// Get a fallback generic icon
    pub fn fallback_icon(&self) -> &'static str {
        match self {
            Self::GoogleMessages => "phone-symbolic",
            Self::WhatsApp | Self::Telegram | Self::Signal => "chat-symbolic",
            Self::Discord | Self::Slack => "system-users-symbolic",
            Self::Unknown => "mail-message-new-symbolic",
        }
    }

    /// Get the web URL for this messenger
    pub fn web_url(&self) -> &'static str {
        match self {
            Self::GoogleMessages => "https://messages.google.com/web",
            Self::WhatsApp => "https://web.whatsapp.com",
            Self::Telegram => "https://web.telegram.org",
            Self::Signal => "https://signal.link",
            Self::Discord => "https://discord.com/app",
            Self::Slack => "https://app.slack.com",
            Self::Unknown => "",
        }
    }
}

/// Notification handler for displaying and managing message notifications
pub struct NotificationHandler {
    config: Config,
}

#[allow(dead_code)]
impl NotificationHandler {
    /// Create a new notification handler
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Update the configuration
    pub fn update_config(&mut self, config: Config) {
        self.config = config;
    }

    /// Handle an incoming notification
    ///
    /// Returns the messenger ID if a notification should be shown
    pub fn handle_notification(&self, data: &NotificationData) -> Option<String> {
        // Detect messenger type
        let messenger_type = MessengerType::from_package(&data.app_package);
        let messenger_id = messenger_type.id();

        debug!(
            "Handling notification from {} ({})",
            data.app_name, messenger_id
        );

        // Check if this messenger is enabled
        if !self.config.is_messenger_enabled(messenger_id) {
            debug!(
                "Messenger {} is disabled, ignoring notification",
                messenger_id
            );
            return None;
        }

        // Check if notifications are enabled globally
        if !self.config.notifications.show_notifications {
            debug!("Notifications are disabled globally");
            return None;
        }

        info!(
            "Processing notification: {} from {} ({})",
            data.title, data.app_name, messenger_id
        );

        Some(messenger_id.to_string())
    }

    /// Check if auto-open is enabled
    pub fn should_auto_open(&self) -> bool {
        self.config.notifications.auto_open
    }

    /// Check if sound should be played
    pub fn should_play_sound(&self) -> bool {
        self.config.notifications.play_sound
    }

    /// Get the web URL for a messenger by ID
    pub fn get_messenger_url(&self, messenger_id: &str) -> Option<String> {
        self.config
            .enabled_messengers
            .iter()
            .find(|m| m.id == messenger_id)
            .map(|m| m.web_url.clone())
    }

    /// Get the display name for a messenger
    pub fn get_messenger_name(&self, messenger_id: &str) -> String {
        self.config
            .enabled_messengers
            .iter()
            .find(|m| m.id == messenger_id)
            .map(|m| m.name.clone())
            .unwrap_or_else(|| messenger_id.to_string())
    }

    /// Format notification summary for display
    pub fn format_summary(&self, data: &NotificationData) -> String {
        if data.title.is_empty() {
            &data.app_name
        } else {
            &data.title
        }
        .clone()
    }

    /// Format notification body for display
    pub fn format_body(&self, data: &NotificationData) -> String {
        const MAX_LEN: usize = 200;
        if data.text.len() > MAX_LEN {
            format!("{}...", &data.text[..MAX_LEN])
        } else {
            data.text.clone()
        }
    }

    /// Build notification actions
    pub fn build_actions(&self, messenger_id: &str) -> Vec<NotificationAction> {
        vec![
            NotificationAction {
                id: "open".to_string(),
                label: "Open".to_string(),
                messenger_id: messenger_id.to_string(),
            },
            NotificationAction {
                id: "dismiss".to_string(),
                label: "Dismiss".to_string(),
                messenger_id: messenger_id.to_string(),
            },
        ]
    }
}

/// Action that can be taken on a notification
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct NotificationAction {
    pub id: String,
    pub label: String,
    pub messenger_id: String,
}

/// Check if a package is a known messaging app
#[allow(dead_code)]
pub fn is_messaging_app(package: &str) -> bool {
    let known_packages = [
        "com.google.android.apps.messaging",
        "com.whatsapp",
        "com.whatsapp.w4b",
        "org.telegram.messenger",
        "org.thoughtcrime.securesms",
        "com.discord",
        "com.Slack",
        "com.facebook.orca",
        "com.instagram.android",
        "com.viber.voip",
        "jp.naver.line.android",
        "com.tencent.mm",
        "com.kakao.talk",
    ];

    known_packages.contains(&package)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_messenger_type_from_package() {
        assert_eq!(
            MessengerType::from_package("com.google.android.apps.messaging"),
            MessengerType::GoogleMessages
        );
        assert_eq!(
            MessengerType::from_package("com.whatsapp"),
            MessengerType::WhatsApp
        );
        assert_eq!(
            MessengerType::from_package("org.telegram.messenger"),
            MessengerType::Telegram
        );
        assert_eq!(
            MessengerType::from_package("unknown.app"),
            MessengerType::Unknown
        );
    }

    #[test]
    fn test_is_messaging_app() {
        assert!(is_messaging_app("com.google.android.apps.messaging"));
        assert!(is_messaging_app("com.whatsapp"));
        assert!(!is_messaging_app("com.spotify.music"));
    }

    #[test]
    fn test_notification_handler() {
        let config = Config::default();
        let handler = NotificationHandler::new(config);

        let data = NotificationData::new(
            "com.google.android.apps.messaging".to_string(),
            "Messages".to_string(),
            "John".to_string(),
            "Hello!".to_string(),
            "device-1".to_string(),
        );

        let result = handler.handle_notification(&data);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "google-messages");
    }

    #[test]
    fn test_notification_handler_disabled() {
        let mut config = Config::default();
        config.toggle_messenger("google-messages", false);

        let handler = NotificationHandler::new(config);

        let data = NotificationData::new(
            "com.google.android.apps.messaging".to_string(),
            "Messages".to_string(),
            "John".to_string(),
            "Hello!".to_string(),
            "device-1".to_string(),
        );

        let result = handler.handle_notification(&data);
        assert!(result.is_none());
    }

    #[test]
    fn test_format_body_truncation() {
        let config = Config::default();
        let handler = NotificationHandler::new(config);

        let long_text = "a".repeat(300);
        let data = NotificationData {
            app_package: "test".to_string(),
            app_name: "Test".to_string(),
            title: "Title".to_string(),
            text: long_text,
            conversation_id: None,
            timestamp: 0,
            device_id: "device".to_string(),
            icon_data: None,
        };

        let formatted = handler.format_body(&data);
        assert!(formatted.len() < 210); // 200 + "..."
        assert!(formatted.ends_with("..."));
    }
}
