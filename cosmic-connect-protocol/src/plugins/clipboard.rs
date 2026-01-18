//! Clipboard Plugin
//!
//! Enables bidirectional text clipboard synchronization between CConnect devices.
//! Monitors local clipboard changes and broadcasts updates to connected devices while
//! preventing infinite sync loops through timestamp-based validation.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - Incoming: `cconnect.clipboard`, `cconnect.clipboard.connect`
//! - Outgoing: `cconnect.clipboard`, `cconnect.clipboard.connect`
//!
//! **Capabilities**: `cconnect.clipboard`
//!
//! ## Clipboard Update
//!
//! Standard clipboard update sent when local clipboard content changes:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.clipboard",
//!     "body": {
//!         "content": "some text"
//!     }
//! }
//! ```
//!
//! ## Connection Sync
//!
//! Initial clipboard sync sent when devices connect, includes timestamp:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.clipboard.connect",
//!     "body": {
//!         "content": "some text",
//!         "timestamp": 1640000000000
//!     }
//! }
//! ```
//!
//! ## Sync Loop Prevention
//!
//! To prevent devices from endlessly updating each other's clipboards:
//!
//! 1. Each clipboard update is timestamped with UNIX epoch milliseconds
//! 2. Devices track the timestamp of their current clipboard content
//! 3. Incoming updates with timestamp ≤ local timestamp are **ignored**
//! 4. Incoming updates with timestamp > local timestamp are **accepted**
//! 5. Connect packets with timestamp `0` are ignored (no content)
//!
//! ## Workflow
//!
//! ### Sending Updates
//! 1. Local clipboard changes detected
//! 2. Record timestamp of the change
//! 3. Send `cconnect.clipboard` packet with new content
//!
//! ### Receiving Updates
//! 1. Receive clipboard packet
//! 2. Extract content and timestamp (if present)
//! 3. Compare with local timestamp
//! 4. If newer, update local clipboard and timestamp
//! 5. If older or equal, ignore
//!
//! ### Device Connection
//! 1. Device connects
//! 2. Send `cconnect.clipboard.connect` with current content and timestamp
//! 3. Peer follows standard receiving workflow
//!
//! ## Example
//!
//! ```rust,ignore
//! use cosmic_connect_core::plugins::clipboard::*;
//! use cosmic_connect_core::{Plugin, PluginManager};
//!
//! // Create and register plugin
//! let mut manager = PluginManager::new();
//! manager.register(Box::new(ClipboardPlugin::new()))?;
//!
//! // Initialize with device
//! manager.init_all(&device).await?;
//! manager.start_all().await?;
//!
//! // Update clipboard
//! let plugin = ClipboardPlugin::new();
//! let packet = plugin.create_clipboard_packet("Hello from device!".to_string()).await;
//! // Send packet to peer...
//!
//! // On device connection, sync clipboard
//! let packet = plugin.create_connect_packet().await;
//! // Send packet to newly connected peer...
//! ```
//!
//! ## References
//!
//! - [Valent Protocol Documentation](https://valent.andyholmes.ca/documentation/protocol.html)
//! - [CConnect Clipboard Plugin](https://invent.kde.org/network/cconnect-kde/tree/master/plugins/clipboard)

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

use super::{Plugin, PluginFactory};

/// Clipboard state with content and timestamp
///
/// Tracks the current clipboard content and when it was last modified.
/// The timestamp is used for sync loop prevention.
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_core::plugins::clipboard::ClipboardState;
///
/// let state = ClipboardState {
///     content: "Hello, World!".to_string(),
///     timestamp: 1640000000000,
/// };
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardState {
    /// Current clipboard text content
    pub content: String,

    /// UNIX epoch timestamp in milliseconds when content was last modified
    pub timestamp: i64,
}

impl ClipboardState {
    /// Create a new clipboard state with current timestamp
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::clipboard::ClipboardState;
    ///
    /// let state = ClipboardState::new("Hello".to_string());
    /// assert_eq!(state.content, "Hello");
    /// assert!(state.timestamp > 0);
    /// ```
    pub fn new(content: String) -> Self {
        Self {
            content,
            timestamp: Utc::now().timestamp_millis(),
        }
    }

    /// Create a clipboard state with explicit timestamp
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::clipboard::ClipboardState;
    ///
    /// let state = ClipboardState::with_timestamp("Hello".to_string(), 1640000000000);
    /// assert_eq!(state.content, "Hello");
    /// assert_eq!(state.timestamp, 1640000000000);
    /// ```
    pub fn with_timestamp(content: String, timestamp: i64) -> Self {
        Self { content, timestamp }
    }

    /// Create an empty clipboard state
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::clipboard::ClipboardState;
    ///
    /// let state = ClipboardState::empty();
    /// assert!(state.content.is_empty());
    /// assert_eq!(state.timestamp, 0);
    /// ```
    pub fn empty() -> Self {
        Self {
            content: String::new(),
            timestamp: 0,
        }
    }

    /// Check if clipboard state is empty
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::clipboard::ClipboardState;
    ///
    /// let empty = ClipboardState::empty();
    /// assert!(empty.is_empty());
    ///
    /// let filled = ClipboardState::new("text".to_string());
    /// assert!(!filled.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    /// Check if this state is newer than another
    ///
    /// Used for sync loop prevention.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::clipboard::ClipboardState;
    ///
    /// let older = ClipboardState::with_timestamp("old".to_string(), 1000);
    /// let newer = ClipboardState::with_timestamp("new".to_string(), 2000);
    ///
    /// assert!(newer.is_newer_than(&older));
    /// assert!(!older.is_newer_than(&newer));
    /// ```
    pub fn is_newer_than(&self, other: &ClipboardState) -> bool {
        self.timestamp > other.timestamp
    }
}

impl Default for ClipboardState {
    fn default() -> Self {
        Self::empty()
    }
}

/// Clipboard sync plugin for text content synchronization
///
/// Handles `cconnect.clipboard` packets for syncing clipboard content
/// between devices. Uses timestamp-based validation to prevent sync loops.
///
/// ## Features
///
/// - Bidirectional clipboard sync
/// - Timestamp-based sync loop prevention
/// - Device connection sync
/// - UTF-8 text content support
/// - Thread-safe state management
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_core::plugins::clipboard::ClipboardPlugin;
/// use cosmic_connect_core::Plugin;
///
/// let plugin = ClipboardPlugin::new();
/// assert_eq!(plugin.name(), "clipboard");
/// ```
#[derive(Debug)]
pub struct ClipboardPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Current clipboard state (content + timestamp)
    state: Arc<RwLock<ClipboardState>>,
}

impl ClipboardPlugin {
    /// Create a new clipboard plugin
    ///
    /// Initializes with empty clipboard state.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::clipboard::ClipboardPlugin;
    ///
    /// let plugin = ClipboardPlugin::new();
    /// ```
    pub fn new() -> Self {
        Self {
            device_id: None,
            state: Arc::new(RwLock::new(ClipboardState::empty())),
        }
    }

    /// Create a standard clipboard update packet
    ///
    /// Creates `cconnect.clipboard` packet for syncing clipboard changes.
    /// Does not include timestamp (standard update).
    ///
    /// # Parameters
    ///
    /// - `content`: New clipboard text content
    ///
    /// # Returns
    ///
    /// Packet ready to be sent
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn example() {
    /// use cosmic_connect_core::plugins::clipboard::ClipboardPlugin;
    ///
    /// let plugin = ClipboardPlugin::new();
    /// let packet = plugin.create_clipboard_packet("Hello!".to_string()).await;
    /// assert_eq!(packet.packet_type, "cconnect.clipboard");
    /// # }
    /// ```
    pub async fn create_clipboard_packet(&self, content: String) -> Packet {
        // Update internal state
        let new_state = ClipboardState::new(content.clone());
        *self.state.write().await = new_state;

        Packet::new("cconnect.clipboard", json!({ "content": content }))
    }

    /// Create a clipboard connect packet
    ///
    /// Creates `cconnect.clipboard.connect` packet with current content
    /// and timestamp. Sent when devices connect to sync initial state.
    ///
    /// # Returns
    ///
    /// Connect packet with timestamp
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn example() {
    /// use cosmic_connect_core::plugins::clipboard::ClipboardPlugin;
    ///
    /// let plugin = ClipboardPlugin::new();
    /// let packet = plugin.create_connect_packet().await;
    /// assert_eq!(packet.packet_type, "cconnect.clipboard.connect");
    /// # }
    /// ```
    pub async fn create_connect_packet(&self) -> Packet {
        let state = self.state.read().await;
        Packet::new(
            "cconnect.clipboard.connect",
            json!({
                "content": state.content,
                "timestamp": state.timestamp
            }),
        )
    }

    /// Get current clipboard content
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn example() {
    /// use cosmic_connect_core::plugins::clipboard::ClipboardPlugin;
    ///
    /// let plugin = ClipboardPlugin::new();
    /// let content = plugin.get_content().await;
    /// println!("Clipboard: {}", content);
    /// # }
    /// ```
    pub async fn get_content(&self) -> String {
        self.state.read().await.content.clone()
    }

    /// Get current clipboard timestamp
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn example() {
    /// use cosmic_connect_core::plugins::clipboard::ClipboardPlugin;
    ///
    /// let plugin = ClipboardPlugin::new();
    /// let timestamp = plugin.get_timestamp().await;
    /// println!("Last modified: {}", timestamp);
    /// # }
    /// ```
    pub async fn get_timestamp(&self) -> i64 {
        self.state.read().await.timestamp
    }

    /// Get complete clipboard state
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn example() {
    /// use cosmic_connect_core::plugins::clipboard::ClipboardPlugin;
    ///
    /// let plugin = ClipboardPlugin::new();
    /// let state = plugin.get_state().await;
    /// println!("Content: {}, Timestamp: {}", state.content, state.timestamp);
    /// # }
    /// ```
    pub async fn get_state(&self) -> ClipboardState {
        self.state.read().await.clone()
    }

    /// Update clipboard content
    ///
    /// Sets new clipboard content with current timestamp.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn example() {
    /// use cosmic_connect_core::plugins::clipboard::ClipboardPlugin;
    ///
    /// let plugin = ClipboardPlugin::new();
    /// plugin.set_content("New content".to_string()).await;
    /// # }
    /// ```
    pub async fn set_content(&self, content: String) {
        *self.state.write().await = ClipboardState::new(content);
    }

    /// Update clipboard with specific timestamp
    ///
    /// Used when applying remote clipboard updates.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn example() {
    /// use cosmic_connect_core::plugins::clipboard::ClipboardPlugin;
    ///
    /// let plugin = ClipboardPlugin::new();
    /// plugin.set_content_with_timestamp("Content".to_string(), 1640000000000).await;
    /// # }
    /// ```
    pub async fn set_content_with_timestamp(&self, content: String, timestamp: i64) {
        *self.state.write().await = ClipboardState::with_timestamp(content, timestamp);
    }

    /// Handle incoming clipboard update packet
    ///
    /// Processes standard clipboard updates (without timestamp).
    /// Always applies the update since standard packets don't include timestamp.
    async fn handle_clipboard_update(&self, packet: &Packet, device: &Device) {
        let content = packet
            .body
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if content.is_empty() {
            debug!(
                "Received empty clipboard update from {} ({})",
                device.name(),
                device.id()
            );
            return;
        }

        info!(
            "Received clipboard update from {} ({}): {} chars",
            device.name(),
            device.id(),
            content.len()
        );

        // Standard updates always applied (no timestamp validation)
        self.set_content(content.to_string()).await;

        debug!(
            "Clipboard updated - timestamp: {}",
            self.get_timestamp().await
        );
    }

    /// Handle incoming clipboard connect packet
    ///
    /// Processes clipboard sync on device connection.
    /// Validates timestamp to prevent applying older content.
    async fn handle_clipboard_connect(&self, packet: &Packet, device: &Device) {
        let content = packet
            .body
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let timestamp = packet
            .body
            .get("timestamp")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        // Ignore packets with timestamp 0 (no content)
        if timestamp == 0 {
            debug!(
                "Ignoring connect packet from {} ({}) with timestamp 0",
                device.name(),
                device.id()
            );
            return;
        }

        let current_state = self.state.read().await.clone();

        // Only apply if incoming timestamp is newer
        if timestamp > current_state.timestamp {
            info!(
                "Received clipboard connect from {} ({}): {} chars (timestamp: {})",
                device.name(),
                device.id(),
                content.len(),
                timestamp
            );

            self.set_content_with_timestamp(content.to_string(), timestamp)
                .await;

            debug!(
                "Clipboard synced - new timestamp: {}",
                self.get_timestamp().await
            );
        } else {
            debug!(
                "Ignoring connect packet from {} ({}) - timestamp {} <= local {}",
                device.name(),
                device.id(),
                timestamp,
                current_state.timestamp
            );
        }
    }
}

impl Default for ClipboardPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ClipboardPlugin {
    fn name(&self) -> &str {
        "clipboard"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.clipboard".to_string(),
            "cconnect.clipboard.connect".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.clipboard".to_string(),
            "cconnect.clipboard.connect".to_string(),
        ]
    }

    async fn init(&mut self, device: &Device) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!("Clipboard plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Clipboard plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        let state = self.state.read().await;
        info!(
            "Clipboard plugin stopped - last timestamp: {}",
            state.timestamp
        );
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        match packet.packet_type.as_str() {
            "cconnect.clipboard" => {
                self.handle_clipboard_update(packet, device).await;
            }
            "cconnect.clipboard.connect" => {
                self.handle_clipboard_connect(packet, device).await;
            }
            _ => {}
        }
        Ok(())
    }
}

/// Factory for creating ClipboardPlugin instances
#[derive(Debug, Clone, Copy)]
pub struct ClipboardPluginFactory;

impl PluginFactory for ClipboardPluginFactory {
    fn name(&self) -> &str {
        "clipboard"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.clipboard".to_string(),
            "cconnect.clipboard.connect".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.clipboard".to_string(),
            "cconnect.clipboard.connect".to_string(),
        ]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(ClipboardPlugin::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceInfo, DeviceType};
    use serde_json::json;

    fn create_test_device() -> Device {
        let info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1716);
        Device::from_discovery(info)
    }

    #[test]
    fn test_clipboard_state_creation() {
        let state = ClipboardState::new("Hello".to_string());
        assert_eq!(state.content, "Hello");
        assert!(state.timestamp > 0);
    }

    #[test]
    fn test_clipboard_state_with_timestamp() {
        let state = ClipboardState::with_timestamp("Test".to_string(), 1640000000000);
        assert_eq!(state.content, "Test");
        assert_eq!(state.timestamp, 1640000000000);
    }

    #[test]
    fn test_clipboard_state_empty() {
        let state = ClipboardState::empty();
        assert!(state.content.is_empty());
        assert_eq!(state.timestamp, 0);
        assert!(state.is_empty());
    }

    #[test]
    fn test_clipboard_state_comparison() {
        let older = ClipboardState::with_timestamp("old".to_string(), 1000);
        let newer = ClipboardState::with_timestamp("new".to_string(), 2000);

        assert!(newer.is_newer_than(&older));
        assert!(!older.is_newer_than(&newer));
        assert!(!older.is_newer_than(&older)); // Same timestamp
    }

    #[test]
    fn test_plugin_creation() {
        let plugin = ClipboardPlugin::new();
        assert_eq!(plugin.name(), "clipboard");
    }

    #[test]
    fn test_capabilities() {
        let plugin = ClipboardPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 2);
        assert!(incoming.contains(&"cconnect.clipboard".to_string()));
        assert!(incoming.contains(&"cconnect.clipboard.connect".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 2);
        assert!(outgoing.contains(&"cconnect.clipboard".to_string()));
        assert!(outgoing.contains(&"cconnect.clipboard.connect".to_string()));
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = ClipboardPlugin::new();
        let device = create_test_device();

        // Initialize
        plugin.init(&device).await.unwrap();
        assert!(plugin.device_id.is_some());

        // Start
        plugin.start().await.unwrap();

        // Stop
        plugin.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_create_clipboard_packet() {
        let plugin = ClipboardPlugin::new();
        let packet = plugin
            .create_clipboard_packet("Test content".to_string())
            .await;

        assert_eq!(packet.packet_type, "cconnect.clipboard");
        assert_eq!(
            packet.body.get("content").and_then(|v| v.as_str()),
            Some("Test content")
        );

        // Check internal state updated
        let content = plugin.get_content().await;
        assert_eq!(content, "Test content");
    }

    #[tokio::test]
    async fn test_create_connect_packet() {
        let plugin = ClipboardPlugin::new();

        // Set some content first
        plugin.set_content("Initial content".to_string()).await;

        let packet = plugin.create_connect_packet().await;

        assert_eq!(packet.packet_type, "cconnect.clipboard.connect");
        assert_eq!(
            packet.body.get("content").and_then(|v| v.as_str()),
            Some("Initial content")
        );
        assert!(packet
            .body
            .get("timestamp")
            .and_then(|v| v.as_i64())
            .is_some());
    }

    #[tokio::test]
    async fn test_get_set_content() {
        let plugin = ClipboardPlugin::new();

        // Initially empty
        let content = plugin.get_content().await;
        assert!(content.is_empty());

        // Set content
        plugin.set_content("New content".to_string()).await;

        // Verify
        let content = plugin.get_content().await;
        assert_eq!(content, "New content");

        // Timestamp should be set
        let timestamp = plugin.get_timestamp().await;
        assert!(timestamp > 0);
    }

    #[tokio::test]
    async fn test_set_content_with_timestamp() {
        let plugin = ClipboardPlugin::new();

        plugin
            .set_content_with_timestamp("Content".to_string(), 1640000000000)
            .await;

        let state = plugin.get_state().await;
        assert_eq!(state.content, "Content");
        assert_eq!(state.timestamp, 1640000000000);
    }

    #[tokio::test]
    async fn test_handle_clipboard_update() {
        let mut plugin = ClipboardPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.clipboard",
            json!({ "content": "Updated clipboard" }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        let content = plugin.get_content().await;
        assert_eq!(content, "Updated clipboard");
    }

    #[tokio::test]
    async fn test_handle_clipboard_connect_newer() {
        let mut plugin = ClipboardPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        // Set old content
        plugin
            .set_content_with_timestamp("Old content".to_string(), 1000)
            .await;

        // Receive newer content
        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.clipboard.connect",
            json!({
                "content": "Newer content",
                "timestamp": 2000i64
            }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        // Should update
        let state = plugin.get_state().await;
        assert_eq!(state.content, "Newer content");
        assert_eq!(state.timestamp, 2000);
    }

    #[tokio::test]
    async fn test_handle_clipboard_connect_older() {
        let mut plugin = ClipboardPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        // Set current content
        plugin
            .set_content_with_timestamp("Current content".to_string(), 2000)
            .await;

        // Receive older content
        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.clipboard.connect",
            json!({
                "content": "Older content",
                "timestamp": 1000i64
            }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        // Should NOT update
        let state = plugin.get_state().await;
        assert_eq!(state.content, "Current content");
        assert_eq!(state.timestamp, 2000);
    }

    #[tokio::test]
    async fn test_handle_clipboard_connect_zero_timestamp() {
        let mut plugin = ClipboardPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        // Set current content
        plugin
            .set_content_with_timestamp("Current content".to_string(), 1000)
            .await;

        // Receive content with timestamp 0
        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.clipboard.connect",
            json!({
                "content": "Zero timestamp content",
                "timestamp": 0i64
            }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        // Should NOT update (timestamp 0 ignored)
        let state = plugin.get_state().await;
        assert_eq!(state.content, "Current content");
        assert_eq!(state.timestamp, 1000);
    }

    #[tokio::test]
    async fn test_handle_empty_clipboard() {
        let mut plugin = ClipboardPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        // Set initial content
        plugin.set_content("Initial".to_string()).await;

        // Receive empty content
        let mut device = create_test_device();
        let packet = Packet::new("cconnect.clipboard", json!({ "content": "" }));

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        // Should not update with empty content
        let content = plugin.get_content().await;
        assert_eq!(content, "Initial");
    }

    #[tokio::test]
    async fn test_multiple_updates() {
        let mut plugin = ClipboardPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();

        // First update
        let packet1 = Packet::new("cconnect.clipboard", json!({ "content": "First update" }));
        plugin.handle_packet(&packet1, &mut device).await.unwrap();

        let content = plugin.get_content().await;
        assert_eq!(content, "First update");

        // Second update
        let packet2 = Packet::new("cconnect.clipboard", json!({ "content": "Second update" }));
        plugin.handle_packet(&packet2, &mut device).await.unwrap();

        let content = plugin.get_content().await;
        assert_eq!(content, "Second update");
    }

    #[tokio::test]
    async fn test_sync_loop_prevention() {
        let mut plugin = ClipboardPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        // Set current state
        plugin
            .set_content_with_timestamp("Current".to_string(), 2000)
            .await;

        let mut device = create_test_device();

        // Try to apply same timestamp (should be ignored)
        let packet = Packet::new(
            "cconnect.clipboard.connect",
            json!({
                "content": "Same timestamp",
                "timestamp": 2000i64
            }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        // Should not update
        let state = plugin.get_state().await;
        assert_eq!(state.content, "Current");
        assert_eq!(state.timestamp, 2000);
    }
}
