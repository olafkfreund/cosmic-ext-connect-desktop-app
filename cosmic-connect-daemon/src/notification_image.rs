//! Notification Image Processing
//!
//! Handles image processing for notification images sent to Android devices.
//! This module provides functionality to resize images, convert them to PNG format,
//! and load images from file paths.
//!
//! ## Purpose
//!
//! Desktop notifications can include images from two sources:
//! - Raw image data in the `image-data` hint (RGBA/RGB pixel data)
//! - File paths in the `image-path` hint
//!
//! For efficient transmission to Android devices over the network, images need to be:
//! - Resized to a reasonable dimension (max 256x256) to reduce bandwidth
//! - Converted to PNG format for compatibility and compression
//!
//! ## Example
//!
//! ```rust,ignore
//! use cosmic_connect_daemon::notification_image::NotificationImage;
//! use cosmic_connect_daemon::notification_listener::ImageData;
//!
//! // Process raw image data from notification hint
//! let image_data = ImageData {
//!     width: 512,
//!     height: 512,
//!     rowstride: 2048,
//!     has_alpha: true,
//!     bits_per_sample: 8,
//!     channels: 4,
//!     data: vec![...],
//! };
//!
//! let processed = NotificationImage::from_image_data(&image_data)?;
//! let png_bytes = processed.to_png()?;
//!
//! // Or load from a file path
//! let processed = NotificationImage::from_path("/path/to/icon.png")?;
//! let png_bytes = processed.to_png()?;
//! ```

use anyhow::{Context, Result};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
use std::io::Cursor;
use std::path::Path;
use tracing::{debug, trace, warn};

use crate::notification_listener::ImageData;

/// Maximum dimension (width or height) for notification images
///
/// Images larger than this will be resized proportionally to fit within
/// this dimension while maintaining aspect ratio. This balances image
/// quality with network transmission efficiency.
pub const MAX_IMAGE_DIMENSION: u32 = 256;

/// Processed notification image ready for transmission
///
/// This struct holds a processed image that has been resized and is ready
/// to be converted to PNG format for sending to Android devices.
#[derive(Debug, Clone)]
pub struct NotificationImage {
    /// Processed image data
    image: DynamicImage,
}

impl NotificationImage {
    /// Create a NotificationImage from raw image data
    ///
    /// Converts the raw RGBA/RGB pixel data from a notification hint into
    /// a processable image format, then resizes it if necessary.
    ///
    /// # Arguments
    ///
    /// * `image_data` - Raw image data from notification `image-data` hint
    ///
    /// # Returns
    ///
    /// A `NotificationImage` ready for PNG conversion, or an error if the
    /// image data is invalid or cannot be processed.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let notification_image = NotificationImage::from_image_data(&image_data)?;
    /// ```
    pub fn from_image_data(image_data: &ImageData) -> Result<Self> {
        debug!(
            "Processing notification image: {}x{}, channels={}, has_alpha={}",
            image_data.width, image_data.height, image_data.channels, image_data.has_alpha
        );

        // Validate dimensions
        if image_data.width <= 0 || image_data.height <= 0 {
            return Err(anyhow::anyhow!(
                "Invalid image dimensions: {}x{}",
                image_data.width,
                image_data.height
            ));
        }

        let width = image_data.width as u32;
        let height = image_data.height as u32;

        // Convert raw bytes to image buffer
        let image = if image_data.has_alpha && image_data.channels == 4 {
            Self::from_rgba_data(image_data, width, height)?
        } else if !image_data.has_alpha && image_data.channels == 3 {
            Self::from_rgb_data(image_data, width, height)?
        } else {
            return Err(anyhow::anyhow!(
                "Unsupported image format: channels={}, has_alpha={}",
                image_data.channels,
                image_data.has_alpha
            ));
        };

        // Resize if necessary
        let resized = Self::resize_if_needed(image, MAX_IMAGE_DIMENSION);

        Ok(Self { image: resized })
    }

    /// Create a NotificationImage from a file path
    ///
    /// Loads an image from the filesystem (typically from the `image-path` hint)
    /// and resizes it if necessary.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the image file (supports PNG, JPEG, and other common formats)
    ///
    /// # Returns
    ///
    /// A `NotificationImage` ready for PNG conversion, or an error if the
    /// file cannot be read or is not a valid image.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let notification_image = NotificationImage::from_path("/usr/share/icons/app-icon.png")?;
    /// ```
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        debug!("Loading notification image from path: {:?}", path);

        let image = image::open(path)
            .with_context(|| format!("Failed to load image from path: {:?}", path))?;

        trace!("Loaded image: {}x{}", image.width(), image.height());

        // Resize if necessary
        let resized = Self::resize_if_needed(image, MAX_IMAGE_DIMENSION);

        Ok(Self { image: resized })
    }

    /// Convert the image to PNG format
    ///
    /// Encodes the processed image as PNG bytes suitable for transmission
    /// to Android devices.
    ///
    /// # Returns
    ///
    /// A `Vec<u8>` containing the PNG-encoded image data.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let png_bytes = notification_image.to_png()?;
    /// // Send png_bytes to Android device
    /// ```
    pub fn to_png(&self) -> Result<Vec<u8>> {
        let mut buffer = Cursor::new(Vec::new());

        self.image
            .write_to(&mut buffer, ImageFormat::Png)
            .context("Failed to encode image as PNG")?;

        let png_bytes = buffer.into_inner();
        debug!("Encoded image as PNG: {} bytes", png_bytes.len());

        Ok(png_bytes)
    }

    /// Get the image dimensions
    ///
    /// Returns the (width, height) of the processed image.
    #[allow(dead_code)]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.image.width(), self.image.height())
    }

    /// Convert RGBA image data to DynamicImage
    fn from_rgba_data(image_data: &ImageData, width: u32, height: u32) -> Result<DynamicImage> {
        trace!("Converting RGBA image data");

        // Calculate expected data size
        let expected_size = (width * height * 4) as usize;

        if image_data.data.len() < expected_size {
            warn!(
                "Image data size mismatch: expected at least {}, got {}",
                expected_size,
                image_data.data.len()
            );
        }

        // Handle rowstride (padding at end of each row)
        let rowstride = image_data.rowstride as usize;
        let row_bytes = (width * 4) as usize;

        let mut pixels = Vec::with_capacity(expected_size);

        for y in 0..height as usize {
            let row_start = y * rowstride;
            let row_end = row_start + row_bytes;

            if row_end > image_data.data.len() {
                return Err(anyhow::anyhow!(
                    "Image data truncated at row {}: need {} bytes, have {}",
                    y,
                    row_end,
                    image_data.data.len()
                ));
            }

            pixels.extend_from_slice(&image_data.data[row_start..row_end]);
        }

        let image_buffer = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, pixels)
            .ok_or_else(|| anyhow::anyhow!("Failed to create RGBA image buffer"))?;

        Ok(DynamicImage::ImageRgba8(image_buffer))
    }

    /// Convert RGB image data to DynamicImage
    fn from_rgb_data(image_data: &ImageData, width: u32, height: u32) -> Result<DynamicImage> {
        trace!("Converting RGB image data to RGBA");

        // RGB data needs to be converted to RGBA
        let rowstride = image_data.rowstride as usize;
        let row_bytes = (width * 3) as usize;

        let mut rgba_pixels = Vec::with_capacity((width * height * 4) as usize);

        for y in 0..height as usize {
            let row_start = y * rowstride;
            let row_end = row_start + row_bytes;

            if row_end > image_data.data.len() {
                return Err(anyhow::anyhow!(
                    "Image data truncated at row {}: need {} bytes, have {}",
                    y,
                    row_end,
                    image_data.data.len()
                ));
            }

            // Convert RGB to RGBA by adding alpha channel
            for x in 0..width as usize {
                let pixel_start = row_start + (x * 3);
                rgba_pixels.push(image_data.data[pixel_start]); // R
                rgba_pixels.push(image_data.data[pixel_start + 1]); // G
                rgba_pixels.push(image_data.data[pixel_start + 2]); // B
                rgba_pixels.push(255); // A (fully opaque)
            }
        }

        let image_buffer = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, rgba_pixels)
            .ok_or_else(|| {
            anyhow::anyhow!("Failed to create RGBA image buffer from RGB data")
        })?;

        Ok(DynamicImage::ImageRgba8(image_buffer))
    }

    /// Resize image if it exceeds maximum dimension
    ///
    /// Resizes the image proportionally to fit within max_dimension while
    /// maintaining aspect ratio. If the image is already smaller, returns
    /// it unchanged.
    fn resize_if_needed(image: DynamicImage, max_dimension: u32) -> DynamicImage {
        let (width, height) = (image.width(), image.height());

        if width <= max_dimension && height <= max_dimension {
            trace!("Image within size limits: {}x{}", width, height);
            return image;
        }

        // Calculate new dimensions maintaining aspect ratio
        let (new_width, new_height) = if width > height {
            let ratio = max_dimension as f32 / width as f32;
            (max_dimension, (height as f32 * ratio) as u32)
        } else {
            let ratio = max_dimension as f32 / height as f32;
            ((width as f32 * ratio) as u32, max_dimension)
        };

        debug!(
            "Resizing image from {}x{} to {}x{}",
            width, height, new_width, new_height
        );

        image.resize(new_width, new_height, image::imageops::FilterType::Lanczos3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resize_if_needed_no_resize() {
        // Create a small test image
        let image =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(100, 100, Rgba([255, 0, 0, 255])));
        let resized = NotificationImage::resize_if_needed(image.clone(), 256);

        assert_eq!(resized.width(), 100);
        assert_eq!(resized.height(), 100);
    }

    #[test]
    fn test_resize_if_needed_width_larger() {
        // Create an image wider than max dimension
        let image =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(512, 256, Rgba([255, 0, 0, 255])));
        let resized = NotificationImage::resize_if_needed(image, 256);

        assert_eq!(resized.width(), 256);
        assert_eq!(resized.height(), 128);
    }

    #[test]
    fn test_resize_if_needed_height_larger() {
        // Create an image taller than max dimension
        let image =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(128, 512, Rgba([255, 0, 0, 255])));
        let resized = NotificationImage::resize_if_needed(image, 256);

        assert_eq!(resized.width(), 64);
        assert_eq!(resized.height(), 256);
    }

    #[test]
    fn test_from_rgba_data() {
        // Create test RGBA data (2x2 red image)
        let width = 2;
        let height = 2;
        let data = vec![
            255, 0, 0, 255, 255, 0, 0, 255, // Row 1
            255, 0, 0, 255, 255, 0, 0, 255, // Row 2
        ];

        let image_data = ImageData {
            width: width as i32,
            height: height as i32,
            rowstride: (width * 4) as i32,
            has_alpha: true,
            bits_per_sample: 8,
            channels: 4,
            data,
        };

        let result = NotificationImage::from_image_data(&image_data);
        assert!(result.is_ok());

        let img = result.unwrap();
        assert_eq!(img.dimensions(), (2, 2));
    }

    #[test]
    fn test_from_rgb_data() {
        // Create test RGB data (2x2 green image)
        let width = 2;
        let height = 2;
        let data = vec![
            0, 255, 0, 0, 255, 0, // Row 1
            0, 255, 0, 0, 255, 0, // Row 2
        ];

        let image_data = ImageData {
            width: width as i32,
            height: height as i32,
            rowstride: (width * 3) as i32,
            has_alpha: false,
            bits_per_sample: 8,
            channels: 3,
            data,
        };

        let result = NotificationImage::from_image_data(&image_data);
        assert!(result.is_ok());

        let img = result.unwrap();
        assert_eq!(img.dimensions(), (2, 2));
    }

    #[test]
    fn test_from_rgba_data_with_rowstride_padding() {
        // Create test RGBA data with padding (2x2 image, rowstride = 12 bytes per row instead of 8)
        let width = 2;
        let height = 2;
        let data = vec![
            255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 0, 0, // Row 1 + padding
            255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 0, 0, // Row 2 + padding
        ];

        let image_data = ImageData {
            width: width as i32,
            height: height as i32,
            rowstride: 12,
            has_alpha: true,
            bits_per_sample: 8,
            channels: 4,
            data,
        };

        let result = NotificationImage::from_image_data(&image_data);
        assert!(result.is_ok());

        let img = result.unwrap();
        assert_eq!(img.dimensions(), (2, 2));
    }

    #[test]
    fn test_to_png() {
        // Create a small test image
        let image =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(10, 10, Rgba([255, 0, 0, 255])));
        let notification_image = NotificationImage { image };

        let png_result = notification_image.to_png();
        assert!(png_result.is_ok());

        let png_bytes = png_result.unwrap();
        assert!(!png_bytes.is_empty());

        // Check PNG magic bytes
        assert_eq!(&png_bytes[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    }

    #[test]
    fn test_invalid_dimensions() {
        let image_data = ImageData {
            width: -1,
            height: 100,
            rowstride: 400,
            has_alpha: true,
            bits_per_sample: 8,
            channels: 4,
            data: vec![0; 40000],
        };

        let result = NotificationImage::from_image_data(&image_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_unsupported_format() {
        let image_data = ImageData {
            width: 10,
            height: 10,
            rowstride: 20,
            has_alpha: false,
            bits_per_sample: 8,
            channels: 2, // Invalid: 2 channels
            data: vec![0; 200],
        };

        let result = NotificationImage::from_image_data(&image_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_path_nonexistent() {
        let result = NotificationImage::from_path("/nonexistent/path/to/image.png");
        assert!(result.is_err());
    }
}
