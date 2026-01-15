//! Error handling for KDE Connect protocol
//!
//! This module provides a comprehensive error type for all protocol operations.
//! Errors are automatically converted from underlying library errors using `thiserror`.
//!
//! ## Error Handling Patterns
//!
//! ### Basic Usage
//!
//! ```rust,no_run
//! use kdeconnect_protocol::{Packet, Result};
//!
//! fn process_packet(data: &[u8]) -> Result<Packet> {
//!     // Errors are automatically converted using From trait
//!     let packet = Packet::from_bytes(data)?;
//!     Ok(packet)
//! }
//! ```
//!
//! ### Error Propagation
//!
//! Use `?` operator for automatic error propagation:
//!
//! ```rust,no_run
//! use kdeconnect_protocol::{Device, DeviceManager, Result};
//!
//! fn save_device(manager: &mut DeviceManager, device: Device) -> Result<()> {
//!     manager.add_device(device);
//!     manager.save_registry()?;  // IO errors auto-converted
//!     Ok(())
//! }
//! ```
//!
//! ### Error Matching
//!
//! Match on specific error variants for custom handling:
//!
//! ```rust,no_run
//! use kdeconnect_protocol::{ProtocolError, Result};
//!
//! # async fn example(device_id: &str) -> Result<()> {
//! match get_device(device_id).await {
//!     Ok(device) => println!("Found device: {}", device.name()),
//!     Err(ProtocolError::DeviceNotFound(id)) => {
//!         eprintln!("Device {} not found", id);
//!     }
//!     Err(ProtocolError::NotPaired) => {
//!         eprintln!("Device not paired, initiating pairing...");
//!     }
//!     Err(e) => return Err(e), // Propagate other errors
//! }
//! # Ok(())
//! # }
//! # async fn get_device(id: &str) -> Result<kdeconnect_protocol::Device> {
//! #     Err(ProtocolError::DeviceNotFound(id.to_string()))
//! # }
//! ```
//!
//! ### Creating Custom Errors
//!
//! Use error constructors for domain-specific errors:
//!
//! ```rust
//! use kdeconnect_protocol::ProtocolError;
//!
//! // Device-specific errors
//! let error = ProtocolError::DeviceNotFound("unknown-device-id".to_string());
//! let error = ProtocolError::NotPaired;
//!
//! // Packet errors
//! let error = ProtocolError::InvalidPacket("missing required field".to_string());
//!
//! // Plugin errors
//! let error = ProtocolError::Plugin("plugin initialization failed".to_string());
//! ```
//!
//! ### Logging Errors
//!
//! Use `tracing` macros for structured logging:
//!
//! ```rust,ignore
//! use tracing::{error, warn, info, debug};
//!
//! // Critical errors that prevent operation
//! if let Err(e) = device.mark_connected(host, port) {
//!     error!("Failed to mark device connected: {}", e);
//!     return Err(e);
//! }
//!
//! // Non-critical errors that are handled
//! if let Err(e) = save_cache().await {
//!     warn!("Failed to save cache: {}", e);
//!     // Continue operation
//! }
//!
//! // Informational messages
//! info!("Device {} connected", device.id());
//!
//! // Debug messages for development
//! debug!("Processing packet: {:?}", packet);
//! ```
//!
//! ## Error Categories
//!
//! ### I/O Errors
//! File system, network, and general I/O failures.
//! Automatically converted from `std::io::Error`.
//!
//! ### Serialization Errors
//! JSON parsing and serialization failures.
//! Automatically converted from `serde_json::Error`.
//!
//! ### TLS Errors
//! Secure connection and certificate validation failures.
//! Automatically converted from `rustls::Error`.
//!
//! ### Certificate Errors
//! Certificate generation and management failures.
//! Automatically converted from `rcgen::Error`.
//!
//! ### Protocol Errors
//! Domain-specific KDE Connect protocol errors:
//! - `DeviceNotFound`: Requested device doesn't exist
//! - `NotPaired`: Operation requires paired device
//! - `InvalidPacket`: Malformed or invalid packet
//! - `Plugin`: Plugin-specific errors

use thiserror::Error;

/// Result type for protocol operations
///
/// This is a type alias for `Result<T, ProtocolError>` that simplifies
/// error handling throughout the protocol implementation.
///
/// # Examples
///
/// ```rust,no_run
/// use kdeconnect_protocol::Result;
///
/// fn example() -> Result<()> {
///     // Your code here
///     Ok(())
/// }
/// ```
pub type Result<T> = std::result::Result<T, ProtocolError>;

/// Errors that can occur during protocol operations
///
/// This enum encompasses all possible errors that can occur when using
/// the KDE Connect protocol. Most errors automatically convert from
/// underlying library errors using the `From` trait.
///
/// # Automatic Conversions
///
/// The following types automatically convert to `ProtocolError`:
/// - `std::io::Error` → `ProtocolError::Io`
/// - `serde_json::Error` → `ProtocolError::Json`
/// - `rustls::Error` → `ProtocolError::Tls`
/// - `rcgen::Error` → `ProtocolError::Certificate`
///
/// # Examples
///
/// ```rust
/// use kdeconnect_protocol::ProtocolError;
///
/// // Create device-specific errors
/// let error = ProtocolError::DeviceNotFound("device-123".to_string());
/// assert_eq!(error.to_string(), "Device not found: device-123");
///
/// let error = ProtocolError::NotPaired;
/// assert_eq!(error.to_string(), "Not paired");
///
/// // Create packet errors
/// let error = ProtocolError::InvalidPacket("missing type field".to_string());
/// assert_eq!(error.to_string(), "Invalid packet: missing type field");
///
/// // Create plugin errors
/// let error = ProtocolError::Plugin("ping plugin failed".to_string());
/// assert_eq!(error.to_string(), "Plugin error: ping plugin failed");
/// ```
#[derive(Error, Debug)]
pub enum ProtocolError {
    /// I/O error (file system, network, etc.)
    ///
    /// Automatically converted from `std::io::Error`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use kdeconnect_protocol::Result;
    /// use std::fs::File;
    ///
    /// fn read_config() -> Result<()> {
    ///     // IO error automatically converts to ProtocolError::Io
    ///     let _file = File::open("/path/to/config.json")?;
    ///     Ok(())
    /// }
    /// ```
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error
    ///
    /// Automatically converted from `serde_json::Error`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use kdeconnect_protocol::{Packet, Result};
    ///
    /// fn parse_packet(json: &str) -> Result<Packet> {
    ///     // JSON error automatically converts to ProtocolError::Json
    ///     let packet: Packet = serde_json::from_str(json)?;
    ///     Ok(packet)
    /// }
    /// ```
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// TLS/SSL error (secure connections, certificate validation)
    ///
    /// Automatically converted from `openssl::ssl::Error`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use kdeconnect_protocol::Result;
    ///
    /// fn establish_secure_connection() -> Result<()> {
    ///     // TLS error automatically converts to ProtocolError::Tls
    ///     Ok(())
    /// }
    /// ```
    #[error("TLS error: {0}")]
    Tls(#[from] openssl::ssl::Error),

    /// Certificate generation or management error
    ///
    /// Automatically converted from `openssl::error::ErrorStack`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use kdeconnect_protocol::{CertificateInfo, Result};
    ///
    /// fn generate_certificate() -> Result<CertificateInfo> {
    ///     // Certificate error automatically converts to ProtocolError::Certificate
    ///     let cert = CertificateInfo::generate("my-device-id")?;
    ///     Ok(cert)
    /// }
    /// ```
    #[error("Certificate error: {0}")]
    Certificate(#[from] openssl::error::ErrorStack),

    /// cosmic-connect-core protocol error
    ///
    /// Automatically converted from `cosmic_connect_core::ProtocolError`.
    /// This enables seamless error propagation from the core TLS layer.
    #[error("Core protocol error: {0}")]
    CoreProtocol(#[from] cosmic_connect_core::ProtocolError),

    /// Certificate validation error
    ///
    /// This error occurs during TLS certificate validation.
    #[error("Certificate validation error: {0}")]
    CertificateValidation(String),

    /// Device not found in registry
    ///
    /// This error occurs when attempting to access a device that doesn't
    /// exist in the device registry.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kdeconnect_protocol::ProtocolError;
    ///
    /// let error = ProtocolError::DeviceNotFound("unknown-device".to_string());
    /// assert_eq!(error.to_string(), "Device not found: unknown-device");
    /// ```
    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    /// Device is not paired
    ///
    /// This error occurs when attempting an operation that requires a paired
    /// device, but the device is not currently paired.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kdeconnect_protocol::ProtocolError;
    ///
    /// let error = ProtocolError::NotPaired;
    /// assert_eq!(error.to_string(), "Not paired");
    /// ```
    #[error("Not paired")]
    NotPaired,

    /// Invalid or malformed packet
    ///
    /// This error occurs when a packet doesn't meet protocol requirements
    /// or contains invalid data.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kdeconnect_protocol::ProtocolError;
    ///
    /// let error = ProtocolError::InvalidPacket("missing type field".to_string());
    /// assert_eq!(error.to_string(), "Invalid packet: missing type field");
    /// ```
    #[error("Invalid packet: {0}")]
    InvalidPacket(String),

    /// Plugin-specific error
    ///
    /// This error occurs during plugin operations (initialization, packet
    /// handling, lifecycle management, etc.).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kdeconnect_protocol::ProtocolError;
    ///
    /// let error = ProtocolError::Plugin("failed to initialize plugin".to_string());
    /// assert_eq!(error.to_string(), "Plugin error: failed to initialize plugin");
    /// ```
    #[error("Plugin error: {0}")]
    Plugin(String),

    /// Network connection error
    ///
    /// This error occurs when a network connection fails or is interrupted.
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Connection timeout
    ///
    /// This error occurs when a network operation times out.
    #[error("Connection timeout: {0}")]
    Timeout(String),

    /// Connection refused
    ///
    /// This error occurs when a connection attempt is actively refused by the remote device.
    #[error("Connection refused: {0}")]
    ConnectionRefused(String),

    /// Network unreachable
    ///
    /// This error occurs when the network is unreachable (no route to host).
    #[error("Network unreachable: {0}")]
    NetworkUnreachable(String),

    /// Protocol version mismatch
    ///
    /// This error occurs when devices use incompatible protocol versions.
    #[error("Protocol version mismatch: {0}")]
    ProtocolVersionMismatch(String),

    /// Configuration error
    ///
    /// This error occurs when configuration is invalid or missing.
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Resource exhausted
    ///
    /// This error occurs when system resources are exhausted (disk full, memory pressure, etc.).
    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),

    /// Permission denied
    ///
    /// This error occurs when an operation fails due to insufficient permissions.
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Operation cancelled
    ///
    /// This error occurs when an operation is explicitly cancelled.
    #[error("Operation cancelled: {0}")]
    Cancelled(String),

    /// Packet size exceeded
    ///
    /// This error occurs when a packet exceeds maximum allowed size (DoS prevention).
    #[error("Packet size exceeded: {0} bytes (max: {1})")]
    PacketSizeExceeded(usize, usize),
}

impl ProtocolError {
    /// Convert a generic I/O error into a more specific network error
    ///
    /// This method examines the error kind and returns a more specific
    /// error variant when possible, providing better error messages to users.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kdeconnect_protocol::ProtocolError;
    /// use std::io::{Error, ErrorKind};
    ///
    /// let io_error = Error::new(ErrorKind::TimedOut, "connection timeout");
    /// let error = ProtocolError::from_io_error(io_error, "connecting to device");
    ///
    /// assert!(matches!(error, ProtocolError::Timeout(_)));
    /// ```
    pub fn from_io_error(error: std::io::Error, context: &str) -> Self {
        use std::io::ErrorKind;

        match error.kind() {
            ErrorKind::TimedOut => {
                ProtocolError::Timeout(format!("{}: {}", context, error))
            }
            ErrorKind::ConnectionRefused => {
                ProtocolError::ConnectionRefused(format!("{}: {}", context, error))
            }
            ErrorKind::NetworkUnreachable => {
                ProtocolError::NetworkUnreachable(format!("{}: {}", context, error))
            }
            ErrorKind::PermissionDenied => {
                ProtocolError::PermissionDenied(format!("{}: {}", context, error))
            }
            ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted | ErrorKind::BrokenPipe => {
                ProtocolError::NetworkError(format!("{}: connection interrupted ({})", context, error))
            }
            _ => ProtocolError::Io(error),
        }
    }

    /// Check if this error is recoverable (transient error that can be retried)
    ///
    /// Returns `true` if the error might succeed on retry, `false` if it's permanent.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kdeconnect_protocol::ProtocolError;
    ///
    /// let error = ProtocolError::Timeout("connection timeout".to_string());
    /// assert!(error.is_recoverable()); // Timeout can be retried
    ///
    /// let error = ProtocolError::NotPaired;
    /// assert!(!error.is_recoverable()); // Device needs to be paired first
    /// ```
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            ProtocolError::Timeout(_)
                | ProtocolError::NetworkError(_)
                | ProtocolError::NetworkUnreachable(_)
                | ProtocolError::ConnectionRefused(_)
                | ProtocolError::Io(_)
        )
    }

    /// Check if this error requires user action
    ///
    /// Returns `true` if the error cannot be resolved automatically and requires
    /// user intervention.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kdeconnect_protocol::ProtocolError;
    ///
    /// let error = ProtocolError::NotPaired;
    /// assert!(error.requires_user_action()); // User needs to pair device
    ///
    /// let error = ProtocolError::Timeout("connection timeout".to_string());
    /// assert!(!error.requires_user_action()); // Can be retried automatically
    /// ```
    pub fn requires_user_action(&self) -> bool {
        matches!(
            self,
            ProtocolError::NotPaired
                | ProtocolError::Certificate(_)
                | ProtocolError::CertificateValidation(_)
                | ProtocolError::PermissionDenied(_)
                | ProtocolError::Configuration(_)
                | ProtocolError::ProtocolVersionMismatch(_)
        )
    }

    /// Get a user-friendly error message suitable for display in UI
    ///
    /// This method returns a simplified, actionable error message that can be
    /// shown to users in notifications or error dialogs.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use kdeconnect_protocol::ProtocolError;
    ///
    /// let error = ProtocolError::NotPaired;
    /// assert_eq!(
    ///     error.user_message(),
    ///     "Device not paired. Please pair the device first."
    /// );
    /// ```
    pub fn user_message(&self) -> String {
        match self {
            ProtocolError::NotPaired => {
                "Device not paired. Please pair the device first.".to_string()
            }
            ProtocolError::DeviceNotFound(id) => {
                format!("Device '{}' not found. Check if the device is connected.", id)
            }
            ProtocolError::Timeout(msg) => {
                format!("Connection timeout: {}. Check network connection.", msg)
            }
            ProtocolError::ConnectionRefused(_) => {
                "Connection refused. Check if KDE Connect is running on the device.".to_string()
            }
            ProtocolError::NetworkUnreachable(_) => {
                "Network unreachable. Check if both devices are on the same network.".to_string()
            }
            ProtocolError::NetworkError(msg) => {
                format!("Network error: {}. Connection may be unstable.", msg)
            }
            ProtocolError::PermissionDenied(msg) => {
                format!("Permission denied: {}. Check file and directory permissions.", msg)
            }
            ProtocolError::ResourceExhausted(msg) => {
                format!("Resource exhausted: {}. Free up space and try again.", msg)
            }
            ProtocolError::Configuration(msg) => {
                format!("Configuration error: {}. Check your settings.", msg)
            }
            ProtocolError::ProtocolVersionMismatch(msg) => {
                format!("Incompatible protocol version: {}. Update both applications.", msg)
            }
            ProtocolError::CertificateValidation(msg) => {
                format!("Certificate validation failed: {}. You may need to re-pair.", msg)
            }
            ProtocolError::PacketSizeExceeded(size, max) => {
                format!("Packet too large ({} bytes, max {} bytes). Try sending smaller files.", size, max)
            }
            ProtocolError::InvalidPacket(msg) => {
                format!("Invalid data received: {}.", msg)
            }
            ProtocolError::Plugin(msg) => {
                format!("Plugin error: {}.", msg)
            }
            ProtocolError::Cancelled(msg) => {
                format!("Operation cancelled: {}.", msg)
            }
            ProtocolError::Io(e) => {
                format!("I/O error: {}.", e)
            }
            ProtocolError::Json(e) => {
                format!("Data format error: {}.", e)
            }
            ProtocolError::Tls(e) => {
                format!("Secure connection error: {}.", e)
            }
            ProtocolError::Certificate(e) => {
                format!("Certificate error: {}. You may need to re-pair.", e)
            }
            ProtocolError::CoreProtocol(e) => {
                format!("Core protocol error: {}.", e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let error = ProtocolError::DeviceNotFound("test-device".to_string());
        assert_eq!(error.to_string(), "Device not found: test-device");

        let error = ProtocolError::NotPaired;
        assert_eq!(error.to_string(), "Not paired");

        let error = ProtocolError::InvalidPacket("bad format".to_string());
        assert_eq!(error.to_string(), "Invalid packet: bad format");

        let error = ProtocolError::Plugin("initialization failed".to_string());
        assert_eq!(error.to_string(), "Plugin error: initialization failed");
    }

    #[test]
    fn test_io_error_conversion() {
        use std::io::{Error, ErrorKind};

        let io_error = Error::new(ErrorKind::NotFound, "file not found");
        let protocol_error: ProtocolError = io_error.into();

        assert!(matches!(protocol_error, ProtocolError::Io(_)));
        assert!(protocol_error.to_string().contains("file not found"));
    }

    #[test]
    fn test_json_error_conversion() {
        let json = r#"{"invalid json"#;
        let json_error = serde_json::from_str::<serde_json::Value>(json).unwrap_err();
        let protocol_error: ProtocolError = json_error.into();

        assert!(matches!(protocol_error, ProtocolError::Json(_)));
    }
}
