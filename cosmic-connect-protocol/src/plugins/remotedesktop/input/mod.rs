//! Input Handling Module
//!
//! Forwards VNC input events (keyboard and mouse) to Linux virtual devices.
//!
//! ## Architecture
//!
//! ```text
//! VNC Server
//!     ↓
//! KeyEvent / PointerEvent
//!     ↓
//! InputHandler
//!     ↓
//! VirtualDevice (mouse-keyboard-input)
//!     ↓
//! Linux Input Subsystem
//! ```
//!
//! ## Usage
//!
//! ```rust,no_run
//! # use cosmic_connect_protocol::plugins::remotedesktop::input::InputHandler;
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut handler = InputHandler::new()?;
//!
//! // Forward key event
//! handler.handle_key_event(0x0041, true).await?; // Press 'A'
//! handler.handle_key_event(0x0041, false).await?; // Release 'A'
//!
//! // Forward pointer event
//! handler.handle_pointer_event(100, 200, 0x01).await?; // Move to (100,200), left button down
//! # Ok(())
//! # }
//! ```

pub mod mapper;

use crate::Result;
use mouse_keyboard_input::{VirtualDevice, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// Input handler for VNC events
pub struct InputHandler {
    /// Virtual device for keyboard and mouse input
    device: VirtualDevice,

    /// Rate limiter: last event timestamp
    last_event: Instant,

    /// Rate limiter: minimum interval between events
    min_interval: Duration,

    /// Current mouse position
    mouse_x: u16,
    mouse_y: u16,

    /// Current button state
    button_state: u8,
}

impl InputHandler {
    /// Create new input handler
    ///
    /// Creates a virtual input device that can send keyboard and mouse events
    /// to the Linux input subsystem.
    pub fn new() -> Result<Self> {
        debug!("Creating input handler with virtual device");

        let device = VirtualDevice::default().map_err(|e| {
            crate::ProtocolError::Plugin(format!("Failed to create virtual device: {}", e))
        })?;

        Ok(Self {
            device,
            last_event: Instant::now(),
            min_interval: Duration::from_millis(10), // Max 100 Hz
            mouse_x: 0,
            mouse_y: 0,
            button_state: 0,
        })
    }

    /// Check if rate limit allows processing this event
    fn check_rate_limit(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_event);

        if elapsed >= self.min_interval {
            self.last_event = now;
            true
        } else {
            false
        }
    }

    /// Handle VNC key event
    ///
    /// # Arguments
    ///
    /// * `keysym` - X11 keysym (VNC standard)
    /// * `down` - true for key press, false for key release
    pub async fn handle_key_event(&mut self, keysym: u32, down: bool) -> Result<()> {
        // Rate limiting
        if !self.check_rate_limit() {
            return Ok(());
        }

        // Map VNC keysym to Linux keycode
        if let Some(keycode) = mapper::keysym_to_keycode(keysym) {
            debug!(
                "Key event: keysym=0x{:08x} keycode={} down={}",
                keysym, keycode, down
            );

            // Send key event
            if down {
                self.device.press(keycode).map_err(|e| {
                    crate::ProtocolError::Plugin(format!("Failed to press key: {}", e))
                })?;
            } else {
                self.device.release(keycode).map_err(|e| {
                    crate::ProtocolError::Plugin(format!("Failed to release key: {}", e))
                })?;
            }
        } else {
            warn!("Unknown keysym: 0x{:08x}", keysym);
        }

        Ok(())
    }

    /// Handle VNC pointer event
    ///
    /// # Arguments
    ///
    /// * `x` - X position (absolute)
    /// * `y` - Y position (absolute)
    /// * `button_mask` - Button state bitmask (bit 0 = left, bit 1 = middle, bit 2 = right)
    pub async fn handle_pointer_event(&mut self, x: u16, y: u16, button_mask: u8) -> Result<()> {
        // Rate limiting
        if !self.check_rate_limit() {
            return Ok(());
        }

        debug!(
            "Pointer event: pos=({}, {}), buttons=0x{:02x}",
            x, y, button_mask
        );

        // Handle mouse movement
        if x != self.mouse_x || y != self.mouse_y {
            let dx = (x as i32) - (self.mouse_x as i32);
            let dy = (y as i32) - (self.mouse_y as i32);

            self.device.move_mouse(dx, dy).map_err(|e| {
                crate::ProtocolError::Plugin(format!("Failed to move mouse: {}", e))
            })?;

            self.mouse_x = x;
            self.mouse_y = y;
        }

        // Handle button changes
        let button_changes = button_mask ^ self.button_state;

        // Left button (bit 0)
        if button_changes & 0x01 != 0 {
            if button_mask & 0x01 != 0 {
                debug!("Left button press");
                self.device.press(BTN_LEFT).map_err(|e| {
                    crate::ProtocolError::Plugin(format!("Failed to press left button: {}", e))
                })?;
            } else {
                debug!("Left button release");
                self.device.release(BTN_LEFT).map_err(|e| {
                    crate::ProtocolError::Plugin(format!("Failed to release left button: {}", e))
                })?;
            }
        }

        // Middle button (bit 1)
        if button_changes & 0x02 != 0 {
            if button_mask & 0x02 != 0 {
                debug!("Middle button press");
                self.device.press(BTN_MIDDLE).map_err(|e| {
                    crate::ProtocolError::Plugin(format!("Failed to press middle button: {}", e))
                })?;
            } else {
                debug!("Middle button release");
                self.device.release(BTN_MIDDLE).map_err(|e| {
                    crate::ProtocolError::Plugin(format!("Failed to release middle button: {}", e))
                })?;
            }
        }

        // Right button (bit 2)
        if button_changes & 0x04 != 0 {
            if button_mask & 0x04 != 0 {
                debug!("Right button press");
                self.device.press(BTN_RIGHT).map_err(|e| {
                    crate::ProtocolError::Plugin(format!("Failed to press right button: {}", e))
                })?;
            } else {
                debug!("Right button release");
                self.device.release(BTN_RIGHT).map_err(|e| {
                    crate::ProtocolError::Plugin(format!("Failed to release right button: {}", e))
                })?;
            }
        }

        // Scroll wheel (bits 3-4)
        // Bit 3 = scroll up, Bit 4 = scroll down
        // Note: VNC scroll events are momentary (button down = scroll action)
        if button_mask & 0x08 != 0 {
            debug!("Scroll up");
            // Simulate scroll with multiple relative movements (since scroll_wheel is not available)
            for _ in 0..3 {
                self.device.move_mouse(0, -1).map_err(|e| {
                    crate::ProtocolError::Plugin(format!("Failed to scroll: {}", e))
                })?;
            }
        }
        if button_mask & 0x10 != 0 {
            debug!("Scroll down");
            for _ in 0..3 {
                self.device.move_mouse(0, 1).map_err(|e| {
                    crate::ProtocolError::Plugin(format!("Failed to scroll: {}", e))
                })?;
            }
        }

        self.button_state = button_mask;

        Ok(())
    }

    /// Get current rate limit interval
    pub fn rate_limit(&self) -> Duration {
        self.min_interval
    }

    /// Set rate limit interval
    ///
    /// # Arguments
    ///
    /// * `interval` - Minimum duration between events
    pub fn set_rate_limit(&mut self, interval: Duration) {
        self.min_interval = interval;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit() {
        // Create handler (may fail if not running as root/with proper permissions)
        if let Ok(mut handler) = InputHandler::new() {
            // First event should be allowed
            assert!(handler.check_rate_limit());

            // Immediate second event should be blocked
            assert!(!handler.check_rate_limit());

            // Wait for rate limit to expire
            std::thread::sleep(Duration::from_millis(11));

            // Should be allowed again
            assert!(handler.check_rate_limit());
        }
    }

    #[test]
    fn test_set_rate_limit() {
        if let Ok(mut handler) = InputHandler::new() {
            let new_interval = Duration::from_millis(50);
            handler.set_rate_limit(new_interval);
            assert_eq!(handler.rate_limit(), new_interval);
        }
    }
}
