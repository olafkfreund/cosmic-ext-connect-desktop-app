//! Logind DBus Backend for Screen Lock Control
//!
//! Provides screen lock/unlock functionality via systemd-logind DBus interface.
//!
//! ## Logind DBus Interface
//!
//! - Service: `org.freedesktop.login1`
//! - Manager: `/org/freedesktop/login1`
//! - Session: `/org/freedesktop/login1/session/<id>`
//!
//! ## Session Methods
//!
//! - `Lock()`: Lock the session screen
//! - `Unlock()`: Unlock the session screen (requires privileges)
//! - `Terminate()`: Terminate the session
//!
//! ## Session Properties
//!
//! - `LockedHint`: Boolean indicating if session is locked
//! - `Active`: Boolean indicating if session is active
//! - `State`: Session state (online, active, closing)

use std::env;
use tracing::{debug, info};
use zbus::zvariant::OwnedValue;
use zbus::Connection;

/// DBus service name for logind
const LOGIND_SERVICE: &str = "org.freedesktop.login1";
/// DBus path for logind manager
const LOGIND_MANAGER_PATH: &str = "/org/freedesktop/login1";
/// DBus interface for logind manager
const LOGIND_MANAGER_INTERFACE: &str = "org.freedesktop.login1.Manager";
/// DBus interface for logind session
const LOGIND_SESSION_INTERFACE: &str = "org.freedesktop.login1.Session";
/// DBus properties interface
const DBUS_PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";

/// Lock state information
#[derive(Debug, Clone)]
pub struct LockState {
    /// Whether the session is locked
    pub is_locked: bool,
    /// Whether the session is active
    pub is_active: bool,
    /// Session state string
    pub state: String,
    /// Session ID
    pub session_id: String,
}

impl Default for LockState {
    fn default() -> Self {
        Self {
            is_locked: false,
            is_active: true,
            state: "unknown".to_string(),
            session_id: String::new(),
        }
    }
}

/// Logind DBus backend for screen lock control
pub struct LogindBackend {
    /// DBus connection
    connection: Option<Connection>,
    /// Current session path
    session_path: Option<String>,
}

impl LogindBackend {
    /// Create a new Logind backend
    pub fn new() -> Self {
        Self {
            connection: None,
            session_path: None,
        }
    }

    /// Connect to the system DBus and discover session
    pub async fn connect(&mut self) -> Result<(), String> {
        if self.connection.is_some() {
            return Ok(());
        }

        let conn = Connection::system()
            .await
            .map_err(|e| format!("Failed to connect to system bus: {}", e))?;

        self.connection = Some(conn);

        // Discover current session
        self.discover_session().await?;

        info!("Connected to logind via DBus");
        Ok(())
    }

    /// Discover the current user's session
    async fn discover_session(&mut self) -> Result<(), String> {
        let conn = self.connection.as_ref().ok_or("Not connected")?;

        // Try to get session from XDG_SESSION_ID first
        if let Ok(session_id) = env::var("XDG_SESSION_ID") {
            let path = format!("{}/session/{}", LOGIND_MANAGER_PATH, session_id);
            if self.session_exists(conn, &path).await {
                self.session_path = Some(path);
                debug!("Using session from XDG_SESSION_ID: {}", session_id);
                return Ok(());
            }
        }

        // Fall back to GetSessionByPID
        let pid = std::process::id();
        let msg = conn
            .call_method(
                Some(LOGIND_SERVICE),
                LOGIND_MANAGER_PATH,
                Some(LOGIND_MANAGER_INTERFACE),
                "GetSessionByPID",
                &(pid,),
            )
            .await
            .map_err(|e| format!("Failed to get session by PID: {}", e))?;

        let path: zbus::zvariant::OwnedObjectPath = msg
            .body()
            .deserialize()
            .map_err(|e| format!("Failed to parse session path: {}", e))?;

        self.session_path = Some(path.to_string());
        debug!("Discovered session path: {}", path);
        Ok(())
    }

    /// Check if a session path exists
    async fn session_exists(&self, conn: &Connection, path: &str) -> bool {
        conn.call_method(
            Some(LOGIND_SERVICE),
            path,
            Some(DBUS_PROPERTIES_INTERFACE),
            "Get",
            &(LOGIND_SESSION_INTERFACE, "Id"),
        )
        .await
        .is_ok()
    }

    /// Lock the screen
    pub async fn lock(&mut self) -> Result<(), String> {
        self.ensure_connected().await?;

        let conn = self.connection.as_ref().ok_or("Not connected")?;
        let path = self.session_path.as_ref().ok_or("Session not discovered")?;

        info!("Locking screen via logind DBus");

        conn.call_method(
            Some(LOGIND_SERVICE),
            path.as_str(),
            Some(LOGIND_SESSION_INTERFACE),
            "Lock",
            &(),
        )
        .await
        .map_err(|e| format!("Failed to lock session: {}", e))?;

        info!("Screen locked successfully");
        Ok(())
    }

    /// Unlock the screen
    ///
    /// Note: This typically requires elevated privileges and may not work
    /// for security reasons on most systems.
    pub async fn unlock(&mut self) -> Result<(), String> {
        self.ensure_connected().await?;

        let conn = self.connection.as_ref().ok_or("Not connected")?;
        let path = self.session_path.as_ref().ok_or("Session not discovered")?;

        info!("Unlocking screen via logind DBus");

        conn.call_method(
            Some(LOGIND_SERVICE),
            path.as_str(),
            Some(LOGIND_SESSION_INTERFACE),
            "Unlock",
            &(),
        )
        .await
        .map_err(|e| format!("Failed to unlock session: {}", e))?;

        info!("Screen unlocked successfully");
        Ok(())
    }

    /// Ensure connection is established
    async fn ensure_connected(&mut self) -> Result<(), String> {
        if self.connection.is_none() {
            self.connect().await?;
        }
        Ok(())
    }

    /// Get the current lock state
    pub async fn get_lock_state(&mut self) -> Result<LockState, String> {
        self.ensure_connected().await?;

        let conn = self.connection.as_ref().ok_or("Not connected")?;
        let path = self.session_path.as_ref().ok_or("Session not discovered")?;

        let mut state = LockState::default();

        // Get session ID
        if let Ok(id) = self.get_property_string(conn, path, "Id").await {
            state.session_id = id;
        }

        // Get LockedHint
        if let Ok(locked) = self.get_property_bool(conn, path, "LockedHint").await {
            state.is_locked = locked;
            debug!("LockedHint: {}", locked);
        }

        // Get Active
        if let Ok(active) = self.get_property_bool(conn, path, "Active").await {
            state.is_active = active;
            debug!("Active: {}", active);
        }

        // Get State
        if let Ok(session_state) = self.get_property_string(conn, path, "State").await {
            debug!("State: {}", session_state);
            state.state = session_state;
        }

        Ok(state)
    }

    /// Check if the screen is currently locked
    pub async fn is_locked(&mut self) -> Result<bool, String> {
        let state = self.get_lock_state().await?;
        Ok(state.is_locked)
    }

    // ========== Power Actions ==========
    // These methods call the Manager interface on /org/freedesktop/login1
    // rather than the Session interface

    /// Execute a power action via logind Manager interface
    async fn execute_power_action(&mut self, method: &str, interactive: bool) -> Result<(), String> {
        self.ensure_connected().await?;
        let conn = self.connection.as_ref().ok_or("Not connected")?;

        info!("{} system via logind DBus", method);

        conn.call_method(
            Some(LOGIND_SERVICE),
            LOGIND_MANAGER_PATH,
            Some(LOGIND_MANAGER_INTERFACE),
            method,
            &(interactive,),
        )
        .await
        .map_err(|e| format!("Failed to {}: {}", method.to_lowercase(), e))?;

        info!("{} initiated", method);
        Ok(())
    }

    /// Power off the system
    pub async fn power_off(&mut self, interactive: bool) -> Result<(), String> {
        self.execute_power_action("PowerOff", interactive).await
    }

    /// Reboot the system
    pub async fn reboot(&mut self, interactive: bool) -> Result<(), String> {
        self.execute_power_action("Reboot", interactive).await
    }

    /// Suspend the system (suspend to RAM)
    pub async fn suspend(&mut self, interactive: bool) -> Result<(), String> {
        self.execute_power_action("Suspend", interactive).await
    }

    /// Hibernate the system (suspend to disk)
    pub async fn hibernate(&mut self, interactive: bool) -> Result<(), String> {
        self.execute_power_action("Hibernate", interactive).await
    }

    /// Check if the system can power off
    pub async fn can_power_off(&mut self) -> Result<bool, String> {
        self.can_action("CanPowerOff").await
    }

    /// Check if the system can reboot
    pub async fn can_reboot(&mut self) -> Result<bool, String> {
        self.can_action("CanReboot").await
    }

    /// Check if the system can suspend
    pub async fn can_suspend(&mut self) -> Result<bool, String> {
        self.can_action("CanSuspend").await
    }

    /// Check if the system can hibernate
    pub async fn can_hibernate(&mut self) -> Result<bool, String> {
        self.can_action("CanHibernate").await
    }

    /// Check if a power action is available
    async fn can_action(&mut self, method: &str) -> Result<bool, String> {
        self.ensure_connected().await?;
        let conn = self.connection.as_ref().ok_or("Not connected")?;

        let msg = conn
            .call_method(
                Some(LOGIND_SERVICE),
                LOGIND_MANAGER_PATH,
                Some(LOGIND_MANAGER_INTERFACE),
                method,
                &(),
            )
            .await
            .map_err(|e| format!("Failed to check {}: {}", method, e))?;

        let result: String = msg
            .body()
            .deserialize()
            .map_err(|e| format!("Failed to parse {} result: {}", method, e))?;

        // logind returns "yes", "no", "challenge", or "na"
        Ok(result == "yes" || result == "challenge")
    }

    /// Get a DBus property as OwnedValue
    async fn get_property_raw(
        &self,
        conn: &Connection,
        path: &str,
        property: &str,
    ) -> Result<OwnedValue, String> {
        let msg = conn
            .call_method(
                Some(LOGIND_SERVICE),
                path,
                Some(DBUS_PROPERTIES_INTERFACE),
                "Get",
                &(LOGIND_SESSION_INTERFACE, property),
            )
            .await
            .map_err(|e| format!("Failed to get {}: {}", property, e))?;

        msg.body()
            .deserialize()
            .map_err(|e| format!("Failed to parse {}: {}", property, e))
    }

    /// Get a boolean property
    async fn get_property_bool(
        &self,
        conn: &Connection,
        path: &str,
        property: &str,
    ) -> Result<bool, String> {
        let variant = self.get_property_raw(conn, path, property).await?;
        variant
            .downcast_ref::<bool>()
            .map_err(|e| format!("Property {} is not a bool: {}", property, e))
    }

    /// Get a string property
    async fn get_property_string(
        &self,
        conn: &Connection,
        path: &str,
        property: &str,
    ) -> Result<String, String> {
        let variant = self.get_property_raw(conn, path, property).await?;
        variant
            .try_into()
            .map_err(|e: zbus::zvariant::Error| format!("Property {} is not a string: {}", property, e))
    }

    /// Check if logind service is available
    pub async fn is_available(&mut self) -> bool {
        if self.connection.is_none() {
            if let Err(e) = self.connect().await {
                debug!("Logind not available: {}", e);
                return false;
            }
        }

        self.session_path.is_some()
    }
}

impl Default for LogindBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_state_default() {
        let state = LockState::default();
        assert!(!state.is_locked);
        assert!(state.is_active);
        assert_eq!(state.state, "unknown");
        assert!(state.session_id.is_empty());
    }

    #[test]
    fn test_logind_backend_new() {
        let backend = LogindBackend::new();
        assert!(backend.connection.is_none());
        assert!(backend.session_path.is_none());
    }

    // Integration tests require logind to be running
    #[tokio::test]
    #[ignore = "Requires logind DBus service"]
    async fn test_logind_connection() {
        let mut backend = LogindBackend::new();
        let result = backend.connect().await;
        if result.is_ok() {
            assert!(backend.connection.is_some());
            assert!(backend.session_path.is_some());
        }
    }

    #[tokio::test]
    #[ignore = "Requires logind DBus service"]
    async fn test_get_lock_state() {
        let mut backend = LogindBackend::new();
        if backend.connect().await.is_ok() {
            let state = backend.get_lock_state().await;
            assert!(state.is_ok());
        }
    }
}
