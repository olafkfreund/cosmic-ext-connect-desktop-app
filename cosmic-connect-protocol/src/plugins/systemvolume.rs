//! System Volume Plugin
//!
//! Allows remote control of system volume and audio sinks using PipeWire/WirePlumber.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `cconnect.systemvolume.request` - Volume control request (incoming)
//! - `cconnect.systemvolume` - Sink list update (outgoing)
//!
//! **Capabilities**:
//! - Incoming: `cconnect.systemvolume.request`
//! - Outgoing: `cconnect.systemvolume`
//!
//! ## Packet Format
//!
//! **Request (incoming)**:
//! ```json
//! {
//!     "type": "cconnect.systemvolume.request",
//!     "body": {
//!         "name": "Sink Name",
//!         "volume": 75,
//!         "muted": false,
//!         "enabled": true,
//!         "requestSinks": false
//!     }
//! }
//! ```
//!
//! **Sink List (outgoing)**:
//! ```json
//! {
//!     "type": "cconnect.systemvolume",
//!     "body": {
//!         "sinkList": [
//!             {
//!                 "name": "Realtek USB Audio",
//!                 "description": "Front Speaker",
//!                 "volume": 100,
//!                 "muted": false,
//!                 "maxVolume": 150,
//!                 "enabled": true
//!             }
//!         ]
//!     }
//! }
//! ```

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

use super::audio_backend::{AudioBackend, AudioSink};
use super::{Plugin, PluginFactory};

/// Packet type for system volume requests (incoming)
pub const PACKET_TYPE_SYSTEMVOLUME_REQUEST: &str = "cconnect.systemvolume.request";

/// Packet type for sink list updates (outgoing)
pub const PACKET_TYPE_SYSTEMVOLUME: &str = "cconnect.systemvolume";

/// System volume request body (incoming)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemVolumeRequest {
    /// Name of the audio sink to control
    pub name: Option<String>,
    /// Volume level (0-100, can go higher for boost)
    pub volume: Option<i32>,
    /// Mute status
    pub muted: Option<bool>,
    /// Set as default/enabled sink
    pub enabled: Option<bool>,
    /// Request list of sinks from this device
    #[serde(rename = "requestSinks", default)]
    pub request_sinks: bool,
}

/// Sink information for protocol (outgoing)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SinkInfo {
    /// Unique sink name/identifier
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Current volume (0-100+)
    pub volume: i32,
    /// Whether the sink is muted
    pub muted: bool,
    /// Maximum volume (typically 150 for boost)
    #[serde(rename = "maxVolume")]
    pub max_volume: i32,
    /// Whether this is the active/default sink
    pub enabled: bool,
}

impl From<AudioSink> for SinkInfo {
    fn from(sink: AudioSink) -> Self {
        Self {
            name: sink.id.to_string(), // Use ID as unique identifier
            description: sink.name,
            volume: sink.volume,
            muted: sink.muted,
            max_volume: sink.max_volume,
            enabled: sink.is_default,
        }
    }
}

/// Sink list response body (outgoing)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SinkListResponse {
    /// List of available sinks
    #[serde(rename = "sinkList")]
    pub sink_list: Vec<SinkInfo>,
}

/// System Volume plugin
///
/// Provides remote control of system volume and audio sinks.
///
/// ## Features
///
/// - List and control audio sinks
/// - Set volume and mute status
/// - Thread-safe sink state storage
/// - Public API for UI integration
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_protocol::plugins::systemvolume::SystemVolumePlugin;
/// use cosmic_connect_protocol::Plugin;
///
/// let plugin = SystemVolumePlugin::new();
/// assert_eq!(plugin.name(), "systemvolume");
/// assert_eq!(plugin.sink_count(), 0);
/// ```
pub struct SystemVolumePlugin {
    device_id: Option<String>,
    packet_sender: Option<mpsc::Sender<(String, Packet)>>,
    /// Thread-safe cache of known sinks (keyed by name from protocol)
    sinks: Arc<RwLock<HashMap<String, SinkInfo>>>,
    /// Mapping from protocol name to PipeWire sink ID
    sink_id_map: Arc<RwLock<HashMap<String, u32>>>,
}

impl SystemVolumePlugin {
    /// Create a new System Volume plugin
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::systemvolume::SystemVolumePlugin;
    ///
    /// let plugin = SystemVolumePlugin::new();
    /// assert_eq!(plugin.sink_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            device_id: None,
            packet_sender: None,
            sinks: Arc::new(RwLock::new(HashMap::new())),
            sink_id_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get all cached audio sinks
    ///
    /// Returns a copy of all known sinks from the last update.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::systemvolume::SystemVolumePlugin;
    ///
    /// let plugin = SystemVolumePlugin::new();
    /// let sinks = plugin.get_sinks();
    /// assert!(sinks.is_empty());
    /// ```
    pub fn get_sinks(&self) -> Vec<SinkInfo> {
        self.sinks
            .try_read()
            .map(|guard| guard.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get a specific sink by name
    ///
    /// # Parameters
    ///
    /// - `name`: The sink name/identifier
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::systemvolume::SystemVolumePlugin;
    ///
    /// let plugin = SystemVolumePlugin::new();
    /// assert!(plugin.get_sink("50").is_none());
    /// ```
    pub fn get_sink(&self, name: &str) -> Option<SinkInfo> {
        self.sinks
            .try_read()
            .ok()
            .and_then(|guard| guard.get(name).cloned())
    }

    /// Get the default/active sink
    ///
    /// Returns the sink marked as enabled/default, if any.
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::systemvolume::SystemVolumePlugin;
    ///
    /// let plugin = SystemVolumePlugin::new();
    /// assert!(plugin.get_default_sink().is_none());
    /// ```
    pub fn get_default_sink(&self) -> Option<SinkInfo> {
        self.sinks
            .try_read()
            .ok()
            .and_then(|guard| guard.values().find(|s| s.enabled).cloned())
    }

    /// Get volume for a specific sink
    ///
    /// # Parameters
    ///
    /// - `name`: The sink name/identifier
    ///
    /// # Returns
    ///
    /// Volume level (0-150) or None if sink not found
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::systemvolume::SystemVolumePlugin;
    ///
    /// let plugin = SystemVolumePlugin::new();
    /// assert!(plugin.get_volume("50").is_none());
    /// ```
    pub fn get_volume(&self, name: &str) -> Option<i32> {
        self.get_sink(name).map(|s| s.volume)
    }

    /// Check if a sink is muted
    ///
    /// # Parameters
    ///
    /// - `name`: The sink name/identifier
    ///
    /// # Returns
    ///
    /// Mute status or None if sink not found
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::systemvolume::SystemVolumePlugin;
    ///
    /// let plugin = SystemVolumePlugin::new();
    /// assert!(plugin.is_muted("50").is_none());
    /// ```
    pub fn is_muted(&self, name: &str) -> Option<bool> {
        self.get_sink(name).map(|s| s.muted)
    }

    /// Get the number of cached sinks
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::systemvolume::SystemVolumePlugin;
    ///
    /// let plugin = SystemVolumePlugin::new();
    /// assert_eq!(plugin.sink_count(), 0);
    /// ```
    pub fn sink_count(&self) -> usize {
        self.sinks.try_read().map(|guard| guard.len()).unwrap_or(0)
    }

    /// Check if any sinks are available
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::systemvolume::SystemVolumePlugin;
    ///
    /// let plugin = SystemVolumePlugin::new();
    /// assert!(!plugin.has_sinks());
    /// ```
    pub fn has_sinks(&self) -> bool {
        self.sink_count() > 0
    }

    /// Update the sink cache (internal use)
    fn update_sink_cache(&self, sinks: Vec<SinkInfo>, id_map: HashMap<String, u32>) {
        if let Ok(mut guard) = self.sinks.try_write() {
            guard.clear();
            guard.extend(sinks.into_iter().map(|s| (s.name.clone(), s)));
        }
        if let Ok(mut guard) = self.sink_id_map.try_write() {
            *guard = id_map;
        }
    }

    /// Get sink ID from cache for volume control
    fn get_sink_id(&self, name: &str) -> Option<u32> {
        // Try to parse as ID first
        if let Ok(id) = name.parse::<u32>() {
            return Some(id);
        }
        // Look up in ID map
        self.sink_id_map
            .try_read()
            .ok()
            .and_then(|guard| guard.get(name).copied())
    }

    /// Send sink list to remote device
    async fn send_sink_list(&mut self) -> Result<()> {
        let sinks = AudioBackend::list_sinks();

        // Build ID map and sink info list
        let id_map: HashMap<String, u32> = sinks.iter().map(|s| (s.id.to_string(), s.id)).collect();
        let sink_list: Vec<SinkInfo> = sinks.into_iter().map(SinkInfo::from).collect();

        // Update cache
        self.update_sink_cache(sink_list.clone(), id_map);

        info!("Sending {} sinks to remote device", sink_list.len());

        let response = SinkListResponse { sink_list };
        let packet = Packet::new(PACKET_TYPE_SYSTEMVOLUME, serde_json::to_value(response)?);

        if let (Some(sender), Some(device_id)) = (&self.packet_sender, &self.device_id) {
            sender
                .send((device_id.clone(), packet))
                .await
                .map_err(|e| {
                    crate::ProtocolError::Transport(format!("Failed to send packet: {}", e))
                })?;
        }

        Ok(())
    }

    /// Handle volume request from remote device
    async fn handle_volume_request(&mut self, packet: &Packet) -> Result<()> {
        let request: SystemVolumeRequest =
            serde_json::from_value(packet.body.clone()).map_err(|e| {
                crate::ProtocolError::InvalidPacket(format!(
                    "Failed to parse volume request: {}",
                    e
                ))
            })?;

        debug!("Received volume request: {:?}", request);

        // Handle sink list request
        if request.request_sinks {
            info!("Remote device requested audio sink list");
            self.send_sink_list().await?;
            return Ok(());
        }

        // Find the sink by name
        let sink_id = if let Some(name) = &request.name {
            // Use cached ID lookup
            self.get_sink_id(name)
                .or_else(|| AudioBackend::find_sink_by_name(name).map(|s| s.id))
        } else {
            // Use default sink if no name specified
            AudioBackend::get_default_sink_id()
        };

        let Some(sink_id) = sink_id else {
            warn!("Could not find sink: {:?}", request.name);
            return Ok(());
        };

        // Apply volume change
        if let Some(volume) = request.volume {
            info!("Setting volume to {}% for sink {}", volume, sink_id);
            if !AudioBackend::set_volume(sink_id, volume) {
                warn!("Failed to set volume for sink {}", sink_id);
            }
        }

        // Apply mute change
        if let Some(muted) = request.muted {
            info!("Setting mute to {} for sink {}", muted, sink_id);
            if !AudioBackend::set_mute(sink_id, muted) {
                warn!("Failed to set mute for sink {}", sink_id);
            }
        }

        // Send updated sink list after changes
        self.send_sink_list().await?;

        Ok(())
    }

    /// Create a volume control request packet
    ///
    /// # Parameters
    ///
    /// - `name`: Sink name to control (None for default)
    /// - `volume`: Volume level to set (0-150)
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::systemvolume::SystemVolumePlugin;
    ///
    /// let plugin = SystemVolumePlugin::new();
    /// let packet = plugin.create_volume_request(Some("50".to_string()), 75);
    /// assert_eq!(packet.packet_type, "cconnect.systemvolume.request");
    /// ```
    pub fn create_volume_request(&self, name: Option<String>, volume: i32) -> Packet {
        let body = serde_json::json!({
            "name": name,
            "volume": volume,
            "requestSinks": false
        });
        Packet::new(PACKET_TYPE_SYSTEMVOLUME_REQUEST, body)
    }

    /// Create a mute control request packet
    ///
    /// # Parameters
    ///
    /// - `name`: Sink name to control (None for default)
    /// - `muted`: Mute status to set
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::systemvolume::SystemVolumePlugin;
    ///
    /// let plugin = SystemVolumePlugin::new();
    /// let packet = plugin.create_mute_request(Some("50".to_string()), true);
    /// assert_eq!(packet.packet_type, "cconnect.systemvolume.request");
    /// ```
    pub fn create_mute_request(&self, name: Option<String>, muted: bool) -> Packet {
        let body = serde_json::json!({
            "name": name,
            "muted": muted,
            "requestSinks": false
        });
        Packet::new(PACKET_TYPE_SYSTEMVOLUME_REQUEST, body)
    }

    /// Create a sink list request packet
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_protocol::plugins::systemvolume::SystemVolumePlugin;
    ///
    /// let plugin = SystemVolumePlugin::new();
    /// let packet = plugin.create_sink_list_request();
    /// assert_eq!(packet.packet_type, "cconnect.systemvolume.request");
    /// ```
    pub fn create_sink_list_request(&self) -> Packet {
        let body = serde_json::json!({
            "requestSinks": true
        });
        Packet::new(PACKET_TYPE_SYSTEMVOLUME_REQUEST, body)
    }
}

impl Default for SystemVolumePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for SystemVolumePlugin {
    fn name(&self) -> &str {
        "systemvolume"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_SYSTEMVOLUME_REQUEST.to_string(),
            "kdeconnect.systemvolume.request".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_SYSTEMVOLUME.to_string(),
            "kdeconnect.systemvolume".to_string(),
        ]
    }

    async fn init(
        &mut self,
        device: &Device,
        packet_sender: mpsc::Sender<(String, Packet)>,
    ) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        self.packet_sender = Some(packet_sender);

        // Check if audio backend is available
        if !AudioBackend::is_available() {
            warn!("wpctl not available - system volume control will not work");
        }

        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("SystemVolume plugin started");

        // Send initial sink list to remote device
        if AudioBackend::is_available() {
            if let Err(e) = self.send_sink_list().await {
                warn!("Failed to send initial sink list: {}", e);
            }
        }

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("SystemVolume plugin stopped");
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, _device: &mut Device) -> Result<()> {
        if packet.is_type(PACKET_TYPE_SYSTEMVOLUME_REQUEST)
            || packet.is_type("kdeconnect.systemvolume.request")
        {
            self.handle_volume_request(packet).await
        } else {
            Ok(())
        }
    }
}

/// Factory for creating SystemVolumePlugin instances
pub struct SystemVolumePluginFactory;

impl PluginFactory for SystemVolumePluginFactory {
    fn name(&self) -> &str {
        "systemvolume"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_SYSTEMVOLUME_REQUEST.to_string(),
            "kdeconnect.systemvolume.request".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            PACKET_TYPE_SYSTEMVOLUME.to_string(),
            "kdeconnect.systemvolume".to_string(),
        ]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(SystemVolumePlugin::new())
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

    fn create_test_sink(
        id: u32,
        name: &str,
        volume: i32,
        muted: bool,
        is_default: bool,
    ) -> SinkInfo {
        SinkInfo {
            name: id.to_string(),
            description: name.to_string(),
            volume,
            muted,
            max_volume: 150,
            enabled: is_default,
        }
    }

    #[test]
    fn test_plugin_creation() {
        let plugin = SystemVolumePlugin::new();
        assert_eq!(plugin.name(), "systemvolume");
        assert_eq!(plugin.sink_count(), 0);
        assert!(!plugin.has_sinks());
    }

    #[test]
    fn test_sink_info_from_audio_sink() {
        let audio_sink = AudioSink {
            id: 50,
            name: "Test Speaker".to_string(),
            volume: 75,
            muted: false,
            is_default: true,
            max_volume: 150,
        };

        let sink_info: SinkInfo = audio_sink.into();
        assert_eq!(sink_info.name, "50");
        assert_eq!(sink_info.description, "Test Speaker");
        assert_eq!(sink_info.volume, 75);
        assert!(!sink_info.muted);
        assert!(sink_info.enabled);
        assert_eq!(sink_info.max_volume, 150);
    }

    #[test]
    fn test_parse_volume_request() {
        let json = serde_json::json!({
            "name": "50",
            "volume": 80,
            "muted": false,
            "requestSinks": false
        });

        let request: SystemVolumeRequest = serde_json::from_value(json).unwrap();
        assert_eq!(request.name, Some("50".to_string()));
        assert_eq!(request.volume, Some(80));
        assert_eq!(request.muted, Some(false));
        assert!(!request.request_sinks);
    }

    #[test]
    fn test_parse_request_sinks() {
        let json = serde_json::json!({
            "requestSinks": true
        });

        let request: SystemVolumeRequest = serde_json::from_value(json).unwrap();
        assert!(request.request_sinks);
        assert!(request.name.is_none());
        assert!(request.volume.is_none());
    }

    #[test]
    fn test_capabilities() {
        let plugin = SystemVolumePlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 2);
        assert!(incoming.contains(&PACKET_TYPE_SYSTEMVOLUME_REQUEST.to_string()));
        assert!(incoming.contains(&"kdeconnect.systemvolume.request".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 2);
        assert!(outgoing.contains(&PACKET_TYPE_SYSTEMVOLUME.to_string()));
        assert!(outgoing.contains(&"kdeconnect.systemvolume".to_string()));
    }

    #[test]
    fn test_sink_cache() {
        let plugin = SystemVolumePlugin::new();

        // Add sinks to cache
        let sinks = vec![
            create_test_sink(50, "Speaker", 75, false, true),
            create_test_sink(51, "Headphones", 50, true, false),
        ];
        let mut id_map = HashMap::new();
        id_map.insert("50".to_string(), 50);
        id_map.insert("51".to_string(), 51);

        plugin.update_sink_cache(sinks, id_map);

        // Verify cache
        assert_eq!(plugin.sink_count(), 2);
        assert!(plugin.has_sinks());

        // Get specific sink
        let sink = plugin.get_sink("50").unwrap();
        assert_eq!(sink.description, "Speaker");
        assert_eq!(sink.volume, 75);
        assert!(!sink.muted);
        assert!(sink.enabled);

        // Get default sink
        let default = plugin.get_default_sink().unwrap();
        assert_eq!(default.name, "50");

        // Get volume
        assert_eq!(plugin.get_volume("50"), Some(75));
        assert_eq!(plugin.get_volume("51"), Some(50));

        // Check muted
        assert_eq!(plugin.is_muted("50"), Some(false));
        assert_eq!(plugin.is_muted("51"), Some(true));
    }

    #[test]
    fn test_get_sinks() {
        let plugin = SystemVolumePlugin::new();

        let sinks = vec![
            create_test_sink(50, "Speaker", 75, false, true),
            create_test_sink(51, "Headphones", 50, false, false),
            create_test_sink(52, "Monitor", 100, false, false),
        ];
        let mut id_map = HashMap::new();
        id_map.insert("50".to_string(), 50);
        id_map.insert("51".to_string(), 51);
        id_map.insert("52".to_string(), 52);

        plugin.update_sink_cache(sinks, id_map);

        let all_sinks = plugin.get_sinks();
        assert_eq!(all_sinks.len(), 3);
    }

    #[test]
    fn test_get_sink_not_found() {
        let plugin = SystemVolumePlugin::new();
        assert!(plugin.get_sink("nonexistent").is_none());
        assert!(plugin.get_volume("nonexistent").is_none());
        assert!(plugin.is_muted("nonexistent").is_none());
    }

    #[test]
    fn test_get_default_sink_none() {
        let plugin = SystemVolumePlugin::new();

        // Add sinks with no default
        let sinks = vec![
            create_test_sink(50, "Speaker", 75, false, false),
            create_test_sink(51, "Headphones", 50, false, false),
        ];
        let mut id_map = HashMap::new();
        id_map.insert("50".to_string(), 50);
        id_map.insert("51".to_string(), 51);

        plugin.update_sink_cache(sinks, id_map);

        assert!(plugin.get_default_sink().is_none());
    }

    #[test]
    fn test_get_sink_id() {
        let plugin = SystemVolumePlugin::new();

        // Test parsing numeric ID directly
        assert_eq!(plugin.get_sink_id("50"), Some(50));
        assert_eq!(plugin.get_sink_id("123"), Some(123));

        // Test non-numeric lookup (returns None when not in cache)
        assert!(plugin.get_sink_id("Speaker").is_none());

        // Add to cache and verify lookup works via get_sink_id
        let sinks = vec![create_test_sink(99, "Speaker", 75, false, true)];
        let mut id_map = HashMap::new();
        id_map.insert("Speaker".to_string(), 99);
        plugin.update_sink_cache(sinks, id_map);

        // Now the name lookup should work through get_sink_id
        assert_eq!(plugin.get_sink_id("Speaker"), Some(99));
    }

    #[test]
    fn test_create_volume_request() {
        let plugin = SystemVolumePlugin::new();
        let packet = plugin.create_volume_request(Some("50".to_string()), 75);

        assert_eq!(packet.packet_type, PACKET_TYPE_SYSTEMVOLUME_REQUEST);
        assert_eq!(packet.body.get("name").and_then(|v| v.as_str()), Some("50"));
        assert_eq!(packet.body.get("volume").and_then(|v| v.as_i64()), Some(75));
        assert_eq!(
            packet.body.get("requestSinks").and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn test_create_mute_request() {
        let plugin = SystemVolumePlugin::new();
        let packet = plugin.create_mute_request(Some("50".to_string()), true);

        assert_eq!(packet.packet_type, PACKET_TYPE_SYSTEMVOLUME_REQUEST);
        assert_eq!(packet.body.get("name").and_then(|v| v.as_str()), Some("50"));
        assert_eq!(
            packet.body.get("muted").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn test_create_sink_list_request() {
        let plugin = SystemVolumePlugin::new();
        let packet = plugin.create_sink_list_request();

        assert_eq!(packet.packet_type, PACKET_TYPE_SYSTEMVOLUME_REQUEST);
        assert_eq!(
            packet.body.get("requestSinks").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = SystemVolumePlugin::new();
        let device = create_test_device();

        plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .unwrap();
        assert!(plugin.device_id.is_some());

        plugin.start().await.unwrap();
        plugin.stop().await.unwrap();
    }

    #[test]
    fn test_sink_list_response_serialization() {
        let response = SinkListResponse {
            sink_list: vec![SinkInfo {
                name: "50".to_string(),
                description: "Test Speaker".to_string(),
                volume: 75,
                muted: false,
                max_volume: 150,
                enabled: true,
            }],
        };

        let json = serde_json::to_value(&response).unwrap();
        let sink_list = json.get("sinkList").unwrap().as_array().unwrap();
        assert_eq!(sink_list.len(), 1);
        assert_eq!(sink_list[0]["name"], "50");
        assert_eq!(sink_list[0]["volume"], 75);
    }
}
