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
//! - [x] XDG Desktop Portal integration for screen selection
//! - [x] Adaptive bitrate control (adjusts encoder based on network throughput)
//! - [x] Multiple viewer management (broadcast channel architecture)
//! - [x] Cursor tracking (DBus signals emitted, mirror UI receives updates)
//! - [x] Annotation system (DBus signals emitted, mirror UI receives updates)
//! - [x] Canvas-based cursor/annotation rendering (Stack + Canvas overlay on video)

pub mod capture;
pub mod decoder;
pub mod portal;
pub mod stream_receiver;
pub mod stream_sender;

use crate::plugins::{Plugin, PluginFactory};
use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

/// Global storage for pending screenshare sessions that survive plugin restarts
/// This is needed because when a remote device reconnects (e.g., to prepare for streaming),
/// the plugin gets stopped and restarted, losing the active_session state.
/// By storing pending sessions globally, we can restore them after reconnection.
static PENDING_SESSIONS: OnceLock<std::sync::Mutex<HashMap<String, ShareConfig>>> = OnceLock::new();

fn pending_sessions() -> &'static std::sync::Mutex<HashMap<String, ShareConfig>> {
    PENDING_SESSIONS.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// Store a pending session for a device (called when we initiate sharing)
fn store_pending_session(device_id: &str, config: ShareConfig) {
    if let Ok(mut sessions) = pending_sessions().lock() {
        info!(
            "Storing pending screenshare session for {} (survives reconnection)",
            device_id
        );
        sessions.insert(device_id.to_string(), config);
    }
}

/// Retrieve and remove a pending session for a device (called when ready packet received)
fn take_pending_session(device_id: &str) -> Option<ShareConfig> {
    if let Ok(mut sessions) = pending_sessions().lock() {
        let config = sessions.remove(device_id);
        if config.is_some() {
            info!("Retrieved pending screenshare session for {}", device_id);
        }
        config
    } else {
        None
    }
}

/// Clear pending session for a device (called on explicit stop)
fn clear_pending_session(device_id: &str) {
    if let Ok(mut sessions) = pending_sessions().lock() {
        if sessions.remove(device_id).is_some() {
            info!("Cleared pending screenshare session for {}", device_id);
        }
    }
}

#[cfg(feature = "screenshare")]
use capture::{CaptureConfig, ScreenCapture};
#[cfg(feature = "screenshare")]
use stream_sender::StreamSender;

const PLUGIN_NAME: &str = "screenshare";
const INCOMING_CAPABILITY: &str = "cconnect.screenshare";
const OUTGOING_CAPABILITY: &str = "cconnect.screenshare";

// Screen share configuration constants
const DEFAULT_FPS: u8 = 30;
const DEFAULT_BITRATE_KBPS: u32 = 2000; // 2 Mbps
const DEFAULT_QUALITY: &str = "medium";
const MAX_VIEWERS: usize = 10; // Max simultaneous viewers

/// Build the list of incoming capabilities for screenshare plugin
fn screenshare_incoming_capabilities() -> Vec<String> {
    vec![
        // Base capability
        INCOMING_CAPABILITY.to_string(),
        "kdeconnect.screenshare".to_string(),
        // Specific packet types
        "cconnect.screenshare.start".to_string(),
        "cconnect.screenshare.frame".to_string(),
        "cconnect.screenshare.cursor".to_string(),
        "cconnect.screenshare.annotation".to_string(),
        "cconnect.screenshare.stop".to_string(),
        "cconnect.screenshare.ready".to_string(),
        // Request packet - allows remote to request us to share our screen
        "cconnect.screenshare.request".to_string(),
        "kdeconnect.screenshare.request".to_string(),
        // KDE Connect compatibility
        "kdeconnect.screenshare.start".to_string(),
        "kdeconnect.screenshare.frame".to_string(),
        "kdeconnect.screenshare.cursor".to_string(),
        "kdeconnect.screenshare.annotation".to_string(),
        "kdeconnect.screenshare.stop".to_string(),
        "kdeconnect.screenshare.ready".to_string(),
    ]
}

/// Screen share mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ShareMode {
    /// Share entire screen
    #[default]
    FullScreen,
    /// Share specific window
    Window,
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
#[derive(Default)]
pub enum VideoCodec {
    /// H.264 codec (widely supported)
    #[default]
    H264,
    /// VP8 codec (WebRTC standard)
    Vp8,
    /// VP9 codec (better compression)
    Vp9,
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

    /// Include system audio in screen share
    #[serde(default)]
    pub include_audio: bool,
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
            include_audio: false,
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

/// Handle to a running streaming task
type StreamingHandle = tokio::task::JoinHandle<()>;

/// Frame data for broadcast to viewers
#[derive(Clone)]
pub struct BroadcastFrame {
    /// Encoded frame data
    pub data: Vec<u8>,
    /// Presentation timestamp
    pub pts: u64,
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

    /// Handle to the capture task (produces frames)
    capture_task: Option<StreamingHandle>,

    /// Handles to sender tasks (one per viewer)
    sender_tasks: std::collections::HashMap<String, StreamingHandle>,

    /// Broadcast channel for frame distribution to multiple viewers
    frame_sender: Option<tokio::sync::broadcast::Sender<BroadcastFrame>>,

    /// Shared flag to signal streaming stop
    stop_streaming: Arc<Mutex<bool>>,

    /// Shared flag to signal streaming pause
    pause_streaming: Arc<Mutex<bool>>,
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
            capture_task: None,
            sender_tasks: std::collections::HashMap::new(),
            frame_sender: None,
            stop_streaming: Arc::new(Mutex::new(false)),
            pause_streaming: Arc::new(Mutex::new(false)),
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

        // Note: Capture and streaming are started when receiver sends ready packet
        // See start_streaming_to_device() which is called from handle_packet()

        Ok(())
    }

    /// Stop screen sharing session
    ///
    /// This is called both for explicit user stops and plugin restarts.
    /// Use `stop_sharing_explicit()` to also clear pending sessions.
    pub async fn stop_sharing(&mut self) -> Result<()> {
        // Stop streaming task first
        self.stop_streaming().await;

        if let Some(session) = self.active_session.take() {
            let stats = session.get_stats();
            info!(
                "Stopped screen share: {} frames, {} MB, {} viewers, {} seconds",
                stats.frames_sent,
                stats.bytes_sent / 1_000_000,
                stats.viewer_count,
                stats.duration_secs
            );
        }

        Ok(())
    }

    /// Stop screen sharing and clear pending sessions
    ///
    /// Use this when user explicitly stops sharing (not for plugin restart).
    pub async fn stop_sharing_explicit(&mut self) -> Result<()> {
        // Clear pending session for this device
        if let Some(device_id) = &self.device_id {
            clear_pending_session(device_id);
        }
        self.stop_sharing().await
    }

    /// Pause screen sharing session
    ///
    /// The capture pipeline is paused but the session remains active.
    /// Viewers will see a frozen frame until resumed.
    pub async fn pause_sharing(&mut self) -> Result<()> {
        self.set_pause_state(true, "pause").await
    }

    /// Resume screen sharing session after pause
    pub async fn resume_sharing(&mut self) -> Result<()> {
        self.set_pause_state(false, "resume").await
    }

    /// Set the pause state for the active session
    async fn set_pause_state(&mut self, paused: bool, action: &str) -> Result<()> {
        if self.active_session.is_none() {
            return Err(ProtocolError::Plugin(format!(
                "No active screen share session to {}",
                action
            )));
        }

        *self.pause_streaming.lock().await = paused;
        info!("Screen share {}d", action);
        Ok(())
    }

    /// Check if screen sharing is currently paused
    pub async fn is_paused(&self) -> bool {
        *self.pause_streaming.lock().await
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
                    sender
                        .send((device_id.clone(), packet))
                        .await
                        .map_err(|_| {
                            ProtocolError::Plugin("Failed to send ready packet".to_string())
                        })?;
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
                // Store pending session BEFORE sending packet
                // This survives plugin restart if device reconnects
                store_pending_session(device_id, config.clone());

                let body = serde_json::to_value(&config).map_err(|e| {
                    ProtocolError::Plugin(format!("Failed to serialize config: {}", e))
                })?;
                let packet = Packet::new("cconnect.screenshare.start", body);

                sender
                    .send((device_id.clone(), packet))
                    .await
                    .map_err(|_| {
                        ProtocolError::Plugin("Failed to send start packet".to_string())
                    })?;

                info!("Sent screen share start to {}", device_id);
                Ok(())
            } else {
                Err(ProtocolError::Plugin("No device ID set".to_string()))
            }
        } else {
            Err(ProtocolError::Plugin(
                "No packet sender available".to_string(),
            ))
        }
    }

    /// Request remote device to share their screen with us
    ///
    /// Sends a request packet asking the remote device to start sharing their screen.
    /// If accepted by the remote, they will send a `screenshare.start` packet and
    /// we can then open the mirror viewer to receive their stream.
    pub async fn request_screen_share(&self) -> Result<()> {
        if let Some(sender) = &self.packet_sender {
            if let Some(device_id) = &self.device_id {
                let packet = Packet::new(
                    "cconnect.screenshare.request",
                    serde_json::json!({
                        "message": "Please share your screen"
                    }),
                );

                sender
                    .send((device_id.clone(), packet))
                    .await
                    .map_err(|_| {
                        ProtocolError::Plugin("Failed to send request packet".to_string())
                    })?;

                info!("Sent screen share request to {}", device_id);
                Ok(())
            } else {
                Err(ProtocolError::Plugin("No device ID set".to_string()))
            }
        } else {
            Err(ProtocolError::Plugin(
                "No packet sender available".to_string(),
            ))
        }
    }

    /// Start streaming to a remote device (viewer)
    ///
    /// This method supports multiple viewers:
    /// - First viewer: initializes capture and starts broadcasting frames
    /// - Additional viewers: spawn new sender tasks subscribed to the broadcast
    #[cfg(feature = "screenshare")]
    pub async fn start_streaming_to_device(
        &mut self,
        host: String,
        port: u16,
        viewer_id: String,
    ) -> Result<()> {
        // Get config from active session
        let config = self
            .active_session
            .as_ref()
            .ok_or_else(|| ProtocolError::Plugin("No active sharing session".to_string()))?
            .config
            .clone();

        info!("Adding viewer {} at {}:{}", viewer_id, host, port);

        // Check if capture is already running
        let is_first_viewer = self.capture_task.is_none();

        if is_first_viewer {
            // First viewer - initialize capture and broadcast channel
            info!(
                "First viewer - starting capture with {} fps, {} kbps",
                config.fps, config.bitrate_kbps
            );

            // Reset stop flag
            *self.stop_streaming.lock().await = false;

            // Create broadcast channel for frame distribution (buffer 16 frames)
            let (tx, _) = tokio::sync::broadcast::channel::<BroadcastFrame>(16);
            self.frame_sender = Some(tx.clone());

            // Request screen share permission via XDG Desktop Portal
            let portal_session = portal::request_screencast().await.ok();
            if let Some(ref session) = portal_session {
                info!(
                    "Portal session acquired: node_id={}",
                    session.pipewire_node_id
                );
            } else {
                warn!("Portal request failed, falling back to test source");
            }

            // Extract PipeWire parameters from portal session
            let (pipewire_fd, pipewire_node_id) = portal_session
                .as_ref()
                .map(|s| (Some(s.fd()), Some(s.pipewire_node_id)))
                .unwrap_or((None, None));

            let capture_config = CaptureConfig {
                fps: config.fps as u32,
                bitrate_kbps: config.bitrate_kbps,
                width: 0,
                height: 0,
                pipewire_node_id,
                pipewire_fd,
                include_audio: config.include_audio,
            };

            // Adaptive bitrate settings
            let adaptive_bitrate = config.adaptive_bitrate;
            let target_bitrate_kbps = config.bitrate_kbps;
            let min_bitrate_kbps = 200_u32;
            let max_bitrate_kbps = target_bitrate_kbps.saturating_mul(2).min(50000);

            let stop_flag = self.stop_streaming.clone();
            let pause_flag = self.pause_streaming.clone();
            let frame_tx = tx.clone();

            // Spawn capture task
            let capture_handle = tokio::spawn(async move {
                // Initialize capture
                let mut capture = ScreenCapture::new(capture_config);
                if let Err(e) = capture.init() {
                    error!("Failed to initialize screen capture: {}", e);
                    return;
                }

                if let Err(e) = capture.start() {
                    error!("Failed to start screen capture: {}", e);
                    return;
                }

                info!("Capture started, broadcasting frames");

                let frame_interval = std::time::Duration::from_millis(1000 / 30);
                let mut last_frame = std::time::Instant::now();
                let mut last_bitrate_check = std::time::Instant::now();
                let bitrate_check_interval = std::time::Duration::from_secs(2);
                let mut frames_captured: u64 = 0;
                let mut was_paused = false;

                loop {
                    // Check stop flag
                    if *stop_flag.lock().await {
                        info!("Capture stop requested");
                        break;
                    }

                    // Check pause flag
                    let is_paused = *pause_flag.lock().await;
                    if is_paused && !was_paused {
                        // Transition to paused state
                        if let Err(e) = capture.pause() {
                            error!("Failed to pause capture: {}", e);
                        }
                        was_paused = true;
                        debug!("Capture paused");
                    } else if !is_paused && was_paused {
                        // Transition to resumed state
                        if let Err(e) = capture.resume() {
                            error!("Failed to resume capture: {}", e);
                        }
                        was_paused = false;
                        debug!("Capture resumed");
                    }

                    // While paused, just sleep and continue checking flags
                    if is_paused {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        continue;
                    }

                    // Adaptive bitrate control (based on subscriber count/health)
                    if adaptive_bitrate && last_bitrate_check.elapsed() >= bitrate_check_interval {
                        last_bitrate_check = std::time::Instant::now();
                        let current_bitrate = capture.current_bitrate_kbps();

                        // Check receiver count - if 0 receivers, we might reduce quality
                        let receiver_count = frame_tx.receiver_count();
                        if receiver_count == 0 && current_bitrate > min_bitrate_kbps {
                            // No active viewers, reduce bitrate to minimum
                            debug!("No active viewers, reducing bitrate to minimum");
                            let _ = capture.set_bitrate(min_bitrate_kbps);
                        } else if receiver_count > 0 && current_bitrate < target_bitrate_kbps {
                            // Viewers present and below target, increase bitrate
                            let new_bitrate = (current_bitrate as f32 * 1.1) as u32;
                            let new_bitrate = new_bitrate.min(max_bitrate_kbps);
                            if new_bitrate != current_bitrate {
                                debug!(
                                    "Adaptive bitrate: {} kbps -> {} kbps ({} viewers)",
                                    current_bitrate, new_bitrate, receiver_count
                                );
                                let _ = capture.set_bitrate(new_bitrate);
                            }
                        }
                    }

                    // Pull frame from capture
                    match capture.pull_frame() {
                        Ok(Some(frame)) => {
                            frames_captured += 1;
                            // Broadcast to all viewers
                            let broadcast_frame = BroadcastFrame {
                                data: frame.data,
                                pts: frame.pts,
                            };
                            // send() returns error if no receivers, which is fine
                            let _ = frame_tx.send(broadcast_frame);
                            last_frame = std::time::Instant::now();
                        }
                        Ok(None) => {
                            let elapsed = last_frame.elapsed();
                            if elapsed < frame_interval {
                                tokio::time::sleep(frame_interval - elapsed).await;
                            }
                        }
                        Err(e) => {
                            error!("Failed to pull frame: {}", e);
                            break;
                        }
                    }
                }

                // Cleanup
                info!("Capture ended: {} frames captured", frames_captured);
                let _ = capture.stop();
            });

            self.capture_task = Some(capture_handle);
        }

        // Spawn sender task for this viewer
        let frame_rx = self
            .frame_sender
            .as_ref()
            .ok_or_else(|| ProtocolError::Plugin("No frame sender available".to_string()))?
            .subscribe();

        let stop_flag = self.stop_streaming.clone();
        let viewer_id_clone = viewer_id.clone();

        let sender_handle = tokio::spawn(async move {
            // Connect to viewer
            let mut sender = StreamSender::new();
            if let Err(e) = sender.connect(&host, port).await {
                error!(
                    "Failed to connect to viewer {} at {}:{}: {}",
                    viewer_id_clone, host, port, e
                );
                return;
            }

            info!(
                "Streaming to viewer {} at {}:{}",
                viewer_id_clone, host, port
            );

            let mut rx = frame_rx;

            loop {
                // Check stop flag
                if *stop_flag.lock().await {
                    info!("Viewer {} streaming stop requested", viewer_id_clone);
                    break;
                }

                // Receive frame from broadcast
                match rx.recv().await {
                    Ok(frame) => {
                        if let Err(e) = sender.send_video_frame(&frame.data, frame.pts).await {
                            error!("Failed to send frame to viewer {}: {}", viewer_id_clone, e);
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Viewer {} lagged {} frames", viewer_id_clone, n);
                        // Continue - we'll catch up
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("Broadcast channel closed for viewer {}", viewer_id_clone);
                        break;
                    }
                }
            }

            // Cleanup
            info!("Streaming to viewer {} ended", viewer_id_clone);
            let _ = sender.send_end_of_stream().await;
            sender.close().await;

            let (frames, bytes) = sender.stats();
            info!(
                "Viewer {} stats: {} frames, {} bytes sent",
                viewer_id_clone, frames, bytes
            );
        });

        self.sender_tasks.insert(viewer_id, sender_handle);
        Ok(())
    }

    /// Start streaming - stub when screenshare feature is disabled
    #[cfg(not(feature = "screenshare"))]
    pub async fn start_streaming_to_device(
        &mut self,
        _host: String,
        _port: u16,
        _viewer_id: String,
    ) -> Result<()> {
        Err(ProtocolError::Plugin(
            "screenshare feature not enabled".to_string(),
        ))
    }

    /// Remove a viewer from the streaming session
    pub async fn remove_viewer_stream(&mut self, viewer_id: &str) {
        if let Some(handle) = self.sender_tasks.remove(viewer_id) {
            handle.abort();
            info!("Removed streaming task for viewer {}", viewer_id);
        }

        // If no more viewers, stop capture to save resources
        if self.sender_tasks.is_empty() {
            info!("No more viewers, stopping capture");
            self.stop_streaming().await;
        }
    }

    /// Get the number of active viewers
    pub fn viewer_count(&self) -> usize {
        self.sender_tasks.len()
    }

    /// Emit an internal packet for DBus signaling
    ///
    /// Internal packets are intercepted by the daemon and converted to DBus signals.
    /// Errors are silently ignored since signal emission is best-effort.
    async fn emit_internal_packet(
        &self,
        device_id: &str,
        packet_type: &str,
        body: serde_json::Value,
    ) {
        if let Some(sender) = &self.packet_sender {
            let packet = Packet::new(packet_type, body);
            let _ = sender.send((device_id.to_string(), packet)).await;
        }
    }

    /// Stop all streaming tasks
    pub async fn stop_streaming(&mut self) {
        // Signal stop and reset pause
        *self.stop_streaming.lock().await = true;
        *self.pause_streaming.lock().await = false;

        // Stop all sender tasks
        for (viewer_id, handle) in self.sender_tasks.drain() {
            info!("Stopping sender task for viewer {}", viewer_id);
            handle.abort();
        }

        // Stop capture task with brief delay to allow graceful shutdown
        if let Some(handle) = self.capture_task.take() {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            handle.abort();
        }

        // Clear frame sender
        self.frame_sender = None;

        info!("All streaming stopped");
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
        screenshare_incoming_capabilities()
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            OUTGOING_CAPABILITY.to_string(),
            "cconnect.screenshare.start".to_string(),
            "cconnect.screenshare.ready".to_string(),
            "cconnect.screenshare.stop".to_string(),
            // Request packet - allows us to request remote to share their screen
            "cconnect.screenshare.request".to_string(),
        ]
    }

    async fn init(
        &mut self,
        device: &Device,
        packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>,
    ) -> Result<()> {
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

        let device_id = device.id();
        debug!("Handling packet type: {}", packet.packet_type);

        if packet.is_type("cconnect.screenshare.start") {
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
                    let ready_packet = Packet::new("cconnect.screenshare.ready", body);
                    if let Err(e) = sender.send((device_id.to_string(), ready_packet)).await {
                        error!("Failed to send ready packet: {}", e);
                    }
                }
            } else {
                // UI not ready, request UI start
                info!("Screen share started by remote, requesting UI launch");
                self.emit_internal_packet(
                    device_id,
                    "cconnect.internal.screenshare.requested",
                    serde_json::json!({}),
                )
                .await;
            }
        } else if packet.is_type("cconnect.screenshare.frame") {
            if !self.receiving {
                warn!("Received frame but not in receiving mode");
                return Ok(());
            }
            // Frames are handled via separate TCP stream
            debug!("Received screen frame packet (unexpected for streaming mode)");
        } else if packet.is_type("cconnect.screenshare.cursor") {
            let position: CursorPosition = serde_json::from_value(packet.body.clone())
                .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

            self.emit_internal_packet(
                device_id,
                "cconnect.internal.screenshare.cursor",
                serde_json::json!({
                    "x": position.x,
                    "y": position.y,
                    "visible": position.visible
                }),
            )
            .await;

            debug!("Cursor updated: ({}, {})", position.x, position.y);
        } else if packet.is_type("cconnect.screenshare.annotation") {
            let annotation: Annotation = serde_json::from_value(packet.body.clone())
                .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

            self.emit_internal_packet(
                device_id,
                "cconnect.internal.screenshare.annotation",
                serde_json::json!({
                    "annotation_type": annotation.annotation_type,
                    "x1": annotation.x1,
                    "y1": annotation.y1,
                    "x2": annotation.x2.unwrap_or(0),
                    "y2": annotation.y2.unwrap_or(0),
                    "color": annotation.color,
                    "width": annotation.width
                }),
            )
            .await;

            debug!("Annotation received: {}", annotation.annotation_type);
        } else if packet.is_type("cconnect.screenshare.stop") {
            info!("Screen share stop from {}", device.name());

            self.emit_internal_packet(
                device_id,
                "cconnect.internal.screenshare.stopped",
                serde_json::json!({}),
            )
            .await;

            self.receiving = false;

            // Remove this device as a viewer if applicable
            let viewer_id = device_id.to_string();
            self.remove_viewer_stream(&viewer_id).await;
            self.remove_viewer(&viewer_id);
        } else if packet.is_type("cconnect.screenshare.ready") {
            let tcp_port = packet
                .body
                .get("tcpPort")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u16;

            info!("Receiver {} is ready on port {}", device.name(), tcp_port);

            // Check if we have an active session, if not try to restore from pending
            // This handles the case where the device reconnected and our plugin was restarted
            if self.active_session.is_none() {
                if let Some(pending_config) = take_pending_session(device_id) {
                    info!(
                        "Restoring pending screenshare session for {} after reconnection",
                        device_id
                    );
                    // Restore the session without re-sending the start packet
                    let session = ShareSession::new(pending_config);
                    self.active_session = Some(session);
                } else {
                    warn!(
                        "Received ready packet from {} but no active or pending session",
                        device.name()
                    );
                    return Ok(());
                }
            } else {
                // Clear the pending session since we have an active one
                clear_pending_session(device_id);
            }

            if let Err(e) = self.add_viewer(device_id.to_string()) {
                warn!("Failed to add viewer {}: {}", device_id, e);
            }

            let host = device.host.clone().ok_or_else(|| {
                error!(
                    "Cannot stream to device {}: no host address available",
                    device.name()
                );
                ProtocolError::Plugin("Device has no host address for streaming".to_string())
            })?;

            let viewer_id = device_id.to_string();
            self.start_streaming_to_device(host.clone(), tcp_port, viewer_id)
                .await
                .inspect_err(|e| {
                    error!("Failed to start streaming to {}:{}: {}", host, tcp_port, e)
                })?;

            self.emit_internal_packet(
                device_id,
                "cconnect.internal.screenshare.started",
                serde_json::json!({ "is_sender": true }),
            )
            .await;

            info!(
                "Started streaming screen share to {} ({}:{}) [viewers: {}]",
                device.name(),
                host,
                tcp_port,
                self.viewer_count()
            );
        } else if packet.is_type("cconnect.screenshare.request")
            || packet.is_type("kdeconnect.screenshare.request")
        {
            // Remote device is requesting us to share our screen with them
            info!(
                "Received screen share request from {} - they want to view our screen",
                device.name()
            );

            // Emit internal signal for UI to handle (show consent dialog or auto-accept)
            self.emit_internal_packet(
                device_id,
                "cconnect.internal.screenshare.share_requested",
                serde_json::json!({
                    "requester_name": device.name(),
                    "requester_id": device_id,
                }),
            )
            .await;
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
        screenshare_incoming_capabilities()
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            OUTGOING_CAPABILITY.to_string(),
            "cconnect.screenshare.start".to_string(),
            "cconnect.screenshare.ready".to_string(),
            "cconnect.screenshare.stop".to_string(),
            "cconnect.screenshare.request".to_string(),
        ]
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
        assert_eq!(
            sent_packet.packet_type,
            "cconnect.internal.screenshare.requested"
        );

        let screenshare_plugin = plugin
            .as_any_mut()
            .downcast_mut::<ScreenSharePlugin>()
            .unwrap();
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
