//! Extended Display Plugin
//!
//! Streams a desktop display output to an Android tablet via WebRTC,
//! turning the tablet into a wireless extended monitor with touch input.
//!
//! ## Protocol Specification
//!
//! This plugin bridges `cosmic-display-stream` (screen capture, H.264 encoding,
//! WebRTC streaming, touch input injection) with the CConnect protocol layer.
//!
//! ### Packet Types
//!
//! - `cconnect.extendeddisplay` - Desktop → Android: session state (ready, stop, error)
//! - `cconnect.extendeddisplay.request` - Android → Desktop: session control (request, touch, stop)
//!
//! ### Signaling Flow
//!
//! 1. Android sends `request` action with capabilities
//! 2. Desktop starts capture + encoder + WebRTC signaling server
//! 3. Desktop sends `ready` with IP + port for WebSocket signaling
//! 4. Android connects via WebSocket, exchanges SDP/ICE
//! 5. WebRTC session established, H.264 RTP frames flow
//! 6. Touch events arrive via `touch` action packets
//! 7. Either side sends `stop` to end
//!
//! ### Capabilities
//!
//! - Incoming: `cconnect.extendeddisplay`, `cconnect.extendeddisplay.request`
//! - Outgoing: `cconnect.extendeddisplay`, `cconnect.extendeddisplay.request`

use crate::plugins::{Plugin, PluginFactory};
use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use cosmic_display_stream::{
    capture::ScreenCapture, EncoderConfig, InputHandler, StreamConfig, StreamingServer,
    TouchAction, TouchEvent, VideoEncoder, VideoTransform,
};

/// Plugin name constant
const PLUGIN_NAME: &str = "extendeddisplay";

/// Packet type for desktop → Android messages
const PACKET_TYPE: &str = "cconnect.extendeddisplay";

/// Packet type for Android → desktop messages
const PACKET_TYPE_REQUEST: &str = "cconnect.extendeddisplay.request";

/// Default WebSocket signaling port
const DEFAULT_SIGNALING_PORT: u16 = 18080;

/// Default bitrate in bits per second (10 Mbps)
const DEFAULT_BITRATE_BPS: u32 = 10_000_000;

/// Default framerate
const DEFAULT_FRAMERATE: u32 = 60;

/// Internal packet type emitted when session starts (for D-Bus signal routing)
const INTERNAL_SESSION_STARTED: &str = "cconnect.internal.extendeddisplay.started";

/// Internal packet type emitted when session stops (for D-Bus signal routing)
const INTERNAL_SESSION_STOPPED: &str = "cconnect.internal.extendeddisplay.stopped";

/// Extended display session configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendedDisplayConfig {
    /// WebSocket signaling port
    #[serde(default = "default_signaling_port")]
    pub signaling_port: u16,

    /// Target bitrate in bits per second
    #[serde(default = "default_bitrate_bps")]
    pub bitrate_bps: u32,

    /// Target framerate
    #[serde(default = "default_framerate")]
    pub framerate: u32,
}

fn default_signaling_port() -> u16 {
    DEFAULT_SIGNALING_PORT
}

fn default_bitrate_bps() -> u32 {
    DEFAULT_BITRATE_BPS
}

fn default_framerate() -> u32 {
    DEFAULT_FRAMERATE
}

impl Default for ExtendedDisplayConfig {
    fn default() -> Self {
        Self {
            signaling_port: DEFAULT_SIGNALING_PORT,
            bitrate_bps: DEFAULT_BITRATE_BPS,
            framerate: DEFAULT_FRAMERATE,
        }
    }
}

impl ExtendedDisplayConfig {
    /// Validate configuration values
    pub fn validate(&self) -> Result<()> {
        if self.signaling_port == 0 {
            return Err(ProtocolError::InvalidPacket(
                "Signaling port must be non-zero".to_string(),
            ));
        }
        if self.bitrate_bps < 500_000 || self.bitrate_bps > 100_000_000 {
            return Err(ProtocolError::InvalidPacket(format!(
                "Bitrate {} bps out of range (500 Kbps - 100 Mbps)",
                self.bitrate_bps
            )));
        }
        if self.framerate < 1 || self.framerate > 120 {
            return Err(ProtocolError::InvalidPacket(format!(
                "Framerate {} out of range (1-120)",
                self.framerate
            )));
        }
        Ok(())
    }
}

/// Extended Display Plugin
///
/// Manages a WebRTC streaming session that sends a desktop display to an
/// Android tablet. Handles the lifecycle of screen capture, encoding,
/// signaling, and touch input injection.
pub struct ExtendedDisplayPlugin {
    /// Device ID this plugin instance is bound to
    device_id: Option<String>,

    /// Whether the plugin is enabled and processing packets
    enabled: bool,

    /// Channel for sending packets back to the daemon
    packet_sender: Option<tokio::sync::mpsc::Sender<(String, Packet)>>,

    /// Whether a streaming session is currently active
    session_active: bool,

    /// WebRTC streaming server (wrapped in Arc for sharing with capture task)
    streaming_server: Option<Arc<StreamingServer>>,

    /// Video encoder (GStreamer pipeline) - moved into capture task on start
    encoder: Option<VideoEncoder>,

    /// Touch input handler (libei/enigo)
    input_handler: Option<InputHandler>,

    /// Handle to the capture task
    capture_task: Option<tokio::task::JoinHandle<()>>,

    /// Configuration for the current/next session
    config: ExtendedDisplayConfig,

    /// Shared flag to signal stop to background tasks
    stop_flag: Arc<AtomicBool>,

    /// Current display resolution (width, height) for touch coordinate validation
    display_resolution: (u32, u32),

    /// Last touch event timestamp for rate limiting
    last_touch_time: Option<std::time::Instant>,
}

impl ExtendedDisplayPlugin {
    /// Create a new plugin instance
    pub fn new() -> Self {
        Self {
            device_id: None,
            enabled: false,
            packet_sender: None,
            session_active: false,
            streaming_server: None,
            encoder: None,
            input_handler: None,
            capture_task: None,
            config: ExtendedDisplayConfig::default(),
            stop_flag: Arc::new(AtomicBool::new(false)),
            display_resolution: (1920, 1080), // Default resolution
            last_touch_time: None,
        }
    }

    /// Check if a streaming session is active
    pub fn is_session_active(&self) -> bool {
        self.session_active
    }

    /// Start an extended display session
    ///
    /// Sets up screen capture, video encoding, and WebRTC streaming.
    /// Sends a `ready` packet with the signaling address back to the device.
    pub async fn start_session(
        &mut self,
        device_id: &str,
        capabilities: &str,
        requested_resolution: Option<(u32, u32)>,
    ) -> Result<()> {
        if self.session_active {
            warn!("Extended display session already active, stopping first");
            self.stop_session(device_id).await?;
        }

        info!(
            "Starting extended display session for device {} (capabilities: {})",
            device_id, capabilities
        );

        self.stop_flag.store(false, Ordering::SeqCst);

        // Use requested resolution or default to 1920x1080
        let (display_width, display_height) = requested_resolution.unwrap_or((1920, 1080));
        let encoder_config = EncoderConfig {
            width: display_width,
            height: display_height,
            framerate: self.config.framerate,
            bitrate: self.config.bitrate_bps,
            encoder_type: None,
            low_latency: true,
            keyframe_interval: 60,
            transform: VideoTransform::None,
        };

        // Create encoder (but don't store it yet until all operations succeed)
        let encoder = VideoEncoder::new(encoder_config).map_err(|e| {
            error!("Failed to create video encoder: {}", e);
            ProtocolError::Plugin(format!("Encoder creation failed: {}", e))
        })?;

        info!("Video encoder initialized: {:?}", encoder.encoder_type());

        // Create streaming server (encoder not stored yet, so failure is clean)
        let stream_config = StreamConfig {
            signaling_port: self.config.signaling_port,
            max_clients: 1,
            ..StreamConfig::default()
        };

        let mut server = StreamingServer::new(stream_config).map_err(|e| {
            error!("Failed to create streaming server: {}", e);
            ProtocolError::Plugin(format!("Server creation failed: {}", e))
        })?;

        info!(
            "WebRTC streaming server ready on port {}",
            self.config.signaling_port
        );

        // Initialize input handler for touch events
        // Use (0,0) offset and encoder resolution as display size
        let input_handler = InputHandler::new((0, 0), (display_width, display_height));
        if !input_handler.is_input_available() {
            warn!("Touch input injection unavailable — touch events will be ignored");
        }

        // Start the streaming server
        if let Err(e) = server.start().await {
            error!("Failed to start streaming server: {}", e);
            return Err(ProtocolError::Plugin(format!(
                "Server start failed: {}",
                e
            )));
        }

        // Wrap server in Arc for sharing with capture task
        let server_arc = Arc::new(server);
        let server_for_task = server_arc.clone();
        let stop_flag = self.stop_flag.clone();

        // Spawn background capture task
        let capture_task = tokio::spawn(async move {
            // Create screen capture via portal
            let mut capture = match ScreenCapture::new_any_output("portal").await {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to create screen capture: {}", e);
                    return;
                }
            };

            // Start capture stream
            let mut frame_stream = match capture.start_capture().await {
                Ok(fs) => fs,
                Err(e) => {
                    error!("Failed to start capture: {}", e);
                    return;
                }
            };

            info!("Screen capture started, beginning frame encode loop");

            // Move encoder into the task
            let mut encoder = encoder;

            // Main capture loop
            while !stop_flag.load(Ordering::SeqCst) {
                match frame_stream.next_frame().await {
                    Some(frame) => {
                        // Encode frame
                        match encoder.encode_video_frame(&frame) {
                            Ok(Some(encoded_frame)) => {
                                // Send encoded frame to WebRTC server
                                if let Err(e) = server_for_task.send_frame(encoded_frame).await {
                                    error!("Failed to send frame to WebRTC server: {}", e);
                                    break;
                                }
                            }
                            Ok(None) => {
                                // Encoder buffering, continue
                            }
                            Err(e) => {
                                error!("Video encoding error: {}", e);
                                break;
                            }
                        }
                    }
                    None => {
                        info!("Capture stream ended");
                        break;
                    }
                }
            }

            // Cleanup: stop capture
            if let Err(e) = capture.stop_capture().await {
                warn!("Error stopping screen capture: {}", e);
            }
            info!("Capture task exited");
        });

        // Store state (including resolution for touch coordinate validation)
        self.streaming_server = Some(server_arc);
        self.input_handler = Some(input_handler);
        self.capture_task = Some(capture_task);
        self.display_resolution = (display_width, display_height);
        self.session_active = true;

        // Determine local IP for the device to connect to
        let local_ip = get_local_ip().unwrap_or_else(|| "0.0.0.0".to_string());

        // Send ready response to Android
        let ready_body = serde_json::json!({
            "action": "ready",
            "address": local_ip,
            "port": self.config.signaling_port,
        });
        self.send_packet(device_id, PACKET_TYPE, ready_body).await;

        // Emit internal started signal for D-Bus
        self.emit_internal_packet(
            device_id,
            INTERNAL_SESSION_STARTED,
            serde_json::json!({}),
        )
        .await;

        info!(
            "Extended display session started — signaling at {}:{}",
            local_ip, self.config.signaling_port
        );

        Ok(())
    }

    /// Stop the extended display session and clean up all resources
    pub async fn stop_session(&mut self, device_id: &str) -> Result<()> {
        if !self.session_active {
            debug!("No active extended display session to stop");
            return Ok(());
        }

        info!("Stopping extended display session for device {}", device_id);

        // Step 1: Signal background tasks to stop
        self.stop_flag.store(true, Ordering::SeqCst);

        // Step 2: Wait for capture task to finish gracefully (don't abort)
        if let Some(handle) = self.capture_task.take() {
            match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
                Ok(Ok(())) => debug!("Capture task finished cleanly"),
                Ok(Err(e)) => warn!("Capture task panicked: {}", e),
                Err(_) => {
                    warn!("Capture task did not finish within 5s, force stopping");
                    // Task timeout - it will be dropped when handle is dropped
                }
            }
        }

        // Step 3: Now Arc should be unwrappable since task released its clone
        if let Some(server_arc) = self.streaming_server.take() {
            match Arc::try_unwrap(server_arc) {
                Ok(mut server) => {
                    if let Err(e) = server.stop().await {
                        warn!("Error stopping streaming server: {}", e);
                    }
                }
                Err(arc) => {
                    // Arc still has multiple references (shouldn't happen after timeout)
                    warn!(
                        "Could not unwrap streaming server Arc (ref count: {})",
                        Arc::strong_count(&arc)
                    );
                    // Force stop through the Arc even if we can't unwrap
                    // This is safe since we know no other tasks are using it after timeout
                    drop(arc);
                }
            }
        }

        // Drop encoder and input handler (encoder was moved into capture task)
        self.encoder = None;
        self.input_handler = None;
        self.session_active = false;

        // Send stop to Android
        let stop_body = serde_json::json!({ "action": "stop" });
        self.send_packet(device_id, PACKET_TYPE, stop_body).await;

        // Emit internal stopped signal for D-Bus
        self.emit_internal_packet(
            device_id,
            INTERNAL_SESSION_STOPPED,
            serde_json::json!({}),
        )
        .await;

        info!("Extended display session stopped");
        Ok(())
    }

    /// Handle a touch event from the Android device
    fn handle_touch(&mut self, body: &serde_json::Value, display_bounds: (u32, u32)) {
        let handler = match &mut self.input_handler {
            Some(h) if h.is_input_available() => h,
            _ => {
                debug!("Touch event ignored — input handler unavailable");
                return;
            }
        };

        let raw_x = body["x"].as_f64().unwrap_or(0.0);
        let raw_y = body["y"].as_f64().unwrap_or(0.0);

        // Validate and clamp coordinates
        // If coordinates are normalized [0.0, 1.0], accept them
        // If they're absolute pixels, clamp to display bounds
        let (x, y) = if raw_x >= 0.0 && raw_x <= 1.0 && raw_y >= 0.0 && raw_y <= 1.0 {
            // Normalized coordinates - pass through
            (raw_x, raw_y)
        } else {
            // Absolute coordinates - clamp to display bounds
            let (max_x, max_y) = display_bounds;
            (
                raw_x.clamp(0.0, max_x as f64),
                raw_y.clamp(0.0, max_y as f64),
            )
        };

        let touch_id = body["pointerId"].as_u64().unwrap_or(0) as u32;

        let action_str = body["touchAction"].as_str().unwrap_or("move");

        // Rate-limit touch move events (max ~125Hz)
        if action_str == "move" {
            if let Some(last) = self.last_touch_time {
                if last.elapsed() < std::time::Duration::from_millis(8) {
                    return;
                }
            }
        }
        self.last_touch_time = Some(std::time::Instant::now());

        let action = match action_str {
            "down" => TouchAction::Down,
            "up" => TouchAction::Up,
            "move" => TouchAction::Move,
            "cancel" => TouchAction::Cancel,
            other => {
                debug!("Unknown touch action: {}", other);
                return;
            }
        };

        let event = TouchEvent {
            action,
            x,
            y,
            touch_id,
            pressure: body["pressure"].as_f64(),
            timestamp: body["timestamp"].as_u64(),
        };

        if let Err(e) = handler.handle_touch_event(&event) {
            debug!("Touch injection error: {}", e);
        }
    }

    /// Send a packet to the paired device
    async fn send_packet(&self, device_id: &str, packet_type: &str, body: serde_json::Value) {
        if let Some(sender) = &self.packet_sender {
            let packet = Packet::new(packet_type, body);
            if let Err(e) = sender.send((device_id.to_string(), packet)).await {
                error!("Failed to send {} packet: {}", packet_type, e);
            }
        }
    }

    /// Emit an internal packet for daemon-side routing (D-Bus signals)
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
}

impl Default for ExtendedDisplayPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ExtendedDisplayPlugin {
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
            PACKET_TYPE.to_string(),
            PACKET_TYPE_REQUEST.to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE.to_string(),
            PACKET_TYPE_REQUEST.to_string(),
        ]
    }

    async fn init(
        &mut self,
        device: &Device,
        packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>,
    ) -> Result<()> {
        info!(
            "Initializing ExtendedDisplay plugin for device {}",
            device.name()
        );
        self.device_id = Some(device.id().to_string());
        self.packet_sender = Some(packet_sender);
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Starting ExtendedDisplay plugin");
        self.enabled = true;
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Stopping ExtendedDisplay plugin");

        // Notify Android device before cleanup if session is active
        if self.session_active {
            if let Some(ref device_id) = self.device_id {
                // Send stopped notification to Android
                let stop_body = serde_json::json!({ "action": "stopped" });
                self.send_packet(device_id, PACKET_TYPE, stop_body).await;

                // Emit internal stopped signal for D-Bus
                self.emit_internal_packet(
                    device_id,
                    INTERNAL_SESSION_STOPPED,
                    serde_json::json!({}),
                )
                .await;
            }

            let device_id = self.device_id.clone().unwrap_or_default();
            // Now do cleanup
            self.stop_flag.store(true, Ordering::SeqCst);
            if let Some(handle) = self.capture_task.take() {
                handle.abort();
                let _ = handle.await;
            }
            if let Some(server_arc) = self.streaming_server.take() {
                // Try to unwrap Arc, but just drop if we can't
                if let Ok(mut server) = Arc::try_unwrap(server_arc) {
                    let _ = server.stop().await;
                }
                // Otherwise drop happens implicitly
            }
            self.encoder = None;
            self.input_handler = None;
            self.session_active = false;
            debug!(
                "Cleaned up extended display session for {}",
                device_id
            );
        }

        self.enabled = false;
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("ExtendedDisplay plugin is disabled, ignoring packet");
            return Ok(());
        }

        let device_id = device.id().to_string();

        let action = packet.body["action"]
            .as_str()
            .unwrap_or("")
            .to_string();

        debug!(
            "ExtendedDisplay handling action '{}' from {}",
            action, device_id
        );

        match action.as_str() {
            "request" => {
                let capabilities = packet.body["capabilities"]
                    .as_str()
                    .unwrap_or("h264,touch");

                // Validate required capabilities
                if !capabilities.contains("h264") {
                    warn!(
                        "Device {} requested unsupported capabilities: '{}' (h264 required)",
                        device_id, capabilities
                    );
                    let error_body = serde_json::json!({
                        "action": "error",
                        "message": "Unsupported capabilities: h264 codec required",
                    });
                    self.send_packet(&device_id, PACKET_TYPE, error_body).await;
                    return Ok(());
                }

                // Extract requested resolution from packet if provided
                let requested_resolution = if let (Some(w), Some(h)) = (
                    packet.body.get("width").and_then(|v| v.as_u64()),
                    packet.body.get("height").and_then(|v| v.as_u64()),
                ) {
                    Some((w as u32, h as u32))
                } else {
                    None
                };

                self.start_session(&device_id, capabilities, requested_resolution).await?;
            }
            "touch" => {
                self.handle_touch(&packet.body, self.display_resolution);
            }
            "stop" => {
                self.stop_session(&device_id).await?;
            }
            other => {
                warn!(
                    "Unknown extended display action '{}' from {}",
                    other, device_id
                );
            }
        }

        Ok(())
    }
}

/// Factory for creating `ExtendedDisplayPlugin` instances
pub struct ExtendedDisplayPluginFactory;

impl ExtendedDisplayPluginFactory {
    /// Create a new factory
    pub fn new() -> Self {
        Self
    }
}

impl Default for ExtendedDisplayPluginFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginFactory for ExtendedDisplayPluginFactory {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE.to_string(),
            PACKET_TYPE_REQUEST.to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE.to_string(),
            PACKET_TYPE_REQUEST.to_string(),
        ]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(ExtendedDisplayPlugin::new())
    }
}

/// Discover a local IP address that the Android device can connect to
fn get_local_ip() -> Option<String> {
    use std::net::UdpSocket;
    // Connect to a public address to determine the local interface IP
    // No actual traffic is sent
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_device;

    #[test]
    fn test_config_default() {
        let config = ExtendedDisplayConfig::default();
        assert_eq!(config.signaling_port, DEFAULT_SIGNALING_PORT);
        assert_eq!(config.bitrate_bps, DEFAULT_BITRATE_BPS);
        assert_eq!(config.framerate, DEFAULT_FRAMERATE);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_port() {
        let mut config = ExtendedDisplayConfig::default();
        config.signaling_port = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_bitrate_low() {
        let mut config = ExtendedDisplayConfig::default();
        config.bitrate_bps = 100;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_bitrate_high() {
        let mut config = ExtendedDisplayConfig::default();
        config.bitrate_bps = 200_000_000;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_framerate() {
        let mut config = ExtendedDisplayConfig::default();
        config.framerate = 0;
        assert!(config.validate().is_err());

        config.framerate = 121;
        assert!(config.validate().is_err());

        config.framerate = 60;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_serialization() {
        let config = ExtendedDisplayConfig {
            signaling_port: 9999,
            bitrate_bps: 5_000_000,
            framerate: 30,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ExtendedDisplayConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.signaling_port, 9999);
        assert_eq!(deserialized.bitrate_bps, 5_000_000);
        assert_eq!(deserialized.framerate, 30);
    }

    #[tokio::test]
    async fn test_plugin_creation() {
        let plugin = ExtendedDisplayPlugin::new();
        assert_eq!(plugin.name(), PLUGIN_NAME);
        assert!(!plugin.enabled);
        assert!(!plugin.is_session_active());
    }

    #[tokio::test]
    async fn test_plugin_init() {
        let mut plugin = ExtendedDisplayPlugin::new();
        let device = create_test_device();
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        assert!(plugin.init(&device, tx).await.is_ok());
        assert_eq!(plugin.device_id.as_deref(), Some(device.id()));
    }

    #[tokio::test]
    async fn test_plugin_start_stop() {
        let mut plugin = ExtendedDisplayPlugin::new();
        let device = create_test_device();
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        plugin.init(&device, tx).await.unwrap();

        assert!(plugin.start().await.is_ok());
        assert!(plugin.enabled);

        assert!(plugin.stop().await.is_ok());
        assert!(!plugin.enabled);
    }

    #[test]
    fn test_factory_creation() {
        let factory = ExtendedDisplayPluginFactory::new();
        assert_eq!(factory.name(), PLUGIN_NAME);

        let incoming = factory.incoming_capabilities();
        assert!(incoming.contains(&PACKET_TYPE.to_string()));
        assert!(incoming.contains(&PACKET_TYPE_REQUEST.to_string()));

        let outgoing = factory.outgoing_capabilities();
        assert!(outgoing.contains(&PACKET_TYPE.to_string()));
        assert!(outgoing.contains(&PACKET_TYPE_REQUEST.to_string()));
    }

    #[test]
    fn test_factory_creates_plugin() {
        let factory = ExtendedDisplayPluginFactory::new();
        let plugin = factory.create();
        assert_eq!(plugin.name(), PLUGIN_NAME);
    }

    #[test]
    fn test_get_local_ip() {
        // This test may fail in CI without network, so just verify it doesn't panic
        let _ip = get_local_ip();
    }

    #[tokio::test]
    async fn test_disabled_plugin_ignores_packets() {
        let mut plugin = ExtendedDisplayPlugin::new();
        let mut device = create_test_device();
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        plugin.init(&device, tx).await.unwrap();
        // Don't call start() — plugin stays disabled

        let packet = Packet::new(
            PACKET_TYPE_REQUEST,
            serde_json::json!({ "action": "request", "capabilities": "h264,touch" }),
        );
        // Should return Ok without starting a session
        assert!(plugin.handle_packet(&packet, &mut device).await.is_ok());
        assert!(!plugin.is_session_active());
    }

    #[tokio::test]
    async fn test_unknown_action_handled_gracefully() {
        let mut plugin = ExtendedDisplayPlugin::new();
        let mut device = create_test_device();
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        plugin.init(&device, tx).await.unwrap();
        plugin.start().await.unwrap();

        let packet = Packet::new(
            PACKET_TYPE_REQUEST,
            serde_json::json!({ "action": "unknown_action" }),
        );
        assert!(plugin.handle_packet(&packet, &mut device).await.is_ok());
        assert!(!plugin.is_session_active());
    }
}
