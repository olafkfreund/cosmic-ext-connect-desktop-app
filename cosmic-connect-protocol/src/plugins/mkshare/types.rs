//! Shared types for MouseKeyboardShare plugin
//!
//! This module defines the common types used across the mkshare module
//! for input capture, injection, and event handling.

use crate::plugins::mousekeyboardshare::ScreenEdge;
use serde::{Deserialize, Serialize};

/// Mouse button identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    /// Left mouse button
    Left,
    /// Right mouse button
    Right,
    /// Middle mouse button (scroll wheel click)
    Middle,
    /// Side button (back)
    Side,
    /// Extra button (forward)
    Extra,
}

impl MouseButton {
    /// Convert to Linux input event code
    pub fn to_linux_code(&self) -> u16 {
        use mouse_keyboard_input::{BTN_EXTRA, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, BTN_SIDE};
        match self {
            Self::Left => BTN_LEFT,
            Self::Right => BTN_RIGHT,
            Self::Middle => BTN_MIDDLE,
            Self::Side => BTN_SIDE,
            Self::Extra => BTN_EXTRA,
        }
    }
}

/// Keyboard modifier keys state
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifiers {
    /// Shift key is pressed
    pub shift: bool,
    /// Ctrl key is pressed
    pub ctrl: bool,
    /// Alt key is pressed
    pub alt: bool,
    /// Meta/Super/Windows key is pressed
    pub meta: bool,
}

impl Modifiers {
    /// Check if any modifier is pressed
    pub fn any(&self) -> bool {
        self.shift || self.ctrl || self.alt || self.meta
    }

    /// Create Ctrl+Alt modifier combination
    pub fn ctrl_alt() -> Self {
        Self {
            ctrl: true,
            alt: true,
            ..Default::default()
        }
    }

    /// Create Ctrl+Alt+Shift modifier combination
    pub fn ctrl_alt_shift() -> Self {
        Self {
            ctrl: true,
            alt: true,
            shift: true,
            ..Default::default()
        }
    }
}

/// Screen/monitor geometry information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenGeometry {
    /// X position of the screen origin (for multi-monitor setups)
    pub x: i32,
    /// Y position of the screen origin
    pub y: i32,
    /// Screen width in pixels
    pub width: u32,
    /// Screen height in pixels
    pub height: u32,
    /// Display name/identifier
    pub name: String,
    /// Whether this is the primary display
    pub primary: bool,
}

impl ScreenGeometry {
    /// Create a new screen geometry
    pub fn new(x: i32, y: i32, width: u32, height: u32, name: impl Into<String>) -> Self {
        Self {
            x,
            y,
            width,
            height,
            name: name.into(),
            primary: false,
        }
    }

    /// Check if a point is within this screen
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && px < self.x + self.width as i32
            && py >= self.y
            && py < self.y + self.height as i32
    }

    /// Get the right edge X coordinate
    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    /// Get the bottom edge Y coordinate
    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }
}

/// Input events that can be captured or injected
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InputEvent {
    /// Relative mouse movement
    MouseMove {
        /// Delta X movement
        dx: i32,
        /// Delta Y movement
        dy: i32,
    },
    /// Absolute mouse position (for edge transitions)
    MousePosition {
        /// Absolute X position
        x: i32,
        /// Absolute Y position
        y: i32,
    },
    /// Mouse button press/release
    MouseButton {
        /// Which button
        button: MouseButton,
        /// True if pressed, false if released
        pressed: bool,
    },
    /// Keyboard key press/release
    Key {
        /// Linux keycode
        keycode: u16,
        /// True if pressed, false if released
        pressed: bool,
        /// Modifier keys state
        modifiers: Modifiers,
    },
    /// Scroll wheel event
    Scroll {
        /// Horizontal scroll delta
        dx: f64,
        /// Vertical scroll delta
        dy: f64,
    },
}

/// Result of edge detection check
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeHit {
    /// Which edge was hit
    pub edge: ScreenEdge,
    /// Cursor position when edge was hit
    pub position: (i32, i32),
    /// Which screen the cursor was on
    pub screen_index: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mouse_button_to_linux_code() {
        assert_eq!(
            MouseButton::Left.to_linux_code(),
            mouse_keyboard_input::BTN_LEFT
        );
        assert_eq!(
            MouseButton::Right.to_linux_code(),
            mouse_keyboard_input::BTN_RIGHT
        );
        assert_eq!(
            MouseButton::Middle.to_linux_code(),
            mouse_keyboard_input::BTN_MIDDLE
        );
    }

    #[test]
    fn test_modifiers_default() {
        let mods = Modifiers::default();
        assert!(!mods.shift);
        assert!(!mods.ctrl);
        assert!(!mods.alt);
        assert!(!mods.meta);
        assert!(!mods.any());
    }

    #[test]
    fn test_modifiers_any() {
        let mods = Modifiers {
            shift: true,
            ..Default::default()
        };
        assert!(mods.any());
    }

    #[test]
    fn test_modifiers_ctrl_alt() {
        let mods = Modifiers::ctrl_alt();
        assert!(mods.ctrl);
        assert!(mods.alt);
        assert!(!mods.shift);
        assert!(!mods.meta);
    }

    #[test]
    fn test_modifiers_ctrl_alt_shift() {
        let mods = Modifiers::ctrl_alt_shift();
        assert!(mods.ctrl);
        assert!(mods.alt);
        assert!(mods.shift);
        assert!(!mods.meta);
    }

    #[test]
    fn test_screen_geometry_contains() {
        let screen = ScreenGeometry::new(0, 0, 1920, 1080, "primary");

        assert!(screen.contains(0, 0));
        assert!(screen.contains(1919, 1079));
        assert!(screen.contains(960, 540));

        assert!(!screen.contains(-1, 0));
        assert!(!screen.contains(0, -1));
        assert!(!screen.contains(1920, 0));
        assert!(!screen.contains(0, 1080));
    }

    #[test]
    fn test_screen_geometry_edges() {
        let screen = ScreenGeometry::new(100, 200, 800, 600, "test");

        assert_eq!(screen.right(), 900);
        assert_eq!(screen.bottom(), 800);
    }

    #[test]
    fn test_input_event_serialization() {
        let event = InputEvent::MouseMove { dx: 10, dy: -5 };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("MouseMove"));

        let parsed: InputEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }
}
