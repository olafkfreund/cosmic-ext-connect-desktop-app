//! File System Utilities with Enhanced Error Handling
//!
//! Provides safe file system operations with proper error handling,
//! disk space checks, and directory creation.

use crate::{ProtocolError, Result};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

/// Check if sufficient disk space is available
///
/// Returns `Ok(())` if space is available, otherwise returns `ResourceExhausted` error.
///
/// # Arguments
///
/// * `path` - Directory path to check space for
/// * `required_bytes` - Number of bytes required
///
/// # Examples
///
/// ```ignore
/// use cosmic_connect_protocol::fs_utils::check_disk_space;
///
/// check_disk_space("/home/user/Downloads", 10_000_000).await?;
/// ```
pub async fn check_disk_space(path: impl AsRef<Path>, required_bytes: u64) -> Result<()> {
    let path = path.as_ref();

    #[cfg(unix)]
    {
        use nix::sys::statvfs::statvfs;

        // Find the path to check - use the path itself if it exists, or its parent
        let check_path = if path.exists() {
            path.to_path_buf()
        } else if let Some(parent) = path.parent() {
            if parent.exists() {
                parent.to_path_buf()
            } else {
                // Fall back to root if parent doesn't exist
                PathBuf::from("/")
            }
        } else {
            PathBuf::from("/")
        };

        match statvfs(&check_path) {
            Ok(stat) => {
                // Available bytes = available blocks * fragment size
                // fragment_size is the actual unit of allocation
                let available_bytes = stat.blocks_available() * stat.fragment_size();

                debug!(
                    "Disk space check for {}: available={} bytes, required={} bytes",
                    path.display(),
                    available_bytes,
                    required_bytes
                );

                if available_bytes < required_bytes {
                    let available_mb = available_bytes / (1024 * 1024);
                    let required_mb = required_bytes / (1024 * 1024);
                    return Err(ProtocolError::ResourceExhausted(format!(
                        "Insufficient disk space: {} MB available, {} MB required",
                        available_mb, required_mb
                    )));
                }

                info!(
                    "Disk space check passed: {} MB available",
                    available_bytes / (1024 * 1024)
                );
            }
            Err(e) => {
                // Log the error but don't fail - let the OS handle it during write
                warn!(
                    "Could not check disk space for {}: {}. Proceeding anyway.",
                    path.display(),
                    e
                );
            }
        }
    }

    #[cfg(not(unix))]
    {
        // On non-Unix platforms, log that we're skipping the check
        debug!(
            "Disk space check for {} (required: {} bytes) - not implemented for this platform",
            path.display(),
            required_bytes
        );
    }

    Ok(())
}

/// Ensure parent directory exists, creating it if necessary
///
/// Returns `Ok(())` if directory exists or was created successfully.
///
/// # Arguments
///
/// * `file_path` - Path to file whose parent directory should exist
///
/// # Errors
///
/// Returns `PermissionDenied` if creation fails due to permissions.
/// Returns `Io` for other filesystem errors.
///
/// # Examples
///
/// ```ignore
/// use cosmic_connect_protocol::fs_utils::ensure_parent_dir;
///
/// ensure_parent_dir("/home/user/Downloads/subdir/file.txt").await?;
/// ```
pub async fn ensure_parent_dir(file_path: impl AsRef<Path>) -> Result<()> {
    let file_path = file_path.as_ref();

    if let Some(parent) = file_path.parent() {
        if !parent.exists() {
            debug!("Creating parent directory: {}", parent.display());

            fs::create_dir_all(parent).await.map_err(|e| {
                // Check if it's a permission error
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    ProtocolError::PermissionDenied(format!(
                        "Cannot create directory {}: permission denied",
                        parent.display()
                    ))
                } else {
                    ProtocolError::from_io_error(
                        e,
                        &format!("creating directory {}", parent.display()),
                    )
                }
            })?;
        }
    }

    Ok(())
}

/// Safe file creation with error handling
///
/// Creates a file, ensuring parent directory exists and handling common errors.
///
/// # Arguments
///
/// * `path` - Path to create file at
///
/// # Errors
///
/// Returns appropriate `ProtocolError` variant for different failure modes:
/// - `PermissionDenied` for permission errors
/// - `ResourceExhausted` for disk full
/// - `Io` for other errors
///
/// # Examples
///
/// ```ignore
/// use cosmic_connect_protocol::fs_utils::create_file_safe;
///
/// let mut file = create_file_safe("/home/user/Downloads/file.txt").await?;
/// file.write_all(b"content").await?;
/// ```
pub async fn create_file_safe(path: impl AsRef<Path>) -> Result<fs::File> {
    let path = path.as_ref();

    // Ensure parent directory exists
    ensure_parent_dir(path).await?;

    // Try to create the file
    let file = fs::File::create(path).await.map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => ProtocolError::PermissionDenied(format!(
            "Cannot create file {}: permission denied",
            path.display()
        )),
        std::io::ErrorKind::Other => {
            // Check if error message contains disk-related keywords
            let error_msg = e.to_string().to_lowercase();
            if error_msg.contains("no space") || error_msg.contains("disk full") {
                ProtocolError::ResourceExhausted(format!(
                    "Disk full: cannot create file {}",
                    path.display()
                ))
            } else {
                ProtocolError::from_io_error(e, &format!("creating file {}", path.display()))
            }
        }
        _ => ProtocolError::from_io_error(e, &format!("creating file {}", path.display())),
    })?;

    debug!("Created file: {}", path.display());
    Ok(file)
}

/// Safe file write with disk full detection
///
/// Writes data to a file, converting disk full errors to `ResourceExhausted`.
///
/// # Arguments
///
/// * `file` - Mutable reference to file
/// * `data` - Data to write
///
/// # Errors
///
/// Returns `ResourceExhausted` if disk is full during write.
/// Returns `Io` for other errors.
pub async fn write_file_safe(file: &mut fs::File, data: &[u8]) -> Result<()> {
    file.write_all(data).await.map_err(|e| match e.kind() {
        std::io::ErrorKind::Other => {
            let error_msg = e.to_string().to_lowercase();
            if error_msg.contains("no space") || error_msg.contains("disk full") {
                ProtocolError::ResourceExhausted("Disk full during file write".to_string())
            } else {
                ProtocolError::Io(e)
            }
        }
        _ => ProtocolError::Io(e),
    })
}

/// Clean up partial file on error
///
/// Attempts to delete a partially written file. Logs errors but doesn't fail.
///
/// # Arguments
///
/// * `path` - Path to file to clean up
pub async fn cleanup_partial_file(path: impl AsRef<Path>) {
    let path = path.as_ref();

    if path.exists() {
        if let Err(e) = fs::remove_file(path).await {
            warn!("Failed to clean up partial file {}: {}", path.display(), e);
        } else {
            debug!("Cleaned up partial file: {}", path.display());
        }
    }
}

/// Get a safe download path, handling filename conflicts
///
/// If the file already exists, appends " (1)", " (2)", etc. to the filename.
///
/// # Arguments
///
/// * `base_dir` - Base directory for downloads
/// * `filename` - Original filename
///
/// # Returns
///
/// A unique path that doesn't conflict with existing files
///
/// # Examples
///
/// ```ignore
/// use cosmic_connect_protocol::fs_utils::get_unique_download_path;
///
/// let path = get_unique_download_path("/home/user/Downloads", "file.txt").await;
/// // Returns: /home/user/Downloads/file.txt
/// // or /home/user/Downloads/file (1).txt if file.txt exists
/// ```
pub async fn get_unique_download_path(base_dir: impl AsRef<Path>, filename: &str) -> PathBuf {
    let base_dir = base_dir.as_ref();
    let mut path = base_dir.join(filename);

    // If file doesn't exist, use it as-is
    if !path.exists() {
        return path;
    }

    // Split filename into name and extension
    let (name, ext) = if let Some(dot_pos) = filename.rfind('.') {
        let (n, e) = filename.split_at(dot_pos);
        (n, e) // e includes the dot
    } else {
        (filename, "")
    };

    // Try incrementing numbers until we find a unique name
    for i in 1..1000 {
        let new_filename = if ext.is_empty() {
            format!("{} ({})", name, i)
        } else {
            format!("{} ({}){}", name, i, ext)
        };

        path = base_dir.join(new_filename);

        if !path.exists() {
            return path;
        }
    }

    // Fallback: use timestamp if we somehow hit 1000 conflicts
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
        .as_secs();

    let new_filename = if ext.is_empty() {
        format!("{}_{}", name, timestamp)
    } else {
        format!("{}_{}{}", name, timestamp, ext)
    };

    base_dir.join(new_filename)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_ensure_parent_dir_creates_nested() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("subdir1/subdir2/file.txt");

        ensure_parent_dir(&file_path).await.unwrap();

        assert!(file_path.parent().unwrap().exists());
    }

    #[tokio::test]
    async fn test_ensure_parent_dir_already_exists() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("file.txt");

        // Parent already exists (temp dir)
        ensure_parent_dir(&file_path).await.unwrap();
    }

    #[tokio::test]
    async fn test_create_file_safe() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("subdir/test.txt");

        let mut file = create_file_safe(&file_path).await.unwrap();
        file.write_all(b"test content").await.unwrap();

        assert!(file_path.exists());
    }

    #[tokio::test]
    async fn test_get_unique_download_path_no_conflict() {
        let temp = TempDir::new().unwrap();

        let path = get_unique_download_path(temp.path(), "test.txt").await;

        assert_eq!(path, temp.path().join("test.txt"));
    }

    #[tokio::test]
    async fn test_get_unique_download_path_with_conflict() {
        let temp = TempDir::new().unwrap();

        // Create existing file
        let existing = temp.path().join("test.txt");
        std::fs::File::create(&existing).unwrap();

        let path = get_unique_download_path(temp.path(), "test.txt").await;

        assert_eq!(path, temp.path().join("test (1).txt"));
    }

    #[tokio::test]
    async fn test_get_unique_download_path_multiple_conflicts() {
        let temp = TempDir::new().unwrap();

        // Create multiple existing files
        std::fs::File::create(temp.path().join("test.txt")).unwrap();
        std::fs::File::create(temp.path().join("test (1).txt")).unwrap();
        std::fs::File::create(temp.path().join("test (2).txt")).unwrap();

        let path = get_unique_download_path(temp.path(), "test.txt").await;

        assert_eq!(path, temp.path().join("test (3).txt"));
    }

    #[tokio::test]
    async fn test_cleanup_partial_file() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("partial.txt");

        // Create a file
        std::fs::File::create(&file_path)
            .unwrap()
            .write_all(b"partial")
            .unwrap();
        assert!(file_path.exists());

        // Clean it up
        cleanup_partial_file(&file_path).await;

        assert!(!file_path.exists());
    }

    #[tokio::test]
    async fn test_cleanup_nonexistent_file() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("nonexistent.txt");

        // Should not error
        cleanup_partial_file(&file_path).await;
    }

    #[tokio::test]
    async fn test_check_disk_space_existing_dir() {
        let temp = TempDir::new().unwrap();

        // Should succeed for reasonable space requirement
        let result = check_disk_space(temp.path(), 1024).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_disk_space_nonexistent_path() {
        let temp = TempDir::new().unwrap();
        let nonexistent = temp.path().join("does_not_exist/file.txt");

        // Should succeed (falls back to parent or root)
        let result = check_disk_space(&nonexistent, 1024).await;
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_check_disk_space_impossible_requirement() {
        let temp = TempDir::new().unwrap();

        // Require an impossibly large amount (1 PB)
        let result = check_disk_space(temp.path(), 1024 * 1024 * 1024 * 1024 * 1024).await;
        assert!(result.is_err());

        // Error should mention insufficient space
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Insufficient disk space"));
    }
}
