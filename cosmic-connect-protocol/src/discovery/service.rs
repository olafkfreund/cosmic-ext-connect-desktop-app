//! Async Discovery Service
//!
//! This module provides an async service that continuously broadcasts device identity
//! and listens for other devices on the network.

use super::events::DiscoveryEvent;
use crate::{DeviceInfo, Packet, ProtocolError, Result};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, RwLock};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

/// Default UDP port for device discovery
pub const DISCOVERY_PORT: u16 = 1716;

/// Port range for fallback when primary port is unavailable
pub const PORT_RANGE_START: u16 = 1714;
pub const PORT_RANGE_END: u16 = 1764;

/// Broadcast address for IPv4
pub const BROADCAST_ADDR: Ipv4Addr = Ipv4Addr::new(255, 255, 255, 255);

/// Default broadcast interval (5 seconds)
pub const DEFAULT_BROADCAST_INTERVAL: Duration = Duration::from_secs(5);

/// Default device timeout (30 seconds)
pub const DEFAULT_DEVICE_TIMEOUT: Duration = Duration::from_secs(30);

/// Configuration for discovery service
#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    /// How often to broadcast identity packets
    pub broadcast_interval: Duration,

    /// How long before a device is considered timed out
    pub device_timeout: Duration,

    /// Whether to enable device timeout checking
    pub enable_timeout_check: bool,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            broadcast_interval: DEFAULT_BROADCAST_INTERVAL,
            device_timeout: DEFAULT_DEVICE_TIMEOUT,
            enable_timeout_check: true,
        }
    }
}

/// Async discovery service
///
/// Runs two concurrent tasks:
/// - Broadcaster: Sends identity packets at regular intervals
/// - Listener: Receives and processes incoming identity packets
pub struct DiscoveryService {
    /// This device's information
    device_info: DeviceInfo,

    /// UDP socket for broadcasting and receiving
    socket: Arc<UdpSocket>,

    /// Event channel sender
    event_tx: mpsc::UnboundedSender<DiscoveryEvent>,

    /// Event channel receiver
    event_rx: Arc<RwLock<mpsc::UnboundedReceiver<DiscoveryEvent>>>,

    /// Service configuration
    config: DiscoveryConfig,

    /// Shutdown signal sender
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,

    /// Last seen timestamps for devices (device_id -> timestamp)
    last_seen: Arc<RwLock<HashMap<String, u64>>>,
}

impl DiscoveryService {
    /// Create a new discovery service
    ///
    /// # Arguments
    ///
    /// * `device_info` - Information about this device
    /// * `config` - Service configuration
    pub fn new(device_info: DeviceInfo, config: DiscoveryConfig) -> Result<Self> {
        // Try to bind to discovery port
        let socket = Self::bind_socket()?;
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        Ok(Self {
            device_info,
            socket: Arc::new(socket),
            event_tx,
            event_rx: Arc::new(RwLock::new(event_rx)),
            config,
            shutdown_tx: None,
            last_seen: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Create a discovery service with default configuration
    pub fn with_defaults(device_info: DeviceInfo) -> Result<Self> {
        Self::new(device_info, DiscoveryConfig::default())
    }

    /// Bind UDP socket with fallback ports
    fn bind_socket() -> Result<UdpSocket> {
        // Try primary port first
        match UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)) {
            Ok(socket) => {
                info!("Bound to UDP port {}", DISCOVERY_PORT);
                socket.set_broadcast(true)?;
                socket.set_nonblocking(true)?;
                Ok(socket)
            }
            Err(e) => {
                warn!(
                    "Failed to bind to primary port {}: {}. Trying fallback range...",
                    DISCOVERY_PORT, e
                );

                // Try fallback ports
                for port in PORT_RANGE_START..=PORT_RANGE_END {
                    if port == DISCOVERY_PORT {
                        continue;
                    }

                    if let Ok(socket) = UdpSocket::bind(("0.0.0.0", port)) {
                        info!("Bound to fallback UDP port {}", port);
                        socket.set_broadcast(true)?;
                        socket.set_nonblocking(true)?;
                        return Ok(socket);
                    }
                }

                Err(ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    format!(
                        "Failed to bind to any port in range {}-{}",
                        PORT_RANGE_START, PORT_RANGE_END
                    ),
                )))
            }
        }
    }

    /// Get the local port this service is bound to
    pub fn local_port(&self) -> Result<u16> {
        Ok(self.socket.local_addr()?.port())
    }

    /// Get a receiver for discovery events
    pub async fn subscribe(&self) -> mpsc::UnboundedReceiver<DiscoveryEvent> {
        let (tx, rx) = mpsc::unbounded_channel();

        // Create a task to forward events
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

    /// Start the discovery service
    ///
    /// Spawns background tasks for broadcasting and listening.
    /// Returns a handle that can be used to stop the service.
    pub async fn start(&mut self) -> Result<()> {
        let port = self.local_port()?;
        info!("Starting discovery service on port {}", port);

        // Send service started event
        let _ = self.event_tx.send(DiscoveryEvent::ServiceStarted { port });

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        // Spawn broadcaster task
        self.spawn_broadcaster(shutdown_rx);

        // Spawn listener task
        self.spawn_listener();

        // Spawn timeout checker if enabled
        if self.config.enable_timeout_check {
            self.spawn_timeout_checker();
        }

        Ok(())
    }

    /// Spawn broadcaster task
    fn spawn_broadcaster(&self, mut shutdown_rx: tokio::sync::oneshot::Receiver<()>) {
        let socket = self.socket.clone();
        let device_info = self.device_info.clone();
        let broadcast_interval = self.config.broadcast_interval;

        tokio::spawn(async move {
            let mut interval = interval(broadcast_interval);

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = Self::broadcast_identity(&socket, &device_info) {
                            error!("Failed to broadcast identity: {}", e);
                        }
                    }
                    _ = &mut shutdown_rx => {
                        info!("Broadcaster shutting down");
                        break;
                    }
                }
            }
        });
    }

    /// Broadcast identity packet
    fn broadcast_identity(socket: &UdpSocket, device_info: &DeviceInfo) -> Result<()> {
        let packet = device_info.to_identity_packet();
        let bytes = packet.to_bytes()?;
        let broadcast_addr = SocketAddr::new(IpAddr::V4(BROADCAST_ADDR), DISCOVERY_PORT);

        match socket.send_to(&bytes, broadcast_addr) {
            Ok(sent) => {
                debug!(
                    "Broadcasted identity packet ({} bytes) for device: {}",
                    sent, device_info.device_name
                );
                Ok(())
            }
            Err(e) => {
                warn!("Failed to send broadcast: {}", e);
                Err(ProtocolError::Io(e))
            }
        }
    }

    /// Send directed identity packet to a specific device
    /// This is sent in response to discovering a device, matching official KDE Connect behavior
    fn send_directed_identity(
        socket: &UdpSocket,
        device_info: &DeviceInfo,
        target_addr: SocketAddr,
    ) -> Result<()> {
        let packet = device_info.to_identity_packet();
        let bytes = packet.to_bytes()?;

        match socket.send_to(&bytes, target_addr) {
            Ok(sent) => {
                debug!(
                    "Sent directed identity packet ({} bytes) to {}",
                    sent, target_addr
                );
                Ok(())
            }
            Err(e) => {
                warn!("Failed to send directed identity to {}: {}", target_addr, e);
                Err(ProtocolError::Io(e))
            }
        }
    }

    /// Spawn listener task
    fn spawn_listener(&self) {
        let socket = self.socket.clone();
        let event_tx = self.event_tx.clone();
        let own_device_id = self.device_info.device_id.clone();
        let own_device_info = self.device_info.clone();
        let last_seen = self.last_seen.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 4096];

            loop {
                match socket.recv_from(&mut buf) {
                    Ok((size, src_addr)) => {
                        if let Err(e) = Self::handle_packet(
                            &buf[..size],
                            src_addr,
                            &own_device_id,
                            &own_device_info,
                            &socket,
                            &event_tx,
                            &last_seen,
                        )
                        .await
                        {
                            debug!("Error handling packet from {}: {}", src_addr, e);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        // No data available, sleep briefly
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(e) => {
                        error!("Error receiving packet: {}", e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });
    }

    /// Handle incoming packet
    async fn handle_packet(
        data: &[u8],
        src_addr: SocketAddr,
        own_device_id: &str,
        own_device_info: &DeviceInfo,
        socket: &UdpSocket,
        event_tx: &mpsc::UnboundedSender<DiscoveryEvent>,
        last_seen: &Arc<RwLock<HashMap<String, u64>>>,
    ) -> Result<()> {
        // Parse packet
        let packet = Packet::from_bytes(data)?;

        if !packet.is_type("kdeconnect.identity") {
            debug!("Ignoring non-identity packet from {}", src_addr);
            return Ok(());
        }

        // Parse device info
        let device_info = DeviceInfo::from_identity_packet(&packet)?;

        // Ignore our own broadcasts
        if device_info.device_id == own_device_id {
            debug!("Ignoring our own broadcast");
            return Ok(());
        }

        let current_time = current_timestamp();
        let mut last_seen_map = last_seen.write().await;

        // Check if this is a new device or update
        let is_new = !last_seen_map.contains_key(&device_info.device_id);
        last_seen_map.insert(device_info.device_id.clone(), current_time);
        drop(last_seen_map);

        // Send directed identity packet back to discovered device
        // This matches official KDE Connect behavior - devices send both broadcasts
        // AND directed packets to each discovered device
        if let Err(e) = Self::send_directed_identity(socket, own_device_info, src_addr) {
            warn!("Failed to send directed identity to {}: {}", src_addr, e);
        }

        // Emit appropriate event
        let event = if is_new {
            info!(
                "Discovered new device: {} ({}) at {}",
                device_info.device_name,
                device_info.device_type.as_str(),
                src_addr
            );
            DiscoveryEvent::tcp_discovered(device_info, src_addr)
        } else {
            debug!(
                "Updated device: {} at {}",
                device_info.device_name, src_addr
            );
            DiscoveryEvent::tcp_updated(device_info, src_addr)
        };

        let _ = event_tx.send(event);
        Ok(())
    }

    /// Spawn timeout checker task
    fn spawn_timeout_checker(&self) {
        let last_seen = self.last_seen.clone();
        let event_tx = self.event_tx.clone();
        let timeout_duration = self.config.device_timeout;

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(5));

            loop {
                interval.tick().await;

                let current_time = current_timestamp();
                let mut last_seen_map = last_seen.write().await;
                let mut timed_out = Vec::new();

                for (device_id, &last_seen_time) in last_seen_map.iter() {
                    if current_time - last_seen_time > timeout_duration.as_secs() {
                        timed_out.push(device_id.clone());
                    }
                }

                for device_id in timed_out {
                    info!("Device timed out: {}", device_id);
                    last_seen_map.remove(&device_id);
                    let _ = event_tx.send(DiscoveryEvent::DeviceTimeout { device_id });
                }
            }
        });
    }

    /// Stop the discovery service
    pub async fn stop(&mut self) {
        info!("Stopping discovery service");

        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        let _ = self.event_tx.send(DiscoveryEvent::ServiceStopped);
    }
}

/// Get current UNIX timestamp in seconds
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceType;

    #[test]
    fn test_discovery_config_defaults() {
        let config = DiscoveryConfig::default();
        assert_eq!(config.broadcast_interval, DEFAULT_BROADCAST_INTERVAL);
        assert_eq!(config.device_timeout, DEFAULT_DEVICE_TIMEOUT);
        assert!(config.enable_timeout_check);
    }

    #[tokio::test]
    async fn test_discovery_service_creation() {
        let device_info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1716);
        let service = DiscoveryService::with_defaults(device_info);
        assert!(service.is_ok());
    }

    #[tokio::test]
    async fn test_discovery_service_port() {
        let device_info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1716);
        let service = DiscoveryService::with_defaults(device_info).unwrap();
        let port = service.local_port().unwrap();
        assert!(port >= PORT_RANGE_START && port <= PORT_RANGE_END);
    }
}
