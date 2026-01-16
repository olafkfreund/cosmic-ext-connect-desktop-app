//! MPRIS Plugin
//!
//! Enables remote control of media players between CConnect devices using MPRIS2
//! (Media Player Remote Interfacing Specification). Supports player discovery, playback
//! control, metadata synchronization, and album art transfer.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - Incoming: `cconnect.mpris`, `cconnect.mpris.request`
//! - Outgoing: `cconnect.mpris`, `cconnect.mpris.request`
//!
//! **Capabilities**: `cconnect.mpris`
//!
//! ## Player Discovery
//!
//! List available media players on the device:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.mpris",
//!     "body": {
//!         "playerList": ["vlc", "spotify"],
//!         "supportAlbumArtPayload": true
//!     }
//! }
//! ```
//!
//! ## Player Status
//!
//! Report current playback state and position:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.mpris",
//!     "body": {
//!         "player": "spotify",
//!         "isPlaying": true,
//!         "pos": 45000,
//!         "length": 180000,
//!         "volume": 75,
//!         "artist": "Artist Name",
//!         "title": "Track Title",
//!         "album": "Album Name",
//!         "albumArtUrl": "/path/to/art.jpg",
//!         "canPlay": true,
//!         "canPause": true,
//!         "canGoNext": true,
//!         "canGoPrevious": true,
//!         "canSeek": true,
//!         "loopStatus": "None",
//!         "shuffle": false
//!     }
//! }
//! ```
//!
//! ## Control Commands
//!
//! ### Request Player List
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.mpris.request",
//!     "body": {
//!         "requestPlayerList": true
//!     }
//! }
//! ```
//!
//! ### Request Now Playing
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.mpris.request",
//!     "body": {
//!         "player": "spotify",
//!         "requestNowPlaying": true
//!     }
//! }
//! ```
//!
//! ### Playback Control
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.mpris.request",
//!     "body": {
//!         "player": "spotify",
//!         "action": "PlayPause"
//!     }
//! }
//! ```
//!
//! Actions: "Play", "Pause", "PlayPause", "Stop", "Next", "Previous"
//!
//! ### Seek
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.mpris.request",
//!     "body": {
//!         "player": "spotify",
//!         "Seek": 5000000
//!     }
//! }
//! ```
//!
//! Note: Seek offset in microseconds (relative)
//!
//! ### Set Position
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.mpris.request",
//!     "body": {
//!         "player": "spotify",
//!         "SetPosition": 60000
//!     }
//! }
//! ```
//!
//! Note: Position in milliseconds (absolute)
//!
//! ### Volume Control
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.mpris.request",
//!     "body": {
//!         "player": "spotify",
//!         "setVolume": 85
//!     }
//! }
//! ```
//!
//! ### Shuffle/Loop Control
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.mpris.request",
//!     "body": {
//!         "player": "spotify",
//!         "setShuffle": true,
//!         "setLoopStatus": "Playlist"
//!     }
//! }
//! ```
//!
//! Loop status: "None", "Track", "Playlist"
//!
//! ## Album Art Transfer
//!
//! Album art is transferred via TCP payload:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.mpris",
//!     "body": {
//!         "transferringAlbumArt": true,
//!         "player": "spotify"
//!     },
//!     "payloadSize": 204800,
//!     "payloadTransferInfo": {
//!         "port": 1739
//!     }
//! }
//! ```
//!
//! ## Example
//!
//! ```rust,ignore
//! use cosmic_connect_core::plugins::mpris::*;
//! use cosmic_connect_core::{Plugin, PluginManager};
//!
//! // Create and register plugin
//! let mut manager = PluginManager::new();
//! manager.register(Box::new(MprisPlugin::new()))?;
//!
//! // Initialize with device
//! manager.init_all(&device).await?;
//! manager.start_all().await?;
//!
//! // Send player list
//! let plugin = MprisPlugin::new();
//! let packet = plugin.create_player_list_packet(vec!["vlc".to_string(), "spotify".to_string()]);
//! // Send packet...
//!
//! // Send now playing
//! let metadata = PlayerMetadata {
//!     artist: Some("Artist".to_string()),
//!     title: Some("Title".to_string()),
//!     album: Some("Album".to_string()),
//!     ..Default::default()
//! };
//! let status = PlayerStatus {
//!     is_playing: true,
//!     position: 45000,
//!     length: 180000,
//!     volume: 75,
//!     ..Default::default()
//! };
//! let packet = plugin.create_status_packet("spotify".to_string(), status, metadata);
//! // Send packet...
//! ```
//!
//! ## References
//!
//! - [Valent Protocol Documentation](https://valent.andyholmes.ca/documentation/protocol.html)
//! - [MPRIS2 Specification](https://specifications.freedesktop.org/mpris-spec/latest/)

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

use super::{Plugin, PluginFactory};

/// Loop status for media playback
///
/// Indicates the repeat/loop mode of the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LoopStatus {
    /// No looping
    #[default]
    None,
    /// Repeat current track
    Track,
    /// Repeat entire playlist
    Playlist,
}

impl LoopStatus {
    /// Convert loop status to string for protocol
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Track => "Track",
            Self::Playlist => "Playlist",
        }
    }

    /// Parse loop status from string
    pub fn parse_str(s: &str) -> Self {
        match s {
            "Track" => Self::Track,
            "Playlist" => Self::Playlist,
            _ => Self::None,
        }
    }
}

/// Playback control action
///
/// Commands for controlling media player playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackAction {
    /// Start playback
    Play,
    /// Pause playback
    Pause,
    /// Toggle play/pause
    PlayPause,
    /// Stop playback
    Stop,
    /// Next track
    Next,
    /// Previous track
    Previous,
}

impl PlaybackAction {
    /// Convert action to string for protocol
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Play => "Play",
            Self::Pause => "Pause",
            Self::PlayPause => "PlayPause",
            Self::Stop => "Stop",
            Self::Next => "Next",
            Self::Previous => "Previous",
        }
    }

    /// Parse action from string
    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "Play" => Some(Self::Play),
            "Pause" => Some(Self::Pause),
            "PlayPause" => Some(Self::PlayPause),
            "Stop" => Some(Self::Stop),
            "Next" => Some(Self::Next),
            "Previous" => Some(Self::Previous),
            _ => None,
        }
    }
}

/// Media player capabilities
///
/// Indicates which operations the player supports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerCapabilities {
    /// Can start playback
    pub can_play: bool,
    /// Can pause playback
    pub can_pause: bool,
    /// Can skip to next track
    pub can_go_next: bool,
    /// Can skip to previous track
    pub can_go_previous: bool,
    /// Can seek within track
    pub can_seek: bool,
}

impl Default for PlayerCapabilities {
    fn default() -> Self {
        Self {
            can_play: true,
            can_pause: true,
            can_go_next: true,
            can_go_previous: true,
            can_seek: true,
        }
    }
}

/// Media player metadata
///
/// Track information and artwork.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerMetadata {
    /// Track artist
    pub artist: Option<String>,
    /// Track title
    pub title: Option<String>,
    /// Album name
    pub album: Option<String>,
    /// Album art URL/path
    pub album_art_url: Option<String>,
}

/// Media player status
///
/// Current playback state and position.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerStatus {
    /// Currently playing
    pub is_playing: bool,
    /// Current position in milliseconds
    pub position: i64,
    /// Track length in milliseconds
    pub length: i64,
    /// Volume (0-100)
    pub volume: i32,
    /// Loop/repeat status
    pub loop_status: LoopStatus,
    /// Shuffle enabled
    pub shuffle: bool,
    /// Player capabilities
    pub capabilities: PlayerCapabilities,
}

impl Default for PlayerStatus {
    fn default() -> Self {
        Self {
            is_playing: false,
            position: 0,
            length: 0,
            volume: 100,
            loop_status: LoopStatus::None,
            shuffle: false,
            capabilities: PlayerCapabilities::default(),
        }
    }
}

/// Complete player state
///
/// Combines status and metadata for a player.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerState {
    /// Player name/identifier
    pub name: String,
    /// Player status
    pub status: PlayerStatus,
    /// Track metadata
    pub metadata: PlayerMetadata,
}

/// MPRIS plugin for media player control
///
/// Handles `cconnect.mpris` packets for controlling and monitoring media
/// players via MPRIS2. Supports player discovery, playback control, metadata
/// synchronization, and album art transfer.
///
/// ## Features
///
/// - Media player discovery
/// - Playback control (play, pause, stop, next, previous)
/// - Position seeking (relative and absolute)
/// - Volume control
/// - Shuffle and loop status
/// - Now-playing metadata
/// - Album art transfer
/// - Player capabilities reporting
/// - Thread-safe state management
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_core::plugins::mpris::MprisPlugin;
/// use cosmic_connect_core::Plugin;
///
/// let plugin = MprisPlugin::new();
/// assert_eq!(plugin.name(), "mpris");
/// ```
#[derive(Debug)]
pub struct MprisPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Map of player name to player state
    players: Arc<RwLock<HashMap<String, PlayerState>>>,

    /// Whether album art payloads are supported
    support_album_art: bool,
}

impl MprisPlugin {
    /// Create a new MPRIS plugin
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::mpris::MprisPlugin;
    ///
    /// let plugin = MprisPlugin::new();
    /// ```
    pub fn new() -> Self {
        Self {
            device_id: None,
            players: Arc::new(RwLock::new(HashMap::new())),
            support_album_art: true,
        }
    }

    /// Create a player list packet
    ///
    /// Announces available media players on the device.
    ///
    /// # Parameters
    ///
    /// - `players`: List of player names/identifiers
    ///
    /// # Returns
    ///
    /// Packet with player list
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::mpris::MprisPlugin;
    ///
    /// let plugin = MprisPlugin::new();
    /// let packet = plugin.create_player_list_packet(vec![
    ///     "vlc".to_string(),
    ///     "spotify".to_string(),
    /// ]);
    /// assert_eq!(packet.packet_type, "cconnect.mpris");
    /// ```
    pub fn create_player_list_packet(&self, players: Vec<String>) -> Packet {
        Packet::new(
            "cconnect.mpris",
            json!({
                "playerList": players,
                "supportAlbumArtPayload": self.support_album_art
            }),
        )
    }

    /// Create a player status packet
    ///
    /// Reports current playback state and metadata.
    ///
    /// # Parameters
    ///
    /// - `player`: Player name/identifier
    /// - `status`: Player status (playing, position, volume, etc.)
    /// - `metadata`: Track metadata (artist, title, album)
    ///
    /// # Returns
    ///
    /// Packet with player status
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::mpris::*;
    ///
    /// let plugin = MprisPlugin::new();
    /// let status = PlayerStatus {
    ///     is_playing: true,
    ///     position: 30000,
    ///     length: 180000,
    ///     volume: 75,
    ///     ..Default::default()
    /// };
    /// let metadata = PlayerMetadata {
    ///     artist: Some("Artist".to_string()),
    ///     title: Some("Title".to_string()),
    ///     album: Some("Album".to_string()),
    ///     ..Default::default()
    /// };
    ///
    /// let packet = plugin.create_status_packet("vlc".to_string(), status, metadata);
    /// assert_eq!(packet.packet_type, "cconnect.mpris");
    /// ```
    pub fn create_status_packet(
        &self,
        player: String,
        status: PlayerStatus,
        metadata: PlayerMetadata,
    ) -> Packet {
        let mut body = json!({
            "player": player,
            "isPlaying": status.is_playing,
            "pos": status.position,
            "length": status.length,
            "volume": status.volume,
            "loopStatus": status.loop_status.as_str(),
            "shuffle": status.shuffle,
            "canPlay": status.capabilities.can_play,
            "canPause": status.capabilities.can_pause,
            "canGoNext": status.capabilities.can_go_next,
            "canGoPrevious": status.capabilities.can_go_previous,
            "canSeek": status.capabilities.can_seek,
        });

        // Add optional metadata fields
        if let Some(artist) = metadata.artist {
            body["artist"] = json!(artist);
        }
        if let Some(title) = metadata.title {
            body["title"] = json!(title);
        }
        if let Some(album) = metadata.album {
            body["album"] = json!(album);
        }
        if let Some(album_art_url) = metadata.album_art_url {
            body["albumArtUrl"] = json!(album_art_url);
        }

        Packet::new("cconnect.mpris", body)
    }

    /// Create a request player list packet
    ///
    /// Requests the remote device to send its player list.
    ///
    /// # Returns
    ///
    /// Request packet
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::mpris::MprisPlugin;
    ///
    /// let plugin = MprisPlugin::new();
    /// let packet = plugin.create_request_player_list_packet();
    /// assert_eq!(packet.packet_type, "cconnect.mpris.request");
    /// ```
    pub fn create_request_player_list_packet(&self) -> Packet {
        Packet::new(
            "cconnect.mpris.request",
            json!({ "requestPlayerList": true }),
        )
    }

    /// Create a request now playing packet
    ///
    /// Requests current track info from a specific player.
    ///
    /// # Parameters
    ///
    /// - `player`: Player name/identifier
    ///
    /// # Returns
    ///
    /// Request packet
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::mpris::MprisPlugin;
    ///
    /// let plugin = MprisPlugin::new();
    /// let packet = plugin.create_request_now_playing_packet("spotify".to_string());
    /// assert_eq!(packet.packet_type, "cconnect.mpris.request");
    /// ```
    pub fn create_request_now_playing_packet(&self, player: String) -> Packet {
        Packet::new(
            "cconnect.mpris.request",
            json!({
                "player": player,
                "requestNowPlaying": true
            }),
        )
    }

    /// Create a playback control packet
    ///
    /// Sends playback command to a player.
    ///
    /// # Parameters
    ///
    /// - `player`: Player name/identifier
    /// - `action`: Playback action to perform
    ///
    /// # Returns
    ///
    /// Control packet
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::mpris::*;
    ///
    /// let plugin = MprisPlugin::new();
    /// let packet = plugin.create_control_packet("vlc".to_string(), PlaybackAction::PlayPause);
    /// assert_eq!(packet.packet_type, "cconnect.mpris.request");
    /// ```
    pub fn create_control_packet(&self, player: String, action: PlaybackAction) -> Packet {
        Packet::new(
            "cconnect.mpris.request",
            json!({
                "player": player,
                "action": action.as_str()
            }),
        )
    }

    /// Create a seek packet
    ///
    /// Seeks relative to current position.
    ///
    /// # Parameters
    ///
    /// - `player`: Player name/identifier
    /// - `offset_microseconds`: Offset in microseconds (can be negative)
    ///
    /// # Returns
    ///
    /// Seek packet
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::mpris::MprisPlugin;
    ///
    /// let plugin = MprisPlugin::new();
    /// // Seek forward 5 seconds
    /// let packet = plugin.create_seek_packet("vlc".to_string(), 5_000_000);
    /// assert_eq!(packet.packet_type, "cconnect.mpris.request");
    /// ```
    pub fn create_seek_packet(&self, player: String, offset_microseconds: i64) -> Packet {
        Packet::new(
            "cconnect.mpris.request",
            json!({
                "player": player,
                "Seek": offset_microseconds
            }),
        )
    }

    /// Create a set position packet
    ///
    /// Sets absolute playback position.
    ///
    /// # Parameters
    ///
    /// - `player`: Player name/identifier
    /// - `position_milliseconds`: Absolute position in milliseconds
    ///
    /// # Returns
    ///
    /// Set position packet
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::mpris::MprisPlugin;
    ///
    /// let plugin = MprisPlugin::new();
    /// // Set position to 1 minute
    /// let packet = plugin.create_set_position_packet("spotify".to_string(), 60000);
    /// assert_eq!(packet.packet_type, "cconnect.mpris.request");
    /// ```
    pub fn create_set_position_packet(&self, player: String, position_milliseconds: i64) -> Packet {
        Packet::new(
            "cconnect.mpris.request",
            json!({
                "player": player,
                "SetPosition": position_milliseconds
            }),
        )
    }

    /// Create a set volume packet
    ///
    /// Sets player volume.
    ///
    /// # Parameters
    ///
    /// - `player`: Player name/identifier
    /// - `volume`: Volume level (0-100)
    ///
    /// # Returns
    ///
    /// Set volume packet
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::mpris::MprisPlugin;
    ///
    /// let plugin = MprisPlugin::new();
    /// let packet = plugin.create_set_volume_packet("vlc".to_string(), 50);
    /// assert_eq!(packet.packet_type, "cconnect.mpris.request");
    /// ```
    pub fn create_set_volume_packet(&self, player: String, volume: i32) -> Packet {
        Packet::new(
            "cconnect.mpris.request",
            json!({
                "player": player,
                "setVolume": volume
            }),
        )
    }

    /// Create a set loop status packet
    ///
    /// Sets player loop/repeat mode.
    ///
    /// # Parameters
    ///
    /// - `player`: Player name/identifier
    /// - `loop_status`: Loop mode
    ///
    /// # Returns
    ///
    /// Set loop status packet
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::mpris::*;
    ///
    /// let plugin = MprisPlugin::new();
    /// let packet = plugin.create_set_loop_status_packet("spotify".to_string(), LoopStatus::Playlist);
    /// assert_eq!(packet.packet_type, "cconnect.mpris.request");
    /// ```
    pub fn create_set_loop_status_packet(&self, player: String, loop_status: LoopStatus) -> Packet {
        Packet::new(
            "cconnect.mpris.request",
            json!({
                "player": player,
                "setLoopStatus": loop_status.as_str()
            }),
        )
    }

    /// Create a set shuffle packet
    ///
    /// Enables or disables shuffle mode.
    ///
    /// # Parameters
    ///
    /// - `player`: Player name/identifier
    /// - `shuffle`: Enable shuffle
    ///
    /// # Returns
    ///
    /// Set shuffle packet
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::mpris::MprisPlugin;
    ///
    /// let plugin = MprisPlugin::new();
    /// let packet = plugin.create_set_shuffle_packet("vlc".to_string(), true);
    /// assert_eq!(packet.packet_type, "cconnect.mpris.request");
    /// ```
    pub fn create_set_shuffle_packet(&self, player: String, shuffle: bool) -> Packet {
        Packet::new(
            "cconnect.mpris.request",
            json!({
                "player": player,
                "setShuffle": shuffle
            }),
        )
    }

    /// Get list of known players
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn example() {
    /// use cosmic_connect_core::plugins::mpris::MprisPlugin;
    ///
    /// let plugin = MprisPlugin::new();
    /// let players = plugin.get_player_list().await;
    /// for player in players {
    ///     println!("Player: {}", player);
    /// }
    /// # }
    /// ```
    pub async fn get_player_list(&self) -> Vec<String> {
        self.players.read().await.keys().cloned().collect()
    }

    /// Get player state
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn example() {
    /// use cosmic_connect_core::plugins::mpris::MprisPlugin;
    ///
    /// let plugin = MprisPlugin::new();
    /// if let Some(state) = plugin.get_player_state("spotify").await {
    ///     println!("Playing: {}", state.status.is_playing);
    /// }
    /// # }
    /// ```
    pub async fn get_player_state(&self, player: &str) -> Option<PlayerState> {
        self.players.read().await.get(player).cloned()
    }

    /// Update player state
    ///
    /// Internal method for updating player state from packets.
    async fn update_player_state(&self, state: PlayerState) {
        self.players.write().await.insert(state.name.clone(), state);
    }

    /// Remove player
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn example() {
    /// use cosmic_connect_core::plugins::mpris::MprisPlugin;
    ///
    /// let plugin = MprisPlugin::new();
    /// plugin.remove_player("vlc").await;
    /// # }
    /// ```
    pub async fn remove_player(&self, player: &str) {
        self.players.write().await.remove(player);
    }

    /// Handle incoming MPRIS status packet
    async fn handle_mpris_status(&self, packet: &Packet, device: &Device) {
        // Check if this is a player list
        if let Some(player_list) = packet.body.get("playerList") {
            if let Some(players) = player_list.as_array() {
                let player_names: Vec<String> = players
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();

                info!(
                    "Received player list from {} ({}): {:?}",
                    device.name(),
                    device.id(),
                    player_names
                );
                return;
            }
        }

        // Otherwise, parse player status
        let player_name = packet
            .body
            .get("player")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if player_name.is_empty() {
            debug!("Received MPRIS packet without player name");
            return;
        }

        let status = PlayerStatus {
            is_playing: packet
                .body
                .get("isPlaying")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            position: packet.body.get("pos").and_then(|v| v.as_i64()).unwrap_or(0),
            length: packet
                .body
                .get("length")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            volume: packet
                .body
                .get("volume")
                .and_then(|v| v.as_i64())
                .unwrap_or(100) as i32,
            loop_status: packet
                .body
                .get("loopStatus")
                .and_then(|v| v.as_str())
                .map(LoopStatus::parse_str)
                .unwrap_or(LoopStatus::None),
            shuffle: packet
                .body
                .get("shuffle")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            capabilities: PlayerCapabilities {
                can_play: packet
                    .body
                    .get("canPlay")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
                can_pause: packet
                    .body
                    .get("canPause")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
                can_go_next: packet
                    .body
                    .get("canGoNext")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
                can_go_previous: packet
                    .body
                    .get("canGoPrevious")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
                can_seek: packet
                    .body
                    .get("canSeek")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
            },
        };

        let metadata = PlayerMetadata {
            artist: packet
                .body
                .get("artist")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            title: packet
                .body
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            album: packet
                .body
                .get("album")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            album_art_url: packet
                .body
                .get("albumArtUrl")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        };

        info!(
            "Received player status from {} ({}): {} - {} / {}",
            device.name(),
            device.id(),
            player_name,
            if status.is_playing {
                "playing"
            } else {
                "paused"
            },
            metadata.title.as_deref().unwrap_or("unknown")
        );

        let state = PlayerState {
            name: player_name,
            status,
            metadata,
        };

        self.update_player_state(state).await;
    }

    /// Handle incoming MPRIS request packet
    fn handle_mpris_request(&self, packet: &Packet, device: &Device) {
        // Log the request for now (actual handling would be in application layer)
        if packet.body.get("requestPlayerList").is_some() {
            info!(
                "Received player list request from {} ({})",
                device.name(),
                device.id()
            );
        } else if packet.body.get("requestNowPlaying").is_some() {
            let player = packet
                .body
                .get("player")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            info!(
                "Received now playing request from {} ({}) for player: {}",
                device.name(),
                device.id(),
                player
            );
        } else if let Some(action) = packet.body.get("action").and_then(|v| v.as_str()) {
            let player = packet
                .body
                .get("player")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            info!(
                "Received control action '{}' from {} ({}) for player: {}",
                action,
                device.name(),
                device.id(),
                player
            );
        }
    }
}

impl Default for MprisPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for MprisPlugin {
    fn name(&self) -> &str {
        "mpris"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.mpris".to_string(),
            "cconnect.mpris.request".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.mpris".to_string(),
            "cconnect.mpris.request".to_string(),
        ]
    }

    async fn init(&mut self, device: &Device) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!("MPRIS plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("MPRIS plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        let player_count = self.players.read().await.len();
        info!("MPRIS plugin stopped - {} players tracked", player_count);
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        match packet.packet_type.as_str() {
            "cconnect.mpris" => {
                self.handle_mpris_status(packet, device).await;
            }
            "cconnect.mpris.request" => {
                self.handle_mpris_request(packet, device);
            }
            _ => {}
        }
        Ok(())
    }
}

/// Factory for creating MprisPlugin instances
#[derive(Debug, Clone, Copy)]
pub struct MprisPluginFactory;

impl PluginFactory for MprisPluginFactory {
    fn name(&self) -> &str {
        "mpris"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.mpris".to_string(),
            "cconnect.mpris.request".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.mpris".to_string(),
            "cconnect.mpris.request".to_string(),
        ]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(MprisPlugin::new())
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

    #[test]
    fn test_loop_status() {
        assert_eq!(LoopStatus::None.as_str(), "None");
        assert_eq!(LoopStatus::Track.as_str(), "Track");
        assert_eq!(LoopStatus::Playlist.as_str(), "Playlist");

        assert_eq!(LoopStatus::parse_str("None"), LoopStatus::None);
        assert_eq!(LoopStatus::parse_str("Track"), LoopStatus::Track);
        assert_eq!(LoopStatus::parse_str("Playlist"), LoopStatus::Playlist);
        assert_eq!(LoopStatus::parse_str("invalid"), LoopStatus::None);
    }

    #[test]
    fn test_playback_action() {
        assert_eq!(PlaybackAction::Play.as_str(), "Play");
        assert_eq!(PlaybackAction::Pause.as_str(), "Pause");
        assert_eq!(PlaybackAction::PlayPause.as_str(), "PlayPause");

        assert_eq!(
            PlaybackAction::parse_str("Play"),
            Some(PlaybackAction::Play)
        );
        assert_eq!(
            PlaybackAction::parse_str("Pause"),
            Some(PlaybackAction::Pause)
        );
        assert_eq!(PlaybackAction::parse_str("invalid"), None);
    }

    #[test]
    fn test_plugin_creation() {
        let plugin = MprisPlugin::new();
        assert_eq!(plugin.name(), "mpris");
    }

    #[test]
    fn test_capabilities() {
        let plugin = MprisPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 2);
        assert!(incoming.contains(&"cconnect.mpris".to_string()));
        assert!(incoming.contains(&"cconnect.mpris.request".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 2);
        assert!(outgoing.contains(&"cconnect.mpris".to_string()));
        assert!(outgoing.contains(&"cconnect.mpris.request".to_string()));
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = MprisPlugin::new();
        let device = create_test_device();

        plugin.init(&device).await.unwrap();
        assert!(plugin.device_id.is_some());

        plugin.start().await.unwrap();
        plugin.stop().await.unwrap();
    }

    #[test]
    fn test_create_player_list_packet() {
        let plugin = MprisPlugin::new();
        let packet =
            plugin.create_player_list_packet(vec!["vlc".to_string(), "spotify".to_string()]);

        assert_eq!(packet.packet_type, "cconnect.mpris");
        let player_list = packet.body.get("playerList").unwrap().as_array().unwrap();
        assert_eq!(player_list.len(), 2);
    }

    #[test]
    fn test_create_status_packet() {
        let plugin = MprisPlugin::new();
        let status = PlayerStatus {
            is_playing: true,
            position: 30000,
            length: 180000,
            volume: 75,
            ..Default::default()
        };
        let metadata = PlayerMetadata {
            artist: Some("Artist".to_string()),
            title: Some("Title".to_string()),
            album: Some("Album".to_string()),
            album_art_url: None,
        };

        let packet = plugin.create_status_packet("vlc".to_string(), status, metadata);

        assert_eq!(packet.packet_type, "cconnect.mpris");
        assert_eq!(
            packet.body.get("player").and_then(|v| v.as_str()),
            Some("vlc")
        );
        assert_eq!(
            packet.body.get("isPlaying").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(packet.body.get("pos").and_then(|v| v.as_i64()), Some(30000));
        assert_eq!(
            packet.body.get("length").and_then(|v| v.as_i64()),
            Some(180000)
        );
        assert_eq!(packet.body.get("volume").and_then(|v| v.as_i64()), Some(75));
        assert_eq!(
            packet.body.get("artist").and_then(|v| v.as_str()),
            Some("Artist")
        );
        assert_eq!(
            packet.body.get("title").and_then(|v| v.as_str()),
            Some("Title")
        );
    }

    #[test]
    fn test_create_request_packets() {
        let plugin = MprisPlugin::new();

        let packet = plugin.create_request_player_list_packet();
        assert_eq!(packet.packet_type, "cconnect.mpris.request");
        assert!(packet.body.get("requestPlayerList").is_some());

        let packet = plugin.create_request_now_playing_packet("spotify".to_string());
        assert_eq!(packet.packet_type, "cconnect.mpris.request");
        assert_eq!(
            packet.body.get("player").and_then(|v| v.as_str()),
            Some("spotify")
        );
        assert!(packet.body.get("requestNowPlaying").is_some());
    }

    #[test]
    fn test_create_control_packet() {
        let plugin = MprisPlugin::new();
        let packet = plugin.create_control_packet("vlc".to_string(), PlaybackAction::PlayPause);

        assert_eq!(packet.packet_type, "cconnect.mpris.request");
        assert_eq!(
            packet.body.get("player").and_then(|v| v.as_str()),
            Some("vlc")
        );
        assert_eq!(
            packet.body.get("action").and_then(|v| v.as_str()),
            Some("PlayPause")
        );
    }

    #[test]
    fn test_create_seek_packet() {
        let plugin = MprisPlugin::new();
        let packet = plugin.create_seek_packet("spotify".to_string(), 5_000_000);

        assert_eq!(packet.packet_type, "cconnect.mpris.request");
        assert_eq!(
            packet.body.get("Seek").and_then(|v| v.as_i64()),
            Some(5_000_000)
        );
    }

    #[test]
    fn test_create_set_position_packet() {
        let plugin = MprisPlugin::new();
        let packet = plugin.create_set_position_packet("vlc".to_string(), 60000);

        assert_eq!(packet.packet_type, "cconnect.mpris.request");
        assert_eq!(
            packet.body.get("SetPosition").and_then(|v| v.as_i64()),
            Some(60000)
        );
    }

    #[test]
    fn test_create_set_volume_packet() {
        let plugin = MprisPlugin::new();
        let packet = plugin.create_set_volume_packet("spotify".to_string(), 50);

        assert_eq!(packet.packet_type, "cconnect.mpris.request");
        assert_eq!(
            packet.body.get("setVolume").and_then(|v| v.as_i64()),
            Some(50)
        );
    }

    #[test]
    fn test_create_set_loop_status_packet() {
        let plugin = MprisPlugin::new();
        let packet = plugin.create_set_loop_status_packet("vlc".to_string(), LoopStatus::Playlist);

        assert_eq!(packet.packet_type, "cconnect.mpris.request");
        assert_eq!(
            packet.body.get("setLoopStatus").and_then(|v| v.as_str()),
            Some("Playlist")
        );
    }

    #[test]
    fn test_create_set_shuffle_packet() {
        let plugin = MprisPlugin::new();
        let packet = plugin.create_set_shuffle_packet("spotify".to_string(), true);

        assert_eq!(packet.packet_type, "cconnect.mpris.request");
        assert_eq!(
            packet.body.get("setShuffle").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[tokio::test]
    async fn test_handle_player_list() {
        let mut plugin = MprisPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.mpris",
            json!({
                "playerList": ["vlc", "spotify"],
                "supportAlbumArtPayload": true
            }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();
        // Player list handled (logged)
    }

    #[tokio::test]
    async fn test_handle_player_status() {
        let mut plugin = MprisPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.mpris",
            json!({
                "player": "spotify",
                "isPlaying": true,
                "pos": 45000i64,
                "length": 180000i64,
                "volume": 75i64,
                "artist": "Test Artist",
                "title": "Test Track",
                "album": "Test Album",
                "loopStatus": "None",
                "shuffle": false,
                "canPlay": true,
                "canPause": true,
                "canGoNext": true,
                "canGoPrevious": true,
                "canSeek": true
            }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        let state = plugin.get_player_state("spotify").await.unwrap();
        assert_eq!(state.name, "spotify");
        assert!(state.status.is_playing);
        assert_eq!(state.status.position, 45000);
        assert_eq!(state.status.volume, 75);
        assert_eq!(state.metadata.artist, Some("Test Artist".to_string()));
        assert_eq!(state.metadata.title, Some("Test Track".to_string()));
    }

    #[tokio::test]
    async fn test_get_player_list() {
        let plugin = MprisPlugin::new();

        // Add some players
        plugin
            .update_player_state(PlayerState {
                name: "vlc".to_string(),
                ..Default::default()
            })
            .await;
        plugin
            .update_player_state(PlayerState {
                name: "spotify".to_string(),
                ..Default::default()
            })
            .await;

        let players = plugin.get_player_list().await;
        assert_eq!(players.len(), 2);
        assert!(players.contains(&"vlc".to_string()));
        assert!(players.contains(&"spotify".to_string()));
    }

    #[tokio::test]
    async fn test_remove_player() {
        let plugin = MprisPlugin::new();

        // Add player
        plugin
            .update_player_state(PlayerState {
                name: "vlc".to_string(),
                ..Default::default()
            })
            .await;

        assert_eq!(plugin.get_player_list().await.len(), 1);

        // Remove player
        plugin.remove_player("vlc").await;

        assert_eq!(plugin.get_player_list().await.len(), 0);
    }

    #[tokio::test]
    async fn test_handle_control_request() {
        let mut plugin = MprisPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.mpris.request",
            json!({
                "player": "spotify",
                "action": "PlayPause"
            }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();
        // Request logged
    }
}
