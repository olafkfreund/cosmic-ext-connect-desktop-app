/// Camera streaming statistics
#[derive(Debug, Clone)]
pub struct CameraStats {
    /// Current frames per second
    pub fps: u32,
    /// Current bitrate in kbps
    pub bitrate: u32,
    /// Is currently streaming
    pub is_streaming: bool,
    /// Current camera ID
    pub camera_id: u32,
    /// Current resolution (e.g., "720p")
    pub resolution: String,
}
