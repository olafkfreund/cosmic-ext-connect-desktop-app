use cosmic_connect_protocol::Device;

/// Device state with battery information
#[derive(Debug, Clone)]
pub struct DeviceState {
    pub device: Device,
    pub battery_level: Option<u8>,
    pub is_charging: bool,
}

/// Application notification for the UI
#[derive(Debug, Clone)]
pub struct AppNotification {
    pub message: String,
    pub kind: super::NotificationType,
    pub action: Option<(String, Box<crate::messages::Message>)>,
}

/// Focus targets for keyboard navigation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusTarget {
    /// Search input field
    Search,
    /// Device at index in the filtered list
    Device(usize),
    /// Quick action button for device (device_index, action_index)
    DeviceAction(usize, usize),
    /// MPRIS player control (player_name, control: "prev", "play", "next")
    MprisControl(String, String),
    /// View mode tab (Devices, History, Transfers) - reserved for future use
    #[allow(dead_code)]
    ViewTab(ViewMode),
    /// Refresh button
    Refresh,
    /// None - nothing focused
    None,
}

/// View mode for the applet UI
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewMode {
    Devices,
    History,
    TransferQueue,
    DeviceDetails(String),
}

/// History event for tracking device interactions
#[derive(Debug, Clone)]
pub struct HistoryEvent {
    #[allow(dead_code)]
    pub timestamp: std::time::SystemTime,
    pub event_type: String,
    pub device_name: String,
    pub details: String,
}
