//! Frame Types for Screen Capture
//!
//! Defines frame representations for the screen capture pipeline.

use std::time::Instant;

/// Raw uncompressed frame from screen capture
#[derive(Debug, Clone)]
pub struct RawFrame {
    /// Frame width in pixels
    pub width: u32,

    /// Frame height in pixels
    pub height: u32,

    /// Pixel format (RGBA, BGRA, etc.)
    pub format: PixelFormat,

    /// Raw pixel data
    pub data: Vec<u8>,

    /// Timestamp when frame was captured
    pub timestamp: Instant,

    /// Stride (bytes per row)
    pub stride: u32,
}

impl RawFrame {
    /// Create a new raw frame
    pub fn new(width: u32, height: u32, format: PixelFormat, data: Vec<u8>) -> Self {
        let stride = width * format.bytes_per_pixel();
        Self {
            width,
            height,
            format,
            data,
            timestamp: Instant::now(),
            stride,
        }
    }

    /// Get frame size in bytes
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Convert to image buffer for saving/encoding
    #[cfg(feature = "remotedesktop")]
    pub fn to_image_buffer(&self) -> Option<image::RgbaImage> {
        use image::{ImageBuffer, Rgba};

        // Convert to RGBA format
        let rgba_data = match self.format {
            PixelFormat::RGBA => self.data.clone(),
            PixelFormat::BGRA => {
                // Convert BGRA to RGBA
                self.data
                    .chunks_exact(4)
                    .flat_map(|pixel| [pixel[2], pixel[1], pixel[0], pixel[3]])
                    .collect()
            }
            PixelFormat::RGB => {
                // Add alpha channel
                self.data
                    .chunks_exact(3)
                    .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
                    .collect()
            }
            PixelFormat::BGR => {
                // Convert BGR to RGBA and add alpha
                self.data
                    .chunks_exact(3)
                    .flat_map(|pixel| [pixel[2], pixel[1], pixel[0], 255])
                    .collect()
            }
        };

        ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(self.width, self.height, rgba_data)
    }
}

/// Pixel format for frame data
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// RGBA 8-bit per channel
    RGBA,
    /// BGRA 8-bit per channel
    BGRA,
    /// RGB 8-bit per channel (no alpha)
    RGB,
    /// BGR 8-bit per channel (no alpha)
    BGR,
}

impl PixelFormat {
    /// Get bytes per pixel for this format
    pub fn bytes_per_pixel(&self) -> u32 {
        match self {
            PixelFormat::RGBA | PixelFormat::BGRA => 4,
            PixelFormat::RGB | PixelFormat::BGR => 3,
        }
    }

    /// Get format name as string
    pub fn as_str(&self) -> &'static str {
        match self {
            PixelFormat::RGBA => "RGBA",
            PixelFormat::BGRA => "BGRA",
            PixelFormat::RGB => "RGB",
            PixelFormat::BGR => "BGR",
        }
    }
}

/// Video encoding type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingType {
    /// Raw uncompressed
    Raw,
    /// H.264 compressed
    H264,
    /// LZ4 lossless compression
    LZ4,
    /// Hextile encoding (VNC standard)
    Hextile,
}

/// Encoded frame ready for transmission
#[derive(Debug, Clone)]
pub struct EncodedFrame {
    /// Original frame width
    pub width: u32,

    /// Original frame height
    pub height: u32,

    /// Encoding type used
    pub encoding: EncodingType,

    /// Encoded data
    pub data: Vec<u8>,

    /// Timestamp when frame was captured
    pub timestamp: Instant,

    /// Compression ratio (if applicable)
    pub compression_ratio: Option<f32>,
}

impl EncodedFrame {
    /// Create a new encoded frame
    pub fn new(
        width: u32,
        height: u32,
        encoding: EncodingType,
        data: Vec<u8>,
        timestamp: Instant,
    ) -> Self {
        Self {
            width,
            height,
            encoding,
            data,
            timestamp,
            compression_ratio: None,
        }
    }

    /// Set compression ratio
    pub fn with_compression_ratio(mut self, original_size: usize) -> Self {
        if !self.data.is_empty() && original_size > 0 {
            self.compression_ratio = Some(original_size as f32 / self.data.len() as f32);
        }
        self
    }

    /// Get encoded size in bytes
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

/// Quality preset for encoding
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QualityPreset {
    /// Low quality, high compression (bandwidth constrained)
    Low,
    /// Medium quality, balanced
    #[default]
    Medium,
    /// High quality, low compression
    High,
}

impl QualityPreset {
    /// Get target bitrate for this preset (bits per second)
    pub fn target_bitrate(&self, width: u32, height: u32, fps: u32) -> u32 {
        let pixels = width * height;
        match self {
            QualityPreset::Low => pixels * fps / 4, // ~0.25 bits per pixel
            QualityPreset::Medium => pixels * fps / 2, // ~0.5 bits per pixel
            QualityPreset::High => pixels * fps,    // ~1 bit per pixel
        }
    }

    /// From string representation
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "low" => Some(QualityPreset::Low),
            "medium" => Some(QualityPreset::Medium),
            "high" => Some(QualityPreset::High),
            _ => None,
        }
    }

    /// To string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            QualityPreset::Low => "low",
            QualityPreset::Medium => "medium",
            QualityPreset::High => "high",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pixel_format_bytes() {
        assert_eq!(PixelFormat::RGBA.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::BGRA.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::RGB.bytes_per_pixel(), 3);
        assert_eq!(PixelFormat::BGR.bytes_per_pixel(), 3);
    }

    #[test]
    fn test_quality_preset_bitrate() {
        let width = 1920;
        let height = 1080;
        let fps = 30;

        let low = QualityPreset::Low.target_bitrate(width, height, fps);
        let medium = QualityPreset::Medium.target_bitrate(width, height, fps);
        let high = QualityPreset::High.target_bitrate(width, height, fps);

        assert!(low < medium);
        assert!(medium < high);
    }

    #[test]
    fn test_quality_preset_from_str() {
        assert_eq!(QualityPreset::from_str("low"), Some(QualityPreset::Low));
        assert_eq!(QualityPreset::from_str("LOW"), Some(QualityPreset::Low));
        assert_eq!(
            QualityPreset::from_str("medium"),
            Some(QualityPreset::Medium)
        );
        assert_eq!(QualityPreset::from_str("high"), Some(QualityPreset::High));
        assert_eq!(QualityPreset::from_str("invalid"), None);
    }

    #[test]
    fn test_raw_frame_creation() {
        let width = 640;
        let height = 480;
        let format = PixelFormat::RGBA;
        let data = vec![0u8; (width * height * 4) as usize];

        let frame = RawFrame::new(width, height, format, data);

        assert_eq!(frame.width, width);
        assert_eq!(frame.height, height);
        assert_eq!(frame.format, format);
        assert_eq!(frame.stride, width * 4);
    }

    #[test]
    fn test_encoded_frame_compression_ratio() {
        let frame = EncodedFrame::new(
            1920,
            1080,
            EncodingType::H264,
            vec![0u8; 100000],
            Instant::now(),
        )
        .with_compression_ratio(1920 * 1080 * 4); // Original RGBA size

        assert!(frame.compression_ratio.is_some());
        let ratio = frame.compression_ratio.unwrap();
        assert!(ratio > 1.0); // Should be compressed
    }
}
