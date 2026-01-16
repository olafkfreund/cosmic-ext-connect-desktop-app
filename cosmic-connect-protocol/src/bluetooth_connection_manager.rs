//! Bluetooth Connection Manager
//!
//! Manages Bluetooth connections to multiple devices using the BluetoothConnection
//! transport. This is analogous to ConnectionManager but specifically for Bluetooth.

use crate::{
    transport::{BluetoothConnection, Transport, CCONNECT_SERVICE_UUID},
    transport_manager::TransportManagerEvent,
    Packet, ProtocolError, Result,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Commands that can be sent to a Bluetooth connection task
enum BluetoothConnectionCommand {
    /// Send a packet
    SendPacket(Packet),
    /// Close the connection
    Close,
}

/// Active Bluetooth connection to a device
struct ActiveBluetoothConnection {
    /// Channel to send commands to the connection task
    command_tx: mpsc::UnboundedSender<BluetoothConnectionCommand>,
    /// Task handling this connection
    task: JoinHandle<()>,
    /// Device ID
    device_id: String,
    /// Bluetooth address
    bt_address: String,
}

/// Bluetooth connection manager for handling multiple Bluetooth connections
pub struct BluetoothConnectionManager {
    /// Active connections (device_id -> connection)
    connections: Arc<RwLock<HashMap<String, ActiveBluetoothConnection>>>,

    /// Event channel sender
    event_tx: mpsc::UnboundedSender<TransportManagerEvent>,

    /// Event channel receiver
    event_rx: Arc<RwLock<mpsc::UnboundedReceiver<TransportManagerEvent>>>,

    /// Bluetooth operation timeout
    timeout: Duration,
}

impl BluetoothConnectionManager {
    /// Create a new Bluetooth connection manager
    pub fn new(timeout: Duration) -> Result<Self> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        Ok(Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            event_rx: Arc::new(RwLock::new(event_rx)),
            timeout,
        })
    }

    /// Start the Bluetooth connection manager
    ///
    /// For Bluetooth, we don't have a "server" like TCP - devices discover
    /// each other via BLE advertising and then connect.
    pub async fn start(&self) -> Result<()> {
        info!("Bluetooth connection manager started");
        Ok(())
    }

    /// Connect to a remote device via Bluetooth
    pub async fn connect(
        &self,
        device_id: &str,
        bt_address: &str,
        service_uuid: Uuid,
    ) -> Result<()> {
        info!(
            "Connecting to device {} at Bluetooth address {}",
            device_id, bt_address
        );

        // Check if already connected
        let connections = self.connections.read().await;
        if connections.contains_key(device_id) {
            info!("Already connected to device {}", device_id);
            return Ok(());
        }
        drop(connections);

        // Create Bluetooth connection
        let connection = BluetoothConnection::connect(bt_address.to_string(), service_uuid)
            .await
            .map_err(|e| {
                ProtocolError::Transport(format!(
                    "Failed to connect to Bluetooth device {}: {}",
                    bt_address, e
                ))
            })?;

        // Spawn connection handler
        Self::spawn_connection_handler(
            connection,
            device_id.to_string(),
            bt_address.to_string(),
            self.event_tx.clone(),
            self.connections.clone(),
        );

        info!(
            "Connected to device {} at Bluetooth address {}",
            device_id, bt_address
        );

        Ok(())
    }

    /// Spawn a task to handle a Bluetooth connection (send/receive)
    fn spawn_connection_handler(
        mut connection: BluetoothConnection,
        device_id: String,
        bt_address: String,
        event_tx: mpsc::UnboundedSender<TransportManagerEvent>,
        connections: Arc<RwLock<HashMap<String, ActiveBluetoothConnection>>>,
    ) {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();

        // Clone for use in the update task
        let connections_for_update = connections.clone();
        let device_id_for_update = device_id.clone();

        let task = tokio::spawn(async move {
            info!("Bluetooth connection handler started for {}", device_id);

            // Store connection in active connections
            {
                let mut conns = connections.write().await;
                conns.insert(
                    device_id.clone(),
                    ActiveBluetoothConnection {
                        command_tx: command_tx.clone(),
                        task: tokio::task::spawn(async {}), // Placeholder
                        device_id: device_id.clone(),
                        bt_address: bt_address.clone(),
                    },
                );
            }

            // Emit connected event
            let _ = event_tx.send(TransportManagerEvent::Connected {
                device_id: device_id.clone(),
                transport_type: crate::TransportType::Bluetooth,
            });

            // Main connection loop
            loop {
                tokio::select! {
                    // Handle commands
                    Some(cmd) = command_rx.recv() => {
                        match cmd {
                            BluetoothConnectionCommand::SendPacket(packet) => {
                                debug!("Sending packet '{}' to {} via Bluetooth", packet.packet_type, device_id);
                                if let Err(e) = connection.send_packet(&packet).await {
                                    error!("Failed to send packet to {} via Bluetooth: {}", device_id, e);
                                    break;
                                }
                            }
                            BluetoothConnectionCommand::Close => {
                                info!("Closing Bluetooth connection to {}", device_id);
                                break;
                            }
                        }
                    }

                    // Receive packets
                    result = connection.receive_packet() => {
                        match result {
                            Ok(packet) => {
                                debug!("Received packet '{}' from {} via Bluetooth", packet.packet_type, device_id);
                                let _ = event_tx.send(TransportManagerEvent::PacketReceived {
                                    device_id: device_id.clone(),
                                    packet,
                                    transport_type: crate::TransportType::Bluetooth,
                                });
                            }
                            Err(e) => {
                                warn!("Error receiving packet from {} via Bluetooth: {}", device_id, e);
                                break;
                            }
                        }
                    }
                }
            }

            // Clean up
            info!("Bluetooth connection handler for {} stopping", device_id);

            // Remove from active connections
            {
                let mut conns = connections.write().await;
                conns.remove(&device_id);
            }

            // Emit disconnected event
            let _ = event_tx.send(TransportManagerEvent::Disconnected {
                device_id: device_id.clone(),
                transport_type: crate::TransportType::Bluetooth,
                reason: Some("Connection closed".to_string()),
            });

            // Close connection
            if let Err(e) = Box::new(connection).close().await {
                warn!("Error closing Bluetooth connection to {}: {}", device_id, e);
            }

            info!("Bluetooth connection handler for {} stopped", device_id);
        });

        // Update the task handle
        // Note: This is a race condition but acceptable since we can abort via command channel
        tokio::spawn(async move {
            let mut conns = connections_for_update.write().await;
            if let Some(conn) = conns.get_mut(&device_id_for_update) {
                conn.task = task;
            }
        });
    }

    /// Send a packet to a device
    pub async fn send_packet(&self, device_id: &str, packet: &Packet) -> Result<()> {
        debug!(
            "Queueing packet '{}' for device {} (Bluetooth)",
            packet.packet_type, device_id
        );

        let connections = self.connections.read().await;
        let connection = connections.get(device_id).ok_or_else(|| {
            ProtocolError::DeviceNotFound(format!(
                "Not connected to device {} via Bluetooth",
                device_id
            ))
        })?;

        connection
            .command_tx
            .send(BluetoothConnectionCommand::SendPacket(packet.clone()))
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "Bluetooth connection closed",
                ))
            })?;

        debug!("Packet queued for device {} (Bluetooth)", device_id);
        Ok(())
    }

    /// Disconnect from a device
    pub async fn disconnect(&self, device_id: &str) -> Result<()> {
        info!("Disconnecting from device {} (Bluetooth)", device_id);

        let mut connections = self.connections.write().await;
        if let Some(active_conn) = connections.remove(device_id) {
            // Send close command
            let _ = active_conn
                .command_tx
                .send(BluetoothConnectionCommand::Close);

            // Abort task
            active_conn.task.abort();

            info!("Disconnected from device {} (Bluetooth)", device_id);
        }

        Ok(())
    }

    /// Check if there's an active connection to a device
    pub async fn has_connection(&self, device_id: &str) -> bool {
        let connections = self.connections.read().await;
        connections.contains_key(device_id)
    }

    /// Get a receiver for connection events
    pub async fn subscribe(&self) -> mpsc::UnboundedReceiver<TransportManagerEvent> {
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

    /// Stop the Bluetooth connection manager
    pub async fn stop(&self) {
        info!("Stopping Bluetooth connection manager");

        // Disconnect all devices
        let device_ids: Vec<String> = {
            let connections = self.connections.read().await;
            connections.keys().cloned().collect()
        };

        for device_id in device_ids {
            let _ = self.disconnect(&device_id).await;
        }

        info!("Bluetooth connection manager stopped");
    }
}
