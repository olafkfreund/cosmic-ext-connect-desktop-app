//! COSMIC Display Stream - Extended Display Streaming to Android Tablets
//!
//! This crate implements screen capture and streaming functionality for COSMIC Desktop,
//! specifically designed to stream a virtual HDMI display output to Android tablets.
//!
//! ## Architecture
//!
//! The implementation is divided into three phases:
//!
//! ### Phase 1: Screen Capture
//! - Use xdg-desktop-portal for screen capture permissions
//! - Connect to PipeWire streams for video data
//! - Filter for HDMI dummy display outputs only
//! - Receive raw video frames
//!
//! ### Phase 2: Video Encoding
//! - Encode frames to H.264 using hardware acceleration
//! - Support VAAPI (Intel/AMD), NVENC (NVIDIA), and software (x264) encoding
//! - Configurable quality, bitrate, and low-latency settings
//! - Automatic hardware encoder detection
//!
//! ### Phase 3: Network Streaming (Current)
//! - Stream encoded video over WebRTC
//! - WebSocket-based signaling server for peer connection setup
//! - ICE/STUN for NAT traversal
//! - Support for WiFi and USB (ADB) transport modes
//!
//! ## Usage Example
//!
//! ```no_run
//! use cosmic_display_stream::capture::ScreenCapture;
//! use futures::StreamExt;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create a screen capture session for HDMI-2 (dummy plug)
//!     let mut capture = ScreenCapture::new("HDMI-2").await?;
//!
//!     // Get output information
//!     if let Some(output) = capture.get_output_info() {
//!         println!("Capturing: {}", output.description());
//!     }
//!
//!     // Start capturing frames
//!     let mut frame_stream = capture.start_capture().await?;
//!
//!     // Process frames
//!     while let Some(frame) = frame_stream.next().await {
//!         println!("Received frame: {}x{} @ {}",
//!             frame.width, frame.height, frame.timestamp);
//!         // TODO: Encode and stream frame
//!     }
//!
//!     // Stop capture
//!     capture.stop_capture().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Requirements
//!
//! - COSMIC Desktop (Wayland)
//! - xdg-desktop-portal-cosmic
//! - PipeWire runtime
//! - HDMI dummy plug hardware (or virtual display)
//!
//! ## Configuration
//!
//! The target display output can be configured at runtime. By default, the
//! implementation filters for HDMI outputs marked as virtual displays by
//! the compositor.

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod capture;
pub mod encoder;
pub mod error;
pub mod output;
pub mod pipewire;
pub mod streaming;

pub use capture::{ScreenCapture, SessionState, VideoFrame, FrameStream};
pub use encoder::{VideoEncoder, EncoderConfig, EncoderType, EncodedFrame};
pub use error::{DisplayStreamError, Result};
pub use output::OutputInfo;
pub use streaming::{StreamingServer, StreamConfig, TransportMode, ConnectionStats};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Check if the runtime environment supports display streaming
///
/// This performs basic checks to verify that required components are available:
/// - PipeWire runtime
/// - xdg-desktop-portal
///
/// # Returns
///
/// `true` if the environment is suitable for display streaming
pub async fn check_requirements() -> bool {
    // Check for PipeWire
    let pipewire_available = std::process::Command::new("pw-cli")
        .arg("info")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);

    if !pipewire_available {
        tracing::warn!("PipeWire not available (pw-cli failed)");
        return false;
    }

    // Check for xdg-desktop-portal
    let portal_available = std::process::Command::new("gdbus")
        .args(["introspect", "--session", "--dest", "org.freedesktop.portal.Desktop", "--object-path", "/org/freedesktop/portal/desktop"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);

    if !portal_available {
        tracing::warn!("xdg-desktop-portal not available");
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
