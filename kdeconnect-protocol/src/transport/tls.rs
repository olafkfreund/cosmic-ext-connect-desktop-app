//! TLS Transport for KDE Connect
//!
//! Provides encrypted TCP connections using TLS with mutual certificate authentication.
//! Used for secure communication between paired devices.
//!
//! Uses tokio-openssl to support TLS 1.0 compatibility with Android KDE Connect app.

use crate::{CertificateInfo, Packet, ProtocolError, Result};
use openssl::ssl::{Ssl, SslAcceptor};
use std::net::SocketAddr;
use std::pin::Pin;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Duration};
use tokio_openssl::SslStream;
use tracing::{debug, error, info, warn};

use super::tls_config;

/// Default timeout for TLS operations (5 minutes for idle connections)
/// We don't use keepalive pings to avoid notification spam on Android,
/// so this timeout needs to be long enough for normal idle periods
const TLS_TIMEOUT: Duration = Duration::from_secs(300);

/// Maximum packet size (10MB - larger than TCP to support file transfers)
const MAX_PACKET_SIZE: usize = 10 * 1024 * 1024;

/// TLS connection to a remote device
pub struct TlsConnection {
    /// TLS stream
    stream: SslStream<TcpStream>,
    /// Remote address
    remote_addr: SocketAddr,
    /// Device ID of remote peer (if known)
    device_id: Option<String>,
}

impl TlsConnection {
    /// Connect to a remote device using TLS
    ///
    /// # Arguments
    ///
    /// * `addr` - Remote socket address
    /// * `our_cert` - Our device certificate
    /// * `peer_cert` - Expected peer certificate (from pairing)
    /// * `server_name` - SNI server name (usually IP address)
    pub async fn connect(
        addr: SocketAddr,
        our_cert: &CertificateInfo,
        peer_cert: Vec<u8>,
        _server_name: &str,
    ) -> Result<Self> {
        info!("Connecting to {} via TLS", addr);

        // Create TLS client config
        let connector = tls_config::create_client_config(our_cert, peer_cert)?;

        // Connect TCP stream
        let tcp_stream = timeout(TLS_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Connection timeout",
                ))
            })??;

        debug!("TCP connection established to {}", addr);

        // Create SSL instance for this connection
        let ssl = Ssl::new(connector.context())?;

        // Perform TLS handshake
        let mut tls_stream = SslStream::new(ssl, tcp_stream)?;

        timeout(TLS_TIMEOUT, Pin::new(&mut tls_stream).connect())
            .await
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "TLS handshake timeout",
                ))
            })?
            .map_err(|e| {
                error!("TLS handshake failed: {}", e);
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    format!("TLS handshake failed: {}", e),
                ))
            })?;

        info!("TLS connection established to {}", addr);

        Ok(Self {
            stream: tls_stream,
            remote_addr: addr,
            device_id: None,
        })
    }

    /// Create from an accepted TLS stream
    pub fn from_stream(stream: SslStream<TcpStream>, remote_addr: SocketAddr) -> Self {
        Self {
            stream,
            remote_addr,
            device_id: None,
        }
    }

    /// Set the device ID for this connection
    pub fn set_device_id(&mut self, device_id: String) {
        self.device_id = Some(device_id);
    }

    /// Get the device ID if known
    pub fn device_id(&self) -> Option<&str> {
        self.device_id.as_deref()
    }

    /// Get remote address
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    /// Send a packet over the TLS connection
    pub async fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        let bytes = packet.to_bytes()?;

        if bytes.len() > MAX_PACKET_SIZE {
            return Err(ProtocolError::InvalidPacket(format!(
                "Packet too large: {} bytes (max {})",
                bytes.len(),
                MAX_PACKET_SIZE
            )));
        }

        debug!(
            "Sending packet '{}' ({} bytes) to {}",
            packet.packet_type,
            bytes.len(),
            self.remote_addr
        );

        // KDE Connect protocol: Send packet data followed by newline
        self.stream.write_all(&bytes).await?;
        self.stream.flush().await?;

        debug!("Packet sent successfully to {}", self.remote_addr);
        Ok(())
    }

    /// Receive a packet from the TLS connection
    pub async fn receive_packet(&mut self) -> Result<Packet> {
        debug!("Waiting for packet from {}", self.remote_addr);

        // Read until newline (packet delimiter)
        let mut packet_bytes = Vec::new();
        let mut byte_buf = [0u8; 1];

        loop {
            match timeout(TLS_TIMEOUT, self.stream.read_exact(&mut byte_buf)).await {
                Ok(Ok(_)) => {
                    packet_bytes.push(byte_buf[0]);
                    if byte_buf[0] == b'\n' {
                        break;
                    }
                    if packet_bytes.len() > MAX_PACKET_SIZE {
                        error!("Packet too large: {} bytes", packet_bytes.len());
                        return Err(ProtocolError::InvalidPacket(format!(
                            "Packet too large: {} bytes (max {})",
                            packet_bytes.len(),
                            MAX_PACKET_SIZE
                        )));
                    }
                }
                Ok(Err(e)) => {
                    warn!("Error reading packet from {}: {}", self.remote_addr, e);
                    return Err(ProtocolError::Io(e));
                }
                Err(_) => {
                    return Err(ProtocolError::Io(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "Read timeout",
                    )));
                }
            }
        }

        debug!(
            "Received packet ({} bytes) from {}",
            packet_bytes.len(),
            self.remote_addr
        );

        let packet = Packet::from_bytes(&packet_bytes)?;
        debug!(
            "Received packet type '{}' from {}",
            packet.packet_type, self.remote_addr
        );

        Ok(packet)
    }

    /// Close the TLS connection
    pub async fn close(mut self) -> Result<()> {
        debug!("Closing TLS connection to {}", self.remote_addr);
        self.stream.shutdown().await?;
        Ok(())
    }
}

/// TLS server for accepting connections from paired devices
pub struct TlsServer {
    /// TCP listener
    listener: TcpListener,
    /// TLS acceptor (not used - we act as TLS client)
    acceptor: std::sync::Arc<SslAcceptor>,
    /// Local address
    local_addr: SocketAddr,
    /// Our device information (for sending identity packet)
    device_info: crate::DeviceInfo,
    /// Our certificate (needed for TLS client handshake)
    our_cert: CertificateInfo,
}

impl TlsServer {
    /// Create a new TLS server
    ///
    /// # Arguments
    ///
    /// * `addr` - Local address to bind to
    /// * `our_cert` - Our device certificate
    /// * `device_info` - Our device information for identity packet
    /// * `trusted_device_certs` - Certificates of all paired devices
    pub async fn new(
        addr: SocketAddr,
        our_cert: &CertificateInfo,
        device_info: crate::DeviceInfo,
        trusted_device_certs: Vec<Vec<u8>>,
    ) -> Result<Self> {
        info!("Starting TLS server on {}", addr);

        // Create TLS server config
        let acceptor = tls_config::create_server_config(our_cert, trusted_device_certs)?;

        // Bind TCP listener
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;

        info!("TLS server listening on {}", local_addr);

        Ok(Self {
            listener,
            acceptor,
            local_addr,
            device_info,
            our_cert: our_cert.clone(),
        })
    }

    /// Get the local address
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Accept an incoming connection with KDE Connect handshake
    ///
    /// KDE Connect protocol v8 requires:
    /// 1. TCP connection established
    /// 2. Client sends plain-text identity packet
    /// 3. Server reads plain-text identity packet
    /// 4. TLS handshake initiated immediately (no server identity in plain text)
    /// 5. After TLS: Server sends identity over encrypted connection
    /// 6. After TLS: Server reads client's encrypted identity
    ///
    /// Returns the TLS connection and the remote device's identity packet
    pub async fn accept(&self) -> Result<(TlsConnection, crate::Packet)> {
        debug!("Waiting for incoming connection");

        // Accept TCP connection
        let (mut tcp_stream, remote_addr) = self.listener.accept().await?;

        debug!("TCP connection accepted from {}", remote_addr);

        // Read plain-text identity packet byte-by-byte to avoid buffering ahead
        // (BufReader would buffer TLS ClientHello data, causing handshake to fail)
        let mut identity_bytes = Vec::new();
        let mut byte_buf = [0u8; 1];

        loop {
            match timeout(TLS_TIMEOUT, tcp_stream.read_exact(&mut byte_buf)).await {
                Ok(Ok(_)) => {
                    identity_bytes.push(byte_buf[0]);
                    // Stop at newline (identity packet delimiter)
                    if byte_buf[0] == b'\n' {
                        break;
                    }
                    // Prevent excessive memory usage
                    if identity_bytes.len() > MAX_PACKET_SIZE {
                        warn!("Identity packet too large from {}", remote_addr);
                        return Err(ProtocolError::InvalidPacket(
                            "Identity packet exceeds maximum size".to_string(),
                        ));
                    }
                }
                Ok(Err(e)) => {
                    warn!("Error reading identity packet from {}: {}", remote_addr, e);
                    return Err(ProtocolError::Io(e));
                }
                Err(_) => {
                    warn!("Timeout reading identity packet from {}", remote_addr);
                    return Err(ProtocolError::Io(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "Timeout reading identity packet",
                    )));
                }
            }
        }

        if identity_bytes.is_empty() {
            warn!("Connection closed before receiving identity from {}", remote_addr);
            return Err(ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Connection closed before identity packet",
            )));
        }

        debug!(
            "Received plain-text identity packet from {} ({} bytes)",
            remote_addr,
            identity_bytes.len()
        );

        // Parse identity packet
        let remote_identity = crate::Packet::from_bytes(&identity_bytes)?;

        if !remote_identity.is_type("kdeconnect.identity") {
            warn!(
                "Received non-identity packet from {}: {}",
                remote_addr, remote_identity.packet_type
            );
            return Err(ProtocolError::InvalidPacket(format!(
                "Expected identity packet, got {}",
                remote_identity.packet_type
            )));
        }

        info!(
            "Received identity from {} at {}",
            remote_identity
                .get_body_field::<String>("deviceName")
                .unwrap_or_default(),
            remote_addr
        );

        // KDE Connect protocol v8: Start TLS immediately after receiving client identity
        // IMPORTANT: KDE Connect uses inverted TLS roles - the device that accepts the TCP
        // connection acts as TLS CLIENT (not server). This matches Qt's startClientEncryption().
        debug!("Starting TLS handshake as CLIENT with {}", remote_addr);

        // Create SSL connector instance (we're the TLS client even though we accepted TCP)
        // Pass empty peer_cert vec since we're not verifying in TOFU model
        let connector = tls_config::create_client_config(&self.our_cert, vec![])?;
        let ssl = Ssl::new(connector.context())?;
        let mut tls_stream = SslStream::new(ssl, tcp_stream)?;

        // Perform TLS handshake as CLIENT
        timeout(TLS_TIMEOUT, Pin::new(&mut tls_stream).connect())
            .await
            .map_err(|_| {
                warn!("TLS handshake timeout from {}", remote_addr);
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "TLS handshake timeout",
                ))
            })?
            .map_err(|e| {
                warn!("TLS handshake failed from {}: {}", remote_addr, e);
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    format!("TLS handshake failed: {}", e),
                ))
            })?;

        info!(
            "TLS connection established with {} at {}",
            remote_identity
                .get_body_field::<String>("deviceName")
                .unwrap_or_default(),
            remote_addr
        );

        // Protocol v8: After TLS handshake, perform second identity exchange over encrypted connection
        let protocol_version = remote_identity
            .get_body_field::<i32>("protocolVersion")
            .unwrap_or(7);

        if protocol_version >= 8 {
            debug!(
                "Protocol v8 detected - performing post-TLS identity exchange with {}",
                remote_addr
            );

            // Step 1: Send our encrypted identity packet
            let our_identity_packet = crate::Packet::new(
                "kdeconnect.identity",
                serde_json::json!({
                    "deviceId": self.device_info.device_id,
                    "deviceName": self.device_info.device_name,
                    "deviceType": self.device_info.device_type.as_str(),
                    "protocolVersion": self.device_info.protocol_version,
                    "incomingCapabilities": self.device_info.incoming_capabilities,
                    "outgoingCapabilities": self.device_info.outgoing_capabilities,
                    "tcpPort": self.device_info.tcp_port,
                }),
            );

            let identity_bytes = our_identity_packet.to_bytes()?;
            tls_stream.write_all(&identity_bytes).await?;
            tls_stream.flush().await?;

            debug!(
                "Sent encrypted identity packet to {} ({} bytes)",
                remote_addr,
                identity_bytes.len()
            );

            // Step 2: Wait for remote device's encrypted identity packet
            let mut encrypted_identity_bytes = Vec::new();
            let mut byte_buf = [0u8; 1];

            loop {
                match timeout(TLS_TIMEOUT, tls_stream.read_exact(&mut byte_buf)).await {
                    Ok(Ok(_)) => {
                        encrypted_identity_bytes.push(byte_buf[0]);
                        if byte_buf[0] == b'\n' {
                            break;
                        }
                        if encrypted_identity_bytes.len() > MAX_PACKET_SIZE {
                            warn!(
                                "Encrypted identity packet too large from {}",
                                remote_addr
                            );
                            return Err(ProtocolError::InvalidPacket(
                                "Encrypted identity packet exceeds maximum size".to_string(),
                            ));
                        }
                    }
                    Ok(Err(e)) => {
                        warn!(
                            "Error reading encrypted identity packet from {}: {}",
                            remote_addr, e
                        );
                        return Err(ProtocolError::Io(e));
                    }
                    Err(_) => {
                        warn!(
                            "Timeout reading encrypted identity packet from {}",
                            remote_addr
                        );
                        return Err(ProtocolError::Io(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "Timeout reading encrypted identity packet",
                        )));
                    }
                }
            }

            debug!(
                "Received encrypted identity packet from {} ({} bytes)",
                remote_addr,
                encrypted_identity_bytes.len()
            );

            // Step 3: Parse and validate the encrypted identity packet
            let encrypted_identity = crate::Packet::from_bytes(&encrypted_identity_bytes)?;

            if !encrypted_identity.is_type("kdeconnect.identity") {
                warn!(
                    "Received non-identity packet over TLS from {}: {}",
                    remote_addr, encrypted_identity.packet_type
                );
                return Err(ProtocolError::InvalidPacket(format!(
                    "Expected identity packet over TLS, got {}",
                    encrypted_identity.packet_type
                )));
            }

            // Validate that the encrypted identity matches the pre-TLS identity
            let encrypted_device_id = encrypted_identity
                .get_body_field::<String>("deviceId")
                .unwrap_or_default();
            let pre_tls_device_id = remote_identity
                .get_body_field::<String>("deviceId")
                .unwrap_or_default();

            if encrypted_device_id != pre_tls_device_id {
                warn!(
                    "Device ID mismatch between pre-TLS ({}) and post-TLS ({}) identity from {}",
                    pre_tls_device_id, encrypted_device_id, remote_addr
                );
                return Err(ProtocolError::InvalidPacket(
                    "Device ID changed during TLS handshake".to_string(),
                ));
            }

            let encrypted_protocol_version = encrypted_identity
                .get_body_field::<i32>("protocolVersion")
                .unwrap_or(0);

            if encrypted_protocol_version != protocol_version {
                warn!(
                    "Protocol version mismatch between pre-TLS ({}) and post-TLS ({}) from {}",
                    protocol_version, encrypted_protocol_version, remote_addr
                );
                return Err(ProtocolError::InvalidPacket(
                    "Protocol version changed during TLS handshake".to_string(),
                ));
            }

            info!(
                "Protocol v8 post-TLS identity exchange completed successfully with {}",
                remote_addr
            );

            // Use the encrypted identity packet as the authoritative one
            Ok((
                TlsConnection::from_stream(tls_stream, remote_addr),
                encrypted_identity,
            ))
        } else {
            // Protocol v7 or older: No post-TLS identity exchange
            Ok((
                TlsConnection::from_stream(tls_stream, remote_addr),
                remote_identity,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_tls_connection_send_receive() {
        // Generate certificates for two devices
        let device1_cert = CertificateInfo::generate("device1").unwrap();
        let device2_cert = CertificateInfo::generate("device2").unwrap();

        let device_info = crate::DeviceInfo {
            device_id: "device2".to_string(),
            device_name: "Test Device 2".to_string(),
            device_type: crate::DeviceType::Desktop,
            protocol_version: 8,
            incoming_capabilities: vec![],
            outgoing_capabilities: vec![],
            tcp_port: 1716,
        };

        // Start TLS server (device2)
        let server_addr = "127.0.0.1:0".parse().unwrap();
        let server = TlsServer::new(
            server_addr,
            &device2_cert,
            device_info,
            vec![device1_cert.certificate.clone()],
        )
        .await
        .unwrap();

        let server_port = server.local_addr().port();
        let server_addr = format!("127.0.0.1:{}", server_port).parse().unwrap();

        // Spawn server task
        let server_task = tokio::spawn(async move {
            // Accept connection
            let (mut conn, _identity) = server.accept().await.unwrap();

            // Receive packet
            let packet = conn.receive_packet().await.unwrap();
            assert_eq!(packet.packet_type, "test.packet");

            // Send response
            let response = Packet::new("test.response", json!({"status": "ok"}));
            conn.send_packet(&response).await.unwrap();

            conn.close().await.unwrap();
        });

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Connect as client (device1)
        let mut client = TlsConnection::connect(
            server_addr,
            &device1_cert,
            device2_cert.certificate.clone(),
            "127.0.0.1",
        )
        .await
        .unwrap();

        // Send packet
        let test_packet = Packet::new("test.packet", json!({"data": "hello"}));
        client.send_packet(&test_packet).await.unwrap();

        // Receive response
        let response = client.receive_packet().await.unwrap();
        assert_eq!(response.packet_type, "test.response");

        client.close().await.unwrap();
        server_task.await.unwrap();
    }
}
