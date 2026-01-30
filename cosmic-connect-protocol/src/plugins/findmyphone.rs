//! Find My Phone Plugin
//!
//! This plugin allows locating devices by making them ring. It works
//! bidirectionally - can make a remote phone ring, or make the desktop
//! ring when requested by a paired device.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `cconnect.findmyphone.request` - Ring request (bidirectional)
//!
//! **Capabilities**:
//! - Incoming: `cconnect.findmyphone.request` - Receive ring requests
//! - Outgoing: `cconnect.findmyphone.request` - Send ring requests
//!
//! ## Behavior
//!
//! - Receiving a packet starts/stops ringing (toggle)
//! - Sending a packet makes the remote device ring
//! - Sound plays using system audio (PulseAudio/PipeWire)
//!
//! ## Sound Playback
//!
//! Tries multiple methods in order:
//! 1. `paplay` (PulseAudio/PipeWire) with system sounds
//! 2. `canberra-gtk-play` (freedesktop sound theme)
//! 3. `pw-play` (PipeWire native)
//! 4. Desktop notification as fallback
//!
//! ## References
//!
//! - [KDE Connect FindMyPhone](https://github.com/KDE/kdeconnect-android)
//! - [Valent Protocol](https://valent.andyholmes.ca/documentation/protocol.html)

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde_json::json;
use std::any::Any;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use super::{Plugin, PluginFactory};

/// Packet type for find my phone requests
pub const PACKET_TYPE_FINDMYPHONE_REQUEST: &str = "cconnect.findmyphone.request";

/// KDE Connect compatible packet type
const PACKET_TYPE_KDECONNECT_FINDMYPHONE: &str = "kdeconnect.findmyphone.request";

/// System sound files to try (in order of preference)
const SYSTEM_SOUNDS: &[&str] = &[
    "/usr/share/sounds/freedesktop/stereo/phone-incoming-call.oga",
    "/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga",
    "/usr/share/sounds/freedesktop/stereo/bell.oga",
    "/usr/share/sounds/gnome/default/alerts/drip.ogg",
    "/usr/share/sounds/Yaru/stereo/phone-incoming-call.oga",
];

/// Find My Phone plugin for locating devices
pub struct FindMyPhonePlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Whether the plugin is enabled
    enabled: bool,

    /// Whether currently ringing
    is_ringing: Arc<AtomicBool>,

    /// Current sound process (if playing)
    sound_process: Option<Child>,
}

impl FindMyPhonePlugin {
    /// Create a new Find My Phone plugin
    pub fn new() -> Self {
        Self {
            device_id: None,
            enabled: false,
            is_ringing: Arc::new(AtomicBool::new(false)),
            sound_process: None,
        }
    }

    /// Create a ring request packet
    ///
    /// This packet makes the remote device ring. Sending it again cancels the ring.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::findmyphone::FindMyPhonePlugin;
    ///
    /// let plugin = FindMyPhonePlugin::new();
    /// let packet = plugin.create_ring_request();
    /// assert_eq!(packet.packet_type, "cconnect.findmyphone.request");
    /// ```
    pub fn create_ring_request(&self) -> Packet {
        debug!("Creating ring request packet");
        Packet::new(PACKET_TYPE_FINDMYPHONE_REQUEST, json!({}))
    }

    /// Handle incoming ring request
    async fn handle_ring_request(&mut self, device: &Device) -> Result<()> {
        let currently_ringing = self.is_ringing.load(Ordering::SeqCst);

        if currently_ringing {
            info!("Stopping ring (requested by {})", device.name());
            self.stop_ringing();
        } else {
            info!("Starting ring (requested by {})", device.name());
            self.start_ringing();
        }

        Ok(())
    }

    /// Start playing the ring sound
    fn start_ringing(&mut self) {
        // Try sound players in order of preference
        let player_attempts: Vec<(&str, Option<Child>)> =
            if let Some(sound_path) = Self::find_sound_file() {
                vec![
                    ("paplay", Self::play_with_paplay(sound_path)),
                    ("canberra-gtk-play", Self::play_with_canberra(sound_path)),
                    ("pw-play", Self::play_with_pwplay(sound_path)),
                    ("sound event", Self::play_sound_event()),
                ]
            } else {
                vec![("sound event", Self::play_sound_event())]
            };

        for (player_name, child_option) in player_attempts {
            if let Some(child) = child_option {
                self.sound_process = Some(child);
                self.is_ringing.store(true, Ordering::SeqCst);
                info!("Ring started using {}", player_name);
                return;
            }
        }

        // Last resort: send notification
        Self::send_notification();
        self.is_ringing.store(true, Ordering::SeqCst);
        warn!("No sound player available, using notification fallback");
    }

    /// Stop the ring sound
    fn stop_ringing(&mut self) {
        if let Some(mut child) = self.sound_process.take() {
            if let Err(e) = child.kill() {
                debug!("Failed to kill sound process: {}", e);
            }
            let _ = child.wait();
        }
        self.is_ringing.store(false, Ordering::SeqCst);
        info!("Ring stopped");
    }

    /// Find an available system sound file
    fn find_sound_file() -> Option<&'static str> {
        SYSTEM_SOUNDS
            .iter()
            .copied()
            .find(|path| std::path::Path::new(path).exists())
    }

    /// Play sound using paplay (PulseAudio/PipeWire)
    fn play_with_paplay(sound_path: &str) -> Option<Child> {
        Command::new("paplay")
            .arg("--loop")
            .arg(sound_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()
    }

    /// Play sound using canberra-gtk-play
    fn play_with_canberra(sound_path: &str) -> Option<Child> {
        Command::new("canberra-gtk-play")
            .arg("-f")
            .arg(sound_path)
            .arg("-l")
            .arg("10") // Loop 10 times
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()
    }

    /// Play sound using pw-play (PipeWire)
    fn play_with_pwplay(sound_path: &str) -> Option<Child> {
        Command::new("pw-play")
            .arg(sound_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()
    }

    /// Play sound event using canberra (uses system theme)
    fn play_sound_event() -> Option<Child> {
        Command::new("canberra-gtk-play")
            .arg("-i")
            .arg("phone-incoming-call")
            .arg("-l")
            .arg("10")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()
    }

    /// Send desktop notification as fallback
    fn send_notification() {
        if let Err(e) = Command::new("notify-send")
            .arg("--urgency=critical")
            .arg("--icon=phone")
            .arg("Find My Device")
            .arg("Your device is being located!")
            .spawn()
        {
            error!("Failed to send notification: {}", e);
        }
    }

    /// Check if a ring request packet
    fn is_ring_request(packet: &Packet) -> bool {
        packet.is_type(PACKET_TYPE_FINDMYPHONE_REQUEST)
            || packet.is_type(PACKET_TYPE_KDECONNECT_FINDMYPHONE)
    }
}

impl Default for FindMyPhonePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for FindMyPhonePlugin {
    fn drop(&mut self) {
        // Ensure sound stops when plugin is dropped
        self.stop_ringing();
    }
}

#[async_trait]
impl Plugin for FindMyPhonePlugin {
    fn name(&self) -> &str {
        "findmyphone"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_FINDMYPHONE_REQUEST.to_string(),
            PACKET_TYPE_KDECONNECT_FINDMYPHONE.to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![PACKET_TYPE_FINDMYPHONE_REQUEST.to_string()]
    }

    async fn init(
        &mut self,
        device: &Device,
        _packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>,
    ) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!(
            "Find My Phone plugin initialized for device {}",
            device.name()
        );
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Find My Phone plugin started");
        self.enabled = true;
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Find My Phone plugin stopped");
        self.enabled = false;
        self.stop_ringing();
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("Find My Phone plugin is disabled, ignoring packet");
            return Ok(());
        }

        if Self::is_ring_request(packet) {
            self.handle_ring_request(device).await?;
        }

        Ok(())
    }
}

/// Factory for creating Find My Phone plugin instances
#[derive(Debug, Clone, Copy)]
pub struct FindMyPhonePluginFactory;

impl PluginFactory for FindMyPhonePluginFactory {
    fn name(&self) -> &str {
        "findmyphone"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_FINDMYPHONE_REQUEST.to_string(),
            PACKET_TYPE_KDECONNECT_FINDMYPHONE.to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![PACKET_TYPE_FINDMYPHONE_REQUEST.to_string()]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(FindMyPhonePlugin::new())
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
        let plugin = FindMyPhonePlugin::new();
        assert_eq!(plugin.name(), "findmyphone");
        assert!(plugin.device_id.is_none());
        assert!(!plugin.enabled);
        assert!(!plugin.is_ringing.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_plugin_initialization() {
        let mut plugin = FindMyPhonePlugin::new();
        let device = create_test_device();

        assert!(plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .is_ok());
        assert_eq!(plugin.device_id, Some(device.id().to_string()));
    }

    #[test]
    fn test_create_ring_request() {
        let plugin = FindMyPhonePlugin::new();
        let packet = plugin.create_ring_request();

        assert_eq!(packet.packet_type, "cconnect.findmyphone.request");
        assert!(packet.body.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_incoming_capabilities() {
        let plugin = FindMyPhonePlugin::new();
        let incoming = plugin.incoming_capabilities();

        assert_eq!(incoming.len(), 2);
        assert!(incoming.contains(&PACKET_TYPE_FINDMYPHONE_REQUEST.to_string()));
        assert!(incoming.contains(&PACKET_TYPE_KDECONNECT_FINDMYPHONE.to_string()));
    }

    #[test]
    fn test_outgoing_capabilities() {
        let plugin = FindMyPhonePlugin::new();
        let outgoing = plugin.outgoing_capabilities();

        assert_eq!(outgoing.len(), 1);
        assert!(outgoing.contains(&PACKET_TYPE_FINDMYPHONE_REQUEST.to_string()));
    }

    #[test]
    fn test_factory() {
        let factory = FindMyPhonePluginFactory;
        assert_eq!(factory.name(), "findmyphone");

        let outgoing = factory.outgoing_capabilities();
        assert!(outgoing.contains(&PACKET_TYPE_FINDMYPHONE_REQUEST.to_string()));

        let incoming = factory.incoming_capabilities();
        assert_eq!(incoming.len(), 2);

        let plugin = factory.create();
        assert_eq!(plugin.name(), "findmyphone");
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = FindMyPhonePlugin::new();
        let device = create_test_device();

        assert!(plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .is_ok());
        assert!(plugin.start().await.is_ok());
        assert!(plugin.enabled);
        assert!(plugin.stop().await.is_ok());
        assert!(!plugin.enabled);
    }

    #[test]
    fn test_is_ring_request() {
        let cconnect_packet = Packet::new(PACKET_TYPE_FINDMYPHONE_REQUEST, json!({}));
        let kdeconnect_packet = Packet::new(PACKET_TYPE_KDECONNECT_FINDMYPHONE, json!({}));
        let other_packet = Packet::new("other.packet.type", json!({}));

        assert!(FindMyPhonePlugin::is_ring_request(&cconnect_packet));
        assert!(FindMyPhonePlugin::is_ring_request(&kdeconnect_packet));
        assert!(!FindMyPhonePlugin::is_ring_request(&other_packet));
    }

    #[test]
    fn test_find_sound_file() {
        // This test just verifies the function doesn't panic
        // Actual result depends on system sound files
        let _result = FindMyPhonePlugin::find_sound_file();
    }

    #[tokio::test]
    async fn test_handle_ring_request() {
        let mut plugin = FindMyPhonePlugin::new();
        let device = create_test_device();

        plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .unwrap();
        plugin.start().await.unwrap();

        // Initially not ringing
        assert!(!plugin.is_ringing.load(Ordering::SeqCst));

        // Handle request - should toggle ring on
        // Note: This may or may not actually play sound depending on system
        let mut test_device = create_test_device();
        let packet = Packet::new(PACKET_TYPE_FINDMYPHONE_REQUEST, json!({}));
        let _ = plugin.handle_packet(&packet, &mut test_device).await;

        // Clean up
        plugin.stop_ringing();
        assert!(!plugin.is_ringing.load(Ordering::SeqCst));
    }
}
