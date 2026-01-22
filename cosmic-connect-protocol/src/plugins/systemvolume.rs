//! System Volume Plugin
//!
//! Allows remote control of system volume and audio sinks.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `cconnect.systemvolume.request` - Volume control request (incoming)
//!
//! **Capabilities**:
//! - Incoming: `cconnect.systemvolume.request`
//!
//! ## Packet Format
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.systemvolume.request",
//!     "body": {
//!         "name": "Sink Name",
//!         "volume": 75,
//!         "muted": false,
//!         "enabled": true,
//!         "requestSinks": false
//!     }
//! }
//! ```

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use tracing::{debug, info, warn};

use super::{Plugin, PluginFactory};

/// Packet type for system volume requests
pub const PACKET_TYPE_SYSTEMVOLUME_REQUEST: &str = "cconnect.systemvolume.request";

/// System volume request body
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemVolumeRequest {
    /// Name of the audio sink to control
    pub name: Option<String>,
    /// Volume level (0-100)
    pub volume: Option<i32>,
    /// Mute status
    pub muted: Option<bool>,
    /// Set as default sink
    pub enabled: Option<bool>,
    /// Request list of sinks from this device
    #[serde(rename = "requestSinks", default)]
    pub request_sinks: bool,
}

/// System Volume plugin
pub struct SystemVolumePlugin {
    device_id: Option<String>,
}

impl SystemVolumePlugin {
    /// Create a new System Volume plugin
    pub fn new() -> Self {
        Self { device_id: None }
    }

    /// Handle volume request
    async fn handle_volume_request(&self, packet: &Packet) -> Result<()> {
        let request: SystemVolumeRequest = serde_json::from_value(packet.body.clone())
            .map_err(|e| crate::ProtocolError::InvalidPacket(format!("Failed to parse volume request: {}", e)))?;

        if request.request_sinks {
            info!("Remote device requested audio sink list");
            // TODO: Respond with list of local PulseAudio/PipeWire sinks
        }

        if let Some(volume) = request.volume {
            info!("Requested volume change to {}% for sink {:?}", volume, request.name);
            // TODO: Implement volume control via pactl or PulseAudio API
        }

        if let Some(muted) = request.muted {
            info!("Requested mute status change to {} for sink {:?}", muted, request.name);
            // TODO: Implement mute control
        }

        Ok(())
    }
}

impl Default for SystemVolumePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for SystemVolumePlugin {
    fn name(&self) -> &str {
        "systemvolume"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_SYSTEMVOLUME_REQUEST.to_string(),
            "kdeconnect.systemvolume".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![]
    }

    async fn init(&mut self, device: &Device, _packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("SystemVolume plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, _device: &mut Device) -> Result<()> {
        if packet.is_type(PACKET_TYPE_SYSTEMVOLUME_REQUEST) {
            self.handle_volume_request(packet).await
        } else {
            Ok(())
        }
    }
}

/// Factory for creating SystemVolumePlugin instances
pub struct SystemVolumePluginFactory;

impl PluginFactory for SystemVolumePluginFactory {
    fn name(&self) -> &str {
        "systemvolume"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_SYSTEMVOLUME_REQUEST.to_string(),
            "kdeconnect.systemvolume".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(SystemVolumePlugin::new())
    }
}
