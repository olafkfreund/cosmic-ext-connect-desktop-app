//! Centralized Error Handling and User Notification System
//!
//! This module provides unified error handling across the daemon, automatically
//! showing user-friendly notifications for errors that require user attention.

use anyhow::Result;
use cosmic_connect_protocol::ProtocolError;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, warn};

use crate::cosmic_notifications::CosmicNotifier;

/// Central error handler that manages error notifications and recovery
#[derive(Clone)]
pub struct ErrorHandler {
    notifier: Arc<RwLock<Option<CosmicNotifier>>>,
}

impl ErrorHandler {
    /// Create a new error handler
    pub fn new() -> Self {
        Self {
            notifier: Arc::new(RwLock::new(None)),
        }
    }

    /// Initialize the error handler with a notifier
    ///
    /// This should be called during daemon startup.
    pub async fn init(&self) -> Result<()> {
        let notifier = CosmicNotifier::new().await?;
        *self.notifier.write().await = Some(notifier);
        debug!("Error handler initialized with notification support");
        Ok(())
    }

    /// Handle a protocol error with appropriate logging and user notification
    ///
    /// This method examines the error type and:
    /// - Logs the error at appropriate level (error, warn, debug)
    /// - Shows user notification if the error requires user attention
    /// - Returns whether the error is recoverable for retry logic
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let handler = ErrorHandler::new();
    /// handler.init().await?;
    ///
    /// match connection_result {
    ///     Err(e) => {
    ///         let recoverable = handler.handle_error(&e, "connecting to device", Some("device-123")).await;
    ///         if recoverable {
    ///             // Retry logic
    ///         }
    ///     }
    ///     Ok(_) => { /* ... */ }
    /// }
    /// ```
    pub async fn handle_error(
        &self,
        error: &ProtocolError,
        context: &str,
        device_id: Option<&str>,
    ) -> bool {
        // Log the error at appropriate level
        if error.is_recoverable() {
            warn!("Recoverable error {}: {}", context, error);
        } else if error.requires_user_action() {
            warn!("User action required {}: {}", context, error);
        } else {
            error!("Critical error {}: {}", context, error);
        }

        // Show user notification if error requires user attention
        if error.requires_user_action() {
            if let Err(e) = self.notify_error(error, device_id).await {
                error!("Failed to show error notification: {}", e);
            }
        }

        // Return whether error is recoverable for retry logic
        error.is_recoverable()
    }

    /// Show an error notification to the user
    async fn notify_error(&self, error: &ProtocolError, device_id: Option<&str>) -> Result<()> {
        let notifier_guard = self.notifier.read().await;
        let Some(notifier) = notifier_guard.as_ref() else {
            debug!("Notifier not initialized, skipping error notification");
            return Ok(());
        };

        let device_name = device_id.unwrap_or("device");

        match error {
            ProtocolError::NotPaired => {
                notifier
                    .notify_error_with_recovery(
                        "Device Not Paired",
                        &format!(
                            "This operation requires pairing with {}.\nPlease pair the device first.",
                            device_name
                        ),
                        Some(("pair", "Pair Device")),
                    )
                    .await?;
            }

            ProtocolError::Timeout(msg) => {
                notifier.notify_connection_timeout(device_name).await?;
            }

            ProtocolError::ConnectionRefused(_) => {
                notifier
                    .notify_network_error(
                        device_name,
                        "Connection refused. Check if CConnect is running on the device.",
                    )
                    .await?;
            }

            ProtocolError::NetworkUnreachable(_) => {
                notifier
                    .notify_network_error(
                        device_name,
                        "Network unreachable. Check if both devices are on the same network.",
                    )
                    .await?;
            }

            ProtocolError::NetworkError(msg) => {
                notifier.notify_network_error(device_name, msg).await?;
            }

            ProtocolError::PermissionDenied(msg) => {
                notifier
                    .notify_permission_error("access resource", msg)
                    .await?;
            }

            ProtocolError::ResourceExhausted(msg) if msg.contains("disk") || msg.contains("space") =>
            {
                notifier
                    .notify_disk_full_error("downloads directory")
                    .await?;
            }

            ProtocolError::Configuration(msg) => {
                notifier.notify_configuration_error(msg).await?;
            }

            ProtocolError::CertificateValidation(msg) => {
                notifier
                    .notify_certificate_error(device_name, &msg)
                    .await?;
            }

            ProtocolError::Certificate(err) => {
                let msg = format!("{}", err);
                notifier
                    .notify_certificate_error(device_name, &msg)
                    .await?;
            }

            ProtocolError::ProtocolVersionMismatch(msg) => {
                notifier
                    .notify_protocol_mismatch(device_name, msg)
                    .await?;
            }

            ProtocolError::Plugin(msg) => {
                // Extract plugin name from error message if possible
                let plugin_name = msg
                    .split_whitespace()
                    .next()
                    .unwrap_or("Unknown plugin");
                notifier
                    .notify_plugin_error(plugin_name, device_name, msg)
                    .await?;
            }

            _ => {
                // Generic error notification
                notifier
                    .notify_error_with_recovery(
                        "Operation Failed",
                        &error.user_message(),
                        None,
                    )
                    .await?;
            }
        }

        Ok(())
    }

    /// Handle a file transfer error with notification
    pub async fn handle_file_transfer_error(
        &self,
        device_name: &str,
        filename: &str,
        error: &ProtocolError,
    ) -> Result<()> {
        error!(
            "File transfer error for {} to {}: {}",
            filename, device_name, error
        );

        let notifier_guard = self.notifier.read().await;
        if let Some(notifier) = notifier_guard.as_ref() {
            notifier
                .notify_file_transfer_error(device_name, filename, &error.user_message())
                .await?;
        }

        Ok(())
    }

    /// Handle a plugin error with optional notification
    ///
    /// Plugin errors are logged but typically don't require user notification
    /// unless they prevent core functionality.
    pub async fn handle_plugin_error(
        &self,
        plugin_name: &str,
        device_id: &str,
        error: &ProtocolError,
        notify_user: bool,
    ) -> Result<()> {
        warn!(
            "Plugin {} error for device {}: {}",
            plugin_name, device_id, error
        );

        if notify_user {
            let notifier_guard = self.notifier.read().await;
            if let Some(notifier) = notifier_guard.as_ref() {
                notifier
                    .notify_plugin_error(plugin_name, device_id, &error.user_message())
                    .await?;
            }
        }

        Ok(())
    }

    /// Check if notifications are enabled
    pub async fn notifications_enabled(&self) -> bool {
        self.notifier.read().await.is_some()
    }
}

impl Default for ErrorHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_error_handler_creation() {
        let handler = ErrorHandler::new();
        assert!(!handler.notifications_enabled().await);
    }

    #[test]
    fn test_error_classification() {
        // Recoverable errors
        let error = ProtocolError::Timeout("test".to_string());
        assert!(error.is_recoverable());
        assert!(!error.requires_user_action());

        let error = ProtocolError::NetworkError("test".to_string());
        assert!(error.is_recoverable());

        // User action required
        let error = ProtocolError::NotPaired;
        assert!(!error.is_recoverable());
        assert!(error.requires_user_action());

        let error = ProtocolError::PermissionDenied("test".to_string());
        assert!(!error.is_recoverable());
        assert!(error.requires_user_action());

        // Critical errors
        let error = ProtocolError::InvalidPacket("test".to_string());
        assert!(!error.is_recoverable());
        assert!(!error.requires_user_action());
    }

    #[test]
    fn test_user_messages() {
        let error = ProtocolError::NotPaired;
        assert!(error
            .user_message()
            .contains("Please pair the device first"));

        let error = ProtocolError::Timeout("connection".to_string());
        assert!(error.user_message().contains("timeout"));
        assert!(error.user_message().contains("network"));

        let error = ProtocolError::PermissionDenied("file access".to_string());
        assert!(error.user_message().contains("Permission denied"));
        assert!(error.user_message().contains("permissions"));
    }
}
