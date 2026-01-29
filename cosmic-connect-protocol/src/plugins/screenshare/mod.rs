//! Screen Share Plugin
//!
//! One-way screen sharing for presentations and demonstrations.
//!
//! ## Protocol Specification
//!
//! This plugin implements one-way screen sharing optimized for presentations,
//! demos, and collaborative work. Unlike RemoteDesktop, this is view-only with
//! no remote control capabilities.
//!
//! ### Packet Types
//!
//! - `cconnect.screenshare.start` - Start screen sharing with configuration
//! - `cconnect.screenshare.frame` - Screen frame data (via payload)
//! - `cconnect.screenshare.cursor` - Cursor position update
//! - `cconnect.screenshare.annotation` - Drawing annotations
//! - `cconnect.screenshare.stop` - Stop screen sharing
//!
//! ### Capabilities
//!
//! - Incoming: `cconnect.screenshare` - Can receive screen shares
//! - Outgoing: `cconnect.screenshare` - Can share screen
//!
//! ### Use Cases
//!
//! - Presentations and demonstrations
//! - Collaborative work and code reviews
//! - Teaching and training sessions
//! - Status updates and walkthroughs
//! - Multi-viewer streaming
//!
//! ## Features
//!
//! - **Window/Screen Selection**: Share single window or full screen
//! - **Cursor Highlighting**: Show presenter's cursor position
//! - **Annotations**: Optional drawing tools for emphasis
//! - **Low Latency**: Optimized for streaming performance
//! - **View-Only**: No remote input control
//! - **Multiple Viewers**: Share to many devices simultaneously
//! - **Adaptive Bitrate**: Adjust quality based on network conditions
//!
//! ## Differences from RemoteDesktop
//!
//! - **One-way only**: Viewers cannot control the shared screen
//! - **Lower latency**: Optimized for streaming without bidirectional control
//! - **Simpler security**: No input control attack surface
//! - **Multiple viewers**: Broadcast to many devices at once
//! - **Presentation focus**: Tools for highlighting and annotating
//!
//! ## Implementation Status
//!
//! - [x] Screen capture implementation (PipeWire for Wayland)
//! - [x] Video encoding (H.264 via x264enc)
//! - [x] Video decoding (H.264 via avdec_h264)
//! - [x] Stream receiver (TCP with custom protocol)
//! - [x] Stream sender (TCP with custom protocol)
//! - [ ] XDG Desktop Portal integration for screen selection
//! - [ ] Cursor tracking and highlighting
//! - [ ] Annotation overlay system
//! - [ ] Adaptive bitrate control
//! - [ ] Multiple viewer management

pub mod capture;
pub mod decoder;
pub mod stream_receiver;
pub mod stream_sender;

use crate::plugins::{Plugin, PluginFactory};
use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashSet;
use tracing::{debug, error, info, warn};

const PLUGIN_NAME: &str = "screenshare";
const INCOMING_CAPABILITY: &str = "cconnect.screenshare";
const OUTGOING_CAPABILITY: &str = "cconnect.screenshare";

// Screen share configuration constants
const DEFAULT_FPS: u8 = 30;
const DEFAULT_BITRATE_KBPS: u32 = 2000; // 2 Mbps
const DEFAULT_QUALITY: &str = "medium";
const MAX_VIEWERS: usize = 10; // Max simultaneous viewers

/// Screen share mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShareMode {
    /// Share entire screen
    FullScreen,
    /// Share specific window
    Window,
}

impl Default for ShareMode {
    fn default() -> Self {
        Self::FullScreen
    }
}

impl ShareMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FullScreen => "fullscreen",
            Self::Window => "window",
        }
    }
}

/// Video codec for encoding
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    /// H.264 codec (widely supported)
    H264,
    /// VP8 codec (WebRTC standard)
    Vp8,
    /// VP9 codec (better compression)
    Vp9,
}

impl Default for VideoCodec {
    fn default() -> Self {
        Self::H264
    }
}

impl VideoCodec {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::Vp8 => "vp8",
            Self::Vp9 => "vp9",
        }
    }
}

/// Screen share configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareConfig {
    /// Share mode (full screen or window)
    #[serde(default)]
    pub mode: ShareMode,

    /// Window title (if mode is Window)
    #[serde(default)]
    pub window_title: Option<String>,

    /// Video codec to use
    #[serde(default)]
    pub codec: VideoCodec,

    /// Target frame rate (FPS)
    #[serde(default = "default_fps")]
    pub fps: u8,

    /// Target bitrate in Kbps
    #[serde(default = "default_bitrate")]
    pub bitrate_kbps: u32,

    /// Quality preset (low, medium, high)
    #[serde(default = "default_quality")]
    pub quality: String,

    /// Enable cursor highlighting
    #[serde(default = "default_true")]
    pub show_cursor: bool,

    /// Enable annotations
    #[serde(default)]
    pub enable_annotations: bool,

    /// Adaptive bitrate based on network
    #[serde(default = "default_true")]
    pub adaptive_bitrate: bool,
}

#[allow(dead_code)]
fn default_fps() -> u8 {
    DEFAULT_FPS
}

#[allow(dead_code)]
fn default_bitrate() -> u32 {
    DEFAULT_BITRATE_KBPS
}

#[allow(dead_code)]
fn default_quality() -> String {
    DEFAULT_QUALITY.to_string()
}

#[allow(dead_code)]
fn default_true() -> bool {
    true
}

impl Default for ShareConfig {
    fn default() -> Self {
        Self {
            mode: ShareMode::default(),
            window_title: None,
            codec: VideoCodec::default(),
            fps: default_fps(),
            bitrate_kbps: default_bitrate(),
            quality: default_quality(),
            show_cursor: true,
            enable_annotations: false,
            adaptive_bitrate: true,
        }
    }
}

impl ShareConfig {
    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        // Validate FPS range
        if self.fps < 1 || self.fps > 60 {
            return Err(ProtocolError::InvalidPacket(format!(
                "Invalid FPS: {}. Must be 1-60",
                self.fps
            )));
        }

        // Validate bitrate
        if self.bitrate_kbps < 100 || self.bitrate_kbps > 50000 {
            return Err(ProtocolError::InvalidPacket(format!(
                "Invalid bitrate: {} Kbps. Must be 100-50000",
                self.bitrate_kbps
            )));
        }

        // Validate quality preset
        match self.quality.as_str() {
            "low" | "medium" | "high" => {}
            _ => {
                return Err(ProtocolError::InvalidPacket(format!(
                    "Invalid quality: {}. Must be low, medium, or high",
                    self.quality
                )))
            }
        }

        // Validate window mode requires window title
        if matches!(self.mode, ShareMode::Window) && self.window_title.is_none() {
            return Err(ProtocolError::InvalidPacket(
                "Window mode requires window_title to be set".to_string(),
            ));
        }

        Ok(())
    }
}

/// Cursor position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPosition {
    /// X coordinate
    pub x: i32,

    /// Y coordinate
    pub y: i32,

    /// Cursor is visible
    #[serde(default = "default_true")]
    pub visible: bool,
}

/// Annotation data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    /// Annotation type (line, circle, rectangle, freehand)
    pub annotation_type: String,

    /// Start X coordinate
    pub x1: i32,

    /// Start Y coordinate
    pub y1: i32,

    /// End X coordinate (for lines/shapes)
    #[serde(default)]
    pub x2: Option<i32>,

    /// End Y coordinate (for lines/shapes)
    #[serde(default)]
    pub y2: Option<i32>,

    /// Color (RGB hex)
    #[serde(default = "default_color")]
    pub color: String,

    /// Line width
    #[serde(default = "default_line_width")]
    pub width: u8,
}

#[allow(dead_code)]
fn default_color() -> String {
    "#FF0000".to_string() // Red
}

#[allow(dead_code)]
fn default_line_width() -> u8 {
    3
}

/// Active screen share session
#[derive(Debug)]
struct ShareSession {
    /// Session configuration
    #[allow(dead_code)]
    config: ShareConfig,

    /// Connected viewers
    viewers: HashSet<String>,

    /// Session start time
    started_at: std::time::Instant,

    /// Total frames sent
    frames_sent: u64,

    /// Total bytes sent
    bytes_sent: u64,

    /// Current cursor position
    cursor_position: Option<CursorPosition>,

    /// Active annotations
    annotations: Vec<Annotation>,
}

impl ShareSession {
    fn new(config: ShareConfig) -> Self {
        Self {
            config,
            viewers: HashSet::new(),
            started_at: std::time::Instant::now(),
            frames_sent: 0,
            bytes_sent: 0,
            cursor_position: None,
            annotations: Vec::new(),
        }
    }

    fn add_viewer(&mut self, viewer_id: String) -> Result<()> {
        if self.viewers.len() >= MAX_VIEWERS {
            return Err(ProtocolError::Plugin(format!(
                "Maximum viewers ({}) reached",
                MAX_VIEWERS
            )));
        }

        self.viewers.insert(viewer_id);
        Ok(())
    }

    fn remove_viewer(&mut self, viewer_id: &str) {
        self.viewers.remove(viewer_id);
    }

    #[allow(dead_code)]
    fn update_stats(&mut self, frame_bytes: u64) {
        self.frames_sent += 1;
        self.bytes_sent += frame_bytes;
    }

    fn get_stats(&self) -> ShareStats {
        let duration = self.started_at.elapsed();
        let avg_fps = if duration.as_secs() > 0 {
            self.frames_sent / duration.as_secs()
        } else {
            0
        };

        let avg_bitrate_kbps = if duration.as_secs() > 0 {
            (self.bytes_sent * 8) / (duration.as_secs() * 1000)
        } else {
            0
        };

        ShareStats {
            duration_secs: duration.as_secs(),
            frames_sent: self.frames_sent,
            bytes_sent: self.bytes_sent,
            viewer_count: self.viewers.len(),
            avg_fps,
            avg_bitrate_kbps,
        }
    }
}

/// Screen share statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareStats {
    /// Session duration in seconds
    pub duration_secs: u64,

    /// Total frames sent
    pub frames_sent: u64,

    /// Total bytes sent
    pub bytes_sent: u64,

    /// Current number of viewers
    pub viewer_count: usize,

    /// Average FPS
    pub avg_fps: u64,

    /// Average bitrate in Kbps
    pub avg_bitrate_kbps: u64,
}

/// Screen Share plugin
pub struct ScreenSharePlugin {
    /// Device ID this plugin is associated with
    device_id: Option<String>,

    /// Plugin enabled state
    enabled: bool,

    /// Active sharing session (this device sharing)
    active_session: Option<ShareSession>,

    /// Receiving session (viewing remote share)
    receiving: bool,

    /// Packet sender for outgoing messages
    packet_sender: Option<tokio::sync::mpsc::Sender<(String, Packet)>>,

    /// Local port to receive stream (set by UI)
    local_port: Option<u16>,
}

impl ScreenSharePlugin {
    /// Create new screen share plugin instance
    pub fn new() -> Self {
        Self {
            device_id: None,
            enabled: false,
            active_session: None,
            receiving: false,
            packet_sender: None,
            local_port: None,
        }
    }

    /// Start screen sharing session
    pub async fn start_sharing(&mut self, config: ShareConfig) -> Result<()> {
        config.validate()?;

        if self.active_session.is_some() {
            warn!("Screen share session already active, stopping existing session");
            self.stop_sharing().await?;
        }

        info!(
            "Starting screen share: {} mode, {} @ {}fps, {} Kbps",
            config.mode.as_str(),
            config.codec.as_str(),
            config.fps,
            config.bitrate_kbps
        );

        let session = ShareSession::new(config);
        self.active_session = Some(session);

        // TODO: Initialize screen capture
        // TODO: Start encoding thread
        // TODO: Start frame sender thread

        Ok(())
    }

    /// Stop screen sharing session
    pub async fn stop_sharing(&mut self) -> Result<()> {
        if let Some(session) = self.active_session.take() {
            let stats = session.get_stats();
            info!(
                "Stopped screen share: {} frames, {} MB, {} viewers, {} seconds",
                stats.frames_sent,
                stats.bytes_sent / 1_000_000,
                stats.viewer_count,
                stats.duration_secs
            );

            // TODO: Stop screen capture
            // TODO: Stop encoding thread
            // TODO: Stop frame sender thread
        }

        Ok(())
    }

    /// Add viewer to active session
    pub fn add_viewer(&mut self, viewer_id: String) -> Result<()> {
        if let Some(session) = &mut self.active_session {
            session.add_viewer(viewer_id.clone())?;
            info!(
                "Added viewer: {} (total: {})",
                viewer_id,
                session.viewers.len()
            );
            Ok(())
        } else {
            Err(ProtocolError::Plugin(
                "No active sharing session".to_string(),
            ))
        }
    }

    /// Remove viewer from active session
    pub fn remove_viewer(&mut self, viewer_id: &str) {
        if let Some(session) = &mut self.active_session {
            session.remove_viewer(viewer_id);
            info!(
                "Removed viewer: {} (remaining: {})",
                viewer_id,
                session.viewers.len()
            );
        }
    }

    /// Update cursor position
    pub fn update_cursor(&mut self, position: CursorPosition) {
        if let Some(session) = &mut self.active_session {
            session.cursor_position = Some(position);
        }
    }

    /// Add annotation
    pub fn add_annotation(&mut self, annotation: Annotation) {
        if let Some(session) = &mut self.active_session {
            session.annotations.push(annotation);

            // Limit annotation history
            if session.annotations.len() > 100 {
                session.annotations.remove(0);
            }
        }
    }

    /// Clear all annotations
    pub fn clear_annotations(&mut self) {
        if let Some(session) = &mut self.active_session {
            session.annotations.clear();
        }
    }

    /// Get share statistics
    pub fn get_stats(&self) -> Option<ShareStats> {
        self.active_session.as_ref().map(|s| s.get_stats())
    }

    /// Set the local port for receiving the stream
    pub async fn set_local_port(&mut self, port: u16) -> Result<()> {
        self.local_port = Some(port);
        
        // If we were already waiting for this (received start), send ready packet
        if self.receiving {
             if let Some(sender) = &self.packet_sender {
                let body = serde_json::json!({ "tcpPort": port });
                let packet = Packet::new("cconnect.screenshare.ready", body);
                // We need device_id
                if let Some(device_id) = &self.device_id {
                    sender.send((device_id.clone(), packet)).await
                        .map_err(|_| ProtocolError::Plugin("Failed to send ready packet".to_string()))?;
                }
            }
        }
        Ok(())
    }

    /// Check if currently sharing
    pub fn is_sharing(&self) -> bool {
        self.active_session.is_some()
    }

    /// Check if currently receiving
    pub fn is_receiving(&self) -> bool {
        self.receiving
    }

    /// Initiate screen sharing to the connected device
    ///
    /// This starts local capture and sends a start packet to the remote device.
    /// The remote device will respond with a ready packet containing their receiving port.
    pub async fn share_to_device(&mut self, config: ShareConfig) -> Result<()> {
        // Validate config
        config.validate()?;

        // Start local sharing session
        self.start_sharing(config.clone()).await?;

        // Send start packet to remote device
        if let Some(sender) = &self.packet_sender {
            if let Some(device_id) = &self.device_id {
                let body = serde_json::to_value(&config)
                    .map_err(|e| ProtocolError::Plugin(format!("Failed to serialize config: {}", e)))?;
                let packet = Packet::new("cconnect.screenshare.start", body);

                sender.send((device_id.clone(), packet)).await
                    .map_err(|_| ProtocolError::Plugin("Failed to send start packet".to_string()))?;

                info!("Sent screen share start to {}", device_id);
                Ok(())
            } else {
                Err(ProtocolError::Plugin("No device ID set".to_string()))
            }
        } else {
            Err(ProtocolError::Plugin("No packet sender available".to_string()))
        }
    }
}

impl Default for ScreenSharePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ScreenSharePlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            // Base capability
            INCOMING_CAPABILITY.to_string(),
            "kdeconnect.screenshare".to_string(),
            // Specific packet types that this plugin handles
            "cconnect.screenshare.start".to_string(),
            "cconnect.screenshare.frame".to_string(),
            "cconnect.screenshare.cursor".to_string(),
            "cconnect.screenshare.annotation".to_string(),
            "cconnect.screenshare.stop".to_string(),
            "cconnect.screenshare.ready".to_string(),
            // KDE Connect compatibility
            "kdeconnect.screenshare.start".to_string(),
            "kdeconnect.screenshare.frame".to_string(),
            "kdeconnect.screenshare.cursor".to_string(),
            "kdeconnect.screenshare.annotation".to_string(),
            "kdeconnect.screenshare.stop".to_string(),
            "kdeconnect.screenshare.ready".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![OUTGOING_CAPABILITY.to_string()]
    }

    async fn init(&mut self, device: &Device, packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>) -> Result<()> {
        info!(
            "Initializing ScreenShare plugin for device {}",
            device.name()
        );
        self.device_id = Some(device.id().to_string());
        self.packet_sender = Some(packet_sender);

        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Starting ScreenShare plugin");
        self.enabled = true;

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Stopping ScreenShare plugin");
        self.enabled = false;

        // Stop any active sessions
        self.stop_sharing().await?;
        self.receiving = false;

        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("ScreenShare plugin is disabled, ignoring packet");
            return Ok(());
        }

        debug!("Handling packet type: {}", packet.packet_type);

        if packet.is_type("cconnect.screenshare.start") {
            // Remote device started sharing
            let config: ShareConfig = serde_json::from_value(packet.body.clone())
                .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

            info!(
                "Receiving screen share from {}: {} @ {}fps",
                device.name(),
                config.codec.as_str(),
                config.fps
            );

            self.receiving = true;

            if let Some(port) = self.local_port {
                // UI is ready, send ready packet immediately
                info!("Sending ready packet with port {}", port);
                if let Some(sender) = &self.packet_sender {
                    let body = serde_json::json!({ "tcpPort": port });
                    let packet = Packet::new("cconnect.screenshare.ready", body);
                    if let Err(e) = sender.send((self.device_id.clone().unwrap_or_default(), packet)).await {
                        error!("Failed to send ready packet: {}", e);
                    }
                }
            } else {
                // UI not ready, request UI start (emit internal packet)
                info!("Screen share started by remote, requesting UI launch");
                if let Some(sender) = &self.packet_sender {
                    let packet = Packet::new("cconnect.internal.screenshare.requested", serde_json::json!({}));
                    if let Err(e) = sender.send((self.device_id.clone().unwrap_or_default(), packet)).await {
                        error!("Failed to send internal request: {}", e);
                    }
                }
            }
        } else if packet.is_type("cconnect.screenshare.frame") {
            // Receive screen frame
            if !self.receiving {
                warn!("Received frame but not in receiving mode");
                return Ok(());
            }

            // Frames are handled via separate TCP stream, this packet type is likely unused
            // in the custom protocol, but kept for compatibility or fallback.
            debug!("Received screen frame packet (unexpected for streaming mode)");
        } else if packet.is_type("cconnect.screenshare.cursor") {
            // Receive cursor position
            let position: CursorPosition = serde_json::from_value(packet.body.clone())
                .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

            // TODO: Update cursor overlay on display
            // Need to send this to UI via DBus if not part of stream
            debug!("Cursor updated: ({}, {})", position.x, position.y);
        } else if packet.is_type("cconnect.screenshare.annotation") {
            // Receive annotation
            let annotation: Annotation = serde_json::from_value(packet.body.clone())
                .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

            // TODO: Draw annotation on display overlay
            debug!("Annotation received: {}", annotation.annotation_type);
        } else if packet.is_type("cconnect.screenshare.stop") {
            // Remote device stopped sharing
            info!("Screen share stopped by {}", device.name());
            self.receiving = false;

            // We should probably inform the UI to close the window via DBus signal?
            // Or let the UI detect stream closure.
        } else if packet.is_type("cconnect.screenshare.ready") {
            // Receiver is ready to receive screen share
            // This is sent by the receiving device after it opens its viewer window
            let tcp_port = packet.body.get("tcpPort")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u16;

            info!(
                "Receiver {} is ready on port {}",
                device.name(),
                tcp_port
            );

            // Add this device as a viewer
            if let Err(e) = self.add_viewer(device.id().to_string()) {
                warn!("Failed to add viewer {}: {}", device.id(), e);
            }

            // TODO: Start GStreamer capture pipeline and connect to receiver's TCP port
            // For now, log that we received the ready signal
            // The actual implementation requires:
            // 1. Starting the GStreamer capture pipeline
            // 2. Connecting StreamSender to receiver's tcpPort
            // 3. Streaming encoded frames
            debug!(
                "Screen share ready signal received - capture pipeline should start streaming to {} port {}",
                device.name(),
                tcp_port
            );
        }

        Ok(())
    }
}

/// Screen Share plugin factory
pub struct ScreenSharePluginFactory;

impl PluginFactory for ScreenSharePluginFactory {
    fn create(&self) -> Box<dyn Plugin> {
        Box::new(ScreenSharePlugin::new())
    }

    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            // Base capability
            INCOMING_CAPABILITY.to_string(),
            "kdeconnect.screenshare".to_string(),
            // Specific packet types that this plugin handles
            "cconnect.screenshare.start".to_string(),
            "cconnect.screenshare.frame".to_string(),
            "cconnect.screenshare.cursor".to_string(),
            "cconnect.screenshare.annotation".to_string(),
            "cconnect.screenshare.stop".to_string(),
            "cconnect.screenshare.ready".to_string(),
            // KDE Connect compatibility
            "kdeconnect.screenshare.start".to_string(),
            "kdeconnect.screenshare.frame".to_string(),
            "kdeconnect.screenshare.cursor".to_string(),
            "kdeconnect.screenshare.annotation".to_string(),
            "kdeconnect.screenshare.stop".to_string(),
            "kdeconnect.screenshare.ready".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![OUTGOING_CAPABILITY.to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_device;

    #[tokio::test]
    async fn test_plugin_creation() {
        let plugin = ScreenSharePlugin::new();
        assert_eq!(plugin.name(), PLUGIN_NAME);
        assert!(!plugin.enabled);
        assert!(!plugin.is_sharing());
        assert!(!plugin.is_receiving());
    }

    #[tokio::test]
    async fn test_config_validation() {
        let config = ShareConfig::default();
        assert!(config.validate().is_ok());

        let mut invalid_fps = config.clone();
        invalid_fps.fps = 100;
        assert!(invalid_fps.validate().is_err());

        let mut invalid_bitrate = config.clone();
        invalid_bitrate.bitrate_kbps = 100000;
        assert!(invalid_bitrate.validate().is_err());

        let mut invalid_quality = config.clone();
        invalid_quality.quality = "invalid".to_string();
        assert!(invalid_quality.validate().is_err());

        let mut window_without_title = config;
        window_without_title.mode = ShareMode::Window;
        assert!(window_without_title.validate().is_err());
    }

    #[tokio::test]
    async fn test_start_stop_sharing() {
        let mut plugin = ScreenSharePlugin::new();
        plugin.enabled = true;

        let config = ShareConfig::default();
        assert!(plugin.start_sharing(config).await.is_ok());
        assert!(plugin.is_sharing());

        assert!(plugin.stop_sharing().await.is_ok());
        assert!(!plugin.is_sharing());
    }

    #[tokio::test]
    async fn test_viewer_management() {
        let mut plugin = ScreenSharePlugin::new();
        plugin.enabled = true;

        let config = ShareConfig::default();
        plugin.start_sharing(config).await.unwrap();

        assert!(plugin.add_viewer("viewer1".to_string()).is_ok());
        assert!(plugin.add_viewer("viewer2".to_string()).is_ok());

        plugin.remove_viewer("viewer1");

        let stats = plugin.get_stats().unwrap();
        assert_eq!(stats.viewer_count, 1);
    }

    #[tokio::test]
    async fn test_handle_start_packet_signaling() {
        let mut device = create_test_device();
        let factory = ScreenSharePluginFactory;
        let mut plugin = factory.create();

        let (tx, mut rx) = tokio::sync::mpsc::channel(100);
        plugin.init(&device, tx).await.unwrap();
        plugin.start().await.unwrap();

        let config = ShareConfig::default();
        let body = serde_json::to_value(&config).unwrap();
        let packet = Packet::new("cconnect.screenshare.start", body);

        // Test signaling (no local port set)
        assert!(plugin.handle_packet(&packet, &mut device).await.is_ok());

        // Should receive internal packet request
        let (dev_id, sent_packet) = rx.recv().await.unwrap();
        assert_eq!(dev_id, device.id());
        assert_eq!(sent_packet.packet_type, "cconnect.internal.screenshare.requested");
        
        let screenshare_plugin = plugin.as_any_mut().downcast_mut::<ScreenSharePlugin>().unwrap();
        assert!(screenshare_plugin.is_receiving());
        
        // Now set port
        screenshare_plugin.set_local_port(12345).await.unwrap();
        
        // Should receive ready packet
        let (dev_id, sent_packet) = rx.recv().await.unwrap();
        assert_eq!(dev_id, device.id());
        assert_eq!(sent_packet.packet_type, "cconnect.screenshare.ready");
        assert_eq!(sent_packet.body["tcpPort"], 12345);
    }
}