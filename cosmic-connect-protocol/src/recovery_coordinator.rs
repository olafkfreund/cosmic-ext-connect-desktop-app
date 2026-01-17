//! Recovery Coordinator
//!
//! Coordinates automatic recovery actions in response to connection events.
//! This module acts as a bridge between the ConnectionManager and RecoveryManager,
//! listening for connection failures and triggering appropriate recovery actions.

use crate::{
    ConnectionEvent, ConnectionManager, DeviceManager, RecoveryManager, Result,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, info, warn};

/// Recovery coordinator that handles automatic reconnection
pub struct RecoveryCoordinator {
    /// Connection manager for initiating reconnections
    connection_manager: Arc<ConnectionManager>,
    /// Device manager for device info
    device_manager: Arc<RwLock<DeviceManager>>,
    /// Recovery manager for reconnection strategies
    recovery_manager: Arc<RecoveryManager>,
}

impl RecoveryCoordinator {
    /// Create a new recovery coordinator
    pub fn new(
        connection_manager: Arc<ConnectionManager>,
        device_manager: Arc<RwLock<DeviceManager>>,
        recovery_manager: Arc<RecoveryManager>,
    ) -> Self {
        Self {
            connection_manager,
            device_manager,
            recovery_manager,
        }
    }

    /// Start coordinating recovery actions based on connection events
    ///
    /// This spawns a background task that listens for disconnection events
    /// and triggers automatic reconnection for paired devices.
    pub async fn start(&self) -> Result<()> {
        info!("Starting recovery coordinator");

        let mut event_rx = self.connection_manager.subscribe().await;
        let device_manager = self.device_manager.clone();
        let recovery_manager = self.recovery_manager.clone();
        let connection_manager = self.connection_manager.clone();

        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                match event {
                    ConnectionEvent::Connected { device_id, .. } => {
                        // Reset reconnection strategy on successful connection
                        recovery_manager
                            .reset_reconnection_strategy(&device_id)
                            .await;
                        debug!("Reset reconnection strategy for {}", device_id);

                        // Clear retry queue for this device
                        recovery_manager.clear_device_retry_queue(&device_id).await;
                    }

                    ConnectionEvent::Disconnected { device_id, reason } => {
                        info!(
                            "Device {} disconnected: {}",
                            device_id,
                            reason.as_deref().unwrap_or("unknown reason")
                        );

                        // Check if device is paired (only auto-reconnect to paired devices)
                        let dm = device_manager.read().await;
                        let should_reconnect = if let Some(device) = dm.get_device(&device_id) {
                            device.is_paired() && device.is_trusted
                        } else {
                            false
                        };
                        drop(dm);

                        if !should_reconnect {
                            debug!(
                                "Skipping auto-reconnect for device {} (not paired or trusted)",
                                device_id
                            );
                            continue;
                        }

                        // Get reconnection delay with exponential backoff
                        if let Some(delay) = recovery_manager.should_reconnect(&device_id).await {
                            info!(
                                "Scheduling reconnection for device {} after {:?}",
                                device_id, delay
                            );

                            // Spawn reconnection task with delay
                            let device_id_clone = device_id.clone();
                            let device_manager_clone = device_manager.clone();
                            let connection_manager_clone = connection_manager.clone();

                            tokio::spawn(async move {
                                // Wait for backoff delay
                                sleep(delay).await;

                                // Get device info for connection
                                let (host_opt, port_opt) = {
                                    let dm = device_manager_clone.read().await;
                                    if let Some(device) = dm.get_device(&device_id_clone) {
                                        (device.host.clone(), device.port)
                                    } else {
                                        (None, None)
                                    }
                                };

                                if let (Some(host), Some(port)) = (host_opt, port_opt) {
                                        info!(
                                            "Attempting reconnection to device {} at {}:{}",
                                            device_id_clone, host, port
                                        );

                                        // Parse socket address
                                        let addr_str = format!("{}:{}", host, port);
                                        if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                                            // Attempt reconnection
                                            match connection_manager_clone
                                                .connect(&device_id_clone, addr)
                                                .await
                                            {
                                                Ok(_) => {
                                                    info!(
                                                        "Successfully reconnected to device {}",
                                                        device_id_clone
                                                    );
                                                }
                                                Err(e) => {
                                                    warn!(
                                                        "Failed to reconnect to device {}: {}",
                                                        device_id_clone, e
                                                    );
                                                    // The next disconnection event will trigger another attempt
                                                }
                                            }
                                        } else {
                                            warn!(
                                                "Invalid address {}:{} for device {}",
                                                host, port, device_id_clone
                                            );
                                        }
                                } else {
                                    debug!(
                                        "Device {} has no host/port info, cannot reconnect",
                                        device_id_clone
                                    );
                                }
                            });
                        } else {
                            warn!(
                                "Max reconnection attempts reached for device {}, giving up",
                                device_id
                            );
                        }
                    }

                    ConnectionEvent::ConnectionError { device_id, message } => {
                        if let Some(id) = device_id {
                            warn!("Connection error for device {}: {}", id, message);
                            // Connection errors are handled like disconnections
                            // The error will typically be followed by a Disconnected event
                        } else {
                            warn!("Connection error: {}", message);
                        }
                    }

                    _ => {
                        // Ignore other events
                    }
                }
            }

            info!("Recovery coordinator stopped");
        });

        info!("Recovery coordinator started");
        Ok(())
    }

    /// Process packet retry queue
    ///
    /// This should be called periodically to retry failed packet sends
    pub async fn process_packet_retries(&self) -> Result<()> {
        let to_retry = self.recovery_manager.process_retry_queue().await;

        for (device_id, packet) in to_retry {
            debug!("Retrying packet '{}' to device {}", packet.packet_type, device_id);

            if let Err(e) = self
                .connection_manager
                .send_packet(&device_id, &packet)
                .await
            {
                warn!(
                    "Failed to retry packet '{}' to device {}: {}",
                    packet.packet_type, device_id, e
                );
                // Packet will be retried again on next process_packet_retries call
                // unless max retries reached
            } else {
                debug!("Successfully retried packet '{}' to device {}", packet.packet_type, device_id);
            }
        }

        Ok(())
    }

    /// Clean up old transfers (should be called periodically, e.g., daily)
    pub async fn cleanup_old_transfers(&self) -> Result<()> {
        self.recovery_manager.cleanup_old_transfers().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CertificateInfo, ConnectionConfig, DeviceInfo, DeviceType};
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_recovery_coordinator_creation() {
        // Create test certificate
        let cert = CertificateInfo::generate_self_signed(
            "test-device",
            vec!["127.0.0.1".to_string()],
        )
        .unwrap();

        // Create test device info
        let device_info = DeviceInfo {
            device_id: "test-device".to_string(),
            device_name: "Test Device".to_string(),
            device_type: DeviceType::Desktop,
            protocol_version: 8,
            incoming_capabilities: vec![],
            outgoing_capabilities: vec![],
            tcp_port: 1716,
        };

        // Create managers
        let device_manager = Arc::new(RwLock::new(DeviceManager::new(None)));
        let connection_manager = Arc::new(
            ConnectionManager::new(
                cert,
                device_info,
                device_manager.clone(),
                ConnectionConfig::default(),
            )
            .unwrap(),
        );

        let temp_dir = tempfile::TempDir::new().unwrap();
        let recovery_manager = Arc::new(RecoveryManager::new(temp_dir.path()));
        recovery_manager.init().await.unwrap();

        // Create coordinator
        let _coordinator = RecoveryCoordinator::new(
            connection_manager,
            device_manager,
            recovery_manager,
        );

        // Just verify it can be created
        // Full integration testing requires running connection manager
    }
}
