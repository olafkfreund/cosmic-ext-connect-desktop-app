//! Wake-on-LAN (WOL) Plugin
//!
//! Enables waking up sleeping or hibernated remote desktop machines by sending
//! magic packets over the network.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `cconnect.wol.request` - Request to wake up a device
//! - `cconnect.wol.status` - Query device power status
//! - `cconnect.wol.config` - Configure MAC address for device
//!
//! **Capabilities**:
//! - Incoming: `cconnect.wol.request`, `cconnect.wol.config`
//! - Outgoing: `cconnect.wol.status`
//!
//! ## Packet Formats
//!
//! ### Wake Request
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.wol.request",
//!     "body": {
//!         "macAddress": "00:11:22:33:44:55"
//!     }
//! }
//! ```
//!
//! ### Configure MAC Address
//!
//! ```json
//! {
//!     "id": 1234567891,
//!     "type": "cconnect.wol.config",
//!     "body": {
//!         "macAddress": "00:11:22:33:44:55"
//!     }
//! }
//! ```
//!
//! ### Status Response
//!
//! ```json
//! {
//!     "id": 1234567892,
//!     "type": "cconnect.wol.status",
//!     "body": {
//!         "awake": true,
//!         "macAddress": "00:11:22:33:44:55"
//!     }
//! }
//! ```
//!
//! ## Wake-on-LAN Magic Packet
//!
//! The magic packet consists of:
//! - 6 bytes of 0xFF (sync stream)
//! - 16 repetitions of the target MAC address (6 bytes each)
//! - Total: 102 bytes
//!
//! The packet is sent via UDP to:
//! - Broadcast address: 255.255.255.255:9
//! - Standard WOL port: 9 (can also use port 7)
//!
//! ## Use Cases
//!
//! - Wake up a desktop machine from sleep/hibernate
//! - Remote power management for desktop-to-desktop connections
//! - Scheduled wake-up for maintenance tasks
//! - Energy-efficient remote desktop access
//!
//! ## Requirements
//!
//! For WOL to work, the target machine must:
//! - Support Wake-on-LAN in BIOS/UEFI
//! - Have WOL enabled in network adapter settings
//! - Be connected via Ethernet (most reliable)
//! - Be on the same subnet or have router WOL forwarding configured
//!
//! ## Platform Support
//!
//! - **Linux**: Full support
//! - **macOS**: Full support
//! - **Windows**: Full support
//!
//! UDP broadcast is available on all platforms.

use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use std::net::UdpSocket;
use tracing::{debug, info, warn};

use super::{Plugin, PluginFactory};

/// Standard WOL port (can also use port 7)
const WOL_PORT: u16 = 9;

/// Broadcast address for WOL packets
const BROADCAST_ADDR: &str = "255.255.255.255";

/// Wake-on-LAN plugin
///
/// Sends magic packets to wake up sleeping/hibernated machines.
#[derive(Debug)]
pub struct WolPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Stored MAC address for this device
    mac_address: Option<String>,

    /// Whether the plugin is enabled
    enabled: bool,
}

impl WolPlugin {
    /// Create a new WOL plugin
    pub fn new() -> Self {
        Self {
            device_id: None,
            mac_address: None,
            enabled: true,
        }
    }

    /// Get the stored MAC address
    pub fn get_mac_address(&self) -> Option<String> {
        self.mac_address.clone()
    }

    /// Set the MAC address (for loading from config)
    pub fn set_mac_address(&mut self, mac: String) {
        self.mac_address = Some(mac);
    }

    /// Parse MAC address from various formats
    ///
    /// Accepts formats like:
    /// - "00:11:22:33:44:55"
    /// - "00-11-22-33-44-55"
    /// - "001122334455"
    fn parse_mac_address(mac_str: &str) -> Result<[u8; 6]> {
        let cleaned = mac_str.replace([':', '-'], "");

        if cleaned.len() != 12 {
            return Err(ProtocolError::InvalidPacket(format!(
                "Invalid MAC address length: {}",
                mac_str
            )));
        }

        let mut mac = [0u8; 6];
        for (i, chunk) in cleaned.as_bytes().chunks(2).enumerate() {
            let hex_str = std::str::from_utf8(chunk).map_err(|_| {
                ProtocolError::InvalidPacket(format!("Invalid MAC address format: {}", mac_str))
            })?;

            mac[i] = u8::from_str_radix(hex_str, 16).map_err(|_| {
                ProtocolError::InvalidPacket(format!("Invalid MAC address hex: {}", mac_str))
            })?;
        }

        Ok(mac)
    }

    /// Format MAC address to standard format (XX:XX:XX:XX:XX:XX)
    fn format_mac_address(mac: &[u8; 6]) -> String {
        format!(
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        )
    }

    /// Create a WOL magic packet
    ///
    /// Format: 6 bytes of 0xFF + 16 repetitions of MAC address
    fn create_magic_packet(mac: &[u8; 6]) -> Vec<u8> {
        let mut packet = Vec::with_capacity(102);

        // Sync stream: 6 bytes of 0xFF
        packet.extend_from_slice(&[0xFF; 6]);

        // Repeat MAC address 16 times
        for _ in 0..16 {
            packet.extend_from_slice(mac);
        }

        packet
    }

    /// Send WOL magic packet to wake a device
    fn send_wol_packet(&self, mac: &[u8; 6]) -> Result<()> {
        let packet = Self::create_magic_packet(mac);
        let addr = format!("{}:{}", BROADCAST_ADDR, WOL_PORT);

        debug!(
            "Sending WOL magic packet to {} ({})",
            Self::format_mac_address(mac),
            addr
        );

        // Create UDP socket
        let socket = UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| ProtocolError::from_io_error(e, "Failed to create UDP socket"))?;

        // Enable broadcast
        socket
            .set_broadcast(true)
            .map_err(|e| ProtocolError::from_io_error(e, "Failed to enable broadcast"))?;

        // Send magic packet
        socket
            .send_to(&packet, &addr)
            .map_err(|e| ProtocolError::from_io_error(e, "Failed to send WOL packet"))?;

        info!(
            "WOL magic packet sent to {} successfully",
            Self::format_mac_address(mac)
        );

        Ok(())
    }

    /// Handle WOL request packet
    async fn handle_wol_request(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        debug!("Handling WOL request from {}", device.name());

        let body = &packet.body;

        // Get MAC address from packet or use stored MAC
        let mac_str = body
            .get("macAddress")
            .and_then(|v| v.as_str())
            .or(self.mac_address.as_deref());

        let Some(mac_str) = mac_str else {
            warn!(
                "WOL request from {} has no MAC address configured",
                device.name()
            );
            return Err(ProtocolError::InvalidPacket(
                "MAC address not provided and not configured".to_string(),
            ));
        };

        // Parse MAC address
        let mac = Self::parse_mac_address(mac_str)?;

        info!(
            "Received WOL request from {} to wake {}",
            device.name(),
            Self::format_mac_address(&mac)
        );

        // Send magic packet
        self.send_wol_packet(&mac)?;

        // TODO: Send status response back to device
        // let response = Packet::new(
        //     "cconnect.wol.status",
        //     json!({
        //         "packetSent": true,
        //         "macAddress": Self::format_mac_address(&mac),
        //     }),
        // );
        // device.send_packet(response).await?;

        Ok(())
    }

    /// Handle WOL config packet (store MAC address)
    async fn handle_wol_config(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        debug!("Handling WOL config from {}", device.name());

        let body = &packet.body;
        let mac_str = body
            .get("macAddress")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProtocolError::InvalidPacket("MAC address not provided in config".to_string())
            })?;

        // Validate MAC address
        let mac = Self::parse_mac_address(mac_str)?;
        let formatted = Self::format_mac_address(&mac);

        info!(
            "Storing MAC address {} for device {}",
            formatted,
            device.name()
        );

        self.mac_address = Some(formatted);

        // Note: Persistence is handled by the daemon via get_mac_address()
        // The daemon should call get_mac_address() after packet handling
        // and save it to DeviceConfig

        Ok(())
    }
}

impl Default for WolPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for WolPlugin {
    fn name(&self) -> &str {
        "wol"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.wol.request".to_string(),
            "cconnect.wol.config".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec!["cconnect.wol.status".to_string()]
    }

    async fn init(&mut self, device: &Device) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!("WOL plugin initialized for device {}", device.name());

        // Note: MAC address loading is handled by the daemon
        // The daemon should call set_mac_address() after plugin initialization
        // with the value from DeviceConfig

        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("WOL plugin started");
        self.enabled = true;
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("WOL plugin stopped");
        self.enabled = false;
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("WOL plugin is disabled, ignoring packet");
            return Ok(());
        }

        match packet.packet_type.as_str() {
            "cconnect.wol.request" => self.handle_wol_request(packet, device).await,
            "cconnect.wol.config" => self.handle_wol_config(packet, device).await,
            _ => {
                warn!("Unknown packet type: {}", packet.packet_type);
                Ok(())
            }
        }
    }
}

/// Factory for creating WolPlugin instances
#[derive(Debug, Clone, Copy)]
pub struct WolPluginFactory;

impl PluginFactory for WolPluginFactory {
    fn name(&self) -> &str {
        "wol"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.wol.request".to_string(),
            "cconnect.wol.config".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec!["cconnect.wol.status".to_string()]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(WolPlugin::new())
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
        let plugin = WolPlugin::new();
        assert_eq!(plugin.name(), "wol");
        assert!(plugin.enabled);
        assert!(plugin.mac_address.is_none());
    }

    #[test]
    fn test_capabilities() {
        let plugin = WolPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 2);
        assert!(incoming.contains(&"cconnect.wol.request".to_string()));
        assert!(incoming.contains(&"cconnect.wol.config".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 1);
        assert!(outgoing.contains(&"cconnect.wol.status".to_string()));
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = WolPlugin::new();
        let device = create_test_device();

        plugin.init(&device).await.unwrap();
        assert!(plugin.device_id.is_some());

        plugin.start().await.unwrap();
        assert!(plugin.enabled);

        plugin.stop().await.unwrap();
        assert!(!plugin.enabled);
    }

    #[test]
    fn test_parse_mac_address_colon_format() {
        let mac_str = "00:11:22:33:44:55";
        let mac = WolPlugin::parse_mac_address(mac_str).unwrap();
        assert_eq!(mac, [0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
    }

    #[test]
    fn test_parse_mac_address_dash_format() {
        let mac_str = "AA-BB-CC-DD-EE-FF";
        let mac = WolPlugin::parse_mac_address(mac_str).unwrap();
        assert_eq!(mac, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn test_parse_mac_address_no_separator() {
        let mac_str = "112233445566";
        let mac = WolPlugin::parse_mac_address(mac_str).unwrap();
        assert_eq!(mac, [0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
    }

    #[test]
    fn test_parse_mac_address_invalid_length() {
        let mac_str = "00:11:22:33:44";
        let result = WolPlugin::parse_mac_address(mac_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_mac_address_invalid_hex() {
        let mac_str = "00:11:22:33:44:ZZ";
        let result = WolPlugin::parse_mac_address(mac_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_format_mac_address() {
        let mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let formatted = WolPlugin::format_mac_address(&mac);
        assert_eq!(formatted, "AA:BB:CC:DD:EE:FF");
    }

    #[test]
    fn test_create_magic_packet() {
        let mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        let packet = WolPlugin::create_magic_packet(&mac);

        // Check packet length (6 sync bytes + 16 * 6 MAC bytes = 102)
        assert_eq!(packet.len(), 102);

        // Check sync stream (first 6 bytes should be 0xFF)
        assert_eq!(&packet[0..6], &[0xFF; 6]);

        // Check first MAC repetition
        assert_eq!(&packet[6..12], &mac);

        // Check last MAC repetition
        assert_eq!(&packet[96..102], &mac);

        // Verify all 16 repetitions
        for i in 0..16 {
            let offset = 6 + (i * 6);
            assert_eq!(&packet[offset..offset + 6], &mac);
        }
    }

    #[tokio::test]
    async fn test_handle_wol_config() {
        let mut plugin = WolPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();
        plugin.start().await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.wol.config",
            json!({
                "macAddress": "AA:BB:CC:DD:EE:FF"
            }),
        );

        let result = plugin.handle_packet(&packet, &mut device).await;
        assert!(result.is_ok());
        assert_eq!(plugin.mac_address, Some("AA:BB:CC:DD:EE:FF".to_string()));
    }

    #[test]
    fn test_factory() {
        let factory = WolPluginFactory;
        assert_eq!(factory.name(), "wol");

        let plugin = factory.create();
        assert_eq!(plugin.name(), "wol");
    }
}
