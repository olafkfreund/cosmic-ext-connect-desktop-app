//! Clipboard History Plugin
//!
//! Maintains and syncs clipboard history across connected desktops.
//! Extends the basic Clipboard plugin with persistent storage and history management.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - Incoming: `cconnect.cliphistory.sync`, `cconnect.cliphistory.add`, `cconnect.cliphistory.pin`,
//!             `cconnect.cliphistory.delete`, `cconnect.cliphistory.search`
//! - Outgoing: `cconnect.cliphistory.sync`, `cconnect.cliphistory.add`, `cconnect.cliphistory.result`
//!
//! **Capabilities**: `cconnect.cliphistory`
//!
//! ## Sync Complete History
//!
//! Sync full clipboard history between devices:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.cliphistory.sync",
//!     "body": {
//!         "items": [
//!             {
//!                 "id": "uuid-1",
//!                 "content": "clipboard item 1",
//!                 "timestamp": 1640000000000,
//!                 "pinned": false
//!             },
//!             {
//!                 "id": "uuid-2",
//!                 "content": "clipboard item 2",
//!                 "timestamp": 1640000001000,
//!                 "pinned": true
//!             }
//!         ]
//!     }
//! }
//! ```
//!
//! ## Add Clipboard Item
//!
//! Add new item to clipboard history:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.cliphistory.add",
//!     "body": {
//!         "id": "uuid-3",
//!         "content": "new clipboard item",
//!         "timestamp": 1640000002000
//!     }
//! }
//! ```
//!
//! ## Pin/Unpin Item
//!
//! Toggle pin status for important items:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.cliphistory.pin",
//!     "body": {
//!         "id": "uuid-2",
//!         "pinned": true
//!     }
//! }
//! ```
//!
//! ## Delete Item
//!
//! Remove item from history:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.cliphistory.delete",
//!     "body": {
//!         "id": "uuid-1"
//!     }
//! }
//! ```
//!
//! ## Search History
//!
//! Query clipboard history with search term:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.cliphistory.search",
//!     "body": {
//!         "query": "search term",
//!         "limit": 10
//!     }
//! }
//! ```
//!
//! ## Search Results
//!
//! Return search results:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.cliphistory.result",
//!     "body": {
//!         "query": "search term",
//!         "items": [...]
//!     }
//! }
//! ```
//!
//! ## Storage
//!
//! - SQLite database: `~/.local/share/cosmic-connect/clipboard_history.db`
//! - Configurable max items (default: 100)
//! - Configurable retention period (default: 30 days)
//! - Pinned items never auto-deleted
//! - Auto-cleanup on startup
//!
//! ## Configuration
//!
//! ```toml
//! [plugins.clipboardhistory]
//! max_items = 100          # Maximum items to keep
//! retention_days = 30      # Days to keep items
//! max_item_size = 10485760 # 10MB max per item
//! sync_on_connect = true   # Sync history on device connection
//! ```
//!
//! ## Example
//!
//! ```rust,ignore
//! use cosmic_connect_core::plugins::clipboardhistory::*;
//! use cosmic_connect_core::{Plugin, PluginManager};
//!
//! // Create and register plugin
//! let mut manager = PluginManager::new();
//! manager.register(Box::new(ClipboardHistoryPlugin::new().await?))?;
//!
//! // Add item to history
//! let plugin = ClipboardHistoryPlugin::new().await?;
//! plugin.add_item("New clipboard content".to_string()).await?;
//!
//! // Search history
//! let results = plugin.search("content".to_string(), 10).await?;
//! ```

use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::any::Any;
use std::path::PathBuf;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::{Plugin, PluginFactory};

/// Maximum content size (10MB)
const MAX_CONTENT_SIZE: usize = 10 * 1024 * 1024;

/// Default maximum items to keep
const DEFAULT_MAX_ITEMS: usize = 100;

/// Default retention period in days
const DEFAULT_RETENTION_DAYS: i64 = 30;

/// Clipboard history item
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClipboardHistoryItem {
    /// Unique item ID
    pub id: String,

    /// Clipboard content (text)
    pub content: String,

    /// UNIX epoch timestamp in milliseconds
    pub timestamp: i64,

    /// Whether item is pinned (never auto-deleted)
    pub pinned: bool,

    /// Content type (for future image support)
    #[serde(default = "default_content_type")]
    pub content_type: String,
}

fn default_content_type() -> String {
    "text/plain".to_string()
}

impl ClipboardHistoryItem {
    /// Create new clipboard history item
    pub fn new(content: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            content,
            timestamp: Utc::now().timestamp_millis(),
            pinned: false,
            content_type: "text/plain".to_string(),
        }
    }

    /// Create item with explicit ID and timestamp
    pub fn with_id_and_timestamp(id: String, content: String, timestamp: i64) -> Self {
        Self {
            id,
            content,
            timestamp,
            pinned: false,
            content_type: "text/plain".to_string(),
        }
    }

    /// Check if item matches search query
    pub fn matches_query(&self, query: &str) -> bool {
        self.content.to_lowercase().contains(&query.to_lowercase())
    }
}

/// Configuration for clipboard history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardHistoryConfig {
    /// Maximum items to keep
    pub max_items: usize,

    /// Retention period in days
    pub retention_days: i64,

    /// Maximum size per item
    pub max_item_size: usize,

    /// Sync history on device connection
    pub sync_on_connect: bool,
}

impl Default for ClipboardHistoryConfig {
    fn default() -> Self {
        Self {
            max_items: DEFAULT_MAX_ITEMS,
            retention_days: DEFAULT_RETENTION_DAYS,
            max_item_size: MAX_CONTENT_SIZE,
            sync_on_connect: true,
        }
    }
}

/// Clipboard history storage (in-memory for now, TODO: SQLite)
#[derive(Debug, Clone)]
struct ClipboardHistoryStorage {
    items: Vec<ClipboardHistoryItem>,
    config: ClipboardHistoryConfig,
}

impl ClipboardHistoryStorage {
    /// Create new storage with config
    fn new(config: ClipboardHistoryConfig) -> Self {
        Self {
            items: Vec::new(),
            config,
        }
    }

    /// Add item to history
    fn add(&mut self, mut item: ClipboardHistoryItem) -> Result<()> {
        // Check size limit
        if item.content.len() > self.config.max_item_size {
            return Err(ProtocolError::invalid_state(format!(
                "Clipboard item too large: {} bytes (max: {})",
                item.content.len(),
                self.config.max_item_size
            )));
        }

        // Don't add if identical to most recent item
        if let Some(latest) = self.items.first() {
            if latest.content == item.content {
                debug!("Ignoring duplicate clipboard item");
                return Ok(());
            }
        }

        // Add to front (most recent first)
        self.items.insert(0, item);

        // Cleanup old items
        self.cleanup();

        Ok(())
    }

    /// Get item by ID
    fn get(&self, id: &str) -> Option<&ClipboardHistoryItem> {
        self.items.iter().find(|item| item.id == id)
    }

    /// Pin/unpin item
    fn set_pinned(&mut self, id: &str, pinned: bool) -> Result<()> {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.pinned = pinned;
            Ok(())
        } else {
            Err(ProtocolError::invalid_state(format!(
                "Item not found: {}",
                id
            )))
        }
    }

    /// Delete item by ID
    fn delete(&mut self, id: &str) -> Result<()> {
        let original_len = self.items.len();
        self.items.retain(|item| item.id != id);

        if self.items.len() == original_len {
            Err(ProtocolError::invalid_state(format!(
                "Item not found: {}",
                id
            )))
        } else {
            Ok(())
        }
    }

    /// Search items by query
    fn search(&self, query: &str, limit: usize) -> Vec<ClipboardHistoryItem> {
        self.items
            .iter()
            .filter(|item| item.matches_query(query))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Get all items
    fn all(&self) -> Vec<ClipboardHistoryItem> {
        self.items.clone()
    }

    /// Cleanup old and excess items
    fn cleanup(&mut self) {
        let cutoff_time = Utc::now().timestamp_millis()
            - (self.config.retention_days * 24 * 60 * 60 * 1000);

        // Remove old unpinned items
        self.items.retain(|item| {
            item.pinned || item.timestamp > cutoff_time
        });

        // Count pinned items
        let pinned_count = self.items.iter().filter(|i| i.pinned).count();

        // If we have more unpinned items than allowed, remove oldest
        if self.items.len() > self.config.max_items {
            let keep_unpinned = self.config.max_items.saturating_sub(pinned_count);

            let mut pinned = Vec::new();
            let mut unpinned = Vec::new();

            for item in self.items.drain(..) {
                if item.pinned {
                    pinned.push(item);
                } else {
                    unpinned.push(item);
                }
            }

            // Keep only the newest unpinned items
            unpinned.truncate(keep_unpinned);

            // Merge back
            self.items = pinned;
            self.items.extend(unpinned);

            // Re-sort by timestamp (newest first)
            self.items.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        }
    }

    /// Merge items from sync packet
    fn merge(&mut self, incoming_items: Vec<ClipboardHistoryItem>) {
        for incoming in incoming_items {
            // Only add if we don't have this ID already
            if !self.items.iter().any(|item| item.id == incoming.id) {
                self.items.push(incoming);
            }
        }

        // Re-sort by timestamp
        self.items.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        // Cleanup after merge
        self.cleanup();
    }
}

/// Clipboard history plugin for persistent clipboard management
pub struct ClipboardHistoryPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Whether the plugin is enabled
    enabled: bool,

    /// In-memory storage (TODO: SQLite)
    storage: ClipboardHistoryStorage,
}

impl ClipboardHistoryPlugin {
    /// Create a new clipboard history plugin
    pub fn new() -> Self {
        Self::with_config(ClipboardHistoryConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: ClipboardHistoryConfig) -> Self {
        info!(
            "Creating ClipboardHistory plugin with max_items={}, retention_days={}",
            config.max_items, config.retention_days
        );

        Self {
            device_id: None,
            enabled: false,
            storage: ClipboardHistoryStorage::new(config),
        }
    }

    /// Add item to clipboard history
    ///
    /// # Parameters
    ///
    /// - `content`: Clipboard text content
    ///
    /// # Returns
    ///
    /// The created item's ID
    pub fn add_item(&mut self, content: String) -> Result<String> {
        let item = ClipboardHistoryItem::new(content);
        let id = item.id.clone();

        self.storage.add(item)?;

        debug!("Added clipboard item: {}", id);
        Ok(id)
    }

    /// Pin or unpin an item
    pub fn set_pinned(&mut self, id: &str, pinned: bool) -> Result<()> {
        self.storage.set_pinned(id, pinned)?;
        info!("Item {} pinned status: {}", id, pinned);
        Ok(())
    }

    /// Delete an item
    pub fn delete_item(&mut self, id: &str) -> Result<()> {
        self.storage.delete(id)?;
        info!("Deleted clipboard item: {}", id);
        Ok(())
    }

    /// Search clipboard history
    pub fn search(&self, query: &str, limit: usize) -> Vec<ClipboardHistoryItem> {
        self.storage.search(query, limit)
    }

    /// Get all items
    pub fn get_all(&self) -> Vec<ClipboardHistoryItem> {
        self.storage.all()
    }

    /// Create sync packet with all items
    pub fn create_sync_packet(&self) -> Packet {
        let items: Vec<serde_json::Value> = self
            .storage
            .all()
            .into_iter()
            .map(|item| {
                json!({
                    "id": item.id,
                    "content": item.content,
                    "timestamp": item.timestamp,
                    "pinned": item.pinned,
                    "content_type": item.content_type
                })
            })
            .collect();

        Packet::new("cconnect.cliphistory.sync", json!({ "items": items }))
    }

    /// Create add item packet
    pub fn create_add_packet(&self, item: &ClipboardHistoryItem) -> Packet {
        Packet::new(
            "cconnect.cliphistory.add",
            json!({
                "id": item.id,
                "content": item.content,
                "timestamp": item.timestamp,
                "content_type": item.content_type
            }),
        )
    }

    /// Create pin packet
    pub fn create_pin_packet(&self, id: &str, pinned: bool) -> Packet {
        Packet::new(
            "cconnect.cliphistory.pin",
            json!({
                "id": id,
                "pinned": pinned
            }),
        )
    }

    /// Create delete packet
    pub fn create_delete_packet(&self, id: &str) -> Packet {
        Packet::new("cconnect.cliphistory.delete", json!({ "id": id }))
    }

    /// Create search result packet
    pub fn create_result_packet(&self, query: &str, items: Vec<ClipboardHistoryItem>) -> Packet {
        let items_json: Vec<serde_json::Value> = items
            .into_iter()
            .map(|item| {
                json!({
                    "id": item.id,
                    "content": item.content,
                    "timestamp": item.timestamp,
                    "pinned": item.pinned,
                    "content_type": item.content_type
                })
            })
            .collect();

        Packet::new(
            "cconnect.cliphistory.result",
            json!({
                "query": query,
                "items": items_json
            }),
        )
    }

    /// Handle sync packet
    async fn handle_sync(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        info!(
            "Received clipboard history sync from {} ({})",
            device.name(),
            device.id()
        );

        let items_json = packet
            .body
            .get("items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ProtocolError::invalid_state("Missing items array"))?;

        let mut items = Vec::new();
        for item_json in items_json {
            let id = item_json
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ProtocolError::invalid_state("Missing item id"))?;

            let content = item_json
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ProtocolError::invalid_state("Missing item content"))?;

            let timestamp = item_json
                .get("timestamp")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| ProtocolError::invalid_state("Missing item timestamp"))?;

            let pinned = item_json
                .get("pinned")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let mut item = ClipboardHistoryItem::with_id_and_timestamp(
                id.to_string(),
                content.to_string(),
                timestamp,
            );
            item.pinned = pinned;

            items.push(item);
        }

        self.storage.merge(items);

        info!("Merged {} clipboard history items", items_json.len());
        Ok(())
    }

    /// Handle add item packet
    async fn handle_add(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        let id = packet
            .body
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProtocolError::invalid_state("Missing id"))?;

        let content = packet
            .body
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProtocolError::invalid_state("Missing content"))?;

        let timestamp = packet
            .body
            .get("timestamp")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| ProtocolError::invalid_state("Missing timestamp"))?;

        info!(
            "Received clipboard item add from {} ({}): {} chars",
            device.name(),
            device.id(),
            content.len()
        );

        let item = ClipboardHistoryItem::with_id_and_timestamp(
            id.to_string(),
            content.to_string(),
            timestamp,
        );

        self.storage.add(item)?;

        Ok(())
    }

    /// Handle pin packet
    async fn handle_pin(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        let id = packet
            .body
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProtocolError::invalid_state("Missing id"))?;

        let pinned = packet
            .body
            .get("pinned")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        info!(
            "Received pin request from {} ({}): {} -> {}",
            device.name(),
            device.id(),
            id,
            pinned
        );

        self.storage.set_pinned(id, pinned)?;

        Ok(())
    }

    /// Handle delete packet
    async fn handle_delete(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        let id = packet
            .body
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProtocolError::invalid_state("Missing id"))?;

        info!(
            "Received delete request from {} ({}): {}",
            device.name(),
            device.id(),
            id
        );

        self.storage.delete(id)?;

        Ok(())
    }

    /// Handle search packet
    async fn handle_search(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        let query = packet
            .body
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let limit = packet
            .body
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        info!(
            "Received search request from {} ({}): '{}' (limit: {})",
            device.name(),
            device.id(),
            query,
            limit
        );

        let results = self.storage.search(query, limit);

        info!("Found {} matching items", results.len());

        // TODO: Send result packet back to device
        // Need packet sending infrastructure
        // let response = self.create_result_packet(query, results);
        // device.send_packet(&response).await?;

        Ok(())
    }
}

impl Default for ClipboardHistoryPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ClipboardHistoryPlugin {
    fn name(&self) -> &str {
        "clipboardhistory"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.cliphistory.sync".to_string(),
            "cconnect.cliphistory.add".to_string(),
            "cconnect.cliphistory.pin".to_string(),
            "cconnect.cliphistory.delete".to_string(),
            "cconnect.cliphistory.search".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.cliphistory.sync".to_string(),
            "cconnect.cliphistory.add".to_string(),
            "cconnect.cliphistory.result".to_string(),
        ]
    }

    async fn init(&mut self, device: &Device) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!(
            "ClipboardHistory plugin initialized for device {}",
            device.name()
        );
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("ClipboardHistory plugin started");
        self.enabled = true;

        // Cleanup on start
        self.storage.cleanup();

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("ClipboardHistory plugin stopped");
        self.enabled = false;
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("ClipboardHistory plugin is disabled, ignoring packet");
            return Ok(());
        }

        match packet.packet_type.as_str() {
            "cconnect.cliphistory.sync" => self.handle_sync(packet, device).await,
            "cconnect.cliphistory.add" => self.handle_add(packet, device).await,
            "cconnect.cliphistory.pin" => self.handle_pin(packet, device).await,
            "cconnect.cliphistory.delete" => self.handle_delete(packet, device).await,
            "cconnect.cliphistory.search" => self.handle_search(packet, device).await,
            _ => {
                warn!("Unknown packet type: {}", packet.packet_type);
                Ok(())
            }
        }
    }
}

/// Factory for creating ClipboardHistory plugin instances
pub struct ClipboardHistoryPluginFactory;

impl PluginFactory for ClipboardHistoryPluginFactory {
    fn create(&self) -> Box<dyn Plugin> {
        Box::new(ClipboardHistoryPlugin::new())
    }

    fn name(&self) -> &str {
        "clipboardhistory"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.cliphistory.sync".to_string(),
            "cconnect.cliphistory.add".to_string(),
            "cconnect.cliphistory.pin".to_string(),
            "cconnect.cliphistory.delete".to_string(),
            "cconnect.cliphistory.search".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.cliphistory.sync".to_string(),
            "cconnect.cliphistory.add".to_string(),
            "cconnect.cliphistory.result".to_string(),
        ]
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
    fn test_clipboard_item_creation() {
        let item = ClipboardHistoryItem::new("Test content".to_string());
        assert_eq!(item.content, "Test content");
        assert!(!item.id.is_empty());
        assert!(item.timestamp > 0);
        assert!(!item.pinned);
    }

    #[test]
    fn test_clipboard_item_matches_query() {
        let item = ClipboardHistoryItem::new("Hello World".to_string());
        assert!(item.matches_query("hello"));
        assert!(item.matches_query("WORLD"));
        assert!(item.matches_query("llo Wo"));
        assert!(!item.matches_query("goodbye"));
    }

    #[test]
    fn test_plugin_creation() {
        let plugin = ClipboardHistoryPlugin::new();
        assert_eq!(plugin.name(), "clipboardhistory");
        assert!(!plugin.enabled);
    }

    #[test]
    fn test_add_item() {
        let mut plugin = ClipboardHistoryPlugin::new();
        let id = plugin.add_item("Test content".to_string()).unwrap();
        assert!(!id.is_empty());

        let items = plugin.get_all();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].content, "Test content");
    }

    #[test]
    fn test_duplicate_prevention() {
        let mut plugin = ClipboardHistoryPlugin::new();
        plugin.add_item("Same content".to_string()).unwrap();
        plugin.add_item("Same content".to_string()).unwrap();

        let items = plugin.get_all();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_pin_unpin() {
        let mut plugin = ClipboardHistoryPlugin::new();
        let id = plugin.add_item("Test content".to_string()).unwrap();

        plugin.set_pinned(&id, true).unwrap();
        let items = plugin.get_all();
        assert!(items[0].pinned);

        plugin.set_pinned(&id, false).unwrap();
        let items = plugin.get_all();
        assert!(!items[0].pinned);
    }

    #[test]
    fn test_delete_item() {
        let mut plugin = ClipboardHistoryPlugin::new();
        let id = plugin.add_item("Test content".to_string()).unwrap();

        assert_eq!(plugin.get_all().len(), 1);

        plugin.delete_item(&id).unwrap();
        assert_eq!(plugin.get_all().len(), 0);
    }

    #[test]
    fn test_search() {
        let mut plugin = ClipboardHistoryPlugin::new();
        plugin.add_item("Hello World".to_string()).unwrap();
        plugin.add_item("Goodbye World".to_string()).unwrap();
        plugin.add_item("Test content".to_string()).unwrap();

        let results = plugin.search("world", 10);
        assert_eq!(results.len(), 2);

        let results = plugin.search("test", 10);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_max_items_limit() {
        let config = ClipboardHistoryConfig {
            max_items: 3,
            retention_days: 30,
            max_item_size: MAX_CONTENT_SIZE,
            sync_on_connect: true,
        };

        let mut plugin = ClipboardHistoryPlugin::with_config(config);

        plugin.add_item("Item 1".to_string()).unwrap();
        plugin.add_item("Item 2".to_string()).unwrap();
        plugin.add_item("Item 3".to_string()).unwrap();
        plugin.add_item("Item 4".to_string()).unwrap();

        let items = plugin.get_all();
        assert_eq!(items.len(), 3);

        // Most recent should be first
        assert_eq!(items[0].content, "Item 4");
    }

    #[test]
    fn test_pinned_items_preserved() {
        let config = ClipboardHistoryConfig {
            max_items: 2,
            retention_days: 30,
            max_item_size: MAX_CONTENT_SIZE,
            sync_on_connect: true,
        };

        let mut plugin = ClipboardHistoryPlugin::with_config(config);

        let id1 = plugin.add_item("Pinned item".to_string()).unwrap();
        plugin.set_pinned(&id1, true).unwrap();

        plugin.add_item("Item 2".to_string()).unwrap();
        plugin.add_item("Item 3".to_string()).unwrap();

        let items = plugin.get_all();
        // Should keep pinned item + 1 unpinned item
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.content == "Pinned item"));
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = ClipboardHistoryPlugin::new();
        let device = create_test_device();

        assert!(plugin.init(&device).await.is_ok());
        assert_eq!(plugin.device_id, Some(device.id().to_string()));

        assert!(plugin.start().await.is_ok());
        assert!(plugin.enabled);

        assert!(plugin.stop().await.is_ok());
        assert!(!plugin.enabled);
    }

    #[tokio::test]
    async fn test_handle_add_packet() {
        let mut plugin = ClipboardHistoryPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();
        plugin.start().await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.cliphistory.add",
            json!({
                "id": "test-id",
                "content": "Added content",
                "timestamp": 1640000000000i64
            }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        let items = plugin.get_all();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].content, "Added content");
    }

    #[tokio::test]
    async fn test_handle_pin_packet() {
        let mut plugin = ClipboardHistoryPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();
        plugin.start().await.unwrap();

        let id = plugin.add_item("Test content".to_string()).unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.cliphistory.pin",
            json!({
                "id": id,
                "pinned": true
            }),
        );

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        let items = plugin.get_all();
        assert!(items[0].pinned);
    }

    #[tokio::test]
    async fn test_handle_delete_packet() {
        let mut plugin = ClipboardHistoryPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();
        plugin.start().await.unwrap();

        let id = plugin.add_item("Test content".to_string()).unwrap();
        assert_eq!(plugin.get_all().len(), 1);

        let mut device = create_test_device();
        let packet = Packet::new("cconnect.cliphistory.delete", json!({ "id": id }));

        plugin.handle_packet(&packet, &mut device).await.unwrap();

        assert_eq!(plugin.get_all().len(), 0);
    }

    #[test]
    fn test_capabilities() {
        let plugin = ClipboardHistoryPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 5);
        assert!(incoming.contains(&"cconnect.cliphistory.sync".to_string()));
        assert!(incoming.contains(&"cconnect.cliphistory.add".to_string()));
        assert!(incoming.contains(&"cconnect.cliphistory.pin".to_string()));
        assert!(incoming.contains(&"cconnect.cliphistory.delete".to_string()));
        assert!(incoming.contains(&"cconnect.cliphistory.search".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 3);
        assert!(outgoing.contains(&"cconnect.cliphistory.sync".to_string()));
        assert!(outgoing.contains(&"cconnect.cliphistory.add".to_string()));
        assert!(outgoing.contains(&"cconnect.cliphistory.result".to_string()));
    }
}
