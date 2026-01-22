//! Integration tests for NetworkShare plugin

use anyhow::Result;
use cosmic_connect_protocol::plugins::{networkshare, Plugin};
use cosmic_connect_protocol::{Device, DeviceInfo, DeviceType, Packet};
use serde_json::json;

/// Mock device for testing
fn create_mock_device() -> Device {
    Device {
        info: DeviceInfo::new("Test Device", DeviceType::Phone, 1716),
        connection_state: cosmic_connect_protocol::ConnectionState::Connected,
        pairing_status: cosmic_connect_protocol::PairingStatus::Paired,
        is_trusted: true,
        last_seen: 0,
        last_connected: Some(0),
        host: Some("127.0.0.1".to_string()),
        port: Some(1716),
        certificate_fingerprint: None,
        certificate_data: None,
    }
}

#[tokio::test]
async fn test_networkshare_plugin_initialization() -> Result<()> {
    let mut plugin = networkshare::NetworkSharePlugin::new();
    let device = create_mock_device();

    let (tx, _rx) = tokio::sync::mpsc::channel(100);
    plugin.init(&device, tx).await?;
    assert_eq!(plugin.name(), "networkshare");

    Ok(())
}

#[tokio::test]
async fn test_networkshare_packet_handling() -> Result<()> {
    let mut plugin = networkshare::NetworkSharePlugin::new();
    let mut device = create_mock_device();

    let (tx, _rx) = tokio::sync::mpsc::channel(100);
    plugin.init(&device, tx).await?;

    let packet = Packet::new(
        "kdeconnect.sftp",
        json!({
            "ip": "192.168.1.10",
            "port": 1739,
            "user": "kdeconnect",
            "password": "password"
        }),
    );

    // Should handle without error
    plugin.handle_packet(&packet, &mut device).await?;

    Ok(())
}
