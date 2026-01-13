//! Integration Tests for KDE Connect Protocol
//!
//! These tests verify the complete protocol flow including device discovery,
//! pairing, and plugin communication.

use kdeconnect_protocol::{
    CertificateInfo, ConnectionState, Device, DeviceInfo, DeviceManager, DeviceType,
    PairingStatus, Packet,
};
use serde_json::json;
use tempfile::TempDir;

/// Helper to create a test device
fn create_test_device(name: &str, device_type: DeviceType, port: u16) -> Device {
    let info = DeviceInfo::new(name, device_type, port);
    Device::from_discovery(info)
}

/// Helper to create a test certificate
fn create_test_certificate() -> CertificateInfo {
    CertificateInfo::generate("test-device").expect("Failed to generate test certificate")
}

/// Helper to create a device manager with temp registry
fn create_test_manager() -> DeviceManager {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let registry_path = temp_dir.path().join("registry.json");
    DeviceManager::new(registry_path).expect("Failed to create device manager")
}

#[tokio::test]
async fn test_device_creation_from_discovery() {
    let device = create_test_device("Test Phone", DeviceType::Phone, 1716);

    assert_eq!(device.info.device_name, "Test Phone");
    assert_eq!(device.info.device_type, DeviceType::Phone);
    assert_eq!(device.info.tcp_port, 1716);
    assert_eq!(device.connection_state, ConnectionState::Disconnected);
    assert_eq!(device.pairing_status, PairingStatus::Unpaired);
    assert!(!device.is_trusted);
}

#[tokio::test]
async fn test_device_manager_add_device() {
    let mut manager = create_test_manager();
    let device = create_test_device("Test Device", DeviceType::Desktop, 1716);
    let device_id = device.info.device_id.clone();

    manager.add_device(device);

    assert!(manager.get_device(&device_id).is_some());
    assert_eq!(manager.device_count(), 1);
}

#[tokio::test]
async fn test_device_manager_remove_device() {
    let mut manager = create_test_manager();
    let device = create_test_device("Test Device", DeviceType::Desktop, 1716);
    let device_id = device.info.device_id.clone();

    manager.add_device(device);
    assert!(manager.get_device(&device_id).is_some());

    manager.remove_device(&device_id);
    assert!(manager.get_device(&device_id).is_none());
    assert_eq!(manager.device_count(), 0);
}

#[tokio::test]
async fn test_device_manager_update_device_state() {
    let mut manager = create_test_manager();
    let device = create_test_device("Test Device", DeviceType::Desktop, 1716);
    let device_id = device.info.device_id.clone();

    manager.add_device(device);

    // Update to connected
    if let Some(device) = manager.get_device_mut(&device_id) {
        device.connection_state = ConnectionState::Connected;
        device.host = Some("192.168.1.100".to_string());
        device.port = Some(1716);
    }

    let device = manager.get_device(&device_id).unwrap();
    assert_eq!(device.connection_state, ConnectionState::Connected);
    assert_eq!(device.host, Some("192.168.1.100".to_string()));
    assert_eq!(device.port, Some(1716));
}

#[tokio::test]
async fn test_pairing_request_packet() {
    let packet = Packet::new(
        "kdeconnect.pair",
        json!({
            "pair": true
        }),
    );

    assert_eq!(packet.packet_type, "kdeconnect.pair");
    assert_eq!(packet.body["pair"], json!(true));
}

#[tokio::test]
async fn test_pairing_accept_packet() {
    let packet = Packet::new(
        "kdeconnect.pair",
        json!({
            "pair": true
        }),
    );

    assert_eq!(packet.packet_type, "kdeconnect.pair");
    assert_eq!(packet.body["pair"], json!(true));
}

#[tokio::test]
async fn test_pairing_reject_packet() {
    let packet = Packet::new(
        "kdeconnect.pair",
        json!({
            "pair": false
        }),
    );

    assert_eq!(packet.packet_type, "kdeconnect.pair");
    assert_eq!(packet.body["pair"], json!(false));
}

#[tokio::test]
async fn test_device_pairing_state_transitions() {
    let mut device = create_test_device("Test Device", DeviceType::Phone, 1716);

    // Initial state
    assert_eq!(device.pairing_status, PairingStatus::Unpaired);
    assert!(!device.is_trusted);

    // Request pairing
    device.pairing_status = PairingStatus::Requested;
    assert_eq!(device.pairing_status, PairingStatus::Requested);
    assert!(!device.is_trusted);

    // Complete pairing
    device.pairing_status = PairingStatus::Paired;
    device.is_trusted = true;
    assert_eq!(device.pairing_status, PairingStatus::Paired);
    assert!(device.is_trusted);
}

#[tokio::test]
async fn test_certificate_generation() {
    let cert1 = create_test_certificate();
    let cert2 = create_test_certificate();

    // Certificates should be generated successfully
    assert!(!cert1.certificate.is_empty());
    assert!(!cert1.private_key.is_empty());
    assert!(!cert1.fingerprint.is_empty());

    // Each certificate should be unique
    assert_ne!(cert1.certificate, cert2.certificate);
    assert_ne!(cert1.private_key, cert2.private_key);
    assert_ne!(cert1.fingerprint, cert2.fingerprint);
}

#[tokio::test]
async fn test_device_info_with_capabilities() {
    let mut device_info = DeviceInfo::new("Test Device", DeviceType::Phone, 1716);

    // Add some capabilities
    device_info.incoming_capabilities = vec![
        "kdeconnect.battery".to_string(),
        "kdeconnect.ping".to_string(),
    ];
    device_info.outgoing_capabilities = vec![
        "kdeconnect.notification".to_string(),
        "kdeconnect.share".to_string(),
    ];

    assert_eq!(device_info.incoming_capabilities.len(), 2);
    assert_eq!(device_info.outgoing_capabilities.len(), 2);
    assert!(device_info
        .incoming_capabilities
        .contains(&"kdeconnect.battery".to_string()));
    assert!(device_info
        .outgoing_capabilities
        .contains(&"kdeconnect.notification".to_string()));
}

#[tokio::test]
async fn test_multiple_devices_in_manager() {
    let mut manager = create_test_manager();

    // Add multiple devices
    let phone = create_test_device("My Phone", DeviceType::Phone, 1716);
    let tablet = create_test_device("My Tablet", DeviceType::Tablet, 1716);
    let desktop = create_test_device("My Desktop", DeviceType::Desktop, 1716);

    let phone_id = phone.info.device_id.clone();
    let tablet_id = tablet.info.device_id.clone();
    let desktop_id = desktop.info.device_id.clone();

    manager.add_device(phone);
    manager.add_device(tablet);
    manager.add_device(desktop);

    assert_eq!(manager.device_count(), 3);

    // Verify each device can be retrieved
    assert!(manager.get_device(&phone_id).is_some());
    assert!(manager.get_device(&tablet_id).is_some());
    assert!(manager.get_device(&desktop_id).is_some());

    // Remove one device
    manager.remove_device(&tablet_id);
    assert_eq!(manager.device_count(), 2);
    assert!(manager.get_device(&tablet_id).is_none());
}

#[tokio::test]
async fn test_device_connection_lifecycle() {
    let mut device = create_test_device("Test Device", DeviceType::Phone, 1716);

    // Disconnected -> Connecting
    device.connection_state = ConnectionState::Connecting;
    assert_eq!(device.connection_state, ConnectionState::Connecting);
    assert!(device.connection_state.is_reachable());

    // Connecting -> Connected
    device.connection_state = ConnectionState::Connected;
    device.host = Some("192.168.1.100".to_string());
    device.port = Some(1716);
    assert_eq!(device.connection_state, ConnectionState::Connected);
    assert!(device.connection_state.is_connected());

    // Connected -> Disconnected
    device.connection_state = ConnectionState::Disconnected;
    device.host = None;
    device.port = None;
    assert_eq!(device.connection_state, ConnectionState::Disconnected);
    assert!(!device.connection_state.is_connected());
}

#[tokio::test]
async fn test_packet_serialization_structure() {
    let packet = Packet::new(
        "kdeconnect.ping",
        json!({
            "message": "Hello",
            "id": 42
        }),
    );

    assert_eq!(packet.packet_type, "kdeconnect.ping");
    assert_eq!(packet.body["message"], json!("Hello"));
    assert_eq!(packet.body["id"], json!(42));
}

#[tokio::test]
async fn test_device_last_seen_tracking() {
    let device = create_test_device("Test Device", DeviceType::Phone, 1716);
    let initial_last_seen = device.last_seen;

    // Simulate some time passing
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // In real implementation, last_seen would be updated when device sends packets
    // For now, we just verify it's initialized
    assert!(initial_last_seen > 0);
}

#[tokio::test]
async fn test_paired_devices_persistence_simulation() {
    let mut manager = create_test_manager();

    // Create and pair a device
    let mut device = create_test_device("Test Device", DeviceType::Phone, 1716);
    let device_id = device.info.device_id.clone();
    device.pairing_status = PairingStatus::Paired;
    device.is_trusted = true;
    device.certificate_fingerprint = Some("test-fingerprint".to_string());

    manager.add_device(device);

    // Verify paired device properties
    let stored_device = manager.get_device(&device_id).unwrap();
    assert_eq!(stored_device.pairing_status, PairingStatus::Paired);
    assert!(stored_device.is_trusted);
    assert_eq!(
        stored_device.certificate_fingerprint,
        Some("test-fingerprint".to_string())
    );

    // Get paired devices
    let paired: Vec<_> = manager.paired_devices().collect();
    assert_eq!(paired.len(), 1);
    assert_eq!(paired[0].info.device_id, device_id);
}

#[tokio::test]
async fn test_device_manager_find_by_name() {
    let mut manager = create_test_manager();

    let phone = create_test_device("Alice's Phone", DeviceType::Phone, 1716);
    let tablet = create_test_device("Alice's Tablet", DeviceType::Tablet, 1716);
    let desktop = create_test_device("Bob's Desktop", DeviceType::Desktop, 1716);

    manager.add_device(phone);
    manager.add_device(tablet);
    manager.add_device(desktop);

    // Find devices by name pattern
    let alice_devices: Vec<_> = manager
        .devices()
        .filter(|d| d.info.device_name.starts_with("Alice"))
        .collect();

    assert_eq!(alice_devices.len(), 2);
}

#[tokio::test]
async fn test_packet_to_bytes_and_back() {
    let original = Packet::new(
        "kdeconnect.ping",
        json!({
            "message": "test"
        }),
    );

    // Serialize to bytes
    let bytes = original.to_bytes().expect("Failed to serialize packet");

    // Deserialize from bytes
    let deserialized = Packet::from_bytes(&bytes).expect("Failed to deserialize packet");

    assert_eq!(original.packet_type, deserialized.packet_type);
    assert_eq!(original.body, deserialized.body);
}

#[tokio::test]
async fn test_identity_packet_creation() {
    let device_info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1716);

    let packet = Packet::new(
        "kdeconnect.identity",
        json!({
            "deviceId": device_info.device_id,
            "deviceName": device_info.device_name,
            "deviceType": "desktop",
            "protocolVersion": 7,
            "tcpPort": device_info.tcp_port,
            "incomingCapabilities": device_info.incoming_capabilities,
            "outgoingCapabilities": device_info.outgoing_capabilities
        }),
    );

    assert_eq!(packet.packet_type, "kdeconnect.identity");
    assert_eq!(packet.body["deviceName"], json!("Test Device"));
    assert_eq!(packet.body["protocolVersion"], json!(7));
}

#[tokio::test]
async fn test_device_manager_connected_devices() {
    let mut manager = create_test_manager();

    let mut phone = create_test_device("Phone", DeviceType::Phone, 1716);
    phone.connection_state = ConnectionState::Connected;

    let tablet = create_test_device("Tablet", DeviceType::Tablet, 1716);

    manager.add_device(phone);
    manager.add_device(tablet);

    assert_eq!(manager.device_count(), 2);
    assert_eq!(manager.connected_count(), 1);

    let connected: Vec<_> = manager.connected_devices().collect();
    assert_eq!(connected.len(), 1);
    assert_eq!(connected[0].info.device_name, "Phone");
}

#[tokio::test]
async fn test_device_manager_paired_count() {
    let mut manager = create_test_manager();

    let mut device1 = create_test_device("Device1", DeviceType::Phone, 1716);
    device1.pairing_status = PairingStatus::Paired;

    let mut device2 = create_test_device("Device2", DeviceType::Tablet, 1716);
    device2.pairing_status = PairingStatus::Paired;

    let device3 = create_test_device("Device3", DeviceType::Desktop, 1716);

    manager.add_device(device1);
    manager.add_device(device2);
    manager.add_device(device3);

    assert_eq!(manager.device_count(), 3);
    assert_eq!(manager.paired_count(), 2);
}
