mod camera;
mod device;
mod screen_share;
mod system;
mod transfer;

pub use camera::CameraStats;
pub use device::{AppNotification, DeviceState, FocusTarget, HistoryEvent, ViewMode};
pub use screen_share::ActiveScreenShare;
pub use system::SystemInfo;
pub use transfer::{ReceivedFile, TransferState, MAX_DISPLAYED_HISTORY_ITEMS, MAX_RECEIVED_FILES_HISTORY};

// Re-export NotificationType from messages module for device module
pub use crate::messages::NotificationType;
