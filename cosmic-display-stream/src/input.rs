//! Input event handling for remote touch input
//!
//! This module implements touch event reception from Android clients and
//! conversion to desktop pointer events. It handles:
//!
//! 1. Receiving touch events from WebRTC data channels
//! 2. Converting normalized tablet coordinates to desktop coordinates
//! 3. Mapping to virtual display position in desktop space
//! 4. Injecting pointer events using libei/reis through the enigo library
//!
//! ## Input Injection Implementation
//!
//! Touch input is injected into the COSMIC desktop using the `enigo` crate with
//! the `libei_tokio` feature. This uses the libei/reis protocol for Wayland
//! emulated input, which is the modern standard for input injection on Wayland.
//!
//! The implementation:
//! - Lazily initializes enigo on first touch event
//! - Falls back gracefully if libei is not available (test environments)
//! - Converts touch events to absolute pointer positions
//! - Handles multi-touch by tracking active touch points
//!
//! ## Coordinate System
//!
//! Touch events from the tablet use normalized coordinates (0.0-1.0):
//! - (0.0, 0.0) = top-left of tablet screen
//! - (1.0, 1.0) = bottom-right of tablet screen
//!
//! These are converted to desktop coordinates based on the virtual display's
//! position and size in the desktop coordinate space.
//!
//! ## Example Mapping
//!
//! ```text
//! Physical Monitor: 0,0 → 1920,1080
//! Virtual Display:  1920,0 → 4480,1600 (2560px wide, positioned right)
//!
//! Tablet touch at (0.5, 0.5) maps to:
//!   desktop_x = 1920 + (0.5 * 2560) = 3200
//!   desktop_y = 0 + (0.5 * 1600) = 800
//! ```
//!
//! ## Requirements
//!
//! - COSMIC Desktop or other compositor with libei/reis support
//! - Remote Desktop portal access for input injection
//! - Proper permissions for emulated input

use crate::error::{DisplayStreamError, Result};
#[cfg(not(test))]
use enigo::Settings;
use enigo::{Button, Coordinate, Direction, Enigo, Mouse};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tracing::{debug, error, trace, warn};

/// Touch action types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TouchAction {
    /// Touch down (finger/stylus pressed)
    Down,
    /// Touch move (finger/stylus moved while pressed)
    Move,
    /// Touch up (finger/stylus released)
    Up,
    /// Touch cancelled (system cancelled the touch)
    Cancel,
}

/// Touch event from Android client
///
/// Coordinates are normalized to 0.0-1.0 range, where:
/// - x: 0.0 = left edge, 1.0 = right edge
/// - y: 0.0 = top edge, 1.0 = bottom edge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TouchEvent {
    /// Normalized x coordinate (0.0-1.0)
    pub x: f64,
    /// Normalized y coordinate (0.0-1.0)
    pub y: f64,
    /// Touch action type
    pub action: TouchAction,
    /// Touch point identifier (for multi-touch tracking)
    pub touch_id: u32,
    /// Optional pressure value (0.0-1.0)
    #[serde(default)]
    pub pressure: Option<f64>,
    /// Timestamp in milliseconds
    #[serde(default)]
    pub timestamp: Option<u64>,
}

impl TouchEvent {
    /// Create a new touch event
    ///
    /// # Arguments
    ///
    /// * `x` - Normalized x coordinate (0.0-1.0)
    /// * `y` - Normalized y coordinate (0.0-1.0)
    /// * `action` - Touch action type
    /// * `touch_id` - Touch point identifier
    ///
    /// # Example
    ///
    /// ```
    /// use cosmic_display_stream::input::{TouchEvent, TouchAction};
    ///
    /// let event = TouchEvent::new(0.5, 0.5, TouchAction::Down, 0);
    /// assert_eq!(event.x, 0.5);
    /// assert_eq!(event.action, TouchAction::Down);
    /// ```
    #[must_use]
    pub fn new(x: f64, y: f64, action: TouchAction, touch_id: u32) -> Self {
        Self {
            x,
            y,
            action,
            touch_id,
            pressure: None,
            timestamp: None,
        }
    }

    /// Create a new touch event with pressure
    #[must_use]
    pub fn with_pressure(mut self, pressure: f64) -> Self {
        self.pressure = Some(pressure.clamp(0.0, 1.0));
        self
    }

    /// Create a new touch event with timestamp
    #[must_use]
    pub fn with_timestamp(mut self, timestamp: u64) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Validate that coordinates are in valid range
    ///
    /// # Errors
    ///
    /// Returns error if coordinates or pressure are outside the valid range (0.0-1.0)
    pub fn validate(&self) -> Result<()> {
        let x = self.x;
        let y = self.y;
        if !(0.0..=1.0).contains(&x) {
            return Err(DisplayStreamError::Input(format!(
                "Invalid x coordinate: {x} (must be 0.0-1.0)"
            )));
        }
        if !(0.0..=1.0).contains(&y) {
            return Err(DisplayStreamError::Input(format!(
                "Invalid y coordinate: {y} (must be 0.0-1.0)"
            )));
        }
        if let Some(pressure) = self.pressure {
            if !(0.0..=1.0).contains(&pressure) {
                return Err(DisplayStreamError::Input(format!(
                    "Invalid pressure: {pressure} (must be 0.0-1.0)"
                )));
            }
        }
        Ok(())
    }
}

/// Desktop coordinates after mapping from tablet space
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopCoordinates {
    /// Absolute x coordinate in desktop space
    pub x: i32,
    /// Absolute y coordinate in desktop space
    pub y: i32,
}

/// Virtual display geometry in desktop coordinate space
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayGeometry {
    /// Offset from origin (x, y)
    pub offset: (i32, i32),
    /// Size (width, height)
    pub size: (u32, u32),
}

impl DisplayGeometry {
    /// Create new display geometry
    ///
    /// # Arguments
    ///
    /// * `offset_x` - X offset in desktop space
    /// * `offset_y` - Y offset in desktop space
    /// * `width` - Display width in pixels
    /// * `height` - Display height in pixels
    #[must_use]
    pub fn new(offset_x: i32, offset_y: i32, width: u32, height: u32) -> Self {
        Self {
            offset: (offset_x, offset_y),
            size: (width, height),
        }
    }

    /// Get the right edge x coordinate
    #[must_use]
    pub fn right(&self) -> i32 {
        self.offset.0 + i32::try_from(self.size.0).unwrap_or(i32::MAX)
    }

    /// Get the bottom edge y coordinate
    #[must_use]
    pub fn bottom(&self) -> i32 {
        self.offset.1 + i32::try_from(self.size.1).unwrap_or(i32::MAX)
    }

    /// Check if a desktop coordinate is within this display
    #[must_use]
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.offset.0 && x < self.right() && y >= self.offset.1 && y < self.bottom()
    }
}

/// Input handler for touch events
///
/// Manages coordinate conversion and pointer event injection for remote
/// touch input from Android tablets.
pub struct InputHandler {
    /// Virtual display geometry in desktop space
    geometry: DisplayGeometry,

    /// Active touch points (for multi-touch tracking)
    active_touches: std::collections::HashMap<u32, DesktopCoordinates>,

    /// Enigo instance for input injection (wrapped in Arc<Mutex<>> for interior mutability)
    enigo: Arc<Mutex<Option<Enigo>>>,

    /// Statistics
    events_processed: u64,
    events_injected: u64,
    events_failed: u64,
}

impl InputHandler {
    /// Create a new input handler
    ///
    /// # Arguments
    ///
    /// * `display_offset` - Virtual display offset (x, y) in desktop space
    /// * `display_size` - Virtual display size (width, height) in pixels
    ///
    /// # Example
    ///
    /// ```
    /// use cosmic_display_stream::input::InputHandler;
    ///
    /// // Virtual display positioned at (1920, 0) with 2560x1600 resolution
    /// let handler = InputHandler::new((1920, 0), (2560, 1600));
    /// ```
    #[must_use]
    pub fn new(display_offset: (i32, i32), display_size: (u32, u32)) -> Self {
        let geometry = DisplayGeometry {
            offset: display_offset,
            size: display_size,
        };

        debug!(
            "Created input handler: offset=({}, {}), size=({}x{})",
            geometry.offset.0, geometry.offset.1, geometry.size.0, geometry.size.1
        );

        // Initialize enigo with default settings
        // We delay initialization until first use to handle potential errors
        let enigo = Arc::new(Mutex::new(None));

        Self {
            geometry,
            active_touches: std::collections::HashMap::new(),
            enigo,
            events_processed: 0,
            events_injected: 0,
            events_failed: 0,
        }
    }

    /// Initialize the enigo instance for input injection
    ///
    /// This is called lazily on first use. It attempts to create an Enigo
    /// instance with libei support for Wayland.
    ///
    /// # Errors
    ///
    /// Returns error if enigo initialization fails (e.g., no libei support)
    fn ensure_enigo_initialized(&self) -> Result<bool> {
        // mut is needed in non-test builds for the assignment in cfg(not(test)) block
        #[allow(unused_mut)]
        let mut enigo_guard = self.enigo.lock().map_err(|e| {
            DisplayStreamError::Input(format!("Failed to acquire enigo lock: {e}"))
        })?;

        if enigo_guard.is_none() {
            // Skip initialization in test mode to avoid panics from missing portals
            #[cfg(test)]
            {
                warn!("Skipping enigo initialization in test mode");
                return Ok(false);
            }

            #[cfg(not(test))]
            {
                debug!("Initializing enigo for input injection");
                return match Enigo::new(&Settings::default()) {
                    Ok(enigo) => {
                        *enigo_guard = Some(enigo);
                        debug!("Enigo initialized successfully");
                        Ok(true)
                    }
                    Err(e) => {
                        // In environments without compositor support, this is expected
                        warn!(
                            "Failed to initialize enigo: {}. Input injection will be simulated.",
                            e
                        );
                        Ok(false)
                    }
                };
            }
        }
        Ok(true)
    }

    /// Update the virtual display geometry
    ///
    /// Call this when the virtual display is moved or resized.
    ///
    /// # Arguments
    ///
    /// * `display_offset` - New offset (x, y)
    /// * `display_size` - New size (width, height)
    pub fn set_display_geometry(&mut self, display_offset: (i32, i32), display_size: (u32, u32)) {
        self.geometry = DisplayGeometry {
            offset: display_offset,
            size: display_size,
        };

        debug!(
            "Updated display geometry: offset=({}, {}), size=({}x{})",
            self.geometry.offset.0,
            self.geometry.offset.1,
            self.geometry.size.0,
            self.geometry.size.1
        );
    }

    /// Get current display geometry
    #[must_use]
    pub fn display_geometry(&self) -> DisplayGeometry {
        self.geometry
    }

    /// Convert normalized tablet coordinates to desktop coordinates
    ///
    /// # Arguments
    ///
    /// * `normalized_x` - Normalized x coordinate (0.0-1.0)
    /// * `normalized_y` - Normalized y coordinate (0.0-1.0)
    ///
    /// # Returns
    ///
    /// Desktop coordinates in absolute pixel space
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn normalize_to_desktop(&self, normalized_x: f64, normalized_y: f64) -> DesktopCoordinates {
        // Convert normalized coordinates to virtual display space
        let display_x = (normalized_x * f64::from(self.geometry.size.0)).round();
        let display_y = (normalized_y * f64::from(self.geometry.size.1)).round();

        // Add offset to get desktop coordinates
        // Note: These casts are intentional and safe since display coordinates
        // are bounded by display size which fits in i32
        let desktop_x = self.geometry.offset.0 + display_x as i32;
        let desktop_y = self.geometry.offset.1 + display_y as i32;

        trace!(
            "Coordinate conversion: ({:.3}, {:.3}) -> ({}, {}) -> ({}, {})",
            normalized_x,
            normalized_y,
            display_x,
            display_y,
            desktop_x,
            desktop_y
        );

        DesktopCoordinates {
            x: desktop_x,
            y: desktop_y,
        }
    }

    /// Handle a touch event from the tablet
    ///
    /// This converts the normalized coordinates to desktop space and injects
    /// the appropriate pointer event.
    ///
    /// # Arguments
    ///
    /// * `event` - Touch event from the tablet
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Touch coordinates are invalid
    /// - Pointer injection fails
    ///
    /// # Example
    ///
    /// ```
    /// use cosmic_display_stream::input::{InputHandler, TouchEvent, TouchAction};
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut handler = InputHandler::new((1920, 0), (2560, 1600));
    ///
    /// // Touch down at center of screen
    /// let event = TouchEvent::new(0.5, 0.5, TouchAction::Down, 0);
    /// handler.handle_touch_event(&event)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn handle_touch_event(&mut self, event: &TouchEvent) -> Result<()> {
        // Validate event coordinates
        event.validate()?;

        self.events_processed += 1;

        // Convert to desktop coordinates
        let desktop_coords = self.normalize_to_desktop(event.x, event.y);

        // Verify coordinates are within display bounds
        if !self.geometry.contains(desktop_coords.x, desktop_coords.y) {
            warn!(
                "Touch coordinates ({}, {}) outside display bounds ({}, {}) to ({}, {})",
                desktop_coords.x,
                desktop_coords.y,
                self.geometry.offset.0,
                self.geometry.offset.1,
                self.geometry.right(),
                self.geometry.bottom()
            );
        }

        // Handle based on action type
        match event.action {
            TouchAction::Down => {
                self.handle_touch_down(event.touch_id, desktop_coords)?;
            }
            TouchAction::Move => {
                self.handle_touch_move(event.touch_id, desktop_coords)?;
            }
            TouchAction::Up => {
                self.handle_touch_up(event.touch_id, desktop_coords)?;
            }
            TouchAction::Cancel => {
                self.handle_touch_cancel(event.touch_id)?;
            }
        }

        Ok(())
    }

    /// Handle touch down event
    fn handle_touch_down(&mut self, touch_id: u32, coords: DesktopCoordinates) -> Result<()> {
        debug!(
            "Touch down: id={}, coords=({}, {})",
            touch_id, coords.x, coords.y
        );

        // Track active touch
        self.active_touches.insert(touch_id, coords);

        // Inject pointer down event
        self.inject_pointer_down(coords)?;

        self.events_injected += 1;
        Ok(())
    }

    /// Handle touch move event
    fn handle_touch_move(&mut self, touch_id: u32, coords: DesktopCoordinates) -> Result<()> {
        trace!(
            "Touch move: id={}, coords=({}, {})",
            touch_id,
            coords.x,
            coords.y
        );

        // Update tracked position
        if let Some(last_coords) = self.active_touches.get_mut(&touch_id) {
            *last_coords = coords;
        } else {
            warn!("Touch move for untracked touch_id: {}", touch_id);
            // Treat as new touch
            return self.handle_touch_down(touch_id, coords);
        }

        // Inject pointer move event
        self.inject_pointer_move(coords)?;

        self.events_injected += 1;
        Ok(())
    }

    /// Handle touch up event
    fn handle_touch_up(&mut self, touch_id: u32, coords: DesktopCoordinates) -> Result<()> {
        debug!(
            "Touch up: id={}, coords=({}, {})",
            touch_id, coords.x, coords.y
        );

        // Remove from active touches
        self.active_touches.remove(&touch_id);

        // Inject pointer up event
        self.inject_pointer_up(coords)?;

        self.events_injected += 1;
        Ok(())
    }

    /// Handle touch cancel event
    fn handle_touch_cancel(&mut self, touch_id: u32) -> Result<()> {
        debug!("Touch cancel: id={}", touch_id);

        // Remove from active touches
        if let Some(coords) = self.active_touches.remove(&touch_id) {
            // Inject pointer up to clean up
            self.inject_pointer_up(coords)?;
            self.events_injected += 1;
        }

        Ok(())
    }

    /// Inject pointer down event
    ///
    /// Uses enigo to inject a left mouse button press at the specified coordinates.
    /// In test environments where enigo cannot initialize, this logs the action.
    ///
    /// # Errors
    ///
    /// Returns error if injection fails
    fn inject_pointer_down(&mut self, coords: DesktopCoordinates) -> Result<()> {
        let initialized = self.ensure_enigo_initialized()?;

        if !initialized {
            // In test mode or without compositor, just log
            trace!("Simulated pointer down at ({}, {})", coords.x, coords.y);
            return Ok(());
        }

        let mut enigo_guard = self.enigo.lock().map_err(|e| {
            DisplayStreamError::Input(format!("Failed to acquire enigo lock: {e}"))
        })?;

        if let Some(enigo) = enigo_guard.as_mut() {
            // Move to position first (using absolute coordinates)
            if let Err(e) = enigo.move_mouse(coords.x, coords.y, Coordinate::Abs) {
                error!(
                    "Failed to move mouse to ({}, {}): {}",
                    coords.x, coords.y, e
                );
                self.events_failed += 1;
                return Err(DisplayStreamError::Input(format!(
                    "Failed to move mouse: {e}"
                )));
            }

            // Press left button
            if let Err(e) = enigo.button(Button::Left, Direction::Press) {
                error!("Failed to press mouse button: {}", e);
                self.events_failed += 1;
                return Err(DisplayStreamError::Input(format!(
                    "Failed to press mouse button: {e}"
                )));
            }

            trace!("Injected pointer down at ({}, {})", coords.x, coords.y);
            Ok(())
        } else {
            Ok(())
        }
    }

    /// Inject pointer move event
    ///
    /// Uses enigo to move the mouse cursor to the specified coordinates.
    /// In test environments where enigo cannot initialize, this logs the action.
    ///
    /// # Errors
    ///
    /// Returns error if injection fails
    fn inject_pointer_move(&mut self, coords: DesktopCoordinates) -> Result<()> {
        let initialized = self.ensure_enigo_initialized()?;

        if !initialized {
            // In test mode or without compositor, just log
            trace!("Simulated pointer move to ({}, {})", coords.x, coords.y);
            return Ok(());
        }

        let mut enigo_guard = self.enigo.lock().map_err(|e| {
            DisplayStreamError::Input(format!("Failed to acquire enigo lock: {e}"))
        })?;

        if let Some(enigo) = enigo_guard.as_mut() {
            if let Err(e) = enigo.move_mouse(coords.x, coords.y, Coordinate::Abs) {
                error!(
                    "Failed to move mouse to ({}, {}): {}",
                    coords.x, coords.y, e
                );
                self.events_failed += 1;
                return Err(DisplayStreamError::Input(format!(
                    "Failed to move mouse: {e}"
                )));
            }

            trace!("Injected pointer move to ({}, {})", coords.x, coords.y);
            Ok(())
        } else {
            Ok(())
        }
    }

    /// Inject pointer up event
    ///
    /// Uses enigo to release the left mouse button at the specified coordinates.
    /// In test environments where enigo cannot initialize, this logs the action.
    ///
    /// # Errors
    ///
    /// Returns error if injection fails
    fn inject_pointer_up(&mut self, coords: DesktopCoordinates) -> Result<()> {
        let initialized = self.ensure_enigo_initialized()?;

        if !initialized {
            // In test mode or without compositor, just log
            trace!("Simulated pointer up at ({}, {})", coords.x, coords.y);
            return Ok(());
        }

        let mut enigo_guard = self.enigo.lock().map_err(|e| {
            DisplayStreamError::Input(format!("Failed to acquire enigo lock: {e}"))
        })?;

        if let Some(enigo) = enigo_guard.as_mut() {
            // Move to position first (using absolute coordinates)
            if let Err(e) = enigo.move_mouse(coords.x, coords.y, Coordinate::Abs) {
                error!(
                    "Failed to move mouse to ({}, {}): {}",
                    coords.x, coords.y, e
                );
                self.events_failed += 1;
                return Err(DisplayStreamError::Input(format!(
                    "Failed to move mouse: {e}"
                )));
            }

            // Release left button
            if let Err(e) = enigo.button(Button::Left, Direction::Release) {
                error!("Failed to release mouse button: {}", e);
                self.events_failed += 1;
                return Err(DisplayStreamError::Input(format!(
                    "Failed to release mouse button: {e}"
                )));
            }

            trace!("Injected pointer up at ({}, {})", coords.x, coords.y);
            Ok(())
        } else {
            Ok(())
        }
    }

    /// Get number of currently active touches
    #[must_use]
    pub fn active_touch_count(&self) -> usize {
        self.active_touches.len()
    }

    /// Get statistics about processed events
    #[must_use]
    pub fn statistics(&self) -> InputStatistics {
        InputStatistics {
            events_processed: self.events_processed,
            events_injected: self.events_injected,
            events_failed: self.events_failed,
            active_touches: self.active_touches.len(),
        }
    }

    /// Reset statistics counters
    pub fn reset_statistics(&mut self) {
        self.events_processed = 0;
        self.events_injected = 0;
        self.events_failed = 0;
    }
}

/// Input handling statistics
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputStatistics {
    /// Total touch events processed
    pub events_processed: u64,
    /// Touch events successfully injected as pointer events
    pub events_injected: u64,
    /// Touch events that failed to inject
    pub events_failed: u64,
    /// Number of currently active touch points
    pub active_touches: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_touch_event_creation() {
        let event = TouchEvent::new(0.5, 0.5, TouchAction::Down, 0);
        assert_eq!(event.x, 0.5);
        assert_eq!(event.y, 0.5);
        assert_eq!(event.action, TouchAction::Down);
        assert_eq!(event.touch_id, 0);
        assert!(event.pressure.is_none());
        assert!(event.timestamp.is_none());
    }

    #[test]
    fn test_touch_event_with_pressure() {
        let event = TouchEvent::new(0.5, 0.5, TouchAction::Down, 0).with_pressure(0.8);
        assert_eq!(event.pressure, Some(0.8));
    }

    #[test]
    fn test_touch_event_validation() {
        let valid = TouchEvent::new(0.5, 0.5, TouchAction::Down, 0);
        assert!(valid.validate().is_ok());

        let invalid_x = TouchEvent::new(1.5, 0.5, TouchAction::Down, 0);
        assert!(invalid_x.validate().is_err());

        let invalid_y = TouchEvent::new(0.5, -0.1, TouchAction::Down, 0);
        assert!(invalid_y.validate().is_err());
    }

    #[test]
    fn test_display_geometry() {
        let geom = DisplayGeometry::new(1920, 0, 2560, 1600);
        assert_eq!(geom.offset, (1920, 0));
        assert_eq!(geom.size, (2560, 1600));
        assert_eq!(geom.right(), 4480);
        assert_eq!(geom.bottom(), 1600);
    }

    #[test]
    fn test_display_geometry_contains() {
        let geom = DisplayGeometry::new(1920, 0, 2560, 1600);

        // Inside bounds
        assert!(geom.contains(1920, 0));
        assert!(geom.contains(3200, 800));
        assert!(geom.contains(4479, 1599));

        // Outside bounds
        assert!(!geom.contains(1919, 0));
        assert!(!geom.contains(4480, 0));
        assert!(!geom.contains(3200, 1600));
        assert!(!geom.contains(0, 0));
    }

    #[test]
    fn test_coordinate_conversion() {
        let handler = InputHandler::new((1920, 0), (2560, 1600));

        // Top-left corner
        let coords = handler.normalize_to_desktop(0.0, 0.0);
        assert_eq!(coords.x, 1920);
        assert_eq!(coords.y, 0);

        // Center
        let coords = handler.normalize_to_desktop(0.5, 0.5);
        assert_eq!(coords.x, 3200);
        assert_eq!(coords.y, 800);

        // Bottom-right corner (should be just inside bounds)
        let coords = handler.normalize_to_desktop(1.0, 1.0);
        assert_eq!(coords.x, 4480);
        assert_eq!(coords.y, 1600);
    }

    #[test]
    fn test_handle_touch_sequence() {
        let mut handler = InputHandler::new((1920, 0), (2560, 1600));

        // Touch down
        let event = TouchEvent::new(0.5, 0.5, TouchAction::Down, 0);
        assert!(handler.handle_touch_event(&event).is_ok());
        assert_eq!(handler.active_touch_count(), 1);

        // Touch move
        let event = TouchEvent::new(0.6, 0.6, TouchAction::Move, 0);
        assert!(handler.handle_touch_event(&event).is_ok());
        assert_eq!(handler.active_touch_count(), 1);

        // Touch up
        let event = TouchEvent::new(0.6, 0.6, TouchAction::Up, 0);
        assert!(handler.handle_touch_event(&event).is_ok());
        assert_eq!(handler.active_touch_count(), 0);
    }

    #[test]
    fn test_multi_touch() {
        let mut handler = InputHandler::new((1920, 0), (2560, 1600));

        // First finger down
        let event1 = TouchEvent::new(0.3, 0.3, TouchAction::Down, 0);
        assert!(handler.handle_touch_event(&event1).is_ok());
        assert_eq!(handler.active_touch_count(), 1);

        // Second finger down
        let event2 = TouchEvent::new(0.7, 0.7, TouchAction::Down, 1);
        assert!(handler.handle_touch_event(&event2).is_ok());
        assert_eq!(handler.active_touch_count(), 2);

        // First finger up
        let event3 = TouchEvent::new(0.3, 0.3, TouchAction::Up, 0);
        assert!(handler.handle_touch_event(&event3).is_ok());
        assert_eq!(handler.active_touch_count(), 1);

        // Second finger up
        let event4 = TouchEvent::new(0.7, 0.7, TouchAction::Up, 1);
        assert!(handler.handle_touch_event(&event4).is_ok());
        assert_eq!(handler.active_touch_count(), 0);
    }

    #[test]
    fn test_statistics() {
        let mut handler = InputHandler::new((1920, 0), (2560, 1600));

        let event = TouchEvent::new(0.5, 0.5, TouchAction::Down, 0);
        assert!(handler.handle_touch_event(&event).is_ok());

        let stats = handler.statistics();
        assert_eq!(stats.events_processed, 1);
        assert_eq!(stats.events_injected, 1);
        assert_eq!(stats.active_touches, 1);

        handler.reset_statistics();
        let stats = handler.statistics();
        assert_eq!(stats.events_processed, 0);
        assert_eq!(stats.events_injected, 0);
    }

    #[test]
    fn test_update_geometry() {
        let mut handler = InputHandler::new((1920, 0), (2560, 1600));

        // Update geometry
        handler.set_display_geometry((0, 0), (1920, 1080));

        let geom = handler.display_geometry();
        assert_eq!(geom.offset, (0, 0));
        assert_eq!(geom.size, (1920, 1080));

        // Verify coordinate conversion uses new geometry
        let coords = handler.normalize_to_desktop(1.0, 1.0);
        assert_eq!(coords.x, 1920);
        assert_eq!(coords.y, 1080);
    }

    #[test]
    fn test_touch_cancel() {
        let mut handler = InputHandler::new((1920, 0), (2560, 1600));

        // Touch down
        let event = TouchEvent::new(0.5, 0.5, TouchAction::Down, 0);
        assert!(handler.handle_touch_event(&event).is_ok());
        assert_eq!(handler.active_touch_count(), 1);

        // Cancel
        let event = TouchEvent::new(0.5, 0.5, TouchAction::Cancel, 0);
        assert!(handler.handle_touch_event(&event).is_ok());
        assert_eq!(handler.active_touch_count(), 0);
    }

    #[test]
    fn test_serialization() {
        let event = TouchEvent::new(0.5, 0.5, TouchAction::Down, 0)
            .with_pressure(0.8)
            .with_timestamp(1_234_567_890);

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: TouchEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.x, event.x);
        assert_eq!(deserialized.y, event.y);
        assert_eq!(deserialized.action, event.action);
        assert_eq!(deserialized.touch_id, event.touch_id);
        assert_eq!(deserialized.pressure, event.pressure);
        assert_eq!(deserialized.timestamp, event.timestamp);
    }
}
