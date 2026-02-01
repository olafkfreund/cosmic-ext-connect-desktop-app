//! DBus Notification Listener
//!
//! Captures system notifications via DBus org.freedesktop.Notifications interface
//! and forwards them to connected devices.
//!
//! ## Architecture
//!
//! This module monitors the session DBus for notification events using `MatchRule`
//! to intercept `org.freedesktop.Notifications.Notify` method calls. All captured
//! notifications are filtered according to configuration and sent via an mpsc channel.
//!
//! ## DBus Notification Specification
//!
//! The freedesktop.org notification specification defines the following parameters:
//!
//! - **app_name**: Name of the application sending the notification
//! - **replaces_id**: ID of notification to replace (0 for new)
//! - **app_icon**: Icon name or image path
//! - **summary**: Title/summary text
//! - **body**: Detailed message body (may include HTML)
//! - **actions**: Array of action IDs and labels (e.g., ["reply", "Reply", "dismiss", "Dismiss"])
//! - **hints**: HashMap of additional metadata
//! - **expire_timeout**: Milliseconds until auto-dismiss (-1 = default, 0 = never)
//!
//! ## Hints
//!
//! Common hint keys include:
//! - `urgency`: 0=low, 1=normal, 2=critical
//! - `category`: Notification category (e.g., "im.received", "email.arrived")
//! - `desktop-entry`: .desktop file name for app identification
//! - `image-data`: Struct containing raw image data (width, height, rowstride, has_alpha, bits_per_sample, channels, data)
//! - `image-path`: String path to image file
//! - `icon_data`: Struct containing raw icon data (same format as image-data)
//! - `sound-file`: String path to notification sound file
//! - `action-icons`: Boolean indicating if actions have icons
//! - `transient`: Boolean, should not persist
//! - `resident`: Boolean, stays after dismissal
//!
//! ## Example
//!
//! ```rust,ignore
//! use cosmic_connect_daemon::notification_listener::{
//!     NotificationListener, NotificationListenerConfig
//! };
//!
//! let config = NotificationListenerConfig {
//!     enabled: true,
//!     excluded_apps: vec!["cosmic-notifications".to_string()],
//!     ..Default::default()
//! };
//!
//! let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
//! let listener = NotificationListener::new(config, tx).await?;
//!
//! // Listen for notifications
//! while let Some(notification) = rx.recv().await {
//!     println!("Got notification: {}", notification.summary);
//! }
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::{debug, info, trace, warn};
use zbus::{Connection, MatchRule};

/// Notification hint value types
///
/// DBus hints can contain various types of data. This enum represents
/// the most common types found in notification hints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HintValue {
    /// String value
    String(String),
    /// 32-bit integer
    Int32(i32),
    /// Unsigned 8-bit integer
    Byte(u8),
    /// Boolean value
    Boolean(bool),
    /// Image data structure
    ImageData(ImageData),
}

/// Image data from notification hints
///
/// Represents the `image-data` hint structure defined in the
/// freedesktop.org notification specification.
///
/// Format: (width, height, rowstride, has_alpha, bits_per_sample, channels, data)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageData {
    /// Image width in pixels
    pub width: i32,
    /// Image height in pixels
    pub height: i32,
    /// Bytes per row
    pub rowstride: i32,
    /// Whether image has alpha channel
    pub has_alpha: bool,
    /// Bits per color sample
    pub bits_per_sample: i32,
    /// Number of channels (3=RGB, 4=RGBA)
    pub channels: i32,
    /// Raw image data bytes
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

/// Captured notification from DBus
///
/// Contains all information from a freedesktop.org notification,
/// ready to be forwarded to connected devices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedNotification {
    /// Application name
    pub app_name: String,

    /// Notification ID (assigned by notification daemon)
    pub notification_id: u32,

    /// Application icon name or path
    pub app_icon: String,

    /// Summary/title text
    pub summary: String,

    /// Body text (may include HTML markup)
    pub body: String,

    /// Action buttons (pairs of id/label)
    pub actions: Vec<(String, String)>,

    /// Notification hints
    pub hints: HashMap<String, HintValue>,

    /// Timeout in milliseconds (-1=default, 0=never)
    pub timeout: i32,

    /// Timestamp when notification was captured
    pub timestamp: u64,
}

/// Rich notification content extracted from hints
///
/// Provides convenient access to all rich content fields from notification hints.
/// All fields are optional as hints may not be present.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct RichNotificationData {
    /// Urgency level: 0=low, 1=normal, 2=critical
    pub urgency: u8,

    /// Notification category (e.g., "im.received", "email.arrived")
    pub category: Option<String>,

    /// Desktop entry name for app identification
    pub desktop_entry: Option<String>,

    /// Raw image data embedded in notification
    pub image_data: Option<ImageData>,

    /// File path to notification image
    pub image_path: Option<String>,

    /// Raw icon data embedded in notification
    pub icon_data: Option<ImageData>,

    /// Path to notification sound file
    pub sound_file: Option<String>,

    /// Whether action buttons have icons
    pub action_icons: bool,
}

impl CapturedNotification {
    /// Get urgency level from hints
    pub fn urgency(&self) -> u8 {
        self.hints
            .get("urgency")
            .and_then(|v| match v {
                HintValue::Byte(b) => Some(*b),
                HintValue::Int32(i) => Some(*i as u8),
                _ => None,
            })
            .unwrap_or(1) // Default to normal urgency
    }

    /// Check if notification is transient (should not persist)
    pub fn is_transient(&self) -> bool {
        self.hints
            .get("transient")
            .and_then(|v| match v {
                HintValue::Boolean(b) => Some(*b),
                _ => None,
            })
            .unwrap_or(false)
    }

    /// Get notification category
    pub fn category(&self) -> Option<&str> {
        self.hints.get("category").and_then(|v| match v {
            HintValue::String(s) => Some(s.as_str()),
            _ => None,
        })
    }

    /// Get desktop entry name
    #[allow(dead_code)]
    pub fn desktop_entry(&self) -> Option<&str> {
        self.hints.get("desktop-entry").and_then(|v| match v {
            HintValue::String(s) => Some(s.as_str()),
            _ => None,
        })
    }

    /// Get image data if present
    #[allow(dead_code)]
    pub fn image_data(&self) -> Option<&ImageData> {
        self.hints.get("image-data").and_then(|v| match v {
            HintValue::ImageData(img) => Some(img),
            _ => None,
        })
    }

    /// Get image path if present
    pub fn image_path(&self) -> Option<&str> {
        self.hints.get("image-path").and_then(|v| match v {
            HintValue::String(s) => Some(s.as_str()),
            _ => None,
        })
    }

    /// Get icon data if present
    pub fn icon_data(&self) -> Option<&ImageData> {
        self.hints.get("icon_data").and_then(|v| match v {
            HintValue::ImageData(img) => Some(img),
            _ => None,
        })
    }

    /// Get sound file path if present
    #[allow(dead_code)]
    pub fn sound_file(&self) -> Option<&str> {
        self.hints.get("sound-file").and_then(|v| match v {
            HintValue::String(s) => Some(s.as_str()),
            _ => None,
        })
    }

    /// Check if actions have icons
    #[allow(dead_code)]
    pub fn action_icons(&self) -> bool {
        self.hints
            .get("action-icons")
            .and_then(|v| match v {
                HintValue::Boolean(b) => Some(*b),
                _ => None,
            })
            .unwrap_or(false)
    }

    /// Extract all rich notification data from hints
    ///
    /// Provides a convenient way to access all rich content fields
    /// in a single structured format.
    #[allow(dead_code)]
    pub fn rich_data(&self) -> RichNotificationData {
        RichNotificationData {
            urgency: self.urgency(),
            category: self.category().map(String::from),
            desktop_entry: self.desktop_entry().map(String::from),
            image_data: self.image_data().cloned(),
            image_path: self.image_path().map(String::from),
            icon_data: self.icon_data().cloned(),
            sound_file: self.sound_file().map(String::from),
            action_icons: self.action_icons(),
        }
    }
}

/// Notification listener configuration
///
/// Controls which notifications are captured and forwarded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationListenerConfig {
    /// Enable notification listener
    pub enabled: bool,

    /// Applications to exclude (exact match on app_name)
    #[serde(default)]
    pub excluded_apps: Vec<String>,

    /// Applications to include (if empty, include all non-excluded)
    #[serde(default)]
    pub included_apps: Vec<String>,

    /// Include transient notifications
    #[serde(default = "default_true")]
    pub include_transient: bool,

    /// Include low-urgency notifications
    #[serde(default = "default_true")]
    pub include_low_urgency: bool,

    /// Maximum body length (truncate if longer, 0 = no limit)
    #[serde(default)]
    pub max_body_length: usize,
}

fn default_true() -> bool {
    true
}

impl Default for NotificationListenerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            excluded_apps: vec![
                // Exclude our own notifications to prevent loops
                "CConnect".to_string(),
                "cosmic-connect".to_string(),
                // Exclude system/meta notifications
                "cosmic-notifications".to_string(),
            ],
            included_apps: Vec::new(),
            include_transient: true,
            include_low_urgency: true,
            max_body_length: 0, // No limit
        }
    }
}

impl NotificationListenerConfig {
    /// Check if an application should be captured
    fn should_capture_app(&self, app_name: &str) -> bool {
        // Check exclusion list first
        if self
            .excluded_apps
            .iter()
            .any(|excluded| excluded == app_name)
        {
            return false;
        }

        // If inclusion list is empty, accept all non-excluded
        if self.included_apps.is_empty() {
            return true;
        }

        // Check inclusion list
        self.included_apps
            .iter()
            .any(|included| included == app_name)
    }

    /// Check if a notification should be captured based on hints
    fn should_capture_notification(&self, notification: &CapturedNotification) -> bool {
        // Filter transient notifications
        if !self.include_transient && notification.is_transient() {
            return false;
        }

        // Filter low-urgency notifications
        if !self.include_low_urgency && notification.urgency() == 0 {
            return false;
        }

        true
    }

    /// Truncate body if needed
    fn truncate_body(&self, body: String) -> String {
        if self.max_body_length > 0 && body.len() > self.max_body_length {
            let truncated = body.chars().take(self.max_body_length).collect::<String>();
            format!("{}...", truncated)
        } else {
            body
        }
    }
}

/// DBus notification listener
///
/// Monitors the session DBus for org.freedesktop.Notifications.Notify calls
/// and captures notification data.
pub struct NotificationListener {
    config: NotificationListenerConfig,
    sender: mpsc::UnboundedSender<CapturedNotification>,
}

impl NotificationListener {
    /// Create a new notification listener
    ///
    /// Connects to the session DBus and sets up notification monitoring.
    ///
    /// # Arguments
    ///
    /// * `config` - Listener configuration
    /// * `sender` - Channel to send captured notifications
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    /// let listener = NotificationListener::new(
    ///     NotificationListenerConfig::default(),
    ///     tx
    /// ).await?;
    /// ```
    pub async fn new(
        config: NotificationListenerConfig,
        sender: mpsc::UnboundedSender<CapturedNotification>,
    ) -> Result<Self> {
        if !config.enabled {
            info!("Notification listener is disabled");
            return Ok(Self { config, sender });
        }

        info!("Starting notification listener");
        debug!("Excluded apps: {:?}", config.excluded_apps);
        debug!("Included apps: {:?}", config.included_apps);

        Ok(Self { config, sender })
    }

    /// Start listening for notifications
    ///
    /// This method creates a DBus match rule and processes incoming notifications.
    /// It runs in a loop and should be spawned as a background task.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tokio::spawn(async move {
    ///     if let Err(e) = listener.listen().await {
    ///         error!("Notification listener error: {}", e);
    ///     }
    /// });
    /// ```
    pub async fn listen(self) -> Result<()> {
        if !self.config.enabled {
            debug!("Notification listener disabled, not starting");
            return Ok(());
        }

        let connection = Connection::session()
            .await
            .context("Failed to connect to session DBus")?;

        info!("Connected to session DBus for notification monitoring");

        // Create match rule for Notify method calls
        let match_rule = MatchRule::builder()
            .msg_type(zbus::message::Type::MethodCall)
            .interface("org.freedesktop.Notifications")?
            .member("Notify")?
            .build();

        let mut stream = zbus::MessageStream::for_match_rule(
            match_rule,
            &connection,
            Some(256), // Buffer size
        )
        .await
        .context("Failed to create message stream")?;

        info!("Notification listener started successfully");

        use futures::StreamExt;
        while let Some(msg_result) = stream.next().await {
            match msg_result {
                Ok(msg) => {
                    if let Err(e) = self.process_notification_message(&msg).await {
                        warn!("Failed to process notification: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Error receiving DBus message: {}", e);
                }
            }
        }

        warn!("Notification listener stream ended unexpectedly");
        Ok(())
    }

    /// Process a DBus notification message
    async fn process_notification_message(&self, msg: &zbus::Message) -> Result<()> {
        // Verify this is a Notify method call
        if let Some(member) = msg.header().member() {
            if member.as_str() != "Notify" {
                return Ok(());
            }
        } else {
            return Ok(());
        }

        trace!("Processing Notify method call");

        // Parse notification parameters
        let notification = self.parse_notification(msg)?;

        // Apply filters
        if !self.config.should_capture_app(&notification.app_name) {
            trace!(
                "Skipping notification from excluded app: {}",
                notification.app_name
            );
            return Ok(());
        }

        if !self.config.should_capture_notification(&notification) {
            trace!("Skipping notification due to filter rules");
            return Ok(());
        }

        debug!(
            "Captured notification: {} - {} (urgency: {})",
            notification.app_name,
            notification.summary,
            notification.urgency()
        );

        // Send to channel
        if let Err(e) = self.sender.send(notification.clone()) {
            warn!("Failed to send notification to channel: {}", e);
        }

        Ok(())
    }

    /// Parse notification parameters from DBus message
    fn parse_notification(&self, msg: &zbus::Message) -> Result<CapturedNotification> {
        use zbus::zvariant::Value;

        // The Notify method has this signature:
        // Notify(app_name: s, replaces_id: u, app_icon: s, summary: s, body: s,
        //        actions: as, hints: a{sv}, expire_timeout: i) -> u
        let body = msg.body();

        // Extract fields using deserialize
        let (app_name, replaces_id, app_icon, summary, body_text, actions, hints_map, timeout): (
            String,
            u32,
            String,
            String,
            String,
            Vec<String>,
            HashMap<String, Value<'_>>,
            i32,
        ) = body
            .deserialize()
            .context("Failed to deserialize Notify parameters")?;

        // Parse actions into (id, label) pairs
        let mut action_pairs = Vec::new();
        let mut action_iter = actions.iter();
        while let Some(id) = action_iter.next() {
            if let Some(label) = action_iter.next() {
                action_pairs.push((id.clone(), label.clone()));
            }
        }

        // Convert hints to our HintValue enum
        let hints = self.parse_hints(hints_map)?;

        // Truncate body if configured
        let body_text = self.config.truncate_body(body_text);

        // Get current timestamp
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(CapturedNotification {
            app_name,
            notification_id: replaces_id,
            app_icon,
            summary,
            body: body_text,
            actions: action_pairs,
            hints,
            timeout,
            timestamp,
        })
    }

    /// Parse DBus hints into HintValue map
    fn parse_hints(
        &self,
        hints_map: HashMap<String, zbus::zvariant::Value<'_>>,
    ) -> Result<HashMap<String, HintValue>> {
        use zbus::zvariant::Value;

        let mut hints = HashMap::new();

        for (key, value) in hints_map {
            let hint_value = match &value {
                Value::Str(s) => HintValue::String(s.to_string()),
                Value::I32(i) => HintValue::Int32(*i),
                Value::U8(b) => HintValue::Byte(*b),
                Value::Bool(b) => HintValue::Boolean(*b),
                Value::Structure(s) => {
                    // Check if this is image-data structure
                    if key == "image-data" || key == "icon_data" {
                        self.parse_image_data(s)?
                    } else {
                        continue; // Skip unknown structures
                    }
                }
                _ => continue, // Skip unsupported types
            };

            hints.insert(key, hint_value);
        }

        Ok(hints)
    }

    /// Parse image-data hint structure
    fn parse_image_data(&self, structure: &zbus::zvariant::Structure<'_>) -> Result<HintValue> {
        use zbus::zvariant::Value;

        let fields = structure.fields();
        if fields.len() != 7 {
            return Err(anyhow::anyhow!(
                "Invalid image-data structure: expected 7 fields, got {}",
                fields.len()
            ));
        }

        let width = match &fields[0] {
            Value::I32(i) => *i,
            _ => return Err(anyhow::anyhow!("Invalid width type in image-data")),
        };

        let height = match &fields[1] {
            Value::I32(i) => *i,
            _ => return Err(anyhow::anyhow!("Invalid height type in image-data")),
        };

        let rowstride = match &fields[2] {
            Value::I32(i) => *i,
            _ => return Err(anyhow::anyhow!("Invalid rowstride type in image-data")),
        };

        let has_alpha = match &fields[3] {
            Value::Bool(b) => *b,
            _ => return Err(anyhow::anyhow!("Invalid has_alpha type in image-data")),
        };

        let bits_per_sample = match &fields[4] {
            Value::I32(i) => *i,
            _ => {
                return Err(anyhow::anyhow!(
                    "Invalid bits_per_sample type in image-data"
                ))
            }
        };

        let channels = match &fields[5] {
            Value::I32(i) => *i,
            _ => return Err(anyhow::anyhow!("Invalid channels type in image-data")),
        };

        let data = match &fields[6] {
            Value::Array(arr) => {
                // Convert array of bytes to Vec<u8>
                arr.iter()
                    .filter_map(|v| match v {
                        Value::U8(b) => Some(*b),
                        _ => None,
                    })
                    .collect()
            }
            _ => return Err(anyhow::anyhow!("Invalid data type in image-data")),
        };

        Ok(HintValue::ImageData(ImageData {
            width,
            height,
            rowstride,
            has_alpha,
            bits_per_sample,
            channels,
            data,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_listener_config_default() {
        let config = NotificationListenerConfig::default();
        assert!(config.enabled);
        assert!(config.include_transient);
        assert!(config.include_low_urgency);
        assert_eq!(config.max_body_length, 0);
        assert!(config.excluded_apps.contains(&"CConnect".to_string()));
    }

    #[test]
    fn test_should_capture_app() {
        let mut config = NotificationListenerConfig::default();

        // Should exclude CConnect
        assert!(!config.should_capture_app("CConnect"));

        // Should include other apps by default
        assert!(config.should_capture_app("Firefox"));

        // Test inclusion list
        config.included_apps = vec!["Firefox".to_string()];
        assert!(config.should_capture_app("Firefox"));
        assert!(!config.should_capture_app("Chrome"));
    }

    #[test]
    fn test_truncate_body() {
        let mut config = NotificationListenerConfig::default();
        config.max_body_length = 10;

        let short = "Short".to_string();
        assert_eq!(config.truncate_body(short.clone()), short);

        let long = "This is a very long message".to_string();
        let truncated = config.truncate_body(long);
        assert_eq!(truncated, "This is a ...");
    }

    #[test]
    fn test_captured_notification_urgency() {
        let mut notification = create_test_notification();

        // Default urgency
        assert_eq!(notification.urgency(), 1);

        // Set urgency via hint
        notification
            .hints
            .insert("urgency".to_string(), HintValue::Byte(2));
        assert_eq!(notification.urgency(), 2);
    }

    #[test]
    fn test_captured_notification_is_transient() {
        let mut notification = create_test_notification();

        assert!(!notification.is_transient());

        notification
            .hints
            .insert("transient".to_string(), HintValue::Boolean(true));
        assert!(notification.is_transient());
    }

    #[test]
    fn test_captured_notification_category() {
        let mut notification = create_test_notification();

        assert_eq!(notification.category(), None);

        notification.hints.insert(
            "category".to_string(),
            HintValue::String("im.received".to_string()),
        );
        assert_eq!(notification.category(), Some("im.received"));
    }

    #[test]
    fn test_image_data_structure() {
        let image_data = ImageData {
            width: 64,
            height: 64,
            rowstride: 256,
            has_alpha: true,
            bits_per_sample: 8,
            channels: 4,
            data: vec![255u8; 16384],
        };

        assert_eq!(image_data.width, 64);
        assert_eq!(image_data.height, 64);
        assert_eq!(image_data.data.len(), 16384);
    }

    #[test]
    fn test_should_capture_notification_filters() {
        let mut config = NotificationListenerConfig::default();
        let notification = create_test_notification();

        // Should capture by default
        assert!(config.should_capture_notification(&notification));

        // Test transient filter
        config.include_transient = false;
        let mut transient_notif = notification.clone();
        transient_notif
            .hints
            .insert("transient".to_string(), HintValue::Boolean(true));
        assert!(!config.should_capture_notification(&transient_notif));

        // Test low urgency filter
        config.include_transient = true;
        config.include_low_urgency = false;
        let mut low_urgency_notif = notification.clone();
        low_urgency_notif
            .hints
            .insert("urgency".to_string(), HintValue::Byte(0));
        assert!(!config.should_capture_notification(&low_urgency_notif));
    }

    #[test]
    fn test_captured_notification_image_path() {
        let mut notification = create_test_notification();

        assert_eq!(notification.image_path(), None);

        notification.hints.insert(
            "image-path".to_string(),
            HintValue::String("/path/to/image.png".to_string()),
        );
        assert_eq!(notification.image_path(), Some("/path/to/image.png"));
    }

    #[test]
    fn test_captured_notification_icon_data() {
        let mut notification = create_test_notification();

        assert!(notification.icon_data().is_none());

        let icon = ImageData {
            width: 32,
            height: 32,
            rowstride: 128,
            has_alpha: true,
            bits_per_sample: 8,
            channels: 4,
            data: vec![255u8; 4096],
        };

        notification
            .hints
            .insert("icon_data".to_string(), HintValue::ImageData(icon.clone()));
        assert!(notification.icon_data().is_some());
        assert_eq!(notification.icon_data().unwrap().width, 32);
    }

    #[test]
    fn test_captured_notification_sound_file() {
        let mut notification = create_test_notification();

        assert_eq!(notification.sound_file(), None);

        notification.hints.insert(
            "sound-file".to_string(),
            HintValue::String("/usr/share/sounds/notification.ogg".to_string()),
        );
        assert_eq!(
            notification.sound_file(),
            Some("/usr/share/sounds/notification.ogg")
        );
    }

    #[test]
    fn test_captured_notification_action_icons() {
        let mut notification = create_test_notification();

        assert!(!notification.action_icons());

        notification
            .hints
            .insert("action-icons".to_string(), HintValue::Boolean(true));
        assert!(notification.action_icons());
    }

    #[test]
    fn test_rich_notification_data() {
        let mut notification = create_test_notification();

        // Add various hints
        notification
            .hints
            .insert("urgency".to_string(), HintValue::Byte(2));
        notification.hints.insert(
            "category".to_string(),
            HintValue::String("im.received".to_string()),
        );
        notification.hints.insert(
            "desktop-entry".to_string(),
            HintValue::String("firefox".to_string()),
        );
        notification.hints.insert(
            "image-path".to_string(),
            HintValue::String("/tmp/image.png".to_string()),
        );
        notification.hints.insert(
            "sound-file".to_string(),
            HintValue::String("/tmp/sound.ogg".to_string()),
        );
        notification
            .hints
            .insert("action-icons".to_string(), HintValue::Boolean(true));

        let rich_data = notification.rich_data();

        assert_eq!(rich_data.urgency, 2);
        assert_eq!(rich_data.category, Some("im.received".to_string()));
        assert_eq!(rich_data.desktop_entry, Some("firefox".to_string()));
        assert_eq!(rich_data.image_path, Some("/tmp/image.png".to_string()));
        assert_eq!(rich_data.sound_file, Some("/tmp/sound.ogg".to_string()));
        assert!(rich_data.action_icons);
    }

    #[test]
    fn test_rich_notification_data_defaults() {
        let notification = create_test_notification();
        let rich_data = notification.rich_data();

        // Check defaults when hints are missing
        assert_eq!(rich_data.urgency, 1); // Default normal
        assert_eq!(rich_data.category, None);
        assert_eq!(rich_data.desktop_entry, None);
        assert_eq!(rich_data.image_data, None);
        assert_eq!(rich_data.image_path, None);
        assert_eq!(rich_data.icon_data, None);
        assert_eq!(rich_data.sound_file, None);
        assert!(!rich_data.action_icons); // Default false
    }

    #[test]
    fn test_rich_notification_data_with_image_data() {
        let mut notification = create_test_notification();

        let image = ImageData {
            width: 128,
            height: 128,
            rowstride: 512,
            has_alpha: true,
            bits_per_sample: 8,
            channels: 4,
            data: vec![0u8; 65536],
        };

        notification.hints.insert(
            "image-data".to_string(),
            HintValue::ImageData(image.clone()),
        );

        let rich_data = notification.rich_data();
        assert!(rich_data.image_data.is_some());
        assert_eq!(rich_data.image_data.unwrap().width, 128);
    }

    // Helper function to create test notification
    fn create_test_notification() -> CapturedNotification {
        CapturedNotification {
            app_name: "TestApp".to_string(),
            notification_id: 1,
            app_icon: "test-icon".to_string(),
            summary: "Test Summary".to_string(),
            body: "Test body".to_string(),
            actions: vec![],
            hints: HashMap::new(),
            timeout: 5000,
            timestamp: 1234567890,
        }
    }
}
