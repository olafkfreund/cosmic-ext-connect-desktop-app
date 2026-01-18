//! Streaming Session for Frame Pipeline
//!
//! Manages the async pipeline from screen capture to encoded frames ready for VNC transmission.
//!
//! ## Architecture
//!
//! ```text
//! Capture Thread           Encoding Thread         Output
//! ┌─────────────┐         ┌──────────────┐       ┌────────┐
//! │  RawFrame   │ ──────> │ FrameEncoder │ ────> │ Output │
//! │   Queue     │ channel │              │       │ Stream │
//! └─────────────┘         └──────────────┘       └────────┘
//!      30 FPS            Async encoding          To VNC
//! ```

use crate::plugins::remotedesktop::capture::{
    EncodedFrame, QualityPreset, RawFrame, WaylandCapture,
};
use crate::plugins::remotedesktop::vnc::encoding::FrameEncoder;
use crate::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

/// Streaming session configuration
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Target frames per second
    pub target_fps: u32,

    /// Quality preset
    pub quality: QualityPreset,

    /// Frame buffer size (bounded channel)
    pub buffer_size: usize,

    /// Enable frame skipping if encoder can't keep up
    pub allow_frame_skip: bool,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            target_fps: 30,
            quality: QualityPreset::Medium,
            buffer_size: 3, // Small buffer to reduce latency
            allow_frame_skip: true,
        }
    }
}

/// Streaming session state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    /// Not started
    Idle,
    /// Running
    Streaming,
    /// Paused
    Paused,
    /// Stopped
    Stopped,
}

/// Streaming session statistics
#[derive(Debug, Clone, Default)]
pub struct StreamStats {
    /// Frames captured
    pub frames_captured: u64,

    /// Frames encoded
    pub frames_encoded: u64,

    /// Frames skipped (when buffer full)
    pub frames_skipped: u64,

    /// Current FPS
    pub current_fps: f64,

    /// Average frame time
    pub avg_frame_time: Duration,
}

/// Streaming session for async frame pipeline
#[cfg(feature = "remotedesktop")]
pub struct StreamingSession {
    /// Configuration
    config: StreamConfig,

    /// Current state
    state: Arc<RwLock<StreamState>>,

    /// Statistics
    stats: Arc<RwLock<StreamStats>>,

    /// Encoded frame output channel (receiver side)
    output_rx: Option<mpsc::Receiver<EncodedFrame>>,

    /// Frame encoding handle
    encoder_handle: Option<tokio::task::JoinHandle<()>>,

    /// Capture handle
    capture_handle: Option<tokio::task::JoinHandle<()>>,
}

#[cfg(feature = "remotedesktop")]
impl StreamingSession {
    /// Create a new streaming session
    pub fn new(config: StreamConfig) -> Self {
        info!("Creating streaming session with {:?}", config);

        Self {
            config,
            state: Arc::new(RwLock::new(StreamState::Idle)),
            stats: Arc::new(RwLock::new(StreamStats::default())),
            output_rx: None,
            encoder_handle: None,
            capture_handle: None,
        }
    }

    /// Start the streaming session
    pub async fn start(&mut self, capture: WaylandCapture) -> Result<()> {
        let mut state = self.state.write().await;
        if *state != StreamState::Idle {
            return Err(crate::ProtocolError::invalid_state(
                "Streaming session already started",
            ));
        }

        info!(
            "Starting streaming session at {} FPS",
            self.config.target_fps
        );

        // Create channels
        let (raw_tx, raw_rx) = mpsc::channel::<RawFrame>(self.config.buffer_size);
        let (encoded_tx, encoded_rx) = mpsc::channel::<EncodedFrame>(self.config.buffer_size);

        // Store output receiver
        self.output_rx = Some(encoded_rx);

        // Spawn capture task
        let capture_state = self.state.clone();
        let capture_stats = self.stats.clone();
        let target_fps = self.config.target_fps;
        let allow_skip = self.config.allow_frame_skip;

        self.capture_handle = Some(tokio::spawn(async move {
            Self::capture_loop(
                capture,
                raw_tx,
                capture_state,
                capture_stats,
                target_fps,
                allow_skip,
            )
            .await;
        }));

        // Spawn encoding task
        let encoder_state = self.state.clone();
        let encoder_stats = self.stats.clone();
        let quality = self.config.quality;

        self.encoder_handle = Some(tokio::spawn(async move {
            Self::encoding_loop(raw_rx, encoded_tx, encoder_state, encoder_stats, quality).await;
        }));

        *state = StreamState::Streaming;
        info!("Streaming session started");

        Ok(())
    }

    /// Capture loop (runs in separate task)
    async fn capture_loop(
        capture: WaylandCapture,
        tx: mpsc::Sender<RawFrame>,
        state: Arc<RwLock<StreamState>>,
        stats: Arc<RwLock<StreamStats>>,
        target_fps: u32,
        allow_skip: bool,
    ) {
        let frame_duration = Duration::from_millis(1000 / target_fps as u64);
        let mut ticker = interval(frame_duration);
        let mut last_fps_check = Instant::now();
        let mut frames_since_check = 0u64;

        loop {
            ticker.tick().await;

            // Check state
            let current_state = *state.read().await;
            if current_state == StreamState::Stopped {
                info!("Capture loop stopped");
                break;
            }
            if current_state == StreamState::Paused {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }

            // Capture frame
            match capture.capture_frame().await {
                Ok(frame) => {
                    // Update stats
                    {
                        let mut stats = stats.write().await;
                        stats.frames_captured += 1;
                        frames_since_check += 1;

                        // Calculate FPS every second
                        let elapsed = last_fps_check.elapsed();
                        if elapsed >= Duration::from_secs(1) {
                            stats.current_fps = frames_since_check as f64 / elapsed.as_secs_f64();
                            last_fps_check = Instant::now();
                            frames_since_check = 0;
                        }
                    }

                    // Send to encoder (non-blocking if configured)
                    match if allow_skip {
                        tx.try_send(frame)
                    } else {
                        tx.send(frame).await.map_err(|_| {
                            mpsc::error::TrySendError::Closed(RawFrame::new(
                                0,
                                0,
                                crate::plugins::remotedesktop::capture::PixelFormat::RGBA,
                                vec![],
                            ))
                        })
                    } {
                        Ok(_) => {}
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            // Buffer full, skip frame
                            debug!("Frame buffer full, skipping frame");
                            let mut stats = stats.write().await;
                            stats.frames_skipped += 1;
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            warn!("Encoding channel closed, stopping capture");
                            break;
                        }
                    }
                }
                Err(e) => {
                    error!("Frame capture error: {}", e);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// Encoding loop (runs in separate task)
    async fn encoding_loop(
        mut rx: mpsc::Receiver<RawFrame>,
        tx: mpsc::Sender<EncodedFrame>,
        state: Arc<RwLock<StreamState>>,
        stats: Arc<RwLock<StreamStats>>,
        quality: QualityPreset,
    ) {
        let encoder = Arc::new(std::sync::Mutex::new(FrameEncoder::new(quality)));
        let mut frame_times = Vec::with_capacity(30);

        while let Some(raw_frame) = rx.recv().await {
            // Check state
            let current_state = *state.read().await;
            if current_state == StreamState::Stopped {
                info!("Encoding loop stopped");
                break;
            }

            let start = Instant::now();

            // Encode frame (runs in blocking task to not block tokio executor)
            let encoder_clone = encoder.clone();
            let result = tokio::task::spawn_blocking(move || {
                let mut encoder = encoder_clone.lock().unwrap();
                encoder.encode(&raw_frame)
            })
            .await;

            match result {
                Ok(Ok(encoded)) => {
                    let encode_time = start.elapsed();
                    frame_times.push(encode_time);
                    if frame_times.len() > 30 {
                        frame_times.remove(0);
                    }

                    // Update stats
                    {
                        let mut stats = stats.write().await;
                        stats.frames_encoded += 1;
                        stats.avg_frame_time =
                            frame_times.iter().sum::<Duration>() / frame_times.len() as u32;
                    }

                    // Send to output
                    if let Err(e) = tx.send(encoded).await {
                        warn!("Output channel closed: {}", e);
                        break;
                    }
                }
                Ok(Err(e)) => {
                    error!("Encoding error: {}", e);
                }
                Err(e) => {
                    error!("Encoding task error: {}", e);
                }
            }
        }
    }

    /// Get next encoded frame (non-blocking)
    pub async fn next_frame(&mut self) -> Option<EncodedFrame> {
        if let Some(rx) = &mut self.output_rx {
            rx.recv().await
        } else {
            None
        }
    }

    /// Get current statistics
    pub async fn stats(&self) -> StreamStats {
        self.stats.read().await.clone()
    }

    /// Get current state
    pub async fn state(&self) -> StreamState {
        *self.state.read().await
    }

    /// Pause streaming
    pub async fn pause(&mut self) -> Result<()> {
        let mut state = self.state.write().await;
        if *state != StreamState::Streaming {
            return Err(crate::ProtocolError::invalid_state(
                "Can only pause when streaming",
            ));
        }
        *state = StreamState::Paused;
        info!("Streaming session paused");
        Ok(())
    }

    /// Resume streaming
    pub async fn resume(&mut self) -> Result<()> {
        let mut state = self.state.write().await;
        if *state != StreamState::Paused {
            return Err(crate::ProtocolError::invalid_state(
                "Can only resume when paused",
            ));
        }
        *state = StreamState::Streaming;
        info!("Streaming session resumed");
        Ok(())
    }

    /// Stop streaming session
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping streaming session");

        // Update state to stop tasks
        *self.state.write().await = StreamState::Stopped;

        // Wait for tasks to finish
        if let Some(handle) = self.capture_handle.take() {
            let _ = handle.await;
        }
        if let Some(handle) = self.encoder_handle.take() {
            let _ = handle.await;
        }

        // Close output channel
        self.output_rx = None;

        let stats = self.stats.read().await;
        info!(
            "Streaming session stopped - captured: {}, encoded: {}, skipped: {}",
            stats.frames_captured, stats.frames_encoded, stats.frames_skipped
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_config_default() {
        let config = StreamConfig::default();
        assert_eq!(config.target_fps, 30);
        assert_eq!(config.quality, QualityPreset::Medium);
        assert!(config.allow_frame_skip);
    }

    #[test]
    fn test_stream_state() {
        let states = [
            StreamState::Idle,
            StreamState::Streaming,
            StreamState::Paused,
            StreamState::Stopped,
        ];

        for state in states {
            assert_eq!(state, state);
        }
    }

    #[tokio::test]
    #[cfg(feature = "remotedesktop")]
    async fn test_session_creation() {
        let config = StreamConfig::default();
        let session = StreamingSession::new(config);

        assert_eq!(session.state().await, StreamState::Idle);
    }
}
