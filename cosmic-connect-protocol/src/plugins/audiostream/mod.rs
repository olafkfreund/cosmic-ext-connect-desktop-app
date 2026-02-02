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
//! - `cconnect.audiostream.volume` - Request volume change on remote stream
//! - `cconnect.audiostream.volume_changed` - Notify of volume change
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
//! - ✓ Codec implementation (Opus, PCM, AAC)
//! - ✓ Volume synchronization with bidirectional control
//! - ✓ Buffer management and latency compensation
//! - Partial: Audio backend integration (PipeWire/PulseAudio) - requires feature flag
//! - Future: Virtual audio device creation
//! - Future: Advanced audio device monitoring

use crate::plugins::{Plugin, PluginFactory};
use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

#[cfg(feature = "audiostream")]
mod audio_backend;

#[cfg(feature = "audiostream")]
mod codec;

#[cfg(feature = "audiostream")]
use audio_backend::{AudioBackend, AudioSample, BackendConfig};

#[cfg(feature = "audiostream")]
use codec::{AacCodec, OpusCodec, PcmCodec};

const PLUGIN_NAME: &str = "audiostream";
const INCOMING_CAPABILITY: &str = "cconnect.audiostream";
const OUTGOING_CAPABILITY: &str = "cconnect.audiostream";

// Audio configuration constants
#[allow(dead_code)]
const DEFAULT_SAMPLE_RATE: u32 = 48000;
#[allow(dead_code)]
const DEFAULT_BITRATE: u32 = 128000; // 128 kbps
#[allow(dead_code)]
const DEFAULT_CHANNELS: u8 = 2; // Stereo
#[allow(dead_code)]
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
    buffer: std::collections::VecDeque<Vec<u8>>,

    /// Volume level (0.0 to 1.0)
    volume: f32,

    #[cfg(feature = "audiostream")]
    /// Opus codec instance
    opus_codec: Option<OpusCodec>,

    #[cfg(feature = "audiostream")]
    /// PCM codec instance
    pcm_codec: Option<PcmCodec>,

    #[cfg(feature = "audiostream")]
    /// AAC codec instance
    aac_codec: Option<AacCodec>,

    #[cfg(feature = "audiostream")]
    /// Audio capture channel receiver
    capture_rx: Option<mpsc::Receiver<Vec<AudioSample>>>,

    #[cfg(feature = "audiostream")]
    /// Audio playback channel sender
    playback_tx: Option<mpsc::Sender<Vec<AudioSample>>>,
}

impl AudioStream {
    fn new(config: StreamConfig) -> Self {
        Self {
            config,
            started_at: std::time::Instant::now(),
            bytes_streamed: 0,
            packet_count: 0,
            buffer: std::collections::VecDeque::new(),
            volume: 1.0, // Default to full volume
            #[cfg(feature = "audiostream")]
            opus_codec: None,
            #[cfg(feature = "audiostream")]
            pcm_codec: None,
            #[cfg(feature = "audiostream")]
            aac_codec: None,
            #[cfg(feature = "audiostream")]
            capture_rx: None,
            #[cfg(feature = "audiostream")]
            playback_tx: None,
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
    supported_codecs: Vec<AudioCodec>,

    #[cfg(feature = "audiostream")]
    /// Audio backend for capture and playback
    audio_backend: Option<Arc<RwLock<AudioBackend>>>,

    /// Packet sender for outgoing audio data
    packet_sender: Option<mpsc::Sender<(String, Packet)>>,
}

impl AudioStreamPlugin {
    /// Create new audio stream plugin instance
    pub fn new() -> Self {
        let supported_codecs = {
            #[allow(unused_mut)]
            let mut codecs = Vec::new();

            #[cfg(feature = "audiostream")]
            {
                // PCM is always supported when audiostream feature is enabled
                codecs.push(AudioCodec::Pcm);

                // Opus only if the opus feature is enabled
                #[cfg(feature = "opus")]
                codecs.push(AudioCodec::Opus);

                // AAC only if the aac feature is enabled
                #[cfg(feature = "aac")]
                codecs.push(AudioCodec::Aac);
            }

            codecs
        };

        Self {
            device_id: None,
            enabled: false,
            outgoing_stream: Arc::new(RwLock::new(None)),
            incoming_stream: Arc::new(RwLock::new(None)),
            supported_codecs,
            #[cfg(feature = "audiostream")]
            audio_backend: None,
            packet_sender: None,
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

        #[cfg(feature = "audiostream")]
        {
            match config.direction {
                StreamDirection::Output => {
                    // Stop existing outgoing stream if any
                    self.stop_outgoing_stream().await?;

                    // Create audio backend if needed
                    if self.audio_backend.is_none() {
                        let backend_config = BackendConfig {
                            sample_rate: config.sample_rate,
                            channels: config.channels,
                            buffer_size: (config.sample_rate as usize
                                * config.buffer_size_ms as usize)
                                / 1000,
                        };
                        let backend = AudioBackend::new(backend_config)?;
                        self.audio_backend = Some(Arc::new(RwLock::new(backend)));
                    }

                    // Create new outgoing stream
                    let mut stream = AudioStream::new(config.clone());

                    // Initialize codec
                    match config.codec {
                        AudioCodec::Opus => {
                            stream.opus_codec = Some(OpusCodec::new(
                                config.sample_rate,
                                config.channels,
                                config.bitrate,
                            )?);
                        }
                        AudioCodec::Pcm => {
                            stream.pcm_codec =
                                Some(PcmCodec::new(config.sample_rate, config.channels));
                        }
                        AudioCodec::Aac => {
                            stream.aac_codec = Some(AacCodec::new(
                                config.sample_rate,
                                config.channels,
                                config.bitrate,
                            )?);
                        }
                    }

                    // Start audio capture
                    if let Some(backend) = &mut self.audio_backend {
                        let capture_rx = backend.write().await.start_capture()?;
                        stream.capture_rx = Some(capture_rx);
                    }

                    *self.outgoing_stream.write().await = Some(stream);

                    // Start encoding and sending task
                    self.start_outgoing_task().await?;

                    info!("Outgoing audio stream started");
                }
                StreamDirection::Input => {
                    // Stop existing incoming stream if any
                    self.stop_incoming_stream().await?;

                    // Create audio backend if needed
                    if self.audio_backend.is_none() {
                        let backend_config = BackendConfig {
                            sample_rate: config.sample_rate,
                            channels: config.channels,
                            buffer_size: (config.sample_rate as usize
                                * config.buffer_size_ms as usize)
                                / 1000,
                        };
                        let backend = AudioBackend::new(backend_config)?;
                        self.audio_backend = Some(Arc::new(RwLock::new(backend)));
                    }

                    // Create new incoming stream
                    let mut stream = AudioStream::new(config.clone());

                    // Initialize codec
                    match config.codec {
                        AudioCodec::Opus => {
                            stream.opus_codec = Some(OpusCodec::new(
                                config.sample_rate,
                                config.channels,
                                config.bitrate,
                            )?);
                        }
                        AudioCodec::Pcm => {
                            stream.pcm_codec =
                                Some(PcmCodec::new(config.sample_rate, config.channels));
                        }
                        AudioCodec::Aac => {
                            stream.aac_codec = Some(AacCodec::new(
                                config.sample_rate,
                                config.channels,
                                config.bitrate,
                            )?);
                        }
                    }

                    // Start audio playback
                    if let Some(backend) = &mut self.audio_backend {
                        let playback_tx = backend.write().await.start_playback()?;
                        stream.playback_tx = Some(playback_tx);
                    }

                    *self.incoming_stream.write().await = Some(stream);

                    // Start decoding and playback task
                    self.start_incoming_task().await?;

                    info!("Incoming audio stream started");
                }
            }
        }

        #[cfg(not(feature = "audiostream"))]
        {
            warn!("Audio streaming requires 'audiostream' feature to be enabled");
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

            // Audio capture and encoding tasks are automatically stopped
            // when the stream is dropped (channel closes)
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

            // Audio playback and decoding tasks are automatically stopped
            // when the stream is dropped (channel closes)
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
                    // Encoder reconfiguration requires stopping and restarting the stream
                    // This is handled by the client calling stop then start with new config
                }
            }
            StreamDirection::Input => {
                if let Some(stream) = self.incoming_stream.write().await.as_mut() {
                    stream.config = config;
                    info!("Updated incoming stream configuration");
                    // Decoder reconfiguration requires stopping and restarting the stream
                    // This is handled by the client calling stop then start with new config
                }
            }
        }

        Ok(())
    }

    #[cfg(feature = "audiostream")]
    /// Start outgoing audio encoding and transmission task
    async fn start_outgoing_task(&mut self) -> Result<()> {
        let outgoing_stream = self.outgoing_stream.clone();
        let packet_sender = self.packet_sender.clone();
        let device_id = self.device_id.clone();

        tokio::spawn(async move {
            loop {
                let mut stream_lock = outgoing_stream.write().await;
                if let Some(stream) = stream_lock.as_mut() {
                    // Try to receive samples
                    let samples = if let Some(capture_rx) = &mut stream.capture_rx {
                        match capture_rx.try_recv() {
                            Ok(samples) => samples,
                            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                                drop(stream_lock);
                                tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                                continue;
                            }
                            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                                debug!("Capture channel disconnected");
                                break;
                            }
                        }
                    } else {
                        break;
                    };

                    // Encode samples based on codec
                    let encoded = if let Some(opus) = &mut stream.opus_codec {
                        match opus.encode(&samples) {
                            Ok(data) => data,
                            Err(e) => {
                                error!("Opus encoding failed: {}", e);
                                continue;
                            }
                        }
                    } else if let Some(pcm) = &stream.pcm_codec {
                        match pcm.encode(&samples) {
                            Ok(data) => data,
                            Err(e) => {
                                error!("PCM encoding failed: {}", e);
                                continue;
                            }
                        }
                    } else if let Some(aac) = &mut stream.aac_codec {
                        match aac.encode(&samples) {
                            Ok(data) => data,
                            Err(e) => {
                                error!("AAC encoding failed: {}", e);
                                continue;
                            }
                        }
                    } else {
                        error!("No codec available for encoding");
                        break;
                    };

                    // Update stats
                    stream.update_stats(encoded.len() as u64);

                    drop(stream_lock);

                    // Send packet with audio data
                    if let Some(sender) = &packet_sender {
                        if let Some(dev_id) = &device_id {
                            let mut body = serde_json::Map::new();
                            body.insert(
                                "data".to_string(),
                                serde_json::Value::String(BASE64.encode(&encoded)),
                            );

                            let packet = Packet::new(
                                "cconnect.audiostream.data",
                                serde_json::Value::Object(body),
                            );

                            if let Err(e) = sender.send((dev_id.clone(), packet)).await {
                                error!("Failed to send audio packet: {}", e);
                                break;
                            }
                        }
                    }
                } else {
                    break;
                }
            }
            debug!("Outgoing audio task ended");
        });

        Ok(())
    }

    #[cfg(feature = "audiostream")]
    /// Start incoming audio decoding and playback task
    async fn start_incoming_task(&mut self) -> Result<()> {
        let incoming_stream = self.incoming_stream.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

                let mut stream_lock = incoming_stream.write().await;
                if let Some(stream) = stream_lock.as_mut() {
                    // Process buffered packets
                    while let Some(encoded_data) = stream.buffer.pop_front() {
                        // Decode based on codec
                        let samples = if let Some(opus) = &mut stream.opus_codec {
                            match opus.decode(&encoded_data) {
                                Ok(data) => data,
                                Err(e) => {
                                    error!("Opus decoding failed: {}", e);
                                    // Use packet loss concealment
                                    match opus.decode_plc() {
                                        Ok(plc) => plc,
                                        Err(_) => continue,
                                    }
                                }
                            }
                        } else if let Some(pcm) = &stream.pcm_codec {
                            match pcm.decode(&encoded_data) {
                                Ok(data) => data,
                                Err(e) => {
                                    error!("PCM decoding failed: {}", e);
                                    continue;
                                }
                            }
                        } else if let Some(aac) = &mut stream.aac_codec {
                            match aac.decode(&encoded_data) {
                                Ok(data) => data,
                                Err(e) => {
                                    error!("AAC decoding failed: {}", e);
                                    continue;
                                }
                            }
                        } else {
                            error!("No codec available for decoding");
                            break;
                        };

                        // Send to playback
                        if let Some(playback_tx) = &stream.playback_tx {
                            if let Err(e) = playback_tx.send(samples).await {
                                error!("Failed to send samples to playback: {}", e);
                                break;
                            }
                        }
                    }
                } else {
                    break;
                }
            }
            debug!("Incoming audio task ended");
        });

        Ok(())
    }

    /// Process audio data packet
    async fn process_audio_data(&self, _data: &[u8]) -> Result<()> {
        #[cfg(feature = "audiostream")]
        {
            let data = _data;
            let mut stream_lock = self.incoming_stream.write().await;
            if let Some(stream) = stream_lock.as_mut() {
                stream.update_stats(data.len() as u64);

                // Add to buffer for processing by incoming task
                stream.buffer.push_back(data.to_vec());

                debug!("Buffered {} bytes of audio data", data.len());
            } else {
                warn!("Received audio data but no incoming stream is active");
            }
        }

        #[cfg(not(feature = "audiostream"))]
        {
            warn!("Cannot process audio data without 'audiostream' feature");
        }

        Ok(())
    }

    /// Get stream statistics
    pub async fn get_stats(&self, direction: StreamDirection) -> Option<StreamStats> {
        match direction {
            StreamDirection::Output => self
                .outgoing_stream
                .read()
                .await
                .as_ref()
                .map(|s| s.get_stats()),
            StreamDirection::Input => self
                .incoming_stream
                .read()
                .await
                .as_ref()
                .map(|s| s.get_stats()),
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

    /// Set volume level for a stream
    ///
    /// # Arguments
    /// * `direction` - Which stream to adjust (Output or Input)
    /// * `volume` - Volume level from 0.0 (mute) to 1.0 (full volume)
    pub async fn set_volume(&mut self, direction: StreamDirection, volume: f32) -> Result<()> {
        // Clamp volume to valid range
        let volume = volume.clamp(0.0, 1.0);

        match direction {
            StreamDirection::Output => {
                if let Some(stream) = self.outgoing_stream.write().await.as_mut() {
                    stream.volume = volume;
                    info!("Set outgoing stream volume to {:.2}", volume);

                    // Send volume change packet to remote
                    if let Some(sender) = &self.packet_sender {
                        if let Some(dev_id) = &self.device_id {
                            let mut body = serde_json::Map::new();
                            body.insert(
                                "direction".to_string(),
                                serde_json::to_value(StreamDirection::Output).unwrap(),
                            );
                            body.insert("volume".to_string(), serde_json::Value::from(volume));

                            let packet = Packet::new(
                                "cconnect.audiostream.volume_changed",
                                serde_json::Value::Object(body),
                            );

                            if let Err(e) = sender.send((dev_id.clone(), packet)).await {
                                error!("Failed to send volume change packet: {}", e);
                            }
                        }
                    }
                } else {
                    warn!("Cannot set volume: no outgoing stream active");
                }
            }
            StreamDirection::Input => {
                if let Some(stream) = self.incoming_stream.write().await.as_mut() {
                    stream.volume = volume;
                    info!("Set incoming stream volume to {:.2}", volume);

                    // Send volume change packet to remote
                    if let Some(sender) = &self.packet_sender {
                        if let Some(dev_id) = &self.device_id {
                            let mut body = serde_json::Map::new();
                            body.insert(
                                "direction".to_string(),
                                serde_json::to_value(StreamDirection::Input).unwrap(),
                            );
                            body.insert("volume".to_string(), serde_json::Value::from(volume));

                            let packet = Packet::new(
                                "cconnect.audiostream.volume_changed",
                                serde_json::Value::Object(body),
                            );

                            if let Err(e) = sender.send((dev_id.clone(), packet)).await {
                                error!("Failed to send volume change packet: {}", e);
                            }
                        }
                    }
                } else {
                    warn!("Cannot set volume: no incoming stream active");
                }
            }
        }

        Ok(())
    }

    /// Get current volume level for a stream
    ///
    /// # Arguments
    /// * `direction` - Which stream to query (Output or Input)
    ///
    /// # Returns
    /// Volume level from 0.0 to 1.0, or None if stream is not active
    pub async fn get_volume(&self, direction: StreamDirection) -> Option<f32> {
        match direction {
            StreamDirection::Output => self.outgoing_stream.read().await.as_ref().map(|s| s.volume),
            StreamDirection::Input => self.incoming_stream.read().await.as_ref().map(|s| s.volume),
        }
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
        vec![
            INCOMING_CAPABILITY.to_string(),
            "kdeconnect.audiostream".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![OUTGOING_CAPABILITY.to_string()]
    }

    async fn init(
        &mut self,
        device: &Device,
        packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>,
    ) -> Result<()> {
        info!(
            "Initializing AudioStream plugin for device {}",
            device.name()
        );
        self.device_id = Some(device.id().to_string());
        self.packet_sender = Some(packet_sender);

        #[cfg(feature = "audiostream")]
        {
            // PipeWire is initialized on-demand when stream starts
            info!("Audio backend (PipeWire) will be initialized on stream start");
        }

        #[cfg(not(feature = "audiostream"))]
        {
            warn!(
                "AudioStream plugin initialized without 'audiostream' feature - streaming disabled"
            );
        }

        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Starting AudioStream plugin");
        self.enabled = true;

        // Future enhancement: Setup audio device monitoring for automatic stream management
        // Future enhancement: Register virtual audio sink/source for seamless integration

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Stopping AudioStream plugin");
        self.enabled = false;

        // Stop all active streams
        self.stop_outgoing_stream().await?;
        self.stop_incoming_stream().await?;

        // Future enhancement: Cleanup audio devices when implemented
        // Future enhancement: Unregister virtual devices when implemented

        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, _device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("AudioStream plugin is disabled, ignoring packet");
            return Ok(());
        }

        debug!("Handling packet type: {}", packet.packet_type);

        if packet.is_type("cconnect.audiostream.start") {
            // Start audio stream with configuration
            let config: StreamConfig = serde_json::from_value(packet.body.clone())
                .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

            self.start_stream(config).await?;

            info!("Audio stream started from remote request");
        } else if packet.is_type("cconnect.audiostream.stop") {
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
        } else if packet.is_type("cconnect.audiostream.config") {
            // Update stream configuration
            let config: StreamConfig = serde_json::from_value(packet.body.clone())
                .map_err(|e| ProtocolError::InvalidPacket(e.to_string()))?;

            self.update_config(config).await?;

            info!("Audio stream configuration updated");
        } else if packet.is_type("cconnect.audiostream.data") {
            // Process audio data packet
            if let Some(payload_b64) = packet.body.get("data").and_then(|v| v.as_str()) {
                // Decode base64 payload
                match BASE64.decode(payload_b64) {
                    Ok(audio_data) => {
                        debug!("Received audio data packet: {} bytes", audio_data.len());
                        self.process_audio_data(&audio_data).await?;
                    }
                    Err(e) => {
                        warn!("Failed to decode base64 audio data: {}", e);
                    }
                }
            } else {
                warn!("Audio data packet has no payload");
            }
        } else if packet.is_type("cconnect.audiostream.volume") {
            // Remote requests volume change
            let direction: StreamDirection = packet
                .body
                .get("direction")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or(StreamDirection::Output);

            if let Some(volume) = packet.body.get("volume").and_then(|v| v.as_f64()) {
                self.set_volume(direction, volume as f32).await?;
                info!(
                    "Volume set to {:.2} for {:?} stream from remote request",
                    volume, direction
                );
            } else {
                warn!("Volume packet missing volume value");
            }
        } else if packet.is_type("cconnect.audiostream.volume_changed") {
            // Remote notifies of volume change
            let direction: StreamDirection = packet
                .body
                .get("direction")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or(StreamDirection::Output);

            if let Some(volume) = packet.body.get("volume").and_then(|v| v.as_f64()) {
                // Update local state to reflect remote volume
                match direction {
                    StreamDirection::Output => {
                        if let Some(stream) = self.outgoing_stream.write().await.as_mut() {
                            stream.volume = volume as f32;
                        }
                    }
                    StreamDirection::Input => {
                        if let Some(stream) = self.incoming_stream.write().await.as_mut() {
                            stream.volume = volume as f32;
                        }
                    }
                }

                info!(
                    "Remote {:?} stream volume changed to {:.2}",
                    direction, volume
                );
            } else {
                warn!("Volume changed packet missing volume value");
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
        vec![
            INCOMING_CAPABILITY.to_string(),
            "kdeconnect.audiostream".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![OUTGOING_CAPABILITY.to_string()]
    }
}

#[cfg(all(test, feature = "audiostream"))]
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
            codec: AudioCodec::Pcm, // Use PCM which is always available
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
            codec: AudioCodec::Pcm, // Use PCM which is always available
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
            codec: AudioCodec::Pcm, // Use PCM which is always available
            sample_rate: 48000,
            ..Default::default()
        };

        plugin.start_stream(config).await.unwrap();

        let new_config = StreamConfig {
            direction: StreamDirection::Output,
            codec: AudioCodec::Pcm, // Use PCM which is always available
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

        // PCM should always be supported with audiostream feature
        #[cfg(feature = "audiostream")]
        {
            assert!(plugin.is_codec_supported(AudioCodec::Pcm));

            let mut expected_codecs = 1;

            #[cfg(feature = "opus")]
            {
                assert!(plugin.is_codec_supported(AudioCodec::Opus));
                expected_codecs += 1;
            }

            #[cfg(not(feature = "opus"))]
            {
                assert!(!plugin.is_codec_supported(AudioCodec::Opus));
            }

            #[cfg(feature = "aac")]
            {
                assert!(plugin.is_codec_supported(AudioCodec::Aac));
                expected_codecs += 1;
            }

            #[cfg(not(feature = "aac"))]
            {
                assert!(!plugin.is_codec_supported(AudioCodec::Aac));
            }

            assert_eq!(plugin.supported_codecs().len(), expected_codecs);
        }
    }

    #[tokio::test]
    async fn test_stream_stats() {
        let mut plugin = AudioStreamPlugin::new();
        plugin.enabled = true;

        let config = StreamConfig {
            direction: StreamDirection::Output,
            codec: AudioCodec::Pcm, // Use PCM which is always available
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

        plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .unwrap();
        plugin.start().await.unwrap();

        let config = StreamConfig {
            codec: AudioCodec::Pcm, // Use PCM which is always available
            ..Default::default()
        };
        let body = serde_json::to_value(&config).unwrap();

        let packet = Packet::new("cconnect.audiostream.start", body);

        assert!(plugin.handle_packet(&packet, &mut device).await.is_ok());
    }

    #[tokio::test]
    async fn test_handle_stop_packet() {
        let mut device = create_test_device();
        let factory = AudioStreamPluginFactory;
        let mut plugin = factory.create();

        plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .unwrap();
        plugin.start().await.unwrap();

        // Start a stream first
        let config = StreamConfig {
            direction: StreamDirection::Output,
            codec: AudioCodec::Pcm, // Use PCM which is always available
            ..Default::default()
        };

        // Need to downcast to access start_stream
        let audio_plugin = plugin
            .as_any_mut()
            .downcast_mut::<AudioStreamPlugin>()
            .unwrap();
        audio_plugin.start_stream(config).await.unwrap();

        // Now stop it
        let mut body = serde_json::Map::new();
        body.insert(
            "direction".to_string(),
            serde_json::to_value(StreamDirection::Output).unwrap(),
        );

        let packet = Packet::new("cconnect.audiostream.stop", serde_json::Value::Object(body));

        assert!(plugin.handle_packet(&packet, &mut device).await.is_ok());

        // Verify stream is stopped
        let outgoing_plugin = plugin.as_any().downcast_ref::<AudioStreamPlugin>().unwrap();
        assert!(outgoing_plugin.outgoing_stream.read().await.is_none());
    }

    #[tokio::test]
    async fn test_volume_control() {
        let mut plugin = AudioStreamPlugin::new();
        plugin.enabled = true;

        let config = StreamConfig {
            direction: StreamDirection::Output,
            codec: AudioCodec::Pcm,
            ..Default::default()
        };

        plugin.start_stream(config).await.unwrap();

        // Check default volume
        let volume = plugin.get_volume(StreamDirection::Output).await;
        assert_eq!(volume, Some(1.0));

        // Set volume to 50%
        plugin
            .set_volume(StreamDirection::Output, 0.5)
            .await
            .unwrap();

        let volume = plugin.get_volume(StreamDirection::Output).await;
        assert_eq!(volume, Some(0.5));

        // Test volume clamping
        plugin
            .set_volume(StreamDirection::Output, 1.5)
            .await
            .unwrap();

        let volume = plugin.get_volume(StreamDirection::Output).await;
        assert_eq!(volume, Some(1.0));

        plugin
            .set_volume(StreamDirection::Output, -0.5)
            .await
            .unwrap();

        let volume = plugin.get_volume(StreamDirection::Output).await;
        assert_eq!(volume, Some(0.0));
    }

    #[tokio::test]
    async fn test_volume_control_no_stream() {
        let mut plugin = AudioStreamPlugin::new();
        plugin.enabled = true;

        // Try to get volume with no stream
        let volume = plugin.get_volume(StreamDirection::Output).await;
        assert_eq!(volume, None);

        // Try to set volume with no stream (should not panic)
        assert!(plugin
            .set_volume(StreamDirection::Output, 0.5)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_handle_volume_packet() {
        let mut device = create_test_device();
        let factory = AudioStreamPluginFactory;
        let mut plugin = factory.create();

        let (tx, mut rx) = tokio::sync::mpsc::channel(100);

        plugin.init(&device, tx).await.unwrap();
        plugin.start().await.unwrap();

        // Start a stream first
        let config = StreamConfig {
            direction: StreamDirection::Output,
            codec: AudioCodec::Pcm,
            ..Default::default()
        };

        let audio_plugin = plugin
            .as_any_mut()
            .downcast_mut::<AudioStreamPlugin>()
            .unwrap();
        audio_plugin.start_stream(config).await.unwrap();

        // Send volume packet
        let mut body = serde_json::Map::new();
        body.insert(
            "direction".to_string(),
            serde_json::to_value(StreamDirection::Output).unwrap(),
        );
        body.insert("volume".to_string(), serde_json::Value::from(0.7));

        let packet = Packet::new(
            "cconnect.audiostream.volume",
            serde_json::Value::Object(body),
        );

        assert!(plugin.handle_packet(&packet, &mut device).await.is_ok());

        // Verify volume was set
        let audio_plugin = plugin.as_any().downcast_ref::<AudioStreamPlugin>().unwrap();
        let volume = audio_plugin.get_volume(StreamDirection::Output).await;
        assert_eq!(volume, Some(0.7));

        // Verify volume_changed packet was sent
        let sent_packet = rx.try_recv();
        assert!(sent_packet.is_ok());

        let (_, packet) = sent_packet.unwrap();
        assert_eq!(packet.packet_type, "cconnect.audiostream.volume_changed");
    }

    #[tokio::test]
    async fn test_handle_volume_changed_packet() {
        let mut device = create_test_device();
        let factory = AudioStreamPluginFactory;
        let mut plugin = factory.create();

        plugin
            .init(&device, tokio::sync::mpsc::channel(100).0)
            .await
            .unwrap();
        plugin.start().await.unwrap();

        // Start a stream first
        let config = StreamConfig {
            direction: StreamDirection::Input,
            codec: AudioCodec::Pcm,
            ..Default::default()
        };

        let audio_plugin = plugin
            .as_any_mut()
            .downcast_mut::<AudioStreamPlugin>()
            .unwrap();
        audio_plugin.start_stream(config).await.unwrap();

        // Send volume_changed packet from remote
        let mut body = serde_json::Map::new();
        body.insert(
            "direction".to_string(),
            serde_json::to_value(StreamDirection::Input).unwrap(),
        );
        body.insert("volume".to_string(), serde_json::Value::from(0.3));

        let packet = Packet::new(
            "cconnect.audiostream.volume_changed",
            serde_json::Value::Object(body),
        );

        assert!(plugin.handle_packet(&packet, &mut device).await.is_ok());

        // Verify volume was updated
        let audio_plugin = plugin.as_any().downcast_ref::<AudioStreamPlugin>().unwrap();
        let volume = audio_plugin.get_volume(StreamDirection::Input).await;
        assert_eq!(volume, Some(0.3));
    }
}
