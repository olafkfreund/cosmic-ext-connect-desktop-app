//! Global Hotkey Registration System
//!
//! Provides global hotkey capture for manual switching between desktops
//! without relying on edge detection.
//!
//! ## Wayland Limitations
//!
//! On Wayland, global hotkeys require compositor support:
//! - XDG Global Shortcuts portal (preferred)
//! - Compositor-specific protocols
//! - Keyboard polling as fallback (less reliable)
//!
//! ## Default Hotkeys
//!
//! - `Ctrl+Alt+Left` - Switch to left device
//! - `Ctrl+Alt+Right` - Switch to right device
//! - `Ctrl+Alt+Up` - Switch to top device
//! - `Ctrl+Alt+Down` - Switch to bottom device
//! - `Ctrl+Alt+Shift+S` - Toggle sharing on/off

use super::types::Modifiers;
use crate::plugins::mousekeyboardshare::ScreenEdge;
use crate::Result;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tokio::sync::broadcast;
use tracing::{debug, info};

/// Unique identifier for a registered hotkey
pub type HotkeyId = u64;

/// Action to perform when hotkey is triggered
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyAction {
    /// Switch to device at specified edge
    SwitchToEdge(ScreenEdge),
    /// Switch to specific device by ID
    SwitchToDevice(String),
    /// Toggle sharing on/off
    ToggleSharing,
    /// Custom action with identifier
    Custom(String),
}

/// Configuration for a single hotkey
#[derive(Debug, Clone)]
pub struct HotkeyConfig {
    /// Modifier keys required
    pub modifiers: Modifiers,
    /// Primary key (Linux keycode)
    pub key: u16,
    /// Action to perform
    pub action: HotkeyAction,
    /// Whether this hotkey is enabled
    pub enabled: bool,
}

impl HotkeyConfig {
    /// Create a new hotkey configuration
    pub fn new(modifiers: Modifiers, key: u16, action: HotkeyAction) -> Self {
        Self {
            modifiers,
            key,
            action,
            enabled: true,
        }
    }

    /// Create a hotkey for switching to an edge
    pub fn switch_to_edge(modifiers: Modifiers, key: u16, edge: ScreenEdge) -> Self {
        Self::new(modifiers, key, HotkeyAction::SwitchToEdge(edge))
    }

    /// Create a hotkey for toggling sharing
    pub fn toggle_sharing(modifiers: Modifiers, key: u16) -> Self {
        Self::new(modifiers, key, HotkeyAction::ToggleSharing)
    }
}

/// Event emitted when a hotkey is triggered
#[derive(Debug, Clone)]
pub struct HotkeyEvent {
    /// Hotkey ID
    pub id: HotkeyId,
    /// Action to perform
    pub action: HotkeyAction,
    /// When the hotkey was triggered
    pub timestamp: Instant,
}

/// Hotkey manager handles registration and detection of global hotkeys
pub struct HotkeyManager {
    /// Registered hotkeys
    hotkeys: Arc<RwLock<HashMap<HotkeyId, HotkeyConfig>>>,
    /// Next hotkey ID
    next_id: AtomicU64,
    /// Event sender
    event_tx: broadcast::Sender<HotkeyEvent>,
    /// Whether manager is active
    active: AtomicBool,
    /// Currently pressed modifiers (for polling)
    pressed_modifiers: Arc<RwLock<Modifiers>>,
    /// Currently pressed keys (for polling)
    pressed_keys: Arc<RwLock<Vec<u16>>>,
}

impl HotkeyManager {
    /// Create a new hotkey manager
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(64);

        Self {
            hotkeys: Arc::new(RwLock::new(HashMap::new())),
            next_id: AtomicU64::new(1),
            event_tx,
            active: AtomicBool::new(false),
            pressed_modifiers: Arc::new(RwLock::new(Modifiers::default())),
            pressed_keys: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create with default hotkeys registered
    pub fn with_defaults() -> Self {
        use mouse_keyboard_input::{KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_S, KEY_UP};

        let manager = Self::new();

        let ctrl_alt = Modifiers::ctrl_alt();
        let ctrl_alt_shift = Modifiers::ctrl_alt_shift();

        // Register Ctrl+Alt+Arrow keys for direction switching
        let direction_hotkeys = [
            (KEY_LEFT, ScreenEdge::Left),
            (KEY_RIGHT, ScreenEdge::Right),
            (KEY_UP, ScreenEdge::Top),
            (KEY_DOWN, ScreenEdge::Bottom),
        ];

        for (key, edge) in direction_hotkeys {
            let _ = manager.register(HotkeyConfig::switch_to_edge(ctrl_alt, key, edge));
        }

        // Ctrl+Alt+Shift+S for toggle
        let _ = manager.register(HotkeyConfig::toggle_sharing(ctrl_alt_shift, KEY_S));

        info!("Registered default hotkeys");
        manager
    }

    /// Subscribe to hotkey events
    pub fn subscribe(&self) -> broadcast::Receiver<HotkeyEvent> {
        self.event_tx.subscribe()
    }

    /// Register a new hotkey
    pub fn register(&self, config: HotkeyConfig) -> Result<HotkeyId> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        if let Ok(mut guard) = self.hotkeys.write() {
            debug!(
                "Registered hotkey {}: key={} modifiers={:?} action={:?}",
                id, config.key, config.modifiers, config.action
            );
            guard.insert(id, config);
        }

        Ok(id)
    }

    /// Unregister a hotkey
    pub fn unregister(&self, id: HotkeyId) -> bool {
        if let Ok(mut guard) = self.hotkeys.write() {
            if guard.remove(&id).is_some() {
                debug!("Unregistered hotkey {}", id);
                return true;
            }
        }
        false
    }

    /// Enable/disable a hotkey
    pub fn set_enabled(&self, id: HotkeyId, enabled: bool) {
        if let Ok(mut guard) = self.hotkeys.write() {
            if let Some(config) = guard.get_mut(&id) {
                config.enabled = enabled;
            }
        }
    }

    /// Get all registered hotkeys
    pub fn list(&self) -> Vec<(HotkeyId, HotkeyConfig)> {
        self.hotkeys
            .read()
            .map(|guard| guard.iter().map(|(id, c)| (*id, c.clone())).collect())
            .unwrap_or_default()
    }

    /// Start the hotkey manager
    pub fn start(&self) {
        self.active.store(true, Ordering::SeqCst);
        info!("Hotkey manager started");
    }

    /// Stop the hotkey manager
    pub fn stop(&self) {
        self.active.store(false, Ordering::SeqCst);
        info!("Hotkey manager stopped");
    }

    /// Check if manager is active
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// Handle a key press event (called by input backend)
    pub fn on_key_press(&self, keycode: u16, modifiers: Modifiers) {
        if !self.is_active() {
            return;
        }

        // Check all hotkeys for a match
        let hotkeys = match self.hotkeys.read() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };

        for (id, config) in &hotkeys {
            if !config.enabled {
                continue;
            }

            if config.key == keycode && config.modifiers == modifiers {
                debug!("Hotkey {} triggered: {:?}", id, config.action);

                let event = HotkeyEvent {
                    id: *id,
                    action: config.action.clone(),
                    timestamp: Instant::now(),
                };

                let _ = self.event_tx.send(event);
                return;
            }
        }
    }

    /// Update pressed modifier state (for polling mode)
    pub fn update_modifier_state(&self, modifiers: Modifiers) {
        if let Ok(mut guard) = self.pressed_modifiers.write() {
            *guard = modifiers;
        }
    }

    /// Add a pressed key (for polling mode)
    pub fn add_pressed_key(&self, keycode: u16) {
        if let Ok(mut guard) = self.pressed_keys.write() {
            if !guard.contains(&keycode) {
                guard.push(keycode);
            }
        }

        // Check for hotkey match
        self.check_hotkeys_polling();
    }

    /// Remove a pressed key (for polling mode)
    pub fn remove_pressed_key(&self, keycode: u16) {
        if let Ok(mut guard) = self.pressed_keys.write() {
            guard.retain(|&k| k != keycode);
        }
    }

    /// Check hotkeys in polling mode
    fn check_hotkeys_polling(&self) {
        if !self.is_active() {
            return;
        }

        let modifiers = self.pressed_modifiers.read().map(|g| *g).unwrap_or_default();
        let pressed = self.pressed_keys.read().map(|g| g.clone()).unwrap_or_default();

        for keycode in pressed {
            self.on_key_press(keycode, modifiers);
        }
    }
}

impl Default for HotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mouse_keyboard_input::{KEY_LEFT, KEY_S};

    #[test]
    fn test_hotkey_manager_creation() {
        let manager = HotkeyManager::new();
        assert!(!manager.is_active());
        assert!(manager.list().is_empty());
    }

    #[test]
    fn test_hotkey_registration() {
        let manager = HotkeyManager::new();

        let config = HotkeyConfig::new(
            Modifiers::default(),
            KEY_LEFT,
            HotkeyAction::SwitchToEdge(ScreenEdge::Left),
        );

        let id = manager.register(config).unwrap();
        assert!(id > 0);
        assert_eq!(manager.list().len(), 1);
    }

    #[test]
    fn test_hotkey_unregistration() {
        let manager = HotkeyManager::new();

        let config = HotkeyConfig::new(
            Modifiers::default(),
            KEY_LEFT,
            HotkeyAction::SwitchToEdge(ScreenEdge::Left),
        );

        let id = manager.register(config).unwrap();
        assert_eq!(manager.list().len(), 1);

        assert!(manager.unregister(id));
        assert!(manager.list().is_empty());
    }

    #[test]
    fn test_hotkey_enable_disable() {
        let manager = HotkeyManager::new();

        let config = HotkeyConfig::new(
            Modifiers::default(),
            KEY_LEFT,
            HotkeyAction::SwitchToEdge(ScreenEdge::Left),
        );

        let id = manager.register(config).unwrap();

        // Initially enabled
        let list = manager.list();
        assert!(list[0].1.enabled);

        // Disable
        manager.set_enabled(id, false);
        let list = manager.list();
        assert!(!list[0].1.enabled);
    }

    #[test]
    fn test_default_hotkeys() {
        let manager = HotkeyManager::with_defaults();
        let hotkeys = manager.list();

        // Should have 5 default hotkeys
        assert_eq!(hotkeys.len(), 5);

        // Check that we have the toggle action
        let has_toggle = hotkeys
            .iter()
            .any(|(_, c)| c.action == HotkeyAction::ToggleSharing);
        assert!(has_toggle);
    }

    #[test]
    fn test_hotkey_event_emission() {
        let manager = HotkeyManager::new();
        manager.start();

        let modifiers = Modifiers {
            ctrl: true,
            alt: true,
            ..Default::default()
        };

        let config = HotkeyConfig::new(modifiers, KEY_S, HotkeyAction::ToggleSharing);
        let _id = manager.register(config).unwrap();

        let mut receiver = manager.subscribe();

        // Trigger the hotkey
        manager.on_key_press(KEY_S, modifiers);

        // Should receive event
        let event = receiver.try_recv();
        assert!(event.is_ok());
        assert_eq!(event.unwrap().action, HotkeyAction::ToggleSharing);
    }

    #[test]
    fn test_hotkey_not_triggered_when_inactive() {
        let manager = HotkeyManager::new();
        // Don't call start()

        let modifiers = Modifiers::default();
        let config = HotkeyConfig::new(modifiers, KEY_S, HotkeyAction::ToggleSharing);
        let _id = manager.register(config).unwrap();

        let mut receiver = manager.subscribe();

        // Try to trigger
        manager.on_key_press(KEY_S, modifiers);

        // Should NOT receive event
        let event = receiver.try_recv();
        assert!(event.is_err());
    }

    #[test]
    fn test_hotkey_not_triggered_when_disabled() {
        let manager = HotkeyManager::new();
        manager.start();

        let modifiers = Modifiers::default();
        let config = HotkeyConfig::new(modifiers, KEY_S, HotkeyAction::ToggleSharing);
        let id = manager.register(config).unwrap();

        // Disable the hotkey
        manager.set_enabled(id, false);

        let mut receiver = manager.subscribe();

        // Try to trigger
        manager.on_key_press(KEY_S, modifiers);

        // Should NOT receive event
        let event = receiver.try_recv();
        assert!(event.is_err());
    }

    #[test]
    fn test_modifiers_must_match() {
        let manager = HotkeyManager::new();
        manager.start();

        let required_modifiers = Modifiers {
            ctrl: true,
            alt: true,
            ..Default::default()
        };

        let config = HotkeyConfig::new(required_modifiers, KEY_S, HotkeyAction::ToggleSharing);
        let _id = manager.register(config).unwrap();

        let mut receiver = manager.subscribe();

        // Wrong modifiers - should not trigger
        let wrong_modifiers = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        manager.on_key_press(KEY_S, wrong_modifiers);

        let event = receiver.try_recv();
        assert!(event.is_err());

        // Correct modifiers - should trigger
        manager.on_key_press(KEY_S, required_modifiers);
        let event = receiver.try_recv();
        assert!(event.is_ok());
    }
}
