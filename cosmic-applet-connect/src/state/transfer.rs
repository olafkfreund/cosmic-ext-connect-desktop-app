/// File transfer state tracking
#[derive(Debug, Clone)]
pub struct TransferState {
    #[allow(dead_code)]
    pub device_id: String,
    pub filename: String,
    pub current: u64,
    pub total: u64,
    pub direction: String,
    pub started_at: std::time::Instant,
    pub last_update: std::time::Instant,
    pub last_bytes: u64,
}

/// A recently received file for history tracking
#[derive(Debug, Clone)]
pub struct ReceivedFile {
    pub filename: String,
    #[allow(dead_code)]
    pub device_id: String,
    pub device_name: String,
    pub timestamp: std::time::Instant,
    pub success: bool,
}

/// Maximum number of received files to track in history (memory limit)
pub const MAX_RECEIVED_FILES_HISTORY: usize = 50;

/// Number of recent files to display in the UI
pub const MAX_DISPLAYED_HISTORY_ITEMS: usize = 10;
