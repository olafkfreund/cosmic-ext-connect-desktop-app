//! MouseKeyboardShare (mkshare) module
//!
//! This module provides the platform abstraction layer for the MouseKeyboardShare plugin,
//! enabling seamless mouse and keyboard sharing across multiple desktops.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                MouseKeyboardShare Plugin                │
//! ├─────────────────────────────────────────────────────────┤
//! │                  Platform Abstraction                   │
//! │         (InputCapture, InputInjection traits)          │
//! ├─────────────────────────────────────────────────────────┤
//! │              Wayland/COSMIC Backend                     │
//! │         (uinput + compositor protocols)                 │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Modules
//!
//! - [`traits`] - Platform abstraction traits for input capture and injection
//! - [`types`] - Shared types used across the module
//! - [`wayland`] - Wayland/COSMIC backend implementation
//! - [`edge_detector`] - Cursor edge detection for screen transitions
//! - [`hotkeys`] - Global hotkey registration and handling
//!
//! ## Usage
//!
//! ```rust,ignore
//! use cosmic_connect_protocol::plugins::mkshare::{InputBackendFactory, InputInjection};
//!
//! // Create platform-appropriate backend
//! let mut backend = InputBackendFactory::create().await?;
//!
//! // Initialize for input injection
//! backend.initialize().await?;
//!
//! // Inject mouse movement
//! backend.inject_mouse_move(10, -5).await?;
//! ```

pub mod edge_detector;
pub mod hotkeys;
pub mod traits;
pub mod types;
pub mod wayland;

// Re-export commonly used items
pub use edge_detector::{EdgeConfig, EdgeDetector, EdgeEvent};
pub use hotkeys::{HotkeyAction, HotkeyConfig, HotkeyEvent, HotkeyId, HotkeyManager};
pub use traits::{InputBackend, InputBackendFactory, InputCapture, InputInjection};
pub use types::{InputEvent, Modifiers, MouseButton, ScreenGeometry};
pub use wayland::WaylandInputBackend;
