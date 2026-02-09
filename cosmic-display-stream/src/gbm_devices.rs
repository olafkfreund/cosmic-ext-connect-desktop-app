//! GBM Device Management for `DMA-BUF` Zero-Copy Capture
//!
//! This module provides GPU buffer management for `DMA-BUF` zero-copy capture from `PipeWire`.
//! It wraps the GBM (Generic Buffer Manager) library to allocate GPU buffers and export them
//! as DMA-BUF file descriptors for efficient screen capture.
//!
//! # Example
//!
//! ```no_run
//! use cosmic_display_stream::gbm_devices::GbmDeviceManager;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a device manager (opens default render node)
//! let mut manager = GbmDeviceManager::new()?;
//!
//! // Test if a format is supported
//! if manager.test_format(gbm::Format::Abgr8888, 0)? {
//!     // Allocate a buffer
//!     let bo = manager.allocate_buffer(1920, 1080, gbm::Format::Abgr8888, &[0])?;
//!
//!     // Export as DMA-BUF
//!     let dmabuf = manager.export_dmabuf(&bo)?;
//!     println!("DMA-BUF fd: {}", dmabuf.fd);
//! }
//! # Ok(())
//! # }
//! ```

use crate::error::{DisplayStreamError, Result};
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Wraps a GBM device handle
pub struct GbmDevice {
    /// The underlying GBM device
    device: gbm::Device<File>,
    /// Path to the DRI device node
    path: PathBuf,
}

impl GbmDevice {
    /// Create a new GBM device from a file
    ///
    /// # Arguments
    ///
    /// * `file` - Opened DRI device file
    /// * `path` - Path to the device node
    ///
    /// # Errors
    ///
    /// Returns an error if GBM device creation fails
    pub fn new(file: File, path: PathBuf) -> Result<Self> {
        let device = gbm::Device::new(file).map_err(|e| {
            DisplayStreamError::Gbm(format!("Failed to create GBM device: {e}"))
        })?;

        Ok(Self { device, path })
    }

    /// Get a reference to the underlying GBM device
    #[must_use]
    pub fn device(&self) -> &gbm::Device<File> {
        &self.device
    }

    /// Get the device path
    #[must_use]
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

/// Information about a DMA-BUF
#[derive(Debug, Clone)]
pub struct DmaBufInfo {
    /// File descriptor for the DMA-BUF
    pub fd: i32,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Pixel format (`FourCC`)
    pub format: u32,
    /// Format modifier
    pub modifier: u64,
    /// Number of planes
    pub num_planes: u32,
    /// Stride for each plane (bytes per row)
    pub strides: Vec<u32>,
    /// Offset for each plane
    pub offsets: Vec<u32>,
}

impl DmaBufInfo {
    /// Create a new `DmaBufInfo`
    ///
    /// # Arguments
    ///
    /// * `fd` - DMA-BUF file descriptor
    /// * `width` - Width in pixels
    /// * `height` - Height in pixels
    /// * `format` - DRM `FourCC` format code
    /// * `modifier` - Format modifier
    /// * `num_planes` - Number of planes
    /// * `strides` - Stride for each plane
    /// * `offsets` - Offset for each plane
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fd: i32,
        width: u32,
        height: u32,
        format: u32,
        modifier: u64,
        num_planes: u32,
        strides: Vec<u32>,
        offsets: Vec<u32>,
    ) -> Self {
        Self {
            fd,
            width,
            height,
            format,
            modifier,
            num_planes,
            strides,
            offsets,
        }
    }
}

/// Manages GBM device handles for buffer allocation
pub struct GbmDeviceManager {
    /// The active GBM device
    device: GbmDevice,
}

impl GbmDeviceManager {
    /// Create a new GBM device manager
    ///
    /// Opens the default render node (`/dev/dri/renderD*`)
    ///
    /// # Errors
    ///
    /// Returns an error if no render node is found or device creation fails
    pub fn new() -> Result<Self> {
        let (file, path) = Self::find_render_node()?;
        let device = GbmDevice::new(file, path)?;

        info!("Opened GBM device: {:?}", device.path());
        Ok(Self { device })
    }

    /// Find and open the default render node
    ///
    /// # Errors
    ///
    /// Returns an error if no render node is found or cannot be opened
    fn find_render_node() -> Result<(File, PathBuf)> {
        let dri_path = PathBuf::from("/dev/dri");

        if !dri_path.exists() {
            return Err(DisplayStreamError::Gbm(
                "/dev/dri directory not found".to_string(),
            ));
        }

        let entries = fs::read_dir(&dri_path).map_err(|e| {
            DisplayStreamError::Gbm(format!("Failed to read /dev/dri: {e}"))
        })?;

        // Find the first renderD* device
        for entry in entries {
            let entry = entry.map_err(|e| {
                DisplayStreamError::Gbm(format!("Failed to read directory entry: {e}"))
            })?;

            let path = entry.path();
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            if file_name.starts_with("renderD") {
                debug!("Found render node: {:?}", path);

                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .map_err(|e| {
                        DisplayStreamError::Gbm(format!(
                            "Failed to open {}: {}",
                            path.display(),
                            e
                        ))
                    })?;

                return Ok((file, path));
            }
        }

        Err(DisplayStreamError::Gbm(
            "No render node found in /dev/dri".to_string(),
        ))
    }

    /// Allocate a GBM buffer object
    ///
    /// # Arguments
    ///
    /// * `width` - Buffer width in pixels
    /// * `height` - Buffer height in pixels
    /// * `format` - GBM pixel format
    /// * `modifiers` - Array of format modifiers
    ///
    /// # Errors
    ///
    /// Returns an error if buffer allocation fails
    pub fn allocate_buffer(
        &self,
        width: u32,
        height: u32,
        format: gbm::Format,
        modifiers: &[u64],
    ) -> Result<gbm::BufferObject<()>> {
        debug!(
            "Allocating buffer: {}x{}, format: {:?}, modifiers: {:?}",
            width, height, format, modifiers
        );

        let bo = if modifiers.is_empty() {
            // Allocate without modifiers
            self.device.device().create_buffer_object::<()>(
                width,
                height,
                format,
                gbm::BufferObjectFlags::RENDERING | gbm::BufferObjectFlags::LINEAR,
            )
        } else {
            // Allocate with modifiers - convert u64 to Modifier
            self.device.device().create_buffer_object_with_modifiers2::<()>(
                width,
                height,
                format,
                modifiers.iter().map(|&m| gbm::Modifier::from(m)),
                gbm::BufferObjectFlags::RENDERING,
            )
        }
        .map_err(|e| {
            DisplayStreamError::Gbm(format!("Failed to allocate buffer: {e}"))
        })?;

        debug!("Buffer allocated successfully");
        Ok(bo)
    }

    /// Export a buffer object as DMA-BUF
    ///
    /// # Arguments
    ///
    /// * `bo` - The buffer object to export
    ///
    /// # Errors
    ///
    /// Returns an error if export fails or plane information cannot be retrieved
    pub fn export_dmabuf(&self, bo: &gbm::BufferObject<()>) -> Result<DmaBufInfo> {
        let width = bo.width();
        let height = bo.height();
        let format = bo.format();
        let modifier = bo.modifier();
        let num_planes = bo.plane_count();

        // Get the primary DMA-BUF fd (plane 0)
        let fd = bo.fd().map_err(|e| {
            DisplayStreamError::Gbm(format!("Failed to get DMA-BUF fd: {e}"))
        })?;

        let fd_raw = fd.as_raw_fd();

        // Collect stride and offset for each plane
        let mut strides = Vec::with_capacity(num_planes as usize);
        let mut offsets = Vec::with_capacity(num_planes as usize);

        for plane in 0..num_planes {
            let stride = bo.stride_for_plane(plane.try_into().unwrap_or(0));
            let offset = bo.offset(plane.try_into().unwrap_or(0));

            strides.push(stride);
            offsets.push(offset);
        }

        // Convert GBM format to DRM FourCC and Modifier to u64
        #[allow(clippy::as_conversions)]
        let fourcc = format as u32;
        let modifier_u64: u64 = modifier.into();

        debug!(
            "Exported DMA-BUF: fd={}, {}x{}, format={:?}, modifier=0x{:x}, planes={}",
            fd_raw, width, height, format, modifier_u64, num_planes
        );

        Ok(DmaBufInfo::new(
            fd_raw,
            width,
            height,
            fourcc,
            modifier_u64,
            num_planes,
            strides,
            offsets,
        ))
    }

    /// Test if a format and modifier combination is supported
    ///
    /// # Arguments
    ///
    /// * `format` - GBM pixel format
    /// * `modifier` - Format modifier
    ///
    /// # Errors
    ///
    /// Returns an error if the test allocation fails
    pub fn test_format(&self, format: gbm::Format, modifier: u64) -> Result<bool> {
        debug!("Testing format: {:?}, modifier: 0x{:x}", format, modifier);

        // Try to allocate a small test buffer
        let modifiers_vec: Vec<u64> = if modifier == 0 {
            Vec::new()
        } else {
            vec![modifier]
        };

        match self.allocate_buffer(64, 64, format, &modifiers_vec) {
            Ok(_bo) => {
                debug!("Format test succeeded");
                Ok(true)
            }
            Err(e) => {
                warn!("Format test failed: {}", e);
                Ok(false)
            }
        }
    }
}

/// Convert SPA video format to GBM format
///
/// # Arguments
///
/// * `spa_format` - SPA video format constant
///
/// # Returns
///
/// The corresponding GBM format, or `None` if unsupported
#[must_use]
pub fn spa_format_to_gbm(spa_format: u32) -> Option<gbm::Format> {
    match spa_format {
        4 => Some(gbm::Format::Abgr8888),  // SPA_VIDEO_FORMAT_RGBA
        15 => Some(gbm::Format::Argb8888), // SPA_VIDEO_FORMAT_BGRA
        5 => Some(gbm::Format::Xbgr8888),  // SPA_VIDEO_FORMAT_RGBx
        16 => Some(gbm::Format::Xrgb8888), // SPA_VIDEO_FORMAT_BGRx
        _ => None,
    }
}

/// Convert GBM format to SPA video format
///
/// # Arguments
///
/// * `gbm_format` - GBM format
///
/// # Returns
///
/// The corresponding SPA video format constant, or `None` if unsupported
#[must_use]
pub fn gbm_to_spa_format(gbm_format: gbm::Format) -> Option<u32> {
    match gbm_format {
        gbm::Format::Abgr8888 => Some(4),  // SPA_VIDEO_FORMAT_RGBA
        gbm::Format::Argb8888 => Some(15), // SPA_VIDEO_FORMAT_BGRA
        gbm::Format::Xbgr8888 => Some(5),  // SPA_VIDEO_FORMAT_RGBx
        gbm::Format::Xrgb8888 => Some(16), // SPA_VIDEO_FORMAT_BGRx
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spa_format_to_gbm_abgr() {
        let result = spa_format_to_gbm(4);
        assert_eq!(result, Some(gbm::Format::Abgr8888));
    }

    #[test]
    fn test_spa_format_to_gbm_argb() {
        let result = spa_format_to_gbm(15);
        assert_eq!(result, Some(gbm::Format::Argb8888));
    }

    #[test]
    fn test_spa_format_to_gbm_xbgr() {
        let result = spa_format_to_gbm(5);
        assert_eq!(result, Some(gbm::Format::Xbgr8888));
    }

    #[test]
    fn test_spa_format_to_gbm_xrgb() {
        let result = spa_format_to_gbm(16);
        assert_eq!(result, Some(gbm::Format::Xrgb8888));
    }

    #[test]
    fn test_gbm_to_spa_format_abgr() {
        let result = gbm_to_spa_format(gbm::Format::Abgr8888);
        assert_eq!(result, Some(4));
    }

    #[test]
    fn test_gbm_to_spa_format_argb() {
        let result = gbm_to_spa_format(gbm::Format::Argb8888);
        assert_eq!(result, Some(15));
    }

    #[test]
    fn test_gbm_to_spa_format_xbgr() {
        let result = gbm_to_spa_format(gbm::Format::Xbgr8888);
        assert_eq!(result, Some(5));
    }

    #[test]
    fn test_gbm_to_spa_format_xrgb() {
        let result = gbm_to_spa_format(gbm::Format::Xrgb8888);
        assert_eq!(result, Some(16));
    }

    #[test]
    fn test_spa_format_roundtrip() {
        let spa_formats = [4, 15, 5, 16];

        for &spa in &spa_formats {
            let gbm = spa_format_to_gbm(spa).expect("Should convert to GBM");
            let back = gbm_to_spa_format(gbm).expect("Should convert back to SPA");
            assert_eq!(spa, back, "Roundtrip failed for SPA format {}", spa);
        }
    }

    #[test]
    fn test_unknown_spa_format_returns_none() {
        assert_eq!(spa_format_to_gbm(999), None);
    }

    #[test]
    fn test_unknown_gbm_format_returns_none() {
        // Test with a format we don't support
        assert_eq!(gbm_to_spa_format(gbm::Format::Rgb565), None);
    }

    #[test]
    fn test_dmabuf_info_creation() {
        let info = DmaBufInfo::new(
            42,                     // fd
            1920,                   // width
            1080,                   // height
            0x34325241,            // format (AR24)
            0x0010_0000_0000_0001, // modifier
            1,                      // num_planes
            vec![7680],            // strides
            vec![0],               // offsets
        );

        assert_eq!(info.fd, 42);
        assert_eq!(info.width, 1920);
        assert_eq!(info.height, 1080);
        assert_eq!(info.format, 0x34325241);
        assert_eq!(info.modifier, 0x0010_0000_0000_0001);
        assert_eq!(info.num_planes, 1);
        assert_eq!(info.strides, vec![7680]);
        assert_eq!(info.offsets, vec![0]);
    }
}
