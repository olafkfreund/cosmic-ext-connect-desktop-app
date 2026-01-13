//! Pairing Service
//!
//! Manages pairing for multiple devices simultaneously.

use super::events::PairingEvent;
use super::handler::{CertificateInfo, PairingHandler, PairingStatus};
use crate::transport::TcpConnection;
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
    certificate: Arc<CertificateInfo>,

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
        })
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
    pub fn certificate(&self) -> &CertificateInfo {
        &self.certificate
    }

    /// Get our certificate fingerprint
    pub fn fingerprint(&self) -> &str {
        &self.certificate.fingerprint
    }

    /// Request pairing with a device
    ///
    /// Sends pairing request packet and starts timeout tracking.
    pub async fn request_pairing(&self, device_info: DeviceInfo, remote_addr: SocketAddr) -> Result<()> {
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

        // Connect and send request
        match self.send_pairing_packet(&packet, remote_addr).await {
            Ok(_) => {
                // Track active request
                let mut requests = self.active_requests.write().await;
                requests.insert(
                    device_id.clone(),
                    PairingRequest {
                        started_at: Instant::now(),
                        device_info: device_info.clone(),
                        remote_addr,
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
    ) -> Result<()> {
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

                let fingerprint = CertificateInfo::calculate_fingerprint(device_cert);

                let _ = self.event_tx.send(PairingEvent::RequestReceived {
                    device_id: device_id.clone(),
                    device_name: device_info.device_name.clone(),
                    their_fingerprint: fingerprint,
                });
            }
            PairingStatus::Paired => {
                info!("Successfully paired with device {}", device_id);

                // Remove from active requests
                let mut requests = self.active_requests.write().await;
                requests.remove(device_id);
                drop(requests);

                let _ = self.event_tx.send(PairingEvent::PairingAccepted {
                    device_id: device_id.clone(),
                    device_name: device_info.device_name.clone(),
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

        // Send response if needed
        if should_respond {
            if let Some(response) = response_packet {
                if let Err(e) = self.send_pairing_packet(&response, remote_addr).await {
                    error!("Failed to send pairing response: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Accept a pairing request (user confirmed)
    pub async fn accept_pairing(&self, device_id: &str, device_info: &DeviceInfo, device_cert: &[u8]) -> Result<()> {
        info!("Accepting pairing with device {}", device_id);

        let mut handler = self.handler.write().await;
        let response = handler.accept_pairing(device_id, device_cert)?;
        drop(handler);

        // Get remote address from active requests
        let requests = self.active_requests.read().await;
        let remote_addr = requests.get(device_id).map(|r| r.remote_addr);
        drop(requests);

        if let Some(addr) = remote_addr {
            // Send accept response
            if let Err(e) = self.send_pairing_packet(&response, addr).await {
                error!("Failed to send pairing accept: {}", e);
                return Err(e);
            }
        }

        // Emit event
        let _ = self.event_tx.send(PairingEvent::PairingAccepted {
            device_id: device_id.to_string(),
            device_name: device_info.device_name.clone(),
        });

        Ok(())
    }

    /// Reject a pairing request (user declined)
    pub async fn reject_pairing(&self, device_id: &str) -> Result<()> {
        info!("Rejecting pairing with device {}", device_id);

        let mut handler = self.handler.write().await;
        let response = handler.reject_pairing();
        drop(handler);

        // Get remote address from active requests
        let mut requests = self.active_requests.write().await;
        let remote_addr = requests.remove(device_id).map(|r| r.remote_addr);
        drop(requests);

        if let Some(addr) = remote_addr {
            // Send reject response
            if let Err(e) = self.send_pairing_packet(&response, addr).await {
                warn!("Failed to send pairing reject: {}", e);
            }
        }

        // Emit event
        let _ = self.event_tx.send(PairingEvent::PairingRejected {
            device_id: device_id.to_string(),
            reason: Some("User declined".to_string()),
        });

        Ok(())
    }

    /// Unpair from a device
    pub async fn unpair(&self, device_id: &str) -> Result<()> {
        info!("Unpairing from device {}", device_id);

        let mut handler = self.handler.write().await;
        let _packet = handler.unpair(device_id)?;
        drop(handler);

        // Note: We don't send the unpair packet here because we need
        // a TLS connection to send it securely. This will be done in Issue #31.

        // Emit event
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

    /// Send a pairing packet to a device
    async fn send_pairing_packet(&self, packet: &Packet, addr: SocketAddr) -> Result<()> {
        debug!("Sending pairing packet to {}", addr);

        let mut conn = TcpConnection::connect(addr).await?;
        conn.send_packet(packet).await?;

        // For pairing, we close the connection immediately after sending
        // Real communication will use TLS (Issue #31)
        conn.close().await?;

        Ok(())
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
    use crate::DeviceType;
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
