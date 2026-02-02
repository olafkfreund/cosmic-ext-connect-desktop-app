//! Systemd Logind Inhibitor Lock Management
//!
//! Provides sleep/shutdown inhibition via systemd-logind DBus interface.
//! Used by the Power plugin to prevent system sleep during file transfers.
//!
//! ## How It Works
//!
//! The systemd-logind service provides an `Inhibit()` method that returns a file
//! descriptor. As long as this file descriptor is kept open, the specified
//! operations (sleep, shutdown, etc.) are blocked or delayed.
//!
//! ## Inhibitor Types
//!
//! - `sleep` - Inhibit suspend/hibernate
//! - `shutdown` - Inhibit system shutdown/reboot
//! - `idle` - Inhibit automatic idle actions
//! - `handle-power-key` - Inhibit power key handling
//! - `handle-suspend-key` - Inhibit suspend key handling
//! - `handle-hibernate-key` - Inhibit hibernate key handling
//! - `handle-lid-switch` - Inhibit lid switch handling

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use tracing::{debug, info, warn};
use zbus::zvariant::OwnedFd as ZbusOwnedFd;
use zbus::Connection;

/// Type of inhibitor lock
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InhibitType {
    /// Inhibit suspend and hibernate
    Sleep,
    /// Inhibit shutdown and reboot
    Shutdown,
    /// Inhibit idle actions
    Idle,
    /// Inhibit all power management
    All,
}

impl InhibitType {
    /// Get the "what" string for logind
    fn as_what(&self) -> &'static str {
        match self {
            InhibitType::Sleep => "sleep",
            InhibitType::Shutdown => "shutdown",
            InhibitType::Idle => "idle",
            InhibitType::All => "sleep:shutdown:idle",
        }
    }
}

/// Inhibitor lock mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InhibitMode {
    /// Block the operation entirely until lock is released
    Block,
    /// Delay the operation briefly to allow cleanup
    Delay,
}

impl InhibitMode {
    /// Get the mode string for logind
    fn as_str(&self) -> &'static str {
        match self {
            InhibitMode::Block => "block",
            InhibitMode::Delay => "delay",
        }
    }
}

/// An active inhibitor lock
///
/// The lock is held as long as this struct exists.
/// Dropping it releases the lock.
pub struct InhibitorLock {
    /// File descriptor returned by logind (keeps lock active)
    _fd: OwnedFd,
    /// What is being inhibited
    what: InhibitType,
    /// Why we're inhibiting
    reason: String,
}

impl InhibitorLock {
    /// Get what is being inhibited
    pub fn what(&self) -> InhibitType {
        self.what
    }

    /// Get the inhibition reason
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl Drop for InhibitorLock {
    fn drop(&mut self) {
        info!(
            "Releasing {} inhibitor lock: {}",
            self.what.as_what(),
            self.reason
        );
        // File descriptor is automatically closed when OwnedFd is dropped
    }
}

/// Systemd inhibitor manager
pub struct SystemdInhibitor {
    /// DBus connection to system bus
    connection: Option<Connection>,
}

impl SystemdInhibitor {
    /// Create a new inhibitor manager
    pub fn new() -> Self {
        Self { connection: None }
    }

    /// Connect to the system DBus
    pub async fn connect(&mut self) -> Result<(), String> {
        match Connection::system().await {
            Ok(conn) => {
                self.connection = Some(conn);
                debug!("Connected to system DBus for inhibitor management");
                Ok(())
            }
            Err(e) => {
                warn!("Failed to connect to system DBus: {}", e);
                Err(format!("DBus connection failed: {}", e))
            }
        }
    }

    /// Acquire an inhibitor lock
    ///
    /// # Arguments
    ///
    /// * `what` - Type of inhibitor (sleep, shutdown, etc.)
    /// * `who` - Application name
    /// * `why` - Reason for inhibition
    /// * `mode` - Block or delay mode
    ///
    /// # Returns
    ///
    /// An `InhibitorLock` that must be kept alive to maintain the lock.
    pub async fn inhibit(
        &mut self,
        what: InhibitType,
        who: &str,
        why: &str,
        mode: InhibitMode,
    ) -> Result<InhibitorLock, String> {
        // Connect if not already connected
        if self.connection.is_none() {
            self.connect().await?;
        }

        let connection = self
            .connection
            .as_ref()
            .ok_or_else(|| "Not connected to DBus".to_string())?;

        info!(
            "Acquiring {} inhibitor lock: {} - {}",
            what.as_what(),
            who,
            why
        );

        // Call org.freedesktop.login1.Manager.Inhibit
        let reply = connection
            .call_method(
                Some("org.freedesktop.login1"),
                "/org/freedesktop/login1",
                Some("org.freedesktop.login1.Manager"),
                "Inhibit",
                &(what.as_what(), who, why, mode.as_str()),
            )
            .await
            .map_err(|e| format!("DBus call failed: {}", e))?;

        // Deserialize the file descriptor from the reply
        let fd: ZbusOwnedFd = reply
            .body()
            .deserialize()
            .map_err(|e| format!("Failed to deserialize FD: {}", e))?;

        // Duplicate the FD before zbus's OwnedFd drops it
        let raw_fd = fd.as_raw_fd();
        let dup_fd = nix::unistd::dup(raw_fd).map_err(|e| format!("Failed to dup FD: {}", e))?;
        // Safety: dup returns a valid file descriptor
        let owned_fd = unsafe { OwnedFd::from_raw_fd(dup_fd) };

        info!("Acquired {} inhibitor lock successfully", what.as_what());

        Ok(InhibitorLock {
            _fd: owned_fd,
            what,
            reason: why.to_string(),
        })
    }

    /// Check if inhibitor locks are supported on this system
    pub async fn is_available(&mut self) -> bool {
        if self.connection.is_none()
            && self.connect().await.is_err() {
                return false;
            }

        let connection = match &self.connection {
            Some(c) => c,
            None => return false,
        };

        // Try to call ListInhibitors to check if logind is available
        let result = connection
            .call_method(
                Some("org.freedesktop.login1"),
                "/org/freedesktop/login1",
                Some("org.freedesktop.login1.Manager"),
                "ListInhibitors",
                &(),
            )
            .await;

        result.is_ok()
    }
}

impl Default for SystemdInhibitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inhibit_type_what() {
        assert_eq!(InhibitType::Sleep.as_what(), "sleep");
        assert_eq!(InhibitType::Shutdown.as_what(), "shutdown");
        assert_eq!(InhibitType::Idle.as_what(), "idle");
        assert_eq!(InhibitType::All.as_what(), "sleep:shutdown:idle");
    }

    #[test]
    fn test_inhibit_mode_str() {
        assert_eq!(InhibitMode::Block.as_str(), "block");
        assert_eq!(InhibitMode::Delay.as_str(), "delay");
    }
}
