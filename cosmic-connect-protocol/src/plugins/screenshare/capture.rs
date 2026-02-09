//! Screen Capture for Screen Share
//!
//! Uses GStreamer with PipeWire source for Wayland screen capture.
//! Encodes to H.264 for network transmission.

#[cfg(feature = "screenshare")]
use crate::Result;
#[cfg(feature = "screenshare")]
use gstreamer as gst;
#[cfg(feature = "screenshare")]
use gstreamer::prelude::*;
#[cfg(feature = "screenshare")]
use gstreamer_app as gst_app;
#[cfg(feature = "screenshare")]
use std::sync::Arc;
#[cfg(feature = "screenshare")]
use tokio::sync::Mutex;
#[cfg(feature = "screenshare")]
use tracing::{debug, info, warn};

/// Screen capture configuration
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    /// Target frame rate
    pub fps: u32,
    /// Target bitrate in kbps
    pub bitrate_kbps: u32,
    /// Video width (0 = auto)
    pub width: u32,
    /// Video height (0 = auto)
    pub height: u32,
    /// PipeWire node ID (from portal session)
    pub pipewire_node_id: Option<u32>,
    /// PipeWire file descriptor (from portal session)
    pub pipewire_fd: Option<i32>,
    /// Include audio capture
    pub include_audio: bool,
    /// Whether cursor metadata mode is active (cursor sent separately, not baked into video)
    pub cursor_metadata_mode: bool,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            fps: 30,
            bitrate_kbps: 2000,
            width: 0,
            height: 0,
            pipewire_node_id: None,
            pipewire_fd: None,
            include_audio: false,
            cursor_metadata_mode: false,
        }
    }
}

/// Encoded frame data
#[derive(Debug, Clone)]
pub struct EncodedFrame {
    /// H.264 NAL unit data
    pub data: Vec<u8>,
    /// Presentation timestamp in nanoseconds
    pub pts: u64,
    /// Duration in nanoseconds
    pub duration: u64,
    /// Is this a keyframe
    pub is_keyframe: bool,
}

/// Screen capture using GStreamer
#[cfg(feature = "screenshare")]
pub struct ScreenCapture {
    pipeline: Option<gst::Pipeline>,
    appsink: Option<gst_app::AppSink>,
    encoder: Option<gst::Element>,
    config: CaptureConfig,
    running: Arc<Mutex<bool>>,
    paused: Arc<Mutex<bool>>,
    current_bitrate_kbps: u32,
}

#[cfg(feature = "screenshare")]
impl ScreenCapture {
    /// Create a new screen capture instance
    pub fn new(config: CaptureConfig) -> Self {
        let current_bitrate_kbps = config.bitrate_kbps;
        Self {
            pipeline: None,
            appsink: None,
            encoder: None,
            running: Arc::new(Mutex::new(false)),
            paused: Arc::new(Mutex::new(false)),
            current_bitrate_kbps,
            config,
        }
    }

    /// Initialize GStreamer and create the capture pipeline
    ///
    /// For Wayland, this requires a PipeWire fd and node_id from the XDG Desktop Portal.
    /// Call `request_screen_share()` first to get these via D-Bus.
    pub fn init(&mut self) -> Result<()> {
        gst::init()
            .map_err(|e| crate::ProtocolError::Plugin(format!("GStreamer init failed: {}", e)))?;

        let pipeline = self.create_pipeline()?;

        let appsink = pipeline
            .by_name("sink")
            .ok_or_else(|| crate::ProtocolError::Plugin("Failed to get appsink".to_string()))?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| crate::ProtocolError::Plugin("Failed to downcast appsink".to_string()))?;

        // Get encoder element for dynamic bitrate control
        let encoder = pipeline.by_name("encoder").ok_or_else(|| {
            crate::ProtocolError::Plugin("Failed to get encoder element".to_string())
        })?;

        self.pipeline = Some(pipeline);
        self.appsink = Some(appsink);
        self.encoder = Some(encoder);

        info!("Screen capture initialized");
        Ok(())
    }

    /// Create the GStreamer pipeline for screen capture
    fn create_pipeline(&self) -> Result<gst::Pipeline> {
        // Build pipeline string based on configuration
        // Note: encoder is named "encoder" for dynamic bitrate control
        let pipeline_str = if let (Some(fd), Some(node_id)) =
            (self.config.pipewire_fd, self.config.pipewire_node_id)
        {
            // PipeWire source with portal fd and node
            if self.config.include_audio {
                // Audio capture implementation approach:
                // The XDG Desktop Portal may provide a separate PipeWire node for audio streams,
                // or a combined audio+video stream depending on the portal implementation.
                // To implement audio capture, the pipeline would need to:
                // 1. Query the portal for available audio nodes (via Restore token)
                // 2. Add a separate pipewiresrc element for the audio stream
                // 3. Use audio encoding (e.g., opusenc) alongside video encoding
                // 4. Multiplex audio and video into a container format (e.g., Matroska/WebM)
                // See: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html
                warn!("Audio capture requested but not yet implemented in GStreamer pipeline");
                format!(
                    "pipewiresrc fd={} path={} do-timestamp=true keepalive-time=1000 ! \
                     videoconvert ! videoscale ! \
                     video/x-raw,framerate={}/1{} ! \
                     x264enc name=encoder tune=zerolatency bitrate={} speed-preset=ultrafast key-int-max=30 ! \
                     video/x-h264,stream-format=byte-stream ! \
                     appsink name=sink emit-signals=true drop=true max-buffers=2",
                    fd,
                    node_id,
                    self.config.fps,
                    self.resolution_caps(),
                    self.config.bitrate_kbps
                )
            } else {
                // Video-only pipeline
                format!(
                    "pipewiresrc fd={} path={} do-timestamp=true keepalive-time=1000 ! \
                     videoconvert ! videoscale ! \
                     video/x-raw,framerate={}/1{} ! \
                     x264enc name=encoder tune=zerolatency bitrate={} speed-preset=ultrafast key-int-max=30 ! \
                     video/x-h264,stream-format=byte-stream ! \
                     appsink name=sink emit-signals=true drop=true max-buffers=2",
                    fd,
                    node_id,
                    self.config.fps,
                    self.resolution_caps(),
                    self.config.bitrate_kbps
                )
            }
        } else {
            // Fallback: use test source for development/testing
            warn!("No PipeWire fd/node_id provided, using test video source");
            format!(
                "videotestsrc pattern=smpte is-live=true ! \
                 video/x-raw,framerate={}/1,width=1280,height=720 ! \
                 videoconvert ! \
                 x264enc name=encoder tune=zerolatency bitrate={} speed-preset=ultrafast key-int-max=30 ! \
                 video/x-h264,stream-format=byte-stream ! \
                 appsink name=sink emit-signals=true drop=true max-buffers=2",
                self.config.fps,
                self.config.bitrate_kbps
            )
        };

        debug!("Creating capture pipeline: {}", pipeline_str);

        let pipeline = gst::parse::launch(&pipeline_str)
            .map_err(|e| crate::ProtocolError::Plugin(format!("Failed to parse pipeline: {}", e)))?
            .downcast::<gst::Pipeline>()
            .map_err(|_| crate::ProtocolError::Plugin("Failed to downcast pipeline".to_string()))?;

        Ok(pipeline)
    }

    /// Generate resolution caps string
    fn resolution_caps(&self) -> String {
        if self.config.width > 0 && self.config.height > 0 {
            format!(",width={},height={}", self.config.width, self.config.height)
        } else {
            String::new()
        }
    }

    /// Start capturing
    pub fn start(&mut self) -> Result<()> {
        let pipeline = self
            .pipeline
            .as_ref()
            .ok_or_else(|| crate::ProtocolError::Plugin("Pipeline not initialized".to_string()))?;

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| crate::ProtocolError::Plugin(format!("Failed to start capture: {}", e)))?;

        info!("Screen capture started");

        // Update running flag asynchronously
        let running = self.running.clone();
        tokio::spawn(async move {
            *running.lock().await = true;
        });

        Ok(())
    }

    /// Stop capturing
    pub fn stop(&mut self) -> Result<()> {
        let Some(pipeline) = &self.pipeline else {
            return Ok(()); // Not initialized, nothing to stop
        };

        pipeline
            .set_state(gst::State::Null)
            .map_err(|e| crate::ProtocolError::Plugin(format!("Failed to stop capture: {}", e)))?;

        info!("Screen capture stopped");

        // Update running flag asynchronously
        let running = self.running.clone();
        let paused = self.paused.clone();
        tokio::spawn(async move {
            *running.lock().await = false;
            *paused.lock().await = false;
        });

        Ok(())
    }

    /// Pause capturing
    pub fn pause(&mut self) -> Result<()> {
        self.set_pipeline_state(gst::State::Paused, true, "pause")
    }

    /// Resume capturing after pause
    pub fn resume(&mut self) -> Result<()> {
        self.set_pipeline_state(gst::State::Playing, false, "resume")
    }

    /// Set pipeline state and update paused flag
    fn set_pipeline_state(&mut self, state: gst::State, paused: bool, action: &str) -> Result<()> {
        let pipeline = self
            .pipeline
            .as_ref()
            .ok_or_else(|| crate::ProtocolError::Plugin("Pipeline not initialized".to_string()))?;

        pipeline.set_state(state).map_err(|e| {
            crate::ProtocolError::Plugin(format!("Failed to {} capture: {}", action, e))
        })?;

        info!("Screen capture {}d", action);

        let paused_flag = self.paused.clone();
        tokio::spawn(async move {
            *paused_flag.lock().await = paused;
        });

        Ok(())
    }

    /// Check if capture is paused
    pub async fn is_paused(&self) -> bool {
        *self.paused.lock().await
    }

    /// Pull the next encoded frame (non-blocking)
    ///
    /// Returns None if no frame is available yet
    pub fn pull_frame(&self) -> Result<Option<EncodedFrame>> {
        let appsink = self
            .appsink
            .as_ref()
            .ok_or_else(|| crate::ProtocolError::Plugin("Appsink not initialized".to_string()))?;

        // Try to pull with a small timeout
        let Some(sample) = appsink.try_pull_sample(gst::ClockTime::from_mseconds(1)) else {
            return Ok(None);
        };

        let buffer = sample
            .buffer()
            .ok_or_else(|| crate::ProtocolError::Plugin("No buffer in sample".to_string()))?;

        let map = buffer
            .map_readable()
            .map_err(|_| crate::ProtocolError::Plugin("Failed to map buffer".to_string()))?;

        let pts = buffer.pts().map_or(0, |t| t.nseconds());
        let duration = buffer.duration().map_or(0, |t| t.nseconds());

        // Check for keyframe (delta unit flag NOT set means keyframe)
        let is_keyframe = !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT);

        Ok(Some(EncodedFrame {
            data: map.to_vec(),
            pts,
            duration,
            is_keyframe,
        }))
    }

    /// Check if capture is running
    pub async fn is_running(&self) -> bool {
        *self.running.lock().await
    }

    /// Update configuration (requires restart)
    pub fn set_config(&mut self, config: CaptureConfig) {
        self.config = config;
    }

    /// Get current configuration
    pub fn config(&self) -> &CaptureConfig {
        &self.config
    }

    /// Get current bitrate in kbps
    pub fn current_bitrate_kbps(&self) -> u32 {
        self.current_bitrate_kbps
    }

    /// Dynamically adjust encoder bitrate while streaming
    ///
    /// This allows adaptive bitrate control based on network conditions.
    /// The change takes effect immediately without stopping the pipeline.
    pub fn set_bitrate(&mut self, bitrate_kbps: u32) -> Result<()> {
        let encoder = self
            .encoder
            .as_ref()
            .ok_or_else(|| crate::ProtocolError::Plugin("Encoder not initialized".to_string()))?;

        // Clamp to valid range and apply
        let bitrate_kbps = bitrate_kbps.clamp(100, 50000);
        encoder.set_property("bitrate", bitrate_kbps);
        self.current_bitrate_kbps = bitrate_kbps;
        debug!("Encoder bitrate adjusted to {} kbps", bitrate_kbps);
        Ok(())
    }
}

#[cfg(feature = "screenshare")]
impl Drop for ScreenCapture {
    fn drop(&mut self) {
        if let Some(pipeline) = &self.pipeline {
            let _ = pipeline.set_state(gst::State::Null);
        }
    }
}

// Stub implementation when screenshare feature is disabled
#[cfg(not(feature = "screenshare"))]
pub struct ScreenCapture {
    config: CaptureConfig,
    current_bitrate_kbps: u32,
}

#[cfg(not(feature = "screenshare"))]
impl ScreenCapture {
    pub fn new(config: CaptureConfig) -> Self {
        let initial_bitrate = config.bitrate_kbps;
        Self {
            config,
            current_bitrate_kbps: initial_bitrate,
        }
    }

    pub fn init(&mut self) -> crate::Result<()> {
        Err(crate::ProtocolError::Plugin(
            "screenshare feature not enabled".to_string(),
        ))
    }

    pub fn start(&mut self) -> crate::Result<()> {
        Err(crate::ProtocolError::Plugin(
            "screenshare feature not enabled".to_string(),
        ))
    }

    pub fn stop(&mut self) -> crate::Result<()> {
        Ok(())
    }

    pub fn pause(&mut self) -> crate::Result<()> {
        Err(crate::ProtocolError::Plugin(
            "screenshare feature not enabled".to_string(),
        ))
    }

    pub fn resume(&mut self) -> crate::Result<()> {
        Err(crate::ProtocolError::Plugin(
            "screenshare feature not enabled".to_string(),
        ))
    }

    pub async fn is_paused(&self) -> bool {
        false
    }

    pub fn pull_frame(&self) -> crate::Result<Option<EncodedFrame>> {
        Ok(None)
    }

    pub async fn is_running(&self) -> bool {
        false
    }

    pub fn set_config(&mut self, config: CaptureConfig) {
        self.config = config;
    }

    pub fn config(&self) -> &CaptureConfig {
        &self.config
    }

    pub fn current_bitrate_kbps(&self) -> u32 {
        self.current_bitrate_kbps
    }

    pub fn set_bitrate(&mut self, _bitrate_kbps: u32) -> crate::Result<()> {
        Err(crate::ProtocolError::Plugin(
            "screenshare feature not enabled".to_string(),
        ))
    }
}

#[cfg(all(test, feature = "screenshare"))]
mod tests {
    use super::*;

    #[test]
    fn test_capture_config_default() {
        let config = CaptureConfig::default();
        assert_eq!(config.fps, 30);
        assert_eq!(config.bitrate_kbps, 2000);
    }

    #[test]
    fn test_capture_init_test_source() {
        // Test with videotestsrc (no PipeWire needed)
        let config = CaptureConfig::default();
        let mut capture = ScreenCapture::new(config);

        // This should work with test source
        assert!(capture.init().is_ok());
    }
}
