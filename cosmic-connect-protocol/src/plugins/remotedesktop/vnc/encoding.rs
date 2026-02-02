//! Frame Encoding for VNC
//!
//! Implements multiple encoding types for VNC framebuffer updates:
//! - Raw: Uncompressed pixel data
//! - LZ4: Fast lossless compression
//! - H.264: Video compression (TODO: Phase 3)
//! - Hextile: VNC standard tile-based encoding (TODO: Phase 4)

use crate::plugins::remotedesktop::capture::{EncodedFrame, EncodingType, QualityPreset, RawFrame};
use crate::Result;
use std::time::Instant;
use tracing::{debug, info};

/// YUV420 buffer wrapper for H.264 encoding
#[cfg(feature = "remotedesktop")]
struct Yuv420Buffer {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

#[cfg(feature = "remotedesktop")]
impl openh264::formats::YUVSource for Yuv420Buffer {
    fn dimensions(&self) -> (usize, usize) {
        (self.width as usize, self.height as usize)
    }

    fn strides(&self) -> (usize, usize, usize) {
        let y_stride = self.width as usize;
        let uv_stride = (self.width / 2) as usize;
        (y_stride, uv_stride, uv_stride)
    }

    fn y(&self) -> &[u8] {
        let y_size = (self.width * self.height) as usize;
        &self.data[0..y_size]
    }

    fn u(&self) -> &[u8] {
        let y_size = (self.width * self.height) as usize;
        let uv_size = (self.width * self.height / 4) as usize;
        &self.data[y_size..y_size + uv_size]
    }

    fn v(&self) -> &[u8] {
        let y_size = (self.width * self.height) as usize;
        let uv_size = (self.width * self.height / 4) as usize;
        &self.data[y_size + uv_size..y_size + 2 * uv_size]
    }
}

/// Frame encoder with support for multiple encoding types
#[cfg(feature = "remotedesktop")]
pub struct FrameEncoder {
    /// Current quality preset
    quality: QualityPreset,

    /// Preferred encoding type
    preferred_encoding: EncodingType,

    /// Statistics
    stats: EncoderStats,
}

#[cfg(feature = "remotedesktop")]
impl FrameEncoder {
    /// Create a new frame encoder
    pub fn new(quality: QualityPreset) -> Self {
        let preferred_encoding = match quality {
            QualityPreset::Low => EncodingType::H264,
            QualityPreset::Medium => EncodingType::LZ4,
            QualityPreset::High => EncodingType::Raw,
        };

        info!("Creating frame encoder with {:?} quality", quality);

        Self {
            quality,
            preferred_encoding,
            stats: EncoderStats::default(),
        }
    }

    /// Encode a raw frame
    pub fn encode(&mut self, frame: &RawFrame) -> Result<EncodedFrame> {
        let start = Instant::now();

        let encoded = match self.preferred_encoding {
            EncodingType::Raw => self.encode_raw(frame)?,
            EncodingType::LZ4 => self.encode_lz4(frame)?,
            EncodingType::H264 => self.encode_h264(frame)?,
            EncodingType::Hextile => self.encode_hextile(frame)?,
        };

        let elapsed = start.elapsed();
        self.stats.frames_encoded += 1;
        self.stats.total_encode_time += elapsed;
        self.stats.total_input_bytes += frame.size();
        self.stats.total_output_bytes += encoded.size();

        debug!(
            "Encoded frame {} ({}x{}) in {:?}: {} -> {} bytes ({:.1}x compression)",
            self.stats.frames_encoded,
            frame.width,
            frame.height,
            elapsed,
            frame.size(),
            encoded.size(),
            frame.size() as f32 / encoded.size() as f32
        );

        Ok(encoded)
    }

    /// Encode as raw (uncompressed) pixels
    fn encode_raw(&self, frame: &RawFrame) -> Result<EncodedFrame> {
        // Raw encoding: just copy the pixel data
        let encoded = EncodedFrame::new(
            frame.width,
            frame.height,
            EncodingType::Raw,
            frame.data.clone(),
            frame.timestamp,
        );

        Ok(encoded.with_compression_ratio(frame.size()))
    }

    /// Encode with LZ4 compression
    #[cfg(feature = "remotedesktop")]
    fn encode_lz4(&self, frame: &RawFrame) -> Result<EncodedFrame> {
        use lz4::block::compress;

        // LZ4 fast compression
        let compressed = compress(&frame.data, None, true)
            .map_err(|e| crate::ProtocolError::Plugin(format!("LZ4 compression failed: {}", e)))?;

        let encoded = EncodedFrame::new(
            frame.width,
            frame.height,
            EncodingType::LZ4,
            compressed,
            frame.timestamp,
        )
        .with_compression_ratio(frame.size());

        Ok(encoded)
    }

    #[cfg(not(feature = "remotedesktop"))]
    fn encode_lz4(&self, frame: &RawFrame) -> Result<EncodedFrame> {
        Err(crate::ProtocolError::unsupported_feature(
            "LZ4 encoding requires remotedesktop feature",
        ))
    }

    /// Encode with H.264 video compression
    #[cfg(feature = "remotedesktop")]
    fn encode_h264(&self, frame: &RawFrame) -> Result<EncodedFrame> {
        use openh264::encoder::{Encoder as H264EncoderImpl, EncoderConfig};

        // Convert frame to YUV420 format required by H.264
        let yuv_buffer = self.rgba_to_yuv420(frame)?;

        // Configure encoder based on quality preset
        let bitrate = self.quality.target_bitrate(frame.width, frame.height, 30);

        let config = EncoderConfig::new()
            .set_bitrate_bps(bitrate)
            .max_frame_rate(30.0)
            .enable_skip_frame(true);

        // Create encoder with API and config
        let api = openh264::OpenH264API::from_source();
        let mut encoder = H264EncoderImpl::with_api_config(api, config).map_err(|e| {
            crate::ProtocolError::Plugin(format!("H.264 encoder creation failed: {}", e))
        })?;

        // Create YUV source wrapper
        let yuv_source = Yuv420Buffer {
            width: frame.width,
            height: frame.height,
            data: yuv_buffer,
        };

        // Encode frame
        let bitstream = encoder
            .encode(&yuv_source)
            .map_err(|e| crate::ProtocolError::Plugin(format!("H.264 encoding failed: {}", e)))?;

        // Collect encoded data from bitstream
        let mut encoded_data = Vec::new();
        bitstream.write_vec(&mut encoded_data);

        let encoded = EncodedFrame::new(
            frame.width,
            frame.height,
            EncodingType::H264,
            encoded_data,
            frame.timestamp,
        )
        .with_compression_ratio(frame.size());

        Ok(encoded)
    }

    #[cfg(not(feature = "remotedesktop"))]
    fn encode_h264(&self, _frame: &RawFrame) -> Result<EncodedFrame> {
        Err(crate::ProtocolError::unsupported_feature(
            "H.264 encoding requires remotedesktop feature",
        ))
    }

    /// Convert RGBA to YUV420 format for H.264 encoding
    #[cfg(feature = "remotedesktop")]
    fn rgba_to_yuv420(&self, frame: &RawFrame) -> Result<Vec<u8>> {
        let width = frame.width as usize;
        let height = frame.height as usize;
        let rgba = &frame.data;

        // YUV420 has Y plane (full resolution) + U/V planes (half resolution)
        let y_size = width * height;
        let uv_size = (width / 2) * (height / 2);
        let total_size = y_size + 2 * uv_size;

        let mut yuv = vec![0u8; total_size];

        // Convert RGBA to YUV using ITU-R BT.601 coefficients
        for y in 0..height {
            for x in 0..width {
                let rgba_idx = (y * width + x) * 4;

                // Handle potential out of bounds
                if rgba_idx + 2 >= rgba.len() {
                    continue;
                }

                let r = rgba[rgba_idx] as f32;
                let g = rgba[rgba_idx + 1] as f32;
                let b = rgba[rgba_idx + 2] as f32;

                // Y component (luminance)
                let y_val = (0.299 * r + 0.587 * g + 0.114 * b) as u8;
                yuv[y * width + x] = y_val;

                // U and V components (chrominance) - subsample to 4:2:0
                if x % 2 == 0 && y % 2 == 0 {
                    let u_val = (128.0 - 0.168736 * r - 0.331264 * g + 0.5 * b) as u8;
                    let v_val = (128.0 + 0.5 * r - 0.418688 * g - 0.081312 * b) as u8;

                    let uv_x = x / 2;
                    let uv_y = y / 2;
                    let uv_width = width / 2;

                    yuv[y_size + uv_y * uv_width + uv_x] = u_val;
                    yuv[y_size + uv_size + uv_y * uv_width + uv_x] = v_val;
                }
            }
        }

        Ok(yuv)
    }

    /// Encode with Hextile (VNC standard)
    fn encode_hextile(&self, frame: &RawFrame) -> Result<EncodedFrame> {
        // Hextile encoding per RFC 6143 Section 7.7.4
        // Divides framebuffer into 16x16 tiles and encodes each tile
        const TILE_SIZE: u32 = 16;

        let mut encoded_data = Vec::new();

        // Process tiles row by row
        for tile_y in (0..frame.height).step_by(TILE_SIZE as usize) {
            for tile_x in (0..frame.width).step_by(TILE_SIZE as usize) {
                let tile_width = TILE_SIZE.min(frame.width - tile_x);
                let tile_height = TILE_SIZE.min(frame.height - tile_y);

                self.encode_hextile_tile(
                    frame,
                    tile_x,
                    tile_y,
                    tile_width,
                    tile_height,
                    &mut encoded_data,
                )?;
            }
        }

        let encoded = EncodedFrame::new(
            frame.width,
            frame.height,
            EncodingType::Hextile,
            encoded_data,
            frame.timestamp,
        )
        .with_compression_ratio(frame.size());

        Ok(encoded)
    }

    /// Encode a single Hextile tile
    fn encode_hextile_tile(
        &self,
        frame: &RawFrame,
        tile_x: u32,
        tile_y: u32,
        tile_width: u32,
        tile_height: u32,
        output: &mut Vec<u8>,
    ) -> Result<()> {
        // Hextile encoding modes (RFC 6143):
        // Bit 0: Raw - tile data is raw pixels
        // Bit 1: BackgroundSpecified - background color follows subencoding byte
        // Bit 2: ForegroundSpecified - foreground color follows background
        // Bit 3: AnySubrects - subrectangles follow
        // Bit 4: SubrectsColoured - each subrect has its own color

        const RAW: u8 = 1 << 0;
        const BACKGROUND_SPECIFIED: u8 = 1 << 1;

        // Extract tile pixels
        let mut tile_pixels = Vec::new();
        let bytes_per_pixel = frame.format.bytes_per_pixel() as usize;

        for y in 0..tile_height {
            let row_start = ((tile_y + y) * frame.width + tile_x) as usize * bytes_per_pixel;
            let row_end = row_start + tile_width as usize * bytes_per_pixel;

            if row_end <= frame.data.len() {
                tile_pixels.extend_from_slice(&frame.data[row_start..row_end]);
            }
        }

        // Analyze tile for compression opportunities
        let (is_uniform, background_color) = self.analyze_tile(&tile_pixels, bytes_per_pixel);

        if is_uniform {
            // Tile is solid color - use background only
            output.push(BACKGROUND_SPECIFIED);
            output.extend_from_slice(&background_color);
        } else {
            // Tile has variations - use raw encoding for simplicity
            // Advanced implementation could detect subrectangles
            output.push(RAW);
            output.extend_from_slice(&tile_pixels);
        }

        Ok(())
    }

    /// Analyze tile to determine if it's uniform color
    fn analyze_tile(&self, pixels: &[u8], bytes_per_pixel: usize) -> (bool, Vec<u8>) {
        if pixels.len() < bytes_per_pixel {
            return (false, Vec::new());
        }

        let first_pixel = &pixels[0..bytes_per_pixel];

        // Check if all pixels match the first pixel
        let is_uniform = pixels
            .chunks_exact(bytes_per_pixel)
            .all(|pixel| pixel == first_pixel);

        (is_uniform, first_pixel.to_vec())
    }

    /// Get encoder statistics
    pub fn stats(&self) -> &EncoderStats {
        &self.stats
    }

    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.stats = EncoderStats::default();
    }

    /// Change quality preset
    pub fn set_quality(&mut self, quality: QualityPreset) {
        info!(
            "Changing encoder quality from {:?} to {:?}",
            self.quality, quality
        );
        self.quality = quality;

        // Update preferred encoding
        self.preferred_encoding = match quality {
            QualityPreset::Low => EncodingType::H264,
            QualityPreset::Medium => EncodingType::LZ4,
            QualityPreset::High => EncodingType::Raw,
        };
    }

    /// Change encoding type
    pub fn set_encoding(&mut self, encoding: EncodingType) {
        info!("Changing encoder type to {:?}", encoding);
        self.preferred_encoding = encoding;
    }
}

/// Encoder statistics
#[derive(Debug, Clone, Default)]
pub struct EncoderStats {
    /// Total frames encoded
    pub frames_encoded: u64,

    /// Total time spent encoding
    pub total_encode_time: std::time::Duration,

    /// Total input bytes (raw frames)
    pub total_input_bytes: usize,

    /// Total output bytes (encoded frames)
    pub total_output_bytes: usize,
}

impl EncoderStats {
    /// Get average encoding time per frame
    pub fn avg_encode_time(&self) -> std::time::Duration {
        if self.frames_encoded == 0 {
            std::time::Duration::ZERO
        } else {
            self.total_encode_time / self.frames_encoded as u32
        }
    }

    /// Get average compression ratio
    pub fn avg_compression_ratio(&self) -> f32 {
        if self.total_output_bytes == 0 {
            1.0
        } else {
            self.total_input_bytes as f32 / self.total_output_bytes as f32
        }
    }

    /// Get encoding throughput (bytes per second)
    pub fn throughput_bps(&self) -> f64 {
        let secs = self.total_encode_time.as_secs_f64();
        if secs == 0.0 {
            0.0
        } else {
            self.total_input_bytes as f64 / secs
        }
    }

    /// Get encoding FPS
    pub fn fps(&self) -> f64 {
        let secs = self.total_encode_time.as_secs_f64();
        if secs == 0.0 {
            0.0
        } else {
            self.frames_encoded as f64 / secs
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::remotedesktop::capture::PixelFormat;

    fn create_test_frame() -> RawFrame {
        let width = 640;
        let height = 480;
        let data = vec![128u8; (width * height * 4) as usize];
        RawFrame::new(width, height, PixelFormat::RGBA, data)
    }

    #[test]
    #[cfg(feature = "remotedesktop")]
    fn test_encoder_creation() {
        let encoder = FrameEncoder::new(QualityPreset::Medium);
        assert_eq!(encoder.quality, QualityPreset::Medium);
        assert_eq!(encoder.preferred_encoding, EncodingType::LZ4);
    }

    #[test]
    #[cfg(feature = "remotedesktop")]
    fn test_raw_encoding() {
        let mut encoder = FrameEncoder::new(QualityPreset::High);
        encoder.set_encoding(EncodingType::Raw);

        let frame = create_test_frame();
        let encoded = encoder.encode(&frame).unwrap();

        assert_eq!(encoded.encoding, EncodingType::Raw);
        assert_eq!(encoded.width, frame.width);
        assert_eq!(encoded.height, frame.height);
        assert_eq!(encoded.data.len(), frame.data.len());
    }

    #[test]
    #[cfg(feature = "remotedesktop")]
    fn test_lz4_encoding() {
        let mut encoder = FrameEncoder::new(QualityPreset::Medium);
        encoder.set_encoding(EncodingType::LZ4);

        let frame = create_test_frame();
        let original_size = frame.size();
        let encoded = encoder.encode(&frame).unwrap();

        assert_eq!(encoded.encoding, EncodingType::LZ4);
        assert_eq!(encoded.width, frame.width);
        assert_eq!(encoded.height, frame.height);
        // LZ4 should compress repeated bytes significantly
        assert!(encoded.data.len() < original_size);

        println!(
            "LZ4 compression: {} -> {} bytes ({:.1}x)",
            original_size,
            encoded.data.len(),
            original_size as f32 / encoded.data.len() as f32
        );
    }

    #[test]
    #[cfg(feature = "remotedesktop")]
    fn test_encoder_stats() {
        let mut encoder = FrameEncoder::new(QualityPreset::Medium);
        encoder.set_encoding(EncodingType::Raw);

        let frame = create_test_frame();

        // Encode multiple frames
        for _ in 0..5 {
            encoder.encode(&frame).unwrap();
        }

        let stats = encoder.stats();
        assert_eq!(stats.frames_encoded, 5);
        assert!(stats.total_encode_time.as_millis() > 0);
        assert!(stats.avg_encode_time().as_millis() > 0);
    }

    #[test]
    #[cfg(feature = "remotedesktop")]
    fn test_quality_presets() {
        let mut encoder = FrameEncoder::new(QualityPreset::Low);
        assert_eq!(encoder.preferred_encoding, EncodingType::H264);

        encoder.set_quality(QualityPreset::Medium);
        assert_eq!(encoder.preferred_encoding, EncodingType::LZ4);

        encoder.set_quality(QualityPreset::High);
        assert_eq!(encoder.preferred_encoding, EncodingType::Raw);
    }

    #[test]
    #[cfg(feature = "remotedesktop")]
    fn test_compression_ratio() {
        let mut encoder = FrameEncoder::new(QualityPreset::Medium);
        encoder.set_encoding(EncodingType::LZ4);

        let frame = create_test_frame();
        encoder.encode(&frame).unwrap();

        let stats = encoder.stats();
        let ratio = stats.avg_compression_ratio();
        // Solid color should compress well
        assert!(
            ratio > 2.0,
            "Compression ratio should be > 2.0, got {}",
            ratio
        );
    }

    #[test]
    #[cfg(feature = "remotedesktop")]
    fn test_h264_encoding() {
        let mut encoder = FrameEncoder::new(QualityPreset::Low);
        encoder.set_encoding(EncodingType::H264);

        let frame = create_test_frame();
        let original_size = frame.size();
        let encoded = encoder.encode(&frame).unwrap();

        assert_eq!(encoded.encoding, EncodingType::H264);
        assert_eq!(encoded.width, frame.width);
        assert_eq!(encoded.height, frame.height);
        // H.264 should produce some output
        assert!(
            !encoded.data.is_empty(),
            "H.264 encoding produced empty data"
        );

        println!(
            "H.264 compression: {} -> {} bytes ({:.1}x)",
            original_size,
            encoded.data.len(),
            original_size as f32 / encoded.data.len() as f32
        );
    }

    #[test]
    #[cfg(feature = "remotedesktop")]
    fn test_h264_quality_presets() {
        let frame = create_test_frame();

        // Test different quality presets
        for quality in [
            QualityPreset::Low,
            QualityPreset::Medium,
            QualityPreset::High,
        ] {
            let mut encoder = FrameEncoder::new(quality);
            encoder.set_encoding(EncodingType::H264);

            let result = encoder.encode(&frame);
            assert!(
                result.is_ok(),
                "H.264 encoding with {:?} quality should succeed",
                quality
            );
        }
    }

    #[test]
    #[cfg(feature = "remotedesktop")]
    fn test_hextile_encoding() {
        let mut encoder = FrameEncoder::new(QualityPreset::Medium);
        encoder.set_encoding(EncodingType::Hextile);

        let frame = create_test_frame();
        let encoded = encoder.encode(&frame).unwrap();

        assert_eq!(encoded.encoding, EncodingType::Hextile);
        assert_eq!(encoded.width, frame.width);
        assert_eq!(encoded.height, frame.height);
        assert!(
            !encoded.data.is_empty(),
            "Hextile encoding produced empty data"
        );

        println!(
            "Hextile compression: {} -> {} bytes ({:.1}x)",
            frame.size(),
            encoded.data.len(),
            frame.size() as f32 / encoded.data.len() as f32
        );
    }

    #[test]
    #[cfg(feature = "remotedesktop")]
    fn test_hextile_uniform_tile() {
        // Create frame with uniform color for optimal Hextile compression
        let width = 640;
        let height = 480;
        let data = vec![200u8; (width * height * 4) as usize]; // Solid gray
        let frame = RawFrame::new(width, height, PixelFormat::RGBA, data);

        let mut encoder = FrameEncoder::new(QualityPreset::Medium);
        encoder.set_encoding(EncodingType::Hextile);

        let original_size = frame.size();
        let encoded = encoder.encode(&frame).unwrap();

        // Uniform color should compress very well with Hextile
        assert!(
            encoded.data.len() < original_size / 10,
            "Hextile should compress uniform color by at least 10x, got {}x",
            original_size as f32 / encoded.data.len() as f32
        );
    }

    #[test]
    #[cfg(feature = "remotedesktop")]
    fn test_hextile_with_pattern() {
        // Create frame with checkerboard pattern
        let width = 640;
        let height = 480;
        let mut data = vec![0u8; (width * height * 4) as usize];

        for y in 0..height {
            for x in 0..width {
                let idx = ((y * width + x) * 4) as usize;
                let color = if (x / 16 + y / 16) % 2 == 0 {
                    255u8
                } else {
                    0u8
                };
                data[idx] = color;
                data[idx + 1] = color;
                data[idx + 2] = color;
                data[idx + 3] = 255;
            }
        }

        let frame = RawFrame::new(width, height, PixelFormat::RGBA, data);
        let mut encoder = FrameEncoder::new(QualityPreset::Medium);
        encoder.set_encoding(EncodingType::Hextile);

        let encoded = encoder.encode(&frame).unwrap();
        assert_eq!(encoded.encoding, EncodingType::Hextile);
    }

    #[test]
    #[cfg(feature = "remotedesktop")]
    fn test_encoding_type_switch() {
        let mut encoder = FrameEncoder::new(QualityPreset::Medium);
        let frame = create_test_frame();

        // Test switching between encoding types
        for encoding in [
            EncodingType::Raw,
            EncodingType::LZ4,
            EncodingType::Hextile,
            EncodingType::H264,
        ] {
            encoder.set_encoding(encoding);
            let result = encoder.encode(&frame);
            assert!(
                result.is_ok(),
                "Encoding with {:?} should succeed",
                encoding
            );
        }
    }

    #[test]
    #[cfg(feature = "remotedesktop")]
    fn test_yuv_conversion() {
        let encoder = FrameEncoder::new(QualityPreset::Medium);
        let frame = create_test_frame();

        let yuv_result = encoder.rgba_to_yuv420(&frame);
        assert!(yuv_result.is_ok(), "YUV conversion should succeed");

        let yuv = yuv_result.unwrap();
        let expected_size = (frame.width * frame.height * 3 / 2) as usize;
        assert_eq!(
            yuv.len(),
            expected_size,
            "YUV420 size should be 1.5x width*height"
        );
    }
}
