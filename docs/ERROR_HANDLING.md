# Error Handling and Recovery Guide

This document describes the comprehensive error handling and automatic recovery system in COSMIC Connect.

## Table of Contents

- [Architecture Overview](#architecture-overview)
- [Error Classification](#error-classification)
- [User Notifications](#user-notifications)
- [Auto-Recovery Mechanisms](#auto-recovery-mechanisms)
- [Resource Management](#resource-management)
- [File System Error Handling](#file-system-error-handling)
- [Integration Guide](#integration-guide)
- [Best Practices](#best-practices)
- [Troubleshooting](#troubleshooting)

## Architecture Overview

The error handling system consists of four main components:

```
┌─────────────────────┐
│  ProtocolError      │  Error classification and user messages
│  (cosmic-connect-   │
│   protocol)         │
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│  ErrorHandler       │  Centralized error processing
│  (daemon)           │  - Logging
└──────────┬──────────┘  - User notifications
           │              - Recovery decisions
           ▼
    ┌──────┴──────┐
    │             │
    ▼             ▼
┌─────────┐  ┌─────────────────┐
│Notifier │  │RecoveryManager  │  Automatic recovery
│         │  │- Reconnection   │  - Exponential backoff
│         │  │- Packet retry   │  - Transfer resumption
│         │  │- State persist  │  - Crash recovery
└─────────┘  └─────────────────┘
```

### Components

1. **ProtocolError** (`cosmic-connect-protocol/src/error.rs`)
   - Comprehensive error enum with 20+ variants
   - Error classification methods
   - User-friendly message generation

2. **ErrorHandler** (`cosmic-connect-daemon/src/error_handler.rs`)
   - Centralized error processing
   - Automatic logging at appropriate levels
   - Triggers user notifications when needed
   - Coordinates with recovery system

3. **CosmicNotifier** (`cosmic-connect-daemon/src/cosmic_notifications.rs`)
   - User notification system via COSMIC Desktop
   - 10+ specialized notification methods
   - Recovery action buttons (Re-pair, Settings, Retry)

4. **RecoveryManager** (`cosmic-connect-protocol/src/recovery.rs`)
   - Reconnection strategies with exponential backoff
   - Packet retry queue
   - Transfer state tracking
   - State persistence for crash recovery

5. **RecoveryCoordinator** (`cosmic-connect-protocol/src/recovery_coordinator.rs`)
   - Bridges ConnectionManager and RecoveryManager
   - Listens to connection events
   - Triggers automatic recovery actions

## Error Classification

Errors are classified into three categories based on their handling requirements:

### 1. Recoverable Errors

Errors that can be automatically retried without user intervention.

**Examples:**
- `Timeout` - Network operation timeout
- `NetworkError` - Temporary network issues
- `NetworkUnreachable` - Network temporarily unavailable
- `ConnectionRefused` - Service not running (may recover)

**Handling:**
- Logged as warnings
- Automatic retry with exponential backoff
- No immediate user notification (unless persistent)

```rust
// Check if error is recoverable
if error.is_recoverable() {
    warn!("Recoverable error: {}", error);
    // Trigger auto-retry
}
```

### 2. User Action Required

Errors that require user intervention to resolve.

**Examples:**
- `NotPaired` - Device not paired
- `PermissionDenied` - Insufficient permissions
- `Certificate` / `CertificateValidation` - Certificate issues
- `Configuration` - Invalid configuration
- `ProtocolVersionMismatch` - Incompatible versions

**Handling:**
- Logged as warnings or errors
- User notification with actionable recovery steps
- No automatic retry (user must fix the issue)

```rust
// Check if user action is needed
if error.requires_user_action() {
    // Show notification with recovery action
    notifier.notify_error_with_recovery(
        "Device Not Paired",
        "Please pair the device first.",
        Some(("pair", "Pair Device"))
    ).await?;
}
```

### 3. Critical Errors

Errors that indicate serious problems and cannot be automatically recovered.

**Examples:**
- `InvalidPacket` - Malformed protocol data
- `TlsError` - TLS/SSL failures
- `InternalError` - Unexpected internal state

**Handling:**
- Logged as errors
- May trigger user notification for context
- No automatic retry

## User Notifications

The notification system provides user-friendly error messages with recovery actions.

### Notification Types

#### Network Errors

```rust
notifier.notify_network_error(
    device_name,
    "Connection refused. Check if KDE Connect is running on the device."
).await?;
```

**Features:**
- Critical urgency for connection failures
- 10-second timeout
- Network error icon

#### File Transfer Errors

```rust
notifier.notify_file_transfer_error(
    device_name,
    filename,
    &error.user_message()
).await?;
```

**Features:**
- Normal urgency
- 8-second timeout
- Document error icon
- Shows filename and device

#### Permission Errors

```rust
notifier.notify_permission_error(
    "access downloads folder",
    "Check folder permissions in System Settings."
).await?;
```

**Features:**
- Critical urgency
- "Open Settings" action button
- Security icon
- Clear instructions for resolution

#### Disk Full Errors

```rust
notifier.notify_disk_full_error(
    "/home/user/Downloads"
).await?;
```

**Features:**
- Critical urgency
- "Free Space" action button
- Warning icon
- Shows affected path

#### Certificate Errors

```rust
notifier.notify_certificate_error(
    device_name,
    "Certificate fingerprint mismatch. Device may have been reset."
).await?;
```

**Features:**
- Critical urgency
- "Re-pair Device" action button
- Security warning icon
- Explains security implications

### Custom Notifications with Recovery Actions

```rust
notifier.notify_error_with_recovery(
    "Operation Failed",
    "The operation could not be completed.",
    Some(("retry", "Retry"))  // Optional recovery action
).await?;
```

## Auto-Recovery Mechanisms

### 1. Automatic Reconnection

Automatically reconnects to paired devices when connection is lost.

**Strategy:**
- Exponential backoff: 2s, 4s, 8s, 16s, 32s (max 60s)
- Up to 5 reconnection attempts
- Only for paired and trusted devices
- Resets on successful connection

**Configuration:**
```rust
// In RecoveryManager
const MAX_RECONNECT_ATTEMPTS: u32 = 5;
const INITIAL_RECONNECT_DELAY: Duration = Duration::from_secs(2);
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);
```

**Usage:**
```rust
// RecoveryCoordinator automatically handles reconnection
// when it receives ConnectionEvent::Disconnected

// Manual check:
if let Some(delay) = recovery_manager.should_reconnect(device_id).await {
    // Wait and reconnect
    sleep(delay).await;
    connection_manager.connect(device_id, addr).await?;
}
```

### 2. Packet Retry

Automatically retries failed packet sends.

**Strategy:**
- 3 retry attempts with 500ms delay
- Packets queued on send failure
- Automatic retry on next process cycle
- Dropped after max retries

**Usage:**
```rust
// Queue packet for retry
recovery_manager.queue_packet_retry(
    device_id.to_string(),
    packet.clone()
).await;

// Process retry queue (call periodically)
recovery_coordinator.process_packet_retries().await?;
```

### 3. Transfer Resumption

Tracks file transfer progress for resumption after interruption.

**Features:**
- State persistence to disk (JSON)
- Tracks bytes received, file path, device ID
- Progress percentage calculation
- Automatic cleanup of old transfers (>24h)

**Usage:**
```rust
// Register new transfer
let state = TransferState::new(
    transfer_id,
    device_id,
    filename,
    file_path,
    total_size
);
recovery_manager.register_transfer(state).await?;

// Update progress during transfer
recovery_manager.update_transfer_progress(
    transfer_id,
    bytes_received
).await?;

// Complete transfer
recovery_manager.complete_transfer(transfer_id).await?;

// Resume interrupted transfer
if let Some(state) = recovery_manager.get_transfer_state(transfer_id).await {
    // Resume from state.bytes_received offset
}
```

### 4. Crash Recovery

Persists critical state to survive daemon restarts.

**Persisted Data:**
- File transfer states
- Bytes received per transfer
- Device connection history

**State File Location:**
```
~/.local/share/cosmic/cosmic-connect/recovery_state.json
```

**Restoration:**
```rust
// Initialize recovery manager (restores state)
let recovery_manager = RecoveryManager::new(state_dir);
recovery_manager.init().await?;

// Get restored transfers
let active_transfers = recovery_manager.get_device_transfers(device_id).await;
```

## Resource Management

Prevents resource exhaustion and ensures system stability through limits and quotas.

### Connection Limits

Prevents connection flooding and DoS attacks.

**Limits:**
- Maximum connections per device: 3
- Maximum total connections: 50

**Usage:**
```rust
use cosmic_connect_protocol::{ResourceManager, ResourceConfig};

let resource_manager = ResourceManager::new(ResourceConfig::default());

// Check if connection can be accepted
resource_manager.can_accept_connection(device_id).await?;

// Register connection
resource_manager.register_connection(
    connection_id.to_string(),
    device_id.to_string()
).await?;

// Update activity timestamp (keeps connection alive)
resource_manager.update_connection_activity(&connection_id).await;

// Unregister on disconnect
resource_manager.unregister_connection(&connection_id).await;
```

**Features:**
- Per-device connection limits (prevents single device DoS)
- Total connection limits (prevents overall exhaustion)
- Automatic stale connection cleanup
- Connection activity tracking

### File Transfer Limits

Prevents memory exhaustion from concurrent transfers.

**Limits:**
- Maximum concurrent transfers: 10
- Maximum transfers per device: 3
- Maximum single file size: 100 MB
- Maximum total transfer size: 1 GB

**Usage:**
```rust
// Check if transfer can start
resource_manager.can_start_transfer(device_id, file_size).await?;

// Register transfer
let transfer_info = resource_manager::TransferInfo::new(
    transfer_id,
    device_id,
    file_size
);
resource_manager.register_transfer(transfer_info).await?;

// Update progress
resource_manager.update_transfer_progress(transfer_id, bytes).await;

// Unregister on completion
resource_manager.unregister_transfer(transfer_id).await;
```

**Features:**
- Concurrent transfer limits
- Per-device transfer limits
- Single file size limits
- Total transfer size limits
- Progress tracking

### Memory Pressure Management

Monitors memory usage and warns when approaching limits.

**Threshold:**
- Memory pressure warning: 500 MB

**Usage:**
```rust
// Get memory statistics
let stats = resource_manager.get_memory_stats().await;

println!("Transfer memory: {} MB", stats.transfer_memory / (1024 * 1024));
println!("Queue memory: {} MB", stats.queue_memory / (1024 * 1024));
println!("Total memory: {} MB", stats.total_memory / (1024 * 1024));

// Check for pressure
if stats.is_under_pressure(threshold) {
    warn!("Memory pressure detected!");
}
```

**Features:**
- Automatic memory usage estimation
- Transfer buffer tracking
- Packet queue memory tracking
- Pressure warnings in logs

### Packet Queue Limits

Prevents unbounded queue growth.

**Limits:**
- Maximum queue size per device: 100 packets

**Usage:**
```rust
// Check if packet can be queued
resource_manager.can_queue_packet(device_id).await?;

// Increment queue size
resource_manager.increment_queue_size(device_id).await?;

// Decrement when packet is sent or dropped
resource_manager.decrement_queue_size(device_id).await;

// Get current queue size
let size = resource_manager.get_queue_size(device_id).await;
```

**Features:**
- Per-device queue limits
- Memory tracking (~1KB per packet)
- Automatic cleanup on connection close

### Resource Summary

Get overall resource usage summary.

```rust
let summary = resource_manager.get_resource_summary().await;
println!("{}", summary);
// Output: "Connections: 5/50, Transfers: 3/10, Memory: 150 MB / 500 MB"
```

### Cleanup Tasks

Periodic cleanup to prevent resource leaks.

```rust
// Clean up stale connections (no activity for 5+ minutes)
resource_manager.cleanup_stale_connections(300).await;
```

**Recommended Schedule:**
- Stale connection cleanup: Every 5 minutes
- Resource summary logging: Every minute (debug builds)

## File System Error Handling

Safe file operations with proper error handling and disk space management.

### Safe File Creation

```rust
use cosmic_connect_protocol::fs_utils::create_file_safe;

// Automatically:
// - Creates parent directories
// - Handles permission errors
// - Detects disk full
// - Provides user-friendly errors
let mut file = create_file_safe(file_path).await?;
```

### Safe File Writing

```rust
use cosmic_connect_protocol::fs_utils::write_file_safe;

// Detects disk full during write
write_file_safe(&mut file, data).await?;
```

### Partial File Cleanup

```rust
use cosmic_connect_protocol::fs_utils::cleanup_partial_file;

// Automatically called on transfer failure
if transfer_result.is_err() {
    cleanup_partial_file(file_path).await;
}
```

### Unique File Paths

```rust
use cosmic_connect_protocol::fs_utils::get_unique_download_path;

// Automatically handles conflicts:
// test.txt -> test (1).txt -> test (2).txt
let unique_path = get_unique_download_path(
    downloads_dir,
    filename
).await;
```

## Integration Guide

### Setting Up Error Handling in Daemon

```rust
use cosmic_connect_protocol::{RecoveryManager, RecoveryCoordinator};
use cosmic_connect_daemon::error_handler::ErrorHandler;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Create error handler
    let error_handler = ErrorHandler::new();
    error_handler.init().await?;

    // 2. Create recovery manager
    let state_dir = config.paths.data_dir.clone();
    let recovery_manager = Arc::new(RecoveryManager::new(&state_dir));
    recovery_manager.init().await?;

    // 3. Create recovery coordinator
    let recovery_coordinator = RecoveryCoordinator::new(
        connection_manager.clone(),
        device_manager.clone(),
        recovery_manager.clone()
    );
    recovery_coordinator.start().await?;

    // 4. Spawn periodic tasks
    tokio::spawn({
        let recovery_coordinator = recovery_coordinator.clone();
        async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                let _ = recovery_coordinator.process_packet_retries().await;
            }
        }
    });

    // 5. Spawn cleanup task (daily)
    tokio::spawn({
        let recovery_coordinator = recovery_coordinator.clone();
        async move {
            let mut interval = tokio::time::interval(Duration::from_secs(86400));
            loop {
                interval.tick().await;
                let _ = recovery_coordinator.cleanup_old_transfers().await;
            }
        }
    });

    Ok(())
}
```

### Handling Errors in Plugin Code

```rust
use cosmic_connect_protocol::ProtocolError;

impl Plugin for MyPlugin {
    async fn handle_packet(&self, packet: &Packet) -> Result<()> {
        // Perform operation
        match perform_operation().await {
            Ok(_) => Ok(()),
            Err(e) => {
                // Log and classify error
                let is_recoverable = error_handler.handle_error(
                    &e,
                    "processing plugin packet",
                    Some(device_id)
                ).await;

                // Queue for retry if recoverable
                if is_recoverable {
                    recovery_manager.queue_packet_retry(
                        device_id.to_string(),
                        packet.clone()
                    ).await;
                }

                Err(e)
            }
        }
    }
}
```

### Handling File Transfer Errors

```rust
use cosmic_connect_protocol::fs_utils::{create_file_safe, write_file_safe, cleanup_partial_file};

async fn receive_file(
    device_id: &str,
    filename: &str,
    data: Vec<u8>
) -> Result<()> {
    let file_path = downloads_dir.join(filename);

    // Create file safely
    let mut file = match create_file_safe(&file_path).await {
        Ok(f) => f,
        Err(e) => {
            error_handler.handle_file_transfer_error(
                device_name,
                filename,
                &e
            ).await?;
            return Err(e);
        }
    };

    // Write data safely
    match write_file_safe(&mut file, &data).await {
        Ok(_) => {
            info!("File received successfully: {}", filename);
            Ok(())
        }
        Err(e) => {
            // Cleanup partial file
            cleanup_partial_file(&file_path).await;

            // Notify error
            error_handler.handle_file_transfer_error(
                device_name,
                filename,
                &e
            ).await?;

            Err(e)
        }
    }
}
```

## Best Practices

### 1. Error Handling

 **DO:**
- Use `ProtocolError` variants for all protocol-level errors
- Call `error_handler.handle_error()` for automatic classification and notification
- Check `is_recoverable()` before automatic retry
- Provide context in error messages (device ID, operation, filename)
- Log errors at appropriate levels (error/warn/debug)

 **DON'T:**
- Use `unwrap()` or `expect()` in production code
- Silently ignore errors
- Show generic error messages to users
- Retry non-recoverable errors
- Skip error logging

### 2. User Notifications

 **DO:**
- Use specific notification methods (`notify_network_error`, `notify_file_transfer_error`, etc.)
- Provide actionable recovery steps
- Include relevant context (device name, filename)
- Use appropriate urgency levels
- Add recovery action buttons when applicable

 **DON'T:**
- Show technical error messages to users
- Spam users with repeated notifications
- Use critical urgency for non-critical issues
- Show notifications for internal/debug errors

### 3. Auto-Recovery

 **DO:**
- Only auto-reconnect to paired devices
- Use exponential backoff for reconnections
- Reset reconnection strategy on success
- Clear retry queues on reconnection
- Track transfer state for resumption

 **DON'T:**
- Reconnect indefinitely without backoff
- Retry without delay (causes connection storms)
- Reconnect to unpaired devices (security risk)
- Keep failed packets in retry queue indefinitely

### 4. File Operations

 **DO:**
- Use `create_file_safe()` for file creation
- Use `write_file_safe()` for writing
- Call `cleanup_partial_file()` on transfer failure
- Use `get_unique_download_path()` to avoid conflicts
- Check disk space before large transfers

 **DON'T:**
- Use `File::create()` directly (may panic)
- Leave partial files on failure
- Overwrite existing files without user consent
- Ignore disk space constraints

## Troubleshooting

### Connection Issues

**Symptom:** Devices frequently disconnecting and reconnecting

**Possible Causes:**
1. Network instability
2. Android app aggressive reconnection
3. Firewall interference

**Solutions:**
1. Check reconnection logs for timing patterns
2. Verify MAX_RECONNECT_ATTEMPTS and delays are appropriate
3. Look for "reconnecting rapidly" warnings in logs
4. Check if socket replacement is working (Issue #52)

**Relevant Logs:**
```
INFO  Device device-123 disconnected: Connection closed
INFO  Scheduling reconnection for device device-123 after 2s
INFO  Attempting reconnection to device device-123 at 192.168.1.100:1716
INFO  Successfully reconnected to device device-123
```

### File Transfer Failures

**Symptom:** File transfers failing without clear error

**Possible Causes:**
1. Insufficient disk space
2. Permission denied on downloads folder
3. Network timeout during transfer
4. Partial file not cleaned up

**Solutions:**
1. Check disk space: `df -h ~/Downloads`
2. Check folder permissions: `ls -ld ~/Downloads`
3. Review transfer state in recovery_state.json
4. Look for "disk full" or "permission denied" in logs

**Relevant Logs:**
```
ERROR Failed to create file "large_file.zip": Disk full: cannot create file /home/user/Downloads/large_file.zip
WARN  Transfer failed, cleaning up partial file: "/home/user/Downloads/large_file.zip"
```

### Packet Retry Issues

**Symptom:** Packets being dropped after retries

**Possible Causes:**
1. Device no longer available
2. Max retry attempts reached
3. Network issues preventing delivery

**Solutions:**
1. Verify device is still connected
2. Check retry queue size: should not grow indefinitely
3. Review MAX_PACKET_RETRIES setting
4. Look for connection events around failed retries

**Relevant Logs:**
```
DEBUG Retrying packet 'kdeconnect.share.request' to device device-123 (attempt 2/3)
WARN  Failed to retry packet 'kdeconnect.share.request' to device device-123: Device not connected
WARN  Dropping packet 'kdeconnect.share.request' to device device-123 after 3 failed attempts
```

### Recovery State Issues

**Symptom:** Transfer states not persisting or being restored

**Possible Causes:**
1. Permission denied on state file
2. Corrupted recovery_state.json
3. State file not being written

**Solutions:**
1. Check file permissions: `ls -l ~/.local/share/cosmic/cosmic-connect/recovery_state.json`
2. Validate JSON: `jq . recovery_state.json`
3. Review init logs for restoration errors
4. Check that persist_transfer_states() is being called

**Relevant Logs:**
```
DEBUG Persisted 2 transfer states to disk
INFO  Restored 2 transfer states from disk
WARN  Failed to restore transfer states: Permission denied
```

---

## Related Documentation

- [Protocol Error Types](../cosmic-connect-protocol/src/error.rs)
- [User Notifications](../cosmic-connect-daemon/src/cosmic_notifications.rs)
- [Error Handler](../cosmic-connect-daemon/src/error_handler.rs)
- [Recovery Manager](../cosmic-connect-protocol/src/recovery.rs)
- [Recovery Coordinator](../cosmic-connect-protocol/src/recovery_coordinator.rs)
- [File System Utils](../cosmic-connect-protocol/src/fs_utils.rs)

## Version History

- **v0.1.0** (2025-01-16): Initial error handling and recovery system
  - ProtocolError classification
  - User notification system
  - Auto-reconnection with exponential backoff
  - Packet retry mechanism
  - Transfer state tracking
  - Crash recovery with state persistence
  - File system error handling

---

*For questions or issues, please file a bug report on GitHub.*
