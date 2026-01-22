//! Mouse & Keyboard Share Plugin
//!
//! Share mouse and keyboard across multiple desktops seamlessly (Synergy/Barrier-like).
//!
//! ## Protocol Specification
//!
//! This plugin implements seamless mouse and keyboard sharing across multiple
//! desktops, similar to Synergy or Barrier. Move your mouse between screens
//! and have the keyboard follow automatically.
//!
//! ### Packet Types
//!
//! - `cconnect.mkshare.config` - Screen arrangement and edge mapping
//! - `cconnect.mkshare.mouse` - Mouse movements and clicks
//! - `cconnect.mkshare.keyboard` - Keyboard events
//! - `cconnect.mkshare.enter` - Mouse entering remote screen
//! - `cconnect.mkshare.leave` - Mouse leaving screen
//! - `cconnect.mkshare.hotkey` - Hotkey-triggered switch
//!
//! ### Capabilities
//!
//! - Incoming: `cconnect.mkshare` - Can receive input from remote
//! - Outgoing: `cconnect.mkshare` - Can send input to remote
//!
//! ### Use Cases
//!
//! - Seamless multi-desktop workspace
//! - Control multiple computers with one keyboard/mouse
//! - Shared clipboard across desktops
//! - Gaming setups with multiple PCs
//! - Development across multiple machines
//!
//! ## Features
//!
//! - **Edge Detection**: Automatically detect cursor at screen edges
//! - **Smooth Transitions**: Seamless mouse movement between screens
//! - **Keyboard Follow**: Keyboard input follows mouse position
//! - **Hotkey Switching**: Quick switch between desktops with hotkeys
//! - **Screen Arrangement**: Configure which edges connect to which desktops
//! - **Clipboard Sync**: Shared clipboard via Clipboard plugin
//! - **Multi-Monitor**: Support for multiple monitors per desktop
//!
//! ## Screen Arrangement
//!
//! Configure which screen edges connect to which remote desktops:
//! - **Top/Bottom/Left/Right**: Map edges to device IDs
//! - **Corner Handling**: Configurable corner behavior
//! - **Dead Zones**: Optional dead zones at screen corners
//!
//! ## Implementation Status
//!
//! TODO: Global input capture (X11, Wayland, compositor integration)
//! TODO: Edge detection and cursor tracking
//! TODO: Input event forwarding to remote
//! TODO: Screen geometry synchronization
//! TODO: Hotkey registration and handling
//! TODO: COSMIC compositor integration for Wayland

use crate::plugins::{Plugin, PluginFactory};
use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use tracing::{debug, info, warn};

const PLUGIN_NAME: &str = "mousekeyboardshare";
const INCOMING_CAPABILITY: &str = "cconnect.mkshare";
const OUTGOING_CAPABILITY: &str = "cconnect.mkshare";

/// Screen edge
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScreenEdge {
    /// Top edge
    Top,
    /// Bottom edge
    Bottom,
    /// Left edge
    Left,
    /// Right edge
    Right,
}

impl ScreenEdge {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    /// Get opposite edge
    pub fn opposite(&self) -> Self {
        match self {
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Top,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// Screen geometry information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenGeometry {
    /// Screen width in pixels
    pub width: u32,

    /// Screen height in pixels
    pub height: u32,

    /// X offset (for multi-monitor setups)
    #[serde(rename = "xOffset", default)]
    pub x_offset: i32,

    /// Y offset (for multi-monitor setups)
    #[serde(rename = "yOffset", default)]
    pub y_offset: i32,

    /// DPI/scale factor
    #[serde(default = "default_scale")]
    pub scale: f32,
}

#[allow(dead_code)]
fn default_scale() -> f32 {
    1.0
}

impl Default for ScreenGeometry {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            x_offset: 0,
            y_offset: 0,
            scale: 1.0,
        }
    }
}

/// Edge mapping configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeMapping {
    /// Device ID for this edge
    #[serde(rename = "deviceId")]
    pub device_id: String,

    /// Which edge on the remote device
    #[serde(rename = "remoteEdge")]
    pub remote_edge: ScreenEdge,

    /// Dead zone size in pixels (to avoid accidental switching)
    #[serde(rename = "deadZone", default = "default_dead_zone")]
    pub dead_zone: u32,
}

#[allow(dead_code)]
fn default_dead_zone() -> u32 {
    50 // 50 pixels from corners
}

/// Mouse & Keyboard Share configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MkShareConfig {
    /// This desktop's screen geometry
    #[serde(rename = "localGeometry")]
    pub local_geometry: ScreenGeometry,

    /// Edge mappings (which edge connects to which device)
    #[serde(rename = "edgeMappings", default)]
    pub edge_mappings: HashMap<ScreenEdge, EdgeMapping>,

    /// Enable edge switching
    #[serde(rename = "enableEdgeSwitching", default = "default_true")]
    pub enable_edge_switching: bool,

    /// Enable hotkey switching
    #[serde(rename = "enableHotkeySwitching", default = "default_true")]
    pub enable_hotkey_switching: bool,

    /// Hotkey combination (e.g., "Ctrl+Shift+Tab")
    #[serde(rename = "switchHotkey", default = "default_hotkey")]
    pub switch_hotkey: String,

    /// Enable clipboard sync
    #[serde(rename = "enableClipboardSync", default = "default_true")]
    pub enable_clipboard_sync: bool,

    /// Edge detection threshold in pixels
    #[serde(rename = "edgeThreshold", default = "default_edge_threshold")]
    pub edge_threshold: u32,
}

#[allow(dead_code)]
fn default_true() -> bool {
    true
}

#[allow(dead_code)]
fn default_hotkey() -> String {
    "Ctrl+Shift+Tab".to_string()
}

#[allow(dead_code)]
fn default_edge_threshold() -> u32 {
    5 // 5 pixels from edge
}

impl Default for MkShareConfig {
    fn default() -> Self {
        Self {
            local_geometry: ScreenGeometry::default(),
            edge_mappings: HashMap::new(),
            enable_edge_switching: true,
            enable_hotkey_switching: true,
            switch_hotkey: default_hotkey(),
            enable_clipboard_sync: true,
            edge_threshold: default_edge_threshold(),
        }
    }
}

impl MkShareConfig {
    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        // Validate geometry
        if self.local_geometry.width == 0 || self.local_geometry.height == 0 {
            return Err(ProtocolError::InvalidPacket(
                "Screen dimensions must be > 0".to_string(),
            ));
        }

        if self.local_geometry.scale <= 0.0 || self.local_geometry.scale > 4.0 {
            return Err(ProtocolError::InvalidPacket(format!(
                "Invalid scale factor: {}. Must be 0.0-4.0",
                self.local_geometry.scale
            )));
        }

        // Validate edge threshold
        if self.edge_threshold > 100 {
            return Err(ProtocolError::InvalidPacket(format!(
                "Edge threshold too large: {}. Max is 100 pixels",
                self.edge_threshold
            )));
        }

        Ok(())
    }

    /// Check if cursor is at screen edge
    pub fn is_at_edge(&self, x: i32, y: i32) -> Option<ScreenEdge> {
        let threshold = self.edge_threshold as i32;
        let width = self.local_geometry.width as i32;
        let height = self.local_geometry.height as i32;

        if x <= threshold {
            Some(ScreenEdge::Left)
        } else if x >= width - threshold {
            Some(ScreenEdge::Right)
        } else if y <= threshold {
            Some(ScreenEdge::Top)
        } else if y >= height - threshold {
            Some(ScreenEdge::Bottom)
        } else {
            None
        }
    }
}

/// Mouse event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseEvent {
    /// X coordinate
    pub x: i32,

    /// Y coordinate
    pub y: i32,

    /// Mouse button (1=left, 2=middle, 3=right, 0=move only)
    #[serde(default)]
    pub button: u8,

    /// Button pressed (true) or released (false)
    #[serde(default)]
    pub pressed: bool,

    /// Scroll delta X
    #[serde(rename = "scrollX", default)]
    pub scroll_x: i32,

    /// Scroll delta Y
    #[serde(rename = "scrollY", default)]
    pub scroll_y: i32,
}

/// Keyboard event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardEvent {
    /// Key code
    #[serde(rename = "keyCode")]
    pub key_code: u32,

    /// Key pressed (true) or released (false)
    pub pressed: bool,

    /// Modifiers (Ctrl, Shift, Alt, etc.)
    #[serde(default)]
    pub modifiers: u32,

    /// Character (for text input)
    #[serde(default)]
    pub character: Option<String>,
}

/// Active sharing state
#[derive(Debug)]
pub enum ShareState {
    /// Local control (mouse on this screen)
    Local,
    /// Remote control (mouse on remote screen)
    Remote {
        device_id: String,
        #[allow(dead_code)]
        entry_edge: ScreenEdge,
    },
}

/// Mouse & Keyboard Share plugin
pub struct MouseKeyboardSharePlugin {
    /// Device ID this plugin is associated with
    device_id: Option<String>,

    /// Plugin enabled state
    enabled: bool,

    /// Current configuration
    config: MkShareConfig,

    /// Current share state
    state: ShareState,

    /// Remote desktop configurations
    remote_configs: HashMap<String, MkShareConfig>,

    /// Last mouse position
    last_mouse_pos: (i32, i32),
}

impl MouseKeyboardSharePlugin {
    /// Create new mouse & keyboard share plugin instance
    pub fn new() -> Self {
        Self {
            device_id: None,
            enabled: false,
            config: MkShareConfig::default(),
            state: ShareState::Local,
            remote_configs: HashMap::new(),
            last_mouse_pos: (0, 0),
        }
    }

    /// Configure screen arrangement
    pub fn configure(&mut self, config: MkShareConfig) -> Result<()> {
        config.validate()?;

        info!(
            "Configuring mouse/keyboard share: {}x{} screen, {} edge mappings",
            config.local_geometry.width,
            config.local_geometry.height,
            config.edge_mappings.len()
        );

        self.config = config;

        // TODO: Update input capture configuration
        // TODO: Register hotkey

        Ok(())
    }

    /// Add edge mapping
    pub fn add_edge_mapping(&mut self, edge: ScreenEdge, mapping: EdgeMapping) {
        let device_id = mapping.device_id.clone();
        let remote_edge = mapping.remote_edge;

        self.config.edge_mappings.insert(edge, mapping);

        info!(
            "Added edge mapping: {} -> {} ({})",
            edge.as_str(),
            device_id,
            remote_edge.as_str()
        );
    }

    /// Remove edge mapping
    pub fn remove_edge_mapping(&mut self, edge: ScreenEdge) {
        if self.config.edge_mappings.remove(&edge).is_some() {
            info!("Removed edge mapping for {} edge", edge.as_str());
        }
    }

    /// Handle mouse movement
    pub fn handle_mouse_move(&mut self, x: i32, y: i32) -> Result<Option<String>> {
        self.last_mouse_pos = (x, y);

        // Check if at edge and should switch
        if let ShareState::Local = self.state {
            if self.config.enable_edge_switching {
                if let Some(edge) = self.config.is_at_edge(x, y) {
                    if let Some(mapping) = self.config.edge_mappings.get(&edge) {
                        // Check dead zone
                        let in_dead_zone = self.is_in_dead_zone(x, y, edge, mapping.dead_zone);

                        if !in_dead_zone {
                            info!(
                                "Cursor at {} edge, switching to device {}",
                                edge.as_str(),
                                mapping.device_id
                            );

                            // Switch to remote
                            self.state = ShareState::Remote {
                                device_id: mapping.device_id.clone(),
                                entry_edge: mapping.remote_edge,
                            };

                            // TODO: Capture input
                            // TODO: Send mouse_enter packet to remote

                            return Ok(Some(mapping.device_id.clone()));
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    /// Check if position is in dead zone
    fn is_in_dead_zone(&self, x: i32, y: i32, edge: ScreenEdge, dead_zone: u32) -> bool {
        let width = self.config.local_geometry.width as i32;
        let height = self.config.local_geometry.height as i32;
        let dz = dead_zone as i32;

        match edge {
            ScreenEdge::Left | ScreenEdge::Right => y < dz || y > height - dz,
            ScreenEdge::Top | ScreenEdge::Bottom => x < dz || x > width - dz,
        }
    }

    /// Return to local control
    pub fn return_to_local(&mut self) {
        if let ShareState::Remote { device_id, .. } = &self.state {
            info!("Returning to local control from device {}", device_id);

            // TODO: Release input capture
            // TODO: Send mouse_leave packet to remote

            self.state = ShareState::Local;
        }
    }

    /// Get current state
    pub fn get_state(&self) -> &ShareState {
        &self.state
    }

    /// Check if currently controlling remote
    pub fn is_remote(&self) -> bool {
        matches!(self.state, ShareState::Remote { .. })
    }
}

impl Default for MouseKeyboardSharePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for MouseKeyboardSharePlugin {
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
            "kdeconnect.mkshare".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![OUTGOING_CAPABILITY.to_string()]
    }

    async fn init(&mut self, device: &Device, _packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>) -> Result<()> {
        info!(
            "Initializing MouseKeyboardShare plugin for device {}",
            device.name()
        );
        self.device_id = Some(device.id().to_string());

        // TODO: Detect screen geometry
        // TODO: Initialize input capture framework

        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Starting MouseKeyboardShare plugin");
        self.enabled = true;

        // TODO: Start global input monitoring
        // TODO: Register hotkeys
        // TODO: Start edge detection

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Stopping MouseKeyboardShare plugin");
        self.enabled = false;

        // Return to local control
        self.return_to_local();

        // TODO: Stop input monitoring
        // TODO: Unregister hotkeys

        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("MouseKeyboardShare plugin is disabled, ignoring packet");
            return Ok(());
        }

        debug!("Handling packet type: {}", packet.packet_type);

        if packet.is_type("cconnect.mkshare.config") {
            // Receive remote configuration
            let remote_config: MkShareConfig = serde_json::from_value(packet.body.clone())
                .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

            self.remote_configs
                .insert(device.id().to_string(), remote_config);

            info!("Received screen configuration from {}", device.name());
        } else if packet.is_type("cconnect.mkshare.mouse") {
            // Receive mouse event from remote
            let mouse_event: MouseEvent = serde_json::from_value(packet.body.clone())
                .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

            // TODO: Inject mouse event into local system
            // TODO: Use RemoteInput plugin for actual injection

            debug!(
                "Received mouse event: ({}, {}), button: {}, pressed: {}",
                mouse_event.x, mouse_event.y, mouse_event.button, mouse_event.pressed
            );
        } else if packet.is_type("cconnect.mkshare.keyboard") {
            // Receive keyboard event from remote
            let kbd_event: KeyboardEvent = serde_json::from_value(packet.body.clone())
                .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

            // TODO: Inject keyboard event into local system
            // TODO: Use RemoteInput plugin for actual injection

            debug!(
                "Received keyboard event: key_code: {}, pressed: {}, modifiers: {}",
                kbd_event.key_code, kbd_event.pressed, kbd_event.modifiers
            );
        } else if packet.is_type("cconnect.mkshare.enter") {
            // Remote desktop's cursor entered this screen
            info!("Remote cursor entered from {}", device.name());

            // TODO: Show cursor at entry point
            // TODO: Start forwarding local input to remote
        } else if packet.is_type("cconnect.mkshare.leave") {
            // Remote desktop's cursor left this screen
            info!("Remote cursor left to {}", device.name());

            // TODO: Hide cursor
            // TODO: Stop forwarding input
        } else if packet.is_type("cconnect.mkshare.hotkey") {
            // Hotkey-triggered switch
            info!("Hotkey switch requested by {}", device.name());

            // TODO: Switch control to requesting device
        }

        Ok(())
    }
}

/// Mouse & Keyboard Share plugin factory
pub struct MouseKeyboardSharePluginFactory;

impl PluginFactory for MouseKeyboardSharePluginFactory {
    fn create(&self) -> Box<dyn Plugin> {
        Box::new(MouseKeyboardSharePlugin::new())
    }

    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            INCOMING_CAPABILITY.to_string(),
            "kdeconnect.mkshare".to_string(),
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
        let plugin = MouseKeyboardSharePlugin::new();
        assert_eq!(plugin.name(), PLUGIN_NAME);
        assert!(!plugin.enabled);
        assert!(!plugin.is_remote());
    }

    #[tokio::test]
    async fn test_config_validation() {
        let config = MkShareConfig::default();
        assert!(config.validate().is_ok());

        let mut invalid_geometry = config.clone();
        invalid_geometry.local_geometry.width = 0;
        assert!(invalid_geometry.validate().is_err());

        let mut invalid_scale = config.clone();
        invalid_scale.local_geometry.scale = 5.0;
        assert!(invalid_scale.validate().is_err());

        let mut invalid_threshold = config;
        invalid_threshold.edge_threshold = 200;
        assert!(invalid_threshold.validate().is_err());
    }

    #[tokio::test]
    async fn test_edge_detection() {
        let config = MkShareConfig {
            local_geometry: ScreenGeometry {
                width: 1920,
                height: 1080,
                ..Default::default()
            },
            edge_threshold: 5,
            ..Default::default()
        };

        assert_eq!(config.is_at_edge(2, 500), Some(ScreenEdge::Left));
        assert_eq!(config.is_at_edge(1918, 500), Some(ScreenEdge::Right));
        assert_eq!(config.is_at_edge(960, 2), Some(ScreenEdge::Top));
        assert_eq!(config.is_at_edge(960, 1078), Some(ScreenEdge::Bottom));
        assert_eq!(config.is_at_edge(960, 540), None);
    }

    #[tokio::test]
    async fn test_screen_edges() {
        assert_eq!(ScreenEdge::Top.as_str(), "top");
        assert_eq!(ScreenEdge::Bottom.as_str(), "bottom");
        assert_eq!(ScreenEdge::Left.as_str(), "left");
        assert_eq!(ScreenEdge::Right.as_str(), "right");

        assert_eq!(ScreenEdge::Top.opposite(), ScreenEdge::Bottom);
        assert_eq!(ScreenEdge::Left.opposite(), ScreenEdge::Right);
    }

    #[tokio::test]
    async fn test_configure() {
        let mut plugin = MouseKeyboardSharePlugin::new();
        plugin.enabled = true;

        let config = MkShareConfig::default();
        assert!(plugin.configure(config).is_ok());
    }

    #[tokio::test]
    async fn test_edge_mapping() {
        let mut plugin = MouseKeyboardSharePlugin::new();
        plugin.enabled = true;

        let mapping = EdgeMapping {
            device_id: "test_device".to_string(),
            remote_edge: ScreenEdge::Left,
            dead_zone: 50,
        };

        plugin.add_edge_mapping(ScreenEdge::Right, mapping);
        assert_eq!(plugin.config.edge_mappings.len(), 1);

        plugin.remove_edge_mapping(ScreenEdge::Right);
        assert_eq!(plugin.config.edge_mappings.len(), 0);
    }

    #[tokio::test]
    async fn test_mouse_move() {
        let mut plugin = MouseKeyboardSharePlugin::new();
        plugin.enabled = true;

        let result = plugin.handle_mouse_move(100, 100);
        assert!(result.is_ok());
        assert_eq!(plugin.last_mouse_pos, (100, 100));
    }

    #[tokio::test]
    async fn test_return_to_local() {
        let mut plugin = MouseKeyboardSharePlugin::new();
        plugin.state = ShareState::Remote {
            device_id: "test".to_string(),
            entry_edge: ScreenEdge::Right,
        };

        plugin.return_to_local();
        assert!(!plugin.is_remote());
    }

    #[tokio::test]
    async fn test_handle_config_packet() {
        let mut device = create_test_device();
        let factory = MouseKeyboardSharePluginFactory;
        let mut plugin = factory.create();

        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();
        plugin.start().await.unwrap();

        let config = MkShareConfig::default();
        let body = serde_json::to_value(&config).unwrap();

        let packet = Packet::new("cconnect.mkshare.config", body);

        assert!(plugin.handle_packet(&packet, &mut device).await.is_ok());
    }

    #[tokio::test]
    async fn test_handle_mouse_packet() {
        let mut device = create_test_device();
        let factory = MouseKeyboardSharePluginFactory;
        let mut plugin = factory.create();

        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();
        plugin.start().await.unwrap();

        let mouse_event = MouseEvent {
            x: 100,
            y: 200,
            button: 1,
            pressed: true,
            scroll_x: 0,
            scroll_y: 0,
        };

        let body = serde_json::to_value(&mouse_event).unwrap();

        let packet = Packet::new("cconnect.mkshare.mouse", body);

        assert!(plugin.handle_packet(&packet, &mut device).await.is_ok());
    }
}
