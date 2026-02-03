//! Desktop Icons Module
//!
//! Generates .desktop files for connected devices that can be placed on the desktop.
//! These desktop entries provide quick access to device actions via cosmic-connect-manager.

use anyhow::{Context, Result};
use cosmic_connect_protocol::{Device, DeviceType};
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info};

use crate::device_config::DeviceConfig;

/// Get the icon name based on device type
///
/// Returns symbolic icon names from the freedesktop icon theme spec
fn get_device_icon(device_type: DeviceType) -> &'static str {
    match device_type {
        DeviceType::Phone => "phone-symbolic",
        DeviceType::Tablet => "tablet-symbolic",
        DeviceType::Desktop => "computer-symbolic",
        DeviceType::Laptop => "laptop-symbolic",
        DeviceType::Tv => "video-display-symbolic",
    }
}

/// Sanitize a string for use in .desktop file fields
///
/// Removes control characters (newlines, carriage returns, etc.) that could
/// break the .desktop file format or potentially inject malicious keys.
///
/// # Arguments
///
/// * `input` - The string to sanitize
///
/// # Returns
///
/// Sanitized string safe for use in .desktop files
fn sanitize_desktop_field(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_control())
        .collect()
}

/// Generate .desktop file content for a device
///
/// Creates a complete desktop entry with the device name, type-appropriate icon,
/// and action shortcuts for common operations.
///
/// # Arguments
///
/// * `device` - Device information
/// * `config` - Optional device-specific configuration
///
/// # Returns
///
/// String containing the complete .desktop file content
pub fn generate_desktop_entry(device: &Device, config: Option<&DeviceConfig>) -> String {
    // Sanitize device name to prevent .desktop file injection attacks
    let raw_name = config
        .and_then(|c| c.nickname.as_ref())
        .unwrap_or(&device.info.device_name);
    let device_name = sanitize_desktop_field(raw_name);

    let icon = get_device_icon(device.info.device_type);
    let device_id = &device.info.device_id;

    let status = if device.is_connected() {
        "Connected"
    } else if device.is_paired() {
        "Paired"
    } else {
        "Available"
    };

    // Issue #143: Updated desktop entry with correct CLI args and drag & drop support
    format!(
        r#"[Desktop Entry]
Version=1.0
Type=Application
Name={name}
Comment={status} {device_type} device - Drop files here to send
Icon={icon}
Exec=cosmic-connect-manager --select-device {device_id} %U
Terminal=false
Categories=Network;FileTransfer;
Keywords=phone;device;sync;transfer;connect;
MimeType=application/octet-stream;text/plain;image/*;video/*;audio/*;
Actions=SendFile;Ping;Find;Browse;

[Desktop Action SendFile]
Name=Send File
Exec=cosmic-connect-manager --select-device {device_id} --tab share

[Desktop Action Ping]
Name=Ping Device
Exec=cosmic-connect-manager --select-device {device_id} --device-action ping

[Desktop Action Find]
Name=Find Device
Exec=cosmic-connect-manager --select-device {device_id} --device-action find

[Desktop Action Browse]
Name=Browse Files
Exec=cosmic-connect-manager --select-device {device_id} --tab files
"#,
        name = device_name,
        status = status,
        device_type = device.info.device_type.as_str(),
        icon = icon,
        device_id = device_id,
    )
}

/// Get the path where desktop icons should be saved in applications directory
///
/// Returns `~/.local/share/applications/cosmic-connect-{device_id}.desktop`
///
/// # Arguments
///
/// * `device_id` - The device ID
///
/// # Returns
///
/// PathBuf to the desktop file location
pub fn get_desktop_icon_path(device_id: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let applications_dir = PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("applications");

    applications_dir.join(format!("cosmic-connect-{}.desktop", device_id))
}

/// Get the path for desktop icon on the actual desktop (Issue #143)
///
/// Uses `dirs::desktop_dir()` to correctly respect the user's configured Desktop
/// location from `~/.config/user-dirs.dirs`, with fallback to `~/Desktop`.
///
/// # Arguments
///
/// * `device_id` - The device ID
///
/// # Returns
///
/// PathBuf to the desktop file location on the actual desktop
pub fn get_user_desktop_icon_path(device_id: &str) -> PathBuf {
    // Use dirs crate to properly read XDG user-dirs.dirs configuration
    let desktop_dir = dirs::desktop_dir().unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join("Desktop")
    });

    desktop_dir.join(format!("cosmic-connect-{}.desktop", device_id))
}

/// Save desktop icon file for a device
///
/// Creates the desktop entry file at the appropriate location. Ensures the
/// parent directory exists before writing.
///
/// # Arguments
///
/// * `device_id` - The device ID
/// * `content` - The .desktop file content
///
/// # Returns
///
/// Result indicating success or failure
pub fn save_desktop_icon(device_id: &str, content: &str) -> Result<()> {
    let desktop_path = get_desktop_icon_path(device_id);

    // Ensure parent directory exists
    if let Some(parent) = desktop_path.parent() {
        fs::create_dir_all(parent).context("Failed to create applications directory")?;
    }

    // Write desktop file
    fs::write(&desktop_path, content).context("Failed to write desktop file")?;

    info!("Created desktop icon at {:?}", desktop_path);
    Ok(())
}

/// Save desktop icon file to user's Desktop folder (Issue #143)
///
/// Creates the desktop entry file on the actual desktop (~/Desktop/) for
/// visibility and drag & drop support.
///
/// # Arguments
///
/// * `device_id` - The device ID
/// * `content` - The .desktop file content
///
/// # Returns
///
/// Result indicating success or failure
pub fn save_user_desktop_icon(device_id: &str, content: &str) -> Result<()> {
    let desktop_path = get_user_desktop_icon_path(device_id);

    // Ensure Desktop directory exists
    if let Some(parent) = desktop_path.parent() {
        // Only create if it doesn't exist - don't fail if Desktop doesn't exist
        if !parent.exists() {
            debug!("Desktop directory doesn't exist: {:?}", parent);
            return Ok(());
        }
    }

    // Write desktop file
    fs::write(&desktop_path, content).context("Failed to write desktop file to Desktop")?;

    // Make executable (required for .desktop files on desktop)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&desktop_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&desktop_path, perms)?;
    }

    info!("Created desktop icon on Desktop at {:?}", desktop_path);
    Ok(())
}

/// Remove desktop icon file for a device
///
/// Deletes the desktop entry file if it exists. Does not fail if the file
/// doesn't exist. Also removes from user's Desktop folder.
///
/// # Arguments
///
/// * `device_id` - The device ID
///
/// # Returns
///
/// Result indicating success or failure
pub fn remove_desktop_icon(device_id: &str) -> Result<()> {
    // Remove from applications directory
    let desktop_path = get_desktop_icon_path(device_id);
    if desktop_path.exists() {
        fs::remove_file(&desktop_path).context("Failed to remove desktop file")?;
        info!("Removed desktop icon at {:?}", desktop_path);
    } else {
        debug!("Desktop icon does not exist: {:?}", desktop_path);
    }

    // Also remove from user's Desktop (Issue #143)
    let user_desktop_path = get_user_desktop_icon_path(device_id);
    if user_desktop_path.exists() {
        fs::remove_file(&user_desktop_path).context("Failed to remove desktop file from Desktop")?;
        info!("Removed desktop icon from Desktop at {:?}", user_desktop_path);
    }

    Ok(())
}

/// Update desktop icon when device state changes
///
/// Regenerates the desktop entry content and updates the file. This should be
/// called when device connection status or configuration changes.
///
/// # Arguments
///
/// * `device` - Updated device information
/// * `config` - Optional device configuration
///
/// # Returns
///
/// Result indicating success or failure
pub fn update_desktop_icon(device: &Device, config: Option<&DeviceConfig>) -> Result<()> {
    let content = generate_desktop_entry(device, config);

    // Save to applications directory (for app launchers)
    save_desktop_icon(&device.info.device_id, &content)?;

    // Issue #143: Also save to user's Desktop for visibility and drag & drop
    save_user_desktop_icon(&device.info.device_id, &content)?;

    debug!("Updated desktop icon for device: {}", device.info.device_id);
    Ok(())
}

/// Ensure desktop icon exists for a paired device
///
/// Creates or updates the desktop icon if the device is paired.
/// Removes the desktop icon if the device is unpaired.
///
/// # Arguments
///
/// * `device` - Device information
/// * `config` - Optional device configuration
///
/// # Returns
///
/// Result indicating success or failure
pub fn sync_desktop_icon(device: &Device, config: Option<&DeviceConfig>) -> Result<()> {
    if device.is_paired() {
        update_desktop_icon(device, config)?;
    } else {
        remove_desktop_icon(&device.info.device_id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic_connect_protocol::{DeviceInfo, PairingStatus};
    use std::fs;

    fn create_test_device() -> Device {
        let info = DeviceInfo::new("Test Phone", DeviceType::Phone, 1716);
        Device::new(
            info,
            cosmic_connect_protocol::ConnectionState::Connected,
            PairingStatus::Paired,
        )
    }

    #[test]
    fn test_get_device_icon() {
        assert_eq!(get_device_icon(DeviceType::Phone), "phone-symbolic");
        assert_eq!(get_device_icon(DeviceType::Tablet), "tablet-symbolic");
        assert_eq!(get_device_icon(DeviceType::Desktop), "computer-symbolic");
        assert_eq!(get_device_icon(DeviceType::Laptop), "laptop-symbolic");
        assert_eq!(get_device_icon(DeviceType::Tv), "video-display-symbolic");
    }

    #[test]
    fn test_generate_desktop_entry() {
        let device = create_test_device();
        let content = generate_desktop_entry(&device, None);

        // Verify essential components
        assert!(content.contains("[Desktop Entry]"));
        assert!(content.contains("Type=Application"));
        assert!(content.contains("Name=Test Phone"));
        assert!(content.contains("Icon=phone-symbolic"));
        assert!(content.contains("Comment=Connected phone device - Drop files here to send"));
        assert!(content.contains(&device.info.device_id));

        // Verify actions
        assert!(content.contains("[Desktop Action SendFile]"));
        assert!(content.contains("[Desktop Action Ping]"));
        assert!(content.contains("[Desktop Action Find]"));
        assert!(content.contains("[Desktop Action Browse]"));
    }

    #[test]
    fn test_generate_desktop_entry_with_nickname() {
        let device = create_test_device();
        let mut config = DeviceConfig::new(device.info.device_id.clone());
        config.nickname = Some("My Phone".to_string());

        let content = generate_desktop_entry(&device, Some(&config));
        assert!(content.contains("Name=My Phone"));
    }

    #[test]
    fn test_desktop_icon_path() {
        let path = get_desktop_icon_path("test_device_123");

        assert!(path.to_string_lossy().contains(".local/share/applications"));
        assert!(path
            .to_string_lossy()
            .contains("cosmic-connect-test_device_123.desktop"));
    }

    #[test]
    fn test_save_and_remove_desktop_icon() {
        let device = create_test_device();
        let device_id = &device.info.device_id;
        let content = generate_desktop_entry(&device, None);

        // Save desktop icon
        let result = save_desktop_icon(device_id, &content);
        assert!(result.is_ok());

        // Verify file exists
        let path = get_desktop_icon_path(device_id);
        assert!(path.exists());

        // Verify content
        let saved_content = fs::read_to_string(&path).unwrap();
        assert_eq!(saved_content, content);

        // Remove desktop icon
        let result = remove_desktop_icon(device_id);
        assert!(result.is_ok());

        // Verify file removed
        assert!(!path.exists());

        // Removing non-existent file should not fail
        let result = remove_desktop_icon(device_id);
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_desktop_icon() {
        let device = create_test_device();
        let device_id = &device.info.device_id;

        // Create initial desktop icon
        let result = update_desktop_icon(&device, None);
        assert!(result.is_ok());

        let path = get_desktop_icon_path(device_id);
        assert!(path.exists());

        // Update with nickname
        let mut config = DeviceConfig::new(device_id.clone());
        config.nickname = Some("Updated Name".to_string());

        let result = update_desktop_icon(&device, Some(&config));
        assert!(result.is_ok());

        // Verify updated content
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("Name=Updated Name"));

        // Cleanup
        remove_desktop_icon(device_id).ok();
    }

    #[test]
    fn test_sync_desktop_icon_paired() {
        let device = create_test_device();
        let device_id = &device.info.device_id;

        // Sync paired device should create icon
        let result = sync_desktop_icon(&device, None);
        assert!(result.is_ok());

        let path = get_desktop_icon_path(device_id);
        assert!(path.exists());

        // Cleanup
        remove_desktop_icon(device_id).ok();
    }

    #[test]
    fn test_sync_desktop_icon_unpaired() {
        let device = create_test_device();
        let device_id = &device.info.device_id;

        // Create desktop icon first
        update_desktop_icon(&device, None).ok();

        // Create unpaired device
        let mut unpaired_device = device.clone();
        unpaired_device.pairing_status = PairingStatus::Unpaired;
        unpaired_device.is_trusted = false;

        // Sync unpaired device should remove icon
        let result = sync_desktop_icon(&unpaired_device, None);
        assert!(result.is_ok());

        let path = get_desktop_icon_path(device_id);
        assert!(!path.exists());
    }

    #[test]
    fn test_device_status_in_comment() {
        // Test connected device
        let connected = create_test_device();
        let content = generate_desktop_entry(&connected, None);
        assert!(content.contains("Comment=Connected phone device - Drop files here to send"));

        // Test paired but disconnected
        let mut paired = connected.clone();
        paired.connection_state = cosmic_connect_protocol::ConnectionState::Disconnected;
        let content = generate_desktop_entry(&paired, None);
        assert!(content.contains("Comment=Paired phone device - Drop files here to send"));

        // Test unpaired
        let mut unpaired = paired.clone();
        unpaired.pairing_status = PairingStatus::Unpaired;
        unpaired.is_trusted = false;
        let content = generate_desktop_entry(&unpaired, None);
        assert!(content.contains("Comment=Available phone device - Drop files here to send"));
    }
}
