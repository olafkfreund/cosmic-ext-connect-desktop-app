//! UPower DBus Backend for Power State Detection
//!
//! Provides real-time battery and power state information via UPower DBus interface.
//!
//! ## UPower DBus Interface
//!
//! - Service: `org.freedesktop.UPower`
//! - Object: `/org/freedesktop/UPower`
//! - Interface: `org.freedesktop.UPower`
//!
//! ## Battery Device Properties
//!
//! - Percentage: Battery charge level (0-100)
//! - State: Charging, Discharging, Full, Empty, etc.
//! - TimeToEmpty: Seconds until empty (when discharging)
//! - TimeToFull: Seconds until full (when charging)
//! - IsPresent: Whether battery is present

use tracing::{debug, info, warn};
use zbus::zvariant::OwnedValue;
use zbus::Connection;

/// Battery charging state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryState {
    /// Battery state is unknown
    Unknown,
    /// Battery is charging
    Charging,
    /// Battery is discharging
    Discharging,
    /// Battery is empty
    Empty,
    /// Battery is fully charged
    FullyCharged,
    /// Battery is pending charge
    PendingCharge,
    /// Battery is pending discharge
    PendingDischarge,
}

impl From<u32> for BatteryState {
    fn from(value: u32) -> Self {
        match value {
            1 => BatteryState::Charging,
            2 => BatteryState::Discharging,
            3 => BatteryState::Empty,
            4 => BatteryState::FullyCharged,
            5 => BatteryState::PendingCharge,
            6 => BatteryState::PendingDischarge,
            _ => BatteryState::Unknown,
        }
    }
}

impl BatteryState {
    /// Check if battery is charging
    pub fn is_charging(&self) -> bool {
        matches!(self, BatteryState::Charging | BatteryState::PendingCharge)
    }

    /// Convert to string for protocol
    pub fn as_str(&self) -> &'static str {
        match self {
            BatteryState::Unknown => "unknown",
            BatteryState::Charging => "charging",
            BatteryState::Discharging => "discharging",
            BatteryState::Empty => "empty",
            BatteryState::FullyCharged => "full",
            BatteryState::PendingCharge => "pending_charge",
            BatteryState::PendingDischarge => "pending_discharge",
        }
    }
}

/// Power status information
#[derive(Debug, Clone)]
pub struct PowerStatus {
    /// Whether the system is running on battery
    pub on_battery: bool,
    /// Whether a battery is present
    pub battery_present: bool,
    /// Battery charge percentage (0-100)
    pub battery_percentage: Option<f64>,
    /// Battery charging state
    pub battery_state: BatteryState,
    /// Time to empty in seconds (when discharging)
    pub time_to_empty: Option<i64>,
    /// Time to full in seconds (when charging)
    pub time_to_full: Option<i64>,
    /// Whether lid is closed (for laptops)
    pub lid_is_closed: bool,
    /// Whether lid is present
    pub lid_is_present: bool,
}

impl Default for PowerStatus {
    fn default() -> Self {
        Self {
            on_battery: false,
            battery_present: false,
            battery_percentage: None,
            battery_state: BatteryState::Unknown,
            time_to_empty: None,
            time_to_full: None,
            lid_is_closed: false,
            lid_is_present: false,
        }
    }
}

/// UPower DBus backend for power state detection
pub struct UPowerBackend {
    /// DBus connection
    connection: Option<Connection>,
}

impl UPowerBackend {
    /// Create a new UPower backend
    pub fn new() -> Self {
        Self { connection: None }
    }

    /// Connect to the system DBus
    pub async fn connect(&mut self) -> Result<(), String> {
        if self.connection.is_some() {
            return Ok(());
        }

        let conn = Connection::system()
            .await
            .map_err(|e| format!("Failed to connect to system bus: {}", e))?;

        self.connection = Some(conn);
        info!("Connected to UPower via DBus");
        Ok(())
    }

    /// Get the current power status
    pub async fn get_power_status(&mut self) -> Result<PowerStatus, String> {
        // Ensure we're connected
        if self.connection.is_none() {
            self.connect().await?;
        }

        let conn = self.connection.as_ref().ok_or("Not connected")?;
        let mut status = PowerStatus::default();

        // Get UPower properties
        if let Err(e) = self.query_upower_properties(conn, &mut status).await {
            warn!("Failed to query UPower properties: {}", e);
        }

        // Get battery device properties
        if let Err(e) = self.query_battery_properties(conn, &mut status).await {
            debug!("Failed to query battery properties: {}", e);
            // Not an error - system might not have a battery
        }

        Ok(status)
    }

    /// Query main UPower properties using direct DBus calls
    async fn query_upower_properties(
        &self,
        conn: &Connection,
        status: &mut PowerStatus,
    ) -> Result<(), String> {
        // Get OnBattery property
        if let Ok(value) = self
            .get_property_bool(conn, "/org/freedesktop/UPower", "org.freedesktop.UPower", "OnBattery")
            .await
        {
            status.on_battery = value;
            debug!("OnBattery: {}", value);
        }

        // Get LidIsClosed property
        if let Ok(value) = self
            .get_property_bool(conn, "/org/freedesktop/UPower", "org.freedesktop.UPower", "LidIsClosed")
            .await
        {
            status.lid_is_closed = value;
            debug!("LidIsClosed: {}", value);
        }

        // Get LidIsPresent property
        if let Ok(value) = self
            .get_property_bool(conn, "/org/freedesktop/UPower", "org.freedesktop.UPower", "LidIsPresent")
            .await
        {
            status.lid_is_present = value;
            debug!("LidIsPresent: {}", value);
        }

        Ok(())
    }

    /// Query battery device properties
    async fn query_battery_properties(
        &self,
        conn: &Connection,
        status: &mut PowerStatus,
    ) -> Result<(), String> {
        // First, find the battery device
        let battery_path = self.find_battery_device(conn).await?;

        // Get IsPresent
        if let Ok(value) = self
            .get_property_bool(conn, &battery_path, "org.freedesktop.UPower.Device", "IsPresent")
            .await
        {
            status.battery_present = value;
            debug!("Battery present: {}", value);
        }

        // Get Percentage
        if let Ok(value) = self
            .get_property_f64(conn, &battery_path, "org.freedesktop.UPower.Device", "Percentage")
            .await
        {
            status.battery_percentage = Some(value);
            debug!("Battery percentage: {}%", value);
        }

        // Get State
        if let Ok(value) = self
            .get_property_u32(conn, &battery_path, "org.freedesktop.UPower.Device", "State")
            .await
        {
            status.battery_state = BatteryState::from(value);
            debug!("Battery state: {:?}", status.battery_state);
        }

        // Get TimeToEmpty
        if let Ok(value) = self
            .get_property_i64(conn, &battery_path, "org.freedesktop.UPower.Device", "TimeToEmpty")
            .await
        {
            if value > 0 {
                status.time_to_empty = Some(value);
                debug!("Time to empty: {}s", value);
            }
        }

        // Get TimeToFull
        if let Ok(value) = self
            .get_property_i64(conn, &battery_path, "org.freedesktop.UPower.Device", "TimeToFull")
            .await
        {
            if value > 0 {
                status.time_to_full = Some(value);
                debug!("Time to full: {}s", value);
            }
        }

        Ok(())
    }

    /// Get a DBus property as OwnedValue
    async fn get_property_raw(
        &self,
        conn: &Connection,
        path: &str,
        interface: &str,
        property: &str,
    ) -> Result<OwnedValue, String> {
        let msg = conn
            .call_method(
                Some("org.freedesktop.UPower"),
                path,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(interface, property),
            )
            .await
            .map_err(|e| format!("Failed to get {}: {}", property, e))?;

        msg.body()
            .deserialize()
            .map_err(|e| format!("Failed to parse {}: {}", property, e))
    }

    /// Get a boolean property via DBus
    async fn get_property_bool(
        &self,
        conn: &Connection,
        path: &str,
        interface: &str,
        property: &str,
    ) -> Result<bool, String> {
        let variant = self.get_property_raw(conn, path, interface, property).await?;
        variant
            .downcast_ref::<bool>()
            .map_err(|e| format!("Property {} is not a bool: {}", property, e))
    }

    /// Get a f64 property via DBus
    async fn get_property_f64(
        &self,
        conn: &Connection,
        path: &str,
        interface: &str,
        property: &str,
    ) -> Result<f64, String> {
        let variant = self.get_property_raw(conn, path, interface, property).await?;
        variant
            .downcast_ref::<f64>()
            .map_err(|e| format!("Property {} is not a f64: {}", property, e))
    }

    /// Get a u32 property via DBus
    async fn get_property_u32(
        &self,
        conn: &Connection,
        path: &str,
        interface: &str,
        property: &str,
    ) -> Result<u32, String> {
        let variant = self.get_property_raw(conn, path, interface, property).await?;
        variant
            .downcast_ref::<u32>()
            .map_err(|e| format!("Property {} is not a u32: {}", property, e))
    }

    /// Get an i64 property via DBus
    async fn get_property_i64(
        &self,
        conn: &Connection,
        path: &str,
        interface: &str,
        property: &str,
    ) -> Result<i64, String> {
        let variant = self.get_property_raw(conn, path, interface, property).await?;
        variant
            .downcast_ref::<i64>()
            .map_err(|e| format!("Property {} is not an i64: {}", property, e))
    }

    /// Find the battery device path
    async fn find_battery_device(&self, conn: &Connection) -> Result<String, String> {
        // Common battery paths to try
        let common_paths = [
            "/org/freedesktop/UPower/devices/battery_BAT0",
            "/org/freedesktop/UPower/devices/battery_BAT1",
            "/org/freedesktop/UPower/devices/DisplayDevice",
        ];

        // Try common paths first
        for path in &common_paths {
            if self.device_exists(conn, path).await {
                debug!("Found battery at {}", path);
                return Ok(path.to_string());
            }
        }

        // If common paths don't work, enumerate devices
        let msg = conn
            .call_method(
                Some("org.freedesktop.UPower"),
                "/org/freedesktop/UPower",
                Some("org.freedesktop.UPower"),
                "EnumerateDevices",
                &(),
            )
            .await
            .map_err(|e| format!("Failed to enumerate devices: {}", e))?;

        let devices: Vec<zbus::zvariant::OwnedObjectPath> = msg
            .body()
            .deserialize()
            .map_err(|e| format!("Failed to parse devices: {}", e))?;

        for device in devices {
            let path = device.as_str();
            if path.contains("battery") || path.contains("BAT") {
                debug!("Found battery device: {}", path);
                return Ok(path.to_string());
            }
        }

        Err("No battery device found".to_string())
    }

    /// Check if a device path exists by trying to get a property
    async fn device_exists(&self, conn: &Connection, path: &str) -> bool {
        conn.call_method(
            Some("org.freedesktop.UPower"),
            path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.UPower.Device", "Type"),
        )
        .await
        .is_ok()
    }

    /// Check if UPower service is available
    pub async fn is_available(&mut self) -> bool {
        if self.connection.is_none() {
            if let Err(e) = self.connect().await {
                debug!("UPower not available: {}", e);
                return false;
            }
        }

        let conn = match &self.connection {
            Some(c) => c,
            None => return false,
        };

        // Try to get a property to verify service is running
        conn.call_method(
            Some("org.freedesktop.UPower"),
            "/org/freedesktop/UPower",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.UPower", "DaemonVersion"),
        )
        .await
        .is_ok()
    }
}

impl Default for UPowerBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_battery_state_from_u32() {
        assert_eq!(BatteryState::from(0), BatteryState::Unknown);
        assert_eq!(BatteryState::from(1), BatteryState::Charging);
        assert_eq!(BatteryState::from(2), BatteryState::Discharging);
        assert_eq!(BatteryState::from(3), BatteryState::Empty);
        assert_eq!(BatteryState::from(4), BatteryState::FullyCharged);
        assert_eq!(BatteryState::from(5), BatteryState::PendingCharge);
        assert_eq!(BatteryState::from(6), BatteryState::PendingDischarge);
        assert_eq!(BatteryState::from(99), BatteryState::Unknown);
    }

    #[test]
    fn test_battery_state_is_charging() {
        assert!(BatteryState::Charging.is_charging());
        assert!(BatteryState::PendingCharge.is_charging());
        assert!(!BatteryState::Discharging.is_charging());
        assert!(!BatteryState::FullyCharged.is_charging());
    }

    #[test]
    fn test_battery_state_as_str() {
        assert_eq!(BatteryState::Charging.as_str(), "charging");
        assert_eq!(BatteryState::Discharging.as_str(), "discharging");
        assert_eq!(BatteryState::FullyCharged.as_str(), "full");
    }

    #[test]
    fn test_power_status_default() {
        let status = PowerStatus::default();
        assert!(!status.on_battery);
        assert!(!status.battery_present);
        assert!(status.battery_percentage.is_none());
        assert_eq!(status.battery_state, BatteryState::Unknown);
    }

    #[test]
    fn test_upower_backend_new() {
        let backend = UPowerBackend::new();
        assert!(backend.connection.is_none());
    }

    // Integration tests require UPower to be running
    #[tokio::test]
    #[ignore = "Requires UPower DBus service"]
    async fn test_upower_connection() {
        let mut backend = UPowerBackend::new();
        let result = backend.connect().await;
        if result.is_ok() {
            assert!(backend.connection.is_some());
        }
    }

    #[tokio::test]
    #[ignore = "Requires UPower DBus service"]
    async fn test_get_power_status() {
        let mut backend = UPowerBackend::new();
        if backend.connect().await.is_ok() {
            let status = backend.get_power_status().await;
            assert!(status.is_ok());
        }
    }
}
