//! Remote Input Plugin
//!
//! This plugin enables remote control of the pointer and keyboard.
//! It supports mouse movements, clicks, scrolling, and keyboard input.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `cconnect.mousepad.request` - Remote input request (incoming)
//! - `cconnect.mousepad.echo` - Echo response (outgoing)
//! - `cconnect.mousepad.keyboardstate` - Keyboard state broadcast (outgoing)
//!
//! **Capabilities**:
//! - Incoming: `cconnect.mousepad.request` - Receives pointer and keyboard events
//! - Outgoing: `cconnect.mousepad.keyboardstate` - Sends keyboard support status
//!
//! ## References
//!
//! - [CConnect MousePad Plugin](https://github.com/KDE/cconnect-kde/tree/master/plugins/mousepad)
//! - [Valent Protocol - MousePad](https://valent.andyholmes.ca/documentation/protocol.html)

use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use mouse_keyboard_input::VirtualDevice;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info, warn};

use super::{Plugin, PluginFactory};

/// Packet type for remote input requests
pub const PACKET_TYPE_MOUSEPAD_REQUEST: &str = "cconnect.mousepad.request";

/// Packet type for echo responses
pub const PACKET_TYPE_MOUSEPAD_ECHO: &str = "cconnect.mousepad.echo";

/// Packet type for keyboard state
pub const PACKET_TYPE_MOUSEPAD_KEYBOARDSTATE: &str = "cconnect.mousepad.keyboardstate";

/// Special key codes for non-printable characters
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[allow(dead_code)]
pub enum SpecialKey {
    Backspace = 1,
    Tab = 2,
    Enter = 12,
    Escape = 27,
    Left = 21,
    Up = 22,
    Right = 23,
    Down = 24,
    PageUp = 25,
    PageDown = 26,
    Home = 28,
    End = 29,
    Delete = 30,
    F1 = 31,
    F2 = 32,
    F3 = 33,
    F4 = 34,
    F5 = 35,
    F6 = 36,
    F7 = 37,
    F8 = 38,
    F9 = 39,
    F10 = 40,
    F11 = 41,
    F12 = 42,
}

/// Remote input request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteInputRequest {
    /// Single readable character input
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,

    /// Non-printable character (0-32)
    #[serde(skip_serializing_if = "Option::is_none", rename = "specialKey")]
    pub special_key: Option<i32>,

    /// Alt modifier key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt: Option<bool>,

    /// Ctrl modifier key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctrl: Option<bool>,

    /// Shift modifier key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shift: Option<bool>,

    /// Super/Windows/Command modifier key
    #[serde(skip_serializing_if = "Option::is_none", rename = "super")]
    pub super_key: Option<bool>,

    /// Single click action
    #[serde(skip_serializing_if = "Option::is_none")]
    pub singleclick: Option<bool>,

    /// Double click action
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doubleclick: Option<bool>,

    /// Middle click action
    #[serde(skip_serializing_if = "Option::is_none")]
    pub middleclick: Option<bool>,

    /// Right click action
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rightclick: Option<bool>,

    /// Single hold (press) action
    #[serde(skip_serializing_if = "Option::is_none")]
    pub singlehold: Option<bool>,

    /// Single release action
    #[serde(skip_serializing_if = "Option::is_none")]
    pub singlerelease: Option<bool>,

    /// Position delta on X axis
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dx: Option<f64>,

    /// Position delta on Y axis
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dy: Option<f64>,

    /// Whether movement is a scroll event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll: Option<bool>,

    /// Request confirmation via echo packet
    #[serde(skip_serializing_if = "Option::is_none", rename = "sendAck")]
    pub send_ack: Option<bool>,
}

/// Remote Input plugin for pointer and keyboard control
pub struct RemoteInputPlugin {
    device_id: Option<String>,
    virtual_device: Arc<Mutex<Option<VirtualDevice>>>,
}

impl RemoteInputPlugin {
    /// Create a new Remote Input plugin
    pub fn new() -> Self {
        Self {
            device_id: None,
            virtual_device: Arc::new(Mutex::new(None)),
        }
    }

    /// Handle a remote input request packet
    async fn handle_request(&self, packet: &Packet) -> Result<()> {
        let request: RemoteInputRequest = serde_json::from_value(packet.body.clone())
            .map_err(|e| ProtocolError::InvalidPacket(format!("Failed to parse request: {}", e)))?;

        // Get or create virtual device
        let device = {
            let mut device_guard = self.virtual_device.lock().unwrap();
            if device_guard.is_none() {
                match VirtualDevice::default() {
                    Ok(dev) => {
                        info!("Created virtual input device");
                        *device_guard = Some(dev);
                    }
                    Err(e) => {
                        error!("Failed to create virtual input device: {}", e);
                        return Err(ProtocolError::Plugin(format!(
                            "Failed to create virtual input device: {}",
                            e
                        )));
                    }
                }
            }
            Arc::clone(&self.virtual_device)
        };

        // Handle mouse movement and scrolling
        if request.dx.is_some() || request.dy.is_some() {
            let dx = request.dx.unwrap_or(0.0) as i32;
            let dy = request.dy.unwrap_or(0.0) as i32;
            let is_scroll = request.scroll.unwrap_or(false);

            let mut device_guard = device.lock().unwrap();
            if let Some(dev) = device_guard.as_mut() {
                if is_scroll {
                    debug!("Remote input: Scroll dx={}, dy={}", dx, dy);
                    if let Err(e) = dev.smooth_scroll(dx, dy) {
                        warn!("Failed to scroll: {}", e);
                    }
                } else {
                    debug!("Remote input: Move pointer dx={}, dy={}", dx, dy);
                    if let Err(e) = dev.smooth_move_mouse(dx, dy) {
                        warn!("Failed to move mouse: {}", e);
                    }
                }
            }
        }

        // Handle mouse clicks
        use mouse_keyboard_input::{BTN_LEFT, BTN_MIDDLE, BTN_RIGHT};

        let mut device_guard = device.lock().unwrap();
        if let Some(dev) = device_guard.as_mut() {
            if request.singleclick.unwrap_or(false) {
                debug!("Remote input: Single click");
                if let Err(e) = dev.click(BTN_LEFT) {
                    warn!("Failed to click: {}", e);
                }
            }
            if request.doubleclick.unwrap_or(false) {
                debug!("Remote input: Double click");
                if let Err(e) = dev.click(BTN_LEFT).and_then(|_| dev.click(BTN_LEFT)) {
                    warn!("Failed to double click: {}", e);
                }
            }
            if request.middleclick.unwrap_or(false) {
                debug!("Remote input: Middle click");
                if let Err(e) = dev.click(BTN_MIDDLE) {
                    warn!("Failed to middle click: {}", e);
                }
            }
            if request.rightclick.unwrap_or(false) {
                debug!("Remote input: Right click");
                if let Err(e) = dev.click(BTN_RIGHT) {
                    warn!("Failed to right click: {}", e);
                }
            }
            if request.singlehold.unwrap_or(false) {
                debug!("Remote input: Single hold");
                if let Err(e) = dev.press(BTN_LEFT) {
                    warn!("Failed to press button: {}", e);
                }
            }
            if request.singlerelease.unwrap_or(false) {
                debug!("Remote input: Single release");
                if let Err(e) = dev.release(BTN_LEFT) {
                    warn!("Failed to release button: {}", e);
                }
            }
        }

        // Handle keyboard input
        if let Some(key) = &request.key {
            debug!("Remote input: Key '{}'", key);
            let mut device_guard = device.lock().unwrap();
            if let Some(dev) = device_guard.as_mut() {
                // Convert string to key codes and send
                for ch in key.chars() {
                    if let Some(key_code) = Self::char_to_keycode(ch) {
                        if let Err(e) = dev.click(key_code) {
                            warn!("Failed to send key '{}': {}", ch, e);
                        }
                    }
                }
            }
        }
        if let Some(special_key) = request.special_key {
            debug!("Remote input: Special key {}", special_key);
            let mut device_guard = device.lock().unwrap();
            if let Some(dev) = device_guard.as_mut() {
                if let Some(key_code) = Self::special_key_to_keycode(special_key) {
                    if let Err(e) = dev.click(key_code) {
                        warn!("Failed to send special key {}: {}", special_key, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Convert character to Linux key code
    fn char_to_keycode(ch: char) -> Option<u16> {
        use mouse_keyboard_input::*;
        match ch {
            'a' | 'A' => Some(KEY_A),
            'b' | 'B' => Some(KEY_B),
            'c' | 'C' => Some(KEY_C),
            'd' | 'D' => Some(KEY_D),
            'e' | 'E' => Some(KEY_E),
            'f' | 'F' => Some(KEY_F),
            'g' | 'G' => Some(KEY_G),
            'h' | 'H' => Some(KEY_H),
            'i' | 'I' => Some(KEY_I),
            'j' | 'J' => Some(KEY_J),
            'k' | 'K' => Some(KEY_K),
            'l' | 'L' => Some(KEY_L),
            'm' | 'M' => Some(KEY_M),
            'n' | 'N' => Some(KEY_N),
            'o' | 'O' => Some(KEY_O),
            'p' | 'P' => Some(KEY_P),
            'q' | 'Q' => Some(KEY_Q),
            'r' | 'R' => Some(KEY_R),
            's' | 'S' => Some(KEY_S),
            't' | 'T' => Some(KEY_T),
            'u' | 'U' => Some(KEY_U),
            'v' | 'V' => Some(KEY_V),
            'w' | 'W' => Some(KEY_W),
            'x' | 'X' => Some(KEY_X),
            'y' | 'Y' => Some(KEY_Y),
            'z' | 'Z' => Some(KEY_Z),
            '0' => Some(11), // KEY_0 (between KEY_9=10 and KEY_MINUS=12)
            '1' => Some(KEY_1),
            '2' => Some(KEY_2),
            '3' => Some(KEY_3),
            '4' => Some(KEY_4),
            '5' => Some(KEY_5),
            '6' => Some(KEY_6),
            '7' => Some(KEY_7),
            '8' => Some(KEY_8),
            '9' => Some(KEY_9),
            ' ' => Some(KEY_SPACE),
            '\n' => Some(KEY_ENTER),
            '\t' => Some(KEY_TAB),
            '.' => Some(KEY_DOT),
            ',' => Some(KEY_COMMA),
            '/' => Some(KEY_SLASH),
            '-' => Some(KEY_MINUS),
            '=' => Some(KEY_EQUAL),
            '[' => Some(KEY_LEFTBRACE),
            ']' => Some(KEY_RIGHTBRACE),
            ';' => Some(KEY_SEMICOLON),
            '\'' => Some(KEY_APOSTROPHE),
            '`' => Some(KEY_GRAVE),
            '\\' => Some(KEY_BACKSLASH),
            _ => None,
        }
    }

    /// Convert special key code to Linux key code
    fn special_key_to_keycode(special: i32) -> Option<u16> {
        use mouse_keyboard_input::*;
        match special {
            1 => Some(KEY_BACKSPACE),    // Backspace
            2 => Some(KEY_TAB),           // Tab
            12 => Some(KEY_ENTER),        // Enter
            27 => Some(KEY_ESC),          // Escape
            21 => Some(KEY_LEFT),         // Left
            22 => Some(KEY_UP),           // Up
            23 => Some(KEY_RIGHT),        // Right
            24 => Some(KEY_DOWN),         // Down
            25 => Some(KEY_PAGEUP),       // PageUp
            26 => Some(KEY_PAGEDOWN),     // PageDown
            28 => Some(KEY_HOME),         // Home
            29 => Some(KEY_END),          // End
            30 => Some(KEY_DELETE),       // Delete
            31 => Some(KEY_F1),           // F1
            32 => Some(KEY_F2),           // F2
            33 => Some(KEY_F3),           // F3
            34 => Some(KEY_F4),           // F4
            35 => Some(KEY_F5),           // F5
            36 => Some(KEY_F6),           // F6
            37 => Some(KEY_F7),           // F7
            38 => Some(KEY_F8),           // F8
            39 => Some(KEY_F9),           // F9
            40 => Some(KEY_F10),          // F10
            41 => Some(KEY_F11),          // F11
            42 => Some(KEY_F12),          // F12
            _ => None,
        }
    }
}

impl Default for RemoteInputPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for RemoteInputPlugin {
    fn name(&self) -> &str {
        "remoteinput"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![PACKET_TYPE_MOUSEPAD_REQUEST.to_string()]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![PACKET_TYPE_MOUSEPAD_KEYBOARDSTATE.to_string()]
    }

    async fn init(&mut self, device: &Device) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!("Remote Input plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Remote Input plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Remote Input plugin stopped");
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, _device: &mut Device) -> Result<()> {
        match packet.packet_type.as_str() {
            PACKET_TYPE_MOUSEPAD_REQUEST => {
                debug!("Received remote input request");
                self.handle_request(packet).await
            }
            _ => {
                warn!("Unexpected packet type: {}", packet.packet_type);
                Ok(())
            }
        }
    }
}

/// Factory for creating Remote Input plugin instances
#[derive(Debug, Clone, Copy)]
pub struct RemoteInputPluginFactory;

impl PluginFactory for RemoteInputPluginFactory {
    fn name(&self) -> &str {
        "remoteinput"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![PACKET_TYPE_MOUSEPAD_REQUEST.to_string()]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![PACKET_TYPE_MOUSEPAD_KEYBOARDSTATE.to_string()]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(RemoteInputPlugin::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceInfo, DeviceType};

    fn create_test_device() -> Device {
        let info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1716);
        Device::from_discovery(info)
    }

    #[tokio::test]
    async fn test_plugin_creation() {
        let plugin = RemoteInputPlugin::new();
        assert_eq!(plugin.name(), "remoteinput");
        assert!(plugin.device_id.is_none());
    }

    #[tokio::test]
    async fn test_plugin_initialization() {
        let mut plugin = RemoteInputPlugin::new();
        let device = create_test_device();

        assert!(plugin.init(&device).await.is_ok());
        assert_eq!(plugin.device_id, Some(device.id().to_string()));
    }

    #[tokio::test]
    async fn test_handle_mouse_movement() {
        let mut plugin = RemoteInputPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let packet = Packet::new(
            "cconnect.mousepad.request",
            serde_json::json!({
                "dx": 10.0,
                "dy": 20.0
            }),
        );

        let mut device_mut = device;
        let result = plugin.handle_packet(&packet, &mut device_mut).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_mouse_click() {
        let mut plugin = RemoteInputPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let packet = Packet::new(
            "cconnect.mousepad.request",
            serde_json::json!({
                "singleclick": true
            }),
        );

        let mut device_mut = device;
        let result = plugin.handle_packet(&packet, &mut device_mut).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_keyboard_input() {
        let mut plugin = RemoteInputPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let packet = Packet::new(
            "cconnect.mousepad.request",
            serde_json::json!({
                "key": "a"
            }),
        );

        let mut device_mut = device;
        let result = plugin.handle_packet(&packet, &mut device_mut).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_special_key() {
        let mut plugin = RemoteInputPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let packet = Packet::new(
            "cconnect.mousepad.request",
            serde_json::json!({
                "specialKey": 1
            }),
        );

        let mut device_mut = device;
        let result = plugin.handle_packet(&packet, &mut device_mut).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_scroll_event() {
        let mut plugin = RemoteInputPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let packet = Packet::new(
            "cconnect.mousepad.request",
            serde_json::json!({
                "dx": 0.0,
                "dy": -5.0,
                "scroll": true
            }),
        );

        let mut device_mut = device;
        let result = plugin.handle_packet(&packet, &mut device_mut).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_modifiers() {
        let mut plugin = RemoteInputPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let packet = Packet::new(
            "cconnect.mousepad.request",
            serde_json::json!({
                "key": "c",
                "ctrl": true
            }),
        );

        let mut device_mut = device;
        let result = plugin.handle_packet(&packet, &mut device_mut).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_factory() {
        let factory = RemoteInputPluginFactory;
        assert_eq!(factory.name(), "remoteinput");

        let incoming = factory.incoming_capabilities();
        assert!(incoming.contains(&PACKET_TYPE_MOUSEPAD_REQUEST.to_string()));

        let outgoing = factory.outgoing_capabilities();
        assert!(outgoing.contains(&PACKET_TYPE_MOUSEPAD_KEYBOARDSTATE.to_string()));

        let plugin = factory.create();
        assert_eq!(plugin.name(), "remoteinput");
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = RemoteInputPlugin::new();
        let device = create_test_device();

        assert!(plugin.init(&device).await.is_ok());
        assert!(plugin.start().await.is_ok());
        assert!(plugin.stop().await.is_ok());
    }
}
