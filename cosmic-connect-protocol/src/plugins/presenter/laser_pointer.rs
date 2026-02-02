//! Laser Pointer Overlay
//!
//! Provides visual laser pointer indicator for presentation mode using Wayland layer-shell.
//!
//! ## Architecture
//!
//! This implementation uses:
//! - Wayland layer-shell protocol (`zwlr_layer_shell_v1`) for overlay surface
//! - `smithay-client-toolkit` for Wayland client management
//! - `tiny-skia` for rendering the colored dot
//! - Separate thread for Wayland event loop to avoid blocking
//!
//! ## Features
//!
//! - Configurable pointer color and size
//! - Smooth position updates
//! - Clean lifecycle management
//! - Thread-safe operation

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use tracing::{debug, error, info, warn};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
    Connection, QueueHandle,
};

/// Laser pointer color (RGBA)
#[derive(Debug, Clone, Copy)]
pub struct LaserPointerColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Default for LaserPointerColor {
    fn default() -> Self {
        // Default to semi-transparent red
        Self {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 0.8,
        }
    }
}

/// Laser pointer configuration
#[derive(Debug, Clone)]
pub struct LaserPointerConfig {
    /// Pointer dot radius in pixels
    pub radius: f32,
    /// Pointer color
    pub color: LaserPointerColor,
    /// Fade out after inactivity (milliseconds)
    pub fade_timeout_ms: u64,
}

impl Default for LaserPointerConfig {
    fn default() -> Self {
        Self {
            radius: 20.0,
            color: LaserPointerColor::default(),
            fade_timeout_ms: 2000,
        }
    }
}

/// Shared state between main thread and Wayland thread
#[derive(Clone)]
struct SharedState {
    position: Arc<Mutex<(f64, f64)>>,
    config: Arc<Mutex<LaserPointerConfig>>,
    needs_redraw: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
}

impl SharedState {
    fn new(config: LaserPointerConfig) -> Self {
        Self {
            position: Arc::new(Mutex::new((0.0, 0.0))),
            config: Arc::new(Mutex::new(config)),
            needs_redraw: Arc::new(AtomicBool::new(false)),
            active: Arc::new(AtomicBool::new(false)),
        }
    }

    fn set_position(&self, x: f64, y: f64) {
        if let Ok(mut pos) = self.position.lock() {
            *pos = (x, y);
            self.needs_redraw.store(true, Ordering::Relaxed);
        }
    }

    fn get_position(&self) -> (f64, f64) {
        self.position
            .lock()
            .unwrap_or_else(|_| panic!("Failed to lock position"))
            .clone()
    }

    fn set_config(&self, config: LaserPointerConfig) {
        if let Ok(mut cfg) = self.config.lock() {
            *cfg = config;
            self.needs_redraw.store(true, Ordering::Relaxed);
        }
    }

    fn get_config(&self) -> LaserPointerConfig {
        self.config
            .lock()
            .unwrap_or_else(|_| panic!("Failed to lock config"))
            .clone()
    }
}

/// Wayland application state
struct LaserPointerApp {
    registry_state: RegistryState,
    output_state: OutputState,
    compositor_state: CompositorState,
    shm_state: Shm,
    layer_shell: LayerShell,
    layer_surface: Option<LayerSurface>,
    shared_state: SharedState,
    pool: Option<SlotPool>,
}

impl LaserPointerApp {
    fn new(
        registry_state: RegistryState,
        output_state: OutputState,
        compositor_state: CompositorState,
        shm_state: Shm,
        layer_shell: LayerShell,
        shared_state: SharedState,
    ) -> Self {
        Self {
            registry_state,
            output_state,
            compositor_state,
            shm_state,
            layer_shell,
            layer_surface: None,
            shared_state,
            pool: None,
        }
    }

    fn create_layer_surface(&mut self, qh: &QueueHandle<Self>) {
        let surface = self.compositor_state.create_surface(qh);

        let layer_surface = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("cosmic-connect-laser-pointer"),
            None,
        );

        // Configure the layer surface
        layer_surface.set_anchor(Anchor::empty());
        layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer_surface.set_exclusive_zone(-1);

        let config = self.shared_state.get_config();
        let size = (config.radius * 2.0).ceil() as u32;
        layer_surface.set_size(size, size);

        layer_surface.commit();
        self.layer_surface = Some(layer_surface);
    }

    fn draw(&mut self, _qh: &QueueHandle<Self>) {
        let Some(layer_surface) = &self.layer_surface else {
            return;
        };

        let config = self.shared_state.get_config();
        let size = (config.radius * 2.0).ceil() as u32;
        let stride = size * 4;
        let buffer_size = (stride * size) as usize;

        // Create pool if it doesn't exist
        if self.pool.is_none() {
            self.pool = Some(
                SlotPool::new(buffer_size * 2, &self.shm_state).expect("Failed to create pool"),
            );
        }

        let pool = self.pool.as_mut().unwrap();

        // Resize pool if needed
        if pool.len() < buffer_size {
            pool.resize(buffer_size * 2).expect("Failed to resize pool");
        }

        // Create a slot for our buffer
        let (buffer, canvas) = pool
            .create_buffer(
                size as i32,
                size as i32,
                stride as i32,
                wl_shm::Format::Argb8888,
            )
            .expect("Failed to create buffer");

        // Render to canvas
        Self::render_pointer(canvas, size, &config);

        // Attach and commit
        let wl_buffer = buffer.wl_buffer();
        layer_surface.wl_surface().attach(Some(wl_buffer), 0, 0);
        layer_surface
            .wl_surface()
            .damage_buffer(0, 0, size as i32, size as i32);
        layer_surface.wl_surface().commit();

        self.shared_state
            .needs_redraw
            .store(false, Ordering::Relaxed);
    }

    fn render_pointer(canvas: &mut [u8], size: u32, config: &LaserPointerConfig) {
        let width = size;
        let height = size;
        let mut pixmap = tiny_skia::PixmapMut::from_bytes(canvas, width, height)
            .expect("Failed to create pixmap");

        // Clear to transparent
        pixmap.fill(tiny_skia::Color::TRANSPARENT);

        // Draw circle
        let center_x = config.radius;
        let center_y = config.radius;

        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(
            (config.color.r * 255.0) as u8,
            (config.color.g * 255.0) as u8,
            (config.color.b * 255.0) as u8,
            (config.color.a * 255.0) as u8,
        );
        paint.anti_alias = true;

        let path = {
            let mut pb = tiny_skia::PathBuilder::new();
            pb.push_circle(center_x, center_y, config.radius);
            pb.finish().expect("Failed to build circle path")
        };

        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    fn update_position(&mut self) {
        let Some(layer_surface) = &self.layer_surface else {
            return;
        };

        let (x, y) = self.shared_state.get_position();
        let config = self.shared_state.get_config();
        let offset = config.radius as i32;

        // Set position relative to pointer coordinates
        layer_surface.set_anchor(Anchor::empty());
        layer_surface.set_margin(y as i32 - offset, 0, 0, x as i32 - offset);
        layer_surface.commit();
    }
}

impl CompositorHandler for LaserPointerApp {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
        // Handle scale changes if needed
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        if self.shared_state.needs_redraw.load(Ordering::Relaxed) {
            self.update_position();
            self.draw(qh);
        }
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
        // Handle transform changes if needed
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
        // Surface entered an output
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
        // Surface left an output
    }
}

impl OutputHandler for LaserPointerApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for LaserPointerApp {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.layer_surface = None;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        _configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        if self.layer_surface.is_some() {
            self.draw(qh);
        }
    }
}

impl ShmHandler for LaserPointerApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm_state
    }
}

impl ProvidesRegistryState for LaserPointerApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState];
}

delegate_compositor!(LaserPointerApp);
delegate_output!(LaserPointerApp);
delegate_shm!(LaserPointerApp);
delegate_layer!(LaserPointerApp);
delegate_registry!(LaserPointerApp);

/// Laser pointer overlay controller
pub struct LaserPointer {
    shared_state: SharedState,
    wayland_thread: Option<thread::JoinHandle<()>>,
}

impl LaserPointer {
    /// Create a new laser pointer overlay
    pub fn new() -> Self {
        Self::with_config(LaserPointerConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: LaserPointerConfig) -> Self {
        info!(
            "Laser pointer overlay initialized (radius: {}px, color: {:?})",
            config.radius, config.color
        );

        let shared_state = SharedState::new(config);

        Self {
            shared_state,
            wayland_thread: None,
        }
    }

    /// Show the laser pointer
    pub fn show(&mut self) {
        if self.shared_state.active.load(Ordering::Relaxed) {
            debug!("Laser pointer already active");
            return;
        }

        info!("Showing laser pointer overlay");
        self.shared_state.active.store(true, Ordering::Relaxed);

        // Start Wayland thread
        let shared_state = self.shared_state.clone();
        self.wayland_thread = Some(thread::spawn(move || {
            if let Err(e) = Self::run_wayland_loop(shared_state) {
                error!("Wayland overlay error: {}", e);
            }
        }));
    }

    /// Hide the laser pointer
    pub fn hide(&mut self) {
        if !self.shared_state.active.load(Ordering::Relaxed) {
            return;
        }

        info!("Hiding laser pointer overlay");
        self.shared_state.active.store(false, Ordering::Relaxed);

        // Wait for thread to finish
        if let Some(handle) = self.wayland_thread.take() {
            let _ = handle.join();
        }
    }

    /// Update laser pointer position with delta movement
    pub fn move_by(&mut self, dx: f64, dy: f64) {
        let (x, y) = self.shared_state.get_position();
        let new_x = x + dx;
        let new_y = y + dy;

        self.shared_state.set_position(new_x, new_y);

        debug!(
            "Laser pointer moved by ({}, {}) to ({}, {})",
            dx, dy, new_x, new_y
        );
    }

    /// Set absolute position
    pub fn set_position(&mut self, x: f64, y: f64) {
        self.shared_state.set_position(x, y);
        debug!("Laser pointer position set to ({}, {})", x, y);
    }

    /// Get current position
    pub fn position(&self) -> (f64, f64) {
        self.shared_state.get_position()
    }

    /// Check if laser pointer is currently active
    pub fn is_active(&self) -> bool {
        self.shared_state.active.load(Ordering::Relaxed)
    }

    /// Get configuration
    pub fn config(&self) -> LaserPointerConfig {
        self.shared_state.get_config()
    }

    /// Update configuration
    pub fn set_config(&mut self, config: LaserPointerConfig) {
        info!("Laser pointer configuration updated");
        self.shared_state.set_config(config);
    }

    fn run_wayland_loop(shared_state: SharedState) -> Result<(), Box<dyn std::error::Error>> {
        let conn = Connection::connect_to_env()?;
        let (globals, mut event_queue): (_, wayland_client::EventQueue<LaserPointerApp>) =
            registry_queue_init(&conn)?;
        let qh: QueueHandle<LaserPointerApp> = event_queue.handle();

        let compositor_state = CompositorState::bind(&globals, &qh)?;
        let output_state = OutputState::new(&globals, &qh);
        let shm_state = Shm::bind(&globals, &qh)?;
        let layer_shell = LayerShell::bind(&globals, &qh)?;

        let registry_state = RegistryState::new(&globals);

        let mut app = LaserPointerApp::new(
            registry_state,
            output_state,
            compositor_state,
            shm_state,
            layer_shell,
            shared_state.clone(),
        );

        app.create_layer_surface(&qh);

        // Event loop
        while shared_state.active.load(Ordering::Relaxed) {
            event_queue.blocking_dispatch(&mut app)?;

            if shared_state.needs_redraw.load(Ordering::Relaxed) {
                app.update_position();
                app.draw(&qh);
            }
        }

        Ok(())
    }
}

impl Default for LaserPointer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for LaserPointer {
    fn drop(&mut self) {
        if self.is_active() {
            warn!("Laser pointer overlay dropped while active");
            self.hide();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_laser_pointer_creation() {
        let pointer = LaserPointer::new();
        assert!(!pointer.is_active());
        assert_eq!(pointer.position(), (0.0, 0.0));
    }

    #[test]
    fn test_show_hide() {
        let mut pointer = LaserPointer::new();

        assert!(!pointer.is_active());

        pointer.show();
        assert!(pointer.is_active());

        pointer.hide();
        assert!(!pointer.is_active());
    }

    #[test]
    fn test_movement() {
        let mut pointer = LaserPointer::new();

        pointer.move_by(10.0, 20.0);
        assert_eq!(pointer.position(), (10.0, 20.0));

        pointer.move_by(-5.0, 15.0);
        assert_eq!(pointer.position(), (5.0, 35.0));
    }

    #[test]
    fn test_set_position() {
        let mut pointer = LaserPointer::new();

        pointer.set_position(100.0, 200.0);
        assert_eq!(pointer.position(), (100.0, 200.0));
    }

    #[test]
    fn test_custom_config() {
        let config = LaserPointerConfig {
            radius: 30.0,
            color: LaserPointerColor {
                r: 0.0,
                g: 1.0,
                b: 0.0,
                a: 1.0,
            },
            fade_timeout_ms: 3000,
        };

        let pointer = LaserPointer::with_config(config.clone());
        assert_eq!(pointer.config().radius, 30.0);
        assert_eq!(pointer.config().fade_timeout_ms, 3000);
    }
}
