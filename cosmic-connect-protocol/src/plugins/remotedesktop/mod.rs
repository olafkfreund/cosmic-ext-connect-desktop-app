//! RemoteDesktop Plugin
//!
//! Enables full remote desktop access (VNC-based) between COSMIC Desktop machines.
//! Provides screen sharing with mouse/keyboard control over Wayland.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `cconnect.remotedesktop.request` - Request to start remote desktop session
//! - `cconnect.remotedesktop.response` - Response with VNC connection info
//! - `cconnect.remotedesktop.control` - Session control (stop/pause/resume)
//! - `cconnect.remotedesktop.event` - Session event notifications
//!
//! **Capabilities**:
//! - Incoming: `cconnect.remotedesktop.request`, `cconnect.remotedesktop.control`
//! - Outgoing: `cconnect.remotedesktop.response`, `cconnect.remotedesktop.event`
//!
//! ## Packet Formats
//!
//! ### Request Session
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.remotedesktop.request",
//!     "body": {
//!         "mode": "control",
//!         "quality": "medium",
//!         "fps": 30,
//!         "monitors": null
//!     }
//! }
//! ```
//!
//! ### Response
//!
//! ```json
//! {
//!     "id": 1234567891,
//!     "type": "cconnect.remotedesktop.response",
//!     "body": {
//!         "status": "ready",
//!         "port": 5900,
//!         "password": "abc12345",
//!         "resolution": {
//!             "width": 1920,
//!             "height": 1080
//!         }
//!     }
//! }
//! ```
//!
//! ### Control
//!
//! ```json
//! {
//!     "id": 1234567892,
//!     "type": "cconnect.remotedesktop.control",
//!     "body": {
//!         "action": "stop"
//!     }
//! }
//! ```
//!
//! ## Architecture
//!
//! - **Wayland Capture**: PipeWire + Desktop Portal for screen capture
//! - **VNC Server**: Minimal RFB 3.8 implementation on port 5900
//! - **Video Encoding**: H.264 for compression, LZ4 for lossless
//! - **Input Handling**: VNC events → Linux virtual input device
//!
//! ## Security
//!
//! - Random VNC password generated per session
//! - All traffic over TLS (via COSMIC Connect)
//! - Portal permissions required for screen capture
//! - Single connection per session (no concurrent access)
//!
//! ## Use Cases
//!
//! - Remote desktop access between COSMIC machines
//! - Remote support and assistance
//! - Screen sharing for presentations
//! - System administration

pub mod capture;
#[cfg(feature = "remotedesktop")]
pub mod input;
#[cfg(feature = "remotedesktop")]
pub mod session;
#[cfg(feature = "remotedesktop")]
pub mod vnc;

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde_json::json;
use std::any::Any;
use tracing::{debug, error, info, warn};

use super::{Plugin, PluginFactory};

#[cfg(feature = "remotedesktop")]
use session::SessionManager;

/// RemoteDesktop plugin for VNC-based screen sharing
pub struct RemoteDesktopPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Whether the plugin is enabled
    enabled: bool,

    /// Session manager (only with remotedesktop feature)
    #[cfg(feature = "remotedesktop")]
    session_manager: SessionManager,
}

impl RemoteDesktopPlugin {
    /// Create a new RemoteDesktop plugin
    pub fn new() -> Self {
        Self {
            device_id: None,
            enabled: false,
            #[cfg(feature = "remotedesktop")]
            session_manager: SessionManager::new(),
        }
    }
}

impl Default for RemoteDesktopPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for RemoteDesktopPlugin {
    fn name(&self) -> &str {
        "remotedesktop"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.remotedesktop.request".to_string(),
            "cconnect.remotedesktop.control".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.remotedesktop.response".to_string(),
            "cconnect.remotedesktop.event".to_string(),
        ]
    }

    async fn init(&mut self, device: &Device, _packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!(
            "RemoteDesktop plugin initialized for device {}",
            device.name()
        );
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("RemoteDesktop plugin started");
        self.enabled = true;
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("RemoteDesktop plugin stopped");
        self.enabled = false;
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("RemoteDesktop plugin is disabled, ignoring packet");
            return Ok(());
        }

        match packet.packet_type.as_str() {
            "cconnect.remotedesktop.request" => self.handle_request(packet, device).await,
            "cconnect.remotedesktop.control" => self.handle_control(packet, device).await,
            _ => {
                warn!("Unknown packet type: {}", packet.packet_type);
                Ok(())
            }
        }
    }
}

impl RemoteDesktopPlugin {
    /// Handle remote desktop session request
    async fn handle_request(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        info!("Received remote desktop request from {}", device.name());

        #[cfg(feature = "remotedesktop")]
        {
            // Parse request
            let _mode = packet
                .body
                .get("mode")
                .and_then(|v: &serde_json::Value| v.as_str())
                .unwrap_or("control");
            let _quality = packet
                .body
                .get("quality")
                .and_then(|v: &serde_json::Value| v.as_str())
                .unwrap_or("medium");
            let _fps = packet
                .body
                .get("fps")
                .and_then(|v: &serde_json::Value| v.as_u64())
                .unwrap_or(30);

            debug!(
                "Request: mode={}, quality={}, fps={}",
                _mode, _quality, _fps
            );

            // Check if session already active
            let state = self.session_manager.state().await;
            if state != session::SessionState::Idle {
                warn!("Session already active, rejecting request");

                // Send busy response
                let _response = Packet::new(
                    "cconnect.remotedesktop.response",
                    json!({
                        "status": "busy",
                    }),
                );
                // TODO: Implement packet sending in Device
                // device.send_packet(&response).await?;
                warn!("Session already active, would send busy response");
                return Ok(());
            }

            // Start session
            match self.session_manager.start_session(5900).await {
                Ok(session_info) => {
                    info!(
                        "Session started: {}x{} on port {}",
                        session_info.width, session_info.height, session_info.port
                    );

                    // Send ready response
                    let _response = Packet::new(
                        "cconnect.remotedesktop.response",
                        json!({
                            "status": "ready",
                            "port": session_info.port,
                            "password": session_info.password,
                            "resolution": {
                                "width": session_info.width,
                                "height": session_info.height,
                            }
                        }),
                    );
                    // TODO: Implement packet sending in Device
                    // device.send_packet(&response).await?;

                    info!("Session ready, would send response to {}", device.name());
                }
                Err(e) => {
                    error!("Failed to start session: {}", e);

                    // Send denied response
                    let _response = Packet::new(
                        "cconnect.remotedesktop.response",
                        json!({
                            "status": "denied",
                            "error": format!("{}", e),
                        }),
                    );
                    // TODO: Implement packet sending in Device
                    // device.send_packet(&response).await?;
                }
            }
        }

        #[cfg(not(feature = "remotedesktop"))]
        {
            warn!("RemoteDesktop feature not enabled");
            let _response = Packet::new(
                "cconnect.remotedesktop.response",
                json!({
                    "status": "denied",
                    "error": "RemoteDesktop feature not enabled",
                }),
            );
            // TODO: Implement packet sending in Device
            // device.send_packet(&response).await?;
        }

        Ok(())
    }

    /// Handle session control commands
    async fn handle_control(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        info!("Received remote desktop control from {}", device.name());

        let body = &packet.body;
        let action = body.get("action").and_then(|v| v.as_str());

        if action.is_none() {
            warn!("Control packet missing action");
            return Ok(());
        }

        let action = action.unwrap();
        debug!("Control action: {}", action);

        #[cfg(feature = "remotedesktop")]
        {
            let result = match action {
                "stop" => {
                    info!("Stopping session");
                    self.session_manager.stop_session().await
                }
                "pause" => {
                    info!("Pausing session");
                    self.session_manager.pause_session().await
                }
                "resume" => {
                    info!("Resuming session");
                    self.session_manager.resume_session().await
                }
                _ => {
                    warn!("Unknown control action: {}", action);
                    Ok(())
                }
            };

            if let Err(e) = result {
                error!("Control action failed: {}", e);

                // Send event notification
                let _event = Packet::new(
                    "cconnect.remotedesktop.event",
                    json!({
                        "event": "error",
                        "message": format!("{}", e),
                    }),
                );
                // TODO: Implement packet sending in Device
                // device.send_packet(&event).await?;
            } else {
                // Send success event
                let _event = Packet::new(
                    "cconnect.remotedesktop.event",
                    json!({
                        "event": "control_success",
                        "action": action,
                    }),
                );
                // TODO: Implement packet sending in Device
                // device.send_packet(&event).await?;
            }
        }

        #[cfg(not(feature = "remotedesktop"))]
        {
            warn!("RemoteDesktop feature not enabled");
        }

        Ok(())
    }
}

/// Factory for creating RemoteDesktopPlugin instances
#[derive(Debug, Clone, Copy)]
pub struct RemoteDesktopPluginFactory;

impl PluginFactory for RemoteDesktopPluginFactory {
    fn name(&self) -> &str {
        "remotedesktop"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.remotedesktop.request".to_string(),
            "cconnect.remotedesktop.control".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.remotedesktop.response".to_string(),
            "cconnect.remotedesktop.event".to_string(),
        ]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(RemoteDesktopPlugin::new())
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
    fn test_plugin_creation() {
        let plugin = RemoteDesktopPlugin::new();
        assert_eq!(plugin.name(), "remotedesktop");
        assert!(!plugin.enabled);
        assert!(plugin.device_id.is_none());
    }

    #[test]
    fn test_capabilities() {
        let plugin = RemoteDesktopPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 2);
        assert!(incoming.contains(&"cconnect.remotedesktop.request".to_string()));
        assert!(incoming.contains(&"cconnect.remotedesktop.control".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 2);
        assert!(outgoing.contains(&"cconnect.remotedesktop.response".to_string()));
        assert!(outgoing.contains(&"cconnect.remotedesktop.event".to_string()));
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = RemoteDesktopPlugin::new();
        let device = create_test_device();

        // Test init
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();
        assert!(plugin.device_id.is_some());
        assert_eq!(plugin.device_id.as_ref().unwrap(), device.id());

        // Test start
        plugin.start().await.unwrap();
        assert!(plugin.enabled);

        // Test stop
        plugin.stop().await.unwrap();
        assert!(!plugin.enabled);
    }

    #[tokio::test]
    async fn test_handle_request() {
        let mut plugin = RemoteDesktopPlugin::new();
        let device = create_test_device();
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();
        plugin.start().await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.remotedesktop.request",
            json!({
                "mode": "control",
                "quality": "medium",
                "fps": 30
            }),
        );

        // Should not panic, but will return "not implemented" response
        let result = plugin.handle_packet(&packet, &mut device).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_disabled_plugin_ignores_packets() {
        let mut plugin = RemoteDesktopPlugin::new();
        let device = create_test_device();
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();
        // Don't start the plugin - it should be disabled

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.remotedesktop.request",
            json!({
                "mode": "control"
            }),
        );

        // Should return Ok but not process the packet
        let result = plugin.handle_packet(&packet, &mut device).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_factory() {
        let factory = RemoteDesktopPluginFactory;
        assert_eq!(factory.name(), "remotedesktop");

        let plugin = factory.create();
        assert_eq!(plugin.name(), "remotedesktop");
    }

    #[test]
    fn test_factory_capabilities() {
        let factory = RemoteDesktopPluginFactory;

        let incoming = factory.incoming_capabilities();
        assert_eq!(incoming.len(), 2);

        let outgoing = factory.outgoing_capabilities();
        assert_eq!(outgoing.len(), 2);
    }
}
