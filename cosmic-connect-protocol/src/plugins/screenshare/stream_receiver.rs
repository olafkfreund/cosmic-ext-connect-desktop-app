//! Network Stream Receiver for Screen Share
//!
//! Handles the custom binary protocol for video streaming.

use crate::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::info;

// Magic header "CSMR" (Cosmic Screen Mirroring)
const MAGIC_HEADER: &[u8; 4] = b"CSMR";

#[cfg(feature = "remotedesktop")]
const HEADER_SIZE: usize = 12;
#[cfg(feature = "remotedesktop")]
const MAX_FRAME_SIZE: usize = 10 * 1024 * 1024; // 10MB sanity limit

/// Stream receiver that handles the TCP connection and parses frames
pub struct StreamReceiver {
    listener: Option<TcpListener>,
    active_stream: Option<TcpStream>,
}

impl StreamReceiver {
    /// Create a new stream receiver
    pub fn new() -> Self {
        Self {
            listener: None,
            active_stream: None,
        }
    }

    /// Bind to a random port and return it
    pub async fn listen(&mut self) -> Result<u16> {
        // Bind to port 0 (random available port)
        let listener = TcpListener::bind("0.0.0.0:0")
            .await
            .map_err(crate::ProtocolError::Io)?;

        let port = listener
            .local_addr()
            .map_err(crate::ProtocolError::Io)?
            .port();

        info!("StreamReceiver listening on port {}", port);
        self.listener = Some(listener);

        Ok(port)
    }

    /// Accept an incoming connection (blocking until connected or error)
    pub async fn accept(&mut self) -> Result<()> {
        if let Some(listener) = &self.listener {
            info!("Waiting for incoming stream connection...");
            let (stream, addr) = listener
                .accept()
                .await
                .map_err(crate::ProtocolError::Io)?;

            info!("Accepted stream connection from {}", addr);
            self.active_stream = Some(stream);
            Ok(())
        } else {
            Err(crate::ProtocolError::InvalidState(
                "Listener not initialized".to_string(),
            ))
        }
    }

    /// Receive and parse the next frame from the stream
    ///
    /// Returns (frame_type, timestamp, payload)
    pub async fn next_frame(&mut self) -> Result<(u8, u64, Vec<u8>)> {
        if let Some(stream) = &mut self.active_stream {
            // Header structure:
            // Magic (4B) | Type (1B) | Timestamp (8B) | Size (4B)
            // Total header size: 17 bytes

            let mut header = [0u8; 17];

            // Read header
            stream
                .read_exact(&mut header)
                .await
                .map_err(crate::ProtocolError::Io)?;

            // Verify magic
            if &header[0..4] != MAGIC_HEADER {
                return Err(crate::ProtocolError::InvalidPacket(
                    "Invalid stream magic header".to_string(),
                ));
            }

            let frame_type = header[4];

            let mut ts_bytes = [0u8; 8];
            ts_bytes.copy_from_slice(&header[5..13]);
            let timestamp = u64::from_be_bytes(ts_bytes);

            let mut size_bytes = [0u8; 4];
            size_bytes.copy_from_slice(&header[13..17]);
            let payload_size = u32::from_be_bytes(size_bytes) as usize;

            // Sanity check size (e.g. max 10MB frame)
            if payload_size > 10 * 1024 * 1024 {
                return Err(crate::ProtocolError::PacketSizeExceeded(
                    payload_size,
                    10 * 1024 * 1024,
                ));
            }

            // Read payload
            let mut payload = vec![0u8; payload_size];
            stream
                .read_exact(&mut payload)
                .await
                .map_err(crate::ProtocolError::Io)?;

            Ok((frame_type, timestamp, payload))
        } else {
            Err(crate::ProtocolError::InvalidState(
                "No active stream".to_string(),
            ))
        }
    }

    /// Close the connection
    pub async fn close(&mut self) {
        if let Some(mut stream) = self.active_stream.take() {
            let _ = stream.shutdown().await;
        }
        self.listener = None;
    }
}
