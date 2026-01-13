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
    /// Automatically converted from `rustls::Error`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use kdeconnect_protocol::Result;
    /// use rustls::ClientConnection;
    ///
    /// fn establish_secure_connection() -> Result<()> {
    ///     // TLS error automatically converts to ProtocolError::Tls
    ///     // let connection = ClientConnection::new(...)?;
    ///     Ok(())
    /// }
    /// ```
    #[error("TLS error: {0}")]
    Tls(#[from] rustls::Error),

    /// Certificate generation or management error
    ///
    /// Automatically converted from `rcgen::Error`.
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
    Certificate(#[from] rcgen::Error),

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
