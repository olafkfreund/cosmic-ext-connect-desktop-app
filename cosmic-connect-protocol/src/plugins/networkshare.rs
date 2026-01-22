//! Network Share Plugin (SFTP)
//!
//! Allows mounting remote device filesystems via SFTP/SSHFS.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `kdeconnect.sftp` - SFTP connection details (incoming)
//!
//! **Capabilities**:
//! - Incoming: `kdeconnect.sftp` - Receive SFTP connection info
//!
//! ## Packet Format
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "kdeconnect.sftp",
//!     "body": {
//!         "ip": "192.168.1.10",
//!         "port": 1739,
//!         "user": "kdeconnect",
//!         "password": "generated_password"
//!     }
//! }
//! ```
//!
//! ## Behavior
//!
//! When this packet is received, the desktop client should mount the remote filesystem
//! using sshfs.
//!
//! `sshfs -p <port> <user>@<ip>:/ <mountpoint> -o password_stdin`
//!
//! ## References
//!
//! - [KDE Connect SFTP Plugin](https://invent.kde.org/network/kdeconnect-kde/-/tree/master/plugins/sftp)

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use tracing::{debug, info, warn};

use super::{Plugin, PluginFactory};

/// Packet type for SFTP connection info
pub const PACKET_TYPE_SFTP: &str = "kdeconnect.sftp";
pub const PACKET_TYPE_CCONNECT_SFTP: &str = "cconnect.sftp";

/// SFTP connection details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpInfo {
    /// IP address of the SFTP server
    pub ip: String,
    /// Port number (optional, default 1739?)
    pub port: Option<u16>,
    /// Username
    pub user: String,
    /// Password
    pub password: String,
    /// Path to mount (optional)
    pub path: Option<String>,
}

/// Network Share plugin for SFTP mounting
pub struct NetworkSharePlugin {
    device_id: Option<String>,
    device_name: Option<String>,
}

impl NetworkSharePlugin {
    /// Create a new Network Share plugin
    pub fn new() -> Self {
        Self {
            device_id: None,
            device_name: None,
        }
    }

    /// Handle SFTP packet
    async fn handle_sftp_packet(&self, packet: &Packet) -> Result<()> {
        let info: SftpInfo = serde_json::from_value(packet.body.clone())
            .map_err(|e| crate::ProtocolError::InvalidPacket(format!("Failed to parse SFTP info: {}", e)))?;

        info!(
            "Received SFTP connection info from {}: {}@{}:{}{}",
            self.device_name.as_deref().unwrap_or("unknown"),
            info.user,
            info.ip,
            info.port.unwrap_or(22), // Default SSH port is 22, but KDE Connect uses custom ports often
            info.path.as_deref().unwrap_or("/")
        );

        // TODO: Emit an event or call a handler to perform the actual mount
        // The protocol library shouldn't probably execute sshfs directly, 
        // but for now we just log the intent.
        // In a full implementation, this would trigger a system mount.
        
        debug!("Ready to mount SFTP share");

        Ok(())
    }
}

impl Default for NetworkSharePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for NetworkSharePlugin {
    fn name(&self) -> &str {
        "networkshare" // Or "sftp" to match KDE Connect convention internally? Let's use networkshare as the plugin name but handle sftp packets.
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_SFTP.to_string(),
            PACKET_TYPE_CCONNECT_SFTP.to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![]
    }

    async fn init(&mut self, device: &Device, _packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        self.device_name = Some(device.name().to_string());
        info!("NetworkShare plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("NetworkShare plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("NetworkShare plugin stopped");
        // TODO: Unmount if mounted?
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, _device: &mut Device) -> Result<()> {
        if packet.is_type("kdeconnect.sftp") {
            self.handle_sftp_packet(packet).await
        } else {
            Ok(())
        }
    }
}

/// Factory for creating NetworkSharePlugin instances
#[derive(Debug, Clone, Copy)]
pub struct NetworkSharePluginFactory;

impl PluginFactory for NetworkSharePluginFactory {
    fn name(&self) -> &str {
        "networkshare"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_SFTP.to_string(),
            PACKET_TYPE_CCONNECT_SFTP.to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(NetworkSharePlugin::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceInfo, DeviceType};
    use serde_json::json;

    fn create_test_device() -> Device {
        let info = DeviceInfo::new("Test Device", DeviceType::Phone, 1716);
        Device::from_discovery(info)
    }

    #[test]
    fn test_plugin_creation() {
        let plugin = NetworkSharePlugin::new();
        assert_eq!(plugin.name(), "networkshare");
    }

    #[test]
    fn test_factory() {
        let factory = NetworkSharePluginFactory;
        assert_eq!(factory.name(), "networkshare");
        assert!(factory.incoming_capabilities().contains(&PACKET_TYPE_SFTP.to_string()));
    }

    #[tokio::test]
    async fn test_handle_sftp_packet() {
        let mut plugin = NetworkSharePlugin::new();
        let device = create_test_device();
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();

        let packet = Packet::new(
            PACKET_TYPE_SFTP,
            json!({
                "ip": "192.168.1.50",
                "port": 1739,
                "user": "testuser",
                "password": "secretpassword",
                "path": "/storage/emulated/0"
            })
        );

        let mut device_mut = device;
        let result = plugin.handle_packet(&packet, &mut device_mut).await;
        assert!(result.is_ok());
    }
}
