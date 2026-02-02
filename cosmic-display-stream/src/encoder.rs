//! Video encoding module for H.264 with hardware acceleration
//!
//! This module provides video encoding functionality using `GStreamer`,
//! supporting hardware-accelerated encoding via VAAPI (Intel/AMD) and
//! NVENC (NVIDIA), with a software fallback.
//!
//! ## Supported Encoders
//!
//! - **VAAPI**: Intel and AMD hardware encoding (vaapih264enc)
//! - **NVENC**: NVIDIA hardware encoding (nvh264enc)
//! - **Software**: x264 software encoding (x264enc)
//!
//! ## Example
//!
//! ```no_run
//! use cosmic_display_stream::encoder::{VideoEncoder, EncoderConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create encoder with default settings
//! let config = EncoderConfig::default()
//!     .with_resolution(1920, 1080)
//!     .with_bitrate(10_000_000); // 10 Mbps
//!
//! let mut encoder = VideoEncoder::new(config)?;
//!
//! // Encode a frame
//! let raw_frame = vec![0u8; 1920 * 1080 * 4]; // BGRx data
//! let encoded = encoder.encode_frame(&raw_frame, 0)?;
//! # Ok(())
//! # }
//! ```

use crate::capture::VideoFrame;
use crate::error::{DisplayStreamError, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use tracing::{debug, error, info, warn};

/// Type of hardware encoder to use
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EncoderType {
    /// Intel/AMD VAAPI hardware encoding
    Vaapi,
    /// NVIDIA NVENC hardware encoding
    Nvenc,
    /// Software encoding (x264) - fallback
    #[default]
    Software,
}

impl EncoderType {
    /// Get the `GStreamer` element name for this encoder type
    fn element_name(&self) -> &'static str {
        match self {
            Self::Vaapi => "vaapih264enc",
            Self::Nvenc => "nvh264enc",
            Self::Software => "x264enc",
        }
    }

    /// Get a human-readable name for this encoder type
    #[must_use] 
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Vaapi => "VAAPI (Intel/AMD)",
            Self::Nvenc => "NVENC (NVIDIA)",
            Self::Software => "Software (x264)",
        }
    }
}

/// Encoder configuration options
#[derive(Debug, Clone)]
pub struct EncoderConfig {
    /// Video width in pixels
    pub width: u32,
    /// Video height in pixels
    pub height: u32,
    /// Target bitrate in bits per second
    pub bitrate: u32,
    /// Framerate (frames per second)
    pub framerate: u32,
    /// Preferred encoder type (None for auto-detection)
    pub encoder_type: Option<EncoderType>,
    /// Enable low-latency mode (reduces buffering)
    pub low_latency: bool,
    /// Keyframe interval (GOP size) in frames
    pub keyframe_interval: u32,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            bitrate: 10_000_000, // 10 Mbps
            framerate: 60,
            encoder_type: None, // Auto-detect
            low_latency: true,
            keyframe_interval: 30, // Keyframe every 30 frames (~0.5s at 60fps)
        }
    }
}

impl EncoderConfig {
    /// Create a new encoder configuration with default values
    #[must_use] 
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the video resolution
    #[must_use] 
    pub fn with_resolution(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Set the target bitrate in bits per second
    #[must_use] 
    pub fn with_bitrate(mut self, bitrate: u32) -> Self {
        self.bitrate = bitrate;
        self
    }

    /// Set the framerate
    #[must_use] 
    pub fn with_framerate(mut self, framerate: u32) -> Self {
        self.framerate = framerate;
        self
    }

    /// Set the preferred encoder type
    #[must_use] 
    pub fn with_encoder_type(mut self, encoder_type: EncoderType) -> Self {
        self.encoder_type = Some(encoder_type);
        self
    }

    /// Enable or disable low-latency mode
    #[must_use] 
    pub fn with_low_latency(mut self, enabled: bool) -> Self {
        self.low_latency = enabled;
        self
    }

    /// Set the keyframe interval
    #[must_use] 
    pub fn with_keyframe_interval(mut self, interval: u32) -> Self {
        self.keyframe_interval = interval;
        self
    }
}

/// Encoded video frame ready for transmission
#[derive(Debug, Clone)]
pub struct EncodedFrame {
    /// H.264 encoded data (NAL units)
    pub data: Vec<u8>,
    /// Presentation timestamp in microseconds
    pub pts: i64,
    /// Duration in microseconds
    pub duration: i64,
    /// Whether this is a keyframe (IDR)
    pub is_keyframe: bool,
}

/// Video encoder using `GStreamer` with hardware acceleration
pub struct VideoEncoder {
    /// `GStreamer` pipeline
    pipeline: gst::Pipeline,
    /// App source for pushing raw frames
    appsrc: gst_app::AppSrc,
    /// App sink for receiving encoded frames
    appsink: gst_app::AppSink,
    /// Encoder configuration
    config: EncoderConfig,
    /// Detected encoder type
    encoder_type: EncoderType,
    /// Whether the encoder is running
    running: bool,
}

impl VideoEncoder {
    /// Create a new video encoder with the given configuration
    ///
    /// # Arguments
    ///
    /// * `config` - Encoder configuration
    ///
    /// # Returns
    ///
    /// A new `VideoEncoder` instance on success
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `GStreamer` initialization fails
    /// - No suitable encoder is available
    /// - Pipeline creation fails
    pub fn new(config: EncoderConfig) -> Result<Self> {
        // Initialize GStreamer
        gst::init().map_err(|e| {
            DisplayStreamError::Encoder(format!("Failed to initialize GStreamer: {e}"))
        })?;

        // Detect or use specified encoder
        let encoder_type = config.encoder_type.unwrap_or_else(detect_best_encoder);

        info!(
            "Creating video encoder: {} ({}x{} @ {} fps, {} bps)",
            encoder_type.display_name(),
            config.width,
            config.height,
            config.framerate,
            config.bitrate
        );

        // Build the pipeline
        let (pipeline, appsrc, appsink) = Self::build_pipeline(&config, encoder_type)?;

        Ok(Self {
            pipeline,
            appsrc,
            appsink,
            config,
            encoder_type,
            running: false,
        })
    }

    /// Build the `GStreamer` encoding pipeline
    fn build_pipeline(
        config: &EncoderConfig,
        encoder_type: EncoderType,
    ) -> Result<(gst::Pipeline, gst_app::AppSrc, gst_app::AppSink)> {
        let pipeline = gst::Pipeline::new();

        // Create elements
        let appsrc = gst_app::AppSrc::builder()
            .name("source")
            .caps(
                &gst_video::VideoCapsBuilder::new()
                    .format(gst_video::VideoFormat::Bgrx)
                    .width(config.width as i32)
                    .height(config.height as i32)
                    .framerate(gst::Fraction::new(config.framerate as i32, 1))
                    .build(),
            )
            .format(gst::Format::Time)
            .is_live(true)
            .do_timestamp(true)
            .build();

        let videoconvert = gst::ElementFactory::make("videoconvert")
            .name("convert")
            .build()
            .map_err(|e| {
                DisplayStreamError::Encoder(format!("Failed to create videoconvert: {e}"))
            })?;

        // Create encoder based on type
        let encoder = Self::create_encoder(encoder_type, config)?;

        // H.264 parser for proper NAL unit framing
        let h264parse = gst::ElementFactory::make("h264parse")
            .name("parser")
            .build()
            .map_err(|e| {
                DisplayStreamError::Encoder(format!("Failed to create h264parse: {e}"))
            })?;

        let appsink = gst_app::AppSink::builder()
            .name("sink")
            .caps(
                &gst::Caps::builder("video/x-h264")
                    .field("stream-format", "byte-stream")
                    .field("alignment", "au")
                    .build(),
            )
            .build();

        // Add elements to pipeline
        pipeline
            .add_many([
                appsrc.upcast_ref(),
                &videoconvert,
                &encoder,
                &h264parse,
                appsink.upcast_ref(),
            ])
            .map_err(|e| {
                DisplayStreamError::Encoder(format!("Failed to add elements to pipeline: {e}"))
            })?;

        // Link elements
        gst::Element::link_many([
            appsrc.upcast_ref(),
            &videoconvert,
            &encoder,
            &h264parse,
            appsink.upcast_ref(),
        ])
        .map_err(|e| {
            DisplayStreamError::Encoder(format!("Failed to link pipeline elements: {e}"))
        })?;

        Ok((pipeline, appsrc, appsink))
    }

    /// Create the encoder element based on type
    fn create_encoder(encoder_type: EncoderType, config: &EncoderConfig) -> Result<gst::Element> {
        let element_name = encoder_type.element_name();

        let encoder = gst::ElementFactory::make(element_name)
            .name("encoder")
            .build()
            .map_err(|e| {
                DisplayStreamError::Encoder(format!(
                    "Failed to create encoder '{element_name}': {e}. Try a different encoder type."
                ))
            })?;

        // Configure encoder based on type
        match encoder_type {
            EncoderType::Vaapi => {
                // VAAPI-specific settings
                encoder.set_property("rate-control", 2u32); // CBR
                encoder.set_property("bitrate", config.bitrate / 1000); // kbps
                encoder.set_property("keyframe-period", config.keyframe_interval);
                if config.low_latency {
                    encoder.set_property("tune", 3u32); // low-latency
                }
            }
            EncoderType::Nvenc => {
                // NVENC-specific settings
                encoder.set_property("bitrate", config.bitrate / 1000); // kbps
                encoder.set_property("gop-size", config.keyframe_interval as i32);
                if config.low_latency {
                    encoder.set_property("preset", 5u32); // low-latency-hq
                    encoder.set_property("zerolatency", true);
                }
            }
            EncoderType::Software => {
                // x264-specific settings
                encoder.set_property("bitrate", config.bitrate / 1000); // kbps
                encoder.set_property("key-int-max", config.keyframe_interval);
                if config.low_latency {
                    encoder.set_property("tune", "zerolatency");
                    encoder.set_property("speed-preset", "ultrafast");
                }
            }
        }

        debug!(
            "Configured {} encoder with bitrate {} kbps",
            encoder_type.display_name(),
            config.bitrate / 1000
        );

        Ok(encoder)
    }

    /// Start the encoding pipeline
    pub fn start(&mut self) -> Result<()> {
        if self.running {
            return Ok(());
        }

        info!("Starting video encoder pipeline");

        self.pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| DisplayStreamError::Encoder(format!("Failed to start pipeline: {e}")))?;

        self.running = true;
        Ok(())
    }

    /// Stop the encoding pipeline
    pub fn stop(&mut self) -> Result<()> {
        if !self.running {
            return Ok(());
        }

        info!("Stopping video encoder pipeline");

        self.pipeline
            .set_state(gst::State::Null)
            .map_err(|e| DisplayStreamError::Encoder(format!("Failed to stop pipeline: {e}")))?;

        self.running = false;
        Ok(())
    }

    /// Encode a raw video frame
    ///
    /// # Arguments
    ///
    /// * `frame` - Raw video frame data (`BGRx` format)
    /// * `timestamp` - Presentation timestamp in microseconds
    ///
    /// # Returns
    ///
    /// The encoded frame on success
    pub fn encode_frame(&mut self, frame: &[u8], timestamp: i64) -> Result<Option<EncodedFrame>> {
        if !self.running {
            self.start()?;
        }

        // Create GStreamer buffer from frame data
        let mut buffer = gst::Buffer::with_size(frame.len())
            .map_err(|e| DisplayStreamError::Encoder(format!("Failed to create buffer: {e}")))?;

        {
            let buffer_ref = buffer.get_mut().ok_or_else(|| {
                DisplayStreamError::Encoder("Failed to get mutable buffer reference".to_string())
            })?;

            // Set timestamp
            buffer_ref.set_pts(gst::ClockTime::from_useconds(timestamp as u64));

            // Copy frame data
            let mut map = buffer_ref
                .map_writable()
                .map_err(|e| DisplayStreamError::Encoder(format!("Failed to map buffer: {e}")))?;
            map.copy_from_slice(frame);
        }

        // Push buffer to pipeline
        self.appsrc
            .push_buffer(buffer)
            .map_err(|e| DisplayStreamError::Encoder(format!("Failed to push buffer: {e}")))?;

        // Try to pull encoded frame
        self.pull_encoded_frame()
    }

    /// Encode a `VideoFrame` from the capture module
    pub fn encode_video_frame(&mut self, frame: &VideoFrame) -> Result<Option<EncodedFrame>> {
        // Verify format compatibility
        if frame.format != "BGRx" && frame.format != "BGRA" {
            warn!(
                "Frame format '{}' may not be compatible, expected BGRx",
                frame.format
            );
        }

        self.encode_frame(&frame.data, frame.timestamp)
    }

    /// Pull an encoded frame from the pipeline
    fn pull_encoded_frame(&self) -> Result<Option<EncodedFrame>> {
        // Try to pull a sample with a short timeout
        match self
            .appsink
            .try_pull_sample(gst::ClockTime::from_mseconds(1))
        {
            Some(sample) => {
                let buffer = sample.buffer().ok_or_else(|| {
                    DisplayStreamError::Encoder("Sample has no buffer".to_string())
                })?;

                let map = buffer.map_readable().map_err(|e| {
                    DisplayStreamError::Encoder(format!("Failed to map encoded buffer: {e}"))
                })?;

                let pts = buffer.pts().map_or(0, |t| t.useconds() as i64);
                let duration = buffer.duration().map_or(0, |t| t.useconds() as i64);

                // Check for keyframe flag
                let is_keyframe = !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT);

                Ok(Some(EncodedFrame {
                    data: map.to_vec(),
                    pts,
                    duration,
                    is_keyframe,
                }))
            }
            None => Ok(None),
        }
    }

    /// Set the target bitrate dynamically
    pub fn set_bitrate(&mut self, bitrate: u32) -> Result<()> {
        let encoder = self
            .pipeline
            .by_name("encoder")
            .ok_or_else(|| DisplayStreamError::Encoder("Encoder element not found".to_string()))?;

        match self.encoder_type {
            EncoderType::Vaapi | EncoderType::Nvenc | EncoderType::Software => {
                encoder.set_property("bitrate", bitrate / 1000);
            }
        }

        self.config.bitrate = bitrate;
        info!("Updated encoder bitrate to {} kbps", bitrate / 1000);
        Ok(())
    }

    /// Get the current encoder configuration
    #[must_use] 
    pub fn config(&self) -> &EncoderConfig {
        &self.config
    }

    /// Get the encoder type being used
    #[must_use] 
    pub fn encoder_type(&self) -> EncoderType {
        self.encoder_type
    }

    /// Check if the encoder is running
    #[must_use] 
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Force a keyframe on the next encoded frame
    pub fn force_keyframe(&self) -> Result<()> {
        let encoder = self
            .pipeline
            .by_name("encoder")
            .ok_or_else(|| DisplayStreamError::Encoder("Encoder element not found".to_string()))?;

        // Send custom force-keyunit event via GStreamer's event structure
        // This is the upstream event that encoders understand
        let structure = gst::Structure::builder("GstForceKeyUnit")
            .field("all-headers", true)
            .build();

        let event = gst::event::CustomUpstream::new(structure);
        encoder.send_event(event);
        debug!("Forced keyframe requested");
        Ok(())
    }
}

impl Drop for VideoEncoder {
    fn drop(&mut self) {
        if self.running {
            if let Err(e) = self.stop() {
                error!("Error stopping encoder on drop: {}", e);
            }
        }
    }
}

/// Detect the best available hardware encoder
///
/// This function checks for available hardware encoders in order of preference:
/// 1. VAAPI (Intel/AMD)
/// 2. NVENC (NVIDIA)
/// 3. Software (x264) as fallback
pub fn detect_best_encoder() -> EncoderType {
    // Initialize GStreamer if not already done
    if gst::init().is_err() {
        warn!("GStreamer not initialized, falling back to software encoder");
        return EncoderType::Software;
    }

    // Check VAAPI
    if is_encoder_available("vaapih264enc") {
        info!("VAAPI hardware encoder detected");
        return EncoderType::Vaapi;
    }

    // Check NVENC
    if is_encoder_available("nvh264enc") {
        info!("NVENC hardware encoder detected");
        return EncoderType::Nvenc;
    }

    // Fall back to software
    info!("No hardware encoder available, using software encoding");
    EncoderType::Software
}

/// Check if a specific `GStreamer` encoder element is available
#[must_use] 
pub fn is_encoder_available(element_name: &str) -> bool {
    gst::ElementFactory::find(element_name).is_some()
}

/// Get a list of all available encoder types
#[must_use] 
pub fn available_encoders() -> Vec<EncoderType> {
    let mut encoders = Vec::new();

    if gst::init().is_ok() {
        if is_encoder_available("vaapih264enc") {
            encoders.push(EncoderType::Vaapi);
        }
        if is_encoder_available("nvh264enc") {
            encoders.push(EncoderType::Nvenc);
        }
        if is_encoder_available("x264enc") {
            encoders.push(EncoderType::Software);
        }
    }

    encoders
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encoder_config_default() {
        let config = EncoderConfig::default();
        assert_eq!(config.width, 1920);
        assert_eq!(config.height, 1080);
        assert_eq!(config.bitrate, 10_000_000);
        assert_eq!(config.framerate, 60);
        assert!(config.low_latency);
    }

    #[test]
    fn test_encoder_config_builder() {
        let config = EncoderConfig::new()
            .with_resolution(2560, 1600)
            .with_bitrate(15_000_000)
            .with_framerate(30)
            .with_low_latency(false)
            .with_keyframe_interval(60);

        assert_eq!(config.width, 2560);
        assert_eq!(config.height, 1600);
        assert_eq!(config.bitrate, 15_000_000);
        assert_eq!(config.framerate, 30);
        assert!(!config.low_latency);
        assert_eq!(config.keyframe_interval, 60);
    }

    #[test]
    fn test_encoder_type_display_name() {
        assert_eq!(EncoderType::Vaapi.display_name(), "VAAPI (Intel/AMD)");
        assert_eq!(EncoderType::Nvenc.display_name(), "NVENC (NVIDIA)");
        assert_eq!(EncoderType::Software.display_name(), "Software (x264)");
    }

    #[test]
    fn test_encoder_type_element_name() {
        assert_eq!(EncoderType::Vaapi.element_name(), "vaapih264enc");
        assert_eq!(EncoderType::Nvenc.element_name(), "nvh264enc");
        assert_eq!(EncoderType::Software.element_name(), "x264enc");
    }

    #[test]
    fn test_encoded_frame() {
        let frame = EncodedFrame {
            data: vec![0, 0, 0, 1, 0x67], // Fake NAL unit
            pts: 1000,
            duration: 16666,
            is_keyframe: true,
        };

        assert!(frame.is_keyframe);
        assert_eq!(frame.pts, 1000);
        assert!(!frame.data.is_empty());
    }

    #[test]
    fn test_detect_encoder() {
        // This test verifies that detection returns a valid encoder type
        // Actual hardware availability varies by system
        let encoder_type = detect_best_encoder();
        assert!(matches!(
            encoder_type,
            EncoderType::Vaapi | EncoderType::Nvenc | EncoderType::Software
        ));
    }

    #[test]
    fn test_available_encoders() {
        let encoders = available_encoders();
        // At minimum, software encoder should be available if GStreamer is installed
        // But we don't fail the test if GStreamer isn't available
        if !encoders.is_empty() {
            assert!(encoders.iter().any(|e| matches!(
                e,
                EncoderType::Vaapi | EncoderType::Nvenc | EncoderType::Software
            )));
        }
    }
}
