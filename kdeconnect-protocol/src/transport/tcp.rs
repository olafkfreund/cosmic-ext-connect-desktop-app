//! Basic TCP Transport for Pairing
//!
//! Simple TCP connection for exchanging pairing packets before TLS is established.

use crate::{Packet, ProtocolError, Result};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};
use tracing::{debug, error};

/// Default timeout for TCP operations
const TCP_TIMEOUT: Duration = Duration::from_secs(10);

/// Maximum packet size (1MB)
const MAX_PACKET_SIZE: usize = 1024 * 1024;

/// Simple TCP connection for pairing
pub struct TcpConnection {
    stream: TcpStream,
    remote_addr: SocketAddr,
}

impl TcpConnection {
    /// Connect to a remote device
    ///
    /// # Arguments
    ///
    /// * `addr` - Remote socket address (IP:port)
    pub async fn connect(addr: SocketAddr) -> Result<Self> {
        debug!("Connecting to {}", addr);

        let stream = timeout(TCP_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Connection timeout",
                ))
            })??;

        debug!("Connected to {}", addr);

        Ok(Self {
            stream,
            remote_addr: addr,
        })
    }

    /// Create from an existing TcpStream
    pub fn from_stream(stream: TcpStream, remote_addr: SocketAddr) -> Self {
        Self {
            stream,
            remote_addr,
        }
    }

    /// Send a packet
    pub async fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        let bytes = packet.to_bytes()?;

        debug!(
            "Sending packet ({} bytes) to {}",
            bytes.len(),
            self.remote_addr
        );

        // Send packet length as 4-byte big-endian
        let len = bytes.len() as u32;
        self.stream.write_all(&len.to_be_bytes()).await?;

        // Send packet data
        self.stream.write_all(&bytes).await?;
        self.stream.flush().await?;

        debug!("Packet sent successfully to {}", self.remote_addr);
        Ok(())
    }

    /// Receive a packet
    pub async fn receive_packet(&mut self) -> Result<Packet> {
        debug!("Waiting for packet from {}", self.remote_addr);

        // Read packet length (4 bytes, big-endian)
        let mut len_bytes = [0u8; 4];
        timeout(TCP_TIMEOUT, self.stream.read_exact(&mut len_bytes))
            .await
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Read timeout",
                ))
            })??;

        let len = u32::from_be_bytes(len_bytes) as usize;

        if len > MAX_PACKET_SIZE {
            error!("Packet too large: {} bytes", len);
            return Err(ProtocolError::InvalidPacket(format!(
                "Packet too large: {} bytes (max {})",
                len, MAX_PACKET_SIZE
            )));
        }

        debug!("Receiving packet ({} bytes) from {}", len, self.remote_addr);

        // Read packet data
        let mut data = vec![0u8; len];
        timeout(TCP_TIMEOUT, self.stream.read_exact(&mut data))
            .await
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Read timeout",
                ))
            })??;

        let packet = Packet::from_bytes(&data)?;
        debug!(
            "Received packet type '{}' from {}",
            packet.packet_type, self.remote_addr
        );

        Ok(packet)
    }

    /// Get remote address
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    /// Close the connection
    pub async fn close(mut self) -> Result<()> {
        debug!("Closing connection to {}", self.remote_addr);
        self.stream.shutdown().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_tcp_connection_send_receive() {
        // Start a listener
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Spawn server task
        let server_task = tokio::spawn(async move {
            let (stream, remote_addr) = listener.accept().await.unwrap();
            let mut conn = TcpConnection::from_stream(stream, remote_addr);

            // Receive packet
            let packet = conn.receive_packet().await.unwrap();
            assert_eq!(packet.packet_type, "test.packet");

            // Send response
            let response = Packet::new("test.response", json!({"status": "ok"}));
            conn.send_packet(&response).await.unwrap();
        });

        // Client connects and sends packet
        let mut client = TcpConnection::connect(addr).await.unwrap();
        let test_packet = Packet::new("test.packet", json!({"data": "hello"}));
        client.send_packet(&test_packet).await.unwrap();

        // Receive response
        let response = client.receive_packet().await.unwrap();
        assert_eq!(response.packet_type, "test.response");

        client.close().await.unwrap();
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_connection_timeout() {
        // Try to connect to a non-existent server
        let result = TcpConnection::connect("127.0.0.1:1".parse().unwrap()).await;
        assert!(result.is_err());
    }
}
