//! Auto-Recovery Mechanisms
//!
//! Provides automatic recovery from connection failures, including:
//! - Automatic reconnection with exponential backoff
//! - Packet retry with limits
//! - Transfer state tracking for resumption
//! - State persistence for daemon crash recovery

use crate::{Device, DeviceManager, Packet, ProtocolError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, info, warn};

/// Maximum number of reconnection attempts before giving up
const MAX_RECONNECT_ATTEMPTS: u32 = 5;

/// Initial reconnection delay
const INITIAL_RECONNECT_DELAY: Duration = Duration::from_secs(2);

/// Maximum reconnection delay
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);

/// Maximum number of packet retry attempts
const MAX_PACKET_RETRIES: u32 = 3;

/// Packet retry delay
const PACKET_RETRY_DELAY: Duration = Duration::from_millis(500);

/// Reconnection strategy with exponential backoff
#[derive(Debug, Clone)]
pub struct ReconnectionStrategy {
    /// Current reconnection attempt number
    pub attempt: u32,
    /// Maximum number of attempts
    pub max_attempts: u32,
    /// Current delay between attempts
    pub current_delay: Duration,
    /// Maximum delay between attempts
    pub max_delay: Duration,
}

impl Default for ReconnectionStrategy {
    fn default() -> Self {
        Self {
            attempt: 0,
            max_attempts: MAX_RECONNECT_ATTEMPTS,
            current_delay: INITIAL_RECONNECT_DELAY,
            max_delay: MAX_RECONNECT_DELAY,
        }
    }
}

impl ReconnectionStrategy {
    /// Create a new reconnection strategy
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset the strategy
    pub fn reset(&mut self) {
        self.attempt = 0;
        self.current_delay = INITIAL_RECONNECT_DELAY;
    }

    /// Check if more attempts are available
    pub fn has_attempts_remaining(&self) -> bool {
        self.attempt < self.max_attempts
    }

    /// Get next delay with exponential backoff
    pub fn next_delay(&mut self) -> Option<Duration> {
        if !self.has_attempts_remaining() {
            return None;
        }

        let delay = self.current_delay;
        self.attempt += 1;

        // Exponential backoff: double the delay each time
        self.current_delay = std::cmp::min(self.current_delay * 2, self.max_delay);

        Some(delay)
    }

    /// Get human-readable status
    pub fn status(&self) -> String {
        format!(
            "Attempt {}/{}, next delay: {:?}",
            self.attempt, self.max_attempts, self.current_delay
        )
    }
}

/// State of an in-progress file transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferState {
    /// Transfer ID (unique identifier)
    pub transfer_id: String,
    /// Device ID
    pub device_id: String,
    /// File name
    pub filename: String,
    /// Full file path where file is being saved
    pub file_path: PathBuf,
    /// Expected total size in bytes
    pub total_size: u64,
    /// Bytes received so far
    pub bytes_received: u64,
    /// Transfer start timestamp
    pub started_at: u64,
    /// Last update timestamp
    pub last_updated: u64,
}

impl TransferState {
    /// Create a new transfer state
    pub fn new(
        transfer_id: String,
        device_id: String,
        filename: String,
        file_path: PathBuf,
        total_size: u64,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs();

        Self {
            transfer_id,
            device_id,
            filename,
            file_path,
            total_size,
            bytes_received: 0,
            started_at: now,
            last_updated: now,
        }
    }

    /// Update bytes received
    pub fn update_progress(&mut self, bytes: u64) {
        self.bytes_received = bytes;
        self.last_updated = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs();
    }

    /// Check if transfer is complete
    pub fn is_complete(&self) -> bool {
        self.bytes_received >= self.total_size
    }

    /// Get transfer progress percentage
    pub fn progress_percentage(&self) -> f64 {
        if self.total_size == 0 {
            return 0.0;
        }
        (self.bytes_received as f64 / self.total_size as f64) * 100.0
    }
}

/// Packet retry queue entry
#[derive(Debug, Clone)]
struct PacketRetryEntry {
    /// The packet to retry
    packet: Packet,
    /// Device ID to send to
    device_id: String,
    /// Current retry attempt
    attempt: u32,
    /// Maximum retry attempts
    max_attempts: u32,
}

/// Recovery manager for handling connection and transfer recovery
pub struct RecoveryManager {
    /// Reconnection strategies per device
    reconnection_strategies: Arc<RwLock<HashMap<String, ReconnectionStrategy>>>,
    /// Active file transfer states
    transfer_states: Arc<RwLock<HashMap<String, TransferState>>>,
    /// Packet retry queue
    retry_queue: Arc<RwLock<Vec<PacketRetryEntry>>>,
    /// Path to state persistence file
    state_file_path: PathBuf,
}

impl RecoveryManager {
    /// Create a new recovery manager
    pub fn new(state_dir: impl AsRef<Path>) -> Self {
        let state_file_path = state_dir.as_ref().join("recovery_state.json");

        Self {
            reconnection_strategies: Arc::new(RwLock::new(HashMap::new())),
            transfer_states: Arc::new(RwLock::new(HashMap::new())),
            retry_queue: Arc::new(RwLock::new(Vec::new())),
            state_file_path,
        }
    }

    /// Initialize recovery manager and restore state
    pub async fn init(&self) -> Result<()> {
        debug!("Initializing recovery manager");

        // Try to restore transfer states from disk
        if let Err(e) = self.restore_transfer_states().await {
            warn!("Failed to restore transfer states: {}", e);
        }

        info!("Recovery manager initialized");
        Ok(())
    }

    /// Get or create a reconnection strategy for a device
    pub async fn get_reconnection_strategy(&self, device_id: &str) -> ReconnectionStrategy {
        let mut strategies = self.reconnection_strategies.write().await;
        strategies
            .entry(device_id.to_string())
            .or_insert_with(ReconnectionStrategy::new)
            .clone()
    }

    /// Reset reconnection strategy for a device (called on successful connection)
    pub async fn reset_reconnection_strategy(&self, device_id: &str) {
        let mut strategies = self.reconnection_strategies.write().await;
        if let Some(strategy) = strategies.get_mut(device_id) {
            strategy.reset();
            debug!("Reset reconnection strategy for device {}", device_id);
        }
    }

    /// Attempt reconnection with exponential backoff
    ///
    /// Returns true if reconnection should be attempted, false if max attempts reached
    pub async fn should_reconnect(&self, device_id: &str) -> Option<Duration> {
        let mut strategies = self.reconnection_strategies.write().await;
        let strategy = strategies
            .entry(device_id.to_string())
            .or_insert_with(ReconnectionStrategy::new);

        if let Some(delay) = strategy.next_delay() {
            info!(
                "Scheduling reconnection for device {} - {}",
                device_id,
                strategy.status()
            );
            Some(delay)
        } else {
            warn!(
                "Max reconnection attempts ({}) reached for device {}",
                strategy.max_attempts, device_id
            );
            None
        }
    }

    /// Register a new file transfer
    pub async fn register_transfer(&self, state: TransferState) -> Result<()> {
        let transfer_id = state.transfer_id.clone();
        let mut states = self.transfer_states.write().await;
        states.insert(transfer_id.clone(), state);
        drop(states);

        // Persist state to disk
        self.persist_transfer_states().await?;

        debug!("Registered transfer {}", transfer_id);
        Ok(())
    }

    /// Update transfer progress
    pub async fn update_transfer_progress(
        &self,
        transfer_id: &str,
        bytes_received: u64,
    ) -> Result<()> {
        let mut states = self.transfer_states.write().await;
        if let Some(state) = states.get_mut(transfer_id) {
            state.update_progress(bytes_received);
            debug!(
                "Updated transfer {} progress: {:.1}%",
                transfer_id,
                state.progress_percentage()
            );
        }
        drop(states);

        // Persist state to disk
        self.persist_transfer_states().await?;

        Ok(())
    }

    /// Complete a transfer (remove from active states)
    pub async fn complete_transfer(&self, transfer_id: &str) -> Result<()> {
        let mut states = self.transfer_states.write().await;
        if let Some(state) = states.remove(transfer_id) {
            info!(
                "Transfer {} completed: {} bytes received",
                transfer_id, state.bytes_received
            );
        }
        drop(states);

        // Persist state to disk
        self.persist_transfer_states().await?;

        Ok(())
    }

    /// Get transfer state by ID
    pub async fn get_transfer_state(&self, transfer_id: &str) -> Option<TransferState> {
        let states = self.transfer_states.read().await;
        states.get(transfer_id).cloned()
    }

    /// Get all active transfers for a device
    pub async fn get_device_transfers(&self, device_id: &str) -> Vec<TransferState> {
        let states = self.transfer_states.read().await;
        states
            .values()
            .filter(|s| s.device_id == device_id)
            .cloned()
            .collect()
    }

    /// Queue a packet for retry
    pub async fn queue_packet_retry(&self, device_id: String, packet: Packet) {
        let entry = PacketRetryEntry {
            packet: packet.clone(),
            device_id: device_id.clone(),
            attempt: 0,
            max_attempts: MAX_PACKET_RETRIES,
        };

        let mut queue = self.retry_queue.write().await;
        queue.push(entry);
        debug!(
            "Queued packet '{}' for retry to device {}",
            packet.packet_type, device_id
        );
    }

    /// Process retry queue and attempt to resend failed packets
    ///
    /// Returns packets that should be retried with their device IDs
    pub async fn process_retry_queue(&self) -> Vec<(String, Packet)> {
        let mut queue = self.retry_queue.write().await;
        let mut to_retry = Vec::new();
        let mut remaining = Vec::new();

        for mut entry in queue.drain(..) {
            entry.attempt += 1;

            if entry.attempt <= entry.max_attempts {
                debug!(
                    "Retrying packet '{}' to device {} (attempt {}/{})",
                    entry.packet.packet_type,
                    entry.device_id,
                    entry.attempt,
                    entry.max_attempts
                );
                to_retry.push((entry.device_id.clone(), entry.packet.clone()));
                remaining.push(entry);
            } else {
                warn!(
                    "Dropping packet '{}' to device {} after {} failed attempts",
                    entry.packet.packet_type, entry.device_id, entry.max_attempts
                );
            }
        }

        *queue = remaining;
        to_retry
    }

    /// Clear retry queue for a specific device (called on successful reconnection)
    pub async fn clear_device_retry_queue(&self, device_id: &str) {
        let mut queue = self.retry_queue.write().await;
        queue.retain(|entry| entry.device_id != device_id);
        debug!("Cleared retry queue for device {}", device_id);
    }

    /// Persist transfer states to disk
    async fn persist_transfer_states(&self) -> Result<()> {
        let states = self.transfer_states.read().await;
        let states_vec: Vec<&TransferState> = states.values().collect();

        let json = serde_json::to_string_pretty(&states_vec)
            .map_err(|e| ProtocolError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        // Ensure parent directory exists
        if let Some(parent) = self.state_file_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                ProtocolError::from_io_error(
                    e,
                    &format!("creating recovery state directory {}", parent.display()),
                )
            })?;
        }

        fs::write(&self.state_file_path, json)
            .await
            .map_err(|e| {
                ProtocolError::from_io_error(
                    e,
                    &format!("writing recovery state file {}", self.state_file_path.display()),
                )
            })?;

        debug!(
            "Persisted {} transfer states to disk",
            states_vec.len()
        );
        Ok(())
    }

    /// Restore transfer states from disk
    async fn restore_transfer_states(&self) -> Result<()> {
        if !self.state_file_path.exists() {
            debug!("No recovery state file found, starting fresh");
            return Ok(());
        }

        let json = fs::read_to_string(&self.state_file_path)
            .await
            .map_err(|e| {
                ProtocolError::from_io_error(
                    e,
                    &format!("reading recovery state file {}", self.state_file_path.display()),
                )
            })?;

        let states_vec: Vec<TransferState> = serde_json::from_str(&json)
            .map_err(|e| ProtocolError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        let mut states = self.transfer_states.write().await;
        for state in states_vec {
            info!(
                "Restored transfer state: {} ({:.1}% complete)",
                state.transfer_id,
                state.progress_percentage()
            );
            states.insert(state.transfer_id.clone(), state);
        }

        info!("Restored {} transfer states from disk", states.len());
        Ok(())
    }

    /// Clean up old transfer states (transfers older than 24 hours)
    pub async fn cleanup_old_transfers(&self) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_secs();
        let cutoff = now.saturating_sub(24 * 60 * 60); // 24 hours ago

        let mut states = self.transfer_states.write().await;
        let original_count = states.len();
        states.retain(|_, state| state.last_updated >= cutoff);
        let removed_count = original_count - states.len();

        if removed_count > 0 {
            info!("Cleaned up {} old transfer states", removed_count);
            drop(states);
            self.persist_transfer_states().await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_reconnection_strategy() {
        let mut strategy = ReconnectionStrategy::new();

        // First attempt
        assert!(strategy.has_attempts_remaining());
        let delay1 = strategy.next_delay();
        assert!(delay1.is_some());
        assert_eq!(delay1.unwrap(), INITIAL_RECONNECT_DELAY);
        assert_eq!(strategy.attempt, 1);

        // Second attempt (should be doubled)
        let delay2 = strategy.next_delay();
        assert!(delay2.is_some());
        assert_eq!(delay2.unwrap(), INITIAL_RECONNECT_DELAY * 2);
        assert_eq!(strategy.attempt, 2);

        // Continue until max attempts
        while strategy.has_attempts_remaining() {
            strategy.next_delay();
        }

        // Should be exhausted
        assert!(!strategy.has_attempts_remaining());
        assert!(strategy.next_delay().is_none());
    }

    #[test]
    fn test_reconnection_strategy_reset() {
        let mut strategy = ReconnectionStrategy::new();

        // Use some attempts
        strategy.next_delay();
        strategy.next_delay();
        assert_eq!(strategy.attempt, 2);

        // Reset
        strategy.reset();
        assert_eq!(strategy.attempt, 0);
        assert_eq!(strategy.current_delay, INITIAL_RECONNECT_DELAY);
    }

    #[test]
    fn test_transfer_state() {
        let mut state = TransferState::new(
            "transfer-1".to_string(),
            "device-1".to_string(),
            "test.txt".to_string(),
            PathBuf::from("/tmp/test.txt"),
            1000,
        );

        assert_eq!(state.bytes_received, 0);
        assert!(!state.is_complete());
        assert_eq!(state.progress_percentage(), 0.0);

        // Update progress
        state.update_progress(500);
        assert_eq!(state.bytes_received, 500);
        assert_eq!(state.progress_percentage(), 50.0);

        // Complete
        state.update_progress(1000);
        assert!(state.is_complete());
        assert_eq!(state.progress_percentage(), 100.0);
    }

    #[tokio::test]
    async fn test_recovery_manager_transfer_tracking() {
        let temp_dir = TempDir::new().unwrap();
        let manager = RecoveryManager::new(temp_dir.path());
        manager.init().await.unwrap();

        // Register transfer
        let state = TransferState::new(
            "transfer-1".to_string(),
            "device-1".to_string(),
            "test.txt".to_string(),
            PathBuf::from("/tmp/test.txt"),
            1000,
        );
        manager.register_transfer(state).await.unwrap();

        // Get transfer
        let retrieved = manager.get_transfer_state("transfer-1").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().transfer_id, "transfer-1");

        // Update progress
        manager
            .update_transfer_progress("transfer-1", 500)
            .await
            .unwrap();

        let updated = manager.get_transfer_state("transfer-1").await;
        assert_eq!(updated.unwrap().bytes_received, 500);

        // Complete transfer
        manager.complete_transfer("transfer-1").await.unwrap();
        assert!(manager.get_transfer_state("transfer-1").await.is_none());
    }

    #[tokio::test]
    async fn test_recovery_manager_packet_retry() {
        let temp_dir = TempDir::new().unwrap();
        let manager = RecoveryManager::new(temp_dir.path());
        manager.init().await.unwrap();

        // Queue packet
        let packet = Packet::new("cconnect.ping", serde_json::json!({}));
        manager
            .queue_packet_retry("device-1".to_string(), packet.clone())
            .await;

        // Process queue
        let to_retry = manager.process_retry_queue().await;
        assert_eq!(to_retry.len(), 1);
        assert_eq!(to_retry[0].0, "device-1");
        assert_eq!(to_retry[0].1.packet_type, "cconnect.ping");
    }

    #[tokio::test]
    async fn test_recovery_manager_persistence() {
        let temp_dir = TempDir::new().unwrap();

        // Create manager and register transfer
        {
            let manager = RecoveryManager::new(temp_dir.path());
            manager.init().await.unwrap();

            let state = TransferState::new(
                "transfer-1".to_string(),
                "device-1".to_string(),
                "test.txt".to_string(),
                PathBuf::from("/tmp/test.txt"),
                1000,
            );
            manager.register_transfer(state).await.unwrap();
        }

        // Create new manager and restore
        {
            let manager = RecoveryManager::new(temp_dir.path());
            manager.init().await.unwrap();

            let restored = manager.get_transfer_state("transfer-1").await;
            assert!(restored.is_some());
            assert_eq!(restored.unwrap().filename, "test.txt");
        }
    }
}
