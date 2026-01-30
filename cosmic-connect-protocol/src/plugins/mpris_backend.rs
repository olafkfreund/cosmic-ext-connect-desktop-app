//! MPRIS2 DBus Backend
//!
//! Provides media player control via the MPRIS2 DBus interface.
//! Handles player discovery, playback control, and property queries.
//!
//! ## MPRIS2 DBus Interface
//!
//! - Bus: Session bus
//! - Service prefix: `org.mpris.MediaPlayer2.*`
//! - Object path: `/org/mpris/MediaPlayer2`
//! - Interfaces:
//!   - `org.mpris.MediaPlayer2` - Application identity
//!   - `org.mpris.MediaPlayer2.Player` - Playback control
//!
//! ## Supported Methods
//!
//! - `Play`, `Pause`, `PlayPause`, `Stop`, `Next`, `Previous`
//! - `Seek` (relative position in microseconds)
//! - `SetPosition` (absolute position)
//!
//! ## Supported Properties
//!
//! - `PlaybackStatus`, `Position`, `Volume`, `LoopStatus`, `Shuffle`
//! - `Metadata` (artist, title, album, length, art URL)
//! - `CanPlay`, `CanPause`, `CanGoNext`, `CanGoPrevious`, `CanSeek`

use std::collections::HashMap;
use tracing::{debug, info, warn};
use zbus::zvariant::{ObjectPath, OwnedValue};
use zbus::Connection;

/// MPRIS2 DBus interface names
const MPRIS_INTERFACE: &str = "org.mpris.MediaPlayer2";
const MPRIS_PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const MPRIS_BUS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";

/// Playback status from MPRIS2
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    #[default]
    Stopped,
}

impl PlaybackStatus {
    pub fn from_str(s: &str) -> Self {
        match s {
            "Playing" => Self::Playing,
            "Paused" => Self::Paused,
            _ => Self::Stopped,
        }
    }

    pub fn is_playing(&self) -> bool {
        matches!(self, Self::Playing)
    }
}

/// Media player metadata
#[derive(Debug, Clone, Default)]
pub struct PlayerMetadata {
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub album_art_url: Option<String>,
    pub length: i64, // microseconds
    pub track_id: Option<String>,
}

/// Player state from MPRIS2
#[derive(Debug, Clone, Default)]
pub struct PlayerState {
    pub name: String,
    pub identity: String,
    pub playback_status: PlaybackStatus,
    pub position: i64, // microseconds
    pub volume: f64,   // 0.0 to 1.0
    pub loop_status: String,
    pub shuffle: bool,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub can_seek: bool,
    pub metadata: PlayerMetadata,
}

/// MPRIS2 DBus backend for media player control
pub struct MprisBackend {
    /// DBus session connection
    connection: Option<Connection>,
}

impl MprisBackend {
    /// Create a new MPRIS backend
    pub fn new() -> Self {
        Self { connection: None }
    }

    /// Connect to the session DBus
    pub async fn connect(&mut self) -> Result<(), String> {
        if self.connection.is_some() {
            return Ok(());
        }

        let conn = Connection::session()
            .await
            .map_err(|e| format!("Failed to connect to session bus: {}", e))?;

        self.connection = Some(conn);
        info!("Connected to session DBus for MPRIS");
        Ok(())
    }

    /// Ensure connection is established
    async fn ensure_connected(&mut self) -> Result<(), String> {
        if self.connection.is_none() {
            self.connect().await?;
        }
        Ok(())
    }

    /// Get the DBus bus name for a player
    fn player_bus_name(player: &str) -> String {
        format!("{}{}", MPRIS_BUS_PREFIX, player)
    }

    /// Discover all MPRIS2 players on the session bus
    pub async fn discover_players(&mut self) -> Result<Vec<String>, String> {
        self.ensure_connected().await?;
        let conn = self.connection.as_ref().ok_or("Not connected")?;

        let dbus_proxy = zbus::fdo::DBusProxy::new(conn)
            .await
            .map_err(|e| format!("Failed to create DBus proxy: {}", e))?;

        let names = dbus_proxy
            .list_names()
            .await
            .map_err(|e| format!("Failed to list DBus names: {}", e))?;

        let players: Vec<String> = names
            .into_iter()
            .filter_map(|name| name.strip_prefix(MPRIS_BUS_PREFIX).map(String::from))
            .collect();

        debug!("Discovered {} MPRIS players: {:?}", players.len(), players);
        Ok(players)
    }

    /// Query player state from DBus
    pub async fn query_player_state(&mut self, player: &str) -> Result<PlayerState, String> {
        self.ensure_connected().await?;
        let conn = self.connection.as_ref().ok_or("Not connected")?;
        let bus_name = Self::player_bus_name(player);

        // Create proxies for player and application interfaces
        let player_proxy = zbus::Proxy::new(
            conn,
            bus_name.as_str(),
            MPRIS_OBJECT_PATH,
            MPRIS_PLAYER_INTERFACE,
        )
        .await
        .map_err(|e| format!("Failed to create player proxy: {}", e))?;

        let mpris_proxy = zbus::Proxy::new(
            conn,
            bus_name.as_str(),
            MPRIS_OBJECT_PATH,
            MPRIS_INTERFACE,
        )
        .await
        .map_err(|e| format!("Failed to create MPRIS proxy: {}", e))?;

        // Query properties with defaults
        let playback_status: String = player_proxy
            .get_property("PlaybackStatus")
            .await
            .unwrap_or_else(|_| "Stopped".to_string());
        let loop_status: String = player_proxy
            .get_property("LoopStatus")
            .await
            .unwrap_or_else(|_| "None".to_string());
        let identity: String = mpris_proxy
            .get_property("Identity")
            .await
            .unwrap_or_else(|_| player.to_string());

        let position: i64 = player_proxy.get_property("Position").await.unwrap_or(0);
        let volume: f64 = player_proxy.get_property("Volume").await.unwrap_or(1.0);
        let shuffle: bool = player_proxy.get_property("Shuffle").await.unwrap_or(false);
        let can_play: bool = player_proxy.get_property("CanPlay").await.unwrap_or(true);
        let can_pause: bool = player_proxy.get_property("CanPause").await.unwrap_or(true);
        let can_go_next: bool = player_proxy.get_property("CanGoNext").await.unwrap_or(true);
        let can_go_previous: bool = player_proxy
            .get_property("CanGoPrevious")
            .await
            .unwrap_or(true);
        let can_seek: bool = player_proxy.get_property("CanSeek").await.unwrap_or(true);

        // Query metadata
        let metadata = self.query_metadata(&player_proxy).await;

        Ok(PlayerState {
            name: player.to_string(),
            identity,
            playback_status: PlaybackStatus::from_str(&playback_status),
            position,
            volume,
            loop_status,
            shuffle,
            can_play,
            can_pause,
            can_go_next,
            can_go_previous,
            can_seek,
            metadata,
        })
    }

    /// Query metadata from player proxy
    async fn query_metadata(&self, player_proxy: &zbus::Proxy<'_>) -> PlayerMetadata {
        let metadata_dict: HashMap<String, OwnedValue> = player_proxy
            .get_property("Metadata")
            .await
            .unwrap_or_default();

        // Helper to extract string fields
        let get_string = |key: &str| -> Option<String> {
            metadata_dict
                .get(key)
                .and_then(|v| <&str>::try_from(v).ok())
                .map(String::from)
        };

        PlayerMetadata {
            artist: get_string("xesam:artist"),
            title: get_string("xesam:title"),
            album: get_string("xesam:album"),
            album_art_url: get_string("mpris:artUrl"),
            track_id: get_string("mpris:trackid"),
            length: metadata_dict
                .get("mpris:length")
                .and_then(|v| i64::try_from(v).ok())
                .unwrap_or(0),
        }
    }

    /// Call a playback control method
    pub async fn call_method(&mut self, player: &str, method: &str) -> Result<(), String> {
        self.ensure_connected().await?;
        let conn = self.connection.as_ref().ok_or("Not connected")?;
        let bus_name = Self::player_bus_name(player);

        let player_proxy = zbus::Proxy::new(
            conn,
            bus_name.as_str(),
            MPRIS_OBJECT_PATH,
            MPRIS_PLAYER_INTERFACE,
        )
        .await
        .map_err(|e| format!("Failed to create player proxy: {}", e))?;

        player_proxy
            .call_method(method, &())
            .await
            .map_err(|e| format!("Failed to call {}: {}", method, e))?;

        info!("Called {} on player {}", method, player);
        Ok(())
    }

    /// Seek relative to current position
    pub async fn seek(&mut self, player: &str, offset_microseconds: i64) -> Result<(), String> {
        self.ensure_connected().await?;
        let conn = self.connection.as_ref().ok_or("Not connected")?;
        let bus_name = Self::player_bus_name(player);

        let player_proxy = zbus::Proxy::new(
            conn,
            bus_name.as_str(),
            MPRIS_OBJECT_PATH,
            MPRIS_PLAYER_INTERFACE,
        )
        .await
        .map_err(|e| format!("Failed to create player proxy: {}", e))?;

        player_proxy
            .call_method("Seek", &(offset_microseconds,))
            .await
            .map_err(|e| format!("Failed to seek: {}", e))?;

        debug!("Seeked {} microseconds on {}", offset_microseconds, player);
        Ok(())
    }

    /// Set absolute position
    pub async fn set_position(
        &mut self,
        player: &str,
        track_id: &str,
        position_microseconds: i64,
    ) -> Result<(), String> {
        self.ensure_connected().await?;
        let conn = self.connection.as_ref().ok_or("Not connected")?;
        let bus_name = Self::player_bus_name(player);

        let player_proxy = zbus::Proxy::new(
            conn,
            bus_name.as_str(),
            MPRIS_OBJECT_PATH,
            MPRIS_PLAYER_INTERFACE,
        )
        .await
        .map_err(|e| format!("Failed to create player proxy: {}", e))?;

        let track_path =
            ObjectPath::try_from(track_id).map_err(|e| format!("Invalid track ID: {}", e))?;

        player_proxy
            .call_method("SetPosition", &(track_path, position_microseconds))
            .await
            .map_err(|e| format!("Failed to set position: {}", e))?;

        debug!("Set position to {} on {}", position_microseconds, player);
        Ok(())
    }

    /// Set volume (0.0 to 1.0+)
    pub async fn set_volume(&mut self, player: &str, volume: f64) -> Result<(), String> {
        self.ensure_connected().await?;
        let conn = self.connection.as_ref().ok_or("Not connected")?;
        let bus_name = Self::player_bus_name(player);

        let player_proxy = zbus::Proxy::new(
            conn,
            bus_name.as_str(),
            MPRIS_OBJECT_PATH,
            MPRIS_PLAYER_INTERFACE,
        )
        .await
        .map_err(|e| format!("Failed to create player proxy: {}", e))?;

        player_proxy
            .set_property("Volume", volume)
            .await
            .map_err(|e| format!("Failed to set volume: {}", e))?;

        debug!("Set volume to {} on {}", volume, player);
        Ok(())
    }

    /// Set loop status
    pub async fn set_loop_status(&mut self, player: &str, loop_status: &str) -> Result<(), String> {
        self.ensure_connected().await?;
        let conn = self.connection.as_ref().ok_or("Not connected")?;
        let bus_name = Self::player_bus_name(player);

        let player_proxy = zbus::Proxy::new(
            conn,
            bus_name.as_str(),
            MPRIS_OBJECT_PATH,
            MPRIS_PLAYER_INTERFACE,
        )
        .await
        .map_err(|e| format!("Failed to create player proxy: {}", e))?;

        player_proxy
            .set_property("LoopStatus", loop_status)
            .await
            .map_err(|e| format!("Failed to set loop status: {}", e))?;

        debug!("Set loop status to {} on {}", loop_status, player);
        Ok(())
    }

    /// Set shuffle
    pub async fn set_shuffle(&mut self, player: &str, shuffle: bool) -> Result<(), String> {
        self.ensure_connected().await?;
        let conn = self.connection.as_ref().ok_or("Not connected")?;
        let bus_name = Self::player_bus_name(player);

        let player_proxy = zbus::Proxy::new(
            conn,
            bus_name.as_str(),
            MPRIS_OBJECT_PATH,
            MPRIS_PLAYER_INTERFACE,
        )
        .await
        .map_err(|e| format!("Failed to create player proxy: {}", e))?;

        player_proxy
            .set_property("Shuffle", shuffle)
            .await
            .map_err(|e| format!("Failed to set shuffle: {}", e))?;

        debug!("Set shuffle to {} on {}", shuffle, player);
        Ok(())
    }

    /// Check if MPRIS service is available
    pub async fn is_available(&mut self) -> bool {
        if let Err(e) = self.ensure_connected().await {
            warn!("MPRIS backend not available: {}", e);
            return false;
        }

        // Check if any players exist
        self.discover_players()
            .await
            .map(|players| !players.is_empty())
            .unwrap_or(false)
    }
}

impl Default for MprisBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playback_status() {
        assert_eq!(PlaybackStatus::from_str("Playing"), PlaybackStatus::Playing);
        assert_eq!(PlaybackStatus::from_str("Paused"), PlaybackStatus::Paused);
        assert_eq!(PlaybackStatus::from_str("Stopped"), PlaybackStatus::Stopped);
        assert_eq!(PlaybackStatus::from_str("unknown"), PlaybackStatus::Stopped);

        assert!(PlaybackStatus::Playing.is_playing());
        assert!(!PlaybackStatus::Paused.is_playing());
        assert!(!PlaybackStatus::Stopped.is_playing());
    }

    #[test]
    fn test_player_bus_name() {
        assert_eq!(
            MprisBackend::player_bus_name("spotify"),
            "org.mpris.MediaPlayer2.spotify"
        );
        assert_eq!(
            MprisBackend::player_bus_name("vlc"),
            "org.mpris.MediaPlayer2.vlc"
        );
    }

    #[test]
    fn test_backend_new() {
        let backend = MprisBackend::new();
        assert!(backend.connection.is_none());
    }

    // Integration tests require DBus session bus
    #[tokio::test]
    #[ignore = "Requires DBus session bus"]
    async fn test_discover_players() {
        let mut backend = MprisBackend::new();
        let result = backend.discover_players().await;
        // May or may not have players
        assert!(result.is_ok());
    }
}
