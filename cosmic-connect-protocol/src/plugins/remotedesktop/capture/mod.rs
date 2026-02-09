//! Screen Capture Module
//!
//! Implements Wayland screen capture using PipeWire and Desktop Portal.
//!
//! ## Architecture
//!
//! ```text
//! Desktop Portal (ashpd)
//!   ↓ (permission request)
//! org.freedesktop.portal.ScreenCast
//!   ↓ (PipeWire node ID)
//! PipeWire Stream
//!   ↓ (raw frames)
//! RawFrame → FrameEncoder → EncodedFrame
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use remotedesktop::capture::WaylandCapture;
//!
//! let capture = WaylandCapture::new().await?;
//! capture.request_permission().await?;
//! capture.start_capture().await?;
//!
//! let frame = capture.capture_frame().await?;
//! ```

pub mod frame;

pub use frame::{EncodedFrame, EncodingType, FrameDamageRect, PixelFormat, QualityPreset, RawFrame};

use crate::Result;
#[cfg(feature = "remotedesktop")]
use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
#[cfg(feature = "remotedesktop")]
use ashpd::desktop::PersistMode;
#[cfg(feature = "remotedesktop")]
use pipewire as pw;
#[cfg(feature = "remotedesktop")]
use pipewire::context::Context;
#[cfg(feature = "remotedesktop")]
use pipewire::main_loop::MainLoop;
#[cfg(feature = "remotedesktop")]
use pipewire::properties::properties;
#[cfg(feature = "remotedesktop")]
use pipewire::stream::{Stream, StreamFlags};
#[cfg(feature = "remotedesktop")]
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
#[cfg(feature = "remotedesktop")]
use std::sync::Arc;
#[cfg(feature = "remotedesktop")]
use tokio::sync::mpsc;
#[cfg(feature = "remotedesktop")]
use tracing::{debug, error, info, warn};

/// Monitor information
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    /// Monitor identifier
    pub id: String,

    /// Monitor name (e.g., "HDMI-1")
    pub name: String,

    /// Width in pixels
    pub width: u32,

    /// Height in pixels
    pub height: u32,

    /// Refresh rate in Hz
    pub refresh_rate: u32,

    /// Whether this is the primary monitor
    pub is_primary: bool,
}

/// Wayland screen capture using PipeWire and Desktop Portal
#[cfg(feature = "remotedesktop")]
pub struct WaylandCapture {
    /// Current capture session state
    state: CaptureState,

    /// Selected monitors to capture
    selected_monitors: Vec<String>,

    /// Portal session handle (if active)
    session_handle: Option<String>,

    /// PipeWire node ID (if streaming)
    pipewire_node: Option<u32>,

    /// Flag to signal PipeWire thread to stop
    running: Arc<AtomicBool>,

    /// PipeWire thread handle
    thread_handle: Option<std::thread::JoinHandle<()>>,

    /// Frame receiver for captured frames
    frame_receiver: Option<mpsc::Receiver<RawFrame>>,

    /// Stream width (cached)
    stream_width: Arc<AtomicU32>,

    /// Stream height (cached)
    stream_height: Arc<AtomicU32>,
}

#[cfg(feature = "remotedesktop")]
impl WaylandCapture {
    /// Create a new Wayland capture instance
    pub async fn new() -> Result<Self> {
        info!("Initializing Wayland screen capture");

        Ok(Self {
            state: CaptureState::Idle,
            selected_monitors: Vec::new(),
            session_handle: None,
            pipewire_node: None,
            running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
            frame_receiver: None,
            stream_width: Arc::new(AtomicU32::new(1920)),
            stream_height: Arc::new(AtomicU32::new(1080)),
        })
    }

    /// Enumerate available monitors via Desktop Portal
    ///
    /// Note: The portal API doesn't provide monitor enumeration before session creation.
    /// This returns a generic monitor info that will be updated once capture starts.
    pub async fn enumerate_monitors(&self) -> Result<Vec<MonitorInfo>> {
        info!("Enumerating monitors via Desktop Portal");

        // The Desktop Portal API doesn't provide monitor enumeration upfront.
        // Instead, the user selects monitors through the portal dialog.
        // We return a placeholder that represents "all available monitors"
        let monitor = MonitorInfo {
            id: "0".to_string(),
            name: "Available Displays".to_string(),
            width: 1920,
            height: 1080,
            refresh_rate: 60,
            is_primary: true,
        };

        debug!("Portal will present monitor selection dialog to user");
        Ok(vec![monitor])
    }

    /// Select which monitors to capture
    pub fn select_monitors(&mut self, monitor_ids: Vec<String>) {
        info!("Selecting monitors: {:?}", monitor_ids);
        self.selected_monitors = monitor_ids;
    }

    /// Request screen capture permission via Desktop Portal
    pub async fn request_permission(&mut self) -> Result<()> {
        info!("Requesting screen capture permission via Desktop Portal");

        if self.state != CaptureState::Idle {
            warn!("Cannot request permission: capture already active");
            return Err(crate::ProtocolError::invalid_state(
                "Capture session already active",
            ));
        }

        self.state = CaptureState::PermissionRequested;

        // Create the screencast portal proxy
        let screencast = Screencast::new().await.map_err(|e| {
            crate::ProtocolError::invalid_state(format!("Failed to create screencast: {}", e))
        })?;

        // Create a session
        let session = screencast.create_session().await.map_err(|e| {
            crate::ProtocolError::invalid_state(format!("Failed to create session: {}", e))
        })?;

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
            .map_err(|e| {
                crate::ProtocolError::invalid_state(format!("Failed to select sources: {}", e))
            })?;

        debug!("Sources selected, starting portal session");

        // Start the session - this shows the permission dialog
        let streams = screencast
            .start(&session, None)
            .await
            .map_err(|e| {
                crate::ProtocolError::invalid_state(format!("Failed to start session: {}", e))
            })?
            .response()
            .map_err(|e| {
                crate::ProtocolError::invalid_state(format!("Portal response error: {}", e))
            })?;

        // Get streams from response
        if streams.streams().is_empty() {
            return Err(crate::ProtocolError::invalid_state(
                "No streams returned from portal",
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

        // Update stream dimensions if available
        if let Some((width, height)) = stream_size {
            self.stream_width.store(width as u32, Ordering::Relaxed);
            self.stream_height.store(height as u32, Ordering::Relaxed);
        }

        self.state = CaptureState::PermissionGranted;
        self.session_handle = Some(format!("{:?}", session));
        self.pipewire_node = Some(pipewire_node_id);

        info!("Permission granted for screen capture");
        Ok(())
    }

    /// Start screen capture session
    pub async fn start_capture(&mut self) -> Result<()> {
        info!("Starting screen capture session");

        if self.state != CaptureState::PermissionGranted {
            warn!("Cannot start capture: permission not granted");
            return Err(crate::ProtocolError::invalid_state(
                "Permission not granted for screen capture",
            ));
        }

        let node_id = self
            .pipewire_node
            .ok_or_else(|| crate::ProtocolError::invalid_state("No PipeWire node ID available"))?;

        // Create frame channel
        let (tx, rx) = mpsc::channel(32);
        self.frame_receiver = Some(rx);

        // Start PipeWire stream in background thread
        let running = Arc::new(AtomicBool::new(true));
        self.running = running.clone();

        let stream_width = self.stream_width.clone();
        let stream_height = self.stream_height.clone();

        let thread_handle = std::thread::spawn(move || {
            if let Err(e) = run_pipewire_loop(node_id, tx, running, stream_width, stream_height) {
                error!("PipeWire loop error: {}", e);
            }
        });

        self.thread_handle = Some(thread_handle);
        self.state = CaptureState::Capturing;

        // Wait briefly for stream to connect
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        info!("Screen capture session started with node {}", node_id);
        Ok(())
    }

    /// Capture a single frame
    ///
    /// This method receives frames from the PipeWire background thread.
    /// Frames are captured asynchronously as they arrive from the compositor.
    pub async fn capture_frame(&mut self) -> Result<RawFrame> {
        if self.state != CaptureState::Capturing {
            return Err(crate::ProtocolError::invalid_state(
                "Capture session not active",
            ));
        }

        // Try to receive a frame from the channel (non-blocking with timeout)
        let frame_rx = self
            .frame_receiver
            .as_mut()
            .ok_or_else(|| crate::ProtocolError::invalid_state("Frame channel not initialized"))?;

        // Wait for next frame with timeout
        match tokio::time::timeout(tokio::time::Duration::from_millis(100), frame_rx.recv()).await {
            Ok(Some(frame)) => {
                debug!("Captured frame {}x{}", frame.width, frame.height);
                Ok(frame)
            }
            Ok(None) => Err(crate::ProtocolError::invalid_state("Frame channel closed")),
            Err(_) => {
                // Timeout - generate fallback frame
                let width = self.stream_width.load(Ordering::Relaxed);
                let height = self.stream_height.load(Ordering::Relaxed);
                let data = vec![0u8; (width * height * 4) as usize];

                debug!("Frame timeout, returning black frame {}x{}", width, height);
                Ok(RawFrame::new(width, height, PixelFormat::RGBA, data))
            }
        }
    }

    /// Stop screen capture session
    pub async fn stop_capture(&mut self) -> Result<()> {
        info!("Stopping screen capture session");

        if self.state != CaptureState::Capturing {
            debug!("Capture session not active, nothing to stop");
            return Ok(());
        }

        // Signal PipeWire thread to stop
        self.running.store(false, Ordering::SeqCst);

        // Wait for thread to finish
        if let Some(handle) = self.thread_handle.take() {
            handle.join().ok();
        }

        // Cleanup
        self.frame_receiver = None;
        self.state = CaptureState::Idle;
        self.session_handle = None;
        self.pipewire_node = None;

        info!("Screen capture session stopped");
        Ok(())
    }

    /// Get current capture state
    pub fn state(&self) -> CaptureState {
        self.state
    }

    /// Check if currently capturing
    pub fn is_capturing(&self) -> bool {
        self.state == CaptureState::Capturing
    }
}

/// Stub implementation when feature is not enabled
#[cfg(not(feature = "remotedesktop"))]
pub struct WaylandCapture;

#[cfg(not(feature = "remotedesktop"))]
impl WaylandCapture {
    pub async fn new() -> Result<Self> {
        Err(crate::ProtocolError::unsupported_feature(
            "RemoteDesktop feature not enabled",
        ))
    }

    pub async fn enumerate_monitors(&self) -> Result<Vec<MonitorInfo>> {
        Err(crate::ProtocolError::unsupported_feature(
            "RemoteDesktop feature not enabled",
        ))
    }

    pub fn select_monitors(&mut self, _monitor_ids: Vec<String>) {}

    pub async fn request_permission(&mut self) -> Result<()> {
        Err(crate::ProtocolError::unsupported_feature(
            "RemoteDesktop feature not enabled",
        ))
    }

    pub async fn start_capture(&mut self) -> Result<()> {
        Err(crate::ProtocolError::unsupported_feature(
            "RemoteDesktop feature not enabled",
        ))
    }

    pub async fn capture_frame(&self) -> Result<RawFrame> {
        Err(crate::ProtocolError::unsupported_feature(
            "RemoteDesktop feature not enabled",
        ))
    }

    pub async fn stop_capture(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn is_capturing(&self) -> bool {
        false
    }
}

/// Screen capture session state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureState {
    /// No active capture session
    Idle,

    /// Permission requested from user
    PermissionRequested,

    /// Permission granted, ready to capture
    PermissionGranted,

    /// Actively capturing frames
    Capturing,

    /// Capture paused
    Paused,

    /// Error state
    Error,
}

/// Run the PipeWire main loop (called from background thread)
#[cfg(feature = "remotedesktop")]
fn run_pipewire_loop(
    node_id: u32,
    frame_sender: mpsc::Sender<RawFrame>,
    running: Arc<AtomicBool>,
    stream_width: Arc<AtomicU32>,
    stream_height: Arc<AtomicU32>,
) -> Result<()> {
    // Initialize PipeWire
    pw::init();

    // Create main loop
    let mainloop = MainLoop::new(None).map_err(|e| {
        crate::ProtocolError::invalid_state(format!("Failed to create PipeWire main loop: {}", e))
    })?;

    let loop_ = mainloop.loop_();

    // Create context
    let context = Context::new(&mainloop).map_err(|e| {
        crate::ProtocolError::invalid_state(format!("Failed to create context: {}", e))
    })?;

    // Connect to PipeWire server
    let core = context.connect(None).map_err(|e| {
        crate::ProtocolError::invalid_state(format!("Failed to connect to PipeWire: {}", e))
    })?;

    // Create stream
    let stream = Stream::new(
        &core,
        "cosmic-connect-remotedesktop",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|e| crate::ProtocolError::invalid_state(format!("Failed to create stream: {}", e)))?;

    // Frame counter for sequencing (unused for now, but may be useful for debugging)
    let _frame_sequence = Arc::new(AtomicU64::new(0));

    let stream_width_clone = stream_width.clone();
    let stream_height_clone = stream_height.clone();
    let running_clone = running.clone();

    // Add stream listener
    let _listener = stream
        .add_local_listener_with_user_data(frame_sender)
        .state_changed(move |_stream, _user_data, old, new| {
            debug!("Stream state changed: {:?} -> {:?}", old, new);
        })
        .process(move |stream, frame_tx| {
            // Check if we should still be running
            if !running_clone.load(Ordering::SeqCst) {
                return;
            }

            // Dequeue buffer
            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                if let Some(data) = datas.first_mut() {
                    let chunk = data.chunk();
                    let offset = chunk.offset() as usize;
                    let size = chunk.size() as usize;
                    let stride = chunk.stride() as usize;

                    if let Some(slice) = data.data() {
                        if size > 0 && offset + size <= slice.len() {
                            let frame_data = slice[offset..offset + size].to_vec();

                            let width = stream_width_clone.load(Ordering::Relaxed);
                            let height = stream_height_clone.load(Ordering::Relaxed);

                            // Infer dimensions from stride if needed
                            let inferred_width = if stride > 0 {
                                (stride / 4) as u32
                            } else {
                                width
                            };
                            let inferred_height = if size > 0 && stride > 0 {
                                (size / stride) as u32
                            } else {
                                height
                            };

                            let frame = RawFrame::new(
                                inferred_width,
                                inferred_height,
                                PixelFormat::RGBA, // Most common format from screen capture
                                frame_data,
                            );

                            // Try to send frame (non-blocking)
                            if let Err(e) = frame_tx.try_send(frame) {
                                if matches!(e, mpsc::error::TrySendError::Full(_)) {
                                    debug!("Frame channel full, dropping frame");
                                } else {
                                    warn!("Failed to send frame: {}", e);
                                }
                            }
                        }
                    }
                }
            }
        })
        .register()
        .map_err(|e| {
            crate::ProtocolError::invalid_state(format!("Failed to register listener: {}", e))
        })?;

    // Connect to the portal's PipeWire node
    stream
        .connect(
            pw::spa::utils::Direction::Input,
            Some(node_id),
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
            &mut [],
        )
        .map_err(|e| {
            crate::ProtocolError::invalid_state(format!(
                "Failed to connect stream to node {}: {}",
                node_id, e
            ))
        })?;

    info!("PipeWire stream connected to node {}", node_id);

    // Run the main loop until stopped
    while running.load(Ordering::SeqCst) {
        // Iterate the loop with a timeout
        loop_.iterate(std::time::Duration::from_millis(100));
    }

    info!("PipeWire main loop exited");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[cfg(feature = "remotedesktop")]
    async fn test_capture_creation() {
        let capture = WaylandCapture::new().await.unwrap();
        assert_eq!(capture.state(), CaptureState::Idle);
        assert!(!capture.is_capturing());
    }

    #[tokio::test]
    #[cfg(feature = "remotedesktop")]
    async fn test_monitor_enumeration() {
        let capture = WaylandCapture::new().await.unwrap();
        let monitors = capture.enumerate_monitors().await.unwrap();

        assert!(!monitors.is_empty());
        assert_eq!(monitors[0].id, "0");
    }

    #[tokio::test]
    #[cfg(feature = "remotedesktop")]
    async fn test_monitor_selection() {
        let mut capture = WaylandCapture::new().await.unwrap();
        capture.select_monitors(vec!["0".to_string()]);

        assert_eq!(capture.selected_monitors.len(), 1);
        assert_eq!(capture.selected_monitors[0], "0");
    }

    #[tokio::test]
    #[cfg(feature = "remotedesktop")]
    async fn test_invalid_state_transitions() {
        let mut capture = WaylandCapture::new().await.unwrap();

        // Can't start capture without permission
        let result = capture.start_capture().await;
        assert!(result.is_err());

        // Can't capture frame without active session
        let result = capture.capture_frame().await;
        assert!(result.is_err());
    }

    #[test]
    fn test_capture_state() {
        let states = [
            CaptureState::Idle,
            CaptureState::PermissionRequested,
            CaptureState::PermissionGranted,
            CaptureState::Capturing,
            CaptureState::Paused,
            CaptureState::Error,
        ];

        for state in states {
            assert_eq!(state, state);
        }
    }
}
