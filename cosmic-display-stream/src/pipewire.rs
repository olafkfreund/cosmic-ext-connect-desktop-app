//! PipeWire stream handling for video frames
//!
//! This module provides integration with PipeWire to receive raw video frames
//! from the screen capture session.

use crate::capture::VideoFrame;
use crate::error::Result;
use pipewire as pw;
use pipewire::context::Context;
use pipewire::main_loop::MainLoop;
use pipewire::properties::properties;
use pipewire::spa::param::ParamType;
use pipewire::spa::utils::Direction;
use pipewire::stream::{Stream, StreamFlags, StreamState};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// PipeWire stream wrapper for receiving video frames
pub struct PipeWireStream {
    /// PipeWire node ID
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

/// Stream properties extracted from PipeWire
#[derive(Debug, Clone)]
pub struct StreamProperties {
    /// Video width in pixels
    pub width: u32,

    /// Video height in pixels
    pub height: u32,

    /// Video format (e.g., "BGRx", "RGBx")
    pub format: String,

    /// Framerate
    pub framerate: u32,
}

impl PipeWireStream {
    /// Connect to a PipeWire node
    ///
    /// # Arguments
    ///
    /// * `node_id` - PipeWire node ID from the portal session
    /// * `frame_sender` - Channel to send captured frames
    ///
    /// # Returns
    ///
    /// A connected PipeWire stream ready to receive frames
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

    /// Disconnect from the PipeWire stream
    pub async fn disconnect(&mut self) -> Result<()> {
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
    pub async fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Get the current stream properties
    pub async fn properties(&self) -> Option<StreamProperties> {
        self.properties.lock().ok().and_then(|p| p.clone())
    }
}

/// Run the PipeWire main loop (called from background thread)
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
            "Failed to create PipeWire main loop: {}",
            e
        ))
    })?;

    let loop_ = mainloop.loop_();

    // Create context
    let context = Context::new(&mainloop).map_err(|e| {
        crate::error::DisplayStreamError::PipeWire(format!("Failed to create context: {}", e))
    })?;

    // Connect to PipeWire server
    let core = context.connect(None).map_err(|e| {
        crate::error::DisplayStreamError::PipeWire(format!("Failed to connect to PipeWire: {}", e))
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
        crate::error::DisplayStreamError::PipeWire(format!("Failed to create stream: {}", e))
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
                    // Try to extract format info from the pod
                    // This is a simplified parser - real implementation would use spa_format_parse
                    debug!("Format param changed, pod size: {}", pod.size());

                    // For now, we'll detect common formats based on the raw data
                    // In production, use proper spa format parsing
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
                    let offset = chunk.offset() as usize;
                    let size = chunk.size() as usize;
                    let stride = chunk.stride() as usize;

                    if let Some(slice) = data.data() {
                        if size > 0 && offset + size <= slice.len() {
                            let frame_data = slice[offset..offset + size].to_vec();

                            let width = stream_width_clone.load(Ordering::Relaxed);
                            let height = stream_height_clone.load(Ordering::Relaxed);
                            let seq = frame_sequence_clone.fetch_add(1, Ordering::Relaxed);

                            // Infer dimensions from stride if needed
                            let inferred_width = if stride > 0 {
                                (stride / 4) as u32
                            } else {
                                width
                            };
                            let inferred_height = if size > 0 && stride > 0 {
                                (size / stride) as u32
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
                                    .map(|d| d.as_micros() as i64)
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
            crate::error::DisplayStreamError::PipeWire(format!("Failed to register listener: {}", e))
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
                "Failed to connect stream to node {}: {}",
                node_id, e
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
        };
        assert_eq!(props.width, 1920);
        assert_eq!(props.height, 1080);
        assert_eq!(props.format, "BGRx");
        assert_eq!(props.framerate, 60);
    }
}
