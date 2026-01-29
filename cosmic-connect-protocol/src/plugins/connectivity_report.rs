//! Connectivity Report Plugin
//!
//! Receives network connectivity status and signal strength from mobile devices.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `cconnect.connectivity_report` - Status report (incoming)
//!
//! **Capabilities**:
//! - Incoming: `cconnect.connectivity_report`
//!
//! ## Packet Format
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.connectivity_report",
//!     "body": {
//!         "signalStrengths": {
//!             "0": {
//!                 "networkType": "4G",
//!                 "signalStrength": 3
//!             }
//!         }
//!     }
//! }
//! ```

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use tracing::info;

use super::{Plugin, PluginFactory};

/// Packet type for connectivity reports
pub const PACKET_TYPE_CONNECTIVITY_REPORT: &str = "cconnect.connectivity_report";

/// Signal strength info for a single subscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalInfo {
    #[serde(rename = "networkType")]
    pub network_type: String,
    #[serde(rename = "signalStrength")]
    pub signal_strength: i32,
}

/// Connectivity report body
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectivityReport {
    #[serde(rename = "signalStrengths")]
    pub signal_strengths: HashMap<String, SignalInfo>,
}

/// Connectivity Report plugin
pub struct ConnectivityReportPlugin {
    device_id: Option<String>,
}

impl ConnectivityReportPlugin {
    /// Create a new Connectivity Report plugin
    pub fn new() -> Self {
        Self { device_id: None }
    }

    /// Handle connectivity report
    async fn handle_report(&self, packet: &Packet) -> Result<()> {
        let report: ConnectivityReport = serde_json::from_value(packet.body.clone())
            .map_err(|e| crate::ProtocolError::InvalidPacket(format!("Failed to parse report: {}", e)))?;

        for (id, info) in report.signal_strengths {
            info!("Connectivity update for sub {}: {} (signal: {}/4)", id, info.network_type, info.signal_strength);
        }

        Ok(())
    }
}

impl Default for ConnectivityReportPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ConnectivityReportPlugin {
    fn name(&self) -> &str {
        "connectivity_report"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_CONNECTIVITY_REPORT.to_string(),
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
        info!("ConnectivityReport plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, _device: &mut Device) -> Result<()> {
        if packet.is_type(PACKET_TYPE_CONNECTIVITY_REPORT) {
            self.handle_report(packet).await
        } else {
            Ok(())
        }
    }
}

/// Factory for creating ConnectivityReportPlugin instances
pub struct ConnectivityReportPluginFactory;

impl PluginFactory for ConnectivityReportPluginFactory {
    fn name(&self) -> &str {
        "connectivity_report"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_CONNECTIVITY_REPORT.to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(ConnectivityReportPlugin::new())
    }
}
