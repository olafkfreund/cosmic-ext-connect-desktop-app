//! COSMIC Desktop Notifications Integration
//!
//! Integrates KDE Connect events with COSMIC Desktop's notification system
//! using the freedesktop.org DBus notification specification.

use anyhow::{Context, Result};
use std::collections::HashMap;
use tracing::debug;
use zbus::Connection;

/// COSMIC notification client
///
/// Sends notifications to COSMIC Desktop via DBus using the
/// org.freedesktop.Notifications interface.
#[derive(Debug, Clone)]
pub struct CosmicNotifier {
    connection: Connection,
}

/// Notification urgency level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    /// Low priority notification
    Low = 0,
    /// Normal priority notification (default)
    Normal = 1,
    /// Critical notification that requires attention
    Critical = 2,
}

/// Notification builder for COSMIC Desktop
#[derive(Debug, Clone)]
pub struct NotificationBuilder {
    app_name: String,
    summary: String,
    body: String,
    icon: String,
    urgency: Urgency,
    timeout: i32,
    actions: Vec<(String, String)>,
    hints: HashMap<String, zbus::zvariant::Value<'static>>,
}

impl NotificationBuilder {
    /// Create a new notification builder
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            app_name: "KDE Connect".to_string(),
            summary: summary.into(),
            body: String::new(),
            icon: "phone-symbolic".to_string(),
            urgency: Urgency::Normal,
            timeout: 5000, // 5 seconds default
            actions: Vec::new(),
            hints: HashMap::new(),
        }
    }

    /// Set notification body text
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    /// Set notification icon
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = icon.into();
        self
    }

    /// Set notification urgency
    pub fn urgency(mut self, urgency: Urgency) -> Self {
        self.urgency = urgency;
        self
    }

    /// Set notification timeout in milliseconds
    pub fn timeout(mut self, timeout_ms: i32) -> Self {
        self.timeout = timeout_ms;
        self
    }

    /// Add an action button to the notification
    pub fn action(mut self, id: impl Into<String>, label: impl Into<String>) -> Self {
        self.actions.push((id.into(), label.into()));
        self
    }

    /// Set a custom hint
    pub fn hint(
        mut self,
        key: impl Into<String>,
        value: zbus::zvariant::Value<'static>,
    ) -> Self {
        self.hints.insert(key.into(), value);
        self
    }

    /// Build and return the notification parameters
    fn build(mut self) -> NotificationParams {
        // Add urgency hint
        self.hints.insert(
            "urgency".to_string(),
            zbus::zvariant::Value::U8(self.urgency as u8),
        );

        // Add category hint for KDE Connect notifications
        self.hints.insert(
            "category".to_string(),
            zbus::zvariant::Value::Str("kde-connect".into()),
        );

        // Flatten actions into a single Vec<String>
        let actions_flat: Vec<String> = self
            .actions
            .into_iter()
            .flat_map(|(id, label)| vec![id, label])
            .collect();

        NotificationParams {
            app_name: self.app_name,
            replaces_id: 0,
            icon: self.icon,
            summary: self.summary,
            body: self.body,
            actions: actions_flat,
            hints: self.hints,
            timeout: self.timeout,
        }
    }
}

/// Notification parameters for DBus call
#[derive(Debug)]
struct NotificationParams {
    app_name: String,
    replaces_id: u32,
    icon: String,
    summary: String,
    body: String,
    actions: Vec<String>,
    hints: HashMap<String, zbus::zvariant::Value<'static>>,
    timeout: i32,
}

impl CosmicNotifier {
    /// Create a new COSMIC notifier
    ///
    /// Connects to the session DBus and prepares to send notifications.
    pub async fn new() -> Result<Self> {
        let connection = Connection::session()
            .await
            .context("Failed to connect to session DBus")?;

        debug!("Connected to COSMIC notifications service");

        Ok(Self { connection })
    }

    /// Send a notification to COSMIC Desktop
    ///
    /// # Example
    ///
    /// ```ignore
    /// let notifier = CosmicNotifier::new().await?;
    ///
    /// notifier.send(
    ///     NotificationBuilder::new("Ping from Phone")
    ///         .body("Hello from your phone!")
    ///         .icon("phone-symbolic")
    /// ).await?;
    /// ```
    pub async fn send(&self, builder: NotificationBuilder) -> Result<u32> {
        let params = builder.build();

        let proxy = zbus::Proxy::new(
            &self.connection,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
        .await
        .context("Failed to create notifications proxy")?;

        let notification_id: u32 = proxy
            .call_method(
                "Notify",
                &(
                    &params.app_name,
                    params.replaces_id,
                    &params.icon,
                    &params.summary,
                    &params.body,
                    &params.actions,
                    &params.hints,
                    params.timeout,
                ),
            )
            .await
            .context("Failed to send notification")?
            .body()
            .deserialize()
            .context("Failed to parse notification ID")?;

        debug!(
            "Sent notification '{}' with ID {}",
            params.summary, notification_id
        );

        Ok(notification_id)
    }

    /// Send a ping notification from a device
    pub async fn notify_ping(
        &self,
        device_name: &str,
        message: Option<&str>,
    ) -> Result<u32> {
        let body = if let Some(msg) = message {
            format!("\"{}\"", msg)
        } else {
            format!("Ping from {}", device_name)
        };

        self.send(
            NotificationBuilder::new(format!("Ping from {}", device_name))
                .body(body)
                .icon("phone-symbolic")
                .timeout(5000),
        )
        .await
    }

    /// Send a notification forwarded from a device
    pub async fn notify_from_device(
        &self,
        device_name: &str,
        app_name: &str,
        title: &str,
        text: &str,
    ) -> Result<u32> {
        let summary = format!("{} ({})", title, device_name);
        let body = if !app_name.is_empty() {
            format!("{}\n{}", app_name, text)
        } else {
            text.to_string()
        };

        self.send(
            NotificationBuilder::new(summary)
                .body(body)
                .icon("phone-symbolic")
                .timeout(10000), // 10 seconds for device notifications
        )
        .await
    }

    /// Send a pairing request notification
    pub async fn notify_pairing_request(&self, device_name: &str) -> Result<u32> {
        self.send(
            NotificationBuilder::new("Pairing Request")
                .body(format!("{} wants to pair with this device", device_name))
                .icon("phone-symbolic")
                .urgency(Urgency::Normal)
                .timeout(0) // Don't auto-dismiss pairing requests
                .action("accept", "Accept")
                .action("reject", "Reject"),
        )
        .await
    }

    /// Send a file received notification
    pub async fn notify_file_received(
        &self,
        device_name: &str,
        filename: &str,
        path: &str,
    ) -> Result<u32> {
        self.send(
            NotificationBuilder::new(format!("File from {}", device_name))
                .body(format!("Received: {}\nSaved to: {}", filename, path))
                .icon("document-save-symbolic")
                .timeout(10000)
                .action("open", "Open")
                .action("show", "Show in Files"),
        )
        .await
    }

    /// Send a battery low warning from a device
    pub async fn notify_battery_low(&self, device_name: &str, level: u8) -> Result<u32> {
        self.send(
            NotificationBuilder::new(format!("{} Battery Low", device_name))
                .body(format!("Battery level: {}%", level))
                .icon("battery-low-symbolic")
                .urgency(Urgency::Normal)
                .timeout(10000),
        )
        .await
    }

    /// Send a device connected notification
    pub async fn notify_device_connected(&self, device_name: &str) -> Result<u32> {
        self.send(
            NotificationBuilder::new("Device Connected")
                .body(format!("{} is now connected", device_name))
                .icon("phone-symbolic")
                .timeout(3000),
        )
        .await
    }

    /// Send a device disconnected notification
    pub async fn notify_device_disconnected(&self, device_name: &str) -> Result<u32> {
        self.send(
            NotificationBuilder::new("Device Disconnected")
                .body(format!("{} has disconnected", device_name))
                .icon("phone-symbolic")
                .timeout(3000),
        )
        .await
    }

    /// Close a notification by ID
    pub async fn close(&self, notification_id: u32) -> Result<()> {
        let proxy = zbus::Proxy::new(
            &self.connection,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
        .await
        .context("Failed to create notifications proxy")?;

        proxy
            .call_method("CloseNotification", &(notification_id,))
            .await
            .context("Failed to close notification")?;

        debug!("Closed notification {}", notification_id);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_builder() {
        let builder = NotificationBuilder::new("Test Summary")
            .body("Test body")
            .icon("test-icon")
            .urgency(Urgency::Critical)
            .timeout(1000);

        let params = builder.build();

        assert_eq!(params.summary, "Test Summary");
        assert_eq!(params.body, "Test body");
        assert_eq!(params.icon, "test-icon");
        assert_eq!(params.timeout, 1000);
        assert!(params.hints.contains_key("urgency"));
    }

    #[test]
    fn test_notification_with_actions() {
        let builder = NotificationBuilder::new("Test")
            .action("action1", "Label 1")
            .action("action2", "Label 2");

        let params = builder.build();

        assert_eq!(
            params.actions,
            vec![
                "action1".to_string(),
                "Label 1".to_string(),
                "action2".to_string(),
                "Label 2".to_string()
            ]
        );
    }

    #[test]
    fn test_urgency_values() {
        assert_eq!(Urgency::Low as u8, 0);
        assert_eq!(Urgency::Normal as u8, 1);
        assert_eq!(Urgency::Critical as u8, 2);
    }
}
