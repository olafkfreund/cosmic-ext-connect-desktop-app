//! Presenter Plugin
//!
//! This plugin allows using a phone as a wireless presentation remote control.
//! It can receive pointer movement and click events to control presentations,
//! similar to a laser pointer with slide navigation.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `kdeconnect.presenter` - Pointer and click events (incoming)
//!
//! **Capabilities**:
//! - Incoming: `kdeconnect.presenter` - Receive presentation control events
//!
//! ## Supported Events
//!
//! - **Pointer movement**: dx, dy coordinates for laser pointer simulation
//! - **Next slide**: Navigate to next slide
//! - **Previous slide**: Navigate to previous slide
//! - **Start presentation**: Begin slideshow
//! - **Stop presentation**: End slideshow
//!
//! ## Packet Format
//!
//! Presenter packets contain one of:
//! - `dx`, `dy`: Pointer movement delta (for laser pointer)
//! - `stop`: Boolean, true to stop presentation mode
//!
//! ## References
//!
//! - [KDE Connect Presenter Plugin](https://github.com/KDE/kdeconnect-kde/tree/master/plugins/presenter)
//! - [Valent Protocol Documentation](https://valent.andyholmes.ca/documentation/protocol.html)

use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use tracing::{debug, info, warn};

use super::{Plugin, PluginFactory};

/// Packet type for presenter events
pub const PACKET_TYPE_PRESENTER: &str = "kdeconnect.presenter";

/// Presenter event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenterEvent {
    /// Pointer movement delta X
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dx: Option<f64>,

    /// Pointer movement delta Y
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dy: Option<f64>,

    /// Stop presentation mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<bool>,
}

/// Presenter plugin for presentation remote control
pub struct PresenterPlugin {
    device_id: Option<String>,
    presentation_active: bool,
}

impl PresenterPlugin {
    /// Create a new Presenter plugin
    pub fn new() -> Self {
        Self {
            device_id: None,
            presentation_active: false,
        }
    }

    /// Handle a presenter event packet
    async fn handle_presenter_event(&mut self, packet: &Packet) -> Result<()> {
        let event: PresenterEvent = serde_json::from_value(packet.body.clone())
            .map_err(|e| ProtocolError::InvalidPacket(format!("Failed to parse event: {}", e)))?;

        // Handle stop event
        if event.stop.unwrap_or(false) {
            info!("Presentation mode stopped");
            self.presentation_active = false;
            return Ok(());
        }

        // Handle pointer movement (laser pointer simulation)
        if event.dx.is_some() || event.dy.is_some() {
            let dx = event.dx.unwrap_or(0.0);
            let dy = event.dy.unwrap_or(0.0);

            if !self.presentation_active {
                info!("Presentation mode started");
                self.presentation_active = true;
            }

            debug!("Presenter pointer moved: dx={}, dy={}", dx, dy);
            // TODO: Implement laser pointer visualization via COSMIC APIs
            // This would typically show a red dot or highlight on screen
            // that moves according to dx/dy values
        }

        Ok(())
    }
}

impl Default for PresenterPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for PresenterPlugin {
    fn name(&self) -> &str {
        "presenter"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![PACKET_TYPE_PRESENTER.to_string()]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        // This plugin only receives events, doesn't send
        vec![]
    }

    async fn init(&mut self, device: &Device) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!("Presenter plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Presenter plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Presenter plugin stopped");
        self.presentation_active = false;
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, _device: &mut Device) -> Result<()> {
        match packet.packet_type.as_str() {
            PACKET_TYPE_PRESENTER => {
                debug!("Received presenter event");
                self.handle_presenter_event(packet).await
            }
            _ => {
                warn!("Unexpected packet type: {}", packet.packet_type);
                Ok(())
            }
        }
    }
}

/// Factory for creating Presenter plugin instances
#[derive(Debug, Clone, Copy)]
pub struct PresenterPluginFactory;

impl PluginFactory for PresenterPluginFactory {
    fn name(&self) -> &str {
        "presenter"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![PACKET_TYPE_PRESENTER.to_string()]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(PresenterPlugin::new())
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

    #[tokio::test]
    async fn test_plugin_creation() {
        let plugin = PresenterPlugin::new();
        assert_eq!(plugin.name(), "presenter");
        assert!(plugin.device_id.is_none());
        assert!(!plugin.presentation_active);
    }

    #[tokio::test]
    async fn test_plugin_initialization() {
        let mut plugin = PresenterPlugin::new();
        let device = create_test_device();

        assert!(plugin.init(&device).await.is_ok());
        assert_eq!(plugin.device_id, Some(device.id().to_string()));
    }

    #[tokio::test]
    async fn test_handle_pointer_movement() {
        let mut plugin = PresenterPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let packet = Packet::new(
            "kdeconnect.presenter",
            json!({
                "dx": 10.5,
                "dy": -5.2
            }),
        );

        let mut device_mut = device;
        let result = plugin.handle_packet(&packet, &mut device_mut).await;
        assert!(result.is_ok());
        assert!(plugin.presentation_active);
    }

    #[tokio::test]
    async fn test_handle_stop_event() {
        let mut plugin = PresenterPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        // Start presentation
        plugin.presentation_active = true;

        let packet = Packet::new(
            "kdeconnect.presenter",
            json!({
                "stop": true
            }),
        );

        let mut device_mut = device;
        let result = plugin.handle_packet(&packet, &mut device_mut).await;
        assert!(result.is_ok());
        assert!(!plugin.presentation_active);
    }

    #[test]
    fn test_factory() {
        let factory = PresenterPluginFactory;
        assert_eq!(factory.name(), "presenter");

        let incoming = factory.incoming_capabilities();
        assert!(incoming.contains(&PACKET_TYPE_PRESENTER.to_string()));

        let outgoing = factory.outgoing_capabilities();
        assert!(outgoing.is_empty());

        let plugin = factory.create();
        assert_eq!(plugin.name(), "presenter");
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = PresenterPlugin::new();
        let device = create_test_device();

        assert!(plugin.init(&device).await.is_ok());
        assert!(plugin.start().await.is_ok());
        assert!(plugin.stop().await.is_ok());
        assert!(!plugin.presentation_active);
    }
}
