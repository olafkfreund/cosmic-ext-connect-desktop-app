//! Desktop Notification System for COSMIC Connect
//!
//! Provides system notification integration using native Linux desktop notifications
//! via the notify-rust crate. Displays notifications for key device events like
//! connection, pairing, and transfer completion.

use notify_rust::{Notification, Timeout};
use tracing::{debug, warn};

/// Application name for notifications
const APP_NAME: &str = "COSMIC Connect";

/// Default timeout for notifications (5 seconds)
const DEFAULT_TIMEOUT_MS: i32 = 5000;

/// Show a device-related notification
///
/// # Arguments
/// * `title` - Notification title
/// * `body` - Notification body text
/// * `icon` - Icon name (should be valid freedesktop icon name)
pub fn show_device_notification(title: &str, body: &str, icon: &str) {
    debug!("Showing device notification: {} - {}", title, body);

    if let Err(e) = Notification::new()
        .appname(APP_NAME)
        .summary(title)
        .body(body)
        .icon(icon)
        .timeout(Timeout::Milliseconds(DEFAULT_TIMEOUT_MS as u32))
        .show()
    {
        warn!("Failed to show device notification: {}", e);
    }
}

/// Show notification for a device being discovered/connected
///
/// # Arguments
/// * `device_name` - Name of the discovered device
pub fn notify_device_discovered(device_name: &str) {
    show_device_notification(
        "Device Discovered",
        &format!("Connected to {}", device_name),
        "phone-symbolic",
    );
}

/// Show notification for a device being disconnected
///
/// # Arguments
/// * `device_name` - Name of the disconnected device
pub fn notify_device_disconnected(device_name: &str) {
    show_device_notification(
        "Device Disconnected",
        &format!("{} is no longer available", device_name),
        "network-wireless-offline-symbolic",
    );
}

/// Show notification for a pairing request
///
/// # Arguments
/// * `device_name` - Name of the device requesting pairing
pub fn notify_pairing_request(device_name: &str) {
    show_device_notification(
        "Pairing Request",
        &format!("{} wants to pair with this computer", device_name),
        "dialog-question-symbolic",
    );
}

/// Show notification for successful pairing
///
/// # Arguments
/// * `device_name` - Name of the newly paired device
pub fn notify_pairing_success(device_name: &str) {
    show_device_notification(
        "Device Paired",
        &format!("Successfully paired with {}", device_name),
        "emblem-ok-symbolic",
    );
}

/// Show notification for failed pairing
///
/// # Arguments
/// * `device_name` - Name of the device
/// * `reason` - Reason for failure (optional)
pub fn notify_pairing_failed(device_name: &str, reason: Option<&str>) {
    let body = if let Some(reason) = reason {
        format!("Failed to pair with {}: {}", device_name, reason)
    } else {
        format!("Failed to pair with {}", device_name)
    };

    show_device_notification("Pairing Failed", &body, "dialog-error-symbolic");
}

/// Show notification for a completed file transfer
///
/// # Arguments
/// * `filename` - Name of the transferred file
/// * `success` - Whether the transfer succeeded
/// * `error_message` - Error message if transfer failed
pub fn notify_transfer_complete(filename: &str, success: bool, error_message: Option<&str>) {
    if success {
        show_device_notification(
            "Transfer Complete",
            &format!("Received: {}", filename),
            "document-send-symbolic",
        );
    } else {
        let body = if let Some(error) = error_message {
            format!("Failed to transfer {}: {}", filename, error)
        } else {
            format!("Failed to transfer {}", filename)
        };

        show_device_notification("Transfer Failed", &body, "dialog-error-symbolic");
    }
}

/// Show notification for daemon reconnection
pub fn notify_daemon_reconnected() {
    show_device_notification(
        "Service Reconnected",
        "COSMIC Connect daemon is back online",
        "emblem-synchronizing-symbolic",
    );
}

/// Show notification for incoming ping
///
/// # Arguments
/// * `device_name` - Name of the device that sent the ping
/// * `message` - Optional message with the ping
pub fn notify_ping_received(device_name: &str, message: Option<&str>) {
    let body = if let Some(msg) = message {
        format!("{} says: {}", device_name, msg)
    } else {
        format!("Ping from {}", device_name)
    };

    show_device_notification(
        "Ping Received",
        &body,
        "preferences-system-notifications-symbolic",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_constants() {
        assert_eq!(APP_NAME, "COSMIC Connect");
        assert_eq!(DEFAULT_TIMEOUT_MS, 5000);
    }
}
