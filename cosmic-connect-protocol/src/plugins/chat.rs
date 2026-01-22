//! Chat Plugin
//!
//! Enables instant messaging between connected desktops.
//! Provides real-time text chat with message history, typing indicators, and read receipts.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - Incoming: `cconnect.chat.message`, `cconnect.chat.typing`, `cconnect.chat.read`, `cconnect.chat.history`
//! - Outgoing: `cconnect.chat.message`, `cconnect.chat.typing`, `cconnect.chat.history_response`
//!
//! **Capabilities**: `cconnect.chat`
//!
//! ## Send Message
//!
//! Send a text message:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.chat.message",
//!     "body": {
//!         "message_id": "msg-uuid",
//!         "text": "Hello from desktop!",
//!         "timestamp": 1640000000000
//!     }
//! }
//! ```
//!
//! ## Typing Indicator
//!
//! Notify that user is typing:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.chat.typing",
//!     "body": {
//!         "is_typing": true
//!     }
//! }
//! ```
//!
//! ## Read Receipt
//!
//! Acknowledge message was read:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.chat.read",
//!     "body": {
//!         "message_id": "msg-uuid"
//!     }
//! }
//! ```
//!
//! ## Request Message History
//!
//! Query chat history:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.chat.history",
//!     "body": {
//!         "limit": 50,
//!         "before_timestamp": 1640000000000
//!     }
//! }
//! ```
//!
//! ## Message History Response
//!
//! Return chat history:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.chat.history_response",
//!     "body": {
//!         "messages": [
//!             {
//!                 "message_id": "msg-uuid",
//!                 "text": "Previous message",
//!                 "timestamp": 1640000000000,
//!                 "from_me": false,
//!                 "read": true
//!             }
//!         ]
//!     }
//! }
//! ```
//!
//! ## Storage
//!
//! - Messages stored in-memory (TODO: SQLite)
//! - Configurable message retention
//! - Per-device chat rooms
//! - Database path: `~/.local/share/cosmic-connect/chat.db`
//!
//! ## Configuration
//!
//! ```toml
//! [plugins.chat]
//! max_messages = 1000      # Maximum messages to keep per device
//! retention_days = 90      # Days to keep messages
//! show_notifications = true # Show desktop notifications
//! ```
//!
//! ## Example
//!
//! ```rust,ignore
//! use cosmic_connect_core::plugins::chat::*;
//!
//! let mut plugin = ChatPlugin::new();
//!
//! // Send a message
//! let msg = plugin.send_message("Hello!".to_string()).await?;
//!
//! // Get message history
//! let history = plugin.get_history(50).await;
//! ```

use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::any::Any;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::{Plugin, PluginFactory};

/// Maximum messages to keep per device
const DEFAULT_MAX_MESSAGES: usize = 1000;

/// Default retention period in days
#[allow(dead_code)]
const DEFAULT_RETENTION_DAYS: i64 = 90;

/// Chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Unique message ID
    pub message_id: String,

    /// Message text content
    pub text: String,

    /// UNIX epoch timestamp in milliseconds
    pub timestamp: i64,

    /// Whether message was sent by me (vs received)
    pub from_me: bool,

    /// Whether message has been read
    pub read: bool,
}

impl ChatMessage {
    /// Create a new chat message
    pub fn new(text: String, from_me: bool) -> Self {
        Self {
            message_id: Uuid::new_v4().to_string(),
            text,
            timestamp: Utc::now().timestamp_millis(),
            from_me,
            read: false,
        }
    }

    /// Create message with explicit ID and timestamp
    pub fn with_id_and_timestamp(
        message_id: String,
        text: String,
        timestamp: i64,
        from_me: bool,
    ) -> Self {
        Self {
            message_id,
            text,
            timestamp,
            from_me,
            read: false,
        }
    }
}

/// Chat configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatConfig {
    /// Maximum messages to keep
    pub max_messages: usize,

    /// Retention period in days
    pub retention_days: i64,

    /// Show desktop notifications
    pub show_notifications: bool,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            max_messages: DEFAULT_MAX_MESSAGES,
            retention_days: DEFAULT_RETENTION_DAYS,
            show_notifications: true,
        }
    }
}

/// Chat message storage
#[derive(Debug, Clone)]
struct ChatStorage {
    /// Messages (newest first)
    messages: VecDeque<ChatMessage>,

    /// Configuration
    config: ChatConfig,
}

impl ChatStorage {
    /// Create new storage
    fn new(config: ChatConfig) -> Self {
        Self {
            messages: VecDeque::new(),
            config,
        }
    }

    /// Add message
    fn add(&mut self, message: ChatMessage) {
        // Add to front (newest first)
        self.messages.push_front(message);

        // Cleanup old messages
        self.cleanup();
    }

    /// Get message by ID
    #[allow(dead_code)]
    fn get(&self, message_id: &str) -> Option<&ChatMessage> {
        self.messages
            .iter()
            .find(|msg| msg.message_id == message_id)
    }

    /// Mark message as read
    fn mark_read(&mut self, message_id: &str) -> Result<()> {
        if let Some(msg) = self
            .messages
            .iter_mut()
            .find(|m| m.message_id == message_id)
        {
            msg.read = true;
            Ok(())
        } else {
            Err(ProtocolError::invalid_state(format!(
                "Message not found: {}",
                message_id
            )))
        }
    }

    /// Get recent messages
    fn get_history(&self, limit: usize, before_timestamp: Option<i64>) -> Vec<ChatMessage> {
        self.messages
            .iter()
            .filter(|msg| {
                if let Some(before) = before_timestamp {
                    msg.timestamp < before
                } else {
                    true
                }
            })
            .take(limit)
            .cloned()
            .collect()
    }

    /// Cleanup old messages
    fn cleanup(&mut self) {
        let cutoff_time =
            Utc::now().timestamp_millis() - (self.config.retention_days * 24 * 60 * 60 * 1000);

        // Remove old messages
        self.messages.retain(|msg| msg.timestamp > cutoff_time);

        // Limit total count
        if self.messages.len() > self.config.max_messages {
            self.messages.truncate(self.config.max_messages);
        }
    }

    /// Get unread count
    fn unread_count(&self) -> usize {
        self.messages.iter().filter(|msg| !msg.read).count()
    }
}

/// Chat plugin for instant messaging
pub struct ChatPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Whether the plugin is enabled
    enabled: bool,

    /// Message storage (TODO: SQLite)
    storage: Arc<RwLock<ChatStorage>>,

    /// Whether remote user is currently typing
    remote_typing: Arc<RwLock<bool>>,
}

impl ChatPlugin {
    /// Create a new chat plugin
    pub fn new() -> Self {
        Self::with_config(ChatConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: ChatConfig) -> Self {
        info!(
            "Creating Chat plugin with max_messages={}, retention_days={}",
            config.max_messages, config.retention_days
        );

        Self {
            device_id: None,
            enabled: false,
            storage: Arc::new(RwLock::new(ChatStorage::new(config))),
            remote_typing: Arc::new(RwLock::new(false)),
        }
    }

    /// Send a message
    pub async fn send_message(&mut self, text: String) -> Result<String> {
        let message = ChatMessage::new(text, true);
        let message_id = message.message_id.clone();

        self.storage.write().await.add(message);

        info!("Sent chat message: {}", message_id);
        Ok(message_id)
    }

    /// Get message history
    pub async fn get_history(&self, limit: usize) -> Vec<ChatMessage> {
        self.storage.read().await.get_history(limit, None)
    }

    /// Get history before timestamp
    pub async fn get_history_before(
        &self,
        limit: usize,
        before_timestamp: i64,
    ) -> Vec<ChatMessage> {
        self.storage
            .read()
            .await
            .get_history(limit, Some(before_timestamp))
    }

    /// Mark message as read
    pub async fn mark_read(&mut self, message_id: &str) -> Result<()> {
        self.storage.write().await.mark_read(message_id)?;
        info!("Marked message as read: {}", message_id);
        Ok(())
    }

    /// Get unread message count
    pub async fn unread_count(&self) -> usize {
        self.storage.read().await.unread_count()
    }

    /// Set typing indicator
    pub async fn set_typing(&mut self, is_typing: bool) {
        *self.remote_typing.write().await = is_typing;
        debug!("Remote typing: {}", is_typing);
    }

    /// Check if remote user is typing
    pub async fn is_remote_typing(&self) -> bool {
        *self.remote_typing.read().await
    }

    /// Create message packet
    pub fn create_message_packet(&self, message: &ChatMessage) -> Packet {
        Packet::new(
            "cconnect.chat.message",
            json!({
                "messageId": message.message_id,
                "text": message.text,
                "timestamp": message.timestamp
            }),
        )
    }

    /// Create typing indicator packet
    pub fn create_typing_packet(&self, is_typing: bool) -> Packet {
        Packet::new("cconnect.chat.typing", json!({ "isTyping": is_typing }))
    }

    /// Create read receipt packet
    pub fn create_read_packet(&self, message_id: &str) -> Packet {
        Packet::new("cconnect.chat.read", json!({ "messageId": message_id }))
    }

    /// Create history request packet
    pub fn create_history_request_packet(
        &self,
        limit: usize,
        before_timestamp: Option<i64>,
    ) -> Packet {
        Packet::new(
            "cconnect.chat.history",
            json!({
                "limit": limit,
                "beforeTimestamp": before_timestamp
            }),
        )
    }

    /// Create history response packet
    pub fn create_history_response_packet(&self, messages: Vec<ChatMessage>) -> Packet {
        let messages_json: Vec<serde_json::Value> = messages
            .into_iter()
            .map(|msg| {
                json!({
                    "messageId": msg.message_id,
                    "text": msg.text,
                    "timestamp": msg.timestamp,
                    "fromMe": msg.from_me,
                    "read": msg.read
                })
            })
            .collect();

        Packet::new(
            "cconnect.chat.history_response",
            json!({ "messages": messages_json }),
        )
    }

    /// Handle incoming message
    async fn handle_message(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        let message_id = packet
            .body
            .get("messageId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProtocolError::invalid_state("Missing messageId"))?;

        let text = packet
            .body
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProtocolError::invalid_state("Missing text"))?;

        let timestamp = packet
            .body
            .get("timestamp")
            .and_then(|v| v.as_i64())
            .unwrap_or_else(|| Utc::now().timestamp_millis());

        info!(
            "Received chat message from {} ({}): {} chars",
            device.name(),
            device.id(),
            text.len()
        );

        let message = ChatMessage::with_id_and_timestamp(
            message_id.to_string(),
            text.to_string(),
            timestamp,
            false, // from_me = false (received)
        );

        self.storage.write().await.add(message);

        // TODO: Send notification if enabled
        // Need notification plugin integration

        Ok(())
    }

    /// Handle typing indicator
    async fn handle_typing(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        let is_typing = packet
            .body
            .get("isTyping")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        debug!(
            "Received typing indicator from {} ({}): {}",
            device.name(),
            device.id(),
            is_typing
        );

        self.set_typing(is_typing).await;

        Ok(())
    }

    /// Handle read receipt
    async fn handle_read(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        let message_id = packet
            .body
            .get("messageId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProtocolError::invalid_state("Missing messageId"))?;

        info!(
            "Received read receipt from {} ({}): {}",
            device.name(),
            device.id(),
            message_id
        );

        self.mark_read(message_id).await?;

        Ok(())
    }

    /// Handle history request
    async fn handle_history_request(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        let limit = packet
            .body
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(50) as usize;

        let before_timestamp = packet.body.get("beforeTimestamp").and_then(|v| v.as_i64());

        info!(
            "Received history request from {} ({}): limit={}",
            device.name(),
            device.id(),
            limit
        );

        let messages = if let Some(before) = before_timestamp {
            self.get_history_before(limit, before).await
        } else {
            self.get_history(limit).await
        };

        info!("Found {} messages", messages.len());

        // TODO: Send history response packet
        // Need packet sending infrastructure

        Ok(())
    }
}

impl Default for ChatPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ChatPlugin {
    fn name(&self) -> &str {
        "chat"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.chat.message".to_string(),
            "cconnect.chat.typing".to_string(),
            "cconnect.chat.read".to_string(),
            "cconnect.chat.history".to_string(),
            "kdeconnect.chat.message".to_string(),
            "kdeconnect.chat.typing".to_string(),
            "kdeconnect.chat.read".to_string(),
            "kdeconnect.chat.history".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.chat.message".to_string(),
            "cconnect.chat.typing".to_string(),
            "cconnect.chat.history_response".to_string(),
        ]
    }

    async fn init(&mut self, device: &Device, _packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!("Chat plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Chat plugin started");
        self.enabled = true;

        // Cleanup on start
        self.storage.write().await.cleanup();

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Chat plugin stopped");
        self.enabled = false;
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("Chat plugin is disabled, ignoring packet");
            return Ok(());
        }

        if packet.is_type("cconnect.chat.message") {
            self.handle_message(packet, device).await
        } else if packet.is_type("cconnect.chat.typing") {
            self.handle_typing(packet, device).await
        } else if packet.is_type("cconnect.chat.read") {
            self.handle_read(packet, device).await
        } else if packet.is_type("cconnect.chat.history") {
            self.handle_history_request(packet, device).await
        } else {
            Ok(())
        }
    }
}

/// Factory for creating Chat plugin instances
pub struct ChatPluginFactory;

impl PluginFactory for ChatPluginFactory {
    fn create(&self) -> Box<dyn Plugin> {
        Box::new(ChatPlugin::new())
    }

    fn name(&self) -> &str {
        "chat"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.chat.message".to_string(),
            "cconnect.chat.typing".to_string(),
            "cconnect.chat.read".to_string(),
            "cconnect.chat.history".to_string(),
            "kdeconnect.chat.message".to_string(),
            "kdeconnect.chat.typing".to_string(),
            "kdeconnect.chat.read".to_string(),
            "kdeconnect.chat.history".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.chat.message".to_string(),
            "cconnect.chat.typing".to_string(),
            "cconnect.chat.history_response".to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceInfo, DeviceType};

    fn create_test_device() -> Device {
        let info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1716);
        Device::from_discovery(info)
    }

    #[test]
    fn test_plugin_creation() {
        let plugin = ChatPlugin::new();
        assert_eq!(plugin.name(), "chat");
        assert!(!plugin.enabled);
    }

    #[tokio::test]
    async fn test_send_message() {
        let mut plugin = ChatPlugin::new();
        let msg_id = plugin.send_message("Hello!".to_string()).await.unwrap();
        assert!(!msg_id.is_empty());

        let history = plugin.get_history(10).await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].text, "Hello!");
        assert!(history[0].from_me);
    }

    #[tokio::test]
    async fn test_message_history() {
        let mut plugin = ChatPlugin::new();

        plugin.send_message("Message 1".to_string()).await.unwrap();
        plugin.send_message("Message 2".to_string()).await.unwrap();
        plugin.send_message("Message 3".to_string()).await.unwrap();

        let history = plugin.get_history(10).await;
        assert_eq!(history.len(), 3);

        // Newest first
        assert_eq!(history[0].text, "Message 3");
        assert_eq!(history[1].text, "Message 2");
        assert_eq!(history[2].text, "Message 1");
    }

    #[tokio::test]
    async fn test_mark_read() {
        let mut plugin = ChatPlugin::new();
        let msg_id = plugin.send_message("Test".to_string()).await.unwrap();

        let history = plugin.get_history(1).await;
        assert!(!history[0].read);

        plugin.mark_read(&msg_id).await.unwrap();

        let history = plugin.get_history(1).await;
        assert!(history[0].read);
    }

    #[tokio::test]
    async fn test_unread_count() {
        let mut plugin = ChatPlugin::new();

        let msg1 = plugin.send_message("Message 1".to_string()).await.unwrap();
        plugin.send_message("Message 2".to_string()).await.unwrap();

        assert_eq!(plugin.unread_count().await, 2);

        plugin.mark_read(&msg1).await.unwrap();

        assert_eq!(plugin.unread_count().await, 1);
    }

    #[tokio::test]
    async fn test_typing_indicator() {
        let mut plugin = ChatPlugin::new();

        assert!(!plugin.is_remote_typing().await);

        plugin.set_typing(true).await;
        assert!(plugin.is_remote_typing().await);

        plugin.set_typing(false).await;
        assert!(!plugin.is_remote_typing().await);
    }

    #[tokio::test]
    async fn test_max_messages_limit() {
        let config = ChatConfig {
            max_messages: 3,
            retention_days: 90,
            show_notifications: true,
        };

        let mut plugin = ChatPlugin::with_config(config);

        plugin.send_message("Message 1".to_string()).await.unwrap();
        plugin.send_message("Message 2".to_string()).await.unwrap();
        plugin.send_message("Message 3".to_string()).await.unwrap();
        plugin.send_message("Message 4".to_string()).await.unwrap();

        let history = plugin.get_history(10).await;
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].text, "Message 4");
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = ChatPlugin::new();
        let device = create_test_device();

        assert!(plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.is_ok());
        assert!(plugin.start().await.is_ok());
        assert!(plugin.enabled);
        assert!(plugin.stop().await.is_ok());
        assert!(!plugin.enabled);
    }

    #[test]
    fn test_capabilities() {
        let plugin = ChatPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 8);
        assert!(incoming.contains(&"cconnect.chat.message".to_string()));
        assert!(incoming.contains(&"cconnect.chat.typing".to_string()));
        assert!(incoming.contains(&"kdeconnect.chat.message".to_string()));
        assert!(incoming.contains(&"kdeconnect.chat.typing".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 3);
        assert!(outgoing.contains(&"cconnect.chat.message".to_string()));
    }

    #[tokio::test]
    async fn test_handle_incoming_message() {
        let mut plugin = ChatPlugin::new();
        let device = create_test_device();
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();
        plugin.start().await.unwrap();

        let mut device = create_test_device();
        let now = Utc::now().timestamp_millis();
        let packet = Packet::new(
            "cconnect.chat.message",
            json!({
                "messageId": "test-id",
                "text": "Hello from remote",
                "timestamp": now
            }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        let history = plugin.get_history(10).await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].text, "Hello from remote");
        assert!(!history[0].from_me);
    }
}
