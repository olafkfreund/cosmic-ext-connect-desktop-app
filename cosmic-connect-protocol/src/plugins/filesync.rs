//! File Sync Plugin
//!
//! Automatic file synchronization between connected desktops.
//!
//! ## Protocol Specification
//!
//! This plugin implements automatic file synchronization similar to Syncthing,
//! enabling users to keep specific folders synchronized across multiple desktops.
//!
//! ### Packet Types
//!
//! - `cconnect.filesync.config` - Sync folder configuration
//! - `cconnect.filesync.index` - File list with hashes and metadata
//! - `cconnect.filesync.transfer` - File data transfer (via payload)
//! - `cconnect.filesync.request` - Request file transfer
//! - `cconnect.filesync.conflict` - Conflict notification
//! - `cconnect.filesync.delete` - File deletion synchronization
//!
//! ### Capabilities
//!
//! - Incoming: `cconnect.filesync` - Can receive file sync operations
//! - Outgoing: `cconnect.filesync` - Can send file sync operations
//!
//! ### Use Cases
//!
//! - Keep work directories synchronized across machines
//! - Automatic backup to another desktop
//! - Collaborative file sharing between desktops
//! - Project synchronization for development
//!
//! ## Features
//!
//! - **Bidirectional Sync**: Automatic two-way synchronization
//! - **Real-time Watching**: inotify-based file system monitoring
//! - **Conflict Resolution**: Multiple strategies for handling conflicts
//! - **Selective Sync**: Ignore patterns and filters
//! - **File Versioning**: Keep previous versions of files
//! - **Delta Sync**: Only transfer changed parts (rsync algorithm)
//! - **Bandwidth Limiting**: Control network usage
//! - **Hash Comparison**: Fast content comparison with BLAKE3
//!
//! ## Conflict Resolution Strategies
//!
//! - **LastModifiedWins**: Use most recently modified file (default)
//! - **KeepBoth**: Rename conflicting file with timestamp
//! - **Manual**: Prompt user for resolution
//! - **SizeBased**: Keep larger file
//!
//! ## Implementation Status
//!
//! - [x] File system monitoring (notify integration)
//! - [x] BLAKE3 hashing for content comparison
//! - [x] Sync logic and plan generation
//! - [ ] File transfer implementation (upload/download)
//! - [ ] SQLite database for sync state (history)
//! - [ ] Delta sync algorithm (rsync-like)
//! - [ ] File versioning system
//! - [ ] Bandwidth limiting implementation

use crate::payload::{PayloadClient, PayloadServer};
use crate::plugins::{Plugin, PluginFactory};
use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::Sender;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use walkdir::WalkDir;

const PLUGIN_NAME: &str = "filesync";
const INCOMING_CAPABILITY: &str = "cconnect.filesync";
const OUTGOING_CAPABILITY: &str = "cconnect.filesync";

// File sync configuration constants
#[allow(dead_code)]
const MAX_FILE_SIZE_MB: u64 = 1024; // 1GB max file size
const DEFAULT_SCAN_INTERVAL_SECS: u64 = 60; // Scan every minute
const DEFAULT_VERSION_KEEP: usize = 5; // Keep 5 previous versions

/// Conflict resolution strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStrategy {
    /// Use the most recently modified file (default)
    LastModifiedWins,
    /// Keep both files, rename with timestamp
    KeepBoth,
    /// Prompt user for manual resolution
    Manual,
    /// Keep larger file
    SizeBased,
}

impl Default for ConflictStrategy {
    fn default() -> Self {
        Self::LastModifiedWins
    }
}

impl ConflictStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LastModifiedWins => "last_modified_wins",
            Self::KeepBoth => "keep_both",
            Self::Manual => "manual",
            Self::SizeBased => "size_based",
        }
    }
}

/// Sync folder configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFolder {
    /// Local path to sync
    /// Folder identifier
    #[serde(rename = "folderId")]
    pub folder_id: String,

    #[serde(rename = "localPath")]
    pub local_path: PathBuf,

    /// Remote path on other device
    #[serde(rename = "remotePath")]
    pub remote_path: PathBuf,

    /// Whether sync is enabled for this folder
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Bidirectional sync (if false, only push)
    #[serde(default = "default_true")]
    pub bidirectional: bool,

    /// Ignore patterns (gitignore-style)
    #[serde(rename = "ignorePatterns", default)]
    pub ignore_patterns: Vec<String>,

    /// Conflict resolution strategy
    #[serde(rename = "conflictStrategy", default)]
    pub conflict_strategy: ConflictStrategy,

    /// Enable file versioning
    #[serde(default = "default_true")]
    pub versioning: bool,

    /// Number of versions to keep
    #[serde(rename = "versionKeep", default = "default_version_keep")]
    pub version_keep: usize,

    /// Scan interval in seconds (0 = real-time watching only)
    #[serde(rename = "scanIntervalSecs", default = "default_scan_interval")]
    pub scan_interval_secs: u64,

    /// Bandwidth limit in KB/s (0 = unlimited)
    #[serde(rename = "bandwidthLimitKbps", default)]
    pub bandwidth_limit_kbps: u32,
}

fn default_true() -> bool {
    true
}

fn default_version_keep() -> usize {
    DEFAULT_VERSION_KEEP
}

fn default_scan_interval() -> u64 {
    DEFAULT_SCAN_INTERVAL_SECS
}

impl SyncFolder {
    pub fn validate(&self) -> Result<()> {
        if !self.local_path.exists() {
            return Err(ProtocolError::InvalidPacket(format!(
                "Local path does not exist: {}",
                self.local_path.display()
            )));
        }

        if !self.local_path.is_dir() {
            return Err(ProtocolError::InvalidPacket(format!(
                "Local path is not a directory: {}",
                self.local_path.display()
            )));
        }

        if self.version_keep == 0 && self.versioning {
            return Err(ProtocolError::InvalidPacket(
                "version_keep must be > 0 when versioning is enabled".to_string(),
            ));
        }

        Ok(())
    }
}

/// File metadata for sync index
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMetadata {
    /// Relative path from sync folder root
    pub path: PathBuf,

    /// File size in bytes
    pub size: u64,

    /// Last modified timestamp (milliseconds since epoch)
    pub modified: i64,

    /// BLAKE3 hash of file content
    pub hash: String,

    /// Whether this is a directory
    #[serde(rename = "isDir")]
    pub is_dir: bool,

    /// File permissions (Unix mode)
    #[serde(default)]
    pub permissions: Option<u32>,
}

/// Sync index containing all files in a folder
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncIndex {
    /// Folder identifier
    #[serde(rename = "folderId")]
    pub folder_id: String,

    /// Files in this folder
    pub files: Vec<FileMetadata>,

    /// Index generation timestamp (milliseconds since epoch)
    pub timestamp: i64,

    /// Total size of all files
    #[serde(rename = "totalSize")]
    pub total_size: u64,

    /// Number of files
    #[serde(rename = "fileCount")]
    pub file_count: usize,
}

/// File conflict information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileConflict {
    /// Folder identifier
    #[serde(rename = "folderId")]
    pub folder_id: String,

    /// File path
    pub path: PathBuf,

    /// Local file metadata
    #[serde(rename = "localMetadata")]
    pub local_metadata: FileMetadata,

    /// Remote file metadata
    #[serde(rename = "remoteMetadata")]
    pub remote_metadata: FileMetadata,

    /// Suggested resolution
    #[serde(rename = "suggestedStrategy")]
    pub suggested_strategy: ConflictStrategy,

    /// Conflict detection timestamp (milliseconds since epoch)
    pub timestamp: i64,
}

/// Action to perform during synchronization
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAction {
    Upload(PathBuf),
    Download(PathBuf),
    DeleteRemote(PathBuf),
    DeleteLocal(PathBuf),
    Conflict(FileConflict),
}

/// Synchronization plan
#[derive(Debug, Clone, Default)]
pub struct SyncPlan {
    pub actions: Vec<SyncAction>,
    pub stats: SyncStats,
}

#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    pub files_to_upload: usize,
    pub files_to_download: usize,
    pub bytes_to_upload: u64,
    pub bytes_to_download: u64,
    pub conflicts: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct FileSyncConfig {
    sync_folders: HashMap<String, SyncFolder>,
}

/// File sync plugin
pub struct FileSyncPlugin {
    /// Device ID this plugin is associated with
    device_id: Option<String>,

    /// Plugin enabled state
    enabled: bool,

    /// Configured sync folders
    sync_folders: Arc<RwLock<HashMap<String, SyncFolder>>>,

    /// Current sync index by folder ID
    sync_indexes: HashMap<String, SyncIndex>,

    /// Pending conflicts
    pending_conflicts: Vec<FileConflict>,

    /// Active transfers (folder_id -> file_path)
    active_transfers: HashMap<String, Vec<PathBuf>>,

    /// File system watcher
    watcher: Option<RecommendedWatcher>,

    /// Watcher task handle
    watcher_handle: Option<tokio::task::JoinHandle<()>>,

    /// Packet sender for proactive updates
    packet_sender: Option<Sender<(String, Packet)>>,

    /// Path to configuration file
    config_path: Option<PathBuf>,
}

impl FileSyncPlugin {
    /// Create new file sync plugin instance
    pub fn new() -> Self {
        Self {
            device_id: None,
            enabled: false,
            sync_folders: Arc::new(RwLock::new(HashMap::new())),
            sync_indexes: HashMap::new(),
            pending_conflicts: Vec::new(),
            active_transfers: HashMap::new(),
            watcher: None,
            watcher_handle: None,
            packet_sender: None,
            config_path: None,
        }
    }

    /// Get the configuration file path for a device
    fn get_config_path(device_id: &str) -> Result<PathBuf> {
        let home_dir = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not determine home directory",
                ))
            })?;

        let plugin_dir = PathBuf::from(home_dir)
            .join(".config")
            .join("cconnect")
            .join(device_id)
            .join("filesync");

        Ok(plugin_dir.join("config.json"))
    }

    /// Load configuration from disk
    async fn load_config(&self) -> Result<()> {
        if let Some(config_path) = &self.config_path {
            if config_path.exists() {
                let contents = tokio::fs::read_to_string(config_path).await.map_err(|e| {
                    ProtocolError::Plugin(format!("Failed to read config file: {}", e))
                })?;

                let loaded_config: FileSyncConfig =
                    serde_json::from_str(&contents).map_err(|e| {
                        ProtocolError::Plugin(format!("Failed to parse config file: {}", e))
                    })?;

                let mut folders = self.sync_folders.write().await;
                *folders = loaded_config.sync_folders;

                info!("Loaded {} sync folders from config", folders.len());
            } else {
                debug!("Config file does not exist yet: {:?}", config_path);
            }
        }
        Ok(())
    }

    /// Save configuration to disk
    async fn save_config(&self) -> Result<()> {
        if let Some(config_path) = &self.config_path {
            if let Some(parent) = config_path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    ProtocolError::Plugin(format!("Failed to create config directory: {}", e))
                })?;
            }

            let folders = self.sync_folders.read().await;
            let config = FileSyncConfig {
                sync_folders: folders.clone(),
            };

            let contents = serde_json::to_string_pretty(&config)
                .map_err(|e| ProtocolError::Plugin(format!("Failed to serialize config: {}", e)))?;

            tokio::fs::write(config_path, contents).await.map_err(|e| {
                ProtocolError::Plugin(format!("Failed to write config file: {}", e))
            })?;

            debug!("Saved configuration to {:?}", config_path);
        }
        Ok(())
    }

    /// Add or update sync folder configuration
    pub async fn configure_folder(
        &mut self,
        folder_id: String,
        local_path: PathBuf,
        conflict_strategy: ConflictStrategy,
    ) -> Result<()> {
        let config = SyncFolder {
            folder_id: folder_id.clone(),
            local_path: local_path.clone(),
            remote_path: PathBuf::new(), // TODO: Allow configuring remote path
            enabled: true,
            bidirectional: true,
            ignore_patterns: Vec::new(),
            conflict_strategy,
            versioning: false,
            version_keep: DEFAULT_VERSION_KEEP,
            scan_interval_secs: DEFAULT_SCAN_INTERVAL_SECS,
            bandwidth_limit_kbps: 0,
        };

        config.validate()?;

        info!(
            "Configuring sync folder '{}': {}",
            folder_id,
            config.local_path.display()
        );

        {
            let mut folders = self.sync_folders.write().await;
            folders.insert(folder_id.clone(), config.clone());
        }

        // Start watching if plugin is enabled
        if self.enabled {
            if let Some(watcher) = &mut self.watcher {
                if let Err(e) = watcher.watch(&config.local_path, RecursiveMode::Recursive) {
                    warn!(
                        "Failed to watch folder {}: {}",
                        config.local_path.display(),
                        e
                    );
                } else {
                    info!("Started watching folder: {}", config.local_path.display());
                }
            }
        }

        // TODO: Trigger initial index generation

        self.save_config().await?;

        Ok(())
    }

    /// Remove sync folder configuration
    pub async fn remove_folder(&mut self, folder_id: &str) -> Result<()> {
        let config = {
            let mut folders = self.sync_folders.write().await;
            folders.remove(folder_id)
        };

        if let Some(config) = config {
            // Clean up related data
            self.sync_indexes.remove(folder_id);
            self.active_transfers.remove(folder_id);
            self.pending_conflicts.retain(|c| c.folder_id != folder_id);

            info!("Removed sync folder '{}'", folder_id);

            // Stop file system watching for this folder
            if let Some(watcher) = &mut self.watcher {
                if let Err(e) = watcher.unwatch(&config.local_path) {
                    warn!(
                        "Failed to unwatch folder {}: {}",
                        config.local_path.display(),
                        e
                    );
                }
            }

            self.save_config().await?;

            Ok(())
        } else {
            Err(ProtocolError::Plugin(format!(
                "Sync folder not found: {}",
                folder_id
            )))
        }
    }

    /// Get list of configured sync folders
    pub async fn get_folders(&self) -> Vec<SyncFolder> {
        let folders = self.sync_folders.read().await;
        folders.values().cloned().collect()
    }

    /// Compute BLAKE3 hash of a file
    fn compute_file_hash<P: AsRef<std::path::Path>>(path: P) -> Result<String> {
        let mut file = fs::File::open(path).map_err(|e| ProtocolError::Io(e))?;
        let mut hasher = blake3::Hasher::new();
        let mut buffer = [0; 65536]; // 64KB buffer

        loop {
            let count = file.read(&mut buffer).map_err(|e| ProtocolError::Io(e))?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }

        Ok(hasher.finalize().to_hex().to_string())
    }

    /// Generate sync index for a folder
    pub async fn generate_index(&self, folder_id: &str) -> Result<SyncIndex> {
        let (config, _) = {
            let folders = self.sync_folders.read().await;
            if let Some(c) = folders.get(folder_id) {
                (c.clone(), ())
            } else {
                return Err(ProtocolError::Plugin(format!(
                    "Sync folder not found: {}",
                    folder_id
                )));
            }
        };

        Self::generate_index_internal(folder_id, &config).await
    }

    async fn generate_index_internal(folder_id: &str, config: &SyncFolder) -> Result<SyncIndex> {
        info!(
            "Generating sync index for folder '{}' at {}",
            folder_id,
            config.local_path.display()
        );

        let mut files = Vec::new();
        let mut total_size = 0;

        for entry in WalkDir::new(&config.local_path)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            // Skip the root folder itself
            if path == config.local_path {
                continue;
            }

            // Calculate relative path
            let relative_path = match path.strip_prefix(&config.local_path) {
                Ok(p) => p.to_path_buf(),
                Err(_) => continue,
            };

            // Basic ignore logic (TODO: Use robust glob matching)
            let path_str = relative_path.to_string_lossy();
            if config
                .ignore_patterns
                .iter()
                .any(|pattern| path_str.contains(pattern))
            {
                continue;
            }
            if path_str.contains(".git") || path_str.contains(".DS_Store") {
                continue;
            }

            let metadata = entry.metadata().map_err(|e| ProtocolError::Io(e.into()))?;
            let is_dir = metadata.is_dir();
            let size = metadata.len();
            let modified = metadata
                .modified()
                .unwrap_or(SystemTime::now())
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;

            // Unix permissions (if applicable)
            #[cfg(unix)]
            let permissions = {
                use std::os::unix::fs::MetadataExt;
                Some(metadata.mode())
            };
            #[cfg(not(unix))]
            let permissions = None;

            let hash = if is_dir {
                String::new()
            } else {
                Self::compute_file_hash(path)?
            };

            if !is_dir {
                total_size += size;
            }

            files.push(FileMetadata {
                path: relative_path,
                size,
                modified,
                hash,
                is_dir,
                permissions,
            });
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        debug!(
            "Generated index for {}: {} files, {} bytes",
            folder_id,
            files.len(),
            total_size
        );

        let index = SyncIndex {
            folder_id: folder_id.to_string(),
            file_count: files.len(),
            files,
            timestamp,
            total_size,
        };

        Ok(index)
    }

    /// Create a synchronization plan by comparing local and remote indexes
    pub async fn create_sync_plan(
        &self,
        folder_id: &str,
        local_index: &SyncIndex,
        remote_index: &SyncIndex,
    ) -> SyncPlan {
        let mut plan = SyncPlan::default();

        // Check if folder exists
        let exists = {
            let folders = self.sync_folders.read().await;
            folders.contains_key(folder_id)
        };

        let config = if exists {
            let folders = self.sync_folders.read().await;
            folders.get(folder_id).cloned()
        } else {
            return plan;
        };

        let config = match config {
            Some(c) => c,
            None => return plan,
        };

        // Efficient lookups
        let local_map: HashMap<&PathBuf, &FileMetadata> =
            local_index.files.iter().map(|f| (&f.path, f)).collect();
        let remote_map: HashMap<&PathBuf, &FileMetadata> =
            remote_index.files.iter().map(|f| (&f.path, f)).collect();

        // 1. Check local files (Uploads / Conflicts)
        for (path, local_file) in &local_map {
            match remote_map.get(path) {
                Some(remote_file) => {
                    // File exists on both sides
                    if local_file.hash != remote_file.hash {
                        // Content differs, check timestamps
                        if local_file.modified > remote_file.modified {
                            // Local is newer -> Upload
                            plan.actions.push(SyncAction::Upload(path.to_path_buf()));
                            plan.stats.files_to_upload += 1;
                            plan.stats.bytes_to_upload += local_file.size;
                        } else if remote_file.modified > local_file.modified {
                            // Remote is newer -> Download
                            plan.actions.push(SyncAction::Download(path.to_path_buf()));
                            plan.stats.files_to_download += 1;
                            plan.stats.bytes_to_download += remote_file.size;
                        } else {
                            // Timestamps differ but logic unsure, treat as conflict
                            plan.stats.conflicts += 1;
                            plan.actions.push(SyncAction::Conflict(FileConflict {
                                folder_id: folder_id.to_string(),
                                path: path.to_path_buf(),
                                local_metadata: (*local_file).clone(),
                                remote_metadata: (*remote_file).clone(),
                                suggested_strategy: config.conflict_strategy,
                                timestamp: SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis() as i64,
                            }));
                        }
                    }
                }
                None => {
                    // Local file only. Assume ADD (Upload).
                    plan.actions.push(SyncAction::Upload(path.to_path_buf()));
                    plan.stats.files_to_upload += 1;
                    plan.stats.bytes_to_upload += local_file.size;
                }
            }
        }

        // 2. Check remote files (Downloads)
        for (path, remote_file) in &remote_map {
            if !local_map.contains_key(path) {
                // Remote file only. Assume ADD (Download).
                plan.actions.push(SyncAction::Download(path.to_path_buf()));
                plan.stats.files_to_download += 1;
                plan.stats.bytes_to_download += remote_file.size;
            }
        }

        debug!(
            "Created sync plan for {}: +{}up, +{}down, {} conflicts",
            folder_id,
            plan.stats.files_to_upload,
            plan.stats.files_to_download,
            plan.stats.conflicts
        );

        plan
    }

    /// Resolve a file conflict
    pub async fn resolve_conflict(
        &mut self,
        conflict: &FileConflict,
        strategy: ConflictStrategy,
    ) -> Result<()> {
        info!(
            "Resolving conflict for {} using {:?}",
            conflict.path.display(),
            strategy
        );

        let device_id = self.device_id.clone().ok_or_else(|| {
            ProtocolError::Plugin("Plugin not initialized (missing device_id)".to_string())
        })?;

        match strategy {
            ConflictStrategy::LastModifiedWins => {
                // Use most recently modified file
                if conflict.local_metadata.modified > conflict.remote_metadata.modified {
                    debug!("Local file is newer, pushing to remote");
                    self.initiate_upload(
                        device_id,
                        conflict.folder_id.clone(),
                        conflict.path.clone(),
                    )
                    .await?;
                } else {
                    debug!("Remote file is newer, pulling from remote");
                    self.request_download(
                        device_id,
                        conflict.folder_id.clone(),
                        conflict.path.clone(),
                    )
                    .await?;
                }
            }
            ConflictStrategy::KeepBoth => {
                // Rename one file with timestamp
                debug!("Keeping both files");

                // Get local path and rename it
                let config = {
                    let folders = self.sync_folders.read().await;
                    folders.get(&conflict.folder_id).cloned()
                };

                if let Some(config) = config {
                    let local_path = config.local_path.join(&conflict.path);
                    if local_path.exists() {
                        let file_stem = local_path
                            .file_stem()
                            .map(|s| s.to_string_lossy())
                            .unwrap_or_default();
                        let extension = local_path
                            .extension()
                            .map(|s| s.to_string_lossy())
                            .unwrap_or_default();

                        let new_name = if extension.is_empty() {
                            format!("{} (Conflict {})", file_stem, conflict.timestamp)
                        } else {
                            format!(
                                "{} (Conflict {}).{}",
                                file_stem, conflict.timestamp, extension
                            )
                        };

                        let new_path = local_path.with_file_name(new_name);
                        if let Err(e) = tokio::fs::rename(&local_path, &new_path).await {
                            warn!(
                                "Failed to rename conflicting file {}: {}",
                                local_path.display(),
                                e
                            );
                            return Err(ProtocolError::Io(e));
                        }

                        // Pull remote file
                        self.request_download(
                            device_id,
                            conflict.folder_id.clone(),
                            conflict.path.clone(),
                        )
                        .await?;
                    }
                }
            }
            ConflictStrategy::Manual => {
                // Manual resolution implies the user will trigger a specific action later,
                // or picked one of the other strategies in the UI which called this method with that strategy.
                // If 'Manual' is passed here, it typically means "defer to user".
                warn!("Manual resolution strategy requested but requires specific action.");
                return Ok(());
            }
            ConflictStrategy::SizeBased => {
                // Keep larger file
                if conflict.local_metadata.size > conflict.remote_metadata.size {
                    debug!("Local file is larger, pushing to remote");
                    self.initiate_upload(
                        device_id,
                        conflict.folder_id.clone(),
                        conflict.path.clone(),
                    )
                    .await?;
                } else {
                    debug!("Remote file is larger, pulling from remote");
                    self.request_download(
                        device_id,
                        conflict.folder_id.clone(),
                        conflict.path.clone(),
                    )
                    .await?;
                }
            }
        }

        // Remove from pending conflicts
        self.pending_conflicts
            .retain(|c| c.folder_id != conflict.folder_id || c.path != conflict.path);

        Ok(())
    }

    /// Get list of pending conflicts
    pub fn get_pending_conflicts(&self) -> &[FileConflict] {
        &self.pending_conflicts
    }

    /// Get sync folder configuration
    pub async fn get_folder_config(&self, folder_id: &str) -> Option<SyncFolder> {
        self.sync_folders.read().await.get(folder_id).cloned()
    }

    /// Get current sync index for a folder
    pub fn get_sync_index(&self, folder_id: &str) -> Option<&SyncIndex> {
        self.sync_indexes.get(folder_id)
    }
}

impl Default for FileSyncPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSyncPlugin {
    async fn initiate_upload(
        &self,
        device_id: String,
        folder_id: String,
        relative_path: PathBuf,
    ) -> Result<()> {
        let path_str = relative_path.to_string_lossy().to_string();

        let config = {
            let folders = self.sync_folders.read().await;
            folders.get(&folder_id).cloned()
        };

        if let Some(config) = config {
            let local_path = config.local_path.join(&relative_path);

            if local_path.exists() && local_path.starts_with(&config.local_path) {
                // Start PayloadServer
                match PayloadServer::new().await {
                    Ok(server) => {
                        let port = server.port();
                        let size = tokio::fs::metadata(&local_path)
                            .await
                            .map_err(|e| ProtocolError::Io(e))?
                            .len();

                        // Create transfer packet
                        let mut transfer_packet = Packet::new(
                            "cconnect.filesync.transfer",
                            serde_json::json!({
                                "folderId": folder_id,
                                "path": path_str
                            }),
                        )
                        .with_payload_size(size as i64);

                        let mut transfer_info = HashMap::new();
                        transfer_info.insert("port".to_string(), serde_json::json!(port));
                        transfer_packet = transfer_packet.with_payload_transfer_info(transfer_info);

                        // Send packet
                        if let Some(sender) = &self.packet_sender {
                            sender
                                .send((device_id, transfer_packet))
                                .await
                                .map_err(|_| {
                                    ProtocolError::Plugin("Failed to send packet".to_string())
                                })?;

                            // Spawn task to send file
                            tokio::spawn(async move {
                                if let Err(e) = server.send_file(&local_path).await {
                                    warn!("Failed to send file {}: {}", local_path.display(), e);
                                } else {
                                    info!("Successfully sent file {}", local_path.display());
                                }
                            });
                        } else {
                            warn!("No packet sender available");
                        }
                    }
                    Err(e) => warn!("Failed to start payload server: {}", e),
                }
            } else {
                warn!(
                    "Requested file not found or invalid path: {}",
                    local_path.display()
                );
            }
        }
        Ok(())
    }

    async fn request_download(
        &self,
        device_id: String,
        folder_id: String,
        relative_path: PathBuf,
    ) -> Result<()> {
        let path_str = relative_path.to_string_lossy().to_string();
        let packet = Packet::new(
            "cconnect.filesync.request",
            serde_json::json!({
                "folderId": folder_id,
                "path": path_str
            }),
        );

        if let Some(sender) = &self.packet_sender {
            sender
                .send((device_id, packet))
                .await
                .map_err(|_| ProtocolError::Plugin("Failed to send packet".to_string()))?;
        }
        Ok(())
    }
}

#[async_trait]
impl Plugin for FileSyncPlugin {
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
                    "kdeconnect.filesync".to_string(),
                ]
            }
    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![OUTGOING_CAPABILITY.to_string()]
    }

    async fn init(
        &mut self,
        device: &Device,
        packet_sender: Sender<(String, Packet)>,
    ) -> Result<()> {
        info!("Initializing FileSync plugin for device {}", device.name());
        self.device_id = Some(device.id().to_string());
        self.packet_sender = Some(packet_sender);

        // Set up config path
        self.config_path = Some(Self::get_config_path(device.id())?);

        // Load existing configuration
        if let Err(e) = self.load_config().await {
            warn!("Failed to load config: {}", e);
        }

        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Starting FileSync plugin");
        self.enabled = true;

        let (tx, mut rx) = tokio::sync::mpsc::channel(100);
        let tx_clone = tx.clone();

        // Initialize watcher
        // Use blocking_send within the sync closure
        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| match res {
                Ok(event) => {
                    let _ = tx_clone.blocking_send(event);
                }
                Err(e) => warn!("Watch error: {:?}", e),
            },
            Config::default(),
        )
        .map_err(|e| ProtocolError::Plugin(e.to_string()))?;

        // Start watching all configured folders
        let folders_guard = self.sync_folders.read().await;
        for config in folders_guard.values() {
            if config.enabled {
                if let Err(e) = watcher.watch(&config.local_path, RecursiveMode::Recursive) {
                    warn!(
                        "Failed to watch folder {}: {}",
                        config.local_path.display(),
                        e
                    );
                }
            }
        }
        drop(folders_guard); // Release lock

        self.watcher = Some(watcher);

        // Spawn watcher event consumer task
        let sync_folders = self.sync_folders.clone();
        let packet_sender = self.packet_sender.clone();
        let device_id = self.device_id.clone();

        let handle = tokio::spawn(async move {
            info!("FileSync watcher task started");

            while let Some(event) = rx.recv().await {
                debug!("Filesystem event: {:?}", event);

                // Ideally identify which folder this belongs to
                // For now, naive approach: check all folders
                let folders = sync_folders.read().await;

                // Use simple debounce or just handle each event (expensive if many)
                // We will simply find the matching folder and regenerate index

                // Naive: check if event path starts with any config path
                if let Some(path) = event.paths.first() {
                    for (fid, config) in folders.iter() {
                        if path.starts_with(&config.local_path) {
                            // Regenerate index for this folder
                            info!("Changes detected in {}, generating index...", fid);

                            if let Ok(index) = Self::generate_index_internal(fid, config).await {
                                // Send index packet
                                if let Some(sender) = &packet_sender {
                                    if let Some(did) = &device_id {
                                        let packet = Packet::new(
                                            "cconnect.filesync.index",
                                            serde_json::to_value(&index)
                                                .unwrap_or(serde_json::Value::Null),
                                        );
                                        let _ = sender.send((did.clone(), packet)).await;
                                    }
                                }
                            }
                            break; // Found the folder
                        }
                    }
                }
            }
            info!("FileSync watcher task stopped");
        });

        self.watcher_handle = Some(handle);

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Stopping FileSync plugin");
        self.enabled = false;
        self.watcher = None;

        // TODO: Cancel active transfers
        // TODO: Save sync state to database

        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("FileSync plugin is disabled, ignoring packet");
            return Ok(());
        }

        debug!("Handling packet type: {}", packet.packet_type);

        if packet.is_type("cconnect.filesync.config") {
            // Receive sync folder configuration
            let folder_id: String = packet
                .body
                .get("folderId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ProtocolError::InvalidPacket("Missing folderId".to_string()))?
                .to_string();

            let config: SyncFolder =
                serde_json::from_value(packet.body.get("config").cloned().ok_or_else(
                    || ProtocolError::InvalidPacket("Missing config".to_string()),
                )?)
                .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

            self.configure_folder(folder_id, config.local_path, config.conflict_strategy)
                .await?;

            info!("Received sync folder configuration");
        } else if packet.is_type("cconnect.filesync.index") {
            // Receive remote sync index
            let index: SyncIndex = serde_json::from_value(packet.body.clone())
                .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

            let folder_id = index.folder_id.clone();

            // Compare with local index
            if let Ok(local_index) = self.generate_index(&folder_id).await {
                let plan = self
                    .create_sync_plan(&folder_id, &local_index, &index)
                    .await;

                if plan.stats.files_to_upload > 0
                    || plan.stats.files_to_download > 0
                    || plan.stats.conflicts > 0
                {
                    info!(
                        "Sync Plan: {} uploads, {} downloads, {} conflicts",
                        plan.stats.files_to_upload,
                        plan.stats.files_to_download,
                        plan.stats.conflicts
                    );
                }

                // Handle conflicts
                for action in &plan.actions {
                    if let SyncAction::Conflict(conflict) = action {
                        let strategy = conflict.suggested_strategy;
                        if strategy == ConflictStrategy::Manual {
                            self.pending_conflicts.push(conflict.clone());
                        } else {
                            if let Err(e) = self.resolve_conflict(conflict, strategy).await {
                                warn!(
                                    "Failed to auto-resolve conflict for {}: {}",
                                    conflict.path.display(),
                                    e
                                );
                                self.pending_conflicts.push(conflict.clone());
                            }
                        }
                    }
                }

                // Store remote index
                self.sync_indexes.insert(folder_id.clone(), index);

                // Execute transfers (Uploads / Downloads)
                let device_id = device.id().to_string();

                for action in plan.actions {
                    match action {
                        SyncAction::Upload(path) => {
                            if let Err(e) = self
                                .initiate_upload(device_id.clone(), folder_id.clone(), path)
                                .await
                            {
                                warn!("Failed to initiate upload: {}", e);
                            }
                        }
                        SyncAction::Download(path) => {
                            if let Err(e) = self
                                .request_download(device_id.clone(), folder_id.clone(), path)
                                .await
                            {
                                warn!("Failed to request download: {}", e);
                            }
                        }
                        SyncAction::DeleteLocal(path) => {
                            if let Some(config) = self.sync_folders.read().await.get(&folder_id)
                            {
                                let local_path = config.local_path.join(&path);
                                if local_path.exists() {
                                    if let Err(e) = tokio::fs::remove_file(&local_path).await {
                                        warn!(
                                            "Failed to delete local file {}: {}",
                                            local_path.display(),
                                            e
                                        );
                                    } else {
                                        info!(
                                            "Deleted local file per sync plan: {}",
                                            local_path.display()
                                        );
                                    }
                                }
                            }
                        }
                        _ => {} // Conflicts and Remote Deletes handled differently or already processed
                    }
                }
            }

            info!("Processed sync index");
        } else if packet.is_type("cconnect.filesync.request") {
            // Handle file transfer request (Remote wants to download from us)
            let folder_id: String = packet
                .body
                .get("folderId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ProtocolError::InvalidPacket("Missing folderId".to_string()))?
                .to_string();

            let path_str: String = packet
                .body
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ProtocolError::InvalidPacket("Missing path".to_string()))?
                .to_string();

            info!(
                "Received request for file {} in folder {}",
                path_str, folder_id
            );

            let device_id = device.id().to_string();
            if let Err(e) = self
                .initiate_upload(device_id, folder_id, PathBuf::from(path_str))
                .await
            {
                warn!("Failed to process file request: {}", e);
            }
        } else if packet.is_type("cconnect.filesync.transfer") {
            // Receive file data transfer (Remote is sending to us)
            let folder_id: String = packet
                .body
                .get("folderId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ProtocolError::InvalidPacket("Missing folderId".to_string()))?
                .to_string();

            let path_str: String = packet
                .body
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ProtocolError::InvalidPacket("Missing path".to_string()))?
                .to_string();

            let path = PathBuf::from(&path_str);

            debug!(
                "Received file transfer offer for {} in {}",
                path.display(),
                folder_id
            );

            let config = {
                let folders = self.sync_folders.read().await;
                folders.get(&folder_id).cloned()
            };

            if let Some(config) = config {
                let target_path = config.local_path.join(&path);

                if !target_path.starts_with(&config.local_path) {
                    warn!(
                        "Security warning: Attempted path traversal to {}",
                        target_path.display()
                    );
                    return Ok(());
                }

                // Check capabilities and device info
                if let Some(transfer_info) = &packet.payload_transfer_info {
                    if let Some(port) = transfer_info.get("port").and_then(|v| v.as_u64()) {
                        let port = port as u16;
                        if let Some(host) = &device.host {
                            let host = host.clone();
                            let size = packet.payload_size.unwrap_or(0);

                            // Ensure parent directory exists
                            if let Some(parent) = target_path.parent() {
                                tokio::fs::create_dir_all(parent).await?;
                            }

                            debug!(
                                "Starting download from {}:{} to {}",
                                host,
                                port,
                                target_path.display()
                            );

                            // Spawn download task
                            tokio::spawn(async move {
                                match PayloadClient::new(&host, port).await {
                                    Ok(client) => {
                                        if let Err(e) =
                                            client.receive_file(&target_path, size as u64).await
                                        {
                                            warn!(
                                                "Failed to receive file {}: {}",
                                                target_path.display(),
                                                e
                                            );
                                        } else {
                                            info!(
                                                "Successfully received file {}",
                                                target_path.display()
                                            );
                                            // TODO: Update sync index locally
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to connect to payload server: {}", e)
                                    }
                                }
                            });
                        } else {
                            warn!("Cannot download: Unknown device host");
                        }
                    } else {
                        warn!("No port in transfer info");
                    }
                }
            }
        } else if packet.is_type("cconnect.filesync.delete") {
            // Synchronize file deletion
            let folder_id: String = packet
                .body
                .get("folderId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ProtocolError::InvalidPacket("Missing folderId".to_string()))?
                .to_string();

            let path_str: String = packet
                .body
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ProtocolError::InvalidPacket("Missing path".to_string()))?
                .to_string();

            let file_path = PathBuf::from(&path_str);

            if let Some(config) = self.sync_folders.read().await.get(&folder_id) {
                let local_path = config.local_path.join(&file_path);
                if local_path.exists() && local_path.starts_with(&config.local_path) {
                    if let Err(e) = tokio::fs::remove_file(&local_path).await {
                        warn!("Failed to delete file {}: {}", local_path.display(), e);
                    } else {
                        info!("Deleted file handled: {}", local_path.display());
                    }
                }
            }
        } else if packet.is_type("cconnect.filesync.conflict") {
            // Receive conflict notification
            let conflict: FileConflict = serde_json::from_value(packet.body.clone())
                .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

            self.pending_conflicts.push(conflict.clone());

            warn!(
                "Conflict detected for {} in folder '{}'",
                conflict.path.display(),
                conflict.folder_id
            );
        }

        Ok(())
    }
}

/// File Sync plugin factory
pub struct FileSyncPluginFactory;

impl PluginFactory for FileSyncPluginFactory {
    fn create(&self) -> Box<dyn Plugin> {
        Box::new(FileSyncPlugin::new())
    }

    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            INCOMING_CAPABILITY.to_string(),
            "kdeconnect.filesync".to_string(),
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
        let plugin = FileSyncPlugin::new();
        assert_eq!(plugin.name(), PLUGIN_NAME);
        assert!(!plugin.enabled);
    }

    #[tokio::test]
    async fn test_configure_folder() {
        let mut plugin = FileSyncPlugin::new();
        plugin.enabled = true;

        let config = SyncFolder {
            folder_id: "test_folder".to_string(),
            local_path: std::env::temp_dir(),
            remote_path: PathBuf::from("/remote/path"),
            enabled: true,
            bidirectional: true,
            ignore_patterns: vec!["*.tmp".to_string()],
            conflict_strategy: ConflictStrategy::LastModifiedWins,
            versioning: true,
            version_keep: 5,
            scan_interval_secs: 60,
            bandwidth_limit_kbps: 0,
        };

        assert!(plugin
            .configure_folder(
                "test_folder".to_string(),
                config.local_path,
                config.conflict_strategy
            )
            .await
            .is_ok());
        assert!(plugin.get_folder_config("test_folder").await.is_some());
    }

    #[tokio::test]
    async fn test_remove_folder() {
        let mut plugin = FileSyncPlugin::new();
        plugin.enabled = true;

        let config = SyncFolder {
            folder_id: "test_folder".to_string(),
            local_path: std::env::temp_dir(),
            remote_path: PathBuf::from("/remote/path"),
            enabled: true,
            bidirectional: true,
            ignore_patterns: Vec::new(),
            conflict_strategy: ConflictStrategy::default(),
            versioning: true,
            version_keep: 5,
            scan_interval_secs: 60,
            bandwidth_limit_kbps: 0,
        };

        plugin
            .configure_folder(
                "test_folder".to_string(),
                config.local_path,
                config.conflict_strategy,
            )
            .await
            .unwrap();
        assert!(plugin.remove_folder("test_folder").await.is_ok());
        assert!(plugin.get_folder_config("test_folder").await.is_none());
    }

    #[tokio::test]
    async fn test_conflict_strategies() {
        assert_eq!(
            ConflictStrategy::LastModifiedWins.as_str(),
            "last_modified_wins"
        );
        assert_eq!(ConflictStrategy::KeepBoth.as_str(), "keep_both");
        assert_eq!(ConflictStrategy::Manual.as_str(), "manual");
        assert_eq!(ConflictStrategy::SizeBased.as_str(), "size_based");
    }

    #[tokio::test]
    async fn test_sync_folder_validation() {
        let valid_config = SyncFolder {
            folder_id: "test_folder".to_string(),
            local_path: std::env::temp_dir(),
            remote_path: PathBuf::from("/remote/path"),
            enabled: true,
            bidirectional: true,
            ignore_patterns: Vec::new(),
            conflict_strategy: ConflictStrategy::default(),
            versioning: true,
            version_keep: 5,
            scan_interval_secs: 60,
            bandwidth_limit_kbps: 0,
        };

        assert!(valid_config.validate().is_ok());

        let invalid_config = SyncFolder {
            folder_id: "test_folder".to_string(),
            local_path: PathBuf::from("/nonexistent/path"),
            remote_path: PathBuf::from("/remote/path"),
            enabled: true,
            bidirectional: true,
            ignore_patterns: Vec::new(),
            conflict_strategy: ConflictStrategy::default(),
            versioning: true,
            version_keep: 5,
            scan_interval_secs: 60,
            bandwidth_limit_kbps: 0,
        };

        assert!(invalid_config.validate().is_err());
    }

    #[tokio::test]
    async fn test_plugin_initialization() {
        let device = create_test_device();
        let factory = FileSyncPluginFactory;
        let mut plugin = factory.create();

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        assert!(plugin.init(&device, tx).await.is_ok());
        assert!(plugin.start().await.is_ok());
        assert!(plugin.stop().await.is_ok());
    }

    #[tokio::test]
    async fn test_handle_config_packet() {
        let mut device = create_test_device();
        let factory = FileSyncPluginFactory;
        let mut plugin = factory.create();

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        plugin.init(&device, tx).await.unwrap();
        plugin.start().await.unwrap();

        let config = SyncFolder {
            folder_id: "test_folder".to_string(),
            local_path: std::env::temp_dir(),
            remote_path: PathBuf::from("/remote/path"),
            enabled: true,
            bidirectional: true,
            ignore_patterns: Vec::new(),
            conflict_strategy: ConflictStrategy::default(),
            versioning: true,
            version_keep: 5,
            scan_interval_secs: 60,
            bandwidth_limit_kbps: 0,
        };

        let mut body = serde_json::Map::new();
        body.insert(
            "folderId".to_string(),
            serde_json::Value::String("test".to_string()),
        );
        body.insert("config".to_string(), serde_json::to_value(&config).unwrap());

        let packet = Packet::new("cconnect.filesync.config", serde_json::Value::Object(body));

        assert!(plugin.handle_packet(&packet, &mut device).await.is_ok());
    }

    #[tokio::test]
    async fn test_pending_conflicts() {
        let plugin = FileSyncPlugin::new();
        assert_eq!(plugin.get_pending_conflicts().len(), 0);
    }
}
