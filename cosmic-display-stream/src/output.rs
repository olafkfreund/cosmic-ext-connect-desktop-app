//! Display output information and filtering

use serde::{Deserialize, Serialize};

/// Information about a display output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputInfo {
    /// Output name (e.g., "HDMI-2", "DP-1")
    pub name: String,

    /// Display resolution width in pixels
    pub width: u32,

    /// Display resolution height in pixels
    pub height: u32,

    /// Refresh rate in Hz
    pub refresh_rate: u32,

    /// Whether this is a virtual/dummy display
    pub is_virtual: bool,
}

impl OutputInfo {
    /// Create a new `OutputInfo`
    #[must_use] 
    pub fn new(name: String, width: u32, height: u32, refresh_rate: u32, is_virtual: bool) -> Self {
        Self {
            name,
            width,
            height,
            refresh_rate,
            is_virtual,
        }
    }

    /// Check if this output is an HDMI dummy plug
    ///
    /// HDMI dummy plugs typically have names like "HDMI-2", "HDMI-A-2", etc.
    /// and are marked as virtual displays by the compositor.
    #[must_use] 
    pub fn is_hdmi_dummy(&self) -> bool {
        self.is_virtual
            && (self.name.starts_with("HDMI-")
                || self.name.starts_with("HDMI-A-")
                || self.name.starts_with("HDMI-B-"))
    }

    /// Format output description for display
    #[must_use] 
    pub fn description(&self) -> String {
        format!(
            "{} ({}x{} @ {}Hz{})",
            self.name,
            self.width,
            self.height,
            self.refresh_rate,
            if self.is_virtual { ", virtual" } else { "" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hdmi_dummy_detection() {
        let hdmi_dummy = OutputInfo::new("HDMI-2".to_string(), 1920, 1080, 60, true);
        assert!(hdmi_dummy.is_hdmi_dummy());

        let hdmi_real = OutputInfo::new("HDMI-1".to_string(), 1920, 1080, 60, false);
        assert!(!hdmi_real.is_hdmi_dummy());

        let dp_virtual = OutputInfo::new("DP-1".to_string(), 1920, 1080, 60, true);
        assert!(!dp_virtual.is_hdmi_dummy());
    }

    #[test]
    fn test_description_formatting() {
        let output = OutputInfo::new("HDMI-2".to_string(), 1920, 1080, 60, true);
        assert_eq!(output.description(), "HDMI-2 (1920x1080 @ 60Hz, virtual)");
    }
}
