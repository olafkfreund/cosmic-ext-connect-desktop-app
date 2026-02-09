//! `PipeWire` stream handling for video frames
//!
//! This module provides integration with `PipeWire` to receive raw video frames
//! from the screen capture session.

use crate::capture::{BufferType, VideoFrame};
use crate::error::Result;
use pipewire as pw;
use pipewire::context::Context;
use pipewire::main_loop::MainLoop;
use pipewire::properties::properties;
use pipewire::spa::buffer::DataType;
use pipewire::spa::param::ParamType;
use pipewire::spa::utils::Direction;
use pipewire::stream::{Stream, StreamFlags, StreamState};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// `PipeWire` stream wrapper for receiving video frames
pub struct PipeWireStream {
    /// `PipeWire` node ID
    node_id: u32,

    /// Whether the stream is connected
    connected: Arc<AtomicBool>,

    /// Flag to signal stream thread to stop
    running: Arc<AtomicBool>,

    /// Background thread handle
    thread_handle: Option<std::thread::JoinHandle<()>>,

    /// Stream properties (cached)
    properties: Arc<std::sync::Mutex<Option<StreamProperties>>>,
}

/// Buffer mode negotiated with `PipeWire`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferMode {
    /// Shared memory buffers (default, always works)
    Shm,
    /// DMA-BUF buffers (zero-copy, requires GPU support)
    DmaBuf,
}

/// Stream properties extracted from `PipeWire`
#[derive(Debug, Clone)]
pub struct StreamProperties {
    /// Video width in pixels
    pub width: u32,

    /// Video height in pixels
    pub height: u32,

    /// Video format (e.g., "`BGRx`", "`RGBx`")
    pub format: String,

    /// Framerate
    pub framerate: u32,

    /// Buffer mode negotiated with `PipeWire`
    pub buffer_mode: BufferMode,

    /// DRM fourcc format code (if DMA-BUF)
    pub drm_format: Option<u32>,

    /// DRM format modifier (if DMA-BUF)
    pub modifier: Option<u64>,
}

impl PipeWireStream {
    /// Connect to a `PipeWire` node
    ///
    /// # Arguments
    ///
    /// * `node_id` - `PipeWire` node ID from the portal session
    /// * `frame_sender` - Channel to send captured frames
    ///
    /// # Returns
    ///
    /// A connected `PipeWire` stream ready to receive frames
    pub async fn connect(node_id: u32, frame_sender: mpsc::Sender<VideoFrame>) -> Result<Self> {
        info!("Connecting to PipeWire node: {}", node_id);

        let connected = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(true));
        let properties = Arc::new(std::sync::Mutex::new(None));

        let running_clone = running.clone();
        let connected_clone = connected.clone();

        // Spawn PipeWire thread
        let thread_handle = std::thread::spawn(move || {
            if let Err(e) = run_pipewire_loop(node_id, frame_sender, running_clone, connected_clone)
            {
                error!("PipeWire loop error: {}", e);
            }
        });

        // Wait briefly for connection
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        Ok(Self {
            node_id,
            connected,
            running,
            thread_handle: Some(thread_handle),
            properties,
        })
    }

    /// Disconnect from the `PipeWire` stream
    pub fn disconnect(&mut self) -> Result<()> {
        info!("Disconnecting from PipeWire node: {}", self.node_id);

        // Signal thread to stop
        self.running.store(false, Ordering::SeqCst);

        // Wait for thread to finish
        if let Some(handle) = self.thread_handle.take() {
            handle.join().ok();
        }

        self.connected.store(false, Ordering::SeqCst);
        if let Ok(mut props) = self.properties.lock() {
            *props = None;
        }

        debug!("PipeWire stream disconnected");
        Ok(())
    }

    /// Check if the stream is connected
    #[must_use] 
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Get the current stream properties
    #[must_use] 
    pub fn properties(&self) -> Option<StreamProperties> {
        self.properties.lock().ok().and_then(|p| p.clone())
    }
}

/// Run the `PipeWire` main loop (called from background thread)
#[allow(clippy::needless_pass_by_value, clippy::too_many_lines)]
fn run_pipewire_loop(
    node_id: u32,
    frame_sender: mpsc::Sender<VideoFrame>,
    running: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
) -> Result<()> {
    // Initialize PipeWire
    pw::init();

    // Create main loop
    let mainloop = MainLoop::new(None).map_err(|e| {
        crate::error::DisplayStreamError::PipeWire(format!(
            "Failed to create PipeWire main loop: {e}"
        ))
    })?;

    let loop_ = mainloop.loop_();

    // Create context
    let context = Context::new(&mainloop).map_err(|e| {
        crate::error::DisplayStreamError::PipeWire(format!("Failed to create context: {e}"))
    })?;

    // Connect to PipeWire server
    let core = context.connect(None).map_err(|e| {
        crate::error::DisplayStreamError::PipeWire(format!("Failed to connect to PipeWire: {e}"))
    })?;

    // Create stream
    let stream = Stream::new(
        &core,
        "cosmic-display-stream",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|e| {
        crate::error::DisplayStreamError::PipeWire(format!("Failed to create stream: {e}"))
    })?;

    // Frame counter for sequencing
    let frame_sequence = Arc::new(AtomicU64::new(0));
    let frame_sequence_clone = frame_sequence.clone();

    // Stream properties for frame creation
    let stream_width = Arc::new(std::sync::atomic::AtomicU32::new(1920));
    let stream_height = Arc::new(std::sync::atomic::AtomicU32::new(1080));
    let stream_width_clone = stream_width.clone();
    let stream_height_clone = stream_height.clone();

    let connected_clone = connected.clone();
    let running_clone = running.clone();

    // Add stream listener
    let _listener = stream
        .add_local_listener_with_user_data(frame_sender)
        .state_changed(move |_stream, _user_data, old, new| {
            debug!("Stream state changed: {:?} -> {:?}", old, new);
            if new == StreamState::Streaming {
                connected_clone.store(true, Ordering::SeqCst);
            }
        })
        .param_changed(move |_stream, _user_data, id, param| {
            // Parse video format from params
            if id == ParamType::Format.as_raw() {
                if let Some(pod) = param {
                    let pod_size = pod.size();
                    debug!("Format param changed, pod size: {}", pod_size);

                    // Try to detect if DMA-BUF was negotiated
                    // The actual format detection would parse the SPA pod structure
                    // For now, we'll detect buffer type in the process callback
                    let raw_data = unsafe {
                        std::slice::from_raw_parts(
                            pod.as_raw_ptr() as *const u8,
                            pod_size as usize,
                        )
                    };

                    debug!("Received format negotiation data ({} bytes)", raw_data.len());

                    // Update stream dimensions from format if possible
                    // This is where we'd parse width/height/format from the SPA pod
                    // Full implementation would use libspa format parsing utilities
                }
            }
        })
        .process(move |stream, frame_tx| {
            // Check if we should still be running
            if !running_clone.load(Ordering::SeqCst) {
                return;
            }

            // Dequeue buffer
            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                if let Some(data) = datas.first_mut() {
                    let chunk = data.chunk();
                    // These casts are safe: u32 always fits in usize on 32/64-bit
                    let offset = usize::try_from(chunk.offset()).unwrap_or(0);
                    let size = usize::try_from(chunk.size()).unwrap_or(0);
                    // Stride can be i32 (negative for bottom-up images), use absolute value
                    let stride = usize::try_from(chunk.stride().unsigned_abs()).unwrap_or(0);

                    // Check buffer type from data type
                    let data_type = data.type_();
                    let is_dmabuf = data_type == DataType::DmaBuf;

                    if is_dmabuf {
                        // DMA-BUF path: extract fd instead of copying data
                        debug!("Processing DMA-BUF frame");
                        let raw_data = data.as_raw();
                        // The fd field is i64 in spa_data struct
                        let fd_i64 = raw_data.fd;

                        // Only process if we have a valid fd (>= 0)
                        if fd_i64 >= 0 {
                            #[allow(clippy::cast_possible_truncation)]
                            let fd_raw = fd_i64 as i32;
                            let width = stream_width_clone.load(Ordering::Relaxed);
                            let height = stream_height_clone.load(Ordering::Relaxed);
                            let seq = frame_sequence_clone.fetch_add(1, Ordering::Relaxed);

                            let frame = VideoFrame {
                                data: Vec::new(), // No CPU copy for DMA-BUF
                                width,
                                height,
                                format: "DMA-BUF".to_string(),
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| i64::try_from(d.as_micros()).unwrap_or(i64::MAX))
                                    .unwrap_or(0),
                                sequence: seq,
                                buffer_type: BufferType::DmaBuf {
                                    fd: fd_raw,
                                    stride: u32::try_from(stride).unwrap_or(0),
                                    offset: u32::try_from(offset).unwrap_or(0),
                                    modifier: 0, // Will be set from format negotiation
                                    drm_format: 0, // Will be set from format negotiation
                                },
                            };

                            if let Err(e) = frame_tx.try_send(frame) {
                                if matches!(e, mpsc::error::TrySendError::Full(_)) {
                                    debug!("Frame channel full, dropping DMA-BUF frame");
                                } else {
                                    warn!("Failed to send DMA-BUF frame: {}", e);
                                }
                            }
                            return; // DMA-BUF path done
                        }
                        debug!("DMA-BUF buffer has invalid fd ({}), falling back to SHM", fd_i64);
                    }

                    // SHM path (existing code, unchanged)
                    if let Some(slice) = data.data() {
                        if size > 0 && offset + size <= slice.len() {
                            let frame_data = slice[offset..offset + size].to_vec();

                            let width = stream_width_clone.load(Ordering::Relaxed);
                            let height = stream_height_clone.load(Ordering::Relaxed);
                            let seq = frame_sequence_clone.fetch_add(1, Ordering::Relaxed);

                            // Infer dimensions from stride if needed
                            // Dimensions from video frames are always within u32 range
                            let inferred_width = if stride > 0 {
                                u32::try_from(stride / 4).unwrap_or(width)
                            } else {
                                width
                            };
                            let inferred_height = if size > 0 && stride > 0 {
                                u32::try_from(size / stride).unwrap_or(height)
                            } else {
                                height
                            };

                            let frame = VideoFrame::new(
                                frame_data,
                                inferred_width,
                                inferred_height,
                                "BGRx".to_string(), // Most common format from screen capture
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| i64::try_from(d.as_micros()).unwrap_or(i64::MAX))
                                    .unwrap_or(0),
                                seq,
                            );

                            // Try to send frame (non-blocking)
                            if let Err(e) = frame_tx.try_send(frame) {
                                if matches!(e, mpsc::error::TrySendError::Full(_)) {
                                    debug!("Frame channel full, dropping frame");
                                } else {
                                    warn!("Failed to send frame: {}", e);
                                }
                            }
                        }
                    }
                }
            }
        })
        .register()
        .map_err(|e| {
            crate::error::DisplayStreamError::PipeWire(format!(
                "Failed to register listener: {e}"
            ))
        })?;

    // Connect to the portal's PipeWire node
    stream
        .connect(
            Direction::Input,
            Some(node_id),
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
            &mut [],
        )
        .map_err(|e| {
            crate::error::DisplayStreamError::PipeWire(format!(
                "Failed to connect stream to node {node_id}: {e}"
            ))
        })?;

    info!("PipeWire stream connected to node {}", node_id);

    // Run the main loop until stopped
    while running.load(Ordering::SeqCst) {
        // Iterate the loop with a timeout
        loop_.iterate(std::time::Duration::from_millis(100));
    }

    info!("PipeWire main loop exited");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_properties() {
        let props = StreamProperties {
            width: 1920,
            height: 1080,
            format: "BGRx".to_string(),
            framerate: 60,
            buffer_mode: BufferMode::Shm,
            drm_format: None,
            modifier: None,
        };
        assert_eq!(props.width, 1920);
        assert_eq!(props.height, 1080);
        assert_eq!(props.format, "BGRx");
        assert_eq!(props.framerate, 60);
        assert_eq!(props.buffer_mode, BufferMode::Shm);
        assert_eq!(props.drm_format, None);
        assert_eq!(props.modifier, None);
    }

    #[test]
    fn test_buffer_mode() {
        let shm_mode = BufferMode::Shm;
        let dmabuf_mode = BufferMode::DmaBuf;

        assert_eq!(shm_mode, BufferMode::Shm);
        assert_eq!(dmabuf_mode, BufferMode::DmaBuf);
        assert_ne!(shm_mode, dmabuf_mode);
    }

    #[test]
    fn test_stream_properties_with_dmabuf() {
        let props = StreamProperties {
            width: 1920,
            height: 1080,
            format: "DMA-BUF".to_string(),
            framerate: 60,
            buffer_mode: BufferMode::DmaBuf,
            drm_format: Some(0x34325258), // DRM_FORMAT_XRGB8888
            modifier: Some(0x0100000000000001), // Example modifier
        };
        assert_eq!(props.buffer_mode, BufferMode::DmaBuf);
        assert_eq!(props.drm_format, Some(0x34325258));
        assert!(props.modifier.is_some());
    }
}
