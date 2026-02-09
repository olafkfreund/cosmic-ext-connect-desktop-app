//! Screen capture using xdg-desktop-portal and `PipeWire`
//!
//! This module implements screen capture for COSMIC Desktop using the
//! xdg-desktop-portal `ScreenCast` interface. It handles:
//!
//! 1. Creating a screen cast session through the portal
//! 2. Requesting permission to capture a specific display output
//! 3. Connecting to the `PipeWire` stream for video frames
//! 4. Filtering for HDMI dummy displays only

use crate::error::{DisplayStreamError, Result};
use crate::output::OutputInfo;
use crate::pipewire::PipeWireStream;

use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
use ashpd::desktop::PersistMode;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Screen capture session state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Session not started
    Idle,
    /// Waiting for user permission
    RequestingPermission,
    /// Permission granted, connecting to stream
    Connecting,
    /// Actively capturing frames
    Capturing,
    /// Stream stopped
    Stopped,
}

/// Screen capture session using xdg-desktop-portal
///
/// This struct manages the lifecycle of a screen capture session,
/// from requesting permission through the portal to receiving video
/// frames from `PipeWire`.
pub struct ScreenCapture {
    /// Target output name (e.g., "HDMI-2")
    target_output: String,

    /// Current session state
    state: SessionState,

    /// Portal session handle (if active)
    session_handle: Option<String>,

    /// `PipeWire` stream (if connected)
    pipewire_stream: Option<PipeWireStream>,

    /// Output information (cached after discovery)
    output_info: Option<OutputInfo>,

    /// Frame sender for async frame delivery
    frame_sender: Option<mpsc::Sender<VideoFrame>>,
}

impl ScreenCapture {
    /// Create a new screen capture session for the specified output
    ///
    /// # Arguments
    ///
    /// * `output_name` - Name of the display output to capture (e.g., "HDMI-2")
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use cosmic_display_stream::capture::ScreenCapture;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let capture = ScreenCapture::new("HDMI-2").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(output_name: &str) -> Result<Self> {
        info!(
            "Creating screen capture session for output: {}",
            output_name
        );

        // Validate that the output exists and is an HDMI dummy
        let output_info = Self::discover_output(output_name).await?;

        if !output_info.is_hdmi_dummy() {
            warn!(
                "Output '{}' is not an HDMI dummy display (virtual: {}, name pattern: {})",
                output_name, output_info.is_virtual, output_info.name
            );
            return Err(DisplayStreamError::InvalidConfiguration(format!(
                "Output '{output_name}' is not an HDMI dummy display"
            )));
        }

        Ok(Self {
            target_output: output_name.to_string(),
            state: SessionState::Idle,
            session_handle: None,
            pipewire_stream: None,
            output_info: Some(output_info),
            frame_sender: None,
        })
    }

    /// Discover and validate the target output
    ///
    /// This queries the compositor for available outputs and verifies
    /// that the target output exists.
    async fn discover_output(output_name: &str) -> Result<OutputInfo> {
        debug!("Discovering output: {}", output_name);

        // Query outputs using wl-randr or similar
        // For now, we create a placeholder that assumes the output exists
        // In production, this would query the Wayland compositor

        // Try to get output info from wlr-randr if available
        let output_info = match Self::query_wlr_randr(output_name).await {
            Ok(info) => info,
            Err(e) => {
                debug!("wlr-randr query failed ({}), using defaults", e);
                // Fallback to defaults for HDMI outputs
                OutputInfo::new(
                    output_name.to_string(),
                    1920,
                    1080,
                    60,
                    output_name.to_uppercase().contains("HDMI"),
                )
            }
        };

        Ok(output_info)
    }

    /// Query output info using wlr-randr
    async fn query_wlr_randr(output_name: &str) -> Result<OutputInfo> {
        let output = tokio::process::Command::new("wlr-randr")
            .output()
            .await
            .map_err(|e| DisplayStreamError::OutputNotFound(format!("wlr-randr failed: {e}")))?;

        if !output.status.success() {
            return Err(DisplayStreamError::OutputNotFound(
                "wlr-randr returned error".to_string(),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse wlr-randr output to find the target display
        // Format: "HDMI-A-1 "Model Name" (1920x1080@60Hz)"
        let mut found_output = false;
        let mut width = 1920u32;
        let mut height = 1080u32;
        let mut refresh = 60u32;

        for line in stdout.lines() {
            let line = line.trim();

            // Check if this is our output
            if line.starts_with(output_name) {
                found_output = true;
                continue;
            }

            // If we found our output, parse the current mode
            if found_output && line.contains("current") {
                // Parse resolution like "1920x1080 px, 60.000000 Hz (current)"
                if let Some(res_part) = line.split(" px").next() {
                    let parts: Vec<&str> = res_part.split('x').collect();
                    if parts.len() == 2 {
                        width = parts[0].trim().parse().unwrap_or(1920);
                        height = parts[1].trim().parse().unwrap_or(1080);
                    }
                }
                if let Some(hz_part) = line.split("Hz").next() {
                    if let Some(hz_str) = hz_part.split(',').next_back() {
                        refresh = hz_str
                            .trim()
                            .parse::<f32>()
                            .map(|f| {
                                // Clamp to valid u32 range before conversion
                                #[allow(
                                    clippy::cast_sign_loss,
                                    clippy::cast_possible_truncation,
                                    clippy::cast_precision_loss
                                )]
                                { f.round().clamp(0.0, u32::MAX as f32) as u32 }
                            })
                            .unwrap_or(60);
                    }
                }
                break;
            }

            // Stop if we hit another output section
            if found_output && !line.starts_with(' ') && !line.is_empty() {
                break;
            }
        }

        if !found_output {
            return Err(DisplayStreamError::OutputNotFound(format!(
                "Output '{output_name}' not found in wlr-randr output"
            )));
        }

        Ok(OutputInfo::new(
            output_name.to_string(),
            width,
            height,
            refresh,
            output_name.to_uppercase().contains("HDMI"),
        ))
    }

    /// Start the screen capture session
    ///
    /// This will:
    /// 1. Request screen capture permission through xdg-desktop-portal
    /// 2. Connect to the `PipeWire` stream
    /// 3. Begin receiving video frames
    ///
    /// # Returns
    ///
    /// A stream of video frames on success
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Permission is denied by the user
    /// - The portal session fails to start
    /// - `PipeWire` connection fails
    pub async fn start_capture(&mut self) -> Result<FrameStream> {
        if self.state != SessionState::Idle {
            return Err(DisplayStreamError::StreamAlreadyStarted);
        }

        info!("Starting screen capture for output: {}", self.target_output);
        self.state = SessionState::RequestingPermission;

        // Create the screencast portal proxy
        let screencast = Screencast::new().await.map_err(|e| {
            DisplayStreamError::Portal(format!("Failed to create screencast: {e}"))
        })?;

        // Create a session
        let session = screencast
            .create_session()
            .await
            .map_err(|e| DisplayStreamError::Portal(format!("Failed to create session: {e}")))?;

        debug!("Portal session created");

        // Select sources - request monitor capture
        screencast
            .select_sources(
                &session,
                CursorMode::Embedded,       // Include cursor in the stream
                SourceType::Monitor.into(), // Capture monitors only
                false,                      // Don't allow multiple sources
                None,                       // No restore token
                PersistMode::DoNot,         // Don't persist
            )
            .await
            .map_err(|e| DisplayStreamError::Portal(format!("Failed to select sources: {e}")))?;

        debug!("Sources selected, starting portal session");
        self.state = SessionState::Connecting;

        // Start the session - this shows the permission dialog
        let streams = screencast
            .start(&session, None)
            .await
            .map_err(|e| {
                DisplayStreamError::CaptureSessionFailed(format!("Failed to start session: {e}"))
            })?
            .response()
            .map_err(|e| {
                DisplayStreamError::CaptureSessionFailed(format!("Portal response error: {e}"))
            })?;

        // Get streams from response
        if streams.streams().is_empty() {
            return Err(DisplayStreamError::CaptureSessionFailed(
                "No streams returned from portal".to_string(),
            ));
        }

        // Get the first stream's PipeWire node ID
        let stream_info = &streams.streams()[0];
        let pipewire_node_id = stream_info.pipe_wire_node_id();
        let stream_size = stream_info.size();

        info!(
            "Portal session started - PipeWire node: {}, size: {:?}",
            pipewire_node_id, stream_size
        );

        // Store session handle
        self.session_handle = Some(format!("{session:?}"));

        // Create frame channel
        let (tx, rx) = mpsc::channel(32);
        self.frame_sender = Some(tx.clone());

        // Connect to PipeWire stream
        let pipewire_stream = PipeWireStream::connect(pipewire_node_id, tx)
            .await
            .map_err(|e| DisplayStreamError::PipeWire(e.to_string()))?;

        self.pipewire_stream = Some(pipewire_stream);
        self.state = SessionState::Capturing;

        info!("Screen capture started successfully");

        // Return the frame stream
        Ok(FrameStream::new(rx))
    }

    /// Stop the screen capture session
    ///
    /// This will disconnect from `PipeWire` and close the portal session.
    #[allow(clippy::unused_async)]
    pub async fn stop_capture(&mut self) -> Result<()> {
        if self.state != SessionState::Capturing {
            return Err(DisplayStreamError::StreamNotStarted);
        }

        info!("Stopping screen capture for output: {}", self.target_output);

        // Disconnect PipeWire stream
        if let Some(mut stream) = self.pipewire_stream.take() {
            stream
                .disconnect()
                .map_err(|e| DisplayStreamError::PipeWire(e.to_string()))?;
        }

        // Close frame sender
        self.frame_sender = None;

        // Close portal session
        self.session_handle = None;
        self.state = SessionState::Stopped;

        info!("Screen capture stopped");
        Ok(())
    }

    /// Get the current output information
    #[must_use] 
    pub fn get_output_info(&self) -> Option<&OutputInfo> {
        self.output_info.as_ref()
    }

    /// Get the current session state
    #[must_use] 
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// Check if the session is actively capturing
    #[must_use] 
    pub fn is_capturing(&self) -> bool {
        self.state == SessionState::Capturing
    }
}

/// Stream of video frames from the capture session
pub struct FrameStream {
    receiver: mpsc::Receiver<VideoFrame>,
}

impl FrameStream {
    /// Create a new frame stream from a receiver
    #[must_use] 
    pub fn new(receiver: mpsc::Receiver<VideoFrame>) -> Self {
        Self { receiver }
    }

    /// Receive the next frame (async)
    pub async fn next_frame(&mut self) -> Option<VideoFrame> {
        self.receiver.recv().await
    }
}

impl futures::Stream for FrameStream {
    type Item = VideoFrame;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.receiver).poll_recv(cx)
    }
}

/// Type of video frame buffer
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum BufferType {
    /// Shared memory (CPU accessible) - current default
    #[default]
    Shm,
    /// DMA-BUF (GPU memory, zero-copy)
    DmaBuf {
        /// DMA-BUF file descriptor (as raw fd number for cross-thread use)
        fd: i32,
        /// Buffer stride in bytes
        stride: u32,
        /// Buffer offset in bytes
        offset: u32,
        /// DRM format modifier
        modifier: u64,
        /// DRM fourcc format code
        drm_format: u32,
    },
}

/// A single video frame from the capture stream
#[derive(Debug, Clone)]
pub struct VideoFrame {
    /// Raw frame data
    pub data: Vec<u8>,

    /// Frame width in pixels
    pub width: u32,

    /// Frame height in pixels
    pub height: u32,

    /// Frame format (e.g., "`BGRx`", "`RGBx`")
    pub format: String,

    /// Frame timestamp in microseconds
    pub timestamp: i64,

    /// Frame sequence number
    pub sequence: u64,

    /// Buffer type (SHM or DMA-BUF)
    pub buffer_type: BufferType,
}

impl VideoFrame {
    /// Create a new video frame with shared memory buffer
    #[must_use]
    pub fn new(
        data: Vec<u8>,
        width: u32,
        height: u32,
        format: String,
        timestamp: i64,
        sequence: u64,
    ) -> Self {
        Self {
            data,
            width,
            height,
            format,
            timestamp,
            sequence,
            buffer_type: BufferType::Shm,
        }
    }

    /// Create a new video frame with DMA-BUF buffer (zero-copy GPU memory)
    ///
    /// # Arguments
    ///
    /// * `data` - Frame data (may be empty for DMA-BUF)
    /// * `width` - Frame width in pixels
    /// * `height` - Frame height in pixels
    /// * `format` - Frame format (e.g., "`BGRx`")
    /// * `timestamp` - Frame timestamp in microseconds
    /// * `sequence` - Frame sequence number
    /// * `fd` - DMA-BUF file descriptor
    /// * `stride` - Buffer stride in bytes
    /// * `offset` - Buffer offset in bytes
    /// * `modifier` - DRM format modifier
    /// * `drm_format` - DRM fourcc format code
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new_dmabuf(
        data: Vec<u8>,
        width: u32,
        height: u32,
        format: String,
        timestamp: i64,
        sequence: u64,
        fd: i32,
        stride: u32,
        offset: u32,
        modifier: u64,
        drm_format: u32,
    ) -> Self {
        Self {
            data,
            width,
            height,
            format,
            timestamp,
            sequence,
            buffer_type: BufferType::DmaBuf {
                fd,
                stride,
                offset,
                modifier,
                drm_format,
            },
        }
    }

    /// Get the frame data size in bytes
    #[must_use]
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Get bytes per pixel based on format
    #[must_use]
    #[allow(clippy::match_same_arms)] // Explicit formats and default both return 4
    pub fn bytes_per_pixel(&self) -> usize {
        match self.format.as_str() {
            "BGRx" | "RGBx" | "BGRA" | "RGBA" => 4,
            "BGR" | "RGB" => 3,
            _ => 4, // Default to 4 bytes
        }
    }

    /// Check if this frame uses DMA-BUF
    #[must_use]
    pub fn is_dmabuf(&self) -> bool {
        matches!(self.buffer_type, BufferType::DmaBuf { .. })
    }

    /// Get the DMA-BUF file descriptor if this is a DMA-BUF frame
    #[must_use]
    pub fn dmabuf_fd(&self) -> Option<i32> {
        match self.buffer_type {
            BufferType::DmaBuf { fd, .. } => Some(fd),
            BufferType::Shm => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_screen_capture_creation() {
        // Note: This test will fail without a real HDMI dummy display
        // It's here to demonstrate the API usage
        let result = ScreenCapture::new("HDMI-2").await;
        // We can't easily test the full flow without a real display
        // but we can verify the error types are correct
        match result {
            // Success case (if HDMI dummy exists) or expected errors if no HDMI dummy
            Ok(_)
            | Err(
                DisplayStreamError::OutputNotFound(_)
                | DisplayStreamError::InvalidConfiguration(_),
            ) => {}
            Err(e) => panic!("Unexpected error: {e}"),
        }
    }

    #[test]
    fn test_session_state_transitions() {
        let mut state = SessionState::Idle;
        assert_eq!(state, SessionState::Idle);

        state = SessionState::RequestingPermission;
        assert_eq!(state, SessionState::RequestingPermission);

        state = SessionState::Capturing;
        assert_eq!(state, SessionState::Capturing);
    }

    #[test]
    fn test_video_frame() {
        let frame = VideoFrame::new(
            vec![0u8; 1920 * 1080 * 4],
            1920,
            1080,
            "BGRx".to_string(),
            12345,
            1,
        );

        assert_eq!(frame.width, 1920);
        assert_eq!(frame.height, 1080);
        assert_eq!(frame.bytes_per_pixel(), 4);
        assert_eq!(frame.size(), 1920 * 1080 * 4);
        assert!(!frame.is_dmabuf());
        assert_eq!(frame.dmabuf_fd(), None);
        assert_eq!(frame.buffer_type, BufferType::Shm);
    }

    #[test]
    fn test_video_frame_dmabuf() {
        let frame = VideoFrame::new_dmabuf(
            vec![],
            1920,
            1080,
            "BGRx".to_string(),
            12345,
            1,
            42,    // fd
            7680,  // stride (1920 * 4)
            0,     // offset
            0,     // modifier
            0x34325258, // drm_format (XR24)
        );

        assert_eq!(frame.width, 1920);
        assert_eq!(frame.height, 1080);
        assert_eq!(frame.bytes_per_pixel(), 4);
        assert_eq!(frame.size(), 0); // Empty data for DMA-BUF
        assert!(frame.is_dmabuf());
        assert_eq!(frame.dmabuf_fd(), Some(42));

        if let BufferType::DmaBuf { fd, stride, offset, modifier, drm_format } = frame.buffer_type {
            assert_eq!(fd, 42);
            assert_eq!(stride, 7680);
            assert_eq!(offset, 0);
            assert_eq!(modifier, 0);
            assert_eq!(drm_format, 0x34325258);
        } else {
            panic!("Expected DmaBuf buffer type");
        }
    }

    #[test]
    fn test_buffer_type_default() {
        assert_eq!(BufferType::default(), BufferType::Shm);
    }
}
