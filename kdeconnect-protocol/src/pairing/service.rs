//! Pairing Service
//!
//! Manages pairing for multiple devices simultaneously.

use super::events::PairingEvent;
use super::handler::{LegacyCertificateInfo, PairingHandler, PairingStatus};
use crate::{DeviceInfo, Packet, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// Pairing timeout duration (30 seconds)
const PAIRING_TIMEOUT: Duration = Duration::from_secs(30);

/// Pairing request state
#[derive(Debug)]
struct PairingRequest {
    /// When the request was initiated
    started_at: Instant,
    /// Remote device information
    device_info: DeviceInfo,
    /// Remote address
    remote_addr: SocketAddr,
    /// Device certificate (PEM encoded)
    device_cert: Vec<u8>,
}

/// Pairing service configuration
#[derive(Debug, Clone)]
pub struct PairingConfig {
    /// Certificate storage directory
    pub cert_dir: PathBuf,
    /// Pairing timeout duration
    pub timeout: Duration,
}

impl Default for PairingConfig {
    fn default() -> Self {
        Self {
            cert_dir: PathBuf::from(".config/kdeconnect/certs"),
            timeout: PAIRING_TIMEOUT,
        }
    }
}

/// Pairing service for managing device pairing
pub struct PairingService {
    /// Our device certificate
    certificate: Arc<LegacyCertificateInfo>,

    /// Pairing handler
    handler: Arc<RwLock<PairingHandler>>,

    /// Active pairing requests (device_id -> request state)
    active_requests: Arc<RwLock<HashMap<String, PairingRequest>>>,

    /// Event channel sender
    event_tx: mpsc::UnboundedSender<PairingEvent>,

    /// Event channel receiver
    event_rx: Arc<RwLock<mpsc::UnboundedReceiver<PairingEvent>>>,

    /// Configuration
    config: PairingConfig,

    /// Connection manager for sending packets over TLS (Protocol v8)
    connection_manager: Option<Arc<RwLock<crate::connection::ConnectionManager>>>,
}

impl PairingService {
    /// Create a new pairing service
    pub fn new(device_id: impl Into<String>, config: PairingConfig) -> Result<Self> {
        let device_id = device_id.into();

        // Create pairing handler
        let handler = PairingHandler::new(device_id.clone(), &config.cert_dir)?;
        let certificate = handler.certificate().clone();

        let (event_tx, event_rx) = mpsc::unbounded_channel();

        Ok(Self {
            certificate: Arc::new(certificate),
            handler: Arc::new(RwLock::new(handler)),
            active_requests: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            event_rx: Arc::new(RwLock::new(event_rx)),
            config,
            connection_manager: None,
        })
    }

    /// Set the connection manager (called after initialization to avoid circular dependencies)
    pub fn set_connection_manager(&mut self, connection_manager: Arc<RwLock<crate::connection::ConnectionManager>>) {
        self.connection_manager = Some(connection_manager);
    }

    /// Get a receiver for pairing events
    pub async fn subscribe(&self) -> mpsc::UnboundedReceiver<PairingEvent> {
        let (tx, rx) = mpsc::unbounded_channel();

        // Forward events
        let event_rx = self.event_rx.clone();
        tokio::spawn(async move {
            let mut rx_lock = event_rx.write().await;
            while let Some(event) = rx_lock.recv().await {
                if tx.send(event).is_err() {
                    break;
                }
            }
        });

        rx
    }

    /// Get our device certificate
    pub fn certificate(&self) -> &LegacyCertificateInfo {
        &self.certificate
    }

    /// Get our certificate fingerprint
    pub fn fingerprint(&self) -> &str {
        &self.certificate.fingerprint
    }

    /// Request pairing with a device
    ///
    /// Sends pairing request packet and starts timeout tracking.
    pub async fn request_pairing(
        &self,
        device_info: DeviceInfo,
        remote_addr: SocketAddr,
    ) -> Result<()> {
        let device_id = device_info.device_id.clone();

        info!(
            "Requesting pairing with device {} ({})",
            device_info.device_name, device_id
        );

        // Check if already paired
        let handler = self.handler.read().await;
        if handler.is_paired(&device_id) {
            warn!("Device {} is already paired", device_id);
            return Ok(());
        }
        drop(handler);

        // Create pairing request packet
        let mut handler = self.handler.write().await;
        let packet = handler.request_pairing();
        drop(handler);

        // For Protocol v8 unpaired devices: Ensure we have an active connection
        // Unpaired devices disconnect immediately after identity exchange, so we may need to reconnect
        if let Some(conn_mgr) = &self.connection_manager {
            let conn_mgr = conn_mgr.read().await;
            let has_connection = conn_mgr.has_connection(&device_id).await;

            if !has_connection {
                info!(
                    "No active connection to {}, establishing connection for pairing",
                    device_id
                );

                // Drop the read lock before calling connect_with_cert (which needs write access)
                drop(conn_mgr);

                // Reconnect using TOFU (Trust On First Use) with empty certificate
                // The connection manager accepts any certificate for unpaired devices
                let conn_mgr = self.connection_manager.as_ref().unwrap().read().await;
                if let Err(e) = conn_mgr.connect_with_cert(&device_id, remote_addr, Vec::new()).await {
                    error!("Failed to establish connection for pairing: {}", e);
                    let _ = self.event_tx.send(PairingEvent::Error {
                        device_id: Some(device_id.clone()),
                        message: format!("Failed to connect for pairing: {}", e),
                    });
                    return Err(e);
                }

                // Give the connection a moment to fully establish
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }

        // Send request over TLS connection (Protocol v8)
        match self.send_pairing_packet(&packet, &device_id).await {
            Ok(_) => {
                // Track active request
                let mut requests = self.active_requests.write().await;
                requests.insert(
                    device_id.clone(),
                    PairingRequest {
                        started_at: Instant::now(),
                        device_info: device_info.clone(),
                        remote_addr,
                        device_cert: Vec::new(), // Will be received in response
                    },
                );
                drop(requests);

                // Emit event
                let _ = self.event_tx.send(PairingEvent::RequestSent {
                    device_id,
                    our_fingerprint: self.fingerprint().to_string(),
                });

                // Start timeout checker
                self.spawn_timeout_checker();

                Ok(())
            }
            Err(e) => {
                error!("Failed to send pairing request: {}", e);
                let _ = self.event_tx.send(PairingEvent::Error {
                    device_id: Some(device_id),
                    message: format!("Failed to send pairing request: {}", e),
                });
                Err(e)
            }
        }
    }

    /// Handle incoming pairing packet
    pub async fn handle_pairing_packet(
        &self,
        packet: &Packet,
        device_info: &DeviceInfo,
        device_cert: &[u8],
        remote_addr: SocketAddr,
    ) -> Result<Option<Packet>> {
        let device_id = &device_info.device_id;

        debug!(
            "Handling pairing packet from device {} at {}",
            device_id, remote_addr
        );

        let mut handler = self.handler.write().await;
        let (should_respond, response_packet) =
            handler.handle_pairing_packet(packet, device_id, device_cert)?;

        let status = handler.status();
        drop(handler);

        // Handle state changes
        match status {
            PairingStatus::RequestedByPeer => {
                info!(
                    "Pairing request received from {} ({})",
                    device_info.device_name, device_id
                );

                // Store the pairing request with certificate for later acceptance
                let mut requests = self.active_requests.write().await;
                requests.insert(
                    device_id.clone(),
                    PairingRequest {
                        started_at: Instant::now(),
                        device_info: device_info.clone(),
                        remote_addr,
                        device_cert: device_cert.to_vec(),
                    },
                );
                drop(requests);

                let fingerprint = LegacyCertificateInfo::calculate_fingerprint(device_cert);

                let _ = self.event_tx.send(PairingEvent::RequestReceived {
                    device_id: device_id.clone(),
                    device_name: device_info.device_name.clone(),
                    their_fingerprint: fingerprint,
                });

                // Start timeout checker
                self.spawn_timeout_checker();
            }
            PairingStatus::Paired => {
                info!("Successfully paired with device {}", device_id);

                // Remove from active requests
                let mut requests = self.active_requests.write().await;
                requests.remove(device_id);
                drop(requests);

                let fingerprint = LegacyCertificateInfo::calculate_fingerprint(device_cert);

                let _ = self.event_tx.send(PairingEvent::PairingAccepted {
                    device_id: device_id.clone(),
                    device_name: device_info.device_name.clone(),
                    certificate_fingerprint: fingerprint,
                });
            }
            PairingStatus::Unpaired => {
                debug!("Pairing rejected or unpaired for device {}", device_id);

                // Remove from active requests
                let mut requests = self.active_requests.write().await;
                requests.remove(device_id);
                drop(requests);

                let _ = self.event_tx.send(PairingEvent::PairingRejected {
                    device_id: device_id.clone(),
                    reason: None,
                });
            }
            _ => {}
        }

        // Return response packet if needed (caller sends it through existing connection)
        Ok(if should_respond { response_packet } else { None })
    }

    /// Accept a pairing request (user confirmed)
    pub async fn accept_pairing(&self, device_id: &str) -> Result<()> {
        info!("Accepting pairing with device {}", device_id);

        // Get the stored pairing request with certificate and address
        debug!("Step 1: Retrieving stored pairing request data for {}", device_id);
        let request_data = {
            let requests = self.active_requests.read().await;
            let data = requests
                .get(device_id)
                .map(|r| (r.device_info.clone(), r.device_cert.clone(), r.remote_addr));
            debug!("Found active request: {}", data.is_some());
            data
        };

        debug!("Step 2: Extracting device info, cert, and address");
        let (device_info, device_cert, remote_addr) = request_data.ok_or_else(|| {
            error!("No active pairing request found for device {}", device_id);
            crate::ProtocolError::Configuration(format!(
                "No active pairing request for device {}",
                device_id
            ))
        })?;
        debug!("Device info: name={}, addr={}", device_info.device_name, remote_addr);

        debug!("Step 3: Creating pairing acceptance response packet");
        let response = {
            let mut handler = self.handler.write().await;
            let resp = handler.accept_pairing(device_id, &device_cert)?;
            debug!("Response packet created: type={}", resp.packet_type);
            resp
        };

        debug!("Step 4: Checking for active TLS connection");
        // Ensure there's an active TLS connection before sending the acceptance packet
        // Unpaired devices disconnect immediately, so we may need to reconnect
        if let Some(conn_mgr) = &self.connection_manager {
            debug!("Connection manager is available");
            let conn_mgr_ref = conn_mgr.read().await;

            // Check if there's an active connection
            debug!("Step 5: Checking has_connection for {}", device_id);
            let has_connection = conn_mgr_ref.has_connection(device_id).await;
            debug!("Active connection exists: {}", has_connection);

            // If no active connection, establish one before sending the acceptance packet
            if !has_connection {
                info!(
                    "No active connection to {}, establishing connection to send pairing acceptance",
                    device_id
                );
                drop(conn_mgr_ref); // Release the read lock before connecting

                debug!("Step 6: Establishing new TLS connection to {} at {} with pairing certificate", device_id, remote_addr);
                // Establish a new TLS connection using the certificate from the pairing request
                // (certificate isn't in DeviceManager yet since we haven't completed pairing)
                let conn_mgr = conn_mgr.read().await;
                match conn_mgr.connect_with_cert(device_id, remote_addr, device_cert.clone()).await {
                    Ok(_) => debug!("Connection established successfully"),
                    Err(e) => {
                        error!("Failed to establish connection: {}", e);
                        return Err(e);
                    }
                }
                drop(conn_mgr);

                debug!("Step 7: Waiting 100ms for connection to stabilize");
                // Wait a brief moment for the connection to be fully established
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                debug!("Connection should be ready");
            } else {
                debug!("Using existing active connection");
            }
        } else {
            error!("Connection manager is not set - cannot send pairing acceptance");
            return Err(crate::ProtocolError::Configuration(
                "Connection manager not set".to_string()
            ));
        }

        debug!("Step 8: Sending pairing acceptance packet to {}", device_id);
        // Send accept response over TLS connection (Protocol v8)
        match self.send_pairing_packet(&response, device_id).await {
            Ok(_) => debug!("Pairing acceptance packet sent successfully"),
            Err(e) => {
                error!("Failed to send pairing acceptance packet: {}", e);
                return Err(e);
            }
        }

        debug!("Step 9: Removing device from active pairing requests");
        // Remove from active requests now that pairing is accepted
        self.active_requests.write().await.remove(device_id);
        debug!("Device removed from active requests");

        debug!("Step 10: Sending PairingAccepted event");
        let _ = self.event_tx.send(PairingEvent::PairingAccepted {
            device_id: device_id.to_string(),
            device_name: device_info.device_name.clone(),
            certificate_fingerprint: LegacyCertificateInfo::calculate_fingerprint(&device_cert),
        });

        info!("Successfully accepted pairing with device {}", device_id);
        Ok(())
    }

    /// Get remote address for an active pairing request
    async fn get_request_addr(&self, device_id: &str) -> Option<SocketAddr> {
        self.active_requests
            .read()
            .await
            .get(device_id)
            .map(|r| r.remote_addr)
    }

    /// Reject a pairing request (user declined)
    pub async fn reject_pairing(&self, device_id: &str) -> Result<()> {
        info!("Rejecting pairing with device {}", device_id);

        let response = {
            let mut handler = self.handler.write().await;
            handler.reject_pairing()
        };

        // Remove from active requests
        self.active_requests.write().await.remove(device_id);

        // Send reject response over TLS connection (Protocol v8)
        if let Err(e) = self.send_pairing_packet(&response, device_id).await {
            warn!("Failed to send pairing reject: {}", e);
        }

        let _ = self.event_tx.send(PairingEvent::PairingRejected {
            device_id: device_id.to_string(),
            reason: Some("User declined".to_string()),
        });

        Ok(())
    }

    /// Unpair from a device
    pub async fn unpair(&self, device_id: &str) -> Result<()> {
        info!("Unpairing from device {}", device_id);

        // TODO(#31): Send unpair packet via TLS connection
        let _packet = self.handler.write().await.unpair(device_id)?;

        let _ = self.event_tx.send(PairingEvent::DeviceUnpaired {
            device_id: device_id.to_string(),
        });

        Ok(())
    }

    /// Check if a device is paired
    pub async fn is_paired(&self, device_id: &str) -> bool {
        let handler = self.handler.read().await;
        handler.is_paired(device_id)
    }

    /// Send a pairing packet to a device over the TLS connection (Protocol v8)
    async fn send_pairing_packet(&self, packet: &Packet, device_id: &str) -> Result<()> {
        debug!("Sending pairing packet '{}' to device {}", packet.packet_type, device_id);

        // Protocol v8: Send pairing packets over the established TLS connection
        if let Some(conn_mgr) = &self.connection_manager {
            let conn_mgr = conn_mgr.read().await;
            conn_mgr.send_packet(device_id, packet).await?;
            Ok(())
        } else {
            Err(crate::ProtocolError::Configuration(
                "Connection manager not set - cannot send pairing packets in Protocol v8".to_string()
            ))
        }
    }

    /// Spawn timeout checker task
    fn spawn_timeout_checker(&self) {
        let active_requests = self.active_requests.clone();
        let event_tx = self.event_tx.clone();
        let timeout = self.config.timeout;

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;

            loop {
                let mut requests = active_requests.write().await;
                let now = Instant::now();
                let mut timed_out = Vec::new();

                for (device_id, request) in requests.iter() {
                    if now.duration_since(request.started_at) > timeout {
                        timed_out.push(device_id.clone());
                    }
                }

                for device_id in timed_out {
                    info!("Pairing request timed out for device {}", device_id);
                    requests.remove(&device_id);

                    let _ = event_tx.send(PairingEvent::PairingTimeout {
                        device_id: device_id.clone(),
                    });
                }

                drop(requests);

                if active_requests.read().await.is_empty() {
                    break;
                }

                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_pairing_service_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = PairingConfig {
            cert_dir: temp_dir.path().to_path_buf(),
            timeout: Duration::from_secs(30),
        };

        let service = PairingService::new("test_device", config).unwrap();
        assert!(!service.fingerprint().is_empty());
    }

    #[tokio::test]
    async fn test_pairing_service_events() {
        let temp_dir = TempDir::new().unwrap();
        let config = PairingConfig {
            cert_dir: temp_dir.path().to_path_buf(),
            timeout: Duration::from_secs(30),
        };

        let service = PairingService::new("test_device", config).unwrap();
        let _events = service.subscribe().await;

        // Events channel should be ready
        assert!(service.event_tx.is_closed() == false);
    }
}
