//! Find My Phone Plugin
//!
//! This plugin allows making a remote device (usually a phone) ring
//! to help locate it, similar to a traditional cordless phone finder.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `cconnect.findmyphone.request` - Ring phone request (outgoing)
//!
//! **Capabilities**:
//! - Outgoing: `cconnect.findmyphone.request` - Send ring requests
//!
//! ## Behavior
//!
//! - Sending a packet makes the phone ring
//! - Sending a second packet cancels the ringing
//!
//! ## References
//!
//! - [CConnect FindMyPhone Plugin](https://github.com/KDE/cconnect-android/blob/master/src/org/kde/kdeconnect/Plugins/FindMyPhonePlugin/)
//! - [Valent Protocol Documentation](https://valent.andyholmes.ca/documentation/protocol.html)

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde_json::json;
use std::any::Any;
use tracing::{debug, info};

use super::{Plugin, PluginFactory};

/// Packet type for find my phone requests
pub const PACKET_TYPE_FINDMYPHONE_REQUEST: &str = "cconnect.findmyphone.request";

/// Find My Phone plugin for locating devices
pub struct FindMyPhonePlugin {
    device_id: Option<String>,
}

impl FindMyPhonePlugin {
    /// Create a new Find My Phone plugin
    pub fn new() -> Self {
        Self { device_id: None }
    }

    /// Create a ring request packet
    ///
    /// This packet makes the remote device ring. Sending it again cancels the ring.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::findmyphone::FindMyPhonePlugin;
    ///
    /// let plugin = FindMyPhonePlugin::new();
    /// let packet = plugin.create_ring_request();
    /// assert_eq!(packet.packet_type, "cconnect.findmyphone.request");
    /// ```
    pub fn create_ring_request(&self) -> Packet {
        debug!("Creating ring request packet");
        Packet::new(PACKET_TYPE_FINDMYPHONE_REQUEST, json!({}))
    }
}

impl Default for FindMyPhonePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for FindMyPhonePlugin {
    fn name(&self) -> &str {
        "findmyphone"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        // This plugin only sends requests, doesn't receive
        vec![]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![PACKET_TYPE_FINDMYPHONE_REQUEST.to_string()]
    }

    async fn init(&mut self, device: &Device) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!(
            "Find My Phone plugin initialized for device {}",
            device.name()
        );
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Find My Phone plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Find My Phone plugin stopped");
        Ok(())
    }

    async fn handle_packet(&mut self, _packet: &Packet, _device: &mut Device) -> Result<()> {
        // This plugin doesn't handle incoming packets
        Ok(())
    }
}

/// Factory for creating Find My Phone plugin instances
#[derive(Debug, Clone, Copy)]
pub struct FindMyPhonePluginFactory;

impl PluginFactory for FindMyPhonePluginFactory {
    fn name(&self) -> &str {
        "findmyphone"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![PACKET_TYPE_FINDMYPHONE_REQUEST.to_string()]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(FindMyPhonePlugin::new())
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
        let plugin = FindMyPhonePlugin::new();
        assert_eq!(plugin.name(), "findmyphone");
        assert!(plugin.device_id.is_none());
    }

    #[tokio::test]
    async fn test_plugin_initialization() {
        let mut plugin = FindMyPhonePlugin::new();
        let device = create_test_device();

        assert!(plugin.init(&device).await.is_ok());
        assert_eq!(plugin.device_id, Some(device.id().to_string()));
    }

    #[test]
    fn test_create_ring_request() {
        let plugin = FindMyPhonePlugin::new();
        let packet = plugin.create_ring_request();

        assert_eq!(packet.packet_type, "cconnect.findmyphone.request");
        assert!(packet.body.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_factory() {
        let factory = FindMyPhonePluginFactory;
        assert_eq!(factory.name(), "findmyphone");

        let outgoing = factory.outgoing_capabilities();
        assert!(outgoing.contains(&PACKET_TYPE_FINDMYPHONE_REQUEST.to_string()));

        let incoming = factory.incoming_capabilities();
        assert!(incoming.is_empty());

        let plugin = factory.create();
        assert_eq!(plugin.name(), "findmyphone");
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = FindMyPhonePlugin::new();
        let device = create_test_device();

        assert!(plugin.init(&device).await.is_ok());
        assert!(plugin.start().await.is_ok());
        assert!(plugin.stop().await.is_ok());
    }
}
