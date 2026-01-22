//! Notification Sync Plugin
//!
//! Mirrors notifications between devices, enabling users to see and interact with
//! notifications from their phone on their desktop and vice versa.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `cconnect.notification` - Send or cancel notification
//! - `cconnect.notification.request` - Request all notifications or dismiss one
//! - `cconnect.notification.action` - Trigger notification action button
//! - `cconnect.notification.reply` - Reply to notification (chat apps)
//!
//! **Capabilities**:
//! - Incoming: All four packet types
//! - Outgoing: All four packet types
//!
//! ## Packet Formats
//!
//! ### Notification (`cconnect.notification`)
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.notification",
//!     "body": {
//!         "id": "notification-id-123",
//!         "appName": "Messages",
//!         "title": "New Message",
//!         "text": "Hello from your phone!",
//!         "ticker": "Messages: New Message - Hello from your phone!",
//!         "isClearable": true,
//!         "time": "1704067200000",
//!         "silent": "false"
//!     }
//! }
//! ```
//!
//! ### Cancel Notification
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.notification",
//!     "body": {
//!         "id": "notification-id-123",
//!         "isCancel": true
//!     }
//! }
//! ```
//!
//! ### Request All Notifications
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.notification.request",
//!     "body": {
//!         "request": true
//!     }
//! }
//! ```
//!
//! ### Dismiss Notification
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.notification.request",
//!     "body": {
//!         "cancel": "notification-id-123"
//!     }
//! }
//! ```
//!
//! ## Features
//!
//! - **Notification Mirroring**: Display remote notifications locally
//! - **Dismissal Sync**: Dismiss notification on one device, gone on all
//! - **Action Buttons**: Trigger notification actions (future)
//! - **Inline Replies**: Reply to messages directly (future)
//! - **Icon Transfer**: Download notification icons (future)
//!
//! ## Use Cases
//!
//! - See phone notifications on desktop
//! - Dismiss notifications from any device
//! - Reply to messages without touching phone
//! - Monitor app notifications
//!
//! ## Example
//!
//! ```rust,ignore
//! use cosmic_connect_core::plugins::notification::{
//!     NotificationPlugin, Notification
//! };
//!
//! // Create plugin
//! let mut plugin = NotificationPlugin::new();
//!
//! // Get active notifications
//! let notifications = plugin.get_all_notifications();
//! for notif in notifications {
//!     println!("{}: {}", notif.title, notif.text);
//! }
//!
//! // Dismiss a notification
//! let packet = plugin.create_dismiss_packet("notif-123");
//! ```
//!
//! ## References
//!
//! - [Valent Protocol - Notification](https://valent.andyholmes.ca/documentation/protocol.html)

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

use super::{Plugin, PluginFactory};

/// Notification data
///
/// Represents a notification from a remote device.
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_core::plugins::notification::Notification;
///
/// let notif = Notification {
///     id: "notif-123".to_string(),
///     app_name: "Messages".to_string(),
///     title: "New Message".to_string(),
///     text: "Hello!".to_string(),
///     ticker: Some("Messages: New Message - Hello!".to_string()),
///     is_clearable: true,
///     time: Some("1704067200000".to_string()),
///     silent: Some("false".to_string()),
///     only_once: None,
///     request_reply_id: None,
///     actions: None,
///     payload_hash: None,
/// };
///
/// assert_eq!(notif.id, "notif-123");
/// assert_eq!(notif.app_name, "Messages");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Notification {
    /// Unique notification ID
    pub id: String,

    /// Source application name
    #[serde(rename = "appName")]
    pub app_name: String,

    /// Notification title
    pub title: String,

    /// Notification body text
    pub text: String,

    /// Combined title and text in a single string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticker: Option<String>,

    /// Whether user can dismiss this notification
    #[serde(rename = "isClearable")]
    pub is_clearable: bool,

    /// UNIX epoch timestamp in milliseconds (as string)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,

    /// "true" for preexisting, "false" for newly received
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silent: Option<String>,

    /// Whether to only show once
    #[serde(rename = "onlyOnce", skip_serializing_if = "Option::is_none")]
    pub only_once: Option<bool>,

    /// UUID for inline reply support
    #[serde(rename = "requestReplyId", skip_serializing_if = "Option::is_none")]
    pub request_reply_id: Option<String>,

    /// Available action button names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<String>>,

    /// MD5 hash of notification icon
    #[serde(rename = "payloadHash", skip_serializing_if = "Option::is_none")]
    pub payload_hash: Option<String>,
}

impl Notification {
    /// Create a new notification
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::notification::Notification;
    ///
    /// let notif = Notification::new(
    ///     "notif-123",
    ///     "Messages",
    ///     "New Message",
    ///     "Hello from your phone!",
    ///     true
    /// );
    ///
    /// assert_eq!(notif.id, "notif-123");
    /// assert!(notif.is_clearable);
    /// ```
    pub fn new(
        id: impl Into<String>,
        app_name: impl Into<String>,
        title: impl Into<String>,
        text: impl Into<String>,
        is_clearable: bool,
    ) -> Self {
        Self {
            id: id.into(),
            app_name: app_name.into(),
            title: title.into(),
            text: text.into(),
            ticker: None,
            is_clearable,
            time: None,
            silent: None,
            only_once: None,
            request_reply_id: None,
            actions: None,
            payload_hash: None,
        }
    }

    /// Check if notification is silent (preexisting)
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::notification::Notification;
    ///
    /// let mut notif = Notification::new("1", "App", "Title", "Text", true);
    /// notif.silent = Some("true".to_string());
    /// assert!(notif.is_silent());
    /// ```
    pub fn is_silent(&self) -> bool {
        self.silent.as_deref() == Some("true")
    }

    /// Check if notification supports inline replies
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::notification::Notification;
    ///
    /// let mut notif = Notification::new("1", "App", "Title", "Text", true);
    /// notif.request_reply_id = Some("reply-uuid".to_string());
    /// assert!(notif.is_repliable());
    /// ```
    pub fn is_repliable(&self) -> bool {
        self.request_reply_id.is_some()
    }

    /// Check if notification has action buttons
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::notification::Notification;
    ///
    /// let mut notif = Notification::new("1", "App", "Title", "Text", true);
    /// notif.actions = Some(vec!["Reply".to_string(), "Mark Read".to_string()]);
    /// assert!(notif.has_actions());
    /// ```
    pub fn has_actions(&self) -> bool {
        self.actions
            .as_ref()
            .map(|a| !a.is_empty())
            .unwrap_or(false)
    }
}

/// Notification sync plugin
///
/// Handles notification mirroring between devices.
///
/// ## Features
///
/// - Receive notifications from remote devices
/// - Store active notifications
/// - Dismiss notifications
/// - Request all notifications (future)
/// - Trigger actions (future)
/// - Send replies (future)
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_core::plugins::notification::NotificationPlugin;
/// use cosmic_connect_core::Plugin;
///
/// let plugin = NotificationPlugin::new();
/// assert_eq!(plugin.name(), "notification");
///
/// // Initially no notifications
/// assert_eq!(plugin.notification_count(), 0);
/// ```
#[derive(Debug)]
pub struct NotificationPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Active notifications by ID
    notifications: Arc<RwLock<HashMap<String, Notification>>>,
}

impl NotificationPlugin {
    /// Create a new notification plugin
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::notification::NotificationPlugin;
    ///
    /// let plugin = NotificationPlugin::new();
    /// assert_eq!(plugin.notification_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            device_id: None,
            notifications: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get notification count
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::notification::NotificationPlugin;
    ///
    /// let plugin = NotificationPlugin::new();
    /// assert_eq!(plugin.notification_count(), 0);
    /// ```
    pub fn notification_count(&self) -> usize {
        self.notifications.read().ok().map(|n| n.len()).unwrap_or(0)
    }

    /// Get a notification by ID
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::notification::NotificationPlugin;
    ///
    /// let plugin = NotificationPlugin::new();
    /// assert!(plugin.get_notification("notif-123").is_none());
    /// ```
    pub fn get_notification(&self, id: &str) -> Option<Notification> {
        self.notifications.read().ok()?.get(id).cloned()
    }

    /// Get all notifications
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::notification::NotificationPlugin;
    ///
    /// let plugin = NotificationPlugin::new();
    /// let notifications = plugin.get_all_notifications();
    /// assert_eq!(notifications.len(), 0);
    /// ```
    pub fn get_all_notifications(&self) -> Vec<Notification> {
        self.notifications
            .read()
            .ok()
            .map(|n| n.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Create a notification packet
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::notification::{NotificationPlugin, Notification};
    ///
    /// let plugin = NotificationPlugin::new();
    /// let notif = Notification::new("123", "App", "Title", "Text", true);
    /// let packet = plugin.create_notification_packet(&notif);
    ///
    /// assert_eq!(packet.packet_type, "cconnect.notification");
    /// ```
    pub fn create_notification_packet(&self, notification: &Notification) -> Packet {
        let body = serde_json::to_value(notification).unwrap_or(json!({}));
        Packet::new("cconnect.notification", body)
    }

    /// Create a cancel notification packet
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::notification::NotificationPlugin;
    ///
    /// let plugin = NotificationPlugin::new();
    /// let packet = plugin.create_cancel_packet("notif-123");
    ///
    /// assert_eq!(packet.packet_type, "cconnect.notification");
    /// ```
    pub fn create_cancel_packet(&self, notification_id: &str) -> Packet {
        let body = json!({
            "id": notification_id,
            "isCancel": true
        });
        Packet::new("cconnect.notification", body)
    }

    /// Create a request all notifications packet
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::notification::NotificationPlugin;
    ///
    /// let plugin = NotificationPlugin::new();
    /// let packet = plugin.create_request_packet();
    ///
    /// assert_eq!(packet.packet_type, "cconnect.notification.request");
    /// ```
    pub fn create_request_packet(&self) -> Packet {
        let body = json!({ "request": true });
        Packet::new("cconnect.notification.request", body)
    }

    /// Create a dismiss notification packet
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::notification::NotificationPlugin;
    ///
    /// let plugin = NotificationPlugin::new();
    /// let packet = plugin.create_dismiss_packet("notif-123");
    ///
    /// assert_eq!(packet.packet_type, "cconnect.notification.request");
    /// ```
    pub fn create_dismiss_packet(&self, notification_id: &str) -> Packet {
        let body = json!({ "cancel": notification_id });
        Packet::new("cconnect.notification.request", body)
    }

    /// Handle incoming notification
    fn handle_notification(&self, packet: &Packet, device: &Device) {
        // Check for cancel
        if let Some(is_cancel) = packet.body.get("isCancel").and_then(|v| v.as_bool()) {
            if is_cancel {
                if let Some(id) = packet.body.get("id").and_then(|v| v.as_str()) {
                    if let Ok(mut notifications) = self.notifications.write() {
                        notifications.remove(id);
                        info!(
                            "Notification {} cancelled from {} ({})",
                            id,
                            device.name(),
                            device.id()
                        );
                    }
                }
                return;
            }
        }

        // Parse notification
        match serde_json::from_value::<Notification>(packet.body.clone()) {
            Ok(notification) => {
                let id = notification.id.clone();
                let silent = notification.is_silent();

                // Store notification
                if let Ok(mut notifications) = self.notifications.write() {
                    notifications.insert(id.clone(), notification.clone());
                }

                // Log notification
                if silent {
                    debug!(
                        "Preexisting notification from {} ({}): {} - {}",
                        device.name(),
                        device.id(),
                        notification.app_name,
                        notification.title
                    );
                } else {
                    info!(
                        "New notification from {} ({}): {} - {} - {}",
                        device.name(),
                        device.id(),
                        notification.app_name,
                        notification.title,
                        notification.text
                    );

                    if notification.is_repliable() {
                        debug!("Notification {} is repliable", id);
                    }
                    if notification.has_actions() {
                        debug!(
                            "Notification {} has actions: {:?}",
                            id,
                            notification.actions.as_ref().unwrap()
                        );
                    }
                }
            }
            Err(e) => {
                warn!("Failed to parse notification from {}: {}", device.name(), e);
            }
        }
    }

    /// Handle notification request
    fn handle_request(&self, packet: &Packet, device: &Device) {
        // Check for request all
        if let Some(true) = packet.body.get("request").and_then(|v| v.as_bool()) {
            info!(
                "Received request for all notifications from {} ({})",
                device.name(),
                device.id()
            );
            // Future: Send all our local notifications to device
            // Requires: Integration with COSMIC notification system to enumerate active notifications
            return;
        }

        // Check for cancel/dismiss
        if let Some(cancel_id) = packet.body.get("cancel").and_then(|v| v.as_str()) {
            info!(
                "Received dismiss request for {} from {} ({})",
                cancel_id,
                device.name(),
                device.id()
            );
            // Future: Dismiss our local notification
            // Requires: Track notification IDs and call CosmicNotifier.close(id)
        }
    }

    /// Handle notification action
    fn handle_action(&self, packet: &Packet, device: &Device) {
        let key = packet.body.get("key").and_then(|v| v.as_str());
        let action = packet.body.get("action").and_then(|v| v.as_str());

        if let (Some(key), Some(action)) = (key, action) {
            info!(
                "Received action '{}' for notification {} from {} ({})",
                action,
                key,
                device.name(),
                device.id()
            );
            // Future: Trigger the notification action button
            // Requires: Store action callbacks and execute on action packet
        }
    }

    /// Handle notification reply
    fn handle_reply(&self, packet: &Packet, device: &Device) {
        let reply_id = packet.body.get("requestReplyId").and_then(|v| v.as_str());
        let message = packet.body.get("message").and_then(|v| v.as_str());

        if let (Some(reply_id), Some(message)) = (reply_id, message) {
            info!(
                "Received reply '{}' for {} from {} ({})",
                message,
                reply_id,
                device.name(),
                device.id()
            );
            // Future: Send inline reply to originating app
            // Requires: Platform-specific integration with messaging apps
        }
    }
}

impl Default for NotificationPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for NotificationPlugin {
    fn name(&self) -> &str {
        "notification"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.notification".to_string(),
            "cconnect.notification.request".to_string(),
            "cconnect.notification.action".to_string(),
            "cconnect.notification.reply".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.notification".to_string(),
            "cconnect.notification.request".to_string(),
            "cconnect.notification.action".to_string(),
            "cconnect.notification.reply".to_string(),
        ]
    }

    async fn init(&mut self, device: &Device, _packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!(
            "Notification plugin initialized for device {}",
            device.name()
        );
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Notification plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        let count = self.notification_count();
        info!(
            "Notification plugin stopped ({} active notifications)",
            count
        );
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if packet.is_type("cconnect.notification") {
            self.handle_notification(packet, device);
        } else if packet.is_type("cconnect.notification.request") {
            self.handle_request(packet, device);
        } else if packet.is_type("cconnect.notification.action") {
            self.handle_action(packet, device);
        } else if packet.is_type("cconnect.notification.reply") {
            self.handle_reply(packet, device);
        }
        Ok(())
    }
}

/// Factory for creating NotificationPlugin instances
#[derive(Debug, Clone, Copy)]
pub struct NotificationPluginFactory;

impl PluginFactory for NotificationPluginFactory {
    fn name(&self) -> &str {
        "notification"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.notification".to_string(),
            "cconnect.notification.request".to_string(),
            "cconnect.notification.action".to_string(),
            "cconnect.notification.reply".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.notification".to_string(),
            "cconnect.notification.request".to_string(),
            "cconnect.notification.action".to_string(),
            "cconnect.notification.reply".to_string(),
        ]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(NotificationPlugin::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceInfo, DeviceType};

    fn create_test_device() -> Device {
        let info = DeviceInfo::new("Test Phone", DeviceType::Phone, 1716);
        Device::from_discovery(info)
    }

    #[test]
    fn test_notification_new() {
        let notif = Notification::new("123", "Messages", "Title", "Text", true);
        assert_eq!(notif.id, "123");
        assert_eq!(notif.app_name, "Messages");
        assert_eq!(notif.title, "Title");
        assert_eq!(notif.text, "Text");
        assert!(notif.is_clearable);
    }

    #[test]
    fn test_notification_is_silent() {
        let mut notif = Notification::new("1", "App", "Title", "Text", true);
        assert!(!notif.is_silent());

        notif.silent = Some("true".to_string());
        assert!(notif.is_silent());

        notif.silent = Some("false".to_string());
        assert!(!notif.is_silent());
    }

    #[test]
    fn test_notification_is_repliable() {
        let mut notif = Notification::new("1", "App", "Title", "Text", true);
        assert!(!notif.is_repliable());

        notif.request_reply_id = Some("reply-uuid".to_string());
        assert!(notif.is_repliable());
    }

    #[test]
    fn test_notification_has_actions() {
        let mut notif = Notification::new("1", "App", "Title", "Text", true);
        assert!(!notif.has_actions());

        notif.actions = Some(vec![]);
        assert!(!notif.has_actions());

        notif.actions = Some(vec!["Reply".to_string()]);
        assert!(notif.has_actions());
    }

    #[test]
    fn test_plugin_creation() {
        let plugin = NotificationPlugin::new();
        assert_eq!(plugin.name(), "notification");
        assert_eq!(plugin.notification_count(), 0);
    }

    #[test]
    fn test_capabilities() {
        let plugin = NotificationPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 4);
        assert!(incoming.contains(&"cconnect.notification".to_string()));
        assert!(incoming.contains(&"cconnect.notification.request".to_string()));
        assert!(incoming.contains(&"cconnect.notification.action".to_string()));
        assert!(incoming.contains(&"cconnect.notification.reply".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 4);
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = NotificationPlugin::new();
        let device = create_test_device();

        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();
        assert!(plugin.device_id.is_some());

        plugin.start().await.unwrap();
        plugin.stop().await.unwrap();
    }

    #[test]
    fn test_create_notification_packet() {
        let plugin = NotificationPlugin::new();
        let notif = Notification::new("123", "Messages", "Title", "Text", true);
        let packet = plugin.create_notification_packet(&notif);

        assert_eq!(packet.packet_type, "cconnect.notification");
        assert_eq!(packet.body.get("id").and_then(|v| v.as_str()), Some("123"));
    }

    #[test]
    fn test_create_cancel_packet() {
        let plugin = NotificationPlugin::new();
        let packet = plugin.create_cancel_packet("notif-123");

        assert_eq!(packet.packet_type, "cconnect.notification");
        assert_eq!(
            packet.body.get("id").and_then(|v| v.as_str()),
            Some("notif-123")
        );
        assert_eq!(
            packet.body.get("isCancel").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn test_create_request_packet() {
        let plugin = NotificationPlugin::new();
        let packet = plugin.create_request_packet();

        assert_eq!(packet.packet_type, "cconnect.notification.request");
        assert_eq!(
            packet.body.get("request").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn test_create_dismiss_packet() {
        let plugin = NotificationPlugin::new();
        let packet = plugin.create_dismiss_packet("notif-123");

        assert_eq!(packet.packet_type, "cconnect.notification.request");
        assert_eq!(
            packet.body.get("cancel").and_then(|v| v.as_str()),
            Some("notif-123")
        );
    }

    #[tokio::test]
    async fn test_handle_notification() {
        let mut plugin = NotificationPlugin::new();
        let device = create_test_device();
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();

        let mut device = create_test_device();
        let notif = Notification::new("123", "Messages", "New Message", "Hello!", true);
        let packet = plugin.create_notification_packet(&notif);

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        assert_eq!(plugin.notification_count(), 1);
        let stored = plugin.get_notification("123").unwrap();
        assert_eq!(stored.title, "New Message");
    }

    #[tokio::test]
    async fn test_handle_cancel_notification() {
        let mut plugin = NotificationPlugin::new();
        let device = create_test_device();
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();

        let mut device = create_test_device();

        // Add notification
        let notif = Notification::new("123", "Messages", "Title", "Text", true);
        let packet = plugin.create_notification_packet(&notif);
        plugin.handle_packet(&packet, &mut device).await.unwrap();
        assert_eq!(plugin.notification_count(), 1);

        // Cancel it
        let cancel_packet = plugin.create_cancel_packet("123");
        plugin
            .handle_packet(&cancel_packet, &mut device)
            .await
            .unwrap();
        assert_eq!(plugin.notification_count(), 0);
    }

    #[tokio::test]
    async fn test_get_all_notifications() {
        let mut plugin = NotificationPlugin::new();
        let device = create_test_device();
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();

        let mut device = create_test_device();

        // Add multiple notifications
        for i in 1..=3 {
            let notif = Notification::new(
                format!("notif-{}", i),
                "App",
                format!("Title {}", i),
                "Text",
                true,
            );
            let packet = plugin.create_notification_packet(&notif);
            plugin.handle_packet(&packet, &mut device).await.unwrap();
        }

        let all = plugin.get_all_notifications();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn test_ignore_non_notification_packets() {
        let mut plugin = NotificationPlugin::new();
        let device = create_test_device();
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new("cconnect.ping", json!({}));

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        assert_eq!(plugin.notification_count(), 0);
    }

    #[test]
    fn test_notification_serialization() {
        let notif = Notification::new("123", "App", "Title", "Text", true);
        let json = serde_json::to_value(&notif).unwrap();

        assert_eq!(json["id"], "123");
        assert_eq!(json["appName"], "App");
        assert_eq!(json["title"], "Title");
        assert_eq!(json["text"], "Text");
        assert_eq!(json["isClearable"], true);
    }
}
