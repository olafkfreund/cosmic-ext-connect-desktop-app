/// Summary of an SMS conversation thread for UI display
#[derive(Debug, Clone)]
pub struct ConversationSummary {
    /// Thread ID
    pub thread_id: i64,
    /// Phone number or address
    pub address: String,
    /// Preview of the latest message
    pub preview: String,
    /// Timestamp of the latest message (ms since epoch)
    pub timestamp: i64,
    /// Number of unread messages
    pub unread_count: usize,
}

/// A single SMS message for display in conversation detail
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SmsMessageDisplay {
    /// Message ID
    pub id: i64,
    /// Message body
    pub body: String,
    /// Timestamp (ms since epoch)
    pub timestamp: i64,
    /// Whether this message was sent by us (true) or received (false)
    pub is_sent: bool,
    /// Read status
    pub is_read: bool,
}
