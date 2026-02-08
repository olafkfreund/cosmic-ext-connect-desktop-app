//! Audio backend implementation using PipeWire
//!
//! Handles audio capture from microphone/system and playback to speakers.
//!
//! ## Architecture
//!
//! The backend uses PipeWire's stream API to:
//! - Capture audio from default input sources (microphone, monitor)
//! - Play audio to default output sink (speakers)
//! - Manage buffers for low-latency streaming
//! - Handle volume control and device selection
//!
//! Each stream runs in its own thread to avoid blocking the async runtime.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use cosmic_connect_protocol::plugins::audiostream::audio_backend::{AudioBackend, BackendConfig};
//!
//! # async fn example() -> cosmic_connect_protocol::Result<()> {
//! let config = BackendConfig::default();
//! let mut backend = AudioBackend::new(config)?;
//!
//! // Start capturing audio
//! let audio_rx = backend.start_capture()?;
//!
//! // Start playback
//! let audio_tx = backend.start_playback()?;
//!
//! // Audio flows through channels
//! # Ok(())
//! # }
//! ```

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::Result;

#[cfg(not(target_os = "linux"))]
use crate::ProtocolError;

#[cfg(target_os = "linux")]
use pipewire as pw;
#[cfg(target_os = "linux")]
use pipewire::context::Context;
#[cfg(target_os = "linux")]
use pipewire::main_loop::MainLoop;
#[cfg(target_os = "linux")]
use pipewire::properties::properties;
#[cfg(target_os = "linux")]
use pipewire::spa::param::audio::{AudioFormat, AudioInfoRaw};
#[cfg(target_os = "linux")]
use pipewire::spa::param::ParamType;
#[cfg(target_os = "linux")]
use pipewire::spa::pod::Pod;
#[cfg(target_os = "linux")]
use pipewire::spa::utils::{Direction, SpaTypes};
#[cfg(target_os = "linux")]
use pipewire::stream::{Stream, StreamFlags};

/// Audio sample type (f32 for PipeWire)
pub type AudioSample = f32;

/// Audio backend configuration
#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Sample rate in Hz
    pub sample_rate: u32,
    /// Number of channels (1=mono, 2=stereo)
    pub channels: u8,
    /// Buffer size in samples per channel
    pub buffer_size: usize,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            buffer_size: 480, // 10ms at 48kHz
        }
    }
}

/// Audio backend for PipeWire
pub struct AudioBackend {
    config: BackendConfig,

    #[cfg(target_os = "linux")]
    /// Capture stream handle and control
    capture_state: Option<AudioStreamState>,

    #[cfg(target_os = "linux")]
    /// Playback stream handle and control
    playback_state: Option<AudioStreamState>,
}

#[cfg(target_os = "linux")]
struct AudioStreamState {
    running: Arc<AtomicBool>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl AudioBackend {
    /// Create new audio backend
    pub fn new(config: BackendConfig) -> Result<Self> {
        info!(
            "Initializing audio backend: {}Hz, {} channels, {} samples buffer",
            config.sample_rate, config.channels, config.buffer_size
        );

        #[cfg(not(target_os = "linux"))]
        {
            warn!("Audio backend is only supported on Linux with PipeWire");
            return Err(ProtocolError::InvalidPacket(
                "Audio backend not supported on this platform".to_string(),
            ));
        }

        #[cfg(target_os = "linux")]
        {
            // Initialize PipeWire
            pw::init();
            info!("PipeWire initialized for audio backend");
        }

        Ok(Self {
            config,
            #[cfg(target_os = "linux")]
            capture_state: None,
            #[cfg(target_os = "linux")]
            playback_state: None,
        })
    }

    /// Start audio capture from system microphone or monitor
    ///
    /// Returns a channel receiver for captured audio samples.
    ///
    /// ## Implementation
    ///
    /// Creates a PipeWire input stream connected to the default
    /// audio source (microphone) and forwards samples through the channel.
    #[cfg(target_os = "linux")]
    pub fn start_capture(&mut self) -> Result<mpsc::Receiver<Vec<AudioSample>>> {
        let (tx, rx) = mpsc::channel(32);

        info!("Starting audio capture stream");

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let config = self.config.clone();

        // Spawn PipeWire capture thread
        let thread_handle = std::thread::spawn(move || {
            if let Err(e) = run_capture_loop(config, tx, running_clone) {
                error!("Capture loop error: {}", e);
            }
        });

        self.capture_state = Some(AudioStreamState {
            running,
            thread_handle: Some(thread_handle),
        });

        info!("Audio capture stream started");
        Ok(rx)
    }

    /// Start audio capture (non-Linux stub)
    #[cfg(not(target_os = "linux"))]
    pub fn start_capture(&mut self) -> Result<mpsc::Receiver<Vec<AudioSample>>> {
        let (_tx, rx) = mpsc::channel(32);
        warn!("Audio capture is not available on this platform");
        Ok(rx)
    }

    /// Start audio playback to system speakers
    ///
    /// Returns a channel sender for audio samples to play.
    ///
    /// ## Implementation
    ///
    /// Creates a PipeWire output stream connected to the default
    /// audio sink (speakers) and plays samples received through the channel.
    #[cfg(target_os = "linux")]
    pub fn start_playback(&mut self) -> Result<mpsc::Sender<Vec<AudioSample>>> {
        let (tx, rx) = mpsc::channel::<Vec<AudioSample>>(32);

        info!("Starting audio playback stream");

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let config = self.config.clone();

        // Spawn PipeWire playback thread
        let thread_handle = std::thread::spawn(move || {
            if let Err(e) = run_playback_loop(config, rx, running_clone) {
                error!("Playback loop error: {}", e);
            }
        });

        self.playback_state = Some(AudioStreamState {
            running,
            thread_handle: Some(thread_handle),
        });

        info!("Audio playback stream started");
        Ok(tx)
    }

    /// Start audio playback (non-Linux stub)
    #[cfg(not(target_os = "linux"))]
    pub fn start_playback(&mut self) -> Result<mpsc::Sender<Vec<AudioSample>>> {
        let (tx, _rx) = mpsc::channel::<Vec<AudioSample>>(32);
        warn!("Audio playback is not available on this platform");
        Ok(tx)
    }

    /// Get current configuration
    #[allow(dead_code)]
    pub fn config(&self) -> &BackendConfig {
        &self.config
    }

    /// Stop capture stream
    #[cfg(target_os = "linux")]
    pub fn stop_capture(&mut self) {
        if let Some(state) = self.capture_state.take() {
            info!("Stopping audio capture");
            state.running.store(false, Ordering::SeqCst);
            if let Some(handle) = state.thread_handle {
                handle.join().ok();
            }
        }
    }

    /// Stop playback stream
    #[cfg(target_os = "linux")]
    pub fn stop_playback(&mut self) {
        if let Some(state) = self.playback_state.take() {
            info!("Stopping audio playback");
            state.running.store(false, Ordering::SeqCst);
            if let Some(handle) = state.thread_handle {
                handle.join().ok();
            }
        }
    }
}

impl Drop for AudioBackend {
    fn drop(&mut self) {
        debug!("Shutting down audio backend");

        #[cfg(target_os = "linux")]
        {
            self.stop_capture();
            self.stop_playback();
        }
    }
}

/// Run the PipeWire capture loop (called from background thread)
#[cfg(target_os = "linux")]
fn run_capture_loop(
    config: BackendConfig,
    sample_sender: mpsc::Sender<Vec<AudioSample>>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    // Create main loop
    let mainloop = MainLoop::new(None).map_err(|e| {
        crate::ProtocolError::Plugin(format!("Failed to create PipeWire main loop: {}", e))
    })?;

    let loop_ = mainloop.loop_();

    // Create context
    let context = Context::new(&mainloop).map_err(|e| {
        crate::ProtocolError::Plugin(format!("Failed to create PipeWire context: {}", e))
    })?;

    // Connect to PipeWire server
    let core = context.connect(None).map_err(|e| {
        crate::ProtocolError::Plugin(format!("Failed to connect to PipeWire: {}", e))
    })?;

    // Create audio format
    let mut audio_info = AudioInfoRaw::new();
    audio_info.set_format(AudioFormat::F32LE);
    audio_info.set_rate(config.sample_rate);
    audio_info.set_channels(config.channels as u32);

    // Serialize to POD
    let obj = pw::spa::pod::Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .map_err(|e| {
        crate::ProtocolError::Plugin(format!("Failed to serialize audio format: {:?}", e))
    })?
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values).ok_or_else(|| {
        crate::ProtocolError::Plugin("Failed to create POD from bytes".to_string())
    })?];

    // Create capture stream
    let stream = Stream::new(
        &core,
        "cosmic-connect-capture",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Communication",
        },
    )
    .map_err(|e| crate::ProtocolError::Plugin(format!("Failed to create capture stream: {}", e)))?;

    let running_clone = running.clone();

    // Add stream listener
    let _listener = stream
        .add_local_listener_with_user_data(sample_sender)
        .state_changed(|_stream, _user_data, old, new| {
            debug!("Capture stream state changed: {:?} -> {:?}", old, new);
        })
        .process(move |stream, sample_tx| {
            // Check if we should still be running
            if !running_clone.load(Ordering::SeqCst) {
                return;
            }

            // Dequeue buffer
            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                if let Some(data) = datas.first_mut() {
                    let chunk = data.chunk();
                    let size = chunk.size() as usize;

                    if let Some(slice) = data.data() {
                        if size > 0 && size <= slice.len() {
                            // Convert bytes to f32 samples
                            let sample_count = size / std::mem::size_of::<f32>();
                            let mut samples = Vec::with_capacity(sample_count);

                            // Safety: We're reading f32 samples from PipeWire buffer
                            unsafe {
                                let ptr = slice.as_ptr() as *const f32;
                                for i in 0..sample_count {
                                    samples.push(*ptr.add(i));
                                }
                            }

                            // Try to send samples (non-blocking)
                            if let Err(e) = sample_tx.try_send(samples) {
                                match e {
                                    mpsc::error::TrySendError::Full(_) => {
                                        debug!("Sample channel full, dropping frame");
                                    }
                                    mpsc::error::TrySendError::Closed(_) => {
                                        warn!("Sample channel closed");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        })
        .register()
        .map_err(|e| {
            crate::ProtocolError::Plugin(format!("Failed to register capture listener: {}", e))
        })?;

    // Connect stream (will auto-connect to default source)
    stream
        .connect(
            Direction::Input,
            None,
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| {
            crate::ProtocolError::Plugin(format!("Failed to connect capture stream: {}", e))
        })?;

    info!("PipeWire capture stream connected");

    // Run the main loop until stopped
    while running.load(Ordering::SeqCst) {
        loop_.iterate(std::time::Duration::from_millis(10));
    }

    info!("PipeWire capture loop exited");
    Ok(())
}

/// Run the PipeWire playback loop (called from background thread)
#[cfg(target_os = "linux")]
fn run_playback_loop(
    config: BackendConfig,
    mut sample_receiver: mpsc::Receiver<Vec<AudioSample>>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    // Create main loop
    let mainloop = MainLoop::new(None).map_err(|e| {
        crate::ProtocolError::Plugin(format!("Failed to create PipeWire main loop: {}", e))
    })?;

    let loop_ = mainloop.loop_();

    // Create context
    let context = Context::new(&mainloop).map_err(|e| {
        crate::ProtocolError::Plugin(format!("Failed to create PipeWire context: {}", e))
    })?;

    // Connect to PipeWire server
    let core = context.connect(None).map_err(|e| {
        crate::ProtocolError::Plugin(format!("Failed to connect to PipeWire: {}", e))
    })?;

    // Create audio format
    let mut audio_info = AudioInfoRaw::new();
    audio_info.set_format(AudioFormat::F32LE);
    audio_info.set_rate(config.sample_rate);
    audio_info.set_channels(config.channels as u32);

    // Serialize to POD
    let obj = pw::spa::pod::Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .map_err(|e| {
        crate::ProtocolError::Plugin(format!("Failed to serialize audio format: {:?}", e))
    })?
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values).ok_or_else(|| {
        crate::ProtocolError::Plugin("Failed to create POD from bytes".to_string())
    })?];

    // Create playback stream
    let stream = Stream::new(
        &core,
        "cosmic-connect-playback",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::MEDIA_ROLE => "Communication",
        },
    )
    .map_err(|e| {
        crate::ProtocolError::Plugin(format!("Failed to create playback stream: {}", e))
    })?;

    // Buffer for accumulating samples between process calls
    let buffer = Arc::new(std::sync::Mutex::new(Vec::<AudioSample>::new()));
    let buffer_clone = buffer.clone();

    let running_clone = running.clone();

    // Add stream listener with user data
    let _listener = stream
        .add_local_listener_with_user_data(buffer_clone.clone())
        .state_changed(|_stream, _user_data, old, new| {
            debug!("Playback stream state changed: {:?} -> {:?}", old, new);
        })
        .process(move |stream, user_data| {
            // Check if we should still be running
            if !running_clone.load(Ordering::SeqCst) {
                return;
            }

            // Dequeue buffer
            if let Some(mut pw_buffer) = stream.dequeue_buffer() {
                let datas = pw_buffer.datas_mut();
                if let Some(data) = datas.first_mut() {
                    if let Some(slice) = data.data() {
                        let max_size = slice.len();
                        let sample_capacity = max_size / std::mem::size_of::<f32>();
                        // Lock buffer
                        let mut buf = user_data.lock().unwrap();

                        // Determine how many samples we can write
                        let samples_to_write = buf.len().min(sample_capacity);

                        if samples_to_write > 0 {
                            // Safety: Writing f32 samples to PipeWire buffer
                            unsafe {
                                let ptr = slice.as_ptr() as *mut f32;
                                for i in 0..samples_to_write {
                                    *ptr.add(i) = buf[i];
                                }
                            }

                            // Remove written samples from buffer
                            buf.drain(0..samples_to_write);

                            // Update chunk metadata
                            let chunk = data.chunk_mut();
                            *chunk.size_mut() =
                                (samples_to_write * std::mem::size_of::<f32>()) as u32;
                            *chunk.stride_mut() =
                                (std::mem::size_of::<f32>() as i32) * (config.channels as i32);
                        } else {
                            // No data available - write silence
                            let chunk = data.chunk_mut();
                            *chunk.size_mut() = 0;
                        }
                    }
                }
            }
        })
        .register()
        .map_err(|e| {
            crate::ProtocolError::Plugin(format!("Failed to register playback listener: {}", e))
        })?;

    // Connect stream (will auto-connect to default sink)
    stream
        .connect(
            Direction::Output,
            None,
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| {
            crate::ProtocolError::Plugin(format!("Failed to connect playback stream: {}", e))
        })?;

    info!("PipeWire playback stream connected");

    // Spawn task to receive samples and add them to buffer
    let buffer_task = buffer.clone();
    let running_task = running.clone();
    std::thread::spawn(move || {
        while running_task.load(Ordering::SeqCst) {
            match sample_receiver.blocking_recv() {
                Some(samples) => {
                    let mut buf = buffer_task.lock().unwrap();
                    buf.extend_from_slice(&samples);
                }
                None => {
                    debug!("Sample receiver channel closed");
                    break;
                }
            }
        }
    });

    // Run the main loop until stopped
    while running.load(Ordering::SeqCst) {
        loop_.iterate(std::time::Duration::from_millis(10));
    }

    info!("PipeWire playback loop exited");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_config_default() {
        let config = BackendConfig::default();
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.channels, 2);
        assert_eq!(config.buffer_size, 480);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_backend_creation() {
        let config = BackendConfig::default();
        let result = AudioBackend::new(config);
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn test_backend_creation_unsupported() {
        let config = BackendConfig::default();
        let result = AudioBackend::new(config);
        assert!(result.is_err());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_capture_start_stop() {
        let config = BackendConfig {
            sample_rate: 48000,
            channels: 2,
            buffer_size: 480,
        };
        let mut backend = AudioBackend::new(config).unwrap();

        // Start capture
        let _rx = backend.start_capture();
        assert!(backend.capture_state.is_some());

        // Stop capture
        backend.stop_capture();
        assert!(backend.capture_state.is_none());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_playback_start_stop() {
        let config = BackendConfig {
            sample_rate: 48000,
            channels: 2,
            buffer_size: 480,
        };
        let mut backend = AudioBackend::new(config).unwrap();

        // Start playback
        let _tx = backend.start_playback();
        assert!(backend.playback_state.is_some());

        // Stop playback
        backend.stop_playback();
        assert!(backend.playback_state.is_none());
    }

    #[test]
    fn test_config_access() {
        let config = BackendConfig {
            sample_rate: 24000,
            channels: 1,
            buffer_size: 240,
        };

        #[cfg(target_os = "linux")]
        {
            let backend = AudioBackend::new(config.clone()).unwrap();
            assert_eq!(backend.config().sample_rate, 24000);
            assert_eq!(backend.config().channels, 1);
            assert_eq!(backend.config().buffer_size, 240);
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Just verify config structure on non-Linux
            assert_eq!(config.sample_rate, 24000);
            assert_eq!(config.channels, 1);
        }
    }
}
