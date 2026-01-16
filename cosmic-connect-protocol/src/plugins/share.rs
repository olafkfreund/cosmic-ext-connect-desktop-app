//! Share Plugin
//!
//! Facilitates file, text, and URL sharing between CConnect devices.
//! Supports single and multiple file transfers with metadata preservation.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - Incoming: `cconnect.share.request`, `cconnect.share.request.update`
//! - Outgoing: `cconnect.share.request`, `cconnect.share.request.update`
//!
//! **Capabilities**: `cconnect.share.request`
//!
//! ## Share Types
//!
//! ### File Transfer
//!
//! Transfers files with optional metadata (timestamps, auto-open).
//! Supports single and multiple file transfers.
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.share.request",
//!     "body": {
//!         "filename": "image.png",
//!         "creationTime": 1640000000000,
//!         "lastModified": 1640000000000,
//!         "open": false
//!     },
//!     "payloadSize": 1048576,
//!     "payloadTransferInfo": {
//!         "port": 1739
//!     }
//! }
//! ```
//!
//! ### Text Sharing
//!
//! Shares text content between devices. The receiving device decides how to present it.
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.share.request",
//!     "body": {
//!         "text": "Some text to share"
//!     }
//! }
//! ```
//!
//! ### URL Sharing
//!
//! Shares URLs. The receiving device typically opens with the default handler.
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.share.request",
//!     "body": {
//!         "url": "https://kdeconnect.kde.org"
//!     }
//! }
//! ```
//!
//! ### Multi-File Transfer
//!
//! For composite transfers, an update packet is sent first with totals:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.share.request.update",
//!     "body": {
//!         "numberOfFiles": 5,
//!         "totalPayloadSize": 10485760
//!     }
//! }
//! ```
//!
//! ## Payload Transfer
//!
//! File payloads are transferred via TCP:
//! 1. Sender includes `payloadTransferInfo` with `port` number
//! 2. Receiver connects to sender's IP on that port
//! 3. Raw file bytes are transferred
//! 4. Connection closes when `payloadSize` bytes received
//!
//! The plugin handles packet creation and metadata. Actual payload transfer
//! is handled by the transport layer.
//!
//! ## Example
//!
//! ```rust,ignore
//! use cosmic_connect_core::plugins::share::*;
//! use cosmic_connect_core::{Plugin, PluginManager};
//!
//! // Create and register plugin
//! let mut manager = PluginManager::new();
//! manager.register(Box::new(SharePlugin::new()))?;
//!
//! // Initialize with device
//! manager.init_all(&device).await?;
//! manager.start_all().await?;
//!
//! // Share text
//! let plugin = SharePlugin::new();
//! let packet = plugin.create_text_packet("Hello from Rust!".to_string());
//! // Send packet to device...
//!
//! // Share URL
//! let packet = plugin.create_url_packet("https://rust-lang.org".to_string());
//! // Send packet to device...
//!
//! // Share file (requires payload transfer setup)
//! let file_info = FileShareInfo {
//!     filename: "document.pdf".to_string(),
//!     size: 1024000,
//!     creation_time: Some(1640000000000),
//!     last_modified: Some(1640000000000),
//!     open: false,
//! };
//! let packet = plugin.create_file_packet(file_info, 1739);
//! // Send packet and handle payload transfer...
//! ```
//!
//! ## References
//!
//! - [Valent Protocol Documentation](https://valent.andyholmes.ca/documentation/protocol.html)

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::{Plugin, PluginFactory};

/// Information about a file being shared
///
/// Contains metadata for file transfers including timestamps and display preferences.
///
/// ## Fields
///
/// - `filename`: Name of the file (with extension)
/// - `size`: File size in bytes
/// - `creation_time`: UNIX epoch timestamp in milliseconds (optional)
/// - `last_modified`: Last modification timestamp in milliseconds (optional)
/// - `open`: Whether to auto-open the file after transfer (default: false)
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_core::plugins::share::FileShareInfo;
///
/// let info = FileShareInfo {
///     filename: "photo.jpg".to_string(),
///     size: 2048000,
///     creation_time: Some(1640000000000),
///     last_modified: Some(1640000000000),
///     open: false,
/// };
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct FileShareInfo {
    /// Filename with extension
    pub filename: String,

    /// File size in bytes
    pub size: i64,

    /// Creation time (UNIX epoch milliseconds)
    pub creation_time: Option<i64>,

    /// Last modified time (UNIX epoch milliseconds)
    pub last_modified: Option<i64>,

    /// Auto-open file after transfer
    pub open: bool,
}

/// Information about a multi-file transfer
///
/// Sent before composite transfers to communicate totals for progress tracking.
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_core::plugins::share::MultiFileInfo;
///
/// let info = MultiFileInfo {
///     number_of_files: 10,
///     total_payload_size: 52428800, // 50 MB
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MultiFileInfo {
    /// Number of files in the transfer
    #[serde(rename = "numberOfFiles")]
    pub number_of_files: i32,

    /// Total size of all files in bytes
    #[serde(rename = "totalPayloadSize")]
    pub total_payload_size: i64,
}

/// Type of content being shared
///
/// Distinguishes between file, text, and URL sharing operations.
#[derive(Debug, Clone, PartialEq)]
pub enum ShareContent {
    /// File transfer with metadata
    File(FileShareInfo),

    /// Text content
    Text(String),

    /// URL to open
    Url(String),
}

/// Record of an incoming or outgoing share
///
/// Tracks share operations for history and progress monitoring.
#[derive(Debug, Clone, PartialEq)]
pub struct ShareRecord {
    /// Unique share ID (typically packet ID)
    pub id: String,

    /// Device ID involved in the share
    pub device_id: String,

    /// Content being shared
    pub content: ShareContent,

    /// Timestamp of share operation (UNIX epoch milliseconds)
    pub timestamp: i64,

    /// Whether this was an incoming (true) or outgoing (false) share
    pub incoming: bool,
}

/// Share plugin for file, text, and URL sharing
///
/// Handles `cconnect.share.request` packets for transferring content between devices.
/// Maintains history of share operations and provides packet creation helpers.
///
/// ## Features
///
/// - Single and multiple file transfers
/// - Text sharing
/// - URL sharing
/// - Share history tracking
/// - Metadata preservation (timestamps, auto-open)
/// - Thread-safe state management
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_core::plugins::share::SharePlugin;
/// use cosmic_connect_core::Plugin;
///
/// let plugin = SharePlugin::new();
/// assert_eq!(plugin.name(), "share");
/// assert_eq!(plugin.share_count(), 0);
/// ```
#[derive(Debug)]
pub struct SharePlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// History of share operations
    shares: Arc<RwLock<Vec<ShareRecord>>>,
}

impl SharePlugin {
    /// Create a new share plugin
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::share::SharePlugin;
    ///
    /// let plugin = SharePlugin::new();
    /// assert_eq!(plugin.share_count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            device_id: None,
            shares: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create a file share packet
    ///
    /// Creates a `cconnect.share.request` packet for file transfer.
    /// Includes payload size and transfer info with the specified port.
    ///
    /// # Parameters
    ///
    /// - `file_info`: File metadata
    /// - `port`: TCP port for payload transfer
    ///
    /// # Returns
    ///
    /// Packet ready to be sent, with payload transfer info included
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::share::{SharePlugin, FileShareInfo};
    ///
    /// let plugin = SharePlugin::new();
    /// let file_info = FileShareInfo {
    ///     filename: "test.txt".to_string(),
    ///     size: 1024,
    ///     creation_time: Some(1640000000000),
    ///     last_modified: Some(1640000000000),
    ///     open: false,
    /// };
    ///
    /// let packet = plugin.create_file_packet(file_info, 1739);
    /// assert_eq!(packet.packet_type, "cconnect.share.request");
    /// assert_eq!(packet.payload_size, Some(1024));
    /// ```
    pub fn create_file_packet(&self, file_info: FileShareInfo, port: u16) -> Packet {
        let mut body = json!({
            "filename": file_info.filename,
        });

        // Add optional fields
        if let Some(creation_time) = file_info.creation_time {
            body["creationTime"] = json!(creation_time);
        }
        if let Some(last_modified) = file_info.last_modified {
            body["lastModified"] = json!(last_modified);
        }
        if file_info.open {
            body["open"] = json!(true);
        }

        // Create payload transfer info
        let mut transfer_info = HashMap::new();
        transfer_info.insert("port".to_string(), json!(port));

        Packet::new("cconnect.share.request", body)
            .with_payload_size(file_info.size)
            .with_payload_transfer_info(transfer_info)
    }

    /// Create a text share packet
    ///
    /// Creates a `cconnect.share.request` packet for text sharing.
    ///
    /// # Parameters
    ///
    /// - `text`: Text content to share
    ///
    /// # Returns
    ///
    /// Packet ready to be sent
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::share::SharePlugin;
    ///
    /// let plugin = SharePlugin::new();
    /// let packet = plugin.create_text_packet("Hello, World!".to_string());
    ///
    /// assert_eq!(packet.packet_type, "cconnect.share.request");
    /// assert_eq!(
    ///     packet.body.get("text").and_then(|v| v.as_str()),
    ///     Some("Hello, World!")
    /// );
    /// ```
    pub fn create_text_packet(&self, text: String) -> Packet {
        Packet::new("cconnect.share.request", json!({ "text": text }))
    }

    /// Create a URL share packet
    ///
    /// Creates a `cconnect.share.request` packet for URL sharing.
    ///
    /// # Parameters
    ///
    /// - `url`: URL to share
    ///
    /// # Returns
    ///
    /// Packet ready to be sent
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::share::SharePlugin;
    ///
    /// let plugin = SharePlugin::new();
    /// let packet = plugin.create_url_packet("https://rust-lang.org".to_string());
    ///
    /// assert_eq!(packet.packet_type, "cconnect.share.request");
    /// assert_eq!(
    ///     packet.body.get("url").and_then(|v| v.as_str()),
    ///     Some("https://rust-lang.org")
    /// );
    /// ```
    pub fn create_url_packet(&self, url: String) -> Packet {
        Packet::new("cconnect.share.request", json!({ "url": url }))
    }

    /// Create a multi-file update packet
    ///
    /// Creates a `cconnect.share.request.update` packet to announce
    /// a composite transfer. Send this before the individual file packets.
    ///
    /// # Parameters
    ///
    /// - `info`: Multi-file transfer information
    ///
    /// # Returns
    ///
    /// Update packet ready to be sent
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::share::{SharePlugin, MultiFileInfo};
    ///
    /// let plugin = SharePlugin::new();
    /// let info = MultiFileInfo {
    ///     number_of_files: 5,
    ///     total_payload_size: 10485760,
    /// };
    ///
    /// let packet = plugin.create_multifile_update_packet(info);
    /// assert_eq!(packet.packet_type, "cconnect.share.request.update");
    /// ```
    pub fn create_multifile_update_packet(&self, info: MultiFileInfo) -> Packet {
        Packet::new("cconnect.share.request.update", json!(info))
    }

    /// Get the number of recorded shares
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::share::SharePlugin;
    ///
    /// let plugin = SharePlugin::new();
    /// assert_eq!(plugin.share_count(), 0);
    /// ```
    pub fn share_count(&self) -> usize {
        // Use try_read for non-async context, return 0 if locked
        self.shares.try_read().map(|s| s.len()).unwrap_or(0)
    }

    /// Get all share records
    ///
    /// Returns a snapshot of the share history.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn example() {
    /// use cosmic_connect_core::plugins::share::SharePlugin;
    ///
    /// let plugin = SharePlugin::new();
    /// let shares = plugin.get_all_shares().await;
    /// for share in shares {
    ///     println!("Share from device: {}", share.device_id);
    /// }
    /// # }
    /// ```
    pub async fn get_all_shares(&self) -> Vec<ShareRecord> {
        self.shares.read().await.clone()
    }

    /// Get incoming shares only
    ///
    /// Filters share history for incoming shares.
    pub async fn get_incoming_shares(&self) -> Vec<ShareRecord> {
        self.shares
            .read()
            .await
            .iter()
            .filter(|s| s.incoming)
            .cloned()
            .collect()
    }

    /// Get outgoing shares only
    ///
    /// Filters share history for outgoing shares.
    pub async fn get_outgoing_shares(&self) -> Vec<ShareRecord> {
        self.shares
            .read()
            .await
            .iter()
            .filter(|s| !s.incoming)
            .cloned()
            .collect()
    }

    /// Clear share history
    ///
    /// Removes all recorded share operations.
    pub async fn clear_history(&self) {
        self.shares.write().await.clear();
    }

    /// Handle an incoming share request packet
    ///
    /// Processes share packets and records them in history.
    /// For file shares, initiates download via PayloadClient.
    async fn handle_share_request(&self, packet: &Packet, device: &Device) {
        let device_id = device.id().to_string();

        // Determine content type
        let content = if let Some(filename) = packet.body.get("filename").and_then(|v| v.as_str()) {
            // File share
            let file_info = FileShareInfo {
                filename: filename.to_string(),
                size: packet.payload_size.unwrap_or(0),
                creation_time: packet.body.get("creationTime").and_then(|v| v.as_i64()),
                last_modified: packet.body.get("lastModified").and_then(|v| v.as_i64()),
                open: packet
                    .body
                    .get("open")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            };

            info!(
                "Received file share from {} ({}): {} ({} bytes)",
                device.name(),
                device_id,
                filename,
                file_info.size
            );

            // Check if we need to download the file
            if let Some(transfer_info) = &packet.payload_transfer_info {
                // Extract port from payloadTransferInfo
                if let Some(port_value) = transfer_info.get("port") {
                    let port = port_value.as_i64().unwrap_or(0) as u16;

                    // Get remote host from device
                    if let Some(host) = &device.host {
                        let host_clone = host.clone();
                        let filename_clone = filename.to_string();
                        let size = file_info.size;
                        let device_name = device.name().to_string();

                        // Spawn background task to download file
                        tokio::spawn(async move {
                            // Create downloads directory
                            let downloads_dir = std::path::PathBuf::from(
                                std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
                            ).join("Downloads");

                            if let Err(e) = tokio::fs::create_dir_all(&downloads_dir).await {
                                warn!("Failed to create downloads directory: {}", e);
                                return;
                            }

                            let file_path = downloads_dir.join(&filename_clone);

                            info!(
                                "Downloading file '{}' from {} ({}:{}) to {:?}",
                                filename_clone, device_name, host_clone, port, file_path
                            );

                            // Connect to payload server and download file
                            use crate::PayloadClient;
                            match PayloadClient::new(&host_clone, port).await {
                                Ok(client) => {
                                    match client.receive_file(&file_path, size as u64).await {
                                        Ok(()) => {
                                            info!(
                                                "Successfully downloaded file '{}' from {}",
                                                filename_clone, device_name
                                            );
                                        }
                                        Err(e) => {
                                            warn!(
                                                "Failed to download file '{}' from {}: {}",
                                                filename_clone, device_name, e
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(
                                        "Failed to connect to payload server {}:{}: {}",
                                        host_clone, port, e
                                    );
                                }
                            }
                        });
                    } else {
                        warn!("Cannot download file: device host not available");
                    }
                } else {
                    warn!("Cannot download file: no port in payloadTransferInfo");
                }
            }

            ShareContent::File(file_info)
        } else if let Some(text) = packet.body.get("text").and_then(|v| v.as_str()) {
            // Text share
            info!(
                "Received text share from {} ({}): {} chars",
                device.name(),
                device_id,
                text.len()
            );

            ShareContent::Text(text.to_string())
        } else if let Some(url) = packet.body.get("url").and_then(|v| v.as_str()) {
            // URL share
            info!(
                "Received URL share from {} ({}): {}",
                device.name(),
                device_id,
                url
            );

            ShareContent::Url(url.to_string())
        } else {
            warn!(
                "Received share request from {} ({}) with unknown content type",
                device.name(),
                device_id
            );
            return;
        };

        // Record share
        let record = ShareRecord {
            id: packet.id.to_string(),
            device_id,
            content,
            timestamp: packet.id,
            incoming: true,
        };

        self.shares.write().await.push(record);

        debug!("Share history size: {}", self.shares.read().await.len());
    }

    /// Handle a multi-file update packet
    ///
    /// Logs multi-file transfer announcement.
    fn handle_multifile_update(&self, packet: &Packet, device: &Device) {
        let number_of_files = packet
            .body
            .get("numberOfFiles")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let total_size = packet
            .body
            .get("totalPayloadSize")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        info!(
            "Received multi-file update from {} ({}): {} files, {} bytes total",
            device.name(),
            device.id(),
            number_of_files,
            total_size
        );
    }
}

impl Default for SharePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for SharePlugin {
    fn name(&self) -> &str {
        "share"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.share.request".to_string(),
            "cconnect.share.request.update".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.share.request".to_string(),
            "cconnect.share.request.update".to_string(),
        ]
    }

    async fn init(&mut self, device: &Device) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!("Share plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Share plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        let share_count = self.shares.read().await.len();
        info!("Share plugin stopped - {} shares recorded", share_count);
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        match packet.packet_type.as_str() {
            "cconnect.share.request" => {
                self.handle_share_request(packet, device).await;
            }
            "cconnect.share.request.update" => {
                self.handle_multifile_update(packet, device);
            }
            _ => {}
        }
        Ok(())
    }
}

/// Factory for creating SharePlugin instances
#[derive(Debug, Clone, Copy)]
pub struct SharePluginFactory;

impl PluginFactory for SharePluginFactory {
    fn name(&self) -> &str {
        "share"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.share.request".to_string(),
            "cconnect.share.request.update".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.share.request".to_string(),
            "cconnect.share.request.update".to_string(),
        ]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(SharePlugin::new())
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
        let plugin = SharePlugin::new();
        assert_eq!(plugin.name(), "share");
        assert_eq!(plugin.share_count(), 0);
    }

    #[test]
    fn test_capabilities() {
        let plugin = SharePlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 2);
        assert!(incoming.contains(&"cconnect.share.request".to_string()));
        assert!(incoming.contains(&"cconnect.share.request.update".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 2);
        assert!(outgoing.contains(&"cconnect.share.request".to_string()));
        assert!(outgoing.contains(&"cconnect.share.request.update".to_string()));
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = SharePlugin::new();
        let device = create_test_device();

        // Initialize
        plugin.init(&device).await.unwrap();
        assert!(plugin.device_id.is_some());

        // Start
        plugin.start().await.unwrap();

        // Stop
        plugin.stop().await.unwrap();
    }

    #[test]
    fn test_create_file_packet() {
        let plugin = SharePlugin::new();
        let file_info = FileShareInfo {
            filename: "test.txt".to_string(),
            size: 1024,
            creation_time: Some(1640000000000),
            last_modified: Some(1640000000000),
            open: false,
        };

        let packet = plugin.create_file_packet(file_info, 1739);

        assert_eq!(packet.packet_type, "cconnect.share.request");
        assert_eq!(
            packet.body.get("filename").and_then(|v| v.as_str()),
            Some("test.txt")
        );
        assert_eq!(packet.payload_size, Some(1024));

        let transfer_info = packet.payload_transfer_info.as_ref().unwrap();
        assert_eq!(
            transfer_info.get("port").and_then(|v| v.as_i64()),
            Some(1739)
        );
    }

    #[test]
    fn test_create_text_packet() {
        let plugin = SharePlugin::new();
        let packet = plugin.create_text_packet("Hello, World!".to_string());

        assert_eq!(packet.packet_type, "cconnect.share.request");
        assert_eq!(
            packet.body.get("text").and_then(|v| v.as_str()),
            Some("Hello, World!")
        );
        assert!(packet.payload_size.is_none());
    }

    #[test]
    fn test_create_url_packet() {
        let plugin = SharePlugin::new();
        let packet = plugin.create_url_packet("https://rust-lang.org".to_string());

        assert_eq!(packet.packet_type, "cconnect.share.request");
        assert_eq!(
            packet.body.get("url").and_then(|v| v.as_str()),
            Some("https://rust-lang.org")
        );
        assert!(packet.payload_size.is_none());
    }

    #[test]
    fn test_create_multifile_update_packet() {
        let plugin = SharePlugin::new();
        let info = MultiFileInfo {
            number_of_files: 5,
            total_payload_size: 10485760,
        };

        let packet = plugin.create_multifile_update_packet(info);

        assert_eq!(packet.packet_type, "cconnect.share.request.update");
        assert_eq!(
            packet.body.get("numberOfFiles").and_then(|v| v.as_i64()),
            Some(5)
        );
        assert_eq!(
            packet.body.get("totalPayloadSize").and_then(|v| v.as_i64()),
            Some(10485760)
        );
    }

    #[tokio::test]
    async fn test_handle_file_share() {
        let mut plugin = SharePlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.share.request",
            json!({
                "filename": "document.pdf",
                "creationTime": 1640000000000i64,
                "lastModified": 1640000000000i64,
                "open": false
            }),
        )
        .with_payload_size(2048);

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        assert_eq!(plugin.share_count(), 1);
        let shares = plugin.get_all_shares().await;
        assert_eq!(shares.len(), 1);
        assert!(shares[0].incoming);

        if let ShareContent::File(file_info) = &shares[0].content {
            assert_eq!(file_info.filename, "document.pdf");
            assert_eq!(file_info.size, 2048);
        } else {
            panic!("Expected File content");
        }
    }

    #[tokio::test]
    async fn test_handle_text_share() {
        let mut plugin = SharePlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.share.request",
            json!({ "text": "Test message" }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        assert_eq!(plugin.share_count(), 1);
        let shares = plugin.get_all_shares().await;

        if let ShareContent::Text(text) = &shares[0].content {
            assert_eq!(text, "Test message");
        } else {
            panic!("Expected Text content");
        }
    }

    #[tokio::test]
    async fn test_handle_url_share() {
        let mut plugin = SharePlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.share.request",
            json!({ "url": "https://example.com" }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        assert_eq!(plugin.share_count(), 1);
        let shares = plugin.get_all_shares().await;

        if let ShareContent::Url(url) = &shares[0].content {
            assert_eq!(url, "https://example.com");
        } else {
            panic!("Expected URL content");
        }
    }

    #[tokio::test]
    async fn test_handle_multifile_update() {
        let mut plugin = SharePlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.share.request.update",
            json!({
                "numberOfFiles": 3,
                "totalPayloadSize": 5242880
            }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        // Update packets don't create share records
        assert_eq!(plugin.share_count(), 0);
    }

    #[tokio::test]
    async fn test_filter_incoming_shares() {
        let plugin = SharePlugin::new();

        // Manually add shares
        let mut shares = plugin.shares.write().await;
        shares.push(ShareRecord {
            id: "1".to_string(),
            device_id: "device1".to_string(),
            content: ShareContent::Text("test1".to_string()),
            timestamp: 1000,
            incoming: true,
        });
        shares.push(ShareRecord {
            id: "2".to_string(),
            device_id: "device2".to_string(),
            content: ShareContent::Text("test2".to_string()),
            timestamp: 2000,
            incoming: false,
        });
        shares.push(ShareRecord {
            id: "3".to_string(),
            device_id: "device3".to_string(),
            content: ShareContent::Text("test3".to_string()),
            timestamp: 3000,
            incoming: true,
        });
        drop(shares);

        let incoming = plugin.get_incoming_shares().await;
        assert_eq!(incoming.len(), 2);
        assert!(incoming.iter().all(|s| s.incoming));

        let outgoing = plugin.get_outgoing_shares().await;
        assert_eq!(outgoing.len(), 1);
        assert!(outgoing.iter().all(|s| !s.incoming));
    }

    #[tokio::test]
    async fn test_clear_history() {
        let plugin = SharePlugin::new();

        // Add a share
        plugin.shares.write().await.push(ShareRecord {
            id: "1".to_string(),
            device_id: "device1".to_string(),
            content: ShareContent::Text("test".to_string()),
            timestamp: 1000,
            incoming: true,
        });

        assert_eq!(plugin.share_count(), 1);

        plugin.clear_history().await;
        assert_eq!(plugin.share_count(), 0);
    }

    #[tokio::test]
    async fn test_multiple_shares() {
        let mut plugin = SharePlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();

        // Share file
        let packet1 = Packet::new(
            "cconnect.share.request",
            json!({ "filename": "file1.txt" }),
        )
        .with_payload_size(100);
        plugin.handle_packet(&packet1, &mut device).await.unwrap();

        // Share text
        let packet2 = Packet::new("cconnect.share.request", json!({ "text": "Hello" }));
        plugin.handle_packet(&packet2, &mut device).await.unwrap();

        // Share URL
        let packet3 = Packet::new(
            "cconnect.share.request",
            json!({ "url": "https://example.com" }),
        );
        plugin.handle_packet(&packet3, &mut device).await.unwrap();

        assert_eq!(plugin.share_count(), 3);

        let shares = plugin.get_all_shares().await;
        assert!(matches!(shares[0].content, ShareContent::File(_)));
        assert!(matches!(shares[1].content, ShareContent::Text(_)));
        assert!(matches!(shares[2].content, ShareContent::Url(_)));
    }

    #[tokio::test]
    async fn test_ignore_invalid_share() {
        let mut plugin = SharePlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();

        let mut device = create_test_device();

        // Packet with no recognizable content
        let packet = Packet::new(
            "cconnect.share.request",
            json!({ "invalidField": "value" }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        // Should not create a share record
        assert_eq!(plugin.share_count(), 0);
    }
}
