//! Screen Capture for Screen Share
//!
//! Uses GStreamer with PipeWire source for Wayland screen capture.
//! Encodes to H.264 for network transmission.

#[cfg(feature = "screenshare")]
use gstreamer as gst;
#[cfg(feature = "screenshare")]
use gstreamer_app as gst_app;
#[cfg(feature = "screenshare")]
use gstreamer::prelude::*;
#[cfg(feature = "screenshare")]
use crate::Result;
#[cfg(feature = "screenshare")]
use tracing::{debug, error, info, warn};
#[cfg(feature = "screenshare")]
use std::sync::Arc;
#[cfg(feature = "screenshare")]
use tokio::sync::Mutex;

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
    config: CaptureConfig,
    running: Arc<Mutex<bool>>,
}

#[cfg(feature = "screenshare")]
impl ScreenCapture {
    /// Create a new screen capture instance
    pub fn new(config: CaptureConfig) -> Self {
        Self {
            pipeline: None,
            appsink: None,
            config,
            running: Arc::new(Mutex::new(false)),
        }
    }

    /// Initialize GStreamer and create the capture pipeline
    ///
    /// For Wayland, this requires a PipeWire fd and node_id from the XDG Desktop Portal.
    /// Call `request_screen_share()` first to get these via D-Bus.
    pub fn init(&mut self) -> Result<()> {
        gst::init().map_err(|e| {
            crate::ProtocolError::Plugin(format!("GStreamer init failed: {}", e))
        })?;

        let pipeline = self.create_pipeline()?;

        let appsink = pipeline
            .by_name("sink")
            .ok_or_else(|| crate::ProtocolError::Plugin("Failed to get appsink".to_string()))?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| crate::ProtocolError::Plugin("Failed to downcast appsink".to_string()))?;

        self.pipeline = Some(pipeline);
        self.appsink = Some(appsink);

        info!("Screen capture initialized");
        Ok(())
    }

    /// Create the GStreamer pipeline for screen capture
    fn create_pipeline(&self) -> Result<gst::Pipeline> {
        // Build pipeline string based on configuration
        let pipeline_str = if let (Some(fd), Some(node_id)) = (self.config.pipewire_fd, self.config.pipewire_node_id) {
            // PipeWire source with portal fd and node
            format!(
                "pipewiresrc fd={} path={} do-timestamp=true keepalive-time=1000 ! \
                 videoconvert ! videoscale ! \
                 video/x-raw,framerate={}/1{} ! \
                 x264enc tune=zerolatency bitrate={} speed-preset=ultrafast key-int-max=30 ! \
                 video/x-h264,stream-format=byte-stream ! \
                 appsink name=sink emit-signals=true drop=true max-buffers=2",
                fd,
                node_id,
                self.config.fps,
                self.resolution_caps(),
                self.config.bitrate_kbps
            )
        } else {
            // Fallback: use test source for development/testing
            warn!("No PipeWire fd/node_id provided, using test video source");
            format!(
                "videotestsrc pattern=smpte is-live=true ! \
                 video/x-raw,framerate={}/1,width=1280,height=720 ! \
                 videoconvert ! \
                 x264enc tune=zerolatency bitrate={} speed-preset=ultrafast key-int-max=30 ! \
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
        if let Some(pipeline) = &self.pipeline {
            pipeline.set_state(gst::State::Playing)
                .map_err(|e| crate::ProtocolError::Plugin(format!("Failed to start capture: {}", e)))?;

            info!("Screen capture started");

            // Update running flag
            let running = self.running.clone();
            tokio::spawn(async move {
                let mut guard = running.lock().await;
                *guard = true;
            });

            Ok(())
        } else {
            Err(crate::ProtocolError::Plugin("Pipeline not initialized".to_string()))
        }
    }

    /// Stop capturing
    pub fn stop(&mut self) -> Result<()> {
        if let Some(pipeline) = &self.pipeline {
            pipeline.set_state(gst::State::Null)
                .map_err(|e| crate::ProtocolError::Plugin(format!("Failed to stop capture: {}", e)))?;

            info!("Screen capture stopped");

            // Update running flag
            let running = self.running.clone();
            tokio::spawn(async move {
                let mut guard = running.lock().await;
                *guard = false;
            });

            Ok(())
        } else {
            Ok(()) // Not initialized, nothing to stop
        }
    }

    /// Pull the next encoded frame (non-blocking)
    ///
    /// Returns None if no frame is available yet
    pub fn pull_frame(&self) -> Result<Option<EncodedFrame>> {
        if let Some(appsink) = &self.appsink {
            // Try to pull with a small timeout
            match appsink.try_pull_sample(gst::ClockTime::from_mseconds(1)) {
                Some(sample) => {
                    let buffer = sample.buffer()
                        .ok_or_else(|| crate::ProtocolError::Plugin("No buffer in sample".to_string()))?;

                    let map = buffer.map_readable()
                        .map_err(|_| crate::ProtocolError::Plugin("Failed to map buffer".to_string()))?;

                    let pts = buffer.pts().map(|t| t.nseconds()).unwrap_or(0);
                    let duration = buffer.duration().map(|t| t.nseconds()).unwrap_or(0);

                    // Check for keyframe (delta unit flag NOT set means keyframe)
                    let flags = buffer.flags();
                    let is_keyframe = !flags.contains(gst::BufferFlags::DELTA_UNIT);

                    Ok(Some(EncodedFrame {
                        data: map.to_vec(),
                        pts,
                        duration,
                        is_keyframe,
                    }))
                }
                None => Ok(None),
            }
        } else {
            Err(crate::ProtocolError::Plugin("Appsink not initialized".to_string()))
        }
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
}

#[cfg(not(feature = "screenshare"))]
impl ScreenCapture {
    pub fn new(config: CaptureConfig) -> Self {
        Self { config }
    }

    pub fn init(&mut self) -> crate::Result<()> {
        Err(crate::ProtocolError::Plugin("screenshare feature not enabled".to_string()))
    }

    pub fn start(&mut self) -> crate::Result<()> {
        Err(crate::ProtocolError::Plugin("screenshare feature not enabled".to_string()))
    }

    pub fn stop(&mut self) -> crate::Result<()> {
        Ok(())
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
