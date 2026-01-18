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
    #[serde(rename = "thread_id")]
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
    #[serde(rename = "thread_id")]
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
pub struct TelephonyPlugin {
    device_id: Option<String>,
}

impl TelephonyPlugin {
    /// Create a new Telephony plugin
    pub fn new() -> Self {
        Self { device_id: None }
    }

    /// Create a mute ringer request packet
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::telephony::TelephonyPlugin;
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

    /// Handle a telephony event packet
    async fn handle_telephony_event(&self, packet: &Packet) -> Result<()> {
        let event: TelephonyEvent = serde_json::from_value(packet.body.clone())
            .map_err(|e| ProtocolError::InvalidPacket(format!("Failed to parse event: {}", e)))?;

        let event_type = CallEvent::from_str(&event.event).unwrap_or_else(|| {
            warn!("Unknown telephony event: {}", event.event);
            CallEvent::Ringing
        });

        match event_type {
            CallEvent::Ringing => {
                info!(
                    "Incoming call from {} ({})",
                    event.phone_number.as_deref().unwrap_or("Unknown"),
                    event.contact_name.as_deref().unwrap_or("Unknown contact")
                );
            }
            CallEvent::Talking => {
                info!(
                    "Call in progress with {} ({})",
                    event.phone_number.as_deref().unwrap_or("Unknown"),
                    event.contact_name.as_deref().unwrap_or("Unknown contact")
                );
            }
            CallEvent::MissedCall => {
                info!(
                    "Missed call from {} ({})",
                    event.phone_number.as_deref().unwrap_or("Unknown"),
                    event.contact_name.as_deref().unwrap_or("Unknown contact")
                );
            }
            CallEvent::Sms => {
                // SMS events via telephony are deprecated
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

        for conversation in &messages.conversations {
            debug!(
                "Thread {}: {} messages",
                conversation.thread_id,
                conversation.messages.len()
            );

            for message in &conversation.messages {
                debug!(
                    "  Message {}: {} from {} at {}",
                    message.id,
                    message.body.chars().take(50).collect::<String>(),
                    message.address,
                    message.date
                );
            }
        }

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

    async fn init(&mut self, device: &Device) -> Result<()> {
        self.device_id = Some(device.id().to_string());
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
        match packet.packet_type.as_str() {
            PACKET_TYPE_TELEPHONY => {
                debug!("Received telephony event");
                self.handle_telephony_event(packet).await
            }
            PACKET_TYPE_SMS_MESSAGES => {
                debug!("Received SMS messages");
                self.handle_sms_messages(packet).await
            }
            _ => {
                warn!("Unexpected packet type: {}", packet.packet_type);
                Ok(())
            }
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

        assert!(plugin.init(&device).await.is_ok());
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

        assert!(plugin.init(&device).await.is_ok());
        assert!(plugin.start().await.is_ok());
        assert!(plugin.stop().await.is_ok());
    }
}
