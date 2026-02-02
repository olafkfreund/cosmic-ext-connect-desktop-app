//! Bluetooth Transport for CConnect
//!
//! Provides Bluetooth RFCOMM connection support for CConnect protocol.
//! Uses BlueZ on Linux for Bluetooth operations via the bluer crate.
//!
//! ## Protocol
//!
//! RFCOMM provides a stream-based serial port emulation over Bluetooth.
//! This matches the Android implementation which uses BluetoothSocket (RFCOMM).
//!
//! ## Connection Flow
//!
//! 1. Desktop registers RFCOMM profile with SERVICE_UUID via BlueZ
//! 2. Android discovers desktop via SDP lookup for SERVICE_UUID
//! 3. Android connects via createRfcommSocketToServiceRecord()
//! 4. Desktop accepts connection via profile handler or listener
//! 5. Bidirectional stream communication begins

use crate::transport::{
    LatencyCategory, Transport, TransportAddress, TransportCapabilities, TransportFactory,
    TransportType,
};
use crate::{Packet, ProtocolError, Result};
use async_trait::async_trait;
use bluer::rfcomm::{Listener, Profile, ProfileHandle, SocketAddr, Stream};
use bluer::{Address, Session};
use futures::StreamExt;
use std::str::FromStr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use tracing::{debug, error, info};
use uuid::Uuid;

/// CConnect Bluetooth service UUID
/// Using a custom UUID for CConnect Bluetooth service
/// This UUID is used for SDP service registration/discovery
pub const CCONNECT_SERVICE_UUID: Uuid = uuid::uuid!("185f3df4-3268-4e3f-9fca-d4d5059915bd");

/// Bluetooth RFCOMM characteristic UUID (for reading) - documented for reference
/// Note: These are not used in RFCOMM mode (only in BLE GATT mode)
pub const RFCOMM_READ_CHAR_UUID: Uuid = uuid::uuid!("8667556c-9a37-4c91-84ed-54ee27d90049");

/// Bluetooth RFCOMM characteristic UUID (for writing) - documented for reference
/// Note: These are not used in RFCOMM mode (only in BLE GATT mode)
pub const RFCOMM_WRITE_CHAR_UUID: Uuid = uuid::uuid!("d0e8434d-cd29-0996-af41-6c90f4e0eb2a");

/// Default timeout for Bluetooth operations
const BT_TIMEOUT: Duration = Duration::from_secs(15);

/// Maximum packet size for Bluetooth (smaller MTU than TCP)
/// RFCOMM typically has ~512 bytes MTU, we use conservative value
const MAX_BT_PACKET_SIZE: usize = 512;

/// RFCOMM channel for CConnect service
/// Android uses dynamic channel allocation via SDP, but we can use a fixed channel
/// for direct connections. Channel 1 is commonly available.
const RFCOMM_CHANNEL: u8 = 1;

/// Bluetooth connection state using RFCOMM stream
pub struct BluetoothConnection {
    /// RFCOMM stream for bidirectional communication
    stream: Stream,

    /// Remote Bluetooth address
    #[allow(dead_code)]
    remote_address: Address,

    /// Remote Bluetooth address as string
    remote_address_str: String,

    /// Connection state
    connected: Arc<Mutex<bool>>,
}

impl BluetoothConnection {
    /// Connect to a Bluetooth device via RFCOMM
    ///
    /// # Arguments
    ///
    /// * `address` - Bluetooth MAC address (e.g., "00:11:22:33:44:55")
    /// * `channel` - Optional RFCOMM channel (defaults to RFCOMM_CHANNEL)
    pub async fn connect(address: String, channel: Option<u8>) -> Result<Self> {
        debug!("Connecting to Bluetooth device via RFCOMM: {}", address);

        // Parse Bluetooth address
        let bt_addr = Address::from_str(&address).map_err(|e| {
            ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid Bluetooth address '{}': {}", address, e),
            ))
        })?;

        // Create socket address with channel
        let ch = channel.unwrap_or(RFCOMM_CHANNEL);
        let socket_addr = SocketAddr::new(bt_addr, ch);

        debug!("Connecting to RFCOMM socket: {} channel {}", bt_addr, ch);

        // Connect with timeout
        let stream = timeout(BT_TIMEOUT, Stream::connect(socket_addr))
            .await
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("Connection timeout to {}", address),
                ))
            })?
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        info!("Successfully connected to Bluetooth device: {}", address);

        Ok(Self {
            stream,
            remote_address: bt_addr,
            remote_address_str: address,
            connected: Arc::new(Mutex::new(true)),
        })
    }

    /// Create a BluetoothConnection from an existing stream
    ///
    /// Used when accepting connections from a listener
    pub fn from_stream(stream: Stream, remote_address: Address) -> Self {
        let remote_address_str = remote_address.to_string();
        info!(
            "Created Bluetooth connection from accepted stream: {}",
            remote_address_str
        );

        Self {
            stream,
            remote_address,
            remote_address_str,
            connected: Arc::new(Mutex::new(true)),
        }
    }

    /// Close the connection
    pub async fn close_conn(mut self) -> Result<()> {
        debug!(
            "Closing Bluetooth connection to {}",
            self.remote_address_str
        );

        *self.connected.lock().await = false;

        // Shutdown the stream
        self.stream
            .shutdown()
            .await
            .map_err(ProtocolError::Io)?;

        Ok(())
    }
}

impl std::fmt::Debug for BluetoothConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BluetoothConnection")
            .field("remote_address", &self.remote_address_str)
            .field("connected", &"<state>")
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
            // RFCOMM is reliable (retransmission built-in)
            reliable: true,
            // RFCOMM is connection-oriented
            connection_oriented: true,
            // Bluetooth typically has medium latency
            latency: LatencyCategory::Medium,
        }
    }

    fn remote_address(&self) -> TransportAddress {
        TransportAddress::Bluetooth {
            address: self.remote_address_str.clone(),
            service_uuid: Some(CCONNECT_SERVICE_UUID),
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
            self.remote_address_str
        );

        // Send packet length as 4-byte big-endian prefix
        let len = bytes.len() as u32;
        let len_bytes = len.to_be_bytes();

        // Write length prefix
        self.stream
            .write_all(&len_bytes)
            .await
            .map_err(ProtocolError::Io)?;

        // Write packet data
        self.stream
            .write_all(&bytes)
            .await
            .map_err(ProtocolError::Io)?;

        debug!("Packet sent successfully to {}", self.remote_address_str);
        Ok(())
    }

    async fn receive_packet(&mut self) -> Result<Packet> {
        debug!(
            "Waiting for packet from Bluetooth device {}",
            self.remote_address_str
        );

        // Read 4-byte length prefix with timeout
        let mut len_buf = [0u8; 4];
        timeout(BT_TIMEOUT, self.stream.read_exact(&mut len_buf))
            .await
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Read timeout waiting for packet length",
                ))
            })?
            .map_err(ProtocolError::Io)?;

        let len = u32::from_be_bytes(len_buf) as usize;

        if len > MAX_BT_PACKET_SIZE {
            error!("Packet too large: {} bytes", len);
            return Err(ProtocolError::InvalidPacket(format!(
                "Packet too large: {} bytes (max {})",
                len, MAX_BT_PACKET_SIZE
            )));
        }

        // Read packet data
        let mut packet_data = vec![0u8; len];
        timeout(BT_TIMEOUT, self.stream.read_exact(&mut packet_data))
            .await
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Read timeout waiting for packet data",
                ))
            })?
            .map_err(ProtocolError::Io)?;

        let packet = Packet::from_bytes(&packet_data)?;
        debug!(
            "Received packet type '{}' from {}",
            packet.packet_type, self.remote_address_str
        );

        Ok(packet)
    }

    async fn close(self: Box<Self>) -> Result<()> {
        self.close_conn().await
    }

    fn is_connected(&self) -> bool {
        // Check connection state
        // Note: We use try_lock to avoid blocking, if locked assume connected
        self.connected
            .try_lock()
            .map(|guard| *guard)
            .unwrap_or(true)
    }
}

/// RFCOMM Listener for accepting incoming Bluetooth connections
pub struct BluetoothListener {
    /// The underlying RFCOMM listener
    listener: Listener,

    /// Local address
    local_addr: SocketAddr,
}

impl BluetoothListener {
    /// Create a new Bluetooth listener on a specific channel
    ///
    /// # Arguments
    ///
    /// * `channel` - RFCOMM channel to listen on (None for any available)
    pub async fn bind(channel: Option<u8>) -> Result<Self> {
        // Use specific channel or any available
        let addr = if let Some(ch) = channel {
            SocketAddr::new(Address::any(), ch)
        } else {
            SocketAddr::any()
        };

        debug!("Binding RFCOMM listener to {:?}", addr);

        let listener = Listener::bind(addr)
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        let local_addr = listener
            .as_ref()
            .local_addr()
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        info!(
            "Bluetooth RFCOMM listener bound on channel {}",
            local_addr.channel
        );

        Ok(Self {
            listener,
            local_addr,
        })
    }

    /// Get the channel this listener is bound to
    pub fn channel(&self) -> u8 {
        self.local_addr.channel
    }

    /// Accept an incoming connection
    pub async fn accept(&self) -> Result<(BluetoothConnection, Address)> {
        debug!("Waiting for incoming Bluetooth connection...");

        let (stream, remote_addr) = self
            .listener
            .accept()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        info!("Accepted Bluetooth connection from {}", remote_addr.addr);

        let connection = BluetoothConnection::from_stream(stream, remote_addr.addr);
        Ok((connection, remote_addr.addr))
    }
}

impl std::fmt::Debug for BluetoothListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BluetoothListener")
            .field("channel", &self.local_addr.channel)
            .finish()
    }
}

/// Profile-based Bluetooth service for SDP registration
///
/// This registers an SDP service record with our UUID so that Android
/// devices can discover and connect to us via `createRfcommSocketToServiceRecord()`.
pub struct BluetoothProfileService {
    /// BlueZ session
    #[allow(dead_code)]
    session: Session,

    /// Profile handle (keeps the profile registered)
    profile_handle: ProfileHandle,

    /// RFCOMM channel being used
    channel: u8,
}

impl BluetoothProfileService {
    /// Register the CConnect Bluetooth profile and start accepting connections
    ///
    /// This creates an SDP service record that Android can discover.
    pub async fn register() -> Result<Self> {
        info!("Registering CConnect Bluetooth profile for SDP discovery...");

        let session = Session::new()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        // Create the profile with our service UUID
        let profile = Profile {
            uuid: CCONNECT_SERVICE_UUID,
            name: Some("COSMIC Connect".to_string()),
            channel: Some(RFCOMM_CHANNEL as u16),
            require_authentication: Some(true), // Require pairing
            require_authorization: Some(false),
            auto_connect: Some(false),
            ..Default::default()
        };

        debug!(
            "Registering RFCOMM profile with UUID: {}",
            CCONNECT_SERVICE_UUID
        );

        // Register the profile
        let profile_handle = session.register_profile(profile).await.map_err(|e| {
            error!("Failed to register Bluetooth profile: {}", e);
            ProtocolError::Io(std::io::Error::other(e))
        })?;

        info!(
            "Bluetooth profile registered successfully on channel {}",
            RFCOMM_CHANNEL
        );
        info!(
            "SDP service record created with UUID: {}",
            CCONNECT_SERVICE_UUID
        );

        Ok(Self {
            session,
            profile_handle,
            channel: RFCOMM_CHANNEL,
        })
    }

    /// Accept an incoming connection from the profile
    ///
    /// This waits for Android devices to connect via SDP lookup.
    pub async fn accept(&mut self) -> Result<(BluetoothConnection, Address)> {
        debug!("Waiting for incoming connection via SDP profile...");

        // Wait for a connection request
        let req = self.profile_handle.next().await.ok_or_else(|| {
            ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Profile handle closed",
            ))
        })?;

        let device_addr = req.device();
        info!("Connection request from device: {}", device_addr);

        // Accept the connection
        let stream = req
            .accept()
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        info!("Accepted connection from {}", device_addr);

        let connection = BluetoothConnection::from_stream(stream, device_addr);
        Ok((connection, device_addr))
    }

    /// Get the RFCOMM channel
    pub fn channel(&self) -> u8 {
        self.channel
    }

    /// Get the service UUID
    pub fn service_uuid(&self) -> Uuid {
        CCONNECT_SERVICE_UUID
    }
}

impl std::fmt::Debug for BluetoothProfileService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BluetoothProfileService")
            .field("channel", &self.channel)
            .field("uuid", &CCONNECT_SERVICE_UUID.to_string())
            .finish()
    }
}

/// Factory for creating Bluetooth connections
#[derive(Debug, Clone)]
pub struct BluetoothTransportFactory {
    /// RFCOMM channel to use for connections
    channel: u8,

    /// Service UUID (for reference/logging)
    #[allow(dead_code)]
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
            channel: RFCOMM_CHANNEL,
            service_uuid: service_uuid.unwrap_or(CCONNECT_SERVICE_UUID),
        }
    }

    /// Create factory with specific channel
    pub fn with_channel(mut self, channel: u8) -> Self {
        self.channel = channel;
        self
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
            TransportAddress::Bluetooth {
                address,
                service_uuid: _,
            } => {
                let connection = BluetoothConnection::connect(address, Some(self.channel)).await?;
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

/// Get available Bluetooth adapters
pub async fn get_adapters() -> Result<Vec<String>> {
    let session = Session::new()
        .await
        .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

    let adapter_names = session
        .adapter_names()
        .await
        .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

    Ok(adapter_names)
}

/// Get paired devices from BlueZ
pub async fn get_paired_devices() -> Result<Vec<(Address, String)>> {
    let session = Session::new()
        .await
        .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

    let adapter_names = session
        .adapter_names()
        .await
        .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

    if adapter_names.is_empty() {
        return Ok(Vec::new());
    }

    let adapter = session
        .adapter(&adapter_names[0])
        .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

    let devices = adapter
        .device_addresses()
        .await
        .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

    let mut paired = Vec::new();
    for addr in devices {
        if let Ok(device) = adapter.device(addr) {
            if device.is_paired().await.unwrap_or(false) {
                let name = device
                    .name()
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| addr.to_string());
                paired.push((addr, name));
            }
        }
    }

    Ok(paired)
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

    #[test]
    fn test_address_parsing() {
        // Valid address
        let addr = Address::from_str("00:11:22:33:44:55");
        assert!(addr.is_ok());

        // Invalid address
        let addr = Address::from_str("invalid");
        assert!(addr.is_err());
    }
}
