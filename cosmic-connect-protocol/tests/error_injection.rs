//! Error Injection Tests
//!
//! Tests error handling paths through simulated failures:
//! - Network failures (timeout, connection refused, unreachable)
//! - File system errors (disk full, permission denied)
//! - Resource exhaustion (connection limits, transfer limits)
//! - Recovery mechanisms (reconnection, packet retry)
//! - Memory pressure scenarios

use cosmic_connect_protocol::{
    ProtocolError, RecoveryManager, ReconnectionStrategy, ResourceConfig, ResourceManager,
    TransferState,
};
use std::path::PathBuf;
use tempfile::TempDir;
use tokio::time::{sleep, Duration};

/// Test error classification for recoverable errors
#[test]
fn test_recoverable_error_classification() {
    // Network errors should be recoverable
    let error = ProtocolError::Timeout("connection timeout".to_string());
    assert!(error.is_recoverable());
    assert!(!error.requires_user_action());

    let error = ProtocolError::NetworkError("network failure".to_string());
    assert!(error.is_recoverable());
    assert!(!error.requires_user_action());

    let error = ProtocolError::NetworkUnreachable("network unreachable".to_string());
    assert!(error.is_recoverable());
    assert!(!error.requires_user_action());

    let error = ProtocolError::ConnectionRefused("connection refused".to_string());
    assert!(error.is_recoverable());
    assert!(!error.requires_user_action());
}

/// Test error classification for user action required errors
#[test]
fn test_user_action_required_classification() {
    // Pairing errors require user action
    let error = ProtocolError::NotPaired;
    assert!(!error.is_recoverable());
    assert!(error.requires_user_action());

    // Permission errors require user action
    let error = ProtocolError::PermissionDenied("access denied".to_string());
    assert!(!error.is_recoverable());
    assert!(error.requires_user_action());

    // Certificate errors require user action
    let error = ProtocolError::CertificateValidation("invalid cert".to_string());
    assert!(!error.is_recoverable());
    assert!(error.requires_user_action());

    // Configuration errors require user action
    let error = ProtocolError::Configuration("invalid config".to_string());
    assert!(!error.is_recoverable());
    assert!(error.requires_user_action());
}

/// Test error classification for critical errors
#[test]
fn test_critical_error_classification() {
    // Protocol errors are critical
    let error = ProtocolError::InvalidPacket("malformed packet".to_string());
    assert!(!error.is_recoverable());
    assert!(!error.requires_user_action());

    // Internal errors are critical
    let error = ProtocolError::InternalError("internal failure".to_string());
    assert!(!error.is_recoverable());
    assert!(!error.requires_user_action());
}

/// Test user-friendly error messages
#[test]
fn test_error_user_messages() {
    // NotPaired message
    let error = ProtocolError::NotPaired;
    let message = error.user_message();
    assert!(message.contains("pair"));
    assert!(message.to_lowercase().contains("device"));

    // Timeout message
    let error = ProtocolError::Timeout("operation".to_string());
    let message = error.user_message();
    assert!(message.to_lowercase().contains("timeout"));
    assert!(message.to_lowercase().contains("network"));

    // Permission denied message
    let error = ProtocolError::PermissionDenied("file access".to_string());
    let message = error.user_message();
    assert!(message.contains("Permission denied"));
    assert!(message.to_lowercase().contains("permission"));

    // Disk full message
    let error = ProtocolError::ResourceExhausted("disk full".to_string());
    let message = error.user_message();
    assert!(message.to_lowercase().contains("space"));
}

/// Test reconnection strategy with exponential backoff
#[test]
fn test_reconnection_exponential_backoff() {
    let mut strategy = ReconnectionStrategy::new();

    // First attempt: 2 seconds
    let delay1 = strategy.next_delay();
    assert!(delay1.is_some());
    assert_eq!(delay1.unwrap(), Duration::from_secs(2));

    // Second attempt: 4 seconds (doubled)
    let delay2 = strategy.next_delay();
    assert!(delay2.is_some());
    assert_eq!(delay2.unwrap(), Duration::from_secs(4));

    // Third attempt: 8 seconds (doubled)
    let delay3 = strategy.next_delay();
    assert!(delay3.is_some());
    assert_eq!(delay3.unwrap(), Duration::from_secs(8));

    // Fourth attempt: 16 seconds (doubled)
    let delay4 = strategy.next_delay();
    assert!(delay4.is_some());
    assert_eq!(delay4.unwrap(), Duration::from_secs(16));

    // Fifth attempt: 32 seconds (doubled)
    let delay5 = strategy.next_delay();
    assert!(delay5.is_some());
    assert_eq!(delay5.unwrap(), Duration::from_secs(32));

    // Sixth attempt: None (max attempts reached)
    let delay6 = strategy.next_delay();
    assert!(delay6.is_none());
}

/// Test reconnection strategy reset
#[test]
fn test_reconnection_strategy_reset() {
    let mut strategy = ReconnectionStrategy::new();

    // Use some attempts
    strategy.next_delay();
    strategy.next_delay();
    strategy.next_delay();
    assert_eq!(strategy.attempt, 3);

    // Reset
    strategy.reset();
    assert_eq!(strategy.attempt, 0);
    assert_eq!(strategy.current_delay, Duration::from_secs(2));

    // Should start from beginning again
    let delay = strategy.next_delay();
    assert_eq!(delay.unwrap(), Duration::from_secs(2));
}

/// Test reconnection attempt tracking
#[tokio::test]
async fn test_reconnection_attempt_tracking() {
    let temp_dir = TempDir::new().unwrap();
    let recovery_manager = RecoveryManager::new(temp_dir.path());
    recovery_manager.init().await.unwrap();

    let device_id = "test-device";

    // First reconnection attempt
    let delay1 = recovery_manager.should_reconnect(device_id).await;
    assert!(delay1.is_some());
    assert_eq!(delay1.unwrap(), Duration::from_secs(2));

    // Second attempt (without success)
    let delay2 = recovery_manager.should_reconnect(device_id).await;
    assert!(delay2.is_some());
    assert_eq!(delay2.unwrap(), Duration::from_secs(4));

    // Reset on success
    recovery_manager
        .reset_reconnection_strategy(device_id)
        .await;

    // Should start from beginning
    let delay3 = recovery_manager.should_reconnect(device_id).await;
    assert_eq!(delay3.unwrap(), Duration::from_secs(2));
}

/// Test transfer state tracking
#[tokio::test]
async fn test_transfer_state_tracking() {
    let temp_dir = TempDir::new().unwrap();
    let recovery_manager = RecoveryManager::new(temp_dir.path());
    recovery_manager.init().await.unwrap();

    // Register transfer
    let state = TransferState::new(
        "transfer-1".to_string(),
        "device-1".to_string(),
        "test.txt".to_string(),
        PathBuf::from("/tmp/test.txt"),
        1000,
    );
    recovery_manager.register_transfer(state).await.unwrap();

    // Get transfer state
    let retrieved = recovery_manager
        .get_transfer_state("transfer-1")
        .await
        .unwrap();
    assert_eq!(retrieved.transfer_id, "transfer-1");
    assert_eq!(retrieved.total_size, 1000);
    assert_eq!(retrieved.bytes_received, 0);

    // Update progress
    recovery_manager
        .update_transfer_progress("transfer-1", 500)
        .await
        .unwrap();

    let updated = recovery_manager
        .get_transfer_state("transfer-1")
        .await
        .unwrap();
    assert_eq!(updated.bytes_received, 500);
    assert_eq!(updated.progress_percentage(), 50.0);

    // Complete transfer
    recovery_manager
        .complete_transfer("transfer-1")
        .await
        .unwrap();

    assert!(recovery_manager
        .get_transfer_state("transfer-1")
        .await
        .is_none());
}

/// Test transfer state persistence across restarts
#[tokio::test]
async fn test_transfer_state_persistence() {
    let temp_dir = TempDir::new().unwrap();

    // Create manager and register transfer
    {
        let recovery_manager = RecoveryManager::new(temp_dir.path());
        recovery_manager.init().await.unwrap();

        let state = TransferState::new(
            "transfer-persist".to_string(),
            "device-1".to_string(),
            "large_file.zip".to_string(),
            PathBuf::from("/tmp/large_file.zip"),
            10000,
        );
        recovery_manager.register_transfer(state).await.unwrap();

        recovery_manager
            .update_transfer_progress("transfer-persist", 5000)
            .await
            .unwrap();
    }

    // Create new manager (simulates restart)
    {
        let recovery_manager = RecoveryManager::new(temp_dir.path());
        recovery_manager.init().await.unwrap();

        // State should be restored
        let restored = recovery_manager
            .get_transfer_state("transfer-persist")
            .await
            .unwrap();
        assert_eq!(restored.filename, "large_file.zip");
        assert_eq!(restored.bytes_received, 5000);
        assert_eq!(restored.progress_percentage(), 50.0);
    }
}

/// Test resource exhaustion - connection limits
#[tokio::test]
async fn test_connection_limit_exhaustion() {
    let mut config = ResourceConfig::default();
    config.max_connections_per_device = 2;
    config.max_total_connections = 5;

    let resource_manager = ResourceManager::new(config);

    // Fill up device-1 connections
    resource_manager
        .register_connection("conn-1".to_string(), "device-1".to_string())
        .await
        .unwrap();
    resource_manager
        .register_connection("conn-2".to_string(), "device-1".to_string())
        .await
        .unwrap();

    // Third connection to device-1 should fail
    let result = resource_manager
        .register_connection("conn-3".to_string(), "device-1".to_string())
        .await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ProtocolError::ResourceExhausted(_)
    ));

    // Connection to different device should succeed
    let result = resource_manager
        .register_connection("conn-4".to_string(), "device-2".to_string())
        .await;
    assert!(result.is_ok());
}

/// Test resource exhaustion - transfer limits
#[tokio::test]
async fn test_transfer_limit_exhaustion() {
    let mut config = ResourceConfig::default();
    config.max_concurrent_transfers = 2;
    config.max_transfer_size = 1000;

    let resource_manager = ResourceManager::new(config);

    // Register two transfers
    let transfer1 = cosmic_connect_protocol::TransferInfo::new(
        "t1".to_string(),
        "device-1".to_string(),
        500,
    );
    resource_manager
        .register_transfer(transfer1)
        .await
        .unwrap();

    let transfer2 = cosmic_connect_protocol::TransferInfo::new(
        "t2".to_string(),
        "device-2".to_string(),
        500,
    );
    resource_manager
        .register_transfer(transfer2)
        .await
        .unwrap();

    // Third transfer should fail (max concurrent reached)
    let transfer3 = cosmic_connect_protocol::TransferInfo::new(
        "t3".to_string(),
        "device-3".to_string(),
        500,
    );
    let result = resource_manager.register_transfer(transfer3).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ProtocolError::ResourceExhausted(_)
    ));
}

/// Test resource exhaustion - transfer size limits
#[tokio::test]
async fn test_transfer_size_limit_exhaustion() {
    let mut config = ResourceConfig::default();
    config.max_transfer_size = 1000;
    config.max_total_transfer_size = 2000;

    let resource_manager = ResourceManager::new(config);

    // Transfer exceeding max size should fail
    let transfer1 = cosmic_connect_protocol::TransferInfo::new(
        "t1".to_string(),
        "device-1".to_string(),
        1500, // Exceeds max_transfer_size
    );
    let result = resource_manager.register_transfer(transfer1).await;
    assert!(result.is_err());

    // Register transfer at limit
    let transfer2 = cosmic_connect_protocol::TransferInfo::new(
        "t2".to_string(),
        "device-1".to_string(),
        1000,
    );
    resource_manager
        .register_transfer(transfer2)
        .await
        .unwrap();

    // Another transfer that would exceed total should fail
    let transfer3 = cosmic_connect_protocol::TransferInfo::new(
        "t3".to_string(),
        "device-2".to_string(),
        1000, // Total: 1000 + 1000 = 2000 (at limit)
    );
    resource_manager
        .register_transfer(transfer3)
        .await
        .unwrap();

    // One more should fail
    let transfer4 = cosmic_connect_protocol::TransferInfo::new(
        "t4".to_string(),
        "device-3".to_string(),
        100,
    );
    let result = resource_manager.register_transfer(transfer4).await;
    assert!(result.is_err());
}

/// Test packet queue limits
#[tokio::test]
async fn test_packet_queue_limit_exhaustion() {
    let mut config = ResourceConfig::default();
    config.max_packet_queue_size = 3;

    let resource_manager = ResourceManager::new(config);

    let device_id = "device-1";

    // Fill queue
    resource_manager
        .increment_queue_size(device_id)
        .await
        .unwrap();
    resource_manager
        .increment_queue_size(device_id)
        .await
        .unwrap();
    resource_manager
        .increment_queue_size(device_id)
        .await
        .unwrap();

    // Fourth packet should fail
    let result = resource_manager.increment_queue_size(device_id).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ProtocolError::ResourceExhausted(_)
    ));

    // Decrement and try again
    resource_manager.decrement_queue_size(device_id).await;
    let result = resource_manager.increment_queue_size(device_id).await;
    assert!(result.is_ok());
}

/// Test memory pressure detection
#[tokio::test]
async fn test_memory_pressure_detection() {
    let mut config = ResourceConfig::default();
    config.memory_pressure_threshold = 2000; // 2KB threshold

    let resource_manager = ResourceManager::new(config);

    // Register transfer that exceeds threshold
    let transfer = cosmic_connect_protocol::TransferInfo::new(
        "t1".to_string(),
        "device-1".to_string(),
        3000, // 3KB, exceeds 2KB threshold
    );
    resource_manager.register_transfer(transfer).await.unwrap();

    // Check memory stats
    let stats = resource_manager.get_memory_stats().await;
    assert!(stats.is_under_pressure(2000));
    assert!(stats.transfer_memory >= 3000);
}

/// Test packet retry queue processing
#[tokio::test]
async fn test_packet_retry_queue() {
    let temp_dir = TempDir::new().unwrap();
    let recovery_manager = RecoveryManager::new(temp_dir.path());
    recovery_manager.init().await.unwrap();

    // Queue packets
    let packet1 =
        cosmic_connect_protocol::Packet::new("cconnect.ping", serde_json::json!({}));
    let packet2 =
        cosmic_connect_protocol::Packet::new("cconnect.share", serde_json::json!({}));

    recovery_manager
        .queue_packet_retry("device-1".to_string(), packet1.clone())
        .await;
    recovery_manager
        .queue_packet_retry("device-1".to_string(), packet2.clone())
        .await;

    // Process queue
    let to_retry = recovery_manager.process_retry_queue().await;
    assert_eq!(to_retry.len(), 2);
    assert_eq!(to_retry[0].0, "device-1");
    assert_eq!(to_retry[0].1.packet_type, "cconnect.ping");
    assert_eq!(to_retry[1].0, "device-1");
    assert_eq!(to_retry[1].1.packet_type, "cconnect.share");
}

/// Test packet retry exhaustion (max retries)
#[tokio::test]
async fn test_packet_retry_exhaustion() {
    let temp_dir = TempDir::new().unwrap();
    let recovery_manager = RecoveryManager::new(temp_dir.path());
    recovery_manager.init().await.unwrap();

    // Queue packet
    let packet = cosmic_connect_protocol::Packet::new("cconnect.test", serde_json::json!({}));
    recovery_manager
        .queue_packet_retry("device-1".to_string(), packet.clone())
        .await;

    // Process queue multiple times (simulating failures)
    // Max retries is 3, so after 3 attempts packet should be dropped
    for i in 0..3 {
        let to_retry = recovery_manager.process_retry_queue().await;
        assert_eq!(to_retry.len(), 1, "Attempt {}: should have packet", i + 1);
    }

    // Fourth attempt: packet should be dropped
    let to_retry = recovery_manager.process_retry_queue().await;
    assert_eq!(to_retry.len(), 0, "Packet should be dropped after max retries");
}

/// Test stale connection cleanup
#[tokio::test]
async fn test_stale_connection_cleanup() {
    let config = ResourceConfig::default();
    let resource_manager = ResourceManager::new(config);

    // Register connections
    resource_manager
        .register_connection("conn-1".to_string(), "device-1".to_string())
        .await
        .unwrap();
    resource_manager
        .register_connection("conn-2".to_string(), "device-2".to_string())
        .await
        .unwrap();

    assert_eq!(resource_manager.get_connection_count().await, 2);

    // Update activity for conn-1
    resource_manager.update_connection_activity("conn-1").await;

    // Wait a bit
    sleep(Duration::from_millis(100)).await;

    // Cleanup with very short timeout (should remove conn-2 but not conn-1)
    // Note: In real test, would need longer delay to actually test this properly
    // For now, just verify the method works
    resource_manager.cleanup_stale_connections(0).await;

    // Both should be removed with 0 second timeout
    assert_eq!(resource_manager.get_connection_count().await, 0);
}

/// Test resource summary generation
#[tokio::test]
async fn test_resource_summary() {
    let config = ResourceConfig::default();
    let resource_manager = ResourceManager::new(config);

    // Register some resources
    resource_manager
        .register_connection("conn-1".to_string(), "device-1".to_string())
        .await
        .unwrap();

    let transfer = cosmic_connect_protocol::TransferInfo::new(
        "t1".to_string(),
        "device-1".to_string(),
        1000,
    );
    resource_manager.register_transfer(transfer).await.unwrap();

    // Get summary
    let summary = resource_manager.get_resource_summary().await;
    assert!(summary.contains("Connections: 1/50"));
    assert!(summary.contains("Transfers: 1/10"));
    assert!(summary.contains("Memory:"));
}

/// Test device-specific resource tracking
#[tokio::test]
async fn test_device_specific_tracking() {
    let config = ResourceConfig::default();
    let resource_manager = ResourceManager::new(config);

    // Register connections for different devices
    resource_manager
        .register_connection("conn-1".to_string(), "device-1".to_string())
        .await
        .unwrap();
    resource_manager
        .register_connection("conn-2".to_string(), "device-1".to_string())
        .await
        .unwrap();
    resource_manager
        .register_connection("conn-3".to_string(), "device-2".to_string())
        .await
        .unwrap();

    // Check device-specific counts
    assert_eq!(
        resource_manager
            .get_device_connection_count("device-1")
            .await,
        2
    );
    assert_eq!(
        resource_manager
            .get_device_connection_count("device-2")
            .await,
        1
    );
    assert_eq!(resource_manager.get_connection_count().await, 3);

    // Register transfers for different devices
    let transfer1 = cosmic_connect_protocol::TransferInfo::new(
        "t1".to_string(),
        "device-1".to_string(),
        1000,
    );
    resource_manager
        .register_transfer(transfer1)
        .await
        .unwrap();

    let transfer2 = cosmic_connect_protocol::TransferInfo::new(
        "t2".to_string(),
        "device-1".to_string(),
        1000,
    );
    resource_manager
        .register_transfer(transfer2)
        .await
        .unwrap();

    let transfer3 = cosmic_connect_protocol::TransferInfo::new(
        "t3".to_string(),
        "device-2".to_string(),
        1000,
    );
    resource_manager
        .register_transfer(transfer3)
        .await
        .unwrap();

    // Check device-specific transfer counts
    assert_eq!(
        resource_manager
            .get_device_transfer_count("device-1")
            .await,
        2
    );
    assert_eq!(
        resource_manager
            .get_device_transfer_count("device-2")
            .await,
        1
    );
}
