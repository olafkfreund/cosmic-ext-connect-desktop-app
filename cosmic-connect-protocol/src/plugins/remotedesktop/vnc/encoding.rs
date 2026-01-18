//! Frame Encoding for VNC
//!
//! Implements multiple encoding types for VNC framebuffer updates:
//! - Raw: Uncompressed pixel data
//! - LZ4: Fast lossless compression
//! - H.264: Video compression (TODO: Phase 3)
//! - Hextile: VNC standard tile-based encoding (TODO: Phase 4)

use crate::plugins::remotedesktop::capture::{
    EncodedFrame, EncodingType, PixelFormat, QualityPreset, RawFrame,
};
use crate::Result;
use std::time::Instant;
use tracing::{debug, info, warn};

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
    fn encode_h264(&self, frame: &RawFrame) -> Result<EncodedFrame> {
        // TODO: Phase 3 - H.264 implementation
        // For now, fall back to LZ4
        warn!("H.264 encoding not yet implemented, falling back to LZ4");
        self.encode_lz4(frame)
    }

    /// Encode with Hextile (VNC standard)
    fn encode_hextile(&self, frame: &RawFrame) -> Result<EncodedFrame> {
        // TODO: Phase 4 - Hextile implementation
        // For now, fall back to Raw
        warn!("Hextile encoding not yet implemented, falling back to Raw");
        self.encode_raw(frame)
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
}
