//! Connection Manager
//!
//! Manages TLS connections to multiple devices, handles connection lifecycle,
//! and routes packets between devices and the application.
//!
//! ## Connection Stability (Issue #52)
//!
//! This implementation uses socket replacement rather than connection rejection
//! when a device attempts to reconnect while already connected. This matches
//! the official KDE Connect behavior and prevents cascade connection failures
//! that can occur with aggressive Android clients.
//!
//! When a duplicate connection is detected:
//! 1. The old connection task is gracefully closed
//! 2. The old socket is replaced with the new one
//! 3. A disconnected event is emitted for the old connection
//! 4. A connected event is emitted for the new connection
//! 5. No rejection is sent to the client, preventing cascade failures

use super::events::ConnectionEvent;
use crate::{
    CertificateInfo, DeviceManager, Packet, ProtocolError, Result, TlsConfig, TlsConnection,
    TlsDeviceInfo, TlsServer,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// Keep-alive interval (send ping every 10 seconds to maintain connection)
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(10);

/// Connection timeout (consider disconnected after 60 seconds of no activity)
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(60);

/// Minimum delay between connection attempts from the same device
/// Issue #52: This is now used for logging warnings, not rejection
/// Socket replacement prevents connection storms while maintaining stability
const MIN_CONNECTION_DELAY: Duration = Duration::from_millis(1000);

/// Commands that can be sent to a connection task
enum ConnectionCommand {
    /// Send a packet
    SendPacket(Packet),
    /// Close the connection
    Close,
}

/// Active connection to a device
struct ActiveConnection {
    /// Channel to send commands to the connection task
    command_tx: mpsc::UnboundedSender<ConnectionCommand>,
    /// Task handling this connection
    task: JoinHandle<()>,
    /// Device ID
    device_id: String,
    /// Remote address
    remote_addr: SocketAddr,
}

/// Connection manager configuration
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// Local address to bind TLS server to
    pub listen_addr: SocketAddr,
    /// Keep-alive interval
    pub keep_alive_interval: Duration,
    /// Connection timeout
    pub connection_timeout: Duration,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0:1716".parse().unwrap(),
            keep_alive_interval: KEEP_ALIVE_INTERVAL,
            connection_timeout: CONNECTION_TIMEOUT,
        }
    }
}

/// Connection manager for handling multiple TLS connections
pub struct ConnectionManager {
    /// Our device certificate
    certificate: Arc<CertificateInfo>,

    /// TLS configuration (rustls-based from cosmic-connect-core)
    tls_config: Arc<TlsConfig>,

    /// Our device information
    device_info: Arc<crate::DeviceInfo>,

    /// Active connections (device_id -> connection)
    connections: Arc<RwLock<HashMap<String, ActiveConnection>>>,

    /// Device manager (for getting paired device certificates)
    device_manager: Arc<RwLock<DeviceManager>>,

    /// Event channel sender
    event_tx: mpsc::UnboundedSender<ConnectionEvent>,

    /// Event channel receiver
    event_rx: Arc<RwLock<mpsc::UnboundedReceiver<ConnectionEvent>>>,

    /// Configuration
    config: ConnectionConfig,

    /// TLS server task handle
    server_task: Arc<RwLock<Option<JoinHandle<()>>>>,

    /// Last connection time per device (for rate limiting to prevent connection storms)
    last_connection_time: Arc<RwLock<HashMap<String, Instant>>>,
}

/// Helper to convert discovery::DeviceInfo to TlsDeviceInfo
fn device_info_to_tls(info: &crate::DeviceInfo) -> TlsDeviceInfo {
    TlsDeviceInfo {
        device_id: info.device_id.clone(),
        device_name: info.device_name.clone(),
        device_type: info.device_type.as_str().to_string(),
        protocol_version: info.protocol_version as i32,
        incoming_capabilities: info.incoming_capabilities.clone(),
        outgoing_capabilities: info.outgoing_capabilities.clone(),
        tcp_port: info.tcp_port,
    }
}

impl ConnectionManager {
    /// Create a new connection manager
    pub fn new(
        certificate: CertificateInfo,
        device_info: crate::DeviceInfo,
        device_manager: Arc<RwLock<DeviceManager>>,
        config: ConnectionConfig,
    ) -> Result<Self> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        // Create TLS configuration from certificate (rustls-based)
        let tls_config = TlsConfig::new(&certificate)?;

        Ok(Self {
            certificate: Arc::new(certificate),
            tls_config: Arc::new(tls_config),
            device_info: Arc::new(device_info),
            connections: Arc::new(RwLock::new(HashMap::new())),
            device_manager,
            event_tx,
            event_rx: Arc::new(RwLock::new(event_rx)),
            config,
            server_task: Arc::new(RwLock::new(None)),
            last_connection_time: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Get a receiver for connection events
    pub async fn subscribe(&self) -> mpsc::UnboundedReceiver<ConnectionEvent> {
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

    /// Start the connection manager and TLS server
    pub async fn start(&self) -> Result<u16> {
        info!("Starting connection manager on {}", self.config.listen_addr);

        // Convert device info to TLS device info
        let tls_device_info = device_info_to_tls(&self.device_info);

        info!("Starting TLS server with rustls (TLS 1.2+, TOFU security model)");

        // Create TLS server (uses TOFU - Trust-On-First-Use, no pre-trusted certs needed)
        let server = TlsServer::new(
            self.config.listen_addr,
            &self.certificate,
            tls_device_info,
        )
        .await?;
        let local_port = server.local_addr().port();

        // Emit started event
        let _ = self
            .event_tx
            .send(ConnectionEvent::ManagerStarted { port: local_port });

        // Spawn server accept task
        let connections = self.connections.clone();
        let event_tx = self.event_tx.clone();
        let device_manager = self.device_manager.clone();
        let device_info = self.device_info.clone();
        let last_connection_time = self.last_connection_time.clone();

        let server_task = tokio::spawn(async move {
            loop {
                match server.accept().await {
                    Ok((connection, core_identity)) => {
                        let remote_addr = connection.remote_addr();
                        let device_name = core_identity
                            .get_body_field::<String>("deviceName")
                            .unwrap_or_else(|| "Unknown".to_string());
                        info!("Accepted connection from {} at {}", device_name, remote_addr);

                        // Convert core Packet to local Packet
                        let remote_identity = Packet::from_core_packet(core_identity);

                        // Spawn connection handler
                        // Note: remote_identity already contains the post-TLS identity packet
                        Self::spawn_connection_handler(
                            connection,
                            remote_addr,
                            device_info.clone(),
                            event_tx.clone(),
                            connections.clone(),
                            device_manager.clone(),
                            Some(remote_identity), // Pass the already-received identity
                            last_connection_time.clone(),
                        );
                    }
                    Err(e) => {
                        error!("Error accepting connection: {}", e);
                    }
                }
            }
        });

        let mut server_task_lock = self.server_task.write().await;
        *server_task_lock = Some(server_task);
        drop(server_task_lock);

        info!("Connection manager started on port {}", local_port);

        Ok(local_port)
    }

    /// Connect to a remote device
    pub async fn connect(&self, device_id: &str, addr: SocketAddr) -> Result<()> {
        info!("Connecting to device {} at {}", device_id, addr);

        // Check if already connected
        let connections = self.connections.read().await;
        if connections.contains_key(device_id) {
            info!("Already connected to device {}", device_id);
            return Ok(());
        }
        drop(connections);

        // Connect with TLS (rustls with TOFU)
        // Note: cosmic-connect-core TLS uses TOFU - no pre-verification needed
        let mut connection = TlsConnection::connect(addr, &self.tls_config).await?;

        connection.set_device_id(device_id.to_string());

        // Spawn connection handler
        // Note: For outgoing connections, we don't have pre-exchanged identity yet
        Self::spawn_connection_handler(
            connection,
            addr,
            self.device_info.clone(),
            self.event_tx.clone(),
            self.connections.clone(),
            self.device_manager.clone(),
            None, // Will perform identity exchange in handler
            self.last_connection_time.clone(),
        );

        info!("Connected to device {} at {}", device_id, addr);

        Ok(())
    }

    /// Connect to a remote device using a provided certificate (for pairing)
    /// This is used during pairing when the device certificate isn't in DeviceManager yet
    pub async fn connect_with_cert(
        &self,
        device_id: &str,
        addr: SocketAddr,
        _peer_cert: Vec<u8>,
    ) -> Result<()> {
        info!("Connecting to device {} at {} for pairing", device_id, addr);

        // Check if already connected
        let connections = self.connections.read().await;
        if connections.contains_key(device_id) {
            info!("Already connected to device {}", device_id);
            return Ok(());
        }
        drop(connections);

        // Connect with TLS (rustls with TOFU)
        // Note: peer_cert is ignored - cosmic-connect-core uses TOFU model
        // Certificate verification happens at application layer via SHA256 fingerprint
        let mut connection = TlsConnection::connect(addr, &self.tls_config).await?;

        connection.set_device_id(device_id.to_string());

        // Spawn connection handler
        // Note: For outgoing connections, we don't have pre-exchanged identity yet
        Self::spawn_connection_handler(
            connection,
            addr,
            self.device_info.clone(),
            self.event_tx.clone(),
            self.connections.clone(),
            self.device_manager.clone(),
            None, // Will perform identity exchange in handler
            self.last_connection_time.clone(),
        );

        info!("Connected to device {} at {} with provided certificate", device_id, addr);

        Ok(())
    }

    /// Send a packet to a device
    pub async fn send_packet(&self, device_id: &str, packet: &Packet) -> Result<()> {
        debug!(
            "Sending packet '{}' to device {}",
            packet.packet_type, device_id
        );

        let connections = self.connections.read().await;
        let connection = connections.get(device_id).ok_or_else(|| {
            ProtocolError::DeviceNotFound(format!("Not connected to device {}", device_id))
        })?;

        connection
            .command_tx
            .send(ConnectionCommand::SendPacket(packet.clone()))
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "Connection closed",
                ))
            })?;

        debug!("Packet queued for device {}", device_id);
        Ok(())
    }

    /// Disconnect from a device
    pub async fn disconnect(&self, device_id: &str) -> Result<()> {
        info!("Disconnecting from device {}", device_id);

        let mut connections = self.connections.write().await;
        if let Some(active_conn) = connections.remove(device_id) {
            // Send close command
            let _ = active_conn.command_tx.send(ConnectionCommand::Close);

            // Abort task
            active_conn.task.abort();

            info!("Disconnected from device {}", device_id);
        }

        Ok(())
    }

    /// Check if there's an active connection to a device
    pub async fn has_connection(&self, device_id: &str) -> bool {
        let connections = self.connections.read().await;
        connections.contains_key(device_id)
    }

    /// Stop the connection manager
    pub async fn stop(&self) {
        info!("Stopping connection manager");

        // Stop server task
        let mut server_task = self.server_task.write().await;
        if let Some(task) = server_task.take() {
            task.abort();
        }
        drop(server_task);

        // Disconnect all devices
        let device_ids: Vec<String> = {
            let connections = self.connections.read().await;
            connections.keys().cloned().collect()
        };

        for device_id in device_ids {
            let _ = self.disconnect(&device_id).await;
        }

        // Emit stopped event
        let _ = self.event_tx.send(ConnectionEvent::ManagerStopped);

        info!("Connection manager stopped");
    }

    /// Spawn a task to handle a connection (send/receive)
    ///
    /// If `remote_identity` is Some, the identity exchange has already been completed
    /// (e.g., by TLS server's accept() method for protocol v8). Otherwise, perform
    /// the identity exchange here.
    fn spawn_connection_handler(
        mut connection: TlsConnection,
        remote_addr: SocketAddr,
        device_info: Arc<crate::DeviceInfo>,
        event_tx: mpsc::UnboundedSender<ConnectionEvent>,
        connections: Arc<RwLock<HashMap<String, ActiveConnection>>>,
        device_manager: Arc<RwLock<DeviceManager>>,
        remote_identity: Option<crate::Packet>,
        last_connection_time: Arc<RwLock<HashMap<String, Instant>>>,
    ) {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();

        let _task = tokio::spawn(async move {
            let device_id: Option<String>;

            // If remote_identity is already provided, skip the identity exchange
            let packet = if let Some(identity_packet) = remote_identity {
                debug!("Using pre-exchanged identity packet from {}", remote_addr);
                identity_packet
            } else {
                // KDE Connect protocol v8: Send our identity over encrypted connection first
                let our_identity = device_info.to_identity_packet();
                let core_identity = our_identity.to_core_packet();
                if let Err(e) = connection.send_packet(&core_identity).await {
                    error!("Failed to send identity over TLS to {}: {}", remote_addr, e);
                    return;
                }
                debug!("Sent encrypted identity packet to {}", remote_addr);

                // Now receive the client's encrypted identity packet
                match connection.receive_packet().await {
                    Ok(core_pkt) => Packet::from_core_packet(core_pkt),
                    Err(e) => {
                        error!(
                            "Failed to receive identity packet from {}: {}",
                            remote_addr, e
                        );
                        return;
                    }
                }
            };

            // Extract device ID from the identity packet
            if let Some(id) = packet.body.get("deviceId").and_then(|v| v.as_str()) {
                device_id = Some(id.to_string());
                connection.set_device_id(id.to_string());

                info!("Connection identified as device {}", id);

                // Update device manager
                let mut dm = device_manager.write().await;
                if let Err(e) =
                    dm.mark_connected(id, remote_addr.ip().to_string(), remote_addr.port())
                {
                    warn!("Failed to mark device {} as connected: {}", id, e);
                }
                drop(dm);

                // Rate limiting: Check if device is connecting too frequently
                // Issue #52: With socket replacement, we no longer reject rapid reconnections
                // Instead, we log a warning to help diagnose client-side issues
                let now = Instant::now();
                let mut last_times = last_connection_time.write().await;
                if let Some(&last_time) = last_times.get(id) {
                    let elapsed = now.duration_since(last_time);
                    if elapsed < MIN_CONNECTION_DELAY {
                        warn!("Device {} reconnecting rapidly ({}ms since last connection) - \
                               this may indicate client-side connection cycling issues",
                              id, elapsed.as_millis());
                    }
                }
                last_times.insert(id.to_string(), now);
                drop(last_times);

                // Store connection in active connections FIRST
                // This must happen before emitting PacketReceived to avoid race condition
                // where a pairing response is attempted before the connection is registered
                let mut conns = connections.write().await;

                // Debug: Check what's in the connections HashMap
                debug!("Current connections in HashMap: {:?}", conns.keys().collect::<Vec<_>>());
                debug!("Looking for device {} in connections HashMap", id);

                // Handle existing connection if device reconnects
                // Issue #52: Instead of rejecting, replace the socket (like official KDE Connect)
                if let Some(old_conn) = conns.remove(id) {
                    // Device trying to reconnect while already connected
                    // Replace the old connection with the new one
                    info!("Device {} reconnecting from {} (old: {}) - replacing socket",
                          id, remote_addr, old_conn.remote_addr);

                    // Send close command to old connection task to clean up gracefully
                    let _ = old_conn.command_tx.send(ConnectionCommand::Close);

                    // Emit disconnected event for old connection
                    let _ = event_tx.send(ConnectionEvent::Disconnected {
                        device_id: id.to_string(),
                        reason: Some("Socket replaced with new connection".to_string()),
                    });

                    // Old connection will be replaced below with new one
                    // This prevents cascade closure on Android client
                }

                conns.insert(
                    id.to_string(),
                    ActiveConnection {
                        command_tx: command_tx.clone(),
                        task: tokio::task::spawn(async {}), // Placeholder task
                        device_id: id.to_string(),
                        remote_addr,
                    },
                );
                drop(conns);

                // Emit connected event
                let _ = event_tx.send(ConnectionEvent::Connected {
                    device_id: id.to_string(),
                    remote_addr,
                });

                // Emit packet received event
                let _ = event_tx.send(ConnectionEvent::PacketReceived {
                    device_id: id.to_string(),
                    packet: packet.clone(),
                    remote_addr,
                });
            } else {
                warn!("Identity packet from {} did not contain deviceId", remote_addr);
                return;
            }

            let device_id = device_id.unwrap();

            // DISABLED: Keepalive pings trigger notifications on Android
            // The phone sends its own pings to keep the connection alive
            // We don't need to send our own keepalive pings
            let mut keepalive_timer: Option<tokio::time::Interval> = None;

            // Main connection loop
            loop {
                tokio::select! {
                    // Handle commands
                    Some(cmd) = command_rx.recv() => {
                        match cmd {
                            ConnectionCommand::SendPacket(packet) => {
                                // Convert applet Packet to core Packet for TLS
                                let core_packet = packet.to_core_packet();
                                if let Err(e) = connection.send_packet(&core_packet).await {
                                    error!("Failed to send packet to {}: {}", device_id, e);
                                    break;
                                }
                            }
                            ConnectionCommand::Close => {
                                info!("Closing connection to {}", device_id);
                                break;
                            }
                        }
                    }

                    // Receive packets
                    result = connection.receive_packet() => {
                        match result {
                            Ok(core_packet) => {
                                // Convert core Packet to applet Packet
                                let packet = crate::Packet::from_core_packet(core_packet);
                                debug!("Received packet '{}' from {}", packet.packet_type, device_id);
                                let _ = event_tx.send(ConnectionEvent::PacketReceived {
                                    device_id: device_id.clone(),
                                    packet,
                                    remote_addr,
                                });
                            }
                            Err(e) => {
                                warn!("Error receiving packet from {}: {}", device_id, e);
                                break;
                            }
                        }
                    }

                    // Keepalive timer - only for paired devices
                    _ = async {
                        if let Some(ref mut timer) = keepalive_timer {
                            timer.tick().await;
                        } else {
                            // For unpaired devices, never send keepalive
                            std::future::pending::<()>().await;
                        }
                    } => {
                        debug!("Sending keepalive ping to paired device {}", device_id);
                        let ping_packet = crate::Packet::new("kdeconnect.ping", serde_json::json!({}));
                        let core_ping = ping_packet.to_core_packet();
                        if let Err(e) = connection.send_packet(&core_ping).await {
                            error!("Failed to send keepalive ping to {}: {}", device_id, e);
                            break;
                        }
                    }
                }
            }

            // Clean up
            info!("Connection handler for {} stopping", device_id);

            // Remove from active connections
            let mut conns = connections.write().await;
            conns.remove(&device_id);
            drop(conns);

            // Update device manager
            let mut dm = device_manager.write().await;
            let _ = dm.mark_disconnected(&device_id);
            drop(dm);

            // Emit disconnected event
            let _ = event_tx.send(ConnectionEvent::Disconnected {
                device_id: device_id.clone(),
                reason: Some("Connection closed".to_string()),
            });

            // Close connection
            let _ = connection.close().await;

            info!("Connection handler for {} stopped", device_id);
        });

        // Note: We can't update the task handle in ActiveConnection here
        // because we just moved it into the spawn. This is a limitation
        // of the current design. We could use Arc<Mutex<JoinHandle>> but
        // it's not necessary since we can abort via the command channel.
    }
}
