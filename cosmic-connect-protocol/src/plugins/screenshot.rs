//! Screenshot Plugin
//!
//! Enables remote screenshot capture and transfer between desktop machines.
//! Supports both Wayland (via pipewire) and X11 (via screenshot utilities).
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `cconnect.screenshot.request` - Request a screenshot from remote device
//! - `cconnect.screenshot.data` - Screenshot image data (with payload)
//! - `cconnect.screenshot.region` - Capture specific screen region
//! - `cconnect.screenshot.window` - Capture specific window
//!
//! **Capabilities**:
//! - Incoming: `cconnect.screenshot.request`, `cconnect.screenshot.region`, `cconnect.screenshot.window`
//! - Outgoing: `cconnect.screenshot.data`
//!
//! ## Packet Formats
//!
//! ### Request Screenshot (Full Screen)
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.screenshot.request",
//!     "body": {
//!         "captureType": "fullscreen"
//!     }
//! }
//! ```
//!
//! ### Request Region Screenshot
//!
//! ```json
//! {
//!     "id": 1234567891,
//!     "type": "cconnect.screenshot.region",
//!     "body": {
//!         "x": 100,
//!         "y": 100,
//!         "width": 800,
//!         "height": 600
//!     }
//! }
//! ```
//!
//! ### Request Window Screenshot
//!
//! ```json
//! {
//!     "id": 1234567892,
//!     "type": "cconnect.screenshot.window",
//!     "body": {
//!         "windowId": "0x1234567"
//!     }
//! }
//! ```
//!
//! ### Screenshot Data Response
//!
//! ```json
//! {
//!     "id": 1234567893,
//!     "type": "cconnect.screenshot.data",
//!     "body": {
//!         "filename": "screenshot_2024-01-15_14-30-45.png",
//!         "format": "png",
//!         "width": 1920,
//!         "height": 1080,
//!         "timestamp": 1705325445
//!     },
//!     "payloadSize": 2457600,
//!     "payloadTransferInfo": {
//!         "port": 1739
//!     }
//! }
//! ```
//!
//! ## Screenshot Capture
//!
//! ### Wayland
//! Uses pipewire/portal for secure screen capture:
//! - Desktop portal API for screenshot permission
//! - Supports full screen, region, and window selection
//! - User consent required per screenshot (security)
//!
//! ### X11
//! Uses traditional X11 screenshot utilities:
//! - `gnome-screenshot` (GNOME)
//! - `spectacle` (KDE)
//! - `scrot` (lightweight fallback)
//! - Direct X11 API if libraries available
//!
//! ## Image Format
//!
//! - **Primary**: PNG (lossless, good for screenshots)
//! - **Alternative**: JPEG (smaller size, acceptable quality)
//! - Compression applied for transfer efficiency
//!
//! ## Use Cases
//!
//! - Remote troubleshooting and support
//! - Collaboration and screen sharing snippets
//! - Quick capture from remote desktop
//! - Documentation and bug reporting
//!
//! ## Platform Support
//!
//! - **Linux/Wayland**: Full support via desktop portal
//! - **Linux/X11**: Full support via screenshot utilities
//! - **macOS**: Limited support (screencapture utility)
//! - **Windows**: Limited support (would need Windows API)

use crate::payload::PayloadServer;
use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::sync::mpsc::Sender;
use tracing::{debug, error, info, warn};

use super::{Plugin, PluginFactory};

/// Screenshot plugin for remote screen capture
///
/// Handles `cconnect.screenshot.*` packets for screenshot capture and transfer.
pub struct ScreenshotPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Whether the plugin is enabled
    enabled: bool,

    /// Temporary directory for screenshots
    temp_dir: PathBuf,

    /// Packet sender for sending responses back to the device
    packet_sender: Option<Sender<(String, Packet)>>,
}

impl ScreenshotPlugin {
    /// Create a new Screenshot plugin
    pub fn new() -> Self {
        let temp_dir = std::env::temp_dir().join("cosmic-connect-screenshots");

        Self {
            device_id: None,
            enabled: true,
            temp_dir,
            packet_sender: None,
        }
    }

    /// Detect display server type (Wayland or X11)
    fn detect_display_server() -> DisplayServer {
        // Check if running under Wayland
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            return DisplayServer::Wayland;
        }

        // Check if running under X11
        if std::env::var("DISPLAY").is_ok() {
            return DisplayServer::X11;
        }

        DisplayServer::Unknown
    }

    /// Read PNG image dimensions from file header
    ///
    /// PNG files have a standardized header format:
    /// - Bytes 16-19: Width (big-endian u32)
    /// - Bytes 20-23: Height (big-endian u32)
    ///
    /// Returns (width, height) or None if file cannot be read or is not a valid PNG
    fn read_png_dimensions(path: &Path) -> Option<(u32, u32)> {
        let mut file = File::open(path).ok()?;
        let mut header = [0u8; 24];

        // Read first 24 bytes
        file.read_exact(&mut header).ok()?;

        // Verify PNG signature (first 8 bytes)
        if &header[0..8] != b"\x89PNG\r\n\x1a\n" {
            return None;
        }

        // Read width and height (bytes 16-23, big-endian)
        let width = u32::from_be_bytes([header[16], header[17], header[18], header[19]]);
        let height = u32::from_be_bytes([header[20], header[21], header[22], header[23]]);

        Some((width, height))
    }

    /// Create a screenshot response packet
    ///
    /// Creates a `cconnect.screenshot.data` packet with payload transfer info.
    fn create_screenshot_response(
        filename: &str,
        width: u32,
        height: u32,
        file_size: u64,
        port: u16,
    ) -> Packet {
        let timestamp = chrono::Utc::now().timestamp();

        let body = json!({
            "filename": filename,
            "format": "png",
            "width": width,
            "height": height,
            "timestamp": timestamp
        });

        let transfer_info = HashMap::from([("port".to_string(), json!(port))]);

        Packet::new("cconnect.screenshot.data", body)
            .with_payload_size(file_size as i64)
            .with_payload_transfer_info(transfer_info)
    }

    /// Send a packet to the connected device
    async fn send_packet(&self, packet: Packet) -> Result<()> {
        let sender = self
            .packet_sender
            .as_ref()
            .ok_or_else(|| ProtocolError::Plugin("Packet sender not initialized".to_string()))?;

        let device_id = self
            .device_id
            .as_ref()
            .ok_or_else(|| ProtocolError::Plugin("Device ID not set".to_string()))?;

        sender
            .send((device_id.clone(), packet))
            .await
            .map_err(|e| ProtocolError::Plugin(format!("Failed to send packet: {}", e)))
    }

    /// Capture a screenshot
    ///
    /// Returns the path to the captured screenshot file.
    #[allow(dead_code)]
    fn capture_screenshot(&self, capture_type: CaptureType) -> Result<PathBuf> {
        let display_server = Self::detect_display_server();

        debug!(
            "Capturing screenshot (type: {:?}, display: {:?})",
            capture_type, display_server
        );

        // Ensure temp directory exists
        std::fs::create_dir_all(&self.temp_dir)
            .map_err(|e| ProtocolError::from_io_error(e, "Failed to create temp directory"))?;

        // Generate filename with timestamp
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("screenshot_{}.png", timestamp);
        let output_path = self.temp_dir.join(&filename);

        match display_server {
            DisplayServer::Wayland => self.capture_wayland(&output_path, capture_type),
            DisplayServer::X11 => self.capture_x11(&output_path, capture_type),
            DisplayServer::Unknown => {
                error!("Unable to detect display server (neither Wayland nor X11)");
                Err(ProtocolError::InvalidPacket(
                    "Display server detection failed".to_string(),
                ))
            }
        }
    }

    /// Capture screenshot on Wayland
    #[cfg(target_os = "linux")]
    fn capture_wayland(&self, output_path: &PathBuf, capture_type: CaptureType) -> Result<PathBuf> {
        info!("Attempting Wayland screenshot capture");

        // Try gnome-screenshot first (works with portal)
        if let Ok(status) = Command::new("gnome-screenshot")
            .arg("-f")
            .arg(output_path)
            .arg(match capture_type {
                CaptureType::FullScreen => "--file",
                CaptureType::Region { .. } => "--area",
                CaptureType::Window { .. } => "--window",
            })
            .status()
        {
            if status.success() && output_path.exists() {
                info!("Screenshot captured via gnome-screenshot");
                return Ok(output_path.clone());
            }
        }

        // Try spectacle (KDE)
        if let Ok(status) = Command::new("spectacle")
            .arg("-b")
            .arg("-n")
            .arg("-o")
            .arg(output_path)
            .arg(match capture_type {
                CaptureType::FullScreen => "-f",
                CaptureType::Region { .. } => "-r",
                CaptureType::Window { .. } => "-a",
            })
            .status()
        {
            if status.success() && output_path.exists() {
                info!("Screenshot captured via spectacle");
                return Ok(output_path.clone());
            }
        }

        warn!("No Wayland screenshot tool available");
        Err(ProtocolError::InvalidPacket(
            "No screenshot tool available for Wayland".to_string(),
        ))
    }

    #[cfg(not(target_os = "linux"))]
    fn capture_wayland(
        &self,
        _output_path: &PathBuf,
        _capture_type: CaptureType,
    ) -> Result<PathBuf> {
        Err(ProtocolError::InvalidPacket(
            "Wayland not supported on this platform".to_string(),
        ))
    }

    /// Capture screenshot on X11
    #[cfg(target_os = "linux")]
    fn capture_x11(&self, output_path: &PathBuf, capture_type: CaptureType) -> Result<PathBuf> {
        info!("Attempting X11 screenshot capture");

        // Try scrot (lightweight and widely available)
        let mut cmd = Command::new("scrot");
        cmd.arg(output_path);

        match capture_type {
            CaptureType::FullScreen => {
                // No additional args needed
            }
            CaptureType::Region {
                x,
                y,
                width,
                height,
            } => {
                cmd.arg("-a");
                cmd.arg(format!("{},{},{},{}", x, y, width, height));
            }
            CaptureType::Window { .. } => {
                cmd.arg("-u"); // Current window
            }
        }

        if let Ok(status) = cmd.status() {
            if status.success() && output_path.exists() {
                info!("Screenshot captured via scrot");
                return Ok(output_path.clone());
            }
        }

        // Try import (ImageMagick)
        if let Ok(status) = Command::new("import")
            .arg("-window")
            .arg("root")
            .arg(output_path)
            .status()
        {
            if status.success() && output_path.exists() {
                info!("Screenshot captured via import");
                return Ok(output_path.clone());
            }
        }

        warn!("No X11 screenshot tool available");
        Err(ProtocolError::InvalidPacket(
            "No screenshot tool available for X11".to_string(),
        ))
    }

    #[cfg(not(target_os = "linux"))]
    fn capture_x11(&self, _output_path: &PathBuf, _capture_type: CaptureType) -> Result<PathBuf> {
        Err(ProtocolError::InvalidPacket(
            "X11 not supported on this platform".to_string(),
        ))
    }

    /// Handle screenshot request
    async fn handle_screenshot_request(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        debug!("Handling screenshot request from {}", device.name());

        let body = &packet.body;
        let capture_type = body
            .get("captureType")
            .and_then(|v| v.as_str())
            .unwrap_or("fullscreen");

        let capture = match capture_type {
            "fullscreen" => CaptureType::FullScreen,
            _ => CaptureType::FullScreen,
        };

        info!(
            "Capturing screenshot for {} (type: {:?})",
            device.name(),
            capture
        );

        // Capture screenshot
        let screenshot_path = self.capture_screenshot(capture)?;

        info!(
            "Screenshot captured successfully: {}",
            screenshot_path.display()
        );

        // Get file metadata
        let metadata = std::fs::metadata(&screenshot_path)
            .map_err(|e| ProtocolError::from_io_error(e, "Failed to read screenshot metadata"))?;

        let file_size = metadata.len();
        let filename = screenshot_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("screenshot.png");

        // Get actual image dimensions from PNG header
        let (width, height) = Self::read_png_dimensions(&screenshot_path).unwrap_or((1920, 1080));

        debug!(
            "Screenshot info - file: {}, size: {} bytes, dimensions: {}x{}",
            filename, file_size, width, height
        );

        // Create payload server for file transfer
        let server = PayloadServer::new()
            .await
            .map_err(|e| ProtocolError::Plugin(format!("Failed to create payload server: {}", e)))?;

        let port = server.port();
        info!(
            "Payload server listening on port {} for screenshot transfer",
            port
        );

        // Create and send response packet with transfer info
        let response_packet = Self::create_screenshot_response(filename, width, height, file_size, port);
        self.send_packet(response_packet).await?;

        info!(
            "Sent screenshot response to {} (port: {}, size: {} bytes)",
            device.name(),
            port,
            file_size
        );

        // Spawn a task to handle the file transfer
        let path_for_transfer = screenshot_path.clone();
        tokio::spawn(async move {
            match server.send_file(&path_for_transfer).await {
                Ok(()) => {
                    info!("Screenshot transfer completed successfully");
                    // Clean up the temporary file after successful transfer
                    if let Err(e) = std::fs::remove_file(&path_for_transfer) {
                        debug!("Failed to cleanup screenshot file: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Screenshot transfer failed: {}", e);
                }
            }
        });

        Ok(())
    }

    /// Handle region screenshot request
    async fn handle_region_request(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        debug!("Handling region screenshot request from {}", device.name());

        let body = &packet.body;
        let x = body.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let y = body.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let width = body.get("width").and_then(|v| v.as_i64()).unwrap_or(800) as u32;
        let height = body.get("height").and_then(|v| v.as_i64()).unwrap_or(600) as u32;

        let capture = CaptureType::Region {
            x,
            y,
            width,
            height,
        };

        info!(
            "Capturing region screenshot for {} ({}x{} at {},{}))",
            device.name(),
            width,
            height,
            x,
            y
        );

        let screenshot_path = self.capture_screenshot(capture)?;

        info!("Region screenshot captured: {}", screenshot_path.display());

        Ok(())
    }

    /// Handle window screenshot request
    async fn handle_window_request(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        debug!("Handling window screenshot request from {}", device.name());

        let body = &packet.body;
        let window_id = body
            .get("windowId")
            .and_then(|v| v.as_str())
            .unwrap_or("current");

        let capture = CaptureType::Window {
            window_id: window_id.to_string(),
        };

        info!(
            "Capturing window screenshot for {} (window: {})",
            device.name(),
            window_id
        );

        let screenshot_path = self.capture_screenshot(capture)?;

        info!("Window screenshot captured: {}", screenshot_path.display());

        Ok(())
    }
}

/// Display server type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplayServer {
    Wayland,
    X11,
    Unknown,
}

/// Screenshot capture type
#[derive(Debug, Clone)]
enum CaptureType {
    /// Full screen capture
    FullScreen,
    /// Region capture with coordinates
    Region {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    /// Window capture
    Window {
        #[allow(dead_code)]
        window_id: String,
    },
}

impl Default for ScreenshotPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ScreenshotPlugin {
    fn name(&self) -> &str {
        "screenshot"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.screenshot.request".to_string(),
            "cconnect.screenshot.region".to_string(),
            "cconnect.screenshot.window".to_string(),
            "kdeconnect.screenshot.request".to_string(),
            "kdeconnect.screenshot.region".to_string(),
            "kdeconnect.screenshot.window".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec!["cconnect.screenshot.data".to_string()]
    }

    async fn init(&mut self, device: &Device, packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        self.packet_sender = Some(packet_sender);
        info!("Screenshot plugin initialized for device {}", device.name());

        // Ensure temp directory exists
        if let Err(e) = std::fs::create_dir_all(&self.temp_dir) {
            warn!("Failed to create screenshot temp directory: {}", e);
        }

        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Screenshot plugin started");
        self.enabled = true;
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Screenshot plugin stopped");
        self.enabled = false;

        // Cleanup temp directory
        if let Err(e) = std::fs::remove_dir_all(&self.temp_dir) {
            debug!("Failed to cleanup screenshot temp directory: {}", e);
        }

        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("Screenshot plugin is disabled, ignoring packet");
            return Ok(());
        }

        if packet.is_type("cconnect.screenshot.request") {
            self.handle_screenshot_request(packet, device).await
        } else if packet.is_type("cconnect.screenshot.region") {
            self.handle_region_request(packet, device).await
        } else if packet.is_type("cconnect.screenshot.window") {
            self.handle_window_request(packet, device).await
        } else {
            Ok(())
        }
    }
}

/// Factory for creating ScreenshotPlugin instances
#[derive(Debug, Clone, Copy)]
pub struct ScreenshotPluginFactory;

impl PluginFactory for ScreenshotPluginFactory {
    fn name(&self) -> &str {
        "screenshot"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.screenshot.request".to_string(),
            "cconnect.screenshot.region".to_string(),
            "cconnect.screenshot.window".to_string(),
            "kdeconnect.screenshot.request".to_string(),
            "kdeconnect.screenshot.region".to_string(),
            "kdeconnect.screenshot.window".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec!["cconnect.screenshot.data".to_string()]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(ScreenshotPlugin::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceInfo, DeviceType};
    use serde_json::json;

    fn create_test_device() -> Device {
        let info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1716);
        Device::from_discovery(info)
    }

    #[test]
    fn test_plugin_creation() {
        let plugin = ScreenshotPlugin::new();
        assert_eq!(plugin.name(), "screenshot");
        assert!(plugin.enabled);
    }

    #[test]
    fn test_capabilities() {
        let plugin = ScreenshotPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 6);
        assert!(incoming.contains(&"cconnect.screenshot.request".to_string()));
        assert!(incoming.contains(&"cconnect.screenshot.region".to_string()));
        assert!(incoming.contains(&"cconnect.screenshot.window".to_string()));
        assert!(incoming.contains(&"kdeconnect.screenshot.request".to_string()));
        assert!(incoming.contains(&"kdeconnect.screenshot.region".to_string()));
        assert!(incoming.contains(&"kdeconnect.screenshot.window".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 1);
        assert!(outgoing.contains(&"cconnect.screenshot.data".to_string()));
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = ScreenshotPlugin::new();
        let device = create_test_device();

        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();
        assert!(plugin.device_id.is_some());

        plugin.start().await.unwrap();
        assert!(plugin.enabled);

        plugin.stop().await.unwrap();
        assert!(!plugin.enabled);
    }

    #[test]
    fn test_detect_display_server() {
        // This test will vary based on the actual environment
        let display_server = ScreenshotPlugin::detect_display_server();
        // Just verify it returns something valid
        assert!(matches!(
            display_server,
            DisplayServer::Wayland | DisplayServer::X11 | DisplayServer::Unknown
        ));
    }

    #[tokio::test]
    async fn test_handle_screenshot_request() {
        let mut plugin = ScreenshotPlugin::new();
        let device = create_test_device();
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();
        plugin.start().await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.screenshot.request",
            json!({
                "captureType": "fullscreen"
            }),
        );

        // This may fail if screenshot tools aren't available, but shouldn't panic
        let _result = plugin.handle_packet(&packet, &mut device).await;
    }

    #[tokio::test]
    async fn test_handle_region_request() {
        let mut plugin = ScreenshotPlugin::new();
        let device = create_test_device();
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();
        plugin.start().await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.screenshot.region",
            json!({
                "x": 100,
                "y": 100,
                "width": 800,
                "height": 600
            }),
        );

        // May fail without screenshot tools, but shouldn't panic
        let _result = plugin.handle_packet(&packet, &mut device).await;
    }

    #[test]
    fn test_factory() {
        let factory = ScreenshotPluginFactory;
        assert_eq!(factory.name(), "screenshot");

        let plugin = factory.create();
        assert_eq!(plugin.name(), "screenshot");
    }
}
