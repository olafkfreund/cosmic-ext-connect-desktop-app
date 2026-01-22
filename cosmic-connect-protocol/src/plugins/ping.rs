//! Ping Plugin
//!
//! Simple connectivity testing plugin that sends and receives ping packets.
//! Used to verify device connectivity and measure basic latency.
//!
//! ## Protocol
//!
//! **Packet Type**: `cconnect.ping`
//!
//! **Capabilities**:
//! - Incoming: `cconnect.ping` - Can receive pings
//! - Outgoing: `cconnect.ping` - Can send pings
//!
//! ## Packet Format
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.ping",
//!     "body": {
//!         "message": "Optional message"
//!     }
//! }
//! ```
//!
//! The `message` field is optional. If omitted, the packet body is empty.
//!
//! ## Behavior
//!
//! - **Receiving**: When a ping is received, it's logged and can trigger notifications
//! - **Bidirectional**: Both devices can send and receive pings
//! - **Simple**: No response required, fire-and-forget
//! - **Notifications**: Pings are typically displayed as notifications
//!
//! ## Use Cases
//!
//! - Connectivity testing
//! - Quick messages between devices
//! - Latency measurement
//! - Keep-alive checks
//!
//! ## Example
//!
//! ```rust,ignore
//! use cosmic_connect_core::plugins::ping::PingPlugin;
//! use cosmic_connect_core::{Plugin, PluginManager};
//!
//! // Create and register plugin
//! let mut manager = PluginManager::new();
//! manager.register(Box::new(PingPlugin::new()))?;
//!
//! // Initialize with device
//! manager.init_all(&device).await?;
//! manager.start_all().await?;
//!
//! // Send a ping
//! let plugin = PingPlugin::new();
//! let packet = plugin.create_ping(Some("Hello!".to_string()));
//! // Send packet to device...
//! ```
//!
//! ## References
//!
//! - [Valent Protocol - Ping](https://valent.andyholmes.ca/documentation/protocol.html)

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{debug, info};

use super::{Plugin, PluginFactory};

/// Ping plugin for connectivity testing
///
/// Handles `cconnect.ping` packets for simple device-to-device communication
/// and connectivity testing.
///
/// ## Features
///
/// - Send and receive ping packets
/// - Optional message in ping
/// - Ping statistics (count)
/// - Thread-safe statistics tracking
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_core::plugins::ping::PingPlugin;
/// use cosmic_connect_core::Plugin;
///
/// let plugin = PingPlugin::new();
/// assert_eq!(plugin.name(), "ping");
/// assert_eq!(plugin.pings_received(), 0);
/// ```
#[derive(Debug)]
pub struct PingPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Count of pings received
    pings_received: Arc<AtomicU64>,

    /// Count of pings sent
    pings_sent: Arc<AtomicU64>,
}

impl PingPlugin {
    /// Create a new ping plugin
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::ping::PingPlugin;
    ///
    /// let plugin = PingPlugin::new();
    /// assert_eq!(plugin.pings_received(), 0);
    /// assert_eq!(plugin.pings_sent(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            device_id: None,
            pings_received: Arc::new(AtomicU64::new(0)),
            pings_sent: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Get the number of pings received
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::ping::PingPlugin;
    ///
    /// let plugin = PingPlugin::new();
    /// assert_eq!(plugin.pings_received(), 0);
    /// ```
    pub fn pings_received(&self) -> u64 {
        self.pings_received.load(Ordering::Relaxed)
    }

    /// Get the number of pings sent
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::ping::PingPlugin;
    ///
    /// let plugin = PingPlugin::new();
    /// assert_eq!(plugin.pings_sent(), 0);
    /// ```
    pub fn pings_sent(&self) -> u64 {
        self.pings_sent.load(Ordering::Relaxed)
    }

    /// Create a ping packet
    ///
    /// Creates a `cconnect.ping` packet with an optional message.
    ///
    /// # Parameters
    ///
    /// - `message`: Optional message to include in the ping
    ///
    /// # Returns
    ///
    /// A `Packet` ready to be sent to the device
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::ping::PingPlugin;
    ///
    /// let plugin = PingPlugin::new();
    ///
    /// // Ping with message
    /// let packet = plugin.create_ping(Some("Hello!".to_string()));
    /// assert_eq!(packet.packet_type, "cconnect.ping");
    ///
    /// // Ping without message
    /// let packet = plugin.create_ping(None);
    /// assert_eq!(packet.packet_type, "cconnect.ping");
    /// ```
    pub fn create_ping(&self, message: Option<String>) -> Packet {
        let body = if let Some(msg) = message {
            json!({ "message": msg })
        } else {
            json!({})
        };

        Packet::new("cconnect.ping", body)
    }

    /// Handle an incoming ping packet
    ///
    /// Processes a received ping, extracts any message, and updates statistics.
    fn handle_ping(&self, packet: &Packet, device: &Device) {
        // Increment received count
        self.pings_received.fetch_add(1, Ordering::Relaxed);

        // Extract optional message
        let message = packet
            .body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if message.is_empty() {
            info!("Received ping from {} ({})", device.name(), device.id());
        } else {
            info!(
                "Received ping from {} ({}): {}",
                device.name(),
                device.id(),
                message
            );
        }

        debug!(
            "Ping statistics - received: {}, sent: {}",
            self.pings_received(),
            self.pings_sent()
        );
    }
}

impl Default for PingPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for PingPlugin {
    fn name(&self) -> &str {
        "ping"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec!["cconnect.ping".to_string(), "kdeconnect.ping".to_string()]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec!["cconnect.ping".to_string()]
    }

    async fn init(&mut self, device: &Device, _packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!("Ping plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Ping plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!(
            "Ping plugin stopped - received: {}, sent: {}",
            self.pings_received(),
            self.pings_sent()
        );
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if packet.is_type("cconnect.ping") {
            self.handle_ping(packet, device);
        }
        Ok(())
    }
}

/// Factory for creating PingPlugin instances
///
/// Creates a new PingPlugin for each device connection, allowing
/// independent ping tracking per device.
///
/// # Example
///
/// ```rust
/// use cosmic_connect_core::plugins::ping::PingPluginFactory;
/// use cosmic_connect_core::plugins::PluginFactory;
/// use std::sync::Arc;
///
/// let factory: Arc<dyn PluginFactory> = Arc::new(PingPluginFactory);
/// let plugin = factory.create();
/// assert_eq!(plugin.name(), "ping");
/// ```
#[derive(Debug, Clone, Copy)]
pub struct PingPluginFactory;

impl PluginFactory for PingPluginFactory {
    fn name(&self) -> &str {
        "ping"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec!["cconnect.ping".to_string(), "kdeconnect.ping".to_string()]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec!["cconnect.ping".to_string()]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(PingPlugin::new())
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
        let plugin = PingPlugin::new();
        assert_eq!(plugin.name(), "ping");
        assert_eq!(plugin.pings_received(), 0);
        assert_eq!(plugin.pings_sent(), 0);
    }

    #[test]
    fn test_capabilities() {
        let plugin = PingPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 2);
        assert!(incoming.contains(&"cconnect.ping".to_string()));
        assert!(incoming.contains(&"kdeconnect.ping".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0], "cconnect.ping");
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = PingPlugin::new();
        let device = create_test_device();

        // Initialize
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();
        assert!(plugin.device_id.is_some());

        // Start
        plugin.start().await.unwrap();

        // Stop
        plugin.stop().await.unwrap();
    }

    #[test]
    fn test_create_ping_with_message() {
        let plugin = PingPlugin::new();
        let packet = plugin.create_ping(Some("Hello!".to_string()));

        assert_eq!(packet.packet_type, "cconnect.ping");
        assert_eq!(
            packet.body.get("message").and_then(|v| v.as_str()),
            Some("Hello!")
        );
    }

    #[test]
    fn test_create_ping_without_message() {
        let plugin = PingPlugin::new();
        let packet = plugin.create_ping(None);

        assert_eq!(packet.packet_type, "cconnect.ping");
        assert!(packet.body.get("message").is_none());
    }

    #[tokio::test]
    async fn test_handle_ping_without_message() {
        let mut plugin = PingPlugin::new();
        let device = create_test_device();
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new("cconnect.ping", json!({}));

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        assert_eq!(plugin.pings_received(), 1);
    }

    #[tokio::test]
    async fn test_handle_ping_with_message() {
        let mut plugin = PingPlugin::new();
        let device = create_test_device();
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new("cconnect.ping", json!({ "message": "Test message" }));

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        assert_eq!(plugin.pings_received(), 1);
    }

    #[tokio::test]
    async fn test_multiple_pings() {
        let mut plugin = PingPlugin::new();
        let device = create_test_device();
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();

        let mut device = create_test_device();

        // Send multiple pings
        for i in 0..5 {
            let packet = Packet::new("cconnect.ping", json!({ "message": format!("Ping {}", i) }));
            plugin.handle_packet(&packet, &mut device).await.unwrap();
        }

        assert_eq!(plugin.pings_received(), 5);
    }

    #[tokio::test]
    async fn test_ignore_non_ping_packets() {
        let mut plugin = PingPlugin::new();
        let device = create_test_device();
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new("cconnect.battery", json!({}));

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        // Should not increment ping counter
        assert_eq!(plugin.pings_received(), 0);
    }

    #[test]
    fn test_statistics() {
        let plugin = PingPlugin::new();

        // Initial state
        assert_eq!(plugin.pings_received(), 0);
        assert_eq!(plugin.pings_sent(), 0);

        // Simulate receiving pings
        plugin.pings_received.fetch_add(3, Ordering::Relaxed);
        assert_eq!(plugin.pings_received(), 3);

        // Simulate sending pings
        plugin.pings_sent.fetch_add(2, Ordering::Relaxed);
        assert_eq!(plugin.pings_sent(), 2);
    }
}
