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
    pub async fn start_session(&mut self, device_id: &str, capabilities: &str) -> Result<()> {
        if self.session_active {
            warn!("Extended display session already active, stopping first");
            self.stop_session(device_id).await?;
        }

        info!(
            "Starting extended display session for device {} (capabilities: {})",
            device_id, capabilities
        );

        self.stop_flag.store(false, Ordering::SeqCst);

        // Detect encoder (None = auto-detect best available)
        let display_width = 1920u32;
        let display_height = 1080u32;
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

        let encoder = match VideoEncoder::new(encoder_config) {
            Ok(enc) => {
                info!("Video encoder initialized: {:?}", enc.encoder_type());
                enc
            }
            Err(e) => {
                error!("Failed to create video encoder: {}", e);
                self.emit_internal_packet(
                    device_id,
                    "cconnect.internal.extendeddisplay.error",
                    serde_json::json!({ "error": format!("Encoder init failed: {}", e) }),
                )
                .await;
                return Err(ProtocolError::Plugin(format!("Encoder init failed: {}", e)));
            }
        };

        // Create streaming server
        let stream_config = StreamConfig {
            signaling_port: self.config.signaling_port,
            max_clients: 1,
            ..StreamConfig::default()
        };

        let mut server = match StreamingServer::new(stream_config) {
            Ok(s) => {
                info!(
                    "WebRTC streaming server ready on port {}",
                    self.config.signaling_port
                );
                s
            }
            Err(e) => {
                error!("Failed to create streaming server: {}", e);
                self.emit_internal_packet(
                    device_id,
                    "cconnect.internal.extendeddisplay.error",
                    serde_json::json!({ "error": format!("Server init failed: {}", e) }),
                )
                .await;
                return Err(ProtocolError::Plugin(format!(
                    "Streaming server init failed: {}",
                    e
                )));
            }
        };

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

        // Store state
        self.streaming_server = Some(server_arc);
        self.input_handler = Some(input_handler);
        self.capture_task = Some(capture_task);
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
            "cconnect.internal.extendeddisplay.started",
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

        // Signal background tasks to stop
        self.stop_flag.store(true, Ordering::SeqCst);

        // Stop capture task first (this will release the server Arc clone)
        if let Some(handle) = self.capture_task.take() {
            handle.abort();
            let _ = handle.await;
        }

        // Stop streaming server - try to unwrap Arc to get exclusive ownership
        if let Some(server_arc) = self.streaming_server.take() {
            match Arc::try_unwrap(server_arc) {
                Ok(mut server) => {
                    if let Err(e) = server.stop().await {
                        warn!("Error stopping streaming server: {}", e);
                    }
                }
                Err(arc) => {
                    // Another reference still exists (shouldn't happen since task is aborted)
                    warn!(
                        "Could not unwrap streaming server Arc (ref count: {}), dropping instead",
                        Arc::strong_count(&arc)
                    );
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
            "cconnect.internal.extendeddisplay.stopped",
            serde_json::json!({}),
        )
        .await;

        info!("Extended display session stopped");
        Ok(())
    }

    /// Handle a touch event from the Android device
    fn handle_touch(&mut self, body: &serde_json::Value) {
        let handler = match &mut self.input_handler {
            Some(h) if h.is_input_available() => h,
            _ => {
                debug!("Touch event ignored — input handler unavailable");
                return;
            }
        };

        let x = body["x"].as_f64().unwrap_or(0.0);
        let y = body["y"].as_f64().unwrap_or(0.0);
        let touch_id = body["pointerId"].as_u64().unwrap_or(0) as u32;

        let action_str = body["touchAction"].as_str().unwrap_or("move");
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
        self.enabled = false;

        if self.session_active {
            let device_id = self.device_id.clone().unwrap_or_default();
            // Signal stop but don't send packets (plugin is shutting down)
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
                self.start_session(&device_id, capabilities).await?;
            }
            "touch" => {
                self.handle_touch(&packet.body);
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
