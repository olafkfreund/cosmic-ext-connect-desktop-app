//! Settings Data Structures
//!
//! This module defines data structures for managing daemon configuration
//! from the desktop application. These structures mirror the daemon's
//! configuration format and provide helper functions for the UI.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Complete daemon configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub device: DeviceConfig,
    pub transport: TransportConfig,
    pub plugins: PluginConfig,
    pub discovery: DiscoveryConfig,
    pub network: NetworkConfig,
}

impl DaemonConfig {
    /// Parse daemon configuration from JSON string
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Convert to JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// Device configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// Device name (user-visible)
    pub name: String,
    /// Device type: "desktop", "laptop", "phone", "tablet", "tv"
    pub device_type: String,
    /// Unique device ID (generated, read-only)
    pub device_id: Option<String>,
}

/// Transport configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    /// Enable TCP/TLS transport
    pub enable_tcp: bool,
    /// Enable Bluetooth transport
    pub enable_bluetooth: bool,
    /// Transport preference
    pub preference: TransportPreference,
    /// TCP connection timeout (seconds)
    pub tcp_timeout_secs: u64,
    /// Bluetooth connection timeout (seconds)
    pub bluetooth_timeout_secs: u64,
    /// Automatically fall back to alternative transport if primary fails
    pub auto_fallback: bool,
    /// Filter Bluetooth devices by address (empty = all devices)
    pub bluetooth_device_filter: Vec<String>,
}

/// Transport preference options
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportPreference {
    /// Prefer TCP, use Bluetooth if TCP unavailable
    PreferTcp,
    /// Prefer Bluetooth, use TCP if Bluetooth unavailable
    PreferBluetooth,
    /// Try TCP first, then Bluetooth
    TcpFirst,
    /// Try Bluetooth first, then TCP
    BluetoothFirst,
    /// Only use TCP
    OnlyTcp,
    /// Only use Bluetooth
    OnlyBluetooth,
}

impl TransportPreference {
    /// Get human-readable display name
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::PreferTcp => "Prefer TCP",
            Self::PreferBluetooth => "Prefer Bluetooth",
            Self::TcpFirst => "TCP First",
            Self::BluetoothFirst => "Bluetooth First",
            Self::OnlyTcp => "TCP Only",
            Self::OnlyBluetooth => "Bluetooth Only",
        }
    }

    /// Get preference as string for DBus
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PreferTcp => "prefer_tcp",
            Self::PreferBluetooth => "prefer_bluetooth",
            Self::TcpFirst => "tcp_first",
            Self::BluetoothFirst => "bluetooth_first",
            Self::OnlyTcp => "only_tcp",
            Self::OnlyBluetooth => "only_bluetooth",
        }
    }

    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "prefer_tcp" => Some(Self::PreferTcp),
            "prefer_bluetooth" => Some(Self::PreferBluetooth),
            "tcp_first" => Some(Self::TcpFirst),
            "bluetooth_first" => Some(Self::BluetoothFirst),
            "only_tcp" => Some(Self::OnlyTcp),
            "only_bluetooth" => Some(Self::OnlyBluetooth),
            _ => None,
        }
    }

    /// Get all available preferences
    pub fn all() -> Vec<Self> {
        vec![
            Self::PreferTcp,
            Self::PreferBluetooth,
            Self::TcpFirst,
            Self::BluetoothFirst,
            Self::OnlyTcp,
            Self::OnlyBluetooth,
        ]
    }
}

/// Plugin configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub enable_ping: bool,
    pub enable_battery: bool,
    pub enable_notification: bool,
    pub enable_share: bool,
    pub enable_clipboard: bool,
    pub enable_mpris: bool,
    pub enable_runcommand: bool,
    pub enable_remoteinput: bool,
    pub enable_findmyphone: bool,
    pub enable_telephony: bool,
    pub enable_presenter: bool,
    pub enable_contacts: bool,
}

impl PluginConfig {
    /// Get plugin enabled status by name
    pub fn get(&self, plugin: &str) -> Option<bool> {
        match plugin {
            "ping" => Some(self.enable_ping),
            "battery" => Some(self.enable_battery),
            "notification" => Some(self.enable_notification),
            "share" => Some(self.enable_share),
            "clipboard" => Some(self.enable_clipboard),
            "mpris" => Some(self.enable_mpris),
            "runcommand" => Some(self.enable_runcommand),
            "remoteinput" => Some(self.enable_remoteinput),
            "findmyphone" => Some(self.enable_findmyphone),
            "telephony" => Some(self.enable_telephony),
            "presenter" => Some(self.enable_presenter),
            "contacts" => Some(self.enable_contacts),
            _ => None,
        }
    }

    /// Set plugin enabled status by name
    pub fn set(&mut self, plugin: &str, enabled: bool) -> bool {
        match plugin {
            "ping" => { self.enable_ping = enabled; true }
            "battery" => { self.enable_battery = enabled; true }
            "notification" => { self.enable_notification = enabled; true }
            "share" => { self.enable_share = enabled; true }
            "clipboard" => { self.enable_clipboard = enabled; true }
            "mpris" => { self.enable_mpris = enabled; true }
            "runcommand" => { self.enable_runcommand = enabled; true }
            "remoteinput" => { self.enable_remoteinput = enabled; true }
            "findmyphone" => { self.enable_findmyphone = enabled; true }
            "telephony" => { self.enable_telephony = enabled; true }
            "presenter" => { self.enable_presenter = enabled; true }
            "contacts" => { self.enable_contacts = enabled; true }
            _ => false,
        }
    }

    /// Convert to HashMap
    pub fn to_map(&self) -> HashMap<String, bool> {
        let mut map = HashMap::new();
        map.insert("ping".to_string(), self.enable_ping);
        map.insert("battery".to_string(), self.enable_battery);
        map.insert("notification".to_string(), self.enable_notification);
        map.insert("share".to_string(), self.enable_share);
        map.insert("clipboard".to_string(), self.enable_clipboard);
        map.insert("mpris".to_string(), self.enable_mpris);
        map.insert("runcommand".to_string(), self.enable_runcommand);
        map.insert("remoteinput".to_string(), self.enable_remoteinput);
        map.insert("findmyphone".to_string(), self.enable_findmyphone);
        map.insert("telephony".to_string(), self.enable_telephony);
        map.insert("presenter".to_string(), self.enable_presenter);
        map.insert("contacts".to_string(), self.enable_contacts);
        map
    }
}

/// Discovery configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// Enable TCP/UDP discovery
    pub enable_tcp_discovery: bool,
    /// Enable Bluetooth discovery
    pub enable_bluetooth_discovery: bool,
    /// UDP broadcast interval (seconds)
    pub broadcast_interval_secs: u64,
    /// Bluetooth scan interval (seconds)
    pub scan_interval_secs: u64,
    /// Device timeout (seconds)
    pub device_timeout_secs: u64,
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Discovery port (default: 1716)
    pub discovery_port: u16,
    /// Transfer port range start
    pub transfer_port_range_start: u16,
    /// Transfer port range end
    pub transfer_port_range_end: u16,
}

/// Get human-readable description for a plugin
pub fn plugin_description(plugin: &str) -> &'static str {
    match plugin {
        "ping" => "Basic connectivity testing",
        "battery" => "Share battery status",
        "share" => "Send and receive files",
        "notification" => "Sync notifications",
        "clipboard" => "Share clipboard content",
        "mpris" => "Media player control",
        "runcommand" => "Execute commands remotely",
        "remoteinput" => "Use device as remote input",
        "findmyphone" => "Make device ring",
        "telephony" => "SMS and call notifications",
        "presenter" => "Presentation remote",
        "contacts" => "Contact synchronization",
        _ => "Unknown plugin",
    }
}

/// Get category for a plugin (for UI grouping)
pub fn plugin_category(plugin: &str) -> &'static str {
    match plugin {
        "ping" | "battery" | "findmyphone" => "Core",
        "mpris" | "notification" => "Media & Notifications",
        "remoteinput" | "presenter" | "runcommand" => "Input & Control",
        "share" => "File Transfer",
        "clipboard" => "Productivity",
        "telephony" | "contacts" => "Mobile (Experimental)",
        _ => "Other",
    }
}

/// Get all plugin names in a sensible order
pub fn all_plugins() -> Vec<&'static str> {
    vec![
        // Core
        "ping",
        "battery",
        "findmyphone",
        // Media & Notifications
        "mpris",
        "notification",
        // Input & Control
        "remoteinput",
        "presenter",
        "runcommand",
        // File Transfer
        "share",
        // Productivity
        "clipboard",
        // Mobile
        "telephony",
        "contacts",
    ]
}

/// Get all plugins grouped by category
pub fn plugins_by_category() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("Core", vec!["ping", "battery", "findmyphone"]),
        ("Media & Notifications", vec!["mpris", "notification"]),
        ("Input & Control", vec!["remoteinput", "presenter", "runcommand"]),
        ("File Transfer", vec!["share"]),
        ("Productivity", vec!["clipboard"]),
        ("Mobile (Experimental)", vec!["telephony", "contacts"]),
    ]
}

/// Get device type display name
pub fn device_type_name(device_type: &str) -> &'static str {
    match device_type {
        "desktop" => "Desktop",
        "laptop" => "Laptop",
        "phone" => "Phone",
        "tablet" => "Tablet",
        "tv" => "TV",
        _ => "Unknown",
    }
}

/// Get all available device types
pub fn all_device_types() -> Vec<(&'static str, &'static str)> {
    vec![
        ("desktop", "Desktop"),
        ("laptop", "Laptop"),
        ("phone", "Phone"),
        ("tablet", "Tablet"),
        ("tv", "TV"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_preference_conversion() {
        let pref = TransportPreference::PreferTcp;
        assert_eq!(pref.as_str(), "prefer_tcp");
        assert_eq!(pref.display_name(), "Prefer TCP");

        let parsed = TransportPreference::from_str("prefer_tcp");
        assert!(parsed.is_some());
    }

    #[test]
    fn test_plugin_config_get_set() {
        let mut config = PluginConfig {
            enable_ping: true,
            enable_battery: false,
            enable_notification: true,
            enable_share: true,
            enable_clipboard: false,
            enable_mpris: true,
            enable_runcommand: false,
            enable_remoteinput: false,
            enable_findmyphone: true,
            enable_telephony: false,
            enable_presenter: false,
            enable_contacts: false,
        };

        assert_eq!(config.get("ping"), Some(true));
        assert_eq!(config.get("battery"), Some(false));

        assert!(config.set("battery", true));
        assert_eq!(config.get("battery"), Some(true));

        assert!(!config.set("unknown", true));
    }

    #[test]
    fn test_plugin_helpers() {
        assert_eq!(plugin_description("ping"), "Basic connectivity testing");
        assert_eq!(plugin_category("ping"), "Core");
        assert_eq!(plugin_category("mpris"), "Media & Notifications");

        let plugins = all_plugins();
        assert_eq!(plugins.len(), 12);
        assert_eq!(plugins[0], "ping");

        let grouped = plugins_by_category();
        assert_eq!(grouped.len(), 6);
        assert_eq!(grouped[0].0, "Core");
    }

    #[test]
    fn test_device_type_helpers() {
        assert_eq!(device_type_name("desktop"), "Desktop");
        assert_eq!(device_type_name("laptop"), "Laptop");

        let types = all_device_types();
        assert_eq!(types.len(), 5);
        assert_eq!(types[0], ("desktop", "Desktop"));
    }
}
