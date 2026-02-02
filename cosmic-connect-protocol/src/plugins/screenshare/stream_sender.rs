//! Network Stream Sender for Screen Share
//!
//! Handles sending encoded video frames to connected viewers.

use crate::Result;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tracing::{debug, error, info};

// Magic header "CSMR" (Cosmic Screen Mirroring)
const MAGIC_HEADER: &[u8; 4] = b"CSMR";

/// Frame type identifiers
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    /// H.264 video frame
    Video = 0x01,
    /// Cursor position update
    Cursor = 0x02,
    /// Annotation data
    Annotation = 0x03,
    /// End of stream
    EndOfStream = 0xFF,
}

impl From<u8> for FrameType {
    fn from(value: u8) -> Self {
        match value {
            0x01 => FrameType::Video,
            0x02 => FrameType::Cursor,
            0x03 => FrameType::Annotation,
            0xFF => FrameType::EndOfStream,
            _ => FrameType::Video, // Default
        }
    }
}

/// Stream sender for transmitting frames to a viewer
pub struct StreamSender {
    stream: Option<TcpStream>,
    frames_sent: u64,
    bytes_sent: u64,
    /// Bytes sent in current measurement window
    window_bytes: u64,
    /// Start time of current measurement window
    window_start: std::time::Instant,
    /// Last calculated throughput in bytes per second
    throughput_bps: u64,
}

/// Measurement window duration for throughput calculation
const THROUGHPUT_WINDOW_MS: u64 = 1000;

impl StreamSender {
    /// Create a new stream sender
    pub fn new() -> Self {
        Self {
            stream: None,
            frames_sent: 0,
            bytes_sent: 0,
            window_bytes: 0,
            window_start: std::time::Instant::now(),
            throughput_bps: 0,
        }
    }

    /// Connect to a viewer at the specified address
    pub async fn connect(&mut self, host: &str, port: u16) -> Result<()> {
        let addr = format!("{}:{}", host, port);
        info!("Connecting to viewer at {}", addr);

        let stream = TcpStream::connect(&addr)
            .await
            .map_err(crate::ProtocolError::Io)?;

        // Set TCP_NODELAY for low latency
        stream
            .set_nodelay(true)
            .map_err(crate::ProtocolError::Io)?;

        info!("Connected to viewer at {}", addr);
        self.stream = Some(stream);
        self.reset_throughput();
        Ok(())
    }

    /// Send an encoded video frame
    ///
    /// Frame format:
    /// - Magic (4 bytes): "CSMR"
    /// - Type (1 byte): Frame type
    /// - Timestamp (8 bytes): PTS in nanoseconds (big-endian)
    /// - Size (4 bytes): Payload size (big-endian)
    /// - Payload: Encoded frame data
    pub async fn send_video_frame(&mut self, data: &[u8], timestamp_ns: u64) -> Result<()> {
        self.send_frame(FrameType::Video, timestamp_ns, data).await
    }

    /// Send a cursor position update
    pub async fn send_cursor(&mut self, x: i32, y: i32, visible: bool) -> Result<()> {
        let mut payload = [0u8; 9];
        payload[0..4].copy_from_slice(&x.to_be_bytes());
        payload[4..8].copy_from_slice(&y.to_be_bytes());
        payload[8] = u8::from(visible);
        self.send_frame(FrameType::Cursor, 0, &payload).await
    }

    /// Send end of stream marker
    pub async fn send_end_of_stream(&mut self) -> Result<()> {
        self.send_frame(FrameType::EndOfStream, 0, &[]).await
    }

    /// Send a frame with the specified type
    async fn send_frame(
        &mut self,
        frame_type: FrameType,
        timestamp_ns: u64,
        payload: &[u8],
    ) -> Result<()> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| crate::ProtocolError::InvalidState("Not connected".to_string()))?;

        // Build header (17 bytes total)
        let mut header = [0u8; 17];
        header[0..4].copy_from_slice(MAGIC_HEADER);
        header[4] = frame_type as u8;
        header[5..13].copy_from_slice(&timestamp_ns.to_be_bytes());
        header[13..17].copy_from_slice(&(payload.len() as u32).to_be_bytes());

        // Write header and payload
        stream.write_all(&header).await.map_err(|e| {
            error!("Failed to write frame header: {}", e);
            crate::ProtocolError::Io(e)
        })?;

        if !payload.is_empty() {
            stream.write_all(payload).await.map_err(|e| {
                error!("Failed to write frame payload: {}", e);
                crate::ProtocolError::Io(e)
            })?;
        }

        // Update stats
        let frame_size = (17 + payload.len()) as u64;
        self.frames_sent += 1;
        self.bytes_sent += frame_size;
        self.window_bytes += frame_size;

        // Update throughput measurement
        self.update_throughput();

        debug!("Sent frame type {:?}, {} bytes", frame_type, payload.len());
        Ok(())
    }

    /// Flush the stream
    pub async fn flush(&mut self) -> Result<()> {
        if let Some(stream) = &mut self.stream {
            stream
                .flush()
                .await
                .map_err(crate::ProtocolError::Io)?;
        }
        Ok(())
    }

    /// Close the connection
    pub async fn close(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            // Try to send end of stream marker
            let _ = self.send_end_of_stream().await;
            let _ = stream.shutdown().await;
            info!(
                "Stream sender closed: {} frames, {} bytes sent",
                self.frames_sent, self.bytes_sent
            );
        }
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    /// Get statistics
    pub fn stats(&self) -> (u64, u64) {
        (self.frames_sent, self.bytes_sent)
    }

    /// Get current throughput in bits per second
    pub fn throughput_bps(&self) -> u64 {
        self.throughput_bps * 8 // Convert bytes/s to bits/s
    }

    /// Get current throughput in kbps
    pub fn throughput_kbps(&self) -> u32 {
        (self.throughput_bps() / 1000) as u32
    }

    /// Update throughput measurement
    fn update_throughput(&mut self) {
        let elapsed = self.window_start.elapsed();
        if elapsed.as_millis() < u128::from(THROUGHPUT_WINDOW_MS) {
            return;
        }

        // Calculate throughput: bytes per second
        let elapsed_secs = elapsed.as_secs_f64();
        if elapsed_secs > 0.0 {
            self.throughput_bps = (self.window_bytes as f64 / elapsed_secs) as u64;
        }

        // Reset window
        self.window_bytes = 0;
        self.window_start = std::time::Instant::now();
    }

    /// Reset throughput measurement (call when connection is established)
    pub fn reset_throughput(&mut self) {
        self.window_bytes = 0;
        self.window_start = std::time::Instant::now();
        self.throughput_bps = 0;
    }
}

impl Default for StreamSender {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_type_conversion() {
        assert_eq!(FrameType::from(0x01), FrameType::Video);
        assert_eq!(FrameType::from(0x02), FrameType::Cursor);
        assert_eq!(FrameType::from(0xFF), FrameType::EndOfStream);
    }

    #[test]
    fn test_sender_creation() {
        let sender = StreamSender::new();
        assert!(!sender.is_connected());
        assert_eq!(sender.stats(), (0, 0));
    }
}
