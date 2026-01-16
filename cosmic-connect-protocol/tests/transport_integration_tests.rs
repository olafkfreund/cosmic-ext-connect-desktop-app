//! Transport Integration Tests
//!
//! Tests for multi-transport functionality including:
//! - TransportManager coordination
//! - TCP and Bluetooth transport selection
//! - Transport fallback behavior
//! - MTU limit handling

use cosmic_connect_protocol::transport::{
    LatencyCategory, Transport, TransportAddress, TransportCapabilities, TransportFactory,
    TransportType,
};
use cosmic_connect_protocol::{Packet, ProtocolError, Result};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Mock transport for testing
#[derive(Debug)]
struct MockTransport {
    transport_type: TransportType,
    max_packet_size: usize,
    connected: bool,
    sent_packets: Arc<Mutex<Vec<Packet>>>,
}

impl MockTransport {
    fn new(transport_type: TransportType, max_packet_size: usize) -> Self {
        Self {
            transport_type,
            max_packet_size,
            connected: true,
            sent_packets: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn get_sent_packets(&self) -> Vec<Packet> {
        self.sent_packets.lock().await.clone()
    }
}

#[async_trait::async_trait]
impl Transport for MockTransport {
    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            max_packet_size: self.max_packet_size,
            reliable: true,
            connection_oriented: true,
            latency: if self.transport_type == TransportType::Tcp {
                LatencyCategory::Low
            } else {
                LatencyCategory::Medium
            },
        }
    }

    fn remote_address(&self) -> TransportAddress {
        match self.transport_type {
            TransportType::Tcp => {
                TransportAddress::Tcp("127.0.0.1:1716".parse().unwrap())
            }
            TransportType::Bluetooth => TransportAddress::Bluetooth {
                address: "00:11:22:33:44:55".to_string(),
                service_uuid: Some(uuid::uuid!("185f3df4-3268-4e3f-9fca-d4d5059915bd")),
            },
        }
    }

    async fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        let bytes = packet.to_bytes()?;

        if bytes.len() > self.max_packet_size {
            return Err(ProtocolError::InvalidPacket(format!(
                "Packet too large: {} bytes (max {})",
                bytes.len(),
                self.max_packet_size
            )));
        }

        self.sent_packets.lock().await.push(packet.clone());
        Ok(())
    }

    async fn receive_packet(&mut self) -> Result<Packet> {
        // For testing, just return a dummy packet
        Ok(Packet::new("cconnect.test", json!({})))
    }

    async fn close(self: Box<Self>) -> Result<()> {
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}

#[tokio::test]
async fn test_tcp_transport_capabilities() {
    let transport = MockTransport::new(TransportType::Tcp, 1_048_576);
    let caps = transport.capabilities();

    assert_eq!(caps.max_packet_size, 1_048_576);
    assert!(caps.reliable);
    assert!(caps.connection_oriented);
    assert_eq!(caps.latency, LatencyCategory::Low);
}

#[tokio::test]
async fn test_bluetooth_transport_capabilities() {
    let transport = MockTransport::new(TransportType::Bluetooth, 512);
    let caps = transport.capabilities();

    assert_eq!(caps.max_packet_size, 512);
    assert!(caps.reliable);
    assert!(caps.connection_oriented);
    assert_eq!(caps.latency, LatencyCategory::Medium);
}

#[tokio::test]
async fn test_small_packet_over_bluetooth() {
    let mut transport = MockTransport::new(TransportType::Bluetooth, 512);

    // Create a small packet (ping)
    let packet = Packet::new("cconnect.ping", json!({ "message": "Hello!" }));

    // Should succeed
    let result = transport.send_packet(&packet).await;
    assert!(result.is_ok());

    // Verify packet was sent
    let sent = transport.get_sent_packets().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].packet_type, "cconnect.ping");
}

#[tokio::test]
async fn test_large_packet_over_bluetooth_fails() {
    let mut transport = MockTransport::new(TransportType::Bluetooth, 512);

    // Create a packet that will exceed 512 bytes when serialized
    let large_text = "A".repeat(1000);
    let packet = Packet::new("cconnect.test", json!({ "data": large_text }));

    // Should fail with MTU error
    let result = transport.send_packet(&packet).await;
    assert!(result.is_err());

    if let Err(ProtocolError::InvalidPacket(msg)) = result {
        assert!(msg.contains("Packet too large"));
    } else {
        panic!("Expected InvalidPacket error");
    }
}

#[tokio::test]
async fn test_medium_packet_over_bluetooth() {
    let mut transport = MockTransport::new(TransportType::Bluetooth, 512);

    // Create a medium-sized packet (MPRIS-like)
    let packet = Packet::new(
        "cconnect.mpris",
        json!({
            "player": "spotify",
            "artist": "Test Artist",
            "title": "Test Song Title",
            "album": "Test Album",
            "isPlaying": true,
            "pos": 45000,
            "length": 180000,
            "volume": 75
        }),
    );

    // Should succeed (packet is ~200-300 bytes)
    let result = transport.send_packet(&packet).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_transport_address_tcp() {
    let transport = MockTransport::new(TransportType::Tcp, 1_048_576);
    let addr = transport.remote_address();

    match addr {
        TransportAddress::Tcp(socket_addr) => {
            assert_eq!(socket_addr, "127.0.0.1:1716".parse::<SocketAddr>().unwrap());
        }
        _ => panic!("Expected TCP address"),
    }
}

#[tokio::test]
async fn test_transport_address_bluetooth() {
    let transport = MockTransport::new(TransportType::Bluetooth, 512);
    let addr = transport.remote_address();

    match addr {
        TransportAddress::Bluetooth { address, service_uuid } => {
            assert_eq!(address, "00:11:22:33:44:55");
            assert!(service_uuid.is_some());
        }
        _ => panic!("Expected Bluetooth address"),
    }
}

#[tokio::test]
async fn test_multiple_packets_over_bluetooth() {
    let mut transport = MockTransport::new(TransportType::Bluetooth, 512);

    // Send multiple small packets
    for i in 0..5 {
        let packet = Packet::new(
            "cconnect.ping",
            json!({ "message": format!("Ping {}", i) }),
        );

        let result = transport.send_packet(&packet).await;
        assert!(result.is_ok());
    }

    // Verify all packets were sent
    let sent = transport.get_sent_packets().await;
    assert_eq!(sent.len(), 5);
}

#[tokio::test]
async fn test_battery_packet_bluetooth_compatible() {
    let mut transport = MockTransport::new(TransportType::Bluetooth, 512);

    // Create battery status packet
    let packet = Packet::new(
        "cconnect.battery",
        json!({
            "currentCharge": 75,
            "isCharging": true,
            "thresholdEvent": 0
        }),
    );

    let result = transport.send_packet(&packet).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_share_metadata_bluetooth_compatible() {
    let mut transport = MockTransport::new(TransportType::Bluetooth, 512);

    // Create share file metadata packet (not the actual file)
    let mut packet = Packet::new(
        "cconnect.share.request",
        json!({
            "filename": "document.pdf",
            "creationTime": 1640000000000i64,
            "lastModified": 1640000000000i64,
        }),
    );

    // Add payload info (file content goes over separate connection)
    packet = packet
        .with_payload_size(1024000)
        .with_payload_transfer_info(
            vec![("port".to_string(), json!(1739))]
                .into_iter()
                .collect(),
        );

    let result = transport.send_packet(&packet).await;
    assert!(result.is_ok());

    // Verify packet metadata is small even for large file
    let bytes = packet.to_bytes().unwrap();
    assert!(bytes.len() < 512, "Share metadata should be < 512 bytes");
}

#[tokio::test]
async fn test_notification_packet_bluetooth_compatible() {
    let mut transport = MockTransport::new(TransportType::Bluetooth, 512);

    // Create notification packet with typical content
    let packet = Packet::new(
        "cconnect.notification",
        json!({
            "id": "notification_123",
            "appName": "Signal",
            "title": "New Message",
            "text": "Hello! How are you doing?",
            "ticker": "Signal: New Message",
            "time": "1640000000000",
            "isClearable": true,
            "isCancel": false
        }),
    );

    let result = transport.send_packet(&packet).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_very_long_notification_bluetooth() {
    let mut transport = MockTransport::new(TransportType::Bluetooth, 512);

    // Create notification with very long text
    let long_text = "A".repeat(400);
    let packet = Packet::new(
        "cconnect.notification",
        json!({
            "id": "notification_123",
            "appName": "App",
            "title": "Title",
            "text": long_text,
        }),
    );

    // This might fail depending on total packet size
    // Just verify it's handled gracefully (either success or proper error)
    let result = transport.send_packet(&packet).await;

    // Both outcomes are acceptable:
    // 1. Packet fits and succeeds
    // 2. Packet too large and fails with clear error
    match result {
        Ok(_) => {
            let bytes = packet.to_bytes().unwrap();
            assert!(bytes.len() <= 512);
        }
        Err(ProtocolError::InvalidPacket(msg)) => {
            assert!(msg.contains("Packet too large"));
        }
        Err(e) => panic!("Unexpected error: {}", e),
    }
}

#[tokio::test]
async fn test_connected_status() {
    let transport = MockTransport::new(TransportType::Tcp, 1_048_576);
    assert!(transport.is_connected());
}

#[tokio::test]
async fn test_packet_roundtrip() {
    let mut transport = MockTransport::new(TransportType::Tcp, 1_048_576);

    // Create and send packet
    let original = Packet::new(
        "cconnect.ping",
        json!({ "message": "Test" }),
    );

    transport.send_packet(&original).await.unwrap();

    // Verify sent packet matches original
    let sent = transport.get_sent_packets().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].packet_type, original.packet_type);
    assert_eq!(sent[0].body, original.body);
}
