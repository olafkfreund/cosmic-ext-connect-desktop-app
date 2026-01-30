//! SQLite Storage Backend for Clipboard History Plugin
//!
//! Provides persistent storage for clipboard history items using SQLite.
//! Items are stored with configurable retention and pin support.
//!
//! ## Database Schema
//!
//! ```sql
//! CREATE TABLE clipboard_items (
//!     id TEXT PRIMARY KEY,
//!     content TEXT NOT NULL,
//!     timestamp INTEGER NOT NULL,
//!     pinned INTEGER NOT NULL DEFAULT 0,
//!     content_type TEXT NOT NULL DEFAULT 'text/plain',
//!     created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
//! );
//!
//! CREATE INDEX idx_clipboard_timestamp ON clipboard_items(timestamp DESC);
//! CREATE INDEX idx_clipboard_pinned ON clipboard_items(pinned);
//! ```
//!
//! ## Storage Location
//!
//! Default path: `~/.local/share/cosmic-connect/clipboard_history.db`

use rusqlite::{params, Connection, Result as SqliteResult};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

use super::clipboardhistory::{ClipboardHistoryConfig, ClipboardHistoryItem};

/// SQLite-backed clipboard history storage
pub struct ClipboardSqliteStorage {
    /// Database connection
    conn: Arc<Mutex<Connection>>,
    /// Configuration
    config: ClipboardHistoryConfig,
}

impl ClipboardSqliteStorage {
    /// Create new storage with default database path
    ///
    /// # Returns
    /// New storage instance or error
    pub fn new(config: ClipboardHistoryConfig) -> Result<Self, String> {
        let db_path = Self::get_db_path()?;
        Self::new_with_path(config, &db_path)
    }

    /// Create storage with explicit database path (for testing)
    pub fn new_with_path(config: ClipboardHistoryConfig, db_path: &PathBuf) -> Result<Self, String> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create db directory: {}", e))?;
        }

        let conn =
            Connection::open(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

        let storage = Self {
            conn: Arc::new(Mutex::new(conn)),
            config,
        };

        storage.init_schema()?;
        Ok(storage)
    }

    /// Get the default database path
    fn get_db_path() -> Result<PathBuf, String> {
        let data_dir = dirs::data_local_dir()
            .ok_or_else(|| "Could not determine local data directory".to_string())?;
        Ok(data_dir.join("cosmic-connect").join("clipboard_history.db"))
    }

    /// Initialize database schema
    fn init_schema(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS clipboard_items (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                pinned INTEGER NOT NULL DEFAULT 0,
                content_type TEXT NOT NULL DEFAULT 'text/plain',
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            );

            CREATE INDEX IF NOT EXISTS idx_clipboard_timestamp
                ON clipboard_items(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_clipboard_pinned
                ON clipboard_items(pinned);
            CREATE INDEX IF NOT EXISTS idx_clipboard_content
                ON clipboard_items(content);
            "#,
        )
        .map_err(|e| format!("Failed to create schema: {}", e))?;

        debug!("Clipboard history database schema initialized");
        Ok(())
    }

    /// Add an item to storage
    ///
    /// Returns Ok(true) if item was added, Ok(false) if duplicate
    pub fn add(&self, item: &ClipboardHistoryItem) -> Result<bool, String> {
        // Check size limit
        if item.content.len() > self.config.max_item_size {
            return Err(format!(
                "Clipboard item too large: {} bytes (max: {})",
                item.content.len(),
                self.config.max_item_size
            ));
        }

        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        // Check if identical to most recent item
        let is_duplicate: bool = conn
            .query_row(
                "SELECT content = ?1 FROM clipboard_items ORDER BY timestamp DESC LIMIT 1",
                params![item.content],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if is_duplicate {
            debug!("Ignoring duplicate clipboard item");
            return Ok(false);
        }

        conn.execute(
            r#"
            INSERT OR REPLACE INTO clipboard_items
                (id, content, timestamp, pinned, content_type)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                item.id,
                item.content,
                item.timestamp,
                item.pinned as i32,
                item.content_type,
            ],
        )
        .map_err(|e| format!("Failed to insert item: {}", e))?;

        debug!("Added clipboard item: {}", item.id);

        // Cleanup old items
        drop(conn);
        self.cleanup()?;

        Ok(true)
    }

    /// Get an item by ID
    pub fn get(&self, id: &str) -> Result<Option<ClipboardHistoryItem>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, content, timestamp, pinned, content_type
                FROM clipboard_items
                WHERE id = ?1
                "#,
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        stmt.query_row(params![id], row_to_item)
            .optional()
            .map_err(|e| format!("Failed to query item: {}", e))
    }

    /// Set pinned status for an item
    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<bool, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        let rows = conn
            .execute(
                "UPDATE clipboard_items SET pinned = ?1 WHERE id = ?2",
                params![pinned as i32, id],
            )
            .map_err(|e| format!("Failed to update item: {}", e))?;

        if rows > 0 {
            info!("Item {} pinned status: {}", id, pinned);
            Ok(true)
        } else {
            warn!("Item {} not found", id);
            Ok(false)
        }
    }

    /// Delete an item by ID
    pub fn delete(&self, id: &str) -> Result<bool, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        let rows = conn
            .execute("DELETE FROM clipboard_items WHERE id = ?1", params![id])
            .map_err(|e| format!("Failed to delete item: {}", e))?;

        if rows > 0 {
            info!("Deleted clipboard item: {}", id);
            Ok(true)
        } else {
            warn!("Item {} not found for deletion", id);
            Ok(false)
        }
    }

    /// Search items by query (case-insensitive)
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<ClipboardHistoryItem>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, content, timestamp, pinned, content_type
                FROM clipboard_items
                WHERE content LIKE '%' || ?1 || '%' COLLATE NOCASE
                ORDER BY timestamp DESC
                LIMIT ?2
                "#,
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let items = stmt
            .query_map(params![query, limit as i64], row_to_item)
            .map_err(|e| format!("Failed to search items: {}", e))?
            .filter_map(Result::ok)
            .collect();

        Ok(items)
    }

    /// Get all items (ordered by timestamp descending)
    pub fn all(&self) -> Result<Vec<ClipboardHistoryItem>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, content, timestamp, pinned, content_type
                FROM clipboard_items
                ORDER BY timestamp DESC
                "#,
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let items = stmt
            .query_map([], row_to_item)
            .map_err(|e| format!("Failed to get items: {}", e))?
            .filter_map(Result::ok)
            .collect();

        Ok(items)
    }

    /// Get items with pagination
    pub fn get_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ClipboardHistoryItem>, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, content, timestamp, pinned, content_type
                FROM clipboard_items
                ORDER BY timestamp DESC
                LIMIT ?1 OFFSET ?2
                "#,
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let items = stmt
            .query_map(params![limit as i64, offset as i64], row_to_item)
            .map_err(|e| format!("Failed to get page: {}", e))?
            .filter_map(Result::ok)
            .collect();

        Ok(items)
    }

    /// Count total items
    pub fn count(&self) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM clipboard_items", [], |row| row.get(0))
            .map_err(|e| format!("Failed to count items: {}", e))?;

        Ok(count as usize)
    }

    /// Merge items from sync (add items that don't exist)
    pub fn merge(&self, items: Vec<ClipboardHistoryItem>) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        let mut added = 0;
        for item in items {
            // Use INSERT OR IGNORE to skip existing IDs
            let rows = conn
                .execute(
                    r#"
                    INSERT OR IGNORE INTO clipboard_items
                        (id, content, timestamp, pinned, content_type)
                    VALUES (?1, ?2, ?3, ?4, ?5)
                    "#,
                    params![
                        item.id,
                        item.content,
                        item.timestamp,
                        item.pinned as i32,
                        item.content_type,
                    ],
                )
                .map_err(|e| format!("Failed to merge item: {}", e))?;

            if rows > 0 {
                added += 1;
            }
        }

        if added > 0 {
            debug!("Merged {} new items from sync", added);
            drop(conn);
            self.cleanup()?;
        }

        Ok(added)
    }

    /// Cleanup old and excess items based on retention policy
    pub fn cleanup(&self) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        let cutoff_time = chrono::Utc::now().timestamp_millis()
            - (self.config.retention_days * 24 * 60 * 60 * 1000);

        // Delete old unpinned items
        let deleted_old = conn
            .execute(
                r#"
                DELETE FROM clipboard_items
                WHERE pinned = 0 AND timestamp < ?1
                "#,
                params![cutoff_time],
            )
            .map_err(|e| format!("Failed to delete old items: {}", e))?;

        // Count pinned items
        let pinned_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_items WHERE pinned = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // Calculate how many unpinned items we can keep
        let max_unpinned = (self.config.max_items as i64).saturating_sub(pinned_count);

        // Delete excess unpinned items (keeping newest)
        let deleted_excess = conn
            .execute(
                r#"
                DELETE FROM clipboard_items
                WHERE pinned = 0 AND id NOT IN (
                    SELECT id FROM clipboard_items
                    WHERE pinned = 0
                    ORDER BY timestamp DESC
                    LIMIT ?1
                )
                "#,
                params![max_unpinned],
            )
            .map_err(|e| format!("Failed to limit items: {}", e))?;

        let total_deleted = deleted_old + deleted_excess;
        if total_deleted > 0 {
            info!(
                "Cleaned up {} clipboard items ({} old, {} excess)",
                total_deleted, deleted_old, deleted_excess
            );
        }

        Ok(total_deleted)
    }

    /// Clear all items
    pub fn clear(&self) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {}", e))?;

        let deleted = conn
            .execute("DELETE FROM clipboard_items", [])
            .map_err(|e| format!("Failed to clear items: {}", e))?;

        info!("Cleared {} clipboard items", deleted);
        Ok(deleted)
    }
}

/// Extension trait for rusqlite to support optional results
trait OptionalExt<T> {
    fn optional(self) -> SqliteResult<Option<T>>;
}

impl<T> OptionalExt<T> for SqliteResult<T> {
    fn optional(self) -> SqliteResult<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Map a database row to a ClipboardHistoryItem
fn row_to_item(row: &rusqlite::Row) -> rusqlite::Result<ClipboardHistoryItem> {
    Ok(ClipboardHistoryItem {
        id: row.get(0)?,
        content: row.get(1)?,
        timestamp: row.get(2)?,
        pinned: row.get::<_, i32>(3)? != 0,
        content_type: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_storage() -> (ClipboardSqliteStorage, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_clipboard.db");
        let config = ClipboardHistoryConfig::default();
        let storage = ClipboardSqliteStorage::new_with_path(config, &db_path).unwrap();
        (storage, temp_dir)
    }

    #[test]
    fn test_add_and_get_item() {
        let (storage, _temp) = create_test_storage();

        let item = ClipboardHistoryItem::new("Test content".to_string());
        let item_id = item.id.clone();

        let added = storage.add(&item).unwrap();
        assert!(added);

        let retrieved = storage.get(&item_id).unwrap().unwrap();
        assert_eq!(retrieved.id, item_id);
        assert_eq!(retrieved.content, "Test content");
        assert!(!retrieved.pinned);
    }

    #[test]
    fn test_duplicate_detection() {
        let (storage, _temp) = create_test_storage();

        let item1 = ClipboardHistoryItem::new("Same content".to_string());
        let item2 = ClipboardHistoryItem::new("Same content".to_string());

        let added1 = storage.add(&item1).unwrap();
        let added2 = storage.add(&item2).unwrap();

        assert!(added1);
        assert!(!added2); // Should be detected as duplicate
    }

    #[test]
    fn test_pin_unpin() {
        let (storage, _temp) = create_test_storage();

        let item = ClipboardHistoryItem::new("Pin me".to_string());
        let item_id = item.id.clone();
        storage.add(&item).unwrap();

        assert!(!storage.get(&item_id).unwrap().unwrap().pinned);

        storage.set_pinned(&item_id, true).unwrap();
        assert!(storage.get(&item_id).unwrap().unwrap().pinned);

        storage.set_pinned(&item_id, false).unwrap();
        assert!(!storage.get(&item_id).unwrap().unwrap().pinned);
    }

    #[test]
    fn test_delete() {
        let (storage, _temp) = create_test_storage();

        let item = ClipboardHistoryItem::new("Delete me".to_string());
        let item_id = item.id.clone();
        storage.add(&item).unwrap();

        assert!(storage.get(&item_id).unwrap().is_some());

        let deleted = storage.delete(&item_id).unwrap();
        assert!(deleted);

        assert!(storage.get(&item_id).unwrap().is_none());
    }

    #[test]
    fn test_search() {
        let (storage, _temp) = create_test_storage();

        storage
            .add(&ClipboardHistoryItem::new("Hello world".to_string()))
            .unwrap();
        storage
            .add(&ClipboardHistoryItem::new("Goodbye world".to_string()))
            .unwrap();
        storage
            .add(&ClipboardHistoryItem::new("Hello there".to_string()))
            .unwrap();

        let results = storage.search("hello", 10).unwrap();
        assert_eq!(results.len(), 2);

        let results = storage.search("world", 10).unwrap();
        assert_eq!(results.len(), 2);

        let results = storage.search("goodbye", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_all_items() {
        let (storage, _temp) = create_test_storage();

        // Use realistic timestamps
        let base_time = chrono::Utc::now().timestamp_millis();

        for i in 0..5 {
            let mut item = ClipboardHistoryItem::new(format!("Item {}", i));
            item.timestamp = base_time + i as i64 * 1000;
            storage.add(&item).unwrap();
        }

        let all = storage.all().unwrap();
        assert_eq!(all.len(), 5);

        // Should be ordered by timestamp descending
        assert!(all[0].timestamp > all[4].timestamp);
    }

    #[test]
    fn test_merge() {
        let (storage, _temp) = create_test_storage();

        let item1 = ClipboardHistoryItem::new("Local item".to_string());
        let item1_id = item1.id.clone();
        storage.add(&item1).unwrap();

        let remote_items = vec![
            ClipboardHistoryItem::new("Remote item 1".to_string()),
            ClipboardHistoryItem::new("Remote item 2".to_string()),
            item1.clone(), // Should be ignored (duplicate ID)
        ];

        let added = storage.merge(remote_items).unwrap();
        assert_eq!(added, 2); // Only 2 new items

        assert_eq!(storage.count().unwrap(), 3);
        assert!(storage.get(&item1_id).unwrap().is_some());
    }

    #[test]
    fn test_clear() {
        let (storage, _temp) = create_test_storage();

        for i in 0..5 {
            storage
                .add(&ClipboardHistoryItem::new(format!("Item {}", i)))
                .unwrap();
        }

        assert_eq!(storage.count().unwrap(), 5);

        storage.clear().unwrap();
        assert_eq!(storage.count().unwrap(), 0);
    }

    #[test]
    fn test_cleanup_respects_pins() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_clipboard.db");
        let config = ClipboardHistoryConfig {
            max_items: 3,
            ..Default::default()
        };
        let storage = ClipboardSqliteStorage::new_with_path(config, &db_path).unwrap();

        // Add 5 items
        for i in 0..5 {
            let mut item = ClipboardHistoryItem::new(format!("Item {}", i));
            item.timestamp = chrono::Utc::now().timestamp_millis() + i as i64 * 1000;
            storage.add(&item).unwrap();
        }

        // Pin one item
        let all = storage.all().unwrap();
        let pinned_id = all[2].id.clone();
        storage.set_pinned(&pinned_id, true).unwrap();

        // Cleanup should keep pinned + 2 newest unpinned
        storage.cleanup().unwrap();

        let remaining = storage.all().unwrap();
        assert!(remaining.len() <= 3);
        assert!(remaining.iter().any(|i| i.id == pinned_id));
    }
}
