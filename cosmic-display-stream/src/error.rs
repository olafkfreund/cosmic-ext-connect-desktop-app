//! Error types for display streaming operations

use thiserror::Error;

/// Result type alias for display streaming operations
pub type Result<T> = std::result::Result<T, DisplayStreamError>;

/// Errors that can occur during display streaming
#[derive(Debug, Error)]
pub enum DisplayStreamError {
    /// Error interacting with xdg-desktop-portal (ashpd error)
    #[error("Portal error: {0}")]
    PortalAshpd(#[from] ashpd::Error),

    /// Error interacting with xdg-desktop-portal (string message)
    #[error("Portal error: {0}")]
    Portal(String),

    /// Error with PipeWire stream
    #[error("PipeWire error: {0}")]
    PipeWire(String),

    /// Target display output not found
    #[error("Display output '{0}' not found")]
    OutputNotFound(String),

    /// Screen capture session failed
    #[error("Screen capture session failed: {0}")]
    CaptureSessionFailed(String),

    /// Permission denied by user
    #[error("Screen capture permission denied")]
    PermissionDenied,

    /// Invalid stream configuration
    #[error("Invalid stream configuration: {0}")]
    InvalidConfiguration(String),

    /// Stream already started
    #[error("Stream already started")]
    StreamAlreadyStarted,

    /// Stream not started
    #[error("Stream not started")]
    StreamNotStarted,

    /// Video encoder error
    #[error("Encoder error: {0}")]
    Encoder(String),

    /// Network streaming error
    #[error("Streaming error: {0}")]
    Streaming(String),

    /// Input event handling error
    #[error("Input error: {0}")]
    Input(String),

    /// Generic I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
