//! Camera Plugin
//!
//! Enables remote camera access and control between devices.
//!
//! ## Protocol Specification
//!
//! This plugin implements remote camera access for various use cases such as using
//! a phone as a webcam, security monitoring, or capturing images remotely.
//!
//! ### Packet Types
//!
//! - `cconnect.camera.request` - Request camera list, start/stop camera
//! - `cconnect.camera` - Camera frame data and metadata
//!
//! ### Capabilities
//!
//! - Incoming: `cconnect.camera` - Can receive camera frames
//! - Outgoing: `cconnect.camera.request` - Can request camera control
//!
//! ### Use Cases
//!
//! - Use phone as webcam for desktop
//! - Remote security monitoring
//! - Capture photos remotely
//! - Video conferencing with mobile camera
//!
//! ## Packet Formats
//!
//! ### Camera Request (`cconnect.camera.request`)
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.camera.request",
//!     "body": {
//!         "command": "list"
//!     }
//! }
//! ```
//!
//! **Commands**:
//! - `list` - Request available cameras
//! - `start` - Start camera stream
//! - `stop` - Stop camera stream
//!
//! ### Start Camera Request
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.camera.request",
//!     "body": {
//!         "command": "start",
//!         "cameraId": "camera-0",
//!         "resolution": "1280x720",
//!         "fps": 30,
//!         "quality": "medium"
//!     }
//! }
//! ```
//!
//! ### Camera Frame (`cconnect.camera`)
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.camera",
//!     "body": {
//!         "cameraId": "camera-0",
//!         "timestamp": 1704067200000,
//!         "frameNumber": 42,
//!         "format": "jpeg",
//!         "width": 1280,
//!         "height": 720
//!     },
//!     "payloadSize": 65536,
//!     "payloadTransferInfo": {
//!         "port": 1739
//!     }
//! }
//! ```
//!
//! ## Features
//!
//! - **Camera Discovery**: List available cameras on device
//! - **Resolution Control**: Configure camera resolution
//! - **Frame Rate Control**: Adjust FPS for bandwidth optimization
//! - **Quality Settings**: Balance quality vs bandwidth
//! - **Session Management**: Start/stop camera streams
//! - **Format Support**: JPEG, H.264 (future: VP8, VP9)
//!
//! ## Implementation Status
//!
//! - [ ] Camera enumeration
//! - [x] V4L2 integration (Linux) - via cosmic-connect-core/video
//! - [x] Video decoding (H.264 via OpenH264)
//! - [x] Frame reception
//! - [x] Session management
//! - [ ] Quality control
//!

use crate::plugins::{Plugin, PluginFactory};
use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::any::Any;
use std::sync::Arc;
use tokio::sync::{mpsc::Sender, Mutex};
#[cfg(feature = "video")]
use tracing::error;
use tracing::{debug, info, warn};

#[cfg(feature = "video")]
use cosmic_connect_core::plugins::camera::{CameraFrame as CoreCameraFrame, FrameType};
#[cfg(feature = "video")]
use cosmic_connect_core::video::{CameraDaemon, CameraDaemonConfig, PixelFormat};

const PLUGIN_NAME: &str = "camera";
const INCOMING_CAPABILITY: &str = "cconnect.camera";
const OUTGOING_CAPABILITY: &str = "cconnect.camera.request";

// Camera packet type constants
const CAMERA_FRAME: &str = "cconnect.camera";
const CAMERA_REQUEST: &str = "cconnect.camera.request";

/// Camera information
///
/// Describes an available camera on the device.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CameraInfo {
    /// Unique camera identifier
    pub id: String,

    /// Human-readable camera name
    pub name: String,

    /// Supported resolutions (e.g., "1920x1080")
    pub resolutions: Vec<String>,

    /// Camera facing direction
    pub facing: CameraFacing,

    /// Maximum supported FPS
    pub max_fps: u8,
}

impl CameraInfo {
    /// Create a new camera info
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        resolutions: Vec<String>,
        facing: CameraFacing,
        max_fps: u8,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            resolutions,
            facing,
            max_fps,
        }
    }
}

/// Camera facing direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CameraFacing {
    /// Front-facing camera (selfie)
    Front,
    /// Back-facing camera (main)
    Back,
    /// External camera
    External,
}

impl Default for CameraFacing {
    fn default() -> Self {
        Self::Back
    }
}

impl CameraFacing {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::Back => "back",
            Self::External => "external",
        }
    }
}

/// Camera session state
///
/// Tracks an active camera streaming session.
#[derive(Debug, Clone)]
pub struct CameraSession {
    /// Camera being used
    pub camera_id: String,

    /// Current resolution
    pub resolution: String,

    /// Frame rate
    pub fps: u8,

    /// Quality setting
    pub quality: CameraQuality,

    /// Frame counter
    pub frame_number: u64,

    /// Session start timestamp
    pub started_at: u64,
}

impl CameraSession {
    /// Create a new camera session
    pub fn new(
        camera_id: impl Into<String>,
        resolution: impl Into<String>,
        fps: u8,
        quality: CameraQuality,
    ) -> Self {
        Self {
            camera_id: camera_id.into(),
            resolution: resolution.into(),
            fps,
            quality,
            frame_number: 0,
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }
    }

    /// Increment frame counter
    pub fn next_frame(&mut self) -> u64 {
        self.frame_number += 1;
        self.frame_number
    }
}

/// Camera quality settings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CameraQuality {
    /// Low quality (high compression)
    Low,
    /// Medium quality (balanced)
    Medium,
    /// High quality (low compression)
    High,
}

impl Default for CameraQuality {
    fn default() -> Self {
        Self::Medium
    }
}

impl CameraQuality {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Get JPEG quality percentage (0-100)
    pub fn jpeg_quality(&self) -> u8 {
        match self {
            Self::Low => 60,
            Self::Medium => 80,
            Self::High => 95,
        }
    }
}

/// Camera configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraConfig {
    /// Requested camera ID
    pub camera_id: String,

    /// Requested resolution
    pub resolution: String,

    /// Requested frame rate
    pub fps: u8,

    /// Quality setting
    pub quality: CameraQuality,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            camera_id: "camera-0".to_string(),
            resolution: "1280x720".to_string(),
            fps: 30,
            quality: CameraQuality::Medium,
        }
    }
}

/// Camera plugin for remote camera access
///
/// Handles camera streaming between devices.
pub struct CameraPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Active camera session
    session: Arc<Mutex<Option<CameraSession>>>,

    /// Available cameras (cached)
    cameras: Arc<Mutex<Vec<CameraInfo>>>,

    /// Packet sender for proactive packets
    packet_sender: Option<Sender<(String, Packet)>>,

    /// Camera daemon for V4L2 output (desktop receiving frames from Android)
    #[cfg(feature = "video")]
    camera_daemon: Arc<Mutex<Option<CameraDaemon>>>,
}

impl CameraPlugin {
    /// Create a new camera plugin
    pub fn new() -> Self {
        Self {
            device_id: None,
            session: Arc::new(Mutex::new(None)),
            cameras: Arc::new(Mutex::new(Vec::new())),
            packet_sender: None,
            #[cfg(feature = "video")]
            camera_daemon: Arc::new(Mutex::new(None)),
        }
    }

    /// Get active session info
    pub async fn get_session(&self) -> Option<CameraSession> {
        self.session.lock().await.clone()
    }

    /// Get available cameras
    pub async fn get_cameras(&self) -> Vec<CameraInfo> {
        self.cameras.lock().await.clone()
    }

    /// Check if camera is currently streaming
    pub async fn is_streaming(&self) -> bool {
        self.session.lock().await.is_some()
    }

    /// Process camera frame with payload data (public API for daemon integration)
    ///
    /// This method should be called by the connection layer when payload data
    /// for a camera frame packet is received.
    #[cfg(feature = "video")]
    pub async fn process_camera_frame_payload(
        &self,
        packet: &Packet,
        payload: Vec<u8>,
    ) -> Result<()> {
        self.handle_camera_frame_with_payload(packet, payload).await
    }

    /// Handle camera list request
    async fn handle_list_request(&self, _packet: &Packet, device: &Device) {
        info!("Received camera list request from {}", device.name());

        // TODO: Enumerate available cameras
        // This would integrate with V4L2 on Linux or platform-specific APIs

        debug!("Camera list request handling not yet implemented");
    }

    /// Handle start camera request
    async fn handle_start_request(&self, packet: &Packet, device: &Device) {
        info!("Received camera start request from {}", device.name());

        // Extract camera configuration from packet body
        let camera_id = packet
            .body
            .get("cameraId")
            .and_then(|v| v.as_str())
            .unwrap_or("camera-0");

        let resolution = packet
            .body
            .get("resolution")
            .and_then(|v| v.as_str())
            .unwrap_or("1280x720");

        let fps = packet
            .body
            .get("fps")
            .and_then(|v| v.as_u64())
            .unwrap_or(30) as u8;

        let quality = packet
            .body
            .get("quality")
            .and_then(|v| v.as_str())
            .and_then(|s| match s {
                "low" => Some(CameraQuality::Low),
                "medium" => Some(CameraQuality::Medium),
                "high" => Some(CameraQuality::High),
                _ => None,
            })
            .unwrap_or(CameraQuality::Medium);

        info!(
            "Starting camera: {} at {} with {} quality",
            camera_id,
            resolution,
            quality.as_str()
        );

        // Parse resolution (format: "1280x720")
        let (_width, _height) = resolution
            .split_once('x')
            .and_then(|(w, h)| {
                let width = w.parse::<u32>().ok()?;
                let height = h.parse::<u32>().ok()?;
                Some((width, height))
            })
            .unwrap_or((1280, 720));

        // Create session
        let session = CameraSession::new(camera_id, resolution, fps, quality);

        // Store session
        *self.session.lock().await = Some(session);

        // Start camera daemon for V4L2 output (if video feature enabled)
        #[cfg(feature = "video")]
        {
            let config = CameraDaemonConfig {
                device_path: std::path::PathBuf::from("/dev/video10"),
                width,
                height,
                fps: fps as u32,
                output_format: PixelFormat::YUYV,
                queue_size: 5,
                enable_perf_monitoring: true,
            };

            let mut daemon = CameraDaemon::new(config);
            if let Err(e) = daemon.start().await {
                error!("Failed to start camera daemon: {}", e);
                return;
            }

            info!("Camera daemon started successfully");
            *self.camera_daemon.lock().await = Some(daemon);
        }

        #[cfg(not(feature = "video"))]
        {
            debug!("Camera start handling not available (video feature disabled)");
        }
    }

    /// Handle stop camera request
    async fn handle_stop_request(&self, _packet: &Packet, device: &Device) {
        info!("Received camera stop request from {}", device.name());

        // Stop camera daemon if running
        #[cfg(feature = "video")]
        {
            let mut daemon_lock = self.camera_daemon.lock().await;
            if let Some(daemon) = daemon_lock.as_mut() {
                if let Err(e) = daemon.stop().await {
                    error!("Failed to stop camera daemon: {}", e);
                } else {
                    info!("Camera daemon stopped successfully");
                }
            }
            *daemon_lock = None;
        }

        // Clear session
        *self.session.lock().await = None;

        info!("Camera stopped");
    }

    /// Handle incoming camera frame
    async fn handle_camera_frame(&self, _packet: &Packet, device: &Device) -> Result<()> {
        debug!("Received camera frame from {}", device.name());

        #[cfg(feature = "video")]
        {
            // Extract frame metadata from packet body
            let frame = CoreCameraFrame::from_packet(packet).map_err(|e| {
                warn!("Failed to parse camera frame packet: {}", e);
                e
            })?;

            debug!(
                "Camera frame: type={:?}, seq={}, size={}, timestamp={}us",
                frame.frame_type, frame.sequence_number, frame.size, frame.timestamp_us
            );

            // Check if we have payload data
            if let Some(payload_size) = packet.payload_size {
                if payload_size == 0 {
                    warn!("Camera frame packet has zero payload size");
                    return Ok(());
                }

                // Receive payload data
                // Note: The actual payload reception would happen through the connection layer
                // For now, we log that we're ready to receive
                debug!(
                    "Ready to receive camera frame payload: {} bytes",
                    payload_size
                );

                // TODO: Integrate with connection layer to receive payload
                // The payload data should be passed to handle_camera_frame_with_payload
            } else {
                warn!("Camera frame packet missing payload size");
            }
        }

        #[cfg(not(feature = "video"))]
        {
            debug!("Camera frame handling not available (video feature disabled)");
        }

        Ok(())
    }

    /// Handle incoming camera frame with payload data
    #[cfg(feature = "video")]
    async fn handle_camera_frame_with_payload(
        &self,
        packet: &Packet,
        payload: Vec<u8>,
    ) -> Result<()> {
        // Parse frame metadata
        let frame = CoreCameraFrame::from_packet(packet)?;

        debug!(
            "Processing camera frame: type={:?}, seq={}, size={}",
            frame.frame_type, frame.sequence_number, frame.size
        );

        // Forward frame to camera daemon for decoding and V4L2 output
        let daemon_lock = self.camera_daemon.lock().await;
        if let Some(daemon) = daemon_lock.as_ref() {
            // Process the frame through the daemon
            daemon
                .process_frame(payload, frame.frame_type, frame.timestamp_us)
                .await
                .map_err(|e| {
                    error!("Failed to process camera frame: {}", e);
                    crate::ProtocolError::Other(format!("Camera frame processing failed: {}", e))
                })?;

            debug!("Camera frame processed successfully");
        } else {
            warn!("Camera daemon not running, dropping frame");
        }

        Ok(())
    }
}

impl Default for CameraPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for CameraPlugin {
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
            INCOMING_CAPABILITY.to_string(),
            CAMERA_FRAME.to_string(),
            "kdeconnect.camera".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![OUTGOING_CAPABILITY.to_string(), CAMERA_REQUEST.to_string()]
    }

    async fn init(
        &mut self,
        device: &Device,
        packet_sender: Sender<(String, Packet)>,
    ) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        self.packet_sender = Some(packet_sender);
        info!("Camera plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Camera plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Camera plugin stopping");

        // Stop camera daemon if running
        #[cfg(feature = "video")]
        {
            let mut daemon_lock = self.camera_daemon.lock().await;
            if let Some(daemon) = daemon_lock.as_mut() {
                if let Err(e) = daemon.stop().await {
                    error!("Failed to stop camera daemon during plugin stop: {}", e);
                } else {
                    info!("Camera daemon stopped");
                }
            }
            *daemon_lock = None;
        }

        // Clean up any active session
        *self.session.lock().await = None;

        info!("Camera plugin stopped");
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if packet.is_type(CAMERA_REQUEST) {
            // Parse command from packet body
            if let Some(command) = packet.body.get("command").and_then(|v| v.as_str()) {
                match command {
                    "list" => self.handle_list_request(packet, device).await,
                    "start" => self.handle_start_request(packet, device).await,
                    "stop" => self.handle_stop_request(packet, device).await,
                    _ => {
                        warn!("Unknown camera command: {}", command);
                    }
                }
            }
        } else if packet.is_type(CAMERA_FRAME) || packet.is_type("kdeconnect.camera") {
            self.handle_camera_frame(packet, device).await?;
        }

        Ok(())
    }
}

/// Factory for creating CameraPlugin instances
#[derive(Debug, Clone, Copy)]
pub struct CameraPluginFactory;

impl PluginFactory for CameraPluginFactory {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            INCOMING_CAPABILITY.to_string(),
            CAMERA_FRAME.to_string(),
            "kdeconnect.camera".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![OUTGOING_CAPABILITY.to_string(), CAMERA_REQUEST.to_string()]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(CameraPlugin::new())
    }
}

/// Create a camera list request packet
///
/// Requests the list of available cameras from a device.
///
/// # Example
///
/// ```rust,ignore
/// use cosmic_connect_core::plugins::camera::create_camera_list_request;
///
/// let packet = create_camera_list_request();
/// // Send packet to device...
/// ```
pub fn create_camera_list_request() -> Packet {
    let body = json!({
        "command": "list"
    });

    Packet::new(CAMERA_REQUEST, body)
}

/// Create a start camera request packet
///
/// Requests to start a camera stream with specified configuration.
///
/// # Parameters
///
/// - `camera_id`: Camera identifier to start
/// - `resolution`: Desired resolution (e.g., "1280x720")
/// - `fps`: Desired frame rate
/// - `quality`: Quality setting
///
/// # Example
///
/// ```rust,ignore
/// use cosmic_connect_core::plugins::camera::{create_camera_start_request, CameraQuality};
///
/// let packet = create_camera_start_request(
///     "camera-0",
///     "1280x720",
///     30,
///     CameraQuality::Medium
/// );
/// // Send packet to device...
/// ```
pub fn create_camera_start_request(
    camera_id: &str,
    resolution: &str,
    fps: u8,
    quality: CameraQuality,
) -> Packet {
    let body = json!({
        "command": "start",
        "cameraId": camera_id,
        "resolution": resolution,
        "fps": fps,
        "quality": quality.as_str()
    });

    Packet::new(CAMERA_REQUEST, body)
}

/// Create a stop camera request packet
///
/// Requests to stop the active camera stream.
///
/// # Example
///
/// ```rust,ignore
/// use cosmic_connect_core::plugins::camera::create_camera_stop_request;
///
/// let packet = create_camera_stop_request();
/// // Send packet to device...
/// ```
pub fn create_camera_stop_request() -> Packet {
    let body = json!({
        "command": "stop"
    });

    Packet::new(CAMERA_REQUEST, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceInfo, DeviceType};

    fn create_test_device() -> Device {
        let info = DeviceInfo::new("Test Device", DeviceType::Phone, 1716);
        Device::from_discovery(info)
    }

    #[test]
    fn test_camera_info_creation() {
        let info = CameraInfo::new(
            "camera-0",
            "Back Camera",
            vec!["1920x1080".to_string(), "1280x720".to_string()],
            CameraFacing::Back,
            60,
        );

        assert_eq!(info.id, "camera-0");
        assert_eq!(info.name, "Back Camera");
        assert_eq!(info.resolutions.len(), 2);
        assert_eq!(info.facing, CameraFacing::Back);
        assert_eq!(info.max_fps, 60);
    }

    #[test]
    fn test_camera_quality() {
        assert_eq!(CameraQuality::Low.jpeg_quality(), 60);
        assert_eq!(CameraQuality::Medium.jpeg_quality(), 80);
        assert_eq!(CameraQuality::High.jpeg_quality(), 95);
    }

    #[test]
    fn test_plugin_creation() {
        let plugin = CameraPlugin::new();
        assert_eq!(plugin.name(), "camera");
    }

    #[test]
    fn test_capabilities() {
        let plugin = CameraPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert!(incoming.contains(&"cconnect.camera".to_string()));
        assert!(incoming.contains(&"kdeconnect.camera".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert!(outgoing.contains(&"cconnect.camera.request".to_string()));
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = CameraPlugin::new();
        let device = create_test_device();
        let (tx, _rx) = tokio::sync::mpsc::channel(100);

        plugin.init(&device, tx).await.unwrap();
        assert!(plugin.device_id.is_some());

        plugin.start().await.unwrap();
        assert!(!plugin.is_streaming().await);

        plugin.stop().await.unwrap();
    }

    #[test]
    fn test_create_list_request() {
        let packet = create_camera_list_request();
        assert_eq!(packet.packet_type, "cconnect.camera.request");
        assert_eq!(
            packet.body.get("command").and_then(|v| v.as_str()),
            Some("list")
        );
    }

    #[test]
    fn test_create_start_request() {
        let packet = create_camera_start_request("camera-0", "1280x720", 30, CameraQuality::Medium);

        assert_eq!(packet.packet_type, "cconnect.camera.request");
        assert_eq!(
            packet.body.get("command").and_then(|v| v.as_str()),
            Some("start")
        );
        assert_eq!(
            packet.body.get("cameraId").and_then(|v| v.as_str()),
            Some("camera-0")
        );
        assert_eq!(
            packet.body.get("resolution").and_then(|v| v.as_str()),
            Some("1280x720")
        );
        assert_eq!(packet.body.get("fps").and_then(|v| v.as_u64()), Some(30));
    }

    #[test]
    fn test_create_stop_request() {
        let packet = create_camera_stop_request();
        assert_eq!(packet.packet_type, "cconnect.camera.request");
        assert_eq!(
            packet.body.get("command").and_then(|v| v.as_str()),
            Some("stop")
        );
    }

    #[tokio::test]
    async fn test_session_management() {
        let plugin = CameraPlugin::new();

        // Initially no session
        assert!(!plugin.is_streaming().await);
        assert!(plugin.get_session().await.is_none());

        // Create a session
        let session = CameraSession::new("camera-0", "1280x720", 30, CameraQuality::Medium);
        *plugin.session.lock().await = Some(session);

        // Now streaming
        assert!(plugin.is_streaming().await);
        assert!(plugin.get_session().await.is_some());
    }

    #[tokio::test]
    async fn test_handle_start_request() {
        let mut plugin = CameraPlugin::new();
        let device = create_test_device();
        let (tx, _rx) = tokio::sync::mpsc::channel(100);

        plugin.init(&device, tx).await.unwrap();
        plugin.start().await.unwrap();

        let mut device = create_test_device();
        let packet = create_camera_start_request("camera-0", "1920x1080", 30, CameraQuality::High);

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        // Session should be created
        assert!(plugin.is_streaming().await);
        let session = plugin.get_session().await.unwrap();
        assert_eq!(session.camera_id, "camera-0");
        assert_eq!(session.resolution, "1920x1080");
        assert_eq!(session.fps, 30);
    }

    #[tokio::test]
    async fn test_handle_stop_request() {
        let mut plugin = CameraPlugin::new();
        let device = create_test_device();
        let (tx, _rx) = tokio::sync::mpsc::channel(100);

        plugin.init(&device, tx).await.unwrap();

        // Start a session first
        let session = CameraSession::new("camera-0", "1280x720", 30, CameraQuality::Medium);
        *plugin.session.lock().await = Some(session);
        assert!(plugin.is_streaming().await);

        // Stop the session
        let mut device = create_test_device();
        let packet = create_camera_stop_request();
        plugin.handle_packet(&packet, &mut device).await.unwrap();

        // Session should be cleared
        assert!(!plugin.is_streaming().await);
    }
}
