//! Resource Management and Limits
//!
//! Provides resource management to prevent exhaustion and ensure system stability.
//! Manages connection limits, memory pressure, concurrent transfers, and quotas.

use crate::{ProtocolError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Maximum number of concurrent connections to a single device
const MAX_CONNECTIONS_PER_DEVICE: usize = 3;

/// Maximum total number of concurrent connections
const MAX_TOTAL_CONNECTIONS: usize = 50;

/// Maximum number of concurrent file transfers
const MAX_CONCURRENT_TRANSFERS: usize = 10;

/// Maximum number of concurrent file transfers per device
const MAX_TRANSFERS_PER_DEVICE: usize = 3;

/// Maximum size for a single file transfer (100 MB)
const MAX_TRANSFER_SIZE: u64 = 100 * 1024 * 1024;

/// Maximum total size of active transfers (1 GB)
const MAX_TOTAL_TRANSFER_SIZE: u64 = 1024 * 1024 * 1024;

/// Memory pressure threshold (bytes) - warn when approaching
const MEMORY_PRESSURE_THRESHOLD: u64 = 500 * 1024 * 1024; // 500 MB

/// Maximum packet queue size per device
const MAX_PACKET_QUEUE_SIZE: usize = 100;

/// Resource management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    /// Maximum connections per device
    pub max_connections_per_device: usize,
    /// Maximum total connections
    pub max_total_connections: usize,
    /// Maximum concurrent transfers
    pub max_concurrent_transfers: usize,
    /// Maximum transfers per device
    pub max_transfers_per_device: usize,
    /// Maximum transfer size in bytes
    pub max_transfer_size: u64,
    /// Maximum total transfer size in bytes
    pub max_total_transfer_size: u64,
    /// Memory pressure threshold in bytes
    pub memory_pressure_threshold: u64,
    /// Maximum packet queue size per device
    pub max_packet_queue_size: usize,
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            max_connections_per_device: MAX_CONNECTIONS_PER_DEVICE,
            max_total_connections: MAX_TOTAL_CONNECTIONS,
            max_concurrent_transfers: MAX_CONCURRENT_TRANSFERS,
            max_transfers_per_device: MAX_TRANSFERS_PER_DEVICE,
            max_transfer_size: MAX_TRANSFER_SIZE,
            max_total_transfer_size: MAX_TOTAL_TRANSFER_SIZE,
            memory_pressure_threshold: MEMORY_PRESSURE_THRESHOLD,
            max_packet_queue_size: MAX_PACKET_QUEUE_SIZE,
        }
    }
}

/// Active connection tracking
#[derive(Debug, Clone)]
struct ConnectionInfo {
    /// Device ID
    device_id: String,
    /// Connection timestamp
    connected_at: u64,
    /// Last activity timestamp
    last_activity: u64,
}

/// Active transfer tracking
#[derive(Debug, Clone)]
pub struct TransferInfo {
    /// Transfer ID
    pub transfer_id: String,
    /// Device ID
    pub device_id: String,
    /// File size in bytes
    pub size: u64,
    /// Transfer start timestamp
    pub started_at: u64,
    /// Bytes transferred so far
    pub bytes_transferred: u64,
}

impl TransferInfo {
    /// Create a new transfer info
    pub fn new(transfer_id: String, device_id: String, size: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs();

        Self {
            transfer_id,
            device_id,
            size,
            started_at: now,
            bytes_transferred: 0,
        }
    }

    /// Update transfer progress
    pub fn update_progress(&mut self, bytes: u64) {
        self.bytes_transferred = bytes;
    }

    /// Check if transfer is complete
    pub fn is_complete(&self) -> bool {
        self.bytes_transferred >= self.size
    }

    /// Get transfer progress percentage
    pub fn progress_percentage(&self) -> f64 {
        if self.size == 0 {
            return 0.0;
        }
        (self.bytes_transferred as f64 / self.size as f64) * 100.0
    }
}

/// Memory usage statistics
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    /// Approximate memory used by active transfers (bytes)
    pub transfer_memory: u64,
    /// Approximate memory used by packet queues (bytes)
    pub queue_memory: u64,
    /// Total estimated memory usage (bytes)
    pub total_memory: u64,
}

impl MemoryStats {
    /// Update total memory usage
    pub fn update_total(&mut self) {
        self.total_memory = self.transfer_memory + self.queue_memory;
    }

    /// Check if under memory pressure
    pub fn is_under_pressure(&self, threshold: u64) -> bool {
        self.total_memory >= threshold
    }
}

/// Resource manager for tracking and limiting resource usage
pub struct ResourceManager {
    /// Configuration
    config: ResourceConfig,
    /// Active connections (connection_id -> info)
    connections: Arc<RwLock<HashMap<String, ConnectionInfo>>>,
    /// Active transfers (transfer_id -> info)
    transfers: Arc<RwLock<HashMap<String, TransferInfo>>>,
    /// Packet queue sizes per device (device_id -> queue_size)
    queue_sizes: Arc<RwLock<HashMap<String, usize>>>,
    /// Memory usage statistics
    memory_stats: Arc<RwLock<MemoryStats>>,
}

impl ResourceManager {
    /// Create a new resource manager
    pub fn new(config: ResourceConfig) -> Self {
        Self {
            config,
            connections: Arc::new(RwLock::new(HashMap::new())),
            transfers: Arc::new(RwLock::new(HashMap::new())),
            queue_sizes: Arc::new(RwLock::new(HashMap::new())),
            memory_stats: Arc::new(RwLock::new(MemoryStats::default())),
        }
    }

    /// Check if a new connection can be accepted
    pub async fn can_accept_connection(&self, device_id: &str) -> Result<()> {
        let connections = self.connections.read().await;

        // Check total connection limit
        if connections.len() >= self.config.max_total_connections {
            return Err(ProtocolError::ResourceExhausted(format!(
                "Maximum total connections ({}) reached",
                self.config.max_total_connections
            )));
        }

        // Check per-device connection limit
        let device_connections = connections
            .values()
            .filter(|c| c.device_id == device_id)
            .count();

        if device_connections >= self.config.max_connections_per_device {
            return Err(ProtocolError::ResourceExhausted(format!(
                "Maximum connections per device ({}) reached for {}",
                self.config.max_connections_per_device, device_id
            )));
        }

        Ok(())
    }

    /// Register a new connection
    pub async fn register_connection(
        &self,
        connection_id: String,
        device_id: String,
    ) -> Result<()> {
        self.can_accept_connection(&device_id).await?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs();

        let info = ConnectionInfo {
            device_id: device_id.clone(),
            connected_at: now,
            last_activity: now,
        };

        let mut connections = self.connections.write().await;
        connections.insert(connection_id.clone(), info);
        drop(connections);

        debug!(
            "Registered connection {} for device {} ({} total)",
            connection_id,
            device_id,
            self.get_connection_count().await
        );

        Ok(())
    }

    /// Unregister a connection
    pub async fn unregister_connection(&self, connection_id: &str) {
        let mut connections = self.connections.write().await;
        if let Some(info) = connections.remove(connection_id) {
            debug!(
                "Unregistered connection {} for device {} ({} remaining)",
                connection_id,
                info.device_id,
                connections.len()
            );
        }
    }

    /// Update connection activity timestamp
    pub async fn update_connection_activity(&self, connection_id: &str) {
        let mut connections = self.connections.write().await;
        if let Some(info) = connections.get_mut(connection_id) {
            info.last_activity = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| Duration::from_secs(0))
                .as_secs();
        }
    }

    /// Get total connection count
    pub async fn get_connection_count(&self) -> usize {
        self.connections.read().await.len()
    }

    /// Get connection count for a specific device
    pub async fn get_device_connection_count(&self, device_id: &str) -> usize {
        let connections = self.connections.read().await;
        connections
            .values()
            .filter(|c| c.device_id == device_id)
            .count()
    }

    /// Check if a new file transfer can be started
    pub async fn can_start_transfer(&self, device_id: &str, size: u64) -> Result<()> {
        let transfers = self.transfers.read().await;

        // Check if transfer size is within limits
        if size > self.config.max_transfer_size {
            return Err(ProtocolError::ResourceExhausted(format!(
                "File size ({} bytes) exceeds maximum allowed ({} bytes)",
                size, self.config.max_transfer_size
            )));
        }

        // Check total concurrent transfer limit
        if transfers.len() >= self.config.max_concurrent_transfers {
            return Err(ProtocolError::ResourceExhausted(format!(
                "Maximum concurrent transfers ({}) reached",
                self.config.max_concurrent_transfers
            )));
        }

        // Check per-device transfer limit
        let device_transfers = transfers
            .values()
            .filter(|t| t.device_id == device_id)
            .count();

        if device_transfers >= self.config.max_transfers_per_device {
            return Err(ProtocolError::ResourceExhausted(format!(
                "Maximum transfers per device ({}) reached for {}",
                self.config.max_transfers_per_device, device_id
            )));
        }

        // Check total transfer size limit
        let total_size: u64 = transfers.values().map(|t| t.size).sum();
        if total_size + size > self.config.max_total_transfer_size {
            return Err(ProtocolError::ResourceExhausted(format!(
                "Total transfer size limit ({} bytes) would be exceeded",
                self.config.max_total_transfer_size
            )));
        }

        Ok(())
    }

    /// Register a new file transfer
    pub async fn register_transfer(&self, transfer_info: TransferInfo) -> Result<()> {
        self.can_start_transfer(&transfer_info.device_id, transfer_info.size)
            .await?;

        let transfer_id = transfer_info.transfer_id.clone();
        let device_id = transfer_info.device_id.clone();
        let size = transfer_info.size;

        let mut transfers = self.transfers.write().await;
        transfers.insert(transfer_id.clone(), transfer_info);
        drop(transfers);

        // Update memory stats
        let mut stats = self.memory_stats.write().await;
        stats.transfer_memory += size;
        stats.update_total();
        drop(stats);

        debug!(
            "Registered transfer {} for device {} ({} bytes, {} active)",
            transfer_id,
            device_id,
            size,
            self.get_transfer_count().await
        );

        // Check for memory pressure
        self.check_memory_pressure().await;

        Ok(())
    }

    /// Unregister a file transfer
    pub async fn unregister_transfer(&self, transfer_id: &str) {
        let mut transfers = self.transfers.write().await;
        if let Some(info) = transfers.remove(transfer_id) {
            let size = info.size;
            let device_id = info.device_id.clone();
            drop(transfers);

            // Update memory stats
            let mut stats = self.memory_stats.write().await;
            stats.transfer_memory = stats.transfer_memory.saturating_sub(size);
            stats.update_total();

            debug!(
                "Unregistered transfer {} for device {} ({} bytes, {} remaining)",
                transfer_id,
                device_id,
                size,
                self.get_transfer_count().await
            );
        }
    }

    /// Update transfer progress
    pub async fn update_transfer_progress(&self, transfer_id: &str, bytes: u64) {
        let mut transfers = self.transfers.write().await;
        if let Some(info) = transfers.get_mut(transfer_id) {
            info.update_progress(bytes);
        }
    }

    /// Get transfer count
    pub async fn get_transfer_count(&self) -> usize {
        self.transfers.read().await.len()
    }

    /// Get transfer count for a specific device
    pub async fn get_device_transfer_count(&self, device_id: &str) -> usize {
        let transfers = self.transfers.read().await;
        transfers
            .values()
            .filter(|t| t.device_id == device_id)
            .count()
    }

    /// Get all active transfers
    pub async fn get_active_transfers(&self) -> Vec<TransferInfo> {
        self.transfers.read().await.values().cloned().collect()
    }

    /// Check if packet queue can accept more packets
    pub async fn can_queue_packet(&self, device_id: &str) -> Result<()> {
        let queue_sizes = self.queue_sizes.read().await;
        let current_size = queue_sizes.get(device_id).copied().unwrap_or(0);

        if current_size >= self.config.max_packet_queue_size {
            return Err(ProtocolError::ResourceExhausted(format!(
                "Packet queue full for device {} ({} packets)",
                device_id, self.config.max_packet_queue_size
            )));
        }

        Ok(())
    }

    /// Increment packet queue size
    pub async fn increment_queue_size(&self, device_id: &str) -> Result<()> {
        self.can_queue_packet(device_id).await?;

        let mut queue_sizes = self.queue_sizes.write().await;
        let size = queue_sizes.entry(device_id.to_string()).or_insert(0);
        *size += 1;

        // Update memory stats (estimate ~1KB per packet)
        let mut stats = self.memory_stats.write().await;
        stats.queue_memory += 1024;
        stats.update_total();

        debug!("Packet queue for device {}: {} packets", device_id, *size);

        Ok(())
    }

    /// Decrement packet queue size
    pub async fn decrement_queue_size(&self, device_id: &str) {
        let mut queue_sizes = self.queue_sizes.write().await;
        if let Some(size) = queue_sizes.get_mut(device_id) {
            *size = size.saturating_sub(1);

            // Update memory stats
            let mut stats = self.memory_stats.write().await;
            stats.queue_memory = stats.queue_memory.saturating_sub(1024);
            stats.update_total();

            if *size == 0 {
                queue_sizes.remove(device_id);
            }
        }
    }

    /// Get packet queue size for a device
    pub async fn get_queue_size(&self, device_id: &str) -> usize {
        self.queue_sizes
            .read()
            .await
            .get(device_id)
            .copied()
            .unwrap_or(0)
    }

    /// Get memory usage statistics
    pub async fn get_memory_stats(&self) -> MemoryStats {
        self.memory_stats.read().await.clone()
    }

    /// Check for memory pressure and warn if needed
    async fn check_memory_pressure(&self) {
        let stats = self.memory_stats.read().await;
        if stats.is_under_pressure(self.config.memory_pressure_threshold) {
            warn!(
                "Memory pressure detected: {} MB used (threshold: {} MB)",
                stats.total_memory / (1024 * 1024),
                self.config.memory_pressure_threshold / (1024 * 1024)
            );
        }
    }

    /// Get resource usage summary
    pub async fn get_resource_summary(&self) -> String {
        let connections = self.get_connection_count().await;
        let transfers = self.get_transfer_count().await;
        let stats = self.get_memory_stats().await;

        format!(
            "Connections: {}/{}, Transfers: {}/{}, Memory: {} MB / {} MB",
            connections,
            self.config.max_total_connections,
            transfers,
            self.config.max_concurrent_transfers,
            stats.total_memory / (1024 * 1024),
            self.config.memory_pressure_threshold / (1024 * 1024)
        )
    }

    /// Clean up stale connections (no activity for 5+ minutes)
    pub async fn cleanup_stale_connections(&self, max_idle_seconds: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs();

        let mut connections = self.connections.write().await;
        let original_count = connections.len();

        connections.retain(|id, info| {
            let idle_time = now.saturating_sub(info.last_activity);
            if idle_time > max_idle_seconds {
                debug!(
                    "Removing stale connection {} (idle for {} seconds)",
                    id, idle_time
                );
                false
            } else {
                true
            }
        });

        let removed = original_count - connections.len();
        if removed > 0 {
            info!("Cleaned up {} stale connections", removed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_limits() {
        let mut config = ResourceConfig::default();
        config.max_connections_per_device = 2;
        config.max_total_connections = 5;

        let manager = ResourceManager::new(config);

        // Register 2 connections for device-1 (at limit)
        assert!(manager
            .register_connection("conn-1".to_string(), "device-1".to_string())
            .await
            .is_ok());
        assert!(manager
            .register_connection("conn-2".to_string(), "device-1".to_string())
            .await
            .is_ok());

        // Third connection to device-1 should fail
        assert!(manager
            .register_connection("conn-3".to_string(), "device-1".to_string())
            .await
            .is_err());

        // Connection to different device should succeed
        assert!(manager
            .register_connection("conn-4".to_string(), "device-2".to_string())
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_transfer_limits() {
        let mut config = ResourceConfig::default();
        config.max_concurrent_transfers = 2;
        config.max_transfer_size = 1000;
        config.max_total_transfer_size = 2000;

        let manager = ResourceManager::new(config);

        // Register first transfer
        let transfer1 = TransferInfo::new("t1".to_string(), "device-1".to_string(), 800);
        assert!(manager.register_transfer(transfer1).await.is_ok());

        // Register second transfer
        let transfer2 = TransferInfo::new("t2".to_string(), "device-2".to_string(), 800);
        assert!(manager.register_transfer(transfer2).await.is_ok());

        // Third transfer should fail (exceeds max concurrent)
        let transfer3 = TransferInfo::new("t3".to_string(), "device-3".to_string(), 500);
        assert!(manager.register_transfer(transfer3).await.is_err());

        // Unregister one transfer
        manager.unregister_transfer("t1").await;

        // Now third transfer should succeed (but would exceed total size)
        let transfer4 = TransferInfo::new("t4".to_string(), "device-3".to_string(), 500);
        assert!(manager.register_transfer(transfer4).await.is_err()); // Total size: 800 + 500 > 2000
    }

    #[tokio::test]
    async fn test_packet_queue_limits() {
        let mut config = ResourceConfig::default();
        config.max_packet_queue_size = 3;

        let manager = ResourceManager::new(config);

        // Add packets up to limit
        assert!(manager.increment_queue_size("device-1").await.is_ok());
        assert!(manager.increment_queue_size("device-1").await.is_ok());
        assert!(manager.increment_queue_size("device-1").await.is_ok());

        // Fourth packet should fail
        assert!(manager.increment_queue_size("device-1").await.is_err());

        // Decrement and try again
        manager.decrement_queue_size("device-1").await;
        assert!(manager.increment_queue_size("device-1").await.is_ok());
    }

    #[tokio::test]
    async fn test_memory_pressure() {
        let mut config = ResourceConfig::default();
        config.memory_pressure_threshold = 2000; // 2KB threshold

        let manager = ResourceManager::new(config);

        // Register large transfer
        let transfer = TransferInfo::new("t1".to_string(), "device-1".to_string(), 3000);
        manager.register_transfer(transfer).await.unwrap();

        let stats = manager.get_memory_stats().await;
        assert!(stats.is_under_pressure(2000));
    }

    #[tokio::test]
    async fn test_transfer_progress() {
        let config = ResourceConfig::default();
        let manager = ResourceManager::new(config);

        let transfer = TransferInfo::new("t1".to_string(), "device-1".to_string(), 1000);
        manager.register_transfer(transfer).await.unwrap();

        manager.update_transfer_progress("t1", 500).await;

        let transfers = manager.get_active_transfers().await;
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].bytes_transferred, 500);
        assert_eq!(transfers[0].progress_percentage(), 50.0);
    }
}
