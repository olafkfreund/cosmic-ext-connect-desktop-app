/// Active screen share session information
#[derive(Debug, Clone)]
pub struct ActiveScreenShare {
    pub device_id: String,
    pub is_sender: bool,     // true if we are sharing, false if receiving
    pub is_paused: bool,     // true if the share is paused
    pub quality: String,     // quality preset: "low", "medium", "high"
    pub fps: u8,             // target framerate: 15, 30, or 60
    pub include_audio: bool, // whether system audio is included in the share
    pub viewer_count: u32,   // number of active viewers (only for sender)
}
