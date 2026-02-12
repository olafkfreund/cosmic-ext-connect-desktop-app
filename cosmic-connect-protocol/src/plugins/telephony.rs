//! Telephony and SMS Plugin
//!
//! This plugin handles phone call notifications and SMS messaging functionality,
//! allowing desktop computers to receive call notifications, mute ringers, and
//! send/receive text messages.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `cconnect.telephony` - Phone call events (incoming)
//! - `cconnect.telephony.request_mute` - Mute ringer request (outgoing)
//! - `cconnect.sms.messages` - SMS message data (incoming)
//! - `cconnect.sms.request_conversations` - Request conversation list (outgoing)
//! - `cconnect.sms.request_conversation` - Request thread messages (outgoing)
//! - `cconnect.sms.request_attachment` - Request message attachment (outgoing)
//! - `cconnect.sms.request` - Send SMS message (outgoing)
//!
//! **Capabilities**:
//! - Incoming: `cconnect.telephony`, `cconnect.sms.messages`
//! - Outgoing: `cconnect.telephony.request_mute`, `cconnect.sms.request*`
//!
//! ## Call Events
//!
//! Phone call events contain:
//! - `event`: One of "ringing", "talking", "missedCall", "sms" (deprecated)
//! - `phoneNumber`: Caller's phone number
//! - `contactName`: Contact name from phone's address book (optional)
//! - `messageBody`: SMS body (deprecated, use SMS plugin instead)
//!
//! ## SMS Messages
//!
//! SMS conversations contain threads with multiple messages, including:
//! - Thread ID
//! - Message timestamps
//! - Message bodies
//! - Read/unread status
//! - Sender information
//!
//! ## References
//!
//! - [CConnect Telephony Plugin](https://github.com/KDE/cconnect-kde/tree/master/plugins/telephony)
//! - [CConnect SMS Plugin](https://lxr.kde.org/source/network/cconnect-kde/plugins/sms/)
//! - [Valent Protocol Documentation](https://valent.andyholmes.ca/documentation/protocol.html)

use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::{Plugin, PluginFactory};

/// Packet type for telephony events
pub const PACKET_TYPE_TELEPHONY: &str = "cconnect.telephony";

/// Packet type for mute ringer request
pub const PACKET_TYPE_TELEPHONY_MUTE: &str = "cconnect.telephony.request_mute";

/// Packet type for SMS messages
pub const PACKET_TYPE_SMS_MESSAGES: &str = "cconnect.sms.messages";

/// Packet type for requesting conversation list
pub const PACKET_TYPE_SMS_REQUEST_CONVERSATIONS: &str = "cconnect.sms.request_conversations";

/// Packet type for requesting conversation messages
pub const PACKET_TYPE_SMS_REQUEST_CONVERSATION: &str = "cconnect.sms.request_conversation";

/// Packet type for requesting message attachment
pub const PACKET_TYPE_SMS_REQUEST_ATTACHMENT: &str = "cconnect.sms.request_attachment";

/// Packet type for sending SMS
pub const PACKET_TYPE_SMS_REQUEST: &str = "cconnect.sms.request";

/// Phone call event types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CallEvent {
    /// Phone is ringing (incoming call)
    Ringing,
    /// Call is active (in conversation)
    Talking,
    /// Call was missed
    MissedCall,
    /// SMS received (deprecated, use SMS plugin)
    Sms,
}

impl CallEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ringing => "ringing",
            Self::Talking => "talking",
            Self::MissedCall => "missedCall",
            Self::Sms => "sms",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ringing" => Some(Self::Ringing),
            "talking" => Some(Self::Talking),
            "missedCall" => Some(Self::MissedCall),
            "sms" => Some(Self::Sms),
            _ => None,
        }
    }
}

/// Telephony event notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelephonyEvent {
    /// Event type
    pub event: String,

    /// Phone number
    #[serde(skip_serializing_if = "Option::is_none", rename = "phoneNumber")]
    pub phone_number: Option<String>,

    /// Contact name from address book
    #[serde(skip_serializing_if = "Option::is_none", rename = "contactName")]
    pub contact_name: Option<String>,

    /// SMS message body (deprecated)
    #[serde(skip_serializing_if = "Option::is_none", rename = "messageBody")]
    pub message_body: Option<String>,
}

/// SMS message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmsMessage {
    /// Message ID
    #[serde(rename = "_id")]
    pub id: i64,

    /// Thread ID
    #[serde(rename = "threadId")]
    pub thread_id: i64,

    /// Phone number/address
    pub address: String,

    /// Message body
    pub body: String,

    /// Timestamp (milliseconds since epoch)
    pub date: i64,

    /// Message type (1 = received, 2 = sent)
    #[serde(rename = "type")]
    pub message_type: i32,

    /// Read status (0 = unread, 1 = read)
    pub read: i32,
}

/// SMS conversation thread
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmsConversation {
    /// Thread ID
    #[serde(rename = "threadId")]
    pub thread_id: i64,

    /// Messages in this conversation
    pub messages: Vec<SmsMessage>,
}

/// SMS messages packet body
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmsMessages {
    /// List of conversations
    pub conversations: Vec<SmsConversation>,
}

/// Request for conversation messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRequest {
    /// Thread ID
    #[serde(rename = "threadID")]
    pub thread_id: i64,

    /// Earliest message timestamp (milliseconds since epoch)
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "rangeStartTimestamp"
    )]
    pub range_start_timestamp: Option<i64>,

    /// Maximum number of messages to return
    #[serde(skip_serializing_if = "Option::is_none", rename = "numberToRequest")]
    pub number_to_request: Option<i32>,
}

/// Request for message attachment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentRequest {
    /// Attachment part ID
    #[serde(rename = "part_id")]
    pub part_id: i64,

    /// Unique file identifier
    pub unique_identifier: String,
}

/// Request to send an SMS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendSmsRequest {
    /// Phone number to send to
    #[serde(rename = "phoneNumber")]
    pub phone_number: String,

    /// Message body
    #[serde(rename = "messageBody")]
    pub message_body: String,
}

/// Telephony and SMS plugin
///
/// Handles phone call notifications and SMS messaging functionality.
///
/// ## Features
///
/// - Phone call notifications (ringing, talking, missed)
/// - SMS conversation management
/// - Thread-safe state caching
/// - Public API for UI integration
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_protocol::plugins::telephony::TelephonyPlugin;
/// use cosmic_connect_protocol::Plugin;
///
/// let plugin = TelephonyPlugin::new();
/// assert_eq!(plugin.name(), "telephony");
/// assert!(!plugin.is_ringing());
/// ```
#[derive(Debug)]
pub struct TelephonyPlugin {
    device_id: Option<String>,

    /// Packet sender for internal D-Bus signal emission
    packet_sender: Option<mpsc::Sender<(String, Packet)>>,

    /// Current call state (if any)
    current_call: Arc<RwLock<Option<TelephonyEvent>>>,

    /// Recent call history (newest first)
    call_history: Arc<RwLock<Vec<TelephonyEvent>>>,

    /// SMS conversations (keyed by thread_id)
    conversations: Arc<RwLock<HashMap<i64, SmsConversation>>>,

    /// Maximum call history entries to keep
    max_history: usize,
}

/// Default maximum call history entries
const DEFAULT_MAX_HISTORY: usize = 100;

impl TelephonyPlugin {
    /// Create a new Telephony plugin
    pub fn new() -> Self {
        Self {
            device_id: None,
            packet_sender: None,
            current_call: Arc::new(RwLock::new(None)),
            call_history: Arc::new(RwLock::new(Vec::new())),
            conversations: Arc::new(RwLock::new(HashMap::new())),
            max_history: DEFAULT_MAX_HISTORY,
        }
    }

    /// Get the current call state
    ///
    /// Returns the current active call event, if any.
    pub fn get_current_call(&self) -> Option<TelephonyEvent> {
        self.current_call.read().ok()?.clone()
    }

    /// Check if phone is currently ringing
    pub fn is_ringing(&self) -> bool {
        self.get_current_call()
            .is_some_and(|c| c.event == CallEvent::Ringing.as_str())
    }

    /// Check if there's an active call
    pub fn has_active_call(&self) -> bool {
        self.get_current_call()
            .is_some_and(|c| c.event == CallEvent::Talking.as_str())
    }

    /// Get recent call history
    ///
    /// Returns call events with newest first.
    pub fn get_call_history(&self) -> Vec<TelephonyEvent> {
        self.call_history
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Get number of calls in history
    pub fn call_history_count(&self) -> usize {
        self.call_history
            .read()
            .map(|guard| guard.len())
            .unwrap_or(0)
    }

    /// Get missed call count from history
    pub fn missed_call_count(&self) -> usize {
        self.call_history
            .read()
            .map(|guard| {
                guard
                    .iter()
                    .filter(|c| c.event == CallEvent::MissedCall.as_str())
                    .count()
            })
            .unwrap_or(0)
    }

    /// Get all SMS conversations
    pub fn get_conversations(&self) -> Vec<SmsConversation> {
        self.conversations
            .read()
            .map(|guard| guard.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get a specific conversation by thread ID
    pub fn get_conversation(&self, thread_id: i64) -> Option<SmsConversation> {
        self.conversations
            .read()
            .ok()
            .and_then(|guard| guard.get(&thread_id).cloned())
    }

    /// Get number of SMS conversations
    pub fn conversation_count(&self) -> usize {
        self.conversations
            .read()
            .map(|guard| guard.len())
            .unwrap_or(0)
    }

    /// Get total unread SMS count across all conversations
    pub fn unread_sms_count(&self) -> usize {
        self.conversations
            .read()
            .map(|guard| {
                guard
                    .values()
                    .flat_map(|c| &c.messages)
                    .filter(|m| m.read == 0 && m.message_type == 1)
                    .count()
            })
            .unwrap_or(0)
    }

    /// Clear current call state
    pub fn clear_current_call(&self) {
        if let Ok(mut guard) = self.current_call.write() {
            *guard = None;
        }
    }

    /// Update current call state (internal)
    fn update_current_call(&self, event: Option<TelephonyEvent>) {
        if let Ok(mut guard) = self.current_call.write() {
            *guard = event;
        }
    }

    /// Add to call history (internal)
    fn add_to_history(&self, event: TelephonyEvent) {
        if let Ok(mut guard) = self.call_history.write() {
            guard.insert(0, event);
            if guard.len() > self.max_history {
                guard.truncate(self.max_history);
            }
        }
    }

    /// Update conversations (internal)
    fn update_conversations(&self, conversations: Vec<SmsConversation>) {
        if let Ok(mut guard) = self.conversations.write() {
            for conv in conversations {
                guard.insert(conv.thread_id, conv);
            }
        }
    }

    /// Create a mute ringer request packet
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::telephony::TelephonyPlugin;
    ///
    /// let plugin = TelephonyPlugin::new();
    /// let packet = plugin.create_mute_request();
    /// assert_eq!(packet.packet_type, "cconnect.telephony.request_mute");
    /// ```
    pub fn create_mute_request(&self) -> Packet {
        debug!("Creating mute ringer request");
        Packet::new(PACKET_TYPE_TELEPHONY_MUTE, json!({}))
    }

    /// Create a request for conversation list
    ///
    /// Requests the latest message in each thread.
    pub fn create_conversations_request(&self) -> Packet {
        debug!("Creating conversations list request");
        Packet::new(PACKET_TYPE_SMS_REQUEST_CONVERSATIONS, json!({}))
    }

    /// Create a request for messages in a conversation
    ///
    /// # Arguments
    ///
    /// * `thread_id` - The conversation thread ID
    /// * `start_timestamp` - Optional earliest message timestamp (ms since epoch)
    /// * `count` - Optional maximum number of messages to return
    pub fn create_conversation_request(
        &self,
        thread_id: i64,
        start_timestamp: Option<i64>,
        count: Option<i32>,
    ) -> Packet {
        debug!("Creating conversation request for thread {}", thread_id);

        let mut body = json!({
            "threadID": thread_id,
        });

        if let Some(ts) = start_timestamp {
            body["rangeStartTimestamp"] = json!(ts);
        }

        if let Some(n) = count {
            body["numberToRequest"] = json!(n);
        }

        Packet::new(PACKET_TYPE_SMS_REQUEST_CONVERSATION, body)
    }

    /// Create a request for a message attachment
    ///
    /// # Arguments
    ///
    /// * `part_id` - The attachment part ID
    /// * `unique_id` - Unique file identifier
    pub fn create_attachment_request(&self, part_id: i64, unique_id: String) -> Packet {
        debug!("Creating attachment request for part {}", part_id);

        Packet::new(
            PACKET_TYPE_SMS_REQUEST_ATTACHMENT,
            json!({
                "part_id": part_id,
                "unique_identifier": unique_id,
            }),
        )
    }

    /// Create a request to send an SMS
    ///
    /// # Arguments
    ///
    /// * `phone_number` - Recipient phone number
    /// * `message` - Message body
    pub fn create_send_sms_request(&self, phone_number: String, message: String) -> Packet {
        debug!("Creating send SMS request to {}", phone_number);

        Packet::new(
            PACKET_TYPE_SMS_REQUEST,
            json!({
                "phoneNumber": phone_number,
                "messageBody": message,
            }),
        )
    }

    /// Emit an internal packet for D-Bus signaling
    ///
    /// Internal packets are intercepted by the daemon and converted to D-Bus signals.
    /// Errors are silently ignored since signal emission is best-effort.
    async fn emit_internal_packet(&self, device_id: &str, packet_type: &str, body: serde_json::Value) {
        if let Some(sender) = &self.packet_sender {
            let packet = Packet::new(packet_type, body);
            let _ = sender.send((device_id.to_string(), packet)).await;
        }
    }

    /// Handle a telephony event packet
    async fn handle_telephony_event(&self, packet: &Packet) -> Result<()> {
        let event: TelephonyEvent = serde_json::from_value(packet.body.clone())
            .map_err(|e| ProtocolError::InvalidPacket(format!("Failed to parse event: {}", e)))?;

        let phone = event.phone_number.as_deref().unwrap_or("Unknown");
        let contact = event.contact_name.as_deref().unwrap_or("Unknown contact");
        let device_id = self.device_id.as_deref().unwrap_or("unknown");

        let event_type = CallEvent::from_str(&event.event).unwrap_or_else(|| {
            warn!("Unknown telephony event: {}", event.event);
            CallEvent::Ringing
        });

        // Common body for all telephony internal signals
        let signal_body = json!({
            "phoneNumber": event.phone_number,
            "contactName": event.contact_name,
        });

        match event_type {
            CallEvent::Ringing => {
                info!("Incoming call from {} ({})", phone, contact);
                self.update_current_call(Some(event.clone()));
                self.add_to_history(event.clone());
                self.emit_internal_packet(
                    device_id,
                    "cconnect.internal.telephony.ringing",
                    signal_body,
                )
                .await;
            }
            CallEvent::Talking => {
                info!("Call in progress with {} ({})", phone, contact);
                self.update_current_call(Some(event.clone()));
                self.emit_internal_packet(
                    device_id,
                    "cconnect.internal.telephony.talking",
                    signal_body,
                )
                .await;
            }
            CallEvent::MissedCall => {
                info!("Missed call from {} ({})", phone, contact);
                self.update_current_call(None);
                self.add_to_history(event.clone());
                self.emit_internal_packet(
                    device_id,
                    "cconnect.internal.telephony.missed_call",
                    signal_body,
                )
                .await;
            }
            CallEvent::Sms => {
                debug!("Received deprecated SMS event via telephony packet");
            }
        }

        Ok(())
    }

    /// Handle SMS messages packet
    async fn handle_sms_messages(&self, packet: &Packet) -> Result<()> {
        let messages: SmsMessages = serde_json::from_value(packet.body.clone())
            .map_err(|e| ProtocolError::InvalidPacket(format!("Failed to parse SMS: {}", e)))?;

        info!(
            "Received {} SMS conversations",
            messages.conversations.len()
        );

        let device_id = self.device_id.as_deref().unwrap_or("unknown");

        // Log conversation details before caching
        for conversation in &messages.conversations {
            debug!(
                "Thread {}: {} messages",
                conversation.thread_id,
                conversation.messages.len()
            );

            for message in &conversation.messages {
                let preview: String = message.body.chars().take(50).collect();
                debug!(
                    "  Message {}: {} from {} at {}",
                    message.id, preview, message.address, message.date
                );

                // Emit signal for unread received messages
                if message.read == 0 && message.message_type == 1 {
                    self.emit_internal_packet(
                        device_id,
                        "cconnect.internal.sms.received",
                        json!({
                            "threadId": message.thread_id,
                            "address": message.address,
                            "body": message.body,
                            "date": message.date,
                        }),
                    )
                    .await;
                }
            }
        }

        let conv_count = messages.conversations.len() as u32;

        // Cache conversations (consumes the data)
        self.update_conversations(messages.conversations);

        // Emit conversations updated signal
        self.emit_internal_packet(
            device_id,
            "cconnect.internal.sms.conversations_updated",
            json!({ "count": conv_count }),
        )
        .await;

        Ok(())
    }
}

impl Default for TelephonyPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for TelephonyPlugin {
    fn name(&self) -> &str {
        "telephony"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_TELEPHONY.to_string(),
            PACKET_TYPE_SMS_MESSAGES.to_string(),
            "kdeconnect.telephony".to_string(),
            "kdeconnect.sms.messages".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_TELEPHONY_MUTE.to_string(),
            PACKET_TYPE_SMS_REQUEST_CONVERSATIONS.to_string(),
            PACKET_TYPE_SMS_REQUEST_CONVERSATION.to_string(),
            PACKET_TYPE_SMS_REQUEST_ATTACHMENT.to_string(),
            PACKET_TYPE_SMS_REQUEST.to_string(),
        ]
    }

    async fn init(
        &mut self,
        device: &Device,
        packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>,
    ) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        self.packet_sender = Some(packet_sender);
        info!("Telephony plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Telephony plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Telephony plugin stopped");
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, _device: &mut Device) -> Result<()> {
        if packet.is_type(PACKET_TYPE_TELEPHONY) || packet.is_type("kdeconnect.telephony") {
            debug!("Received telephony event");
            self.handle_telephony_event(packet).await
        } else if packet.is_type(PACKET_TYPE_SMS_MESSAGES)
            || packet.is_type("kdeconnect.sms.messages")
        {
            debug!("Received SMS messages");
            self.handle_sms_messages(packet).await
        } else {
            warn!("Unexpected packet type: {}", packet.packet_type);
            Ok(())
        }
    }
}

/// Factory for creating Telephony plugin instances
#[derive(Debug, Clone, Copy)]
pub struct TelephonyPluginFactory;

impl PluginFactory for TelephonyPluginFactory {
    fn name(&self) -> &str {
        "telephony"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_TELEPHONY.to_string(),
            PACKET_TYPE_SMS_MESSAGES.to_string(),
            "kdeconnect.telephony".to_string(),
            "kdeconnect.sms.messages".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_TELEPHONY_MUTE.to_string(),
            PACKET_TYPE_SMS_REQUEST_CONVERSATIONS.to_string(),
            PACKET_TYPE_SMS_REQUEST_CONVERSATION.to_string(),
            PACKET_TYPE_SMS_REQUEST_ATTACHMENT.to_string(),
            PACKET_TYPE_SMS_REQUEST.to_string(),
        ]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(TelephonyPlugin::new())
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

    #[tokio::test]
    async fn test_plugin_creation() {
        let plugin = TelephonyPlugin::new();
        assert_eq!(plugin.name(), "telephony");
        assert!(plugin.device_id.is_none());
    }

    #[tokio::test]
    async fn test_plugin_initialization() {
        let mut plugin = TelephonyPlugin::new();
        let device = create_test_device();

        assert!(plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .is_ok());
        assert_eq!(plugin.device_id, Some(device.id().to_string()));
    }

    #[test]
    fn test_create_mute_request() {
        let plugin = TelephonyPlugin::new();
        let packet = plugin.create_mute_request();

        assert_eq!(packet.packet_type, "cconnect.telephony.request_mute");
        assert!(packet.body.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_create_conversations_request() {
        let plugin = TelephonyPlugin::new();
        let packet = plugin.create_conversations_request();

        assert_eq!(packet.packet_type, "cconnect.sms.request_conversations");
        assert!(packet.body.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_create_conversation_request() {
        let plugin = TelephonyPlugin::new();
        let packet = plugin.create_conversation_request(123, Some(1000000), Some(50));

        assert_eq!(packet.packet_type, "cconnect.sms.request_conversation");
        assert_eq!(packet.body["threadID"], 123);
        assert_eq!(packet.body["rangeStartTimestamp"], 1000000);
        assert_eq!(packet.body["numberToRequest"], 50);
    }

    #[test]
    fn test_create_send_sms_request() {
        let plugin = TelephonyPlugin::new();
        let packet =
            plugin.create_send_sms_request("+1234567890".to_string(), "Hello!".to_string());

        assert_eq!(packet.packet_type, "cconnect.sms.request");
        assert_eq!(packet.body["phoneNumber"], "+1234567890");
        assert_eq!(packet.body["messageBody"], "Hello!");
    }

    #[test]
    fn test_call_event_conversion() {
        assert_eq!(CallEvent::Ringing.as_str(), "ringing");
        assert_eq!(CallEvent::from_str("talking"), Some(CallEvent::Talking));
        assert_eq!(CallEvent::from_str("invalid"), None);
    }

    #[tokio::test]
    async fn test_handle_telephony_event() {
        let plugin = TelephonyPlugin::new();

        let packet = Packet::new(
            "cconnect.telephony",
            json!({
                "event": "ringing",
                "phoneNumber": "+1234567890",
                "contactName": "John Doe"
            }),
        );

        assert!(plugin.handle_telephony_event(&packet).await.is_ok());

        // Verify state was updated
        assert!(plugin.is_ringing());
        assert_eq!(plugin.call_history_count(), 1);
    }

    #[test]
    fn test_factory() {
        let factory = TelephonyPluginFactory;
        assert_eq!(factory.name(), "telephony");

        let incoming = factory.incoming_capabilities();
        assert!(incoming.contains(&PACKET_TYPE_TELEPHONY.to_string()));
        assert!(incoming.contains(&PACKET_TYPE_SMS_MESSAGES.to_string()));

        let outgoing = factory.outgoing_capabilities();
        assert!(outgoing.contains(&PACKET_TYPE_TELEPHONY_MUTE.to_string()));
        assert!(outgoing.contains(&PACKET_TYPE_SMS_REQUEST.to_string()));

        let plugin = factory.create();
        assert_eq!(plugin.name(), "telephony");
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = TelephonyPlugin::new();
        let device = create_test_device();

        assert!(plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .is_ok());
        assert!(plugin.start().await.is_ok());
        assert!(plugin.stop().await.is_ok());
    }

    #[tokio::test]
    async fn test_call_state_transitions() {
        let plugin = TelephonyPlugin::new();

        // Initial state
        assert!(!plugin.is_ringing());
        assert!(!plugin.has_active_call());
        assert!(plugin.get_current_call().is_none());

        // Ringing call
        let ringing = Packet::new(
            "cconnect.telephony",
            json!({
                "event": "ringing",
                "phoneNumber": "+1234567890"
            }),
        );
        plugin.handle_telephony_event(&ringing).await.unwrap();
        assert!(plugin.is_ringing());
        assert!(!plugin.has_active_call());

        // Call answered (talking)
        let talking = Packet::new(
            "cconnect.telephony",
            json!({
                "event": "talking",
                "phoneNumber": "+1234567890"
            }),
        );
        plugin.handle_telephony_event(&talking).await.unwrap();
        assert!(!plugin.is_ringing());
        assert!(plugin.has_active_call());

        // Clear call
        plugin.clear_current_call();
        assert!(!plugin.is_ringing());
        assert!(!plugin.has_active_call());
    }

    #[tokio::test]
    async fn test_missed_call_handling() {
        let plugin = TelephonyPlugin::new();

        // Missed call event
        let missed = Packet::new(
            "cconnect.telephony",
            json!({
                "event": "missedCall",
                "phoneNumber": "+1234567890",
                "contactName": "Jane Doe"
            }),
        );
        plugin.handle_telephony_event(&missed).await.unwrap();

        // Should clear current call but add to history
        assert!(plugin.get_current_call().is_none());
        assert_eq!(plugin.call_history_count(), 1);
        assert_eq!(plugin.missed_call_count(), 1);
    }

    #[tokio::test]
    async fn test_sms_conversations() {
        let plugin = TelephonyPlugin::new();

        // Initial state
        assert_eq!(plugin.conversation_count(), 0);
        assert_eq!(plugin.unread_sms_count(), 0);

        // Receive SMS messages
        let sms_packet = Packet::new(
            "cconnect.sms.messages",
            json!({
                "conversations": [
                    {
                        "threadId": 1,
                        "messages": [
                            {
                                "_id": 100,
                                "threadId": 1,
                                "address": "+1234567890",
                                "body": "Hello!",
                                "date": 1700000000000_i64,
                                "type": 1,
                                "read": 0
                            }
                        ]
                    },
                    {
                        "threadId": 2,
                        "messages": [
                            {
                                "_id": 200,
                                "threadId": 2,
                                "address": "+0987654321",
                                "body": "Hi there!",
                                "date": 1700000001000_i64,
                                "type": 1,
                                "read": 1
                            }
                        ]
                    }
                ]
            }),
        );
        plugin.handle_sms_messages(&sms_packet).await.unwrap();

        assert_eq!(plugin.conversation_count(), 2);
        assert_eq!(plugin.unread_sms_count(), 1);

        // Get specific conversation
        let conv = plugin.get_conversation(1).unwrap();
        assert_eq!(conv.thread_id, 1);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].body, "Hello!");

        // Non-existent conversation
        assert!(plugin.get_conversation(999).is_none());
    }

    #[tokio::test]
    async fn test_call_history_limit() {
        let mut plugin = TelephonyPlugin::new();
        plugin.max_history = 3;

        // Add more events than the limit
        for i in 0..5 {
            let packet = Packet::new(
                "cconnect.telephony",
                json!({
                    "event": "ringing",
                    "phoneNumber": format!("+123456789{}", i)
                }),
            );
            plugin.handle_telephony_event(&packet).await.unwrap();
        }

        // Should be truncated to max_history
        assert_eq!(plugin.call_history_count(), 3);

        // Newest should be first
        let history = plugin.get_call_history();
        assert_eq!(history[0].phone_number.as_deref(), Some("+1234567894"));
    }
}
