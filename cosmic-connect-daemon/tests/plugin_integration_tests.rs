//! Integration tests for plugin functionality via DBus
//!
//! Tests the integration between plugins and the DBus interface,
//! ensuring plugin actions work correctly end-to-end.

use anyhow::Result;
use cosmic_connect_protocol::plugins::{
    battery, clipboard, mpris, notification, ping, share, Plugin, PluginFactory,
};
use cosmic_connect_protocol::{Device, DeviceInfo, DeviceType, Packet};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

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
async fn test_battery_plugin_initialization() -> Result<()> {
    // Test that battery plugin can be created and initialized
    let mut plugin = battery::BatteryPlugin::new();
    let device = create_mock_device();

    plugin.init(&device).await?;
    assert_eq!(plugin.name(), "battery");

    Ok(())
}

#[tokio::test]
async fn test_battery_plugin_capabilities() {
    let plugin = battery::BatteryPlugin::new();

    let incoming = plugin.incoming_capabilities();
    assert!(incoming.contains(&"cconnect.battery".to_string()));
    assert!(incoming.contains(&"cconnect.battery.request".to_string()));

    let outgoing = plugin.outgoing_capabilities();
    assert!(outgoing.contains(&"cconnect.battery".to_string()));
    assert!(outgoing.contains(&"cconnect.battery.request".to_string()));
}

#[tokio::test]
async fn test_battery_status_query() {
    let mut plugin = battery::BatteryPlugin::new();
    let device = create_mock_device();

    plugin.init(&device).await.unwrap();

    // Initially no battery status
    assert!(plugin.get_battery_status().is_none());

    // Create a battery status for the packet
    let battery_status = battery::BatteryStatus {
        current_charge: 85,
        is_charging: true,
        threshold_event: 0,
    };

    // Simulate receiving battery packet
    let battery_packet = plugin.create_battery_packet(&battery_status);
    let mut device_mut = device;
    plugin
        .handle_packet(&battery_packet, &mut device_mut)
        .await
        .unwrap();

    // Now should have battery status
    let status = plugin.get_battery_status();
    assert!(status.is_some());
    let status = status.unwrap();
    assert_eq!(status.current_charge, 85);
    assert_eq!(status.is_charging, true);
}

#[tokio::test]
async fn test_notification_plugin_initialization() -> Result<()> {
    let mut plugin = notification::NotificationPlugin::new();
    let device = create_mock_device();

    plugin.init(&device).await?;
    assert_eq!(plugin.name(), "notification");
    assert_eq!(plugin.notification_count(), 0);

    Ok(())
}

#[tokio::test]
async fn test_notification_packet_creation() {
    let plugin = notification::NotificationPlugin::new();

    let notification =
        notification::Notification::new("test-123", "Test App", "Test Title", "Test Body", true);

    let packet = plugin.create_notification_packet(&notification);
    assert_eq!(packet.packet_type, "cconnect.notification");

    // Check packet body contains notification data
    let body = packet.body;
    assert_eq!(body["id"], "test-123");
    assert_eq!(body["appName"], "Test App");
    assert_eq!(body["title"], "Test Title");
    assert_eq!(body["text"], "Test Body");
}

#[tokio::test]
async fn test_ping_plugin_initialization() -> Result<()> {
    let mut plugin = ping::PingPlugin::new();
    let device = create_mock_device();

    plugin.init(&device).await?;
    assert_eq!(plugin.name(), "ping");

    Ok(())
}

#[tokio::test]
async fn test_ping_packet_creation() {
    let plugin = ping::PingPlugin::new();

    let packet = plugin.create_ping(Some("Hello!".to_string()));
    assert_eq!(packet.packet_type, "cconnect.ping");
    assert_eq!(packet.body["message"], "Hello!");

    let packet_no_message = plugin.create_ping(None);
    assert_eq!(packet_no_message.packet_type, "cconnect.ping");
}

#[tokio::test]
async fn test_plugin_trait_downcast() {
    use cosmic_connect_protocol::plugins::Plugin;
    use std::any::Any;

    // Test that we can downcast from trait object
    let plugin: Box<dyn Plugin> = Box::new(battery::BatteryPlugin::new());

    // Downcast to concrete type
    let battery_plugin = plugin.as_any().downcast_ref::<battery::BatteryPlugin>();
    assert!(battery_plugin.is_some());

    let battery_plugin = battery_plugin.unwrap();
    assert_eq!(battery_plugin.name(), "battery");
}

#[tokio::test]
async fn test_plugin_manager_battery_query() {
    use cosmic_connect_protocol::plugins::PluginManager;

    let mut manager = PluginManager::new();

    // Register battery plugin factory
    manager.register_factory(Arc::new(battery::BatteryPluginFactory));

    let device = create_mock_device();
    let device_id = device.info.device_id.clone();

    // Initialize plugins for device (they auto-start after init)
    manager
        .init_device_plugins(&device_id, &device)
        .await
        .unwrap();

    // Initially no battery status
    let status = manager.get_device_battery_status(&device_id);
    assert!(status.is_none());

    // TODO: Test receiving battery packet and querying status
    // This requires access to device packet handling which is not
    // exposed in the current plugin API
}

/// Test that plugins can be created via factories
#[tokio::test]
async fn test_plugin_factories() {
    let battery_factory = battery::BatteryPluginFactory;
    assert_eq!(battery_factory.name(), "battery");
    let plugin = battery_factory.create();
    assert_eq!(plugin.name(), "battery");

    let ping_factory = ping::PingPluginFactory;
    assert_eq!(ping_factory.name(), "ping");
    let plugin = ping_factory.create();
    assert_eq!(plugin.name(), "ping");

    let notification_factory = notification::NotificationPluginFactory;
    assert_eq!(notification_factory.name(), "notification");
    let plugin = notification_factory.create();
    assert_eq!(plugin.name(), "notification");
}

/// Test multiple plugins can coexist
#[tokio::test]
async fn test_multiple_plugins() -> Result<()> {
    use cosmic_connect_protocol::plugins::PluginManager;

    let mut manager = PluginManager::new();

    // Register multiple plugin factories
    manager.register_factory(Arc::new(battery::BatteryPluginFactory));
    manager.register_factory(Arc::new(ping::PingPluginFactory));
    manager.register_factory(Arc::new(notification::NotificationPluginFactory));

    let device = create_mock_device();
    let device_id = device.info.device_id.clone();

    // Initialize all plugins for device (they auto-start after init)
    manager.init_device_plugins(&device_id, &device).await?;

    // All should be working
    Ok(())
}

#[tokio::test]
async fn test_plugin_lifecycle() -> Result<()> {
    let mut plugin = battery::BatteryPlugin::new();
    let device = create_mock_device();

    // Test lifecycle: init -> start -> stop
    plugin.init(&device).await?;
    plugin.start().await?;
    plugin.stop().await?;

    Ok(())
}

// ============================================================================
// End-to-End Integration Tests
// ============================================================================

#[tokio::test]
async fn test_clipboard_sync_between_devices() -> Result<()> {
    // Test clipboard synchronization between two mock devices
    let mut plugin1 = clipboard::ClipboardPlugin::new();
    let mut plugin2 = clipboard::ClipboardPlugin::new();

    let device1 = create_mock_device();
    let mut device2 = create_mock_device();
    device2.info.device_id = "device2".to_string();

    plugin1.init(&device1).await?;
    plugin2.init(&device2).await?;

    // Device 1 sends clipboard content
    let test_content = "Hello from device 1!";
    let packet = plugin1.create_clipboard_packet(test_content.to_string()).await;
    assert_eq!(packet.packet_type, "cconnect.clipboard");
    assert_eq!(packet.body["content"], test_content);

    // Device 2 receives and processes the clipboard packet
    plugin2.handle_packet(&packet, &mut device2).await?;

    // Verify the clipboard was updated on device 2
    let received_content = plugin2.get_clipboard_content();
    assert_eq!(received_content, Some(test_content.to_string()));

    Ok(())
}

#[tokio::test]
async fn test_clipboard_connect_packet() -> Result<()> {
    let mut plugin = clipboard::ClipboardPlugin::new();
    let device = create_mock_device();

    plugin.init(&device).await?;

    // Test clipboard connect packet (sent on device connection)
    let connect_packet = plugin.create_clipboard_connect_packet().await;
    assert_eq!(connect_packet.packet_type, "cconnect.clipboard.connect");

    // Verify timestamp is present
    assert!(connect_packet.body.get("timestamp").is_some());

    Ok(())
}

#[tokio::test]
async fn test_share_plugin_text() -> Result<()> {
    let mut plugin = share::SharePlugin::new();
    let device = create_mock_device();

    plugin.init(&device).await?;

    // Test sharing text
    let test_text = "Shared text content";
    let packet = plugin.create_share_text_packet(test_text.to_string());
    assert_eq!(packet.packet_type, "cconnect.share.request");
    assert_eq!(packet.body["text"], test_text);

    // Handle incoming text share
    let mut device_mut = device.clone();
    plugin.handle_packet(&packet, &mut device_mut).await?;

    Ok(())
}

#[tokio::test]
async fn test_share_plugin_url() -> Result<()> {
    let plugin = share::SharePlugin::new();

    // Test sharing URL
    let test_url = "https://example.com";
    let packet = plugin.create_share_url_packet(test_url.to_string());
    assert_eq!(packet.packet_type, "cconnect.share.request");
    assert_eq!(packet.body["url"], test_url);

    Ok(())
}

#[tokio::test]
async fn test_share_plugin_file() -> Result<()> {
    let plugin = share::SharePlugin::new();

    // Test file share packet creation
    let filename = "test_file.txt";
    let filesize = 1024;
    let packet = plugin.create_share_file_packet(filename.to_string(), filesize);
    assert_eq!(packet.packet_type, "cconnect.share.request");

    // Verify packet contains file metadata
    let body = packet.body;
    assert_eq!(body["filename"], filename);
    assert!(body.get("numberOfFiles").is_some());
    assert!(body.get("totalPayloadSize").is_some());

    Ok(())
}

#[tokio::test]
async fn test_mpris_plugin_initialization() -> Result<()> {
    let mut plugin = mpris::MprisPlugin::new();
    let device = create_mock_device();

    plugin.init(&device).await?;
    assert_eq!(plugin.name(), "mpris");

    Ok(())
}

#[tokio::test]
async fn test_mpris_control_commands() -> Result<()> {
    let plugin = mpris::MprisPlugin::new();

    // Test play/pause command
    let play_packet = plugin.create_play_pause_command("test-player".to_string());
    assert_eq!(play_packet.packet_type, "cconnect.mpris.request");
    assert_eq!(play_packet.body["action"], "PlayPause");
    assert_eq!(play_packet.body["player"], "test-player");

    // Test next command
    let next_packet = plugin.create_next_command("test-player".to_string());
    assert_eq!(next_packet.packet_type, "cconnect.mpris.request");
    assert_eq!(next_packet.body["action"], "Next");

    // Test previous command
    let prev_packet = plugin.create_previous_command("test-player".to_string());
    assert_eq!(prev_packet.packet_type, "cconnect.mpris.request");
    assert_eq!(prev_packet.body["action"], "Previous");

    // Test stop command
    let stop_packet = plugin.create_stop_command("test-player".to_string());
    assert_eq!(stop_packet.packet_type, "cconnect.mpris.request");
    assert_eq!(stop_packet.body["action"], "Stop");

    Ok(())
}

#[tokio::test]
async fn test_mpris_player_list() -> Result<()> {
    let mut plugin = mpris::MprisPlugin::new();
    let device = create_mock_device();

    plugin.init(&device).await?;

    // Create a player list packet
    let players = vec!["spotify".to_string(), "vlc".to_string()];
    let packet = plugin.create_player_list_packet(players.clone());
    assert_eq!(packet.packet_type, "cconnect.mpris");
    assert_eq!(packet.body["playerList"], serde_json::to_value(players)?);

    Ok(())
}

#[tokio::test]
async fn test_complete_ping_exchange() -> Result<()> {
    // Simulate a complete ping request/response cycle
    let mut plugin1 = ping::PingPlugin::new();
    let mut plugin2 = ping::PingPlugin::new();

    let device1 = create_mock_device();
    let mut device2 = create_mock_device();
    device2.info.device_id = "device2".to_string();

    plugin1.init(&device1).await?;
    plugin2.init(&device2).await?;

    // Device 1 sends ping
    let message = Some("Test ping".to_string());
    let ping_packet = plugin1.create_ping(message.clone());

    // Device 2 receives ping
    plugin2.handle_packet(&ping_packet, &mut device2).await?;

    // Verify packet structure
    assert_eq!(ping_packet.packet_type, "cconnect.ping");
    assert_eq!(ping_packet.body["message"], "Test ping");

    Ok(())
}

#[tokio::test]
async fn test_battery_request_response_cycle() -> Result<()> {
    // Test complete battery request and response cycle
    let mut plugin = battery::BatteryPlugin::new();
    let mut device = create_mock_device();

    plugin.init(&device).await?;

    // Create battery request packet
    let request_packet = plugin.create_battery_request();
    assert_eq!(request_packet.packet_type, "cconnect.battery.request");
    assert_eq!(request_packet.body["request"], true);

    // Simulate receiving battery status response
    let battery_status = battery::BatteryStatus {
        current_charge: 75,
        is_charging: false,
        threshold_event: 0,
    };
    let response_packet = plugin.create_battery_packet(&battery_status);

    // Handle the response
    plugin
        .handle_packet(&response_packet, &mut device)
        .await?;

    // Verify battery status was updated
    let status = plugin.get_battery_status();
    assert!(status.is_some());
    let status = status.unwrap();
    assert_eq!(status.current_charge, 75);
    assert_eq!(status.is_charging, false);

    Ok(())
}

#[tokio::test]
async fn test_notification_send_and_dismiss() -> Result<()> {
    let mut plugin = notification::NotificationPlugin::new();
    let mut device = create_mock_device();

    plugin.init(&device).await?;

    // Create and send notification
    let notification = notification::Notification::new(
        "test-notif-123",
        "TestApp",
        "Test Title",
        "Test Body",
        true,
    );
    let packet = plugin.create_notification_packet(&notification);

    // Handle incoming notification
    plugin.handle_packet(&packet, &mut device).await?;

    // Verify notification count increased
    assert_eq!(plugin.notification_count(), 1);

    // Create dismiss packet
    let dismiss_packet = plugin.create_dismiss_packet("test-notif-123".to_string());
    assert_eq!(dismiss_packet.packet_type, "cconnect.notification.request");
    assert_eq!(dismiss_packet.body["cancel"], "test-notif-123");

    Ok(())
}

#[tokio::test]
async fn test_plugin_manager_multi_device() -> Result<()> {
    use cosmic_connect_protocol::plugins::PluginManager;

    let mut manager = PluginManager::new();

    // Register all plugin factories
    manager.register_factory(Arc::new(battery::BatteryPluginFactory));
    manager.register_factory(Arc::new(ping::PingPluginFactory));
    manager.register_factory(Arc::new(notification::NotificationPluginFactory));
    manager.register_factory(Arc::new(clipboard::ClipboardPluginFactory));
    manager.register_factory(Arc::new(share::SharePluginFactory));

    // Create two devices
    let device1 = create_mock_device();
    let mut device2 = create_mock_device();
    device2.info.device_id = "device2".to_string();

    let device1_id = device1.info.device_id.clone();
    let device2_id = device2.info.device_id.clone();

    // Initialize plugins for both devices
    manager.init_device_plugins(&device1_id, &device1).await?;
    manager.init_device_plugins(&device2_id, &device2).await?;

    // Verify both devices have plugins initialized
    assert!(manager.get_device_plugin(&device1_id, "battery").is_some());
    assert!(manager.get_device_plugin(&device2_id, "battery").is_some());
    assert!(manager.get_device_plugin(&device1_id, "ping").is_some());
    assert!(manager.get_device_plugin(&device2_id, "ping").is_some());

    // Cleanup one device
    manager.cleanup_device_plugins(&device1_id).await;

    // Verify device1 plugins removed but device2 remains
    assert!(manager.get_device_plugin(&device1_id, "battery").is_none());
    assert!(manager.get_device_plugin(&device2_id, "battery").is_some());

    Ok(())
}

#[tokio::test]
async fn test_packet_routing_to_correct_plugin() -> Result<()> {
    use cosmic_connect_protocol::plugins::PluginManager;

    let mut manager = PluginManager::new();

    // Register plugin factories
    manager.register_factory(Arc::new(battery::BatteryPluginFactory));
    manager.register_factory(Arc::new(ping::PingPluginFactory));

    let device = create_mock_device();
    let device_id = device.info.device_id.clone();

    manager.init_device_plugins(&device_id, &device).await?;

    // Create packets of different types
    let ping_plugin = ping::PingPlugin::new();
    let battery_plugin = battery::BatteryPlugin::new();

    let ping_packet = ping_plugin.create_ping(Some("Test".to_string()));
    let battery_packet = battery_plugin.create_battery_request();

    // Verify packets have correct types
    assert_eq!(ping_packet.packet_type, "cconnect.ping");
    assert_eq!(battery_packet.packet_type, "cconnect.battery.request");

    // Route packets through manager
    manager.route_packet(&device_id, &ping_packet).await;
    manager.route_packet(&device_id, &battery_packet).await;

    // Both should succeed (no panics/errors)
    Ok(())
}

#[tokio::test]
async fn test_plugin_capabilities_matching() -> Result<()> {
    // Test that plugin capabilities correctly match packet types
    let battery_plugin = battery::BatteryPlugin::new();
    let ping_plugin = ping::PingPlugin::new();
    let clipboard_plugin = clipboard::ClipboardPlugin::new();

    // Battery plugin capabilities
    let battery_incoming = battery_plugin.incoming_capabilities();
    assert!(battery_incoming.contains(&"cconnect.battery".to_string()));
    assert!(battery_incoming.contains(&"cconnect.battery.request".to_string()));

    // Ping plugin capabilities
    let ping_incoming = ping_plugin.incoming_capabilities();
    assert!(ping_incoming.contains(&"cconnect.ping".to_string()));

    // Clipboard plugin capabilities
    let clipboard_incoming = clipboard_plugin.incoming_capabilities();
    assert!(clipboard_incoming.contains(&"cconnect.clipboard".to_string()));
    assert!(clipboard_incoming.contains(&"cconnect.clipboard.connect".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_share_plugin_capabilities() {
    let plugin = share::SharePlugin::new();

    let incoming = plugin.incoming_capabilities();
    assert!(incoming.contains(&"cconnect.share.request".to_string()));
    assert!(incoming.contains(&"cconnect.share.request.update".to_string()));

    let outgoing = plugin.outgoing_capabilities();
    assert!(outgoing.contains(&"cconnect.share.request".to_string()));
}

#[tokio::test]
async fn test_clipboard_timestamp_loop_prevention() -> Result<()> {
    // Test that clipboard plugin uses timestamps to prevent sync loops
    let mut plugin = clipboard::ClipboardPlugin::new();
    let mut device = create_mock_device();

    plugin.init(&device).await?;

    // First clipboard update
    let content1 = "First content";
    let packet1 = plugin.create_clipboard_packet(content1.to_string()).await;

    // Verify timestamp is present
    assert!(packet1.body.get("timestamp").is_some());

    // Second update should have different timestamp
    tokio::time::sleep(Duration::from_millis(10)).await;
    let content2 = "Second content";
    let packet2 = plugin.create_clipboard_packet(content2.to_string()).await;

    // Timestamps should differ (preventing loops)
    let ts1 = packet1.body.get("timestamp").unwrap().as_i64().unwrap();
    let ts2 = packet2.body.get("timestamp").unwrap().as_i64().unwrap();
    assert!(ts2 > ts1);

    Ok(())
}
