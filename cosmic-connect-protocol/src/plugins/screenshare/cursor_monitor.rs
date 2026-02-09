//! Lightweight PipeWire cursor metadata monitor
//!
//! Runs alongside the GStreamer capture pipeline to extract `SPA_META_Cursor`
//! metadata from PipeWire buffers. This provides cursor position updates at
//! PipeWire's native rate, independent of the video frame rate.
//!
//! When the portal grants `CursorMode::Metadata`, the cursor is not baked
//! into the video stream. Instead, PipeWire attaches cursor position as
//! buffer metadata that this monitor extracts and sends to viewers.

#[cfg(feature = "screenshare")]
use {
    pipewire::{
        self as pw,
        context::Context,
        main_loop::MainLoop,
        properties::properties,
        spa::{sys as spa_sys, utils::Direction},
        stream::{Stream, StreamFlags, StreamState},
    },
    std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    tracing::{debug, error, info, warn},
};

/// Cursor position update extracted from PipeWire metadata
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorUpdate {
    /// X coordinate on screen
    pub x: i32,
    /// Y coordinate on screen
    pub y: i32,
    /// Whether the cursor is visible (id != 0)
    pub visible: bool,
}

/// SPA_META_Cursor type value (from spa/buffer/meta.h enum)
#[cfg(any(feature = "screenshare", test))]
const SPA_META_CURSOR: u32 = 5;

/// Extract cursor position from a PipeWire buffer's `SPA_META_Cursor` metadata
///
/// # Safety
///
/// The raw `spa_buffer` pointer must be valid for the duration of this call.
/// This is guaranteed when called from within the process callback while the
/// buffer is dequeued.
#[cfg(feature = "screenshare")]
pub(crate) unsafe fn extract_cursor_position(
    spa_buffer: *const spa_sys::spa_buffer,
) -> Option<CursorUpdate> {
    if spa_buffer.is_null() {
        return None;
    }

    let buffer = &*spa_buffer;
    if buffer.n_metas == 0 || buffer.metas.is_null() {
        return None;
    }

    let metas = std::slice::from_raw_parts(buffer.metas, buffer.n_metas as usize);

    for meta in metas {
        if meta.type_ != SPA_META_CURSOR {
            continue;
        }

        if meta.data.is_null()
            || meta.size < std::mem::size_of::<spa_sys::spa_meta_cursor>() as u32
        {
            return None;
        }

        let cursor_meta = &*(meta.data as *const spa_sys::spa_meta_cursor);
        let visible = cursor_meta.id != 0;

        return Some(CursorUpdate {
            x: cursor_meta.position.x,
            y: cursor_meta.position.y,
            visible,
        });
    }

    None
}

/// Lightweight PipeWire stream monitor that extracts cursor metadata
///
/// Connects to the same PipeWire node as GStreamer (via a duplicated fd)
/// but only reads `SPA_META_Cursor` metadata, not pixel data.
#[cfg(feature = "screenshare")]
pub struct CursorMonitor {
    /// Flag to signal the monitor thread to stop
    running: Arc<AtomicBool>,
    /// Background thread handle
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

#[cfg(feature = "screenshare")]
impl CursorMonitor {
    /// Start monitoring cursor metadata from a PipeWire node
    ///
    /// # Arguments
    ///
    /// * `node_id` - PipeWire node ID from the portal session
    /// * `sender` - Channel to send cursor updates
    pub fn start(
        node_id: u32,
        sender: tokio::sync::mpsc::Sender<CursorUpdate>,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let thread_handle = std::thread::spawn(move || {
            if let Err(e) = run_cursor_monitor(node_id, sender, running_clone) {
                error!("Cursor monitor error: {}", e);
            }
        });

        Self {
            running,
            thread_handle: Some(thread_handle),
        }
    }

    /// Stop the cursor monitor
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread_handle.take() {
            handle.join().ok();
        }
        info!("Cursor monitor stopped");
    }
}

#[cfg(feature = "screenshare")]
impl Drop for CursorMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Run the PipeWire main loop for cursor metadata extraction
#[cfg(feature = "screenshare")]
fn run_cursor_monitor(
    node_id: u32,
    sender: tokio::sync::mpsc::Sender<CursorUpdate>,
    running: Arc<AtomicBool>,
) -> crate::Result<()> {
    pw::init();

    let mainloop = MainLoop::new(None).map_err(|e| {
        crate::ProtocolError::Plugin(format!("Cursor monitor: failed to create main loop: {e}"))
    })?;

    let loop_ = mainloop.loop_();

    let context = Context::new(&mainloop).map_err(|e| {
        crate::ProtocolError::Plugin(format!("Cursor monitor: failed to create context: {e}"))
    })?;

    let core = context.connect(None).map_err(|e| {
        crate::ProtocolError::Plugin(format!("Cursor monitor: failed to connect: {e}"))
    })?;

    let stream = Stream::new(
        &core,
        "cosmic-connect-cursor-monitor",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|e| {
        crate::ProtocolError::Plugin(format!("Cursor monitor: failed to create stream: {e}"))
    })?;

    let running_clone = running.clone();

    let _listener = stream
        .add_local_listener_with_user_data(sender)
        .state_changed(move |_stream, _user_data, old, new| {
            debug!("Cursor monitor state: {:?} -> {:?}", old, new);
            if matches!(new, StreamState::Error(_)) {
                warn!("Cursor monitor stream entered error state");
            }
        })
        .process(move |stream, cursor_tx| {
            if !running_clone.load(Ordering::SeqCst) {
                return;
            }

            // Dequeue raw buffer for metadata access
            let raw_pw_buf = unsafe { stream.dequeue_raw_buffer() };
            if raw_pw_buf.is_null() {
                return;
            }

            let spa_buf = unsafe { (*raw_pw_buf).buffer };
            if !spa_buf.is_null() {
                if let Some(update) = unsafe { extract_cursor_position(spa_buf) } {
                    let _ = cursor_tx.try_send(update);
                }
            }

            // Queue buffer back immediately — we don't touch pixel data
            unsafe { stream.queue_raw_buffer(raw_pw_buf) };
        })
        .register()
        .map_err(|e| {
            crate::ProtocolError::Plugin(format!(
                "Cursor monitor: failed to register listener: {e}"
            ))
        })?;

    // Connect to the same portal node
    stream
        .connect(
            Direction::Input,
            Some(node_id),
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
            &mut [],
        )
        .map_err(|e| {
            crate::ProtocolError::Plugin(format!(
                "Cursor monitor: failed to connect to node {node_id}: {e}"
            ))
        })?;

    info!("Cursor monitor connected to PipeWire node {}", node_id);

    // Run until stopped
    while running.load(Ordering::SeqCst) {
        loop_.iterate(std::time::Duration::from_millis(100));
    }

    info!("Cursor monitor main loop exited");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_update_creation() {
        let update = CursorUpdate {
            x: 100,
            y: 200,
            visible: true,
        };
        assert_eq!(update.x, 100);
        assert_eq!(update.y, 200);
        assert!(update.visible);
    }

    #[test]
    fn test_spa_meta_cursor_constant() {
        // SPA_META_Cursor is the 6th value (0-indexed) in the enum:
        // Invalid=0, Header=1, VideoCrop=2, VideoDamage=3, Bitmap=4, Cursor=5
        assert_eq!(SPA_META_CURSOR, 5);
    }

    #[cfg(feature = "screenshare")]
    #[test]
    fn test_extract_cursor_position_null_buffer() {
        let result = unsafe { extract_cursor_position(std::ptr::null()) };
        assert!(result.is_none());
    }

    #[cfg(feature = "screenshare")]
    #[test]
    fn test_extract_cursor_position_no_metas() {
        let buffer = spa_sys::spa_buffer {
            n_metas: 0,
            metas: std::ptr::null_mut(),
            n_datas: 0,
            datas: std::ptr::null_mut(),
        };
        let result = unsafe { extract_cursor_position(&buffer) };
        assert!(result.is_none());
    }

    #[cfg(feature = "screenshare")]
    #[test]
    fn test_extract_cursor_position_valid() {
        let mut cursor_meta = spa_sys::spa_meta_cursor {
            id: 1, // Non-zero = visible
            flags: 0,
            position: spa_sys::spa_point { x: 500, y: 300 },
            hotspot: spa_sys::spa_point { x: 0, y: 0 },
            bitmap_offset: 0,
        };

        let mut meta = spa_sys::spa_meta {
            type_: SPA_META_CURSOR,
            size: std::mem::size_of::<spa_sys::spa_meta_cursor>() as u32,
            data: &mut cursor_meta as *mut _ as *mut std::ffi::c_void,
        };

        let buffer = spa_sys::spa_buffer {
            n_metas: 1,
            metas: &mut meta,
            n_datas: 0,
            datas: std::ptr::null_mut(),
        };

        let result = unsafe { extract_cursor_position(&buffer) };
        assert!(result.is_some());
        let update = result.unwrap();
        assert_eq!(update.x, 500);
        assert_eq!(update.y, 300);
        assert!(update.visible);
    }

    #[cfg(feature = "screenshare")]
    #[test]
    fn test_extract_cursor_position_invisible() {
        let mut cursor_meta = spa_sys::spa_meta_cursor {
            id: 0, // Zero = invisible
            flags: 0,
            position: spa_sys::spa_point { x: 100, y: 200 },
            hotspot: spa_sys::spa_point { x: 0, y: 0 },
            bitmap_offset: 0,
        };

        let mut meta = spa_sys::spa_meta {
            type_: SPA_META_CURSOR,
            size: std::mem::size_of::<spa_sys::spa_meta_cursor>() as u32,
            data: &mut cursor_meta as *mut _ as *mut std::ffi::c_void,
        };

        let buffer = spa_sys::spa_buffer {
            n_metas: 1,
            metas: &mut meta,
            n_datas: 0,
            datas: std::ptr::null_mut(),
        };

        let result = unsafe { extract_cursor_position(&buffer) };
        assert!(result.is_some());
        let update = result.unwrap();
        assert_eq!(update.x, 100);
        assert_eq!(update.y, 200);
        assert!(!update.visible);
    }

    #[cfg(feature = "screenshare")]
    #[test]
    fn test_extract_cursor_position_wrong_meta_type() {
        // Use a different meta type (e.g., SPA_META_Header = 1)
        let mut meta = spa_sys::spa_meta {
            type_: 1, // SPA_META_Header, not Cursor
            size: 64,
            data: std::ptr::null_mut(),
        };

        let buffer = spa_sys::spa_buffer {
            n_metas: 1,
            metas: &mut meta,
            n_datas: 0,
            datas: std::ptr::null_mut(),
        };

        let result = unsafe { extract_cursor_position(&buffer) };
        assert!(result.is_none());
    }
}
