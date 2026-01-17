//! Audio Stream Plugin
//!
//! Stream audio between connected desktops in real-time.
//!
//! ## Protocol Specification
//!
//! This plugin implements the KDE Connect AudioStream protocol for streaming
//! audio output and microphone input between devices.
//!
//! ### Packet Types
//!
//! - `cconnect.audiostream.start` - Start audio stream with configuration
//! - `cconnect.audiostream.data` - Audio data packet (via payload)
//! - `cconnect.audiostream.stop` - Stop audio stream
//! - `cconnect.audiostream.config` - Update stream configuration
//!
//! ### Capabilities
//!
//! - Incoming: `cconnect.audiostream` - Can receive audio streams
//! - Outgoing: `cconnect.audiostream` - Can send audio streams
//!
//! ### Use Cases
//!
//! - Forward desktop audio to another room
//! - Conference calls with remote audio
//! - Media playback on remote speakers
//! - Voice chat between desktops
//! - Distributed audio setups
//!
//! ## Features
//!
//! - **Multiple Codecs**: Opus (recommended), PCM, AAC
//! - **Quality Control**: Configurable bitrate and sample rate
//! - **Low Latency Mode**: Minimize audio delay
//! - **Multi-channel**: Stereo and mono support
//! - **Buffer Management**: Smooth playback with network jitter
//! - **Virtual Devices**: Create virtual audio sinks/sources
//!
//! ## Audio Backend
//!
//! - **PipeWire** (preferred): Native COSMIC audio, low latency
//! - **PulseAudio** (fallback): Wider compatibility
//!
//! ## Implementation Status
//!
//! TODO: Audio backend integration (PipeWire/PulseAudio)
//! TODO: Codec implementation (Opus, PCM, AAC)
//! TODO: Buffer management and latency compensation
//! TODO: Virtual audio device creation
//! TODO: Volume synchronization

use crate::plugins::{Plugin, PluginFactory};
use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

const PLUGIN_NAME: &str = "audiostream";
const INCOMING_CAPABILITY: &str = "cconnect.audiostream";
const OUTGOING_CAPABILITY: &str = "cconnect.audiostream";

// Audio configuration constants
const DEFAULT_SAMPLE_RATE: u32 = 48000;
const DEFAULT_BITRATE: u32 = 128000; // 128 kbps
const DEFAULT_CHANNELS: u8 = 2; // Stereo
const MAX_BUFFER_SIZE_MS: u32 = 500; // 500ms max buffer
const MIN_BUFFER_SIZE_MS: u32 = 50; // 50ms min buffer

/// Audio codec type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioCodec {
    /// Opus codec - best quality/latency tradeoff (recommended)
    Opus,
    /// PCM - uncompressed, lowest latency
    Pcm,
    /// AAC - good quality, higher latency
    Aac,
}

impl Default for AudioCodec {
    fn default() -> Self {
        Self::Opus
    }
}

impl AudioCodec {
    /// Get codec name as string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Opus => "opus",
            Self::Pcm => "pcm",
            Self::Aac => "aac",
        }
    }
}

/// Audio stream direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamDirection {
    /// Output stream (desktop audio to remote)
    Output,
    /// Input stream (microphone to remote)
    Input,
}

/// Audio stream configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    /// Audio codec to use
    #[serde(default)]
    pub codec: AudioCodec,

    /// Sample rate in Hz (8000, 16000, 24000, 48000)
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,

    /// Bitrate in bits per second (for compressed codecs)
    #[serde(default = "default_bitrate")]
    pub bitrate: u32,

    /// Number of audio channels (1=mono, 2=stereo)
    #[serde(default = "default_channels")]
    pub channels: u8,

    /// Stream direction
    pub direction: StreamDirection,

    /// Enable low latency mode
    #[serde(default)]
    pub low_latency: bool,

    /// Buffer size in milliseconds
    #[serde(default = "default_buffer_size")]
    pub buffer_size_ms: u32,
}

fn default_sample_rate() -> u32 {
    DEFAULT_SAMPLE_RATE
}

fn default_bitrate() -> u32 {
    DEFAULT_BITRATE
}

fn default_channels() -> u8 {
    DEFAULT_CHANNELS
}

fn default_buffer_size() -> u32 {
    if cfg!(feature = "low_latency") {
        MIN_BUFFER_SIZE_MS
    } else {
        150 // 150ms default balance
    }
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            codec: AudioCodec::default(),
            sample_rate: default_sample_rate(),
            bitrate: default_bitrate(),
            channels: default_channels(),
            direction: StreamDirection::Output,
            low_latency: false,
            buffer_size_ms: default_buffer_size(),
        }
    }
}

impl StreamConfig {
    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        // Validate sample rate
        match self.sample_rate {
            8000 | 16000 | 24000 | 48000 => {}
            _ => {
                return Err(ProtocolError::InvalidPacket(format!(
                    "Invalid sample rate: {}. Must be 8000, 16000, 24000, or 48000",
                    self.sample_rate
                )))
            }
        }

        // Validate channels
        if self.channels < 1 || self.channels > 2 {
            return Err(ProtocolError::InvalidPacket(format!(
                "Invalid channel count: {}. Must be 1 (mono) or 2 (stereo)",
                self.channels
            )));
        }

        // Validate buffer size
        if self.buffer_size_ms < MIN_BUFFER_SIZE_MS || self.buffer_size_ms > MAX_BUFFER_SIZE_MS {
            return Err(ProtocolError::InvalidPacket(format!(
                "Invalid buffer size: {}ms. Must be between {}ms and {}ms",
                self.buffer_size_ms, MIN_BUFFER_SIZE_MS, MAX_BUFFER_SIZE_MS
            )));
        }

        // Validate bitrate for compressed codecs
        if matches!(self.codec, AudioCodec::Opus | AudioCodec::Aac) {
            if self.bitrate < 32000 || self.bitrate > 512000 {
                warn!(
                    "Bitrate {}bps may not be optimal. Recommended: 64-320 kbps",
                    self.bitrate
                );
            }
        }

        Ok(())
    }
}

/// Active audio stream state
#[derive(Debug)]
struct AudioStream {
    /// Stream configuration
    config: StreamConfig,

    /// Stream start timestamp
    started_at: std::time::Instant,

    /// Total bytes streamed
    bytes_streamed: u64,

    /// Packets sent/received
    packet_count: u64,

    /// Audio buffer (for playback)
    /// TODO: Implement actual audio buffering
    #[allow(dead_code)]
    buffer: Vec<u8>,
}

impl AudioStream {
    fn new(config: StreamConfig) -> Self {
        Self {
            config,
            started_at: std::time::Instant::now(),
            bytes_streamed: 0,
            packet_count: 0,
            buffer: Vec::new(),
        }
    }

    fn update_stats(&mut self, bytes: u64) {
        self.bytes_streamed += bytes;
        self.packet_count += 1;
    }

    fn get_stats(&self) -> StreamStats {
        let duration = self.started_at.elapsed();
        let bitrate = if duration.as_secs() > 0 {
            (self.bytes_streamed * 8) / duration.as_secs()
        } else {
            0
        };

        StreamStats {
            duration_secs: duration.as_secs(),
            bytes_streamed: self.bytes_streamed,
            packet_count: self.packet_count,
            current_bitrate: bitrate,
        }
    }
}

/// Stream statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamStats {
    /// Stream duration in seconds
    pub duration_secs: u64,

    /// Total bytes streamed
    pub bytes_streamed: u64,

    /// Number of packets sent/received
    pub packet_count: u64,

    /// Current bitrate in bits per second
    pub current_bitrate: u64,
}

/// Audio Stream plugin
pub struct AudioStreamPlugin {
    /// Device ID this plugin is associated with
    device_id: Option<String>,

    /// Plugin enabled state
    enabled: bool,

    /// Active outgoing stream (this device sending audio)
    outgoing_stream: Arc<RwLock<Option<AudioStream>>>,

    /// Active incoming stream (receiving audio from remote)
    incoming_stream: Arc<RwLock<Option<AudioStream>>>,

    /// Supported codecs on this system
    /// TODO: Detect from audio backend
    supported_codecs: Vec<AudioCodec>,
}

impl AudioStreamPlugin {
    /// Create new audio stream plugin instance
    pub fn new() -> Self {
        Self {
            device_id: None,
            enabled: false,
            outgoing_stream: Arc::new(RwLock::new(None)),
            incoming_stream: Arc::new(RwLock::new(None)),
            supported_codecs: vec![AudioCodec::Opus, AudioCodec::Pcm, AudioCodec::Aac],
        }
    }

    /// Start audio stream with configuration
    pub async fn start_stream(&mut self, config: StreamConfig) -> Result<()> {
        config.validate()?;

        info!(
            "Starting audio stream: {:?} {} {}Hz {} ch",
            config.direction,
            config.codec.as_str(),
            config.sample_rate,
            config.channels
        );

        match config.direction {
            StreamDirection::Output => {
                // Stop existing outgoing stream if any
                self.stop_outgoing_stream().await?;

                // Create new outgoing stream
                let stream = AudioStream::new(config.clone());
                *self.outgoing_stream.write().await = Some(stream);

                // TODO: Initialize audio capture from PipeWire/PulseAudio
                // TODO: Start encoding thread
                info!("Outgoing audio stream started");
            }
            StreamDirection::Input => {
                // Stop existing incoming stream if any
                self.stop_incoming_stream().await?;

                // Create new incoming stream
                let stream = AudioStream::new(config.clone());
                *self.incoming_stream.write().await = Some(stream);

                // TODO: Initialize audio playback sink
                // TODO: Start decoding thread
                info!("Incoming audio stream started");
            }
        }

        Ok(())
    }

    /// Stop outgoing audio stream
    pub async fn stop_outgoing_stream(&mut self) -> Result<()> {
        let mut stream_lock = self.outgoing_stream.write().await;
        if let Some(stream) = stream_lock.take() {
            let stats = stream.get_stats();
            info!(
                "Stopped outgoing stream: {} packets, {} bytes, {} seconds",
                stats.packet_count, stats.bytes_streamed, stats.duration_secs
            );

            // TODO: Stop audio capture
            // TODO: Stop encoding thread
        }
        Ok(())
    }

    /// Stop incoming audio stream
    pub async fn stop_incoming_stream(&mut self) -> Result<()> {
        let mut stream_lock = self.incoming_stream.write().await;
        if let Some(stream) = stream_lock.take() {
            let stats = stream.get_stats();
            info!(
                "Stopped incoming stream: {} packets, {} bytes, {} seconds",
                stats.packet_count, stats.bytes_streamed, stats.duration_secs
            );

            // TODO: Stop audio playback
            // TODO: Stop decoding thread
        }
        Ok(())
    }

    /// Update stream configuration
    pub async fn update_config(&mut self, config: StreamConfig) -> Result<()> {
        config.validate()?;

        match config.direction {
            StreamDirection::Output => {
                if let Some(stream) = self.outgoing_stream.write().await.as_mut() {
                    stream.config = config;
                    info!("Updated outgoing stream configuration");
                    // TODO: Reconfigure encoder
                }
            }
            StreamDirection::Input => {
                if let Some(stream) = self.incoming_stream.write().await.as_mut() {
                    stream.config = config;
                    info!("Updated incoming stream configuration");
                    // TODO: Reconfigure decoder
                }
            }
        }

        Ok(())
    }

    /// Process audio data packet
    async fn process_audio_data(&self, data: &[u8]) -> Result<()> {
        let mut stream_lock = self.incoming_stream.write().await;
        if let Some(stream) = stream_lock.as_mut() {
            stream.update_stats(data.len() as u64);

            // TODO: Decode audio data based on codec
            // TODO: Write to audio playback buffer
            // TODO: Handle buffer underrun/overrun

            debug!("Processed {} bytes of audio data", data.len());
        } else {
            warn!("Received audio data but no incoming stream is active");
        }

        Ok(())
    }

    /// Get stream statistics
    pub async fn get_stats(&self, direction: StreamDirection) -> Option<StreamStats> {
        match direction {
            StreamDirection::Output => {
                self.outgoing_stream
                    .read()
                    .await
                    .as_ref()
                    .map(|s| s.get_stats())
            }
            StreamDirection::Input => {
                self.incoming_stream
                    .read()
                    .await
                    .as_ref()
                    .map(|s| s.get_stats())
            }
        }
    }

    /// Check if a codec is supported
    pub fn is_codec_supported(&self, codec: AudioCodec) -> bool {
        self.supported_codecs.contains(&codec)
    }

    /// Get list of supported codecs
    pub fn supported_codecs(&self) -> &[AudioCodec] {
        &self.supported_codecs
    }
}

impl Default for AudioStreamPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for AudioStreamPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![INCOMING_CAPABILITY.to_string()]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![OUTGOING_CAPABILITY.to_string()]
    }

    async fn init(&mut self, device: &Device) -> Result<()> {
        info!("Initializing AudioStream plugin for device {}", device.name());
        self.device_id = Some(device.id().to_string());

        // TODO: Detect available audio backends
        // TODO: Initialize PipeWire/PulseAudio connection
        // TODO: Query available audio devices

        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Starting AudioStream plugin");
        self.enabled = true;

        // TODO: Setup audio device monitoring
        // TODO: Register virtual audio sink/source

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Stopping AudioStream plugin");
        self.enabled = false;

        // Stop all active streams
        self.stop_outgoing_stream().await?;
        self.stop_incoming_stream().await?;

        // TODO: Cleanup audio devices
        // TODO: Unregister virtual devices

        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, _device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("AudioStream plugin is disabled, ignoring packet");
            return Ok(());
        }

        debug!("Handling packet type: {}", packet.packet_type);

        match packet.packet_type.as_str() {
            "cconnect.audiostream.start" => {
                // Start audio stream with configuration
                let config: StreamConfig = serde_json::from_value(packet.body.clone())
                    .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

                self.start_stream(config).await?;

                info!("Audio stream started from remote request");
            }

            "cconnect.audiostream.stop" => {
                // Stop audio stream
                let direction: StreamDirection = packet
                    .body
                    .get("direction")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or(StreamDirection::Output);

                match direction {
                    StreamDirection::Output => self.stop_outgoing_stream().await?,
                    StreamDirection::Input => self.stop_incoming_stream().await?,
                }

                info!("Audio stream stopped from remote request");
            }

            "cconnect.audiostream.config" => {
                // Update stream configuration
                let config: StreamConfig = serde_json::from_value(packet.body.clone())
                    .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

                self.update_config(config).await?;

                info!("Audio stream configuration updated");
            }

            "cconnect.audiostream.data" => {
                // Process audio data packet
                // TODO: Extract audio data from packet payload
                // For now, just acknowledge receipt
                if let Some(_payload) = packet.body.get("data").and_then(|v| v.as_str()) {
                    // TODO: Decode base64 payload
                    debug!("Received audio data packet (payload size unknown)");
                } else {
                    warn!("Audio data packet has no payload");
                }
            }

            _ => {
                warn!("Unknown AudioStream packet type: {}", packet.packet_type);
            }
        }

        Ok(())
    }
}

/// Audio Stream plugin factory
pub struct AudioStreamPluginFactory;

impl PluginFactory for AudioStreamPluginFactory {
    fn create(&self) -> Box<dyn Plugin> {
        Box::new(AudioStreamPlugin::new())
    }

    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![INCOMING_CAPABILITY.to_string()]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![OUTGOING_CAPABILITY.to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_device;

    #[tokio::test]
    async fn test_plugin_creation() {
        let plugin = AudioStreamPlugin::new();
        assert_eq!(plugin.name(), PLUGIN_NAME);
        assert!(!plugin.enabled);
    }

    #[tokio::test]
    async fn test_stream_config_validation() {
        let config = StreamConfig::default();
        assert!(config.validate().is_ok());

        let mut invalid_config = config.clone();
        invalid_config.sample_rate = 44100; // Invalid sample rate
        assert!(invalid_config.validate().is_err());

        let mut invalid_channels = config.clone();
        invalid_channels.channels = 5; // Invalid channel count
        assert!(invalid_channels.validate().is_err());

        let mut invalid_buffer = config;
        invalid_buffer.buffer_size_ms = 1000; // Too large
        assert!(invalid_buffer.validate().is_err());
    }

    #[tokio::test]
    async fn test_start_stop_outgoing_stream() {
        let mut plugin = AudioStreamPlugin::new();
        plugin.enabled = true;

        let config = StreamConfig {
            direction: StreamDirection::Output,
            ..Default::default()
        };

        assert!(plugin.start_stream(config).await.is_ok());
        assert!(plugin.outgoing_stream.read().await.is_some());

        assert!(plugin.stop_outgoing_stream().await.is_ok());
        assert!(plugin.outgoing_stream.read().await.is_none());
    }

    #[tokio::test]
    async fn test_start_stop_incoming_stream() {
        let mut plugin = AudioStreamPlugin::new();
        plugin.enabled = true;

        let config = StreamConfig {
            direction: StreamDirection::Input,
            ..Default::default()
        };

        assert!(plugin.start_stream(config).await.is_ok());
        assert!(plugin.incoming_stream.read().await.is_some());

        assert!(plugin.stop_incoming_stream().await.is_ok());
        assert!(plugin.incoming_stream.read().await.is_none());
    }

    #[tokio::test]
    async fn test_update_config() {
        let mut plugin = AudioStreamPlugin::new();
        plugin.enabled = true;

        let config = StreamConfig {
            direction: StreamDirection::Output,
            sample_rate: 48000,
            ..Default::default()
        };

        plugin.start_stream(config).await.unwrap();

        let new_config = StreamConfig {
            direction: StreamDirection::Output,
            sample_rate: 24000,
            ..Default::default()
        };

        assert!(plugin.update_config(new_config).await.is_ok());

        let stream = plugin.outgoing_stream.read().await;
        assert_eq!(stream.as_ref().unwrap().config.sample_rate, 24000);
    }

    #[tokio::test]
    async fn test_codec_support() {
        let plugin = AudioStreamPlugin::new();

        assert!(plugin.is_codec_supported(AudioCodec::Opus));
        assert!(plugin.is_codec_supported(AudioCodec::Pcm));
        assert!(plugin.is_codec_supported(AudioCodec::Aac));

        assert_eq!(plugin.supported_codecs().len(), 3);
    }

    #[tokio::test]
    async fn test_stream_stats() {
        let mut plugin = AudioStreamPlugin::new();
        plugin.enabled = true;

        let config = StreamConfig {
            direction: StreamDirection::Output,
            ..Default::default()
        };

        plugin.start_stream(config).await.unwrap();

        // Simulate some data streaming
        if let Some(stream) = plugin.outgoing_stream.write().await.as_mut() {
            stream.update_stats(1024);
            stream.update_stats(2048);
        }

        let stats = plugin.get_stats(StreamDirection::Output).await;
        assert!(stats.is_some());

        let stats = stats.unwrap();
        assert_eq!(stats.bytes_streamed, 3072);
        assert_eq!(stats.packet_count, 2);
    }

    #[tokio::test]
    async fn test_handle_start_packet() {
        let mut device = create_test_device();
        let factory = AudioStreamPluginFactory;
        let mut plugin = factory.create();

        plugin.init(&device).await.unwrap();
        plugin.start().await.unwrap();

        let config = StreamConfig::default();
        let body = serde_json::to_value(&config).unwrap();

        let packet = Packet {
            id: 1,
            packet_type: "cconnect.audiostream.start".to_string(),
            body,
        };

        assert!(plugin.handle_packet(&packet, &mut device).await.is_ok());
    }

    #[tokio::test]
    async fn test_handle_stop_packet() {
        let mut device = create_test_device();
        let factory = AudioStreamPluginFactory;
        let mut plugin = factory.create();

        plugin.init(&device).await.unwrap();
        plugin.start().await.unwrap();

        // Start a stream first
        let config = StreamConfig {
            direction: StreamDirection::Output,
            ..Default::default()
        };

        // Need to downcast to access start_stream
        let audio_plugin = plugin.as_any_mut().downcast_mut::<AudioStreamPlugin>().unwrap();
        audio_plugin.start_stream(config).await.unwrap();

        // Now stop it
        let mut body = serde_json::Map::new();
        body.insert(
            "direction".to_string(),
            serde_json::to_value(StreamDirection::Output).unwrap(),
        );

        let packet = Packet {
            id: 2,
            packet_type: "cconnect.audiostream.stop".to_string(),
            body: serde_json::Value::Object(body),
        };

        assert!(plugin.handle_packet(&packet, &mut device).await.is_ok());

        // Verify stream is stopped
        let outgoing_plugin = plugin
            .as_any()
            .downcast_ref::<AudioStreamPlugin>()
            .unwrap();
        assert!(outgoing_plugin.outgoing_stream.read().await.is_none());
    }
}
