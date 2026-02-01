//! D-Bus Service Module
//!
//! Provides a D-Bus interface for cosmic-connect integration.
//! Receives message notifications from the daemon and controls popup visibility.

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info};
use zbus::{connection, interface, Connection};

/// Notification data received via D-Bus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationData {
    /// Android package name (e.g., com.google.android.apps.messaging)
    pub app_package: String,
    /// App display name (e.g., Messages)
    pub app_name: String,
    /// Notification title (usually sender name)
    pub title: String,
    /// Message content
    pub text: String,
    /// Optional conversation ID for deep linking
    pub conversation_id: Option<String>,
    /// Message timestamp (Unix milliseconds)
    pub timestamp: i64,
    /// Device ID that sent the notification
    pub device_id: String,
    /// Optional icon data as base64
    pub icon_data: Option<String>,
}

impl NotificationData {
    /// Create a new notification data instance
    pub fn new(
        app_package: String,
        app_name: String,
        title: String,
        text: String,
        device_id: String,
    ) -> Self {
        Self {
            app_package,
            app_name,
            title,
            text,
            conversation_id: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
            device_id,
            icon_data: None,
        }
    }
}

/// D-Bus commands sent to the application
#[derive(Debug, Clone)]
pub enum DbusCommand {
    /// Show popup for a specific messenger
    ShowMessenger(String),
    /// Hide the popup
    HidePopup,
    /// Toggle popup visibility
    TogglePopup,
    /// Received a notification
    NotificationReceived(NotificationData),
}

/// D-Bus service for the messages popup
pub struct MessagesPopupService {
    sender: mpsc::Sender<DbusCommand>,
    visible: std::sync::atomic::AtomicBool,
}

impl MessagesPopupService {
    pub fn new(sender: mpsc::Sender<DbusCommand>) -> Self {
        Self {
            sender,
            visible: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

#[interface(name = "org.cosmicde.MessagesPopup")]
impl MessagesPopupService {
    /// Called by cosmic-connect when a message notification arrives
    #[allow(clippy::too_many_arguments)]
    async fn notify_message(
        &self,
        app_package: String,
        app_name: String,
        title: String,
        text: String,
        conversation_id: String,
        timestamp: i64,
        device_id: String,
    ) {
        debug!(
            "D-Bus: notify_message from {} - {} (package: {})",
            device_id, title, app_package
        );

        let data = NotificationData {
            app_package,
            app_name,
            title,
            text,
            conversation_id: (!conversation_id.is_empty()).then_some(conversation_id),
            timestamp,
            device_id,
            icon_data: None,
        };

        if let Err(e) = self
            .sender
            .send(DbusCommand::NotificationReceived(data))
            .await
        {
            error!("Failed to send notification to app: {}", e);
        }
    }

    /// Open specific messenger popup
    async fn open_messenger(&self, messenger_id: String) {
        debug!("D-Bus: open_messenger - {}", messenger_id);

        if let Err(e) = self
            .sender
            .send(DbusCommand::ShowMessenger(messenger_id))
            .await
        {
            error!("Failed to send show command: {}", e);
        }
    }

    /// Hide the popup
    async fn hide(&self) {
        debug!("D-Bus: hide popup");

        if let Err(e) = self.sender.send(DbusCommand::HidePopup).await {
            error!("Failed to send hide command: {}", e);
        }
    }

    /// Toggle popup visibility
    async fn toggle(&self) {
        debug!("D-Bus: toggle popup");

        if let Err(e) = self.sender.send(DbusCommand::TogglePopup).await {
            error!("Failed to send toggle command: {}", e);
        }
    }

    /// Check if popup is currently visible
    fn is_visible(&self) -> bool {
        self.visible.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Set visibility state (called internally)
    fn set_visible(&self, visible: bool) {
        self.visible
            .store(visible, std::sync::atomic::Ordering::Relaxed);
    }

    /// Get list of supported messenger IDs
    fn get_supported_messengers(&self) -> Vec<String> {
        vec![
            "google-messages".to_string(),
            "whatsapp".to_string(),
            "telegram".to_string(),
            "signal".to_string(),
            "discord".to_string(),
            "slack".to_string(),
        ]
    }

    /// Ping to check if service is alive
    fn ping(&self) -> String {
        "pong".to_string()
    }
}

/// Start the D-Bus service
pub async fn start_dbus_service(sender: mpsc::Sender<DbusCommand>) -> zbus::Result<Connection> {
    let service = MessagesPopupService::new(sender);

    let connection = connection::Builder::session()?
        .name("org.cosmicde.MessagesPopup")?
        .serve_at("/org/cosmicde/MessagesPopup", service)?
        .build()
        .await?;

    info!("D-Bus service started: org.cosmicde.MessagesPopup");

    Ok(connection)
}

/// Client for calling the D-Bus service from other applications
#[allow(dead_code)]
pub struct MessagesPopupClient {
    connection: Connection,
}

#[allow(dead_code)]
impl MessagesPopupClient {
    /// Connect to the messages popup D-Bus service
    pub async fn connect() -> zbus::Result<Self> {
        let connection = Connection::session().await?;
        Ok(Self { connection })
    }

    /// Send a notification to the messages popup
    pub async fn notify_message(&self, data: &NotificationData) -> zbus::Result<()> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            "org.cosmicde.MessagesPopup",
            "/org/cosmicde/MessagesPopup",
            "org.cosmicde.MessagesPopup",
        )
        .await?;

        proxy
            .call_method(
                "NotifyMessage",
                &(
                    &data.app_package,
                    &data.app_name,
                    &data.title,
                    &data.text,
                    data.conversation_id.as_deref().unwrap_or(""),
                    data.timestamp,
                    &data.device_id,
                ),
            )
            .await?;

        Ok(())
    }

    /// Open a specific messenger
    pub async fn open_messenger(&self, messenger_id: &str) -> zbus::Result<()> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            "org.cosmicde.MessagesPopup",
            "/org/cosmicde/MessagesPopup",
            "org.cosmicde.MessagesPopup",
        )
        .await?;

        proxy.call_method("OpenMessenger", &(messenger_id,)).await?;

        Ok(())
    }

    /// Toggle the popup visibility
    pub async fn toggle(&self) -> zbus::Result<()> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            "org.cosmicde.MessagesPopup",
            "/org/cosmicde/MessagesPopup",
            "org.cosmicde.MessagesPopup",
        )
        .await?;

        proxy.call_method("Toggle", &()).await?;

        Ok(())
    }

    /// Check if the service is available
    pub async fn ping(&self) -> zbus::Result<bool> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            "org.cosmicde.MessagesPopup",
            "/org/cosmicde/MessagesPopup",
            "org.cosmicde.MessagesPopup",
        )
        .await?;

        let response: String = proxy.call_method("Ping", &()).await?.body().deserialize()?;

        Ok(response == "pong")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_data_new() {
        let data = NotificationData::new(
            "com.google.android.apps.messaging".to_string(),
            "Messages".to_string(),
            "John Doe".to_string(),
            "Hello there!".to_string(),
            "device-123".to_string(),
        );

        assert_eq!(data.app_package, "com.google.android.apps.messaging");
        assert_eq!(data.title, "John Doe");
        assert_eq!(data.text, "Hello there!");
        assert!(data.conversation_id.is_none());
        assert!(data.timestamp > 0);
    }
}
