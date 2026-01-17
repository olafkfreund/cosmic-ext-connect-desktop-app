//! Bluetooth Transport for CConnect
//!
//! Provides Bluetooth RFCOMM connection support for CConnect protocol.
//! Uses BlueZ on Linux for Bluetooth operations.

use crate::transport::{
    LatencyCategory, Transport, TransportAddress, TransportCapabilities, TransportFactory,
    TransportType,
};
use crate::{Packet, ProtocolError, Result};
use async_trait::async_trait;
use btleplug::api::{
    Central, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// CConnect Bluetooth service UUID
/// Using a custom UUID for CConnect Bluetooth service
pub const CCONNECT_SERVICE_UUID: Uuid = uuid::uuid!("185f3df4-3268-4e3f-9fca-d4d5059915bd");

/// Bluetooth RFCOMM characteristic UUID (for reading)
pub const RFCOMM_READ_CHAR_UUID: Uuid = uuid::uuid!("8667556c-9a37-4c91-84ed-54ee27d90049");

/// Bluetooth RFCOMM characteristic UUID (for writing)
pub const RFCOMM_WRITE_CHAR_UUID: Uuid = uuid::uuid!("d0e8434d-cd29-0996-af41-6c90f4e0eb2a");

/// Default timeout for Bluetooth operations
const BT_TIMEOUT: Duration = Duration::from_secs(15);

/// Maximum packet size for Bluetooth (smaller MTU than TCP)
/// RFCOMM typically has ~512 bytes MTU, we use conservative value
const MAX_BT_PACKET_SIZE: usize = 512;

/// Bluetooth connection state
pub struct BluetoothConnection {
    /// Bluetooth peripheral device
    peripheral: Peripheral,

    /// Remote Bluetooth address
    remote_address: String,

    /// Service UUID
    service_uuid: Uuid,

    /// Read characteristic
    read_char: Option<btleplug::api::Characteristic>,

    /// Write characteristic
    write_char: Option<btleplug::api::Characteristic>,

    /// Connection state
    connected: Arc<Mutex<bool>>,
}

impl BluetoothConnection {
    /// Connect to a Bluetooth device
    ///
    /// # Arguments
    ///
    /// * `address` - Bluetooth MAC address (e.g., "00:11:22:33:44:55")
    /// * `service_uuid` - Service UUID to connect to
    pub async fn connect(address: String, service_uuid: Uuid) -> Result<Self> {
        debug!("Connecting to Bluetooth device: {}", address);

        // Get Bluetooth adapter
        let manager = Manager::new()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        let adapters = manager
            .adapters()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No Bluetooth adapter found"
            )))?;

        // Find the peripheral
        let peripheral = Self::find_peripheral(&adapter, &address, service_uuid).await?;

        // Connect to peripheral
        peripheral
            .connect()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        debug!("Connected to peripheral, discovering services...");

        // Discover services
        peripheral
            .discover_services()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        // Find characteristics
        let (read_char, write_char) = Self::find_characteristics(&peripheral, service_uuid).await?;

        // Subscribe to notifications for reading
        if let Some(ref char) = read_char {
            peripheral
                .subscribe(char)
                .await
                .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;
        }

        info!("Successfully connected to Bluetooth device: {}", address);

        Ok(Self {
            peripheral,
            remote_address: address,
            service_uuid,
            read_char,
            write_char,
            connected: Arc::new(Mutex::new(true)),
        })
    }

    /// Find a peripheral by address
    async fn find_peripheral(
        adapter: &Adapter,
        address: &str,
        service_uuid: Uuid,
    ) -> Result<Peripheral> {
        debug!("Scanning for Bluetooth device: {}", address);

        // Start scanning
        adapter
            .start_scan(ScanFilter {
                services: vec![service_uuid],
            })
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        // Wait for device to be discovered (with timeout)
        let start = std::time::Instant::now();
        let timeout_duration = Duration::from_secs(10);

        while start.elapsed() < timeout_duration {
            let peripherals = adapter
                .peripherals()
                .await
                .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

            for peripheral in peripherals {
                if let Ok(Some(props)) = peripheral.properties().await {
                    if let Some(local_name) = props.local_name {
                        if local_name.contains(address) {
                            adapter
                                .stop_scan()
                                .await
                                .map_err(|e| ProtocolError::Io(std::io::Error::other(
                                    e,
                                )))?;
                            return Ok(peripheral);
                        }
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        adapter
            .stop_scan()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        Err(ProtocolError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Bluetooth device not found: {}", address),
        )))
    }

    /// Find read and write characteristics
    async fn find_characteristics(
        peripheral: &Peripheral,
        service_uuid: Uuid,
    ) -> Result<(Option<btleplug::api::Characteristic>, Option<btleplug::api::Characteristic>)> {
        let characteristics = peripheral
            .characteristics();

        let mut read_char = None;
        let mut write_char = None;

        for char in characteristics {
            if char.service_uuid == service_uuid {
                if char.uuid == RFCOMM_READ_CHAR_UUID {
                    read_char = Some(char.clone());
                } else if char.uuid == RFCOMM_WRITE_CHAR_UUID {
                    write_char = Some(char.clone());
                }
            }
        }

        if read_char.is_none() || write_char.is_none() {
            warn!("Could not find all required characteristics");
        }

        Ok((read_char, write_char))
    }

    /// Close the connection
    pub async fn close_conn(self) -> Result<()> {
        debug!("Closing Bluetooth connection to {}", self.remote_address);

        *self.connected.lock().await = false;

        self.peripheral
            .disconnect()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        Ok(())
    }
}

impl std::fmt::Debug for BluetoothConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BluetoothConnection")
            .field("remote_address", &self.remote_address)
            .field("service_uuid", &self.service_uuid)
            .finish()
    }
}

// Implement Transport trait for BluetoothConnection
#[async_trait]
impl Transport for BluetoothConnection {
    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            // Bluetooth has smaller MTU than TCP
            max_packet_size: MAX_BT_PACKET_SIZE,
            // Bluetooth is reliable
            reliable: true,
            // Bluetooth is connection-oriented
            connection_oriented: true,
            // Bluetooth typically has medium latency
            latency: LatencyCategory::Medium,
        }
    }

    fn remote_address(&self) -> TransportAddress {
        TransportAddress::Bluetooth {
            address: self.remote_address.clone(),
            service_uuid: Some(self.service_uuid),
        }
    }

    async fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        let bytes = packet.to_bytes()?;

        if bytes.len() > MAX_BT_PACKET_SIZE {
            return Err(ProtocolError::InvalidPacket(format!(
                "Packet too large for Bluetooth: {} bytes (max {})",
                bytes.len(),
                MAX_BT_PACKET_SIZE
            )));
        }

        debug!(
            "Sending packet ({} bytes) to Bluetooth device {}",
            bytes.len(),
            self.remote_address
        );

        let write_char = self.write_char.as_ref().ok_or_else(|| {
            ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "Write characteristic not available",
            ))
        })?;

        // Send packet length as 4-byte big-endian
        let len = bytes.len() as u32;
        let mut data_to_send = len.to_be_bytes().to_vec();
        data_to_send.extend_from_slice(&bytes);

        // Write data to characteristic
        self.peripheral
            .write(write_char, &data_to_send, WriteType::WithResponse)
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        debug!("Packet sent successfully to {}", self.remote_address);
        Ok(())
    }

    async fn receive_packet(&mut self) -> Result<Packet> {
        debug!("Waiting for packet from Bluetooth device {}", self.remote_address);

        let _read_char = self.read_char.as_ref().ok_or_else(|| {
            ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "Read characteristic not available",
            ))
        })?;

        // Read notifications from peripheral
        let mut notification_stream = self.peripheral.notifications().await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        // Wait for notification with timeout
        use futures::StreamExt;
        let notification = timeout(BT_TIMEOUT, StreamExt::next(&mut notification_stream))
            .await
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Read timeout",
                ))
            })?
            .ok_or_else(|| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Notification stream ended",
                ))
            })?;

        let data = notification.value;

        // Parse packet length
        if data.len() < 4 {
            return Err(ProtocolError::InvalidPacket(
                "Packet too short (missing length)".to_string(),
            ));
        }

        let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;

        if len > MAX_BT_PACKET_SIZE {
            error!("Packet too large: {} bytes", len);
            return Err(ProtocolError::InvalidPacket(format!(
                "Packet too large: {} bytes (max {})",
                len, MAX_BT_PACKET_SIZE
            )));
        }

        // Extract packet data (skip length prefix)
        let packet_data = &data[4..];

        if packet_data.len() != len {
            return Err(ProtocolError::InvalidPacket(format!(
                "Packet size mismatch: expected {} bytes, got {}",
                len,
                packet_data.len()
            )));
        }

        let packet = Packet::from_bytes(packet_data)?;
        debug!(
            "Received packet type '{}' from {}",
            packet.packet_type, self.remote_address
        );

        Ok(packet)
    }

    async fn close(self: Box<Self>) -> Result<()> {
        self.close_conn().await
    }

    fn is_connected(&self) -> bool {
        // Check connection state
        // Note: We use try_lock to avoid blocking, if locked assume connected
        self.connected.try_lock().map(|guard| *guard).unwrap_or(true)
    }
}

/// Factory for creating Bluetooth connections
#[derive(Debug, Clone)]
pub struct BluetoothTransportFactory {
    /// Service UUID to use for connections
    service_uuid: Uuid,
}

impl BluetoothTransportFactory {
    /// Create a new Bluetooth transport factory
    ///
    /// # Arguments
    ///
    /// * `service_uuid` - Optional service UUID (defaults to CConnect UUID)
    pub fn new(service_uuid: Option<Uuid>) -> Self {
        Self {
            service_uuid: service_uuid.unwrap_or(CCONNECT_SERVICE_UUID),
        }
    }
}

impl Default for BluetoothTransportFactory {
    fn default() -> Self {
        Self::new(None)
    }
}

#[async_trait]
impl TransportFactory for BluetoothTransportFactory {
    async fn connect(&self, address: TransportAddress) -> Result<Box<dyn Transport>> {
        match address {
            TransportAddress::Bluetooth { address, service_uuid } => {
                let uuid = service_uuid.unwrap_or(self.service_uuid);
                let connection = BluetoothConnection::connect(address, uuid).await?;
                Ok(Box::new(connection))
            }
            _ => Err(ProtocolError::InvalidPacket(
                "Bluetooth factory can only create Bluetooth connections".to_string(),
            )),
        }
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Bluetooth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bluetooth_capabilities() {
        // Mock test - actual testing requires hardware
        assert_eq!(MAX_BT_PACKET_SIZE, 512);
    }

    #[test]
    fn test_service_uuid() {
        // Verify the service UUID is valid
        assert_eq!(
            CCONNECT_SERVICE_UUID.to_string(),
            "185f3df4-3268-4e3f-9fca-d4d5059915bd"
        );
    }

    #[test]
    fn test_factory_creation() {
        let factory = BluetoothTransportFactory::default();
        assert_eq!(factory.transport_type(), TransportType::Bluetooth);
        assert_eq!(factory.service_uuid, CCONNECT_SERVICE_UUID);
    }
}
