//! COSMIC Desktop Notifications Integration
//!
//! Integrates CConnect events with COSMIC Desktop's notification system
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
            app_name: "CConnect".to_string(),
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

        // Add category hint for CConnect notifications
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

    /// Send a pairing timeout notification
    pub async fn notify_pairing_timeout(&self, device_name: &str) -> Result<u32> {
        self.send(
            NotificationBuilder::new("Pairing Timeout")
                .body(format!("Pairing request to {} timed out", device_name))
                .icon("dialog-warning-symbolic")
                .timeout(5000),
        )
        .await
    }

    /// Send a pairing error notification
    pub async fn notify_pairing_error(&self, device_name: &str, error: &str) -> Result<u32> {
        self.send(
            NotificationBuilder::new("Pairing Failed")
                .body(format!("Failed to pair with {}: {}", device_name, error))
                .icon("dialog-error-symbolic")
                .timeout(7000),
        )
        .await
    }

    /// Send a network error notification
    pub async fn notify_network_error(
        &self,
        device_name: &str,
        error_message: &str,
    ) -> Result<u32> {
        self.send(
            NotificationBuilder::new("Connection Error")
                .body(format!(
                    "Cannot connect to {}: {}\nCheck network connection.",
                    device_name, error_message
                ))
                .icon("network-error-symbolic")
                .urgency(Urgency::Normal)
                .timeout(7000),
        )
        .await
    }

    /// Send a file transfer error notification
    pub async fn notify_file_transfer_error(
        &self,
        device_name: &str,
        filename: &str,
        error_message: &str,
    ) -> Result<u32> {
        self.send(
            NotificationBuilder::new("File Transfer Failed")
                .body(format!(
                    "Failed to send {} to {}: {}",
                    filename, device_name, error_message
                ))
                .icon("dialog-error-symbolic")
                .urgency(Urgency::Normal)
                .timeout(7000),
        )
        .await
    }

    /// Send a plugin error notification
    pub async fn notify_plugin_error(
        &self,
        plugin_name: &str,
        device_name: &str,
        error_message: &str,
    ) -> Result<u32> {
        self.send(
            NotificationBuilder::new(format!("{} Plugin Error", plugin_name))
                .body(format!(
                    "Plugin error on {}: {}",
                    device_name, error_message
                ))
                .icon("dialog-warning-symbolic")
                .urgency(Urgency::Low)
                .timeout(5000),
        )
        .await
    }

    /// Send a permission denied error notification
    pub async fn notify_permission_error(&self, operation: &str, details: &str) -> Result<u32> {
        self.send(
            NotificationBuilder::new("Permission Denied")
                .body(format!(
                    "Cannot {}: {}\nCheck file and directory permissions.",
                    operation, details
                ))
                .icon("dialog-error-symbolic")
                .urgency(Urgency::Normal)
                .timeout(7000)
                .action("settings", "Open Settings"),
        )
        .await
    }

    /// Send a disk space error notification
    pub async fn notify_disk_full_error(&self, path: &str) -> Result<u32> {
        self.send(
            NotificationBuilder::new("Disk Full")
                .body(format!(
                    "Cannot save file: Insufficient disk space at {}\nFree up space and try again.",
                    path
                ))
                .icon("drive-harddisk-symbolic")
                .urgency(Urgency::Normal)
                .timeout(10000),
        )
        .await
    }

    /// Send a configuration error notification
    pub async fn notify_configuration_error(&self, error_message: &str) -> Result<u32> {
        self.send(
            NotificationBuilder::new("Configuration Error")
                .body(format!(
                    "Configuration problem: {}\nCheck your settings.",
                    error_message
                ))
                .icon("preferences-system-symbolic")
                .urgency(Urgency::Normal)
                .timeout(7000)
                .action("settings", "Open Settings"),
        )
        .await
    }

    /// Send a certificate validation error notification
    pub async fn notify_certificate_error(&self, device_name: &str, details: &str) -> Result<u32> {
        self.send(
            NotificationBuilder::new("Security Error")
                .body(format!(
                    "Certificate validation failed for {}: {}\nYou may need to re-pair the device.",
                    device_name, details
                ))
                .icon("security-low-symbolic")
                .urgency(Urgency::Normal)
                .timeout(10000)
                .action("repair", "Re-pair Device"),
        )
        .await
    }

    /// Send a protocol version mismatch error notification
    pub async fn notify_protocol_mismatch(
        &self,
        device_name: &str,
        details: &str,
    ) -> Result<u32> {
        self.send(
            NotificationBuilder::new("Incompatible Version")
                .body(format!(
                    "{}: {}\nUpdate both applications to the latest version.",
                    device_name, details
                ))
                .icon("system-software-update-symbolic")
                .urgency(Urgency::Normal)
                .timeout(10000),
        )
        .await
    }

    /// Send a connection timeout error notification
    pub async fn notify_connection_timeout(&self, device_name: &str) -> Result<u32> {
        self.send(
            NotificationBuilder::new("Connection Timeout")
                .body(format!(
                    "Could not reach {}\nCheck if the device is on and connected to the network.",
                    device_name
                ))
                .icon("network-error-symbolic")
                .urgency(Urgency::Low)
                .timeout(7000),
        )
        .await
    }

    /// Send a generic error notification with recovery action
    pub async fn notify_error_with_recovery(
        &self,
        title: &str,
        message: &str,
        recovery_action: Option<(&str, &str)>,
    ) -> Result<u32> {
        let mut builder = NotificationBuilder::new(title)
            .body(message)
            .icon("dialog-error-symbolic")
            .urgency(Urgency::Normal)
            .timeout(7000);

        if let Some((action_id, action_label)) = recovery_action {
            builder = builder.action(action_id, action_label);
        }

        self.send(builder).await
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

    /// Subscribe to notification action signals
    ///
    /// Returns a stream of (notification_id, action_key) tuples when users click notification actions.
    pub async fn subscribe_actions(
        &self,
    ) -> Result<impl futures::Stream<Item = (u32, String)> + Unpin> {
        use futures::stream::StreamExt;

        // Create a proxy for the notifications service
        let proxy = zbus::Proxy::new(
            &self.connection,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
        .await
        .context("Failed to create notifications proxy")?;

        // Get the message stream for the proxy
        let mut stream = zbus::MessageStream::for_match_rule(
            zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender("org.freedesktop.Notifications")?
                .interface("org.freedesktop.Notifications")?
                .member("ActionInvoked")?
                .build(),
            &self.connection,
            Some(64),
        )
        .await
        .context("Failed to create message stream")?;

        let action_stream = async_stream::stream! {
            while let Some(msg_result) = stream.next().await {
                // Handle the Result from the stream
                if let Ok(msg) = msg_result {
                    // Check if this is an ActionInvoked signal
                    if let Some(member) = msg.header().member() {
                        if member.as_str() == "ActionInvoked" {
                            // Deserialize the message body
                            if let Ok((notification_id, action_key)) = msg.body().deserialize::<(u32, String)>() {
                                debug!(
                                    "Notification action invoked: id={}, action={}",
                                    notification_id, action_key
                                );
                                yield (notification_id, action_key);
                            }
                        }
                    }
                }
            }
        };

        Ok(Box::pin(action_stream))
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

    #[test]
    fn test_error_notification_with_recovery_action() {
        let builder = NotificationBuilder::new("Error Title")
            .body("Error message with recovery option")
            .action("retry", "Retry")
            .action("cancel", "Cancel");

        let params = builder.build();

        assert_eq!(params.summary, "Error Title");
        assert_eq!(params.body, "Error message with recovery option");
        assert_eq!(
            params.actions,
            vec![
                "retry".to_string(),
                "Retry".to_string(),
                "cancel".to_string(),
                "Cancel".to_string()
            ]
        );
    }

    #[test]
    fn test_notification_hints() {
        use zbus::zvariant::Value;

        let builder = NotificationBuilder::new("Test")
            .hint("x-custom-hint", Value::Str("test-value".into()));

        let params = builder.build();

        assert!(params.hints.contains_key("x-custom-hint"));
        assert!(params.hints.contains_key("urgency"));
        assert!(params.hints.contains_key("category"));
    }

    #[test]
    fn test_critical_urgency_notification() {
        let builder = NotificationBuilder::new("Critical Error")
            .body("System failure detected")
            .urgency(Urgency::Critical)
            .timeout(0); // No auto-dismiss

        let params = builder.build();

        assert_eq!(params.timeout, 0);
        if let Some(Value::U8(urgency)) = params.hints.get("urgency") {
            assert_eq!(*urgency, Urgency::Critical as u8);
        } else {
            panic!("Urgency hint not found or wrong type");
        }
    }
}
