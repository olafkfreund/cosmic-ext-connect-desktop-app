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
//! Basic notification (backward compatible):
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
//! ### Rich Notification (Desktop → Android)
//!
//! Extended notification with images, actions, urgency, and metadata:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.notification",
//!     "body": {
//!         "id": "desktop-Thunderbird-1704067200000",
//!         "appName": "Thunderbird",
//!         "title": "New Email",
//!         "text": "You have a new message from Alice",
//!         "ticker": "Thunderbird: New Email - You have a new message from Alice",
//!         "isClearable": true,
//!         "time": "1704067200000",
//!         "silent": "false",
//!         "imageData": "iVBORw0KGgoAAAANSUhEUgAAAAUA...",
//!         "appIcon": "iVBORw0KGgoAAAANSUhEUgAAAAUA...",
//!         "urgency": 1,
//!         "category": "email",
//!         "actions": ["Reply", "Mark as Read"],
//!         "actionButtons": [
//!             {"id": "reply", "label": "Reply"},
//!             {"id": "mark_read", "label": "Mark as Read"}
//!         ]
//!     }
//! }
//! ```
//!
//! **Extended Fields**:
//! - `imageData` (string, optional): Base64 encoded notification image (PNG)
//! - `appIcon` (string, optional): Base64 encoded application icon (PNG)
//! - `urgency` (number, optional): Urgency level (0=low, 1=normal, 2=critical)
//! - `category` (string, optional): Notification category (e.g., "email", "im", "device")
//! - `actions` (array, optional): Legacy action labels for backward compatibility
//! - `actionButtons` (array, optional): Structured actions with IDs and labels
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
//! ### Dismiss Notification (Desktop → Android)
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
//! ### Action Invocation (Android → Desktop)
//!
//! Sent when user taps an action button on a mirrored notification:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.notification.action",
//!     "body": {
//!         "key": "desktop-Thunderbird-1704067200000",
//!         "action": "reply"
//!     }
//! }
//! ```
//!
//! **Fields**:
//! - `key` (string): The notification ID that contains the action
//! - `action` (string): The action ID (from `actionButtons[].id`)
//!
//! ### Notification Dismissal (Android → Desktop)
//!
//! Sent when notification is dismissed on Android:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.notification",
//!     "body": {
//!         "id": "desktop-Thunderbird-1704067200000",
//!         "isCancel": true
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
//! use cosmic_connect_protocol::plugins::notification::{
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

/// Notification urgency level
///
/// Follows the freedesktop.org notification spec urgency levels.
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_protocol::plugins::notification::NotificationUrgency;
///
/// let urgency = NotificationUrgency::Critical;
/// assert_eq!(urgency.to_byte(), 2);
/// ```
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum NotificationUrgency {
    /// Low priority notification (urgency=0)
    Low = 0,
    /// Normal priority notification (urgency=1, default)
    #[default]
    Normal = 1,
    /// Critical/urgent notification (urgency=2)
    Critical = 2,
}

impl NotificationUrgency {
    /// Convert urgency to byte representation
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::NotificationUrgency;
    ///
    /// assert_eq!(NotificationUrgency::Low.to_byte(), 0);
    /// assert_eq!(NotificationUrgency::Normal.to_byte(), 1);
    /// assert_eq!(NotificationUrgency::Critical.to_byte(), 2);
    /// ```
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    /// Create urgency from byte value
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::NotificationUrgency;
    ///
    /// assert_eq!(NotificationUrgency::from_byte(0), NotificationUrgency::Low);
    /// assert_eq!(NotificationUrgency::from_byte(1), NotificationUrgency::Normal);
    /// assert_eq!(NotificationUrgency::from_byte(2), NotificationUrgency::Critical);
    /// assert_eq!(NotificationUrgency::from_byte(99), NotificationUrgency::Normal);
    /// ```
    pub fn from_byte(value: u8) -> Self {
        match value {
            0 => Self::Low,
            2 => Self::Critical,
            _ => Self::Normal,
        }
    }
}

/// Notification action button
///
/// Represents an actionable button in a notification with both an identifier
/// and display label. The identifier is used in action invocation packets.
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_protocol::plugins::notification::NotificationAction;
///
/// let action = NotificationAction {
///     id: "reply".to_string(),
///     label: "Reply".to_string(),
/// };
///
/// assert_eq!(action.id, "reply");
/// assert_eq!(action.label, "Reply");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationAction {
    /// Unique action identifier (used in action packets)
    pub id: String,

    /// User-visible action label
    pub label: String,
}

impl NotificationAction {
    /// Create a new notification action
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::NotificationAction;
    ///
    /// let action = NotificationAction::new("reply", "Reply");
    /// assert_eq!(action.id, "reply");
    /// assert_eq!(action.label, "Reply");
    /// ```
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// Clickable link within a notification
///
/// Represents a hyperlink embedded in notification text or rich content.
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_protocol::plugins::notification::NotificationLink;
///
/// let link = NotificationLink {
///     url: "https://example.com/article".to_string(),
///     title: Some("Read More".to_string()),
///     start: 10,
///     length: 9,
/// };
///
/// assert_eq!(link.url, "https://example.com/article");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationLink {
    /// The URL to open when clicked
    pub url: String,

    /// Optional display title for the link
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Starting position in the text (character offset)
    pub start: usize,

    /// Length of the linked text (character count)
    pub length: usize,
}

impl NotificationLink {
    /// Create a new notification link
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::NotificationLink;
    ///
    /// let link = NotificationLink::new(
    ///     "https://example.com",
    ///     Some("Example"),
    ///     0,
    ///     7
    /// );
    ///
    /// assert_eq!(link.url, "https://example.com");
    /// assert_eq!(link.title, Some("Example".to_string()));
    /// ```
    pub fn new(
        url: impl Into<String>,
        title: Option<impl Into<String>>,
        start: usize,
        length: usize,
    ) -> Self {
        Self {
            url: url.into(),
            title: title.map(|t| t.into()),
            start,
            length,
        }
    }
}

/// Notification data
///
/// Represents a notification from a remote device.
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_protocol::plugins::notification::Notification;
///
/// let notif = Notification::new("notif-123", "Messages", "New Message", "Hello!", true);
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

    /// Whether this notification is from a messaging app
    #[serde(rename = "isMessagingApp", default)]
    pub is_messaging_app: bool,

    /// Package name of the messaging app
    #[serde(rename = "packageName", skip_serializing_if = "Option::is_none")]
    pub package_name: Option<String>,

    /// URL for the web interface of the messaging app
    #[serde(rename = "webUrl", skip_serializing_if = "Option::is_none")]
    pub web_url: Option<String>,

    /// Unique identifier for the conversation
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,

    /// Whether this is a group chat
    #[serde(rename = "isGroupChat", default)]
    pub is_group_chat: bool,

    /// Name of the group chat
    #[serde(rename = "groupName", skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,

    /// Whether the notification supports a quick reply action
    #[serde(rename = "hasReplyAction", default)]
    pub has_reply_action: bool,

    /// Base64 encoded sender avatar image
    #[serde(rename = "senderAvatar", skip_serializing_if = "Option::is_none")]
    pub sender_avatar: Option<String>,

    /// Rich HTML formatted body text
    #[serde(rename = "richBody", skip_serializing_if = "Option::is_none")]
    pub rich_body: Option<String>,

    /// Base64 encoded notification image
    #[serde(rename = "imageData", skip_serializing_if = "Option::is_none")]
    pub image_data: Option<String>,

    /// Clickable links in the notification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<NotificationLink>>,

    /// Base64 encoded video thumbnail
    #[serde(rename = "videoThumbnail", skip_serializing_if = "Option::is_none")]
    pub video_thumbnail: Option<String>,

    /// Notification urgency level (0=low, 1=normal, 2=critical)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urgency: Option<u8>,

    /// Notification category (e.g., "email", "im", "device")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,

    /// Base64 encoded application icon
    #[serde(rename = "appIcon", skip_serializing_if = "Option::is_none")]
    pub app_icon: Option<String>,

    /// Structured action buttons with IDs and labels
    #[serde(rename = "actionButtons", skip_serializing_if = "Option::is_none")]
    pub action_buttons: Option<Vec<NotificationAction>>,
}

impl Notification {
    /// Create a new notification
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::Notification;
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
            is_messaging_app: false,
            package_name: None,
            web_url: None,
            conversation_id: None,
            is_group_chat: false,
            group_name: None,
            has_reply_action: false,
            sender_avatar: None,
            rich_body: None,
            image_data: None,
            links: None,
            video_thumbnail: None,
            urgency: None,
            category: None,
            app_icon: None,
            action_buttons: None,
        }
    }

    /// Check if notification is silent (preexisting)
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::Notification;
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
    /// use cosmic_connect_protocol::plugins::notification::Notification;
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
    /// use cosmic_connect_protocol::plugins::notification::Notification;
    ///
    /// let mut notif = Notification::new("1", "App", "Title", "Text", true);
    /// notif.actions = Some(vec!["Reply".to_string(), "Mark Read".to_string()]);
    /// assert!(notif.has_actions());
    /// ```
    pub fn has_actions(&self) -> bool {
        self.actions.as_ref().is_some_and(|a| !a.is_empty())
    }

    /// Check if notification has rich HTML content
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::Notification;
    ///
    /// let mut notif = Notification::new("1", "App", "Title", "Text", true);
    /// notif.rich_body = Some("<b>Bold</b> text".to_string());
    /// assert!(notif.has_rich_content());
    /// ```
    pub fn has_rich_content(&self) -> bool {
        self.rich_body.is_some()
    }

    /// Check if notification has an image
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::Notification;
    ///
    /// let mut notif = Notification::new("1", "App", "Title", "Text", true);
    /// notif.image_data = Some("base64encodeddata".to_string());
    /// assert!(notif.has_image());
    /// ```
    pub fn has_image(&self) -> bool {
        // Issue #180: Check all image sources including app_icon as fallback
        self.image_data.is_some() || self.sender_avatar.is_some() || self.app_icon.is_some()
    }

    /// Check if notification has clickable links
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::{Notification, NotificationLink};
    ///
    /// let mut notif = Notification::new("1", "App", "Title", "Text", true);
    /// notif.links = Some(vec![NotificationLink::new("https://example.com", None::<String>, 0, 10)]);
    /// assert!(notif.has_links());
    /// ```
    pub fn has_links(&self) -> bool {
        self.links.as_ref().is_some_and(|l| !l.is_empty())
    }

    /// Get decoded image data
    ///
    /// Decodes base64 image data if present.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::Notification;
    /// use base64::{engine::general_purpose, Engine as _};
    ///
    /// let mut notif = Notification::new("1", "App", "Title", "Text", true);
    /// notif.image_data = Some(general_purpose::STANDARD.encode(b"fake image data"));
    /// assert!(notif.get_image_bytes().is_some());
    /// ```
    pub fn get_image_bytes(&self) -> Option<Vec<u8>> {
        use base64::{engine::general_purpose, Engine as _};

        // Issue #180: Priority order for images:
        // 1. image_data - Main notification image/large icon
        // 2. sender_avatar - For messaging notifications
        // 3. app_icon - Fallback to app icon
        self.image_data
            .as_ref()
            .and_then(|data| general_purpose::STANDARD.decode(data).ok())
            .or_else(|| {
                self.sender_avatar
                    .as_ref()
                    .and_then(|data| general_purpose::STANDARD.decode(data).ok())
            })
            .or_else(|| {
                self.app_icon
                    .as_ref()
                    .and_then(|data| general_purpose::STANDARD.decode(data).ok())
            })
    }

    /// Get decoded video thumbnail bytes
    ///
    /// Decodes base64 video thumbnail data if present.
    pub fn get_video_thumbnail_bytes(&self) -> Option<Vec<u8>> {
        use base64::{engine::general_purpose, Engine as _};

        self.video_thumbnail
            .as_ref()
            .and_then(|data| general_purpose::STANDARD.decode(data).ok())
    }

    /// Get notification urgency level
    ///
    /// Returns the urgency level or Normal if not specified.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::{Notification, NotificationUrgency};
    ///
    /// let mut notif = Notification::new("1", "App", "Title", "Text", true);
    /// assert_eq!(notif.get_urgency(), NotificationUrgency::Normal);
    ///
    /// notif.urgency = Some(2);
    /// assert_eq!(notif.get_urgency(), NotificationUrgency::Critical);
    /// ```
    pub fn get_urgency(&self) -> NotificationUrgency {
        self.urgency
            .map(NotificationUrgency::from_byte)
            .unwrap_or_default()
    }

    /// Check if notification has an app icon
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::Notification;
    ///
    /// let mut notif = Notification::new("1", "App", "Title", "Text", true);
    /// assert!(!notif.has_app_icon());
    ///
    /// notif.app_icon = Some("base64icondata".to_string());
    /// assert!(notif.has_app_icon());
    /// ```
    pub fn has_app_icon(&self) -> bool {
        self.app_icon.is_some()
    }

    /// Get decoded app icon bytes
    ///
    /// Decodes base64 app icon data if present.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::Notification;
    /// use base64::{engine::general_purpose, Engine as _};
    ///
    /// let mut notif = Notification::new("1", "App", "Title", "Text", true);
    /// notif.app_icon = Some(general_purpose::STANDARD.encode(b"icon data"));
    /// assert!(notif.get_app_icon_bytes().is_some());
    /// ```
    pub fn get_app_icon_bytes(&self) -> Option<Vec<u8>> {
        use base64::{engine::general_purpose, Engine as _};

        self.app_icon
            .as_ref()
            .and_then(|data| general_purpose::STANDARD.decode(data).ok())
    }

    /// Check if notification has structured action buttons
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::{Notification, NotificationAction};
    ///
    /// let mut notif = Notification::new("1", "App", "Title", "Text", true);
    /// assert!(!notif.has_action_buttons());
    ///
    /// notif.action_buttons = Some(vec![NotificationAction::new("reply", "Reply")]);
    /// assert!(notif.has_action_buttons());
    /// ```
    pub fn has_action_buttons(&self) -> bool {
        self.action_buttons.as_ref().is_some_and(|a| !a.is_empty())
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
/// use cosmic_connect_protocol::plugins::notification::NotificationPlugin;
/// use cosmic_connect_protocol::Plugin;
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
    /// use cosmic_connect_protocol::plugins::notification::NotificationPlugin;
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
    /// use cosmic_connect_protocol::plugins::notification::NotificationPlugin;
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
    /// use cosmic_connect_protocol::plugins::notification::NotificationPlugin;
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
    /// use cosmic_connect_protocol::plugins::notification::NotificationPlugin;
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
    /// use cosmic_connect_protocol::plugins::notification::{NotificationPlugin, Notification};
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
    /// use cosmic_connect_protocol::plugins::notification::NotificationPlugin;
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
    /// use cosmic_connect_protocol::plugins::notification::NotificationPlugin;
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
    /// use cosmic_connect_protocol::plugins::notification::NotificationPlugin;
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

    /// Create an action invocation packet (Android → Desktop)
    ///
    /// This packet is sent when a user taps an action button in a notification
    /// on the remote device (e.g., Android phone). The desktop receives this
    /// packet and triggers the corresponding action in the desktop notification.
    ///
    /// # Arguments
    ///
    /// * `notification_id` - ID of the notification containing the action
    /// * `action_id` - ID of the action button that was tapped
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::NotificationPlugin;
    ///
    /// let plugin = NotificationPlugin::new();
    /// let packet = plugin.create_action_invocation_packet("desktop-App-123", "reply");
    ///
    /// assert_eq!(packet.packet_type, "cconnect.notification.action");
    /// assert_eq!(packet.body["key"], "desktop-App-123");
    /// assert_eq!(packet.body["action"], "reply");
    /// ```
    pub fn create_action_invocation_packet(
        &self,
        notification_id: &str,
        action_id: &str,
    ) -> Packet {
        let body = json!({
            "key": notification_id,
            "action": action_id
        });
        Packet::new("cconnect.notification.action", body)
    }

    /// Create a notification dismissal packet (Android → Desktop)
    ///
    /// This packet is sent when a notification is dismissed on the remote device
    /// (e.g., swiped away on Android). The desktop receives this and removes the
    /// corresponding mirrored notification.
    ///
    /// # Arguments
    ///
    /// * `notification_id` - ID of the notification that was dismissed
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::NotificationPlugin;
    ///
    /// let plugin = NotificationPlugin::new();
    /// let packet = plugin.create_notification_dismissal_packet("desktop-App-123");
    ///
    /// assert_eq!(packet.packet_type, "cconnect.notification");
    /// assert_eq!(packet.body["id"], "desktop-App-123");
    /// assert_eq!(packet.body["isCancel"], true);
    /// ```
    pub fn create_notification_dismissal_packet(&self, notification_id: &str) -> Packet {
        let body = json!({
            "id": notification_id,
            "isCancel": true
        });
        Packet::new("cconnect.notification", body)
    }

    /// Create a notification packet from a captured desktop notification
    ///
    /// Creates a notification packet suitable for sending desktop notifications to
    /// connected devices. This is used when capturing notifications from the local
    /// desktop notification system and forwarding them to remote devices.
    ///
    /// # Arguments
    ///
    /// * `app_name` - Name of the application that generated the notification
    /// * `summary` - Notification title/summary text
    /// * `body` - Notification body text (will be truncated if >2000 chars)
    /// * `timestamp` - UNIX timestamp in milliseconds
    /// * `image_data` - Optional notification image as raw bytes (will be base64 encoded)
    /// * `actions` - Optional list of action (key, label) pairs
    /// * `urgency` - Optional urgency level (None defaults to Normal)
    /// * `category` - Optional category string (e.g., "email", "im", "device")
    /// * `app_icon` - Optional application icon as raw bytes (will be base64 encoded)
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::notification::{NotificationPlugin, NotificationUrgency};
    ///
    /// let plugin = NotificationPlugin::new();
    /// let packet = NotificationPlugin::create_desktop_notification_packet(
    ///     "Thunderbird",
    ///     "New Email",
    ///     "You have 3 new messages",
    ///     1704067200000,
    ///     None,
    ///     &[("reply".to_string(), "Reply".to_string())],
    ///     Some(NotificationUrgency::Normal),
    ///     Some("email"),
    ///     None
    /// );
    ///
    /// assert_eq!(packet.packet_type, "cconnect.notification");
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn create_desktop_notification_packet(
        app_name: &str,
        summary: &str,
        body: &str,
        timestamp: i64,
        image_data: Option<&[u8]>,
        actions: &[(String, String)],
        urgency: Option<NotificationUrgency>,
        category: Option<&str>,
        app_icon: Option<&[u8]>,
    ) -> Packet {
        use base64::{engine::general_purpose, Engine as _};

        // Generate unique ID from app name and timestamp
        let id = format!("desktop-{}-{}", app_name, timestamp);

        // Truncate body if too long
        const MAX_BODY_LENGTH: usize = 2000;
        let truncated_body = if body.len() > MAX_BODY_LENGTH {
            format!("{}...", &body[..MAX_BODY_LENGTH])
        } else {
            body.to_string()
        };

        // Create ticker (combined title and text)
        let ticker = format!("{}: {} - {}", app_name, summary, truncated_body);

        // Encode image data as base64 if provided
        let encoded_image = image_data.map(|data| general_purpose::STANDARD.encode(data));

        // Encode app icon as base64 if provided
        let encoded_app_icon = app_icon.map(|data| general_purpose::STANDARD.encode(data));

        // Convert actions to structured format with both IDs and labels
        let action_buttons = if !actions.is_empty() {
            Some(
                actions
                    .iter()
                    .map(|(id, label)| NotificationAction::new(id, label))
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        };

        // Extract action labels for backward compatibility
        let action_labels = if !actions.is_empty() {
            Some(
                actions
                    .iter()
                    .map(|(_, label)| label.clone())
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        };

        // Build notification JSON body
        let mut notification_body = json!({
            "id": id,
            "appName": app_name,
            "title": summary,
            "text": truncated_body,
            "ticker": ticker,
            "isClearable": true,
            "time": timestamp.to_string(),
            "silent": "false"
        });

        // Add optional fields
        if let Some(image) = encoded_image {
            notification_body["imageData"] = json!(image);
        }

        if let Some(icon) = encoded_app_icon {
            notification_body["appIcon"] = json!(icon);
        }

        if let Some(actions) = action_labels {
            notification_body["actions"] = json!(actions);
        }

        if let Some(buttons) = action_buttons {
            notification_body["actionButtons"] = json!(buttons);
        }

        if let Some(urgency_level) = urgency {
            notification_body["urgency"] = json!(urgency_level.to_byte());
        }

        if let Some(cat) = category {
            notification_body["category"] = json!(cat);
        }

        Packet::new("cconnect.notification", notification_body)
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
            "kdeconnect.notification".to_string(),
            "kdeconnect.notification.request".to_string(),
            "kdeconnect.notification.action".to_string(),
            "kdeconnect.notification.reply".to_string(),
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

    async fn init(
        &mut self,
        device: &Device,
        _packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>,
    ) -> Result<()> {
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
        if packet.is_type("cconnect.notification") || packet.is_type("kdeconnect.notification") {
            self.handle_notification(packet, device);
        } else if packet.is_type("cconnect.notification.request")
            || packet.is_type("kdeconnect.notification.request")
        {
            self.handle_request(packet, device);
        } else if packet.is_type("cconnect.notification.action")
            || packet.is_type("kdeconnect.notification.action")
        {
            self.handle_action(packet, device);
        } else if packet.is_type("cconnect.notification.reply")
            || packet.is_type("kdeconnect.notification.reply")
        {
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
            "kdeconnect.notification".to_string(),
            "kdeconnect.notification.request".to_string(),
            "kdeconnect.notification.action".to_string(),
            "kdeconnect.notification.reply".to_string(),
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
        assert_eq!(incoming.len(), 8);
        assert!(incoming.contains(&"cconnect.notification".to_string()));
        assert!(incoming.contains(&"cconnect.notification.request".to_string()));
        assert!(incoming.contains(&"cconnect.notification.action".to_string()));
        assert!(incoming.contains(&"cconnect.notification.reply".to_string()));
        assert!(incoming.contains(&"kdeconnect.notification".to_string()));
        assert!(incoming.contains(&"kdeconnect.notification.request".to_string()));
        assert!(incoming.contains(&"kdeconnect.notification.action".to_string()));
        assert!(incoming.contains(&"kdeconnect.notification.reply".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 4);
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = NotificationPlugin::new();
        let device = create_test_device();

        plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .unwrap();
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
        plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .unwrap();

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
        plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .unwrap();

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
        plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .unwrap();

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
        plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .unwrap();

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

    #[test]
    fn test_messaging_notification_serialization() {
        let mut notif = Notification::new("123", "App", "Title", "Text", true);
        notif.is_messaging_app = true;
        notif.package_name = Some("com.whatsapp".to_string());
        notif.web_url = Some("https://web.whatsapp.com".to_string());
        notif.conversation_id = Some("conv_123".to_string());
        notif.is_group_chat = true;
        notif.group_name = Some("Family".to_string());
        notif.has_reply_action = true;

        let json = serde_json::to_value(&notif).unwrap();

        assert_eq!(json["isMessagingApp"], true);
        assert_eq!(json["packageName"], "com.whatsapp");
        assert_eq!(json["webUrl"], "https://web.whatsapp.com");
        assert_eq!(json["conversationId"], "conv_123");
        assert_eq!(json["isGroupChat"], true);
        assert_eq!(json["groupName"], "Family");
        assert_eq!(json["hasReplyAction"], true);

        // Round trip
        let deserialized: Notification = serde_json::from_value(json).unwrap();
        assert_eq!(notif, deserialized);
    }

    #[test]
    fn test_notification_link_creation() {
        let link = NotificationLink::new("https://example.com", Some("Example"), 0, 7);

        assert_eq!(link.url, "https://example.com");
        assert_eq!(link.title, Some("Example".to_string()));
        assert_eq!(link.start, 0);
        assert_eq!(link.length, 7);
    }

    #[test]
    fn test_notification_link_serialization() {
        let link = NotificationLink::new("https://example.com", Some("Example"), 10, 5);
        let json = serde_json::to_value(&link).unwrap();

        assert_eq!(json["url"], "https://example.com");
        assert_eq!(json["title"], "Example");
        assert_eq!(json["start"], 10);
        assert_eq!(json["length"], 5);

        // Round trip
        let deserialized: NotificationLink = serde_json::from_value(json).unwrap();
        assert_eq!(link, deserialized);
    }

    #[test]
    fn test_rich_notification_creation() {
        use base64::{engine::general_purpose, Engine as _};

        let mut notif = Notification::new("123", "App", "Title", "Text", true);
        notif.rich_body = Some("<b>Bold</b> and <i>italic</i> text".to_string());
        notif.image_data = Some(general_purpose::STANDARD.encode(b"fake image data"));
        notif.links = Some(vec![NotificationLink::new(
            "https://example.com",
            Some("Link"),
            0,
            4,
        )]);

        assert!(notif.has_rich_content());
        assert!(notif.has_image());
        assert!(notif.has_links());
    }

    #[test]
    fn test_rich_notification_serialization() {
        use base64::{engine::general_purpose, Engine as _};

        let mut notif = Notification::new("123", "App", "Title", "Text", true);
        notif.rich_body = Some("<b>Bold</b> text".to_string());
        notif.image_data = Some(general_purpose::STANDARD.encode(b"fake image"));
        notif.links = Some(vec![NotificationLink::new(
            "https://example.com",
            None::<String>,
            0,
            10,
        )]);
        notif.video_thumbnail = Some(general_purpose::STANDARD.encode(b"fake thumbnail"));

        let json = serde_json::to_value(&notif).unwrap();

        assert_eq!(json["richBody"], "<b>Bold</b> text");
        assert!(json["imageData"].is_string());
        assert!(json["links"].is_array());
        assert!(json["videoThumbnail"].is_string());

        // Round trip
        let deserialized: Notification = serde_json::from_value(json).unwrap();
        assert_eq!(notif, deserialized);
    }

    #[test]
    fn test_notification_image_decoding() {
        use base64::{engine::general_purpose, Engine as _};

        let mut notif = Notification::new("123", "App", "Title", "Text", true);
        let image_data = b"fake image data";
        notif.image_data = Some(general_purpose::STANDARD.encode(image_data));

        let decoded = notif.get_image_bytes().unwrap();
        assert_eq!(decoded, image_data);
    }

    #[test]
    fn test_notification_sender_avatar_fallback() {
        use base64::{engine::general_purpose, Engine as _};

        let mut notif = Notification::new("123", "App", "Title", "Text", true);
        let avatar_data = b"avatar data";
        notif.sender_avatar = Some(general_purpose::STANDARD.encode(avatar_data));

        // Should fall back to sender_avatar if image_data is not present
        let decoded = notif.get_image_bytes().unwrap();
        assert_eq!(decoded, avatar_data);
    }

    #[test]
    fn test_notification_video_thumbnail_decoding() {
        use base64::{engine::general_purpose, Engine as _};

        let mut notif = Notification::new("123", "App", "Title", "Text", true);
        let thumbnail_data = b"thumbnail data";
        notif.video_thumbnail = Some(general_purpose::STANDARD.encode(thumbnail_data));

        let decoded = notif.get_video_thumbnail_bytes().unwrap();
        assert_eq!(decoded, thumbnail_data);
    }

    #[test]
    fn test_notification_has_rich_content() {
        let mut notif = Notification::new("1", "App", "Title", "Text", true);
        assert!(!notif.has_rich_content());

        notif.rich_body = Some("<b>Rich</b>".to_string());
        assert!(notif.has_rich_content());
    }

    #[test]
    fn test_notification_has_image() {
        let mut notif = Notification::new("1", "App", "Title", "Text", true);
        assert!(!notif.has_image());

        notif.image_data = Some("base64data".to_string());
        assert!(notif.has_image());

        notif.image_data = None;
        notif.sender_avatar = Some("avatar".to_string());
        assert!(notif.has_image());
    }

    #[test]
    fn test_notification_has_links() {
        let mut notif = Notification::new("1", "App", "Title", "Text", true);
        assert!(!notif.has_links());

        notif.links = Some(vec![]);
        assert!(!notif.has_links());

        notif.links = Some(vec![NotificationLink::new(
            "https://example.com",
            None::<String>,
            0,
            10,
        )]);
        assert!(notif.has_links());
    }

    #[tokio::test]
    async fn test_handle_rich_notification() {
        use base64::{engine::general_purpose, Engine as _};

        let mut plugin = NotificationPlugin::new();
        let device = create_test_device();
        plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .unwrap();

        let mut notif =
            Notification::new("123", "WhatsApp", "New Message", "Check this out!", true);
        notif.rich_body = Some(
            "<b>Important:</b> Check <a href=\"https://example.com\">this link</a>".to_string(),
        );
        notif.image_data = Some(general_purpose::STANDARD.encode(b"image data"));
        notif.links = Some(vec![NotificationLink::new(
            "https://example.com",
            Some("Link"),
            20,
            4,
        )]);

        let packet = plugin.create_notification_packet(&notif);
        let mut device = create_test_device();
        plugin.handle_packet(&packet, &mut device).await.unwrap();

        let stored = plugin.get_notification("123").unwrap();
        assert_eq!(
            stored.rich_body,
            Some(
                "<b>Important:</b> Check <a href=\"https://example.com\">this link</a>".to_string()
            )
        );
        assert!(stored.has_rich_content());
        assert!(stored.has_image());
        assert!(stored.has_links());
    }

    #[test]
    fn test_create_desktop_notification_packet_basic() {
        let packet = NotificationPlugin::create_desktop_notification_packet(
            "Thunderbird",
            "New Email",
            "You have a new message",
            1704067200000,
            None,
            &[],
            None,
            None,
            None,
        );

        assert_eq!(packet.packet_type, "cconnect.notification");
        assert_eq!(packet.body["appName"], "Thunderbird");
        assert_eq!(packet.body["title"], "New Email");
        assert_eq!(packet.body["text"], "You have a new message");
        assert_eq!(packet.body["isClearable"], true);
        assert_eq!(packet.body["silent"], "false");
        assert_eq!(packet.body["time"], "1704067200000");

        // Check ID format
        let id = packet.body["id"].as_str().unwrap();
        assert!(id.starts_with("desktop-Thunderbird-"));

        // Check ticker format
        let ticker = packet.body["ticker"].as_str().unwrap();
        assert!(ticker.contains("Thunderbird"));
        assert!(ticker.contains("New Email"));
        assert!(ticker.contains("You have a new message"));
    }

    #[test]
    fn test_create_desktop_notification_packet_truncation() {
        let long_body = "a".repeat(2500);
        let packet = NotificationPlugin::create_desktop_notification_packet(
            "App",
            "Title",
            &long_body,
            1704067200000,
            None,
            &[],
            None,
            None,
            None,
        );

        let text = packet.body["text"].as_str().unwrap();
        assert_eq!(text.len(), 2003); // 2000 + "..."
        assert!(text.ends_with("..."));
    }

    #[test]
    fn test_create_desktop_notification_packet_with_image() {
        use base64::{engine::general_purpose, Engine as _};

        let image_data = b"fake image data";
        let packet = NotificationPlugin::create_desktop_notification_packet(
            "App",
            "Title",
            "Body",
            1704067200000,
            Some(image_data),
            &[],
            None,
            None,
            None,
        );

        assert!(packet.body["imageData"].is_string());
        let encoded = packet.body["imageData"].as_str().unwrap();
        let decoded = general_purpose::STANDARD.decode(encoded).unwrap();
        assert_eq!(decoded, image_data);
    }

    #[test]
    fn test_create_desktop_notification_packet_with_actions() {
        let actions = vec![
            ("reply".to_string(), "Reply".to_string()),
            ("mark_read".to_string(), "Mark as Read".to_string()),
        ];

        let packet = NotificationPlugin::create_desktop_notification_packet(
            "App",
            "Title",
            "Body",
            1704067200000,
            None,
            &actions,
            None,
            None,
            None,
        );

        assert!(packet.body["actions"].is_array());
        let action_labels = packet.body["actions"].as_array().unwrap();
        assert_eq!(action_labels.len(), 2);
        assert_eq!(action_labels[0], "Reply");
        assert_eq!(action_labels[1], "Mark as Read");

        // Check structured action buttons
        assert!(packet.body["actionButtons"].is_array());
        let action_buttons = packet.body["actionButtons"].as_array().unwrap();
        assert_eq!(action_buttons.len(), 2);
        assert_eq!(action_buttons[0]["id"], "reply");
        assert_eq!(action_buttons[0]["label"], "Reply");
        assert_eq!(action_buttons[1]["id"], "mark_read");
        assert_eq!(action_buttons[1]["label"], "Mark as Read");
    }

    #[test]
    fn test_create_desktop_notification_packet_complete() {
        use base64::{engine::general_purpose, Engine as _};

        let image_data = b"image bytes";
        let app_icon_data = b"icon bytes";
        let actions = vec![("reply".to_string(), "Reply".to_string())];

        let packet = NotificationPlugin::create_desktop_notification_packet(
            "Signal",
            "New Message",
            "Hello from Signal",
            1704067200000,
            Some(image_data),
            &actions,
            Some(NotificationUrgency::Normal),
            Some("im"),
            Some(app_icon_data),
        );

        assert_eq!(packet.packet_type, "cconnect.notification");
        assert_eq!(packet.body["appName"], "Signal");
        assert_eq!(packet.body["title"], "New Message");
        assert_eq!(packet.body["text"], "Hello from Signal");
        assert!(packet.body["imageData"].is_string());
        assert!(packet.body["appIcon"].is_string());
        assert!(packet.body["actions"].is_array());
        assert!(packet.body["actionButtons"].is_array());
        assert_eq!(packet.body["urgency"], 1);
        assert_eq!(packet.body["category"], "im");

        // Verify image can be decoded
        let encoded = packet.body["imageData"].as_str().unwrap();
        let decoded = general_purpose::STANDARD.decode(encoded).unwrap();
        assert_eq!(decoded, image_data);

        // Verify app icon can be decoded
        let encoded_icon = packet.body["appIcon"].as_str().unwrap();
        let decoded_icon = general_purpose::STANDARD.decode(encoded_icon).unwrap();
        assert_eq!(decoded_icon, app_icon_data);
    }

    #[test]
    fn test_notification_urgency() {
        assert_eq!(NotificationUrgency::Low.to_byte(), 0);
        assert_eq!(NotificationUrgency::Normal.to_byte(), 1);
        assert_eq!(NotificationUrgency::Critical.to_byte(), 2);

        assert_eq!(NotificationUrgency::from_byte(0), NotificationUrgency::Low);
        assert_eq!(
            NotificationUrgency::from_byte(1),
            NotificationUrgency::Normal
        );
        assert_eq!(
            NotificationUrgency::from_byte(2),
            NotificationUrgency::Critical
        );
        assert_eq!(
            NotificationUrgency::from_byte(99),
            NotificationUrgency::Normal
        );
    }

    #[test]
    fn test_notification_action_creation() {
        let action = NotificationAction::new("reply", "Reply");
        assert_eq!(action.id, "reply");
        assert_eq!(action.label, "Reply");
    }

    #[test]
    fn test_notification_action_serialization() {
        let action = NotificationAction::new("mark_read", "Mark as Read");
        let json = serde_json::to_value(&action).unwrap();

        assert_eq!(json["id"], "mark_read");
        assert_eq!(json["label"], "Mark as Read");

        let deserialized: NotificationAction = serde_json::from_value(json).unwrap();
        assert_eq!(action, deserialized);
    }

    #[test]
    fn test_notification_get_urgency() {
        let mut notif = Notification::new("1", "App", "Title", "Text", true);
        assert_eq!(notif.get_urgency(), NotificationUrgency::Normal);

        notif.urgency = Some(0);
        assert_eq!(notif.get_urgency(), NotificationUrgency::Low);

        notif.urgency = Some(2);
        assert_eq!(notif.get_urgency(), NotificationUrgency::Critical);
    }

    #[test]
    fn test_notification_has_app_icon() {
        let mut notif = Notification::new("1", "App", "Title", "Text", true);
        assert!(!notif.has_app_icon());

        notif.app_icon = Some("base64icon".to_string());
        assert!(notif.has_app_icon());
    }

    #[test]
    fn test_notification_get_app_icon_bytes() {
        use base64::{engine::general_purpose, Engine as _};

        let mut notif = Notification::new("1", "App", "Title", "Text", true);
        let icon_data = b"icon bytes";
        notif.app_icon = Some(general_purpose::STANDARD.encode(icon_data));

        let decoded = notif.get_app_icon_bytes().unwrap();
        assert_eq!(decoded, icon_data);
    }

    // Issue #180: Test app_icon fallback in get_image_bytes()
    #[test]
    fn test_get_image_bytes_app_icon_fallback() {
        use base64::{engine::general_purpose, Engine as _};

        let mut notif = Notification::new("1", "App", "Title", "Text", true);

        // No images - should return None
        assert!(notif.get_image_bytes().is_none());
        assert!(!notif.has_image());

        // Only app_icon - should use it as fallback
        let icon_data = b"app icon data";
        notif.app_icon = Some(general_purpose::STANDARD.encode(icon_data));
        assert!(notif.has_image());
        assert_eq!(notif.get_image_bytes().unwrap(), icon_data);

        // Add sender_avatar - should take priority over app_icon
        let avatar_data = b"sender avatar data";
        notif.sender_avatar = Some(general_purpose::STANDARD.encode(avatar_data));
        assert_eq!(notif.get_image_bytes().unwrap(), avatar_data);

        // Add image_data - should take priority over all others
        let image_data = b"main image data";
        notif.image_data = Some(general_purpose::STANDARD.encode(image_data));
        assert_eq!(notif.get_image_bytes().unwrap(), image_data);
    }

    #[test]
    fn test_notification_has_action_buttons() {
        let mut notif = Notification::new("1", "App", "Title", "Text", true);
        assert!(!notif.has_action_buttons());

        notif.action_buttons = Some(vec![]);
        assert!(!notif.has_action_buttons());

        notif.action_buttons = Some(vec![NotificationAction::new("reply", "Reply")]);
        assert!(notif.has_action_buttons());
    }

    #[test]
    fn test_notification_extended_fields_serialization() {
        use base64::{engine::general_purpose, Engine as _};

        let mut notif = Notification::new("123", "App", "Title", "Text", true);
        notif.urgency = Some(2);
        notif.category = Some("email".to_string());
        notif.app_icon = Some(general_purpose::STANDARD.encode(b"icon"));
        notif.action_buttons = Some(vec![
            NotificationAction::new("reply", "Reply"),
            NotificationAction::new("delete", "Delete"),
        ]);

        let json = serde_json::to_value(&notif).unwrap();

        assert_eq!(json["urgency"], 2);
        assert_eq!(json["category"], "email");
        assert!(json["appIcon"].is_string());
        assert!(json["actionButtons"].is_array());
        assert_eq!(json["actionButtons"].as_array().unwrap().len(), 2);

        let deserialized: Notification = serde_json::from_value(json).unwrap();
        assert_eq!(notif, deserialized);
    }

    #[test]
    fn test_create_action_invocation_packet() {
        let plugin = NotificationPlugin::new();
        let packet = plugin.create_action_invocation_packet("desktop-App-123", "reply");

        assert_eq!(packet.packet_type, "cconnect.notification.action");
        assert_eq!(packet.body["key"], "desktop-App-123");
        assert_eq!(packet.body["action"], "reply");
    }

    #[test]
    fn test_create_notification_dismissal_packet() {
        let plugin = NotificationPlugin::new();
        let packet = plugin.create_notification_dismissal_packet("desktop-App-123");

        assert_eq!(packet.packet_type, "cconnect.notification");
        assert_eq!(packet.body["id"], "desktop-App-123");
        assert_eq!(packet.body["isCancel"], true);
    }

    #[test]
    fn test_desktop_notification_with_urgency_levels() {
        // Test low urgency
        let packet_low = NotificationPlugin::create_desktop_notification_packet(
            "App",
            "Title",
            "Body",
            1704067200000,
            None,
            &[],
            Some(NotificationUrgency::Low),
            None,
            None,
        );
        assert_eq!(packet_low.body["urgency"], 0);

        // Test normal urgency
        let packet_normal = NotificationPlugin::create_desktop_notification_packet(
            "App",
            "Title",
            "Body",
            1704067200000,
            None,
            &[],
            Some(NotificationUrgency::Normal),
            None,
            None,
        );
        assert_eq!(packet_normal.body["urgency"], 1);

        // Test critical urgency
        let packet_critical = NotificationPlugin::create_desktop_notification_packet(
            "App",
            "Title",
            "Body",
            1704067200000,
            None,
            &[],
            Some(NotificationUrgency::Critical),
            None,
            None,
        );
        assert_eq!(packet_critical.body["urgency"], 2);
    }

    #[test]
    fn test_desktop_notification_with_category() {
        let packet = NotificationPlugin::create_desktop_notification_packet(
            "Thunderbird",
            "New Email",
            "You have mail",
            1704067200000,
            None,
            &[],
            None,
            Some("email"),
            None,
        );

        assert_eq!(packet.body["category"], "email");
    }

    #[test]
    fn test_desktop_notification_backward_compatibility() {
        // Packet without new fields should still work
        let packet = NotificationPlugin::create_desktop_notification_packet(
            "App",
            "Title",
            "Body",
            1704067200000,
            None,
            &[],
            None,
            None,
            None,
        );

        // Basic fields should be present
        assert!(packet.body["id"].is_string());
        assert!(packet.body["appName"].is_string());
        assert!(packet.body["title"].is_string());
        assert!(packet.body["text"].is_string());

        // Extended fields should not be present when not provided
        assert!(packet.body["urgency"].is_null());
        assert!(packet.body["category"].is_null());
        assert!(packet.body["appIcon"].is_null());
        assert!(packet.body["actionButtons"].is_null());
    }
}
