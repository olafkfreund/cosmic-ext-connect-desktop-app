//! Cursor Edge Detection System
//!
//! Monitors cursor position and detects when it reaches screen edges,
//! triggering transitions to connected remote desktops.
//!
//! ## How It Works
//!
//! 1. Poll cursor position at configurable interval (default 5ms)
//! 2. Check if cursor is within threshold of any screen edge
//! 3. Verify edge is mapped to a remote device
//! 4. Emit EdgeEvent with calculated remote coordinates
//!
//! ## Coordinate Translation
//!
//! When transitioning between screens, coordinates are mapped:
//! - Left edge → Remote's right edge, Y ratio preserved
//! - Right edge → Remote's left edge, Y ratio preserved
//! - Top edge → Remote's bottom edge, X ratio preserved
//! - Bottom edge → Remote's top edge, X ratio preserved

use super::traits::InputCapture;
use super::types::ScreenGeometry;
use crate::plugins::mousekeyboardshare::ScreenEdge;
use crate::Result;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{debug, trace, warn};

/// Default assumed remote screen width for coordinate mapping
const DEFAULT_REMOTE_WIDTH: i32 = 1920;
/// Default assumed remote screen height for coordinate mapping
const DEFAULT_REMOTE_HEIGHT: i32 = 1080;

/// Configuration for edge detection
#[derive(Debug, Clone)]
pub struct EdgeConfig {
    /// Pixels from edge to trigger transition (default: 1)
    pub edge_threshold: u32,
    /// Corner dead zone size in pixels (default: 20)
    pub corner_dead_zone: u32,
    /// Polling interval in milliseconds (default: 5)
    pub poll_interval_ms: u64,
    /// Edge to device ID mappings
    pub edge_mappings: HashMap<ScreenEdge, String>,
}

impl Default for EdgeConfig {
    fn default() -> Self {
        Self {
            edge_threshold: 1,
            corner_dead_zone: 20,
            poll_interval_ms: 5,
            edge_mappings: HashMap::new(),
        }
    }
}

/// Event emitted when cursor hits a mapped edge
#[derive(Debug, Clone)]
pub struct EdgeEvent {
    /// Which edge was hit
    pub edge: ScreenEdge,
    /// Target device ID for the transition
    pub target_device: String,
    /// Cursor position when edge was hit (local coordinates)
    pub local_position: (i32, i32),
    /// Calculated position on remote screen
    pub remote_position: (i32, i32),
    /// Which local screen the cursor was on
    pub screen_index: usize,
    /// When the edge was hit
    pub timestamp: Instant,
}

/// Edge detector monitors cursor position and emits transition events
pub struct EdgeDetector {
    /// Configuration
    config: Arc<RwLock<EdgeConfig>>,
    /// Input backend for cursor position
    input_backend: Arc<dyn InputCapture>,
    /// Channel for edge events
    event_tx: broadcast::Sender<EdgeEvent>,
    /// Whether detector is running
    running: AtomicBool,
    /// Last emitted edge (for debouncing)
    last_edge: Arc<RwLock<Option<(ScreenEdge, Instant)>>>,
    /// Debounce duration to prevent rapid re-triggers
    debounce_ms: u64,
}

impl EdgeDetector {
    /// Create a new edge detector
    pub fn new(input_backend: Arc<dyn InputCapture>) -> Self {
        let (event_tx, _) = broadcast::channel(64);

        Self {
            config: Arc::new(RwLock::new(EdgeConfig::default())),
            input_backend,
            event_tx,
            running: AtomicBool::new(false),
            last_edge: Arc::new(RwLock::new(None)),
            debounce_ms: 500, // 500ms debounce
        }
    }

    /// Create with custom configuration
    pub fn with_config(input_backend: Arc<dyn InputCapture>, config: EdgeConfig) -> Self {
        let detector = Self::new(input_backend);
        if let Ok(mut guard) = detector.config.write() {
            *guard = config;
        }
        detector
    }

    /// Subscribe to edge events
    pub fn subscribe(&self) -> broadcast::Receiver<EdgeEvent> {
        self.event_tx.subscribe()
    }

    /// Update configuration
    pub fn set_config(&self, config: EdgeConfig) {
        if let Ok(mut guard) = self.config.write() {
            *guard = config;
        }
    }

    /// Map an edge to a device
    pub fn map_edge(&self, edge: ScreenEdge, device_id: String) {
        if let Ok(mut guard) = self.config.write() {
            guard.edge_mappings.insert(edge, device_id);
        }
    }

    /// Unmap an edge
    pub fn unmap_edge(&self, edge: &ScreenEdge) {
        if let Ok(mut guard) = self.config.write() {
            guard.edge_mappings.remove(edge);
        }
    }

    /// Start the edge detection loop
    pub async fn start(&self) -> Result<()> {
        if self.running.swap(true, Ordering::SeqCst) {
            warn!("Edge detector already running");
            return Ok(());
        }

        debug!("Starting edge detection loop");

        while self.running.load(Ordering::SeqCst) {
            let poll_interval = self
                .config
                .read()
                .map(|c| c.poll_interval_ms)
                .unwrap_or(5);

            if let Some((x, y)) = self.input_backend.cursor_position() {
                if let Some(event) = self.check_edges(x, y) {
                    // Check debounce
                    let should_emit = self.check_debounce(&event.edge);

                    if should_emit {
                        trace!("Edge hit: {:?} at ({}, {})", event.edge, x, y);

                        // Save edge before moving event
                        let edge = event.edge;
                        let _ = self.event_tx.send(event);

                        // Update last edge
                        if let Ok(mut guard) = self.last_edge.write() {
                            *guard = Some((edge, Instant::now()));
                        }
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(poll_interval)).await;
        }

        debug!("Edge detection loop stopped");
        Ok(())
    }

    /// Stop the edge detection loop
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Check if detector is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Check debounce - returns true if we should emit
    fn check_debounce(&self, edge: &ScreenEdge) -> bool {
        if let Ok(guard) = self.last_edge.read() {
            if let Some((last_edge, last_time)) = *guard {
                if last_edge == *edge
                    && last_time.elapsed() < Duration::from_millis(self.debounce_ms)
                {
                    return false;
                }
            }
        }
        true
    }

    /// Check if cursor is at any edge
    fn check_edges(&self, x: i32, y: i32) -> Option<EdgeEvent> {
        let config = self.config.read().ok()?;
        let screens = self.input_backend.screen_geometry();

        if screens.is_empty() {
            return None;
        }

        // Find which screen the cursor is on
        for (screen_idx, screen) in screens.iter().enumerate() {
            if !screen.contains(x, y) {
                continue;
            }

            // Check each edge
            if let Some(event) =
                self.check_edge_at_screen(x, y, screen, screen_idx, &config, &screens)
            {
                return Some(event);
            }
        }

        None
    }

    /// Check edges for a specific screen
    fn check_edge_at_screen(
        &self,
        x: i32,
        y: i32,
        screen: &ScreenGeometry,
        screen_idx: usize,
        config: &EdgeConfig,
        _all_screens: &[ScreenGeometry],
    ) -> Option<EdgeEvent> {
        let threshold = config.edge_threshold as i32;
        let dead_zone = config.corner_dead_zone as i32;

        // Skip if in corner dead zone
        if self.is_in_corner_dead_zone(x, y, screen, dead_zone) {
            return None;
        }

        // Determine which edge the cursor is at and check if it's mapped
        let edge = self.detect_edge(x, y, screen, threshold)?;
        let device_id = config.edge_mappings.get(&edge)?;

        // Calculate remote position based on edge
        let remote_position = self.calculate_default_remote_position(x, y, screen, edge);

        Some(EdgeEvent {
            edge,
            target_device: device_id.clone(),
            local_position: (x, y),
            remote_position,
            screen_index: screen_idx,
            timestamp: Instant::now(),
        })
    }

    /// Check if cursor is in a corner dead zone
    fn is_in_corner_dead_zone(&self, x: i32, y: i32, screen: &ScreenGeometry, dead_zone: i32) -> bool {
        let near_left = x < screen.x + dead_zone;
        let near_right = x > screen.right() - dead_zone;
        let near_top = y < screen.y + dead_zone;
        let near_bottom = y > screen.bottom() - dead_zone;
        (near_left || near_right) && (near_top || near_bottom)
    }

    /// Detect which edge the cursor is at, if any
    fn detect_edge(&self, x: i32, y: i32, screen: &ScreenGeometry, threshold: i32) -> Option<ScreenEdge> {
        if x <= screen.x + threshold {
            Some(ScreenEdge::Left)
        } else if x >= screen.right() - threshold {
            Some(ScreenEdge::Right)
        } else if y <= screen.y + threshold {
            Some(ScreenEdge::Top)
        } else if y >= screen.bottom() - threshold {
            Some(ScreenEdge::Bottom)
        } else {
            None
        }
    }

    /// Calculate remote position using default screen dimensions
    fn calculate_default_remote_position(
        &self,
        x: i32,
        y: i32,
        screen: &ScreenGeometry,
        edge: ScreenEdge,
    ) -> (i32, i32) {
        match edge {
            ScreenEdge::Left => {
                let y_ratio = (y - screen.y) as f64 / screen.height as f64;
                let remote_y = (y_ratio * DEFAULT_REMOTE_HEIGHT as f64) as i32;
                (DEFAULT_REMOTE_WIDTH - 1, remote_y)
            }
            ScreenEdge::Right => {
                let y_ratio = (y - screen.y) as f64 / screen.height as f64;
                let remote_y = (y_ratio * DEFAULT_REMOTE_HEIGHT as f64) as i32;
                (0, remote_y)
            }
            ScreenEdge::Top => {
                let x_ratio = (x - screen.x) as f64 / screen.width as f64;
                let remote_x = (x_ratio * DEFAULT_REMOTE_WIDTH as f64) as i32;
                (remote_x, DEFAULT_REMOTE_HEIGHT - 1)
            }
            ScreenEdge::Bottom => {
                let x_ratio = (x - screen.x) as f64 / screen.width as f64;
                let remote_x = (x_ratio * DEFAULT_REMOTE_WIDTH as f64) as i32;
                (remote_x, 0)
            }
        }
    }

    /// Calculate remote position given local position and edge
    pub fn calculate_remote_position(
        local_pos: (i32, i32),
        local_screen: &ScreenGeometry,
        remote_screen: &ScreenGeometry,
        edge: ScreenEdge,
    ) -> (i32, i32) {
        let (x, y) = local_pos;

        match edge {
            ScreenEdge::Left => {
                // Map to remote's right edge
                let y_ratio = (y - local_screen.y) as f64 / local_screen.height as f64;
                let remote_y = remote_screen.y + (y_ratio * remote_screen.height as f64) as i32;
                (remote_screen.right() - 1, remote_y)
            }
            ScreenEdge::Right => {
                // Map to remote's left edge
                let y_ratio = (y - local_screen.y) as f64 / local_screen.height as f64;
                let remote_y = remote_screen.y + (y_ratio * remote_screen.height as f64) as i32;
                (remote_screen.x, remote_y)
            }
            ScreenEdge::Top => {
                // Map to remote's bottom edge
                let x_ratio = (x - local_screen.x) as f64 / local_screen.width as f64;
                let remote_x = remote_screen.x + (x_ratio * remote_screen.width as f64) as i32;
                (remote_x, remote_screen.bottom() - 1)
            }
            ScreenEdge::Bottom => {
                // Map to remote's top edge
                let x_ratio = (x - local_screen.x) as f64 / local_screen.width as f64;
                let remote_x = remote_screen.x + (x_ratio * remote_screen.width as f64) as i32;
                (remote_x, remote_screen.y)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_screen() -> ScreenGeometry {
        ScreenGeometry::new(0, 0, 1920, 1080, "test")
    }

    fn create_remote_screen() -> ScreenGeometry {
        ScreenGeometry::new(0, 0, 2560, 1440, "remote")
    }

    #[test]
    fn test_edge_config_default() {
        let config = EdgeConfig::default();
        assert_eq!(config.edge_threshold, 1);
        assert_eq!(config.corner_dead_zone, 20);
        assert_eq!(config.poll_interval_ms, 5);
        assert!(config.edge_mappings.is_empty());
    }

    #[test]
    fn test_calculate_remote_position_left_edge() {
        let local = create_test_screen();
        let remote = create_remote_screen();

        // Middle of left edge
        let (rx, ry) =
            EdgeDetector::calculate_remote_position((0, 540), &local, &remote, ScreenEdge::Left);

        assert_eq!(rx, 2559); // Right edge of remote
        assert_eq!(ry, 720); // Middle height (540/1080 * 1440 = 720)
    }

    #[test]
    fn test_calculate_remote_position_right_edge() {
        let local = create_test_screen();
        let remote = create_remote_screen();

        // Middle of right edge
        let (rx, ry) =
            EdgeDetector::calculate_remote_position((1919, 540), &local, &remote, ScreenEdge::Right);

        assert_eq!(rx, 0); // Left edge of remote
        assert_eq!(ry, 720); // Middle height
    }

    #[test]
    fn test_calculate_remote_position_top_edge() {
        let local = create_test_screen();
        let remote = create_remote_screen();

        // Middle of top edge
        let (rx, ry) =
            EdgeDetector::calculate_remote_position((960, 0), &local, &remote, ScreenEdge::Top);

        assert_eq!(rx, 1280); // Middle width (960/1920 * 2560 = 1280)
        assert_eq!(ry, 1439); // Bottom edge of remote
    }

    #[test]
    fn test_calculate_remote_position_bottom_edge() {
        let local = create_test_screen();
        let remote = create_remote_screen();

        // Middle of bottom edge
        let (rx, ry) =
            EdgeDetector::calculate_remote_position((960, 1079), &local, &remote, ScreenEdge::Bottom);

        assert_eq!(rx, 1280); // Middle width
        assert_eq!(ry, 0); // Top edge of remote
    }

    #[test]
    fn test_calculate_remote_position_preserves_ratio() {
        let local = create_test_screen();
        let remote = create_remote_screen();

        // 1/4 from top on left edge
        let (_, ry) =
            EdgeDetector::calculate_remote_position((0, 270), &local, &remote, ScreenEdge::Left);

        // 270/1080 = 0.25, 0.25 * 1440 = 360
        assert_eq!(ry, 360);
    }
}
