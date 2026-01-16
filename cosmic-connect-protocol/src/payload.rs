//! Payload Transfer System
//!
//! Handles TCP-based file transfers for the Share plugin.
//! Implements the CConnect payload transfer protocol.
//!
//! ## Protocol
//!
//! File transfers in CConnect use TCP:
//! 1. Sender creates a TCP server on an available port (1739+ range)
//! 2. Sender sends a share packet with file metadata and port
//! 3. Receiver connects to sender's IP:port
//! 4. Raw file bytes are streamed
//! 5. Connection closes when all bytes transferred
//!
//! ## Usage
//!
//! ### Sending a File
//!
//! ```rust,ignore
//! use cosmic_connect_core::payload::{PayloadServer, FileTransferInfo};
//!
//! // Create file info
//! let file_info = FileTransferInfo::from_path("/path/to/file.pdf").await?;
//!
//! // Start payload server
//! let server = PayloadServer::new().await?;
//! let port = server.port();
//!
//! // Send share packet with port info
//! let packet = share_plugin.create_file_packet(file_info.into(), port);
//! // ... send packet via connection ...
//!
//! // Accept connection and transfer file
//! server.send_file("/path/to/file.pdf").await?;
//! ```
//!
//! ### Receiving a File
//!
//! ```rust,ignore
//! use cosmic_connect_core::payload::PayloadClient;
//!
//! // Extract info from received packet
//! let filename = packet.body["filename"].as_str().unwrap();
//! let size = packet.payload_size.unwrap();
//! let port = packet.payload_transfer_info["port"].as_u64().unwrap() as u16;
//!
//! // Connect and receive file
//! let client = PayloadClient::new(remote_addr, port).await?;
//! client.receive_file("/path/to/save/file.pdf", size).await?;
//! ```

use crate::fs_utils::{cleanup_partial_file, create_file_safe, write_file_safe};
use crate::{ProtocolError, Result};
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Duration};
use tracing::{debug, info, warn};

/// Default timeout for TCP connections (30 seconds)
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(30);

/// Default timeout for read/write operations (60 seconds)
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(60);

/// Buffer size for file streaming (64KB)
const BUFFER_SIZE: usize = 65536;

/// Port range for payload servers (CConnect standard)
const PORT_RANGE_START: u16 = 1739;
const PORT_RANGE_END: u16 = 1764;

/// Information about a file to be transferred
///
/// Contains metadata extracted from the filesystem.
#[derive(Debug, Clone)]
pub struct FileTransferInfo {
    /// File name (with extension)
    pub filename: String,

    /// File size in bytes
    pub size: u64,

    /// File path for reading
    pub path: String,

    /// Creation time (UNIX milliseconds)
    pub creation_time: Option<i64>,

    /// Last modified time (UNIX milliseconds)
    pub last_modified: Option<i64>,
}

impl FileTransferInfo {
    /// Extract file info from a path
    ///
    /// Reads file metadata including size and timestamps.
    ///
    /// # Errors
    ///
    /// Returns error if file doesn't exist or metadata cannot be read.
    pub async fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| ProtocolError::Io(e))?;

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| ProtocolError::InvalidPacket("Invalid filename".to_string()))?
            .to_string();

        let size = metadata.len();

        // Extract timestamps
        let creation_time = metadata
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64);

        let last_modified = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64);

        Ok(Self {
            filename,
            size,
            path: path.to_string_lossy().to_string(),
            creation_time,
            last_modified,
        })
    }
}

/// Converts FileTransferInfo to Share plugin's FileShareInfo
impl From<FileTransferInfo> for crate::plugins::share::FileShareInfo {
    fn from(info: FileTransferInfo) -> Self {
        Self {
            filename: info.filename,
            size: info.size as i64,
            creation_time: info.creation_time,
            last_modified: info.last_modified,
            open: false,
        }
    }
}

/// Progress callback for file transfers
///
/// Reports transferred bytes and total expected size.
/// Return `false` to cancel the transfer.
pub type ProgressCallback = Box<dyn Fn(u64, u64) -> bool + Send + Sync>;

/// TCP server for sending file payloads
///
/// Listens on an available port and accepts a single connection
/// to transfer file data.
pub struct PayloadServer {
    listener: TcpListener,
    port: u16,
    progress_callback: Option<ProgressCallback>,
}

impl PayloadServer {
    /// Create a new payload server on an available port
    ///
    /// Binds to 0.0.0.0 in the CConnect port range (1739-1764).
    ///
    /// # Errors
    ///
    /// Returns error if no ports are available in the range.
    pub async fn new() -> Result<Self> {
        // Try to bind to a port in the CConnect range
        for port in PORT_RANGE_START..=PORT_RANGE_END {
            let addr = format!("0.0.0.0:{}", port);
            if let Ok(listener) = TcpListener::bind(&addr).await {
                info!("Payload server listening on port {}", port);
                return Ok(Self {
                    listener,
                    port,
                    progress_callback: None,
                });
            }
        }

        Err(ProtocolError::Io(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!(
                "Failed to bind payload server - all ports in range {}-{} are in use",
                PORT_RANGE_START, PORT_RANGE_END
            ),
        )))
    }

    /// Set a progress callback for transfer updates
    ///
    /// The callback receives (bytes_transferred, total_bytes) and returns
    /// `true` to continue or `false` to cancel the transfer.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let server = PayloadServer::new().await?;
    /// server.with_progress(Box::new(|transferred, total| {
    ///     println!("Progress: {}/{} bytes", transferred, total);
    ///     true // continue transfer
    /// }));
    /// ```
    pub fn with_progress(mut self, callback: ProgressCallback) -> Self {
        self.progress_callback = Some(callback);
        self
    }

    /// Get the port this server is listening on
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get the socket address this server is bound to
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.listener
            .local_addr()
            .map_err(|e| ProtocolError::Io(e))
    }

    /// Accept a connection and send a file
    ///
    /// Waits for exactly one connection, then streams the file.
    /// The connection is closed after the file is fully sent.
    ///
    /// # Parameters
    ///
    /// - `file_path`: Path to the file to send
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Connection times out
    /// - File cannot be opened
    /// - Transfer fails or is interrupted
    /// - Transfer is cancelled via progress callback
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let server = PayloadServer::new().await?;
    /// server.send_file("/path/to/document.pdf").await?;
    /// ```
    pub async fn send_file(self, file_path: impl AsRef<Path>) -> Result<()> {
        let file_path = file_path.as_ref();
        info!("Waiting for connection to send file: {:?}", file_path);

        // Get file size for progress tracking
        let file_size = tokio::fs::metadata(file_path)
            .await
            .map_err(|e| ProtocolError::Io(e))?
            .len();

        // Accept connection with timeout
        let (mut stream, remote_addr) = timeout(CONNECTION_TIMEOUT, self.listener.accept())
            .await
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Connection timeout",
                ))
            })?
            .map_err(|e| ProtocolError::Io(e))?;

        info!("Accepted connection from {} for file transfer", remote_addr);

        // Open file
        let mut file = File::open(file_path)
            .await
            .map_err(|e| ProtocolError::Io(e))?;

        // Stream file data
        let mut buffer = vec![0u8; BUFFER_SIZE];
        let mut total_bytes = 0u64;

        loop {
            // Read from file
            let bytes_read = timeout(TRANSFER_TIMEOUT, file.read(&mut buffer))
                .await
                .map_err(|_| {
                    ProtocolError::Io(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "File read timeout",
                    ))
                })?
                .map_err(|e| ProtocolError::Io(e))?;

            if bytes_read == 0 {
                break; // EOF
            }

            // Write to stream
            timeout(
                TRANSFER_TIMEOUT,
                stream.write_all(&buffer[..bytes_read]),
            )
            .await
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Stream write timeout",
                ))
            })?
            .map_err(|e| ProtocolError::Io(e))?;

            total_bytes += bytes_read as u64;

            debug!(
                "Transferred {} bytes ({}/{} total)",
                bytes_read, total_bytes, file_size
            );

            // Call progress callback if set
            if let Some(ref callback) = self.progress_callback {
                if !callback(total_bytes, file_size) {
                    info!("Transfer cancelled by progress callback");
                    return Err(ProtocolError::Io(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "Transfer cancelled",
                    )));
                }
            }
        }

        // Flush stream
        stream.flush().await.map_err(|e| ProtocolError::Io(e))?;

        info!(
            "File transfer complete: {} bytes sent to {}",
            total_bytes, remote_addr
        );

        Ok(())
    }
}

/// TCP client for receiving file payloads
///
/// Connects to a remote payload server and downloads file data.
pub struct PayloadClient {
    stream: TcpStream,
    progress_callback: Option<ProgressCallback>,
}

impl PayloadClient {
    /// Connect to a remote payload server
    ///
    /// # Parameters
    ///
    /// - `host`: Remote host IP address
    /// - `port`: Remote port number
    ///
    /// # Errors
    ///
    /// Returns error if connection fails or times out.
    pub async fn new(host: impl ToSocketAddrs, port: u16) -> Result<Self> {
        let addrs: Vec<SocketAddr> = host
            .to_socket_addrs()
            .map_err(|e| ProtocolError::Io(e))?
            .collect();

        if addrs.is_empty() {
            return Err(ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "No addresses resolved for host",
            )));
        }

        // Try first address with port
        let addr = SocketAddr::new(addrs[0].ip(), port);
        info!("Connecting to payload server at {}", addr);

        let stream = timeout(CONNECTION_TIMEOUT, TcpStream::connect(&addr))
            .await
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Connection timeout",
                ))
            })?
            .map_err(|e| ProtocolError::Io(e))?;

        info!("Connected to payload server at {}", addr);

        Ok(Self {
            stream,
            progress_callback: None,
        })
    }

    /// Set a progress callback for transfer updates
    ///
    /// The callback receives (bytes_transferred, total_bytes) and returns
    /// `true` to continue or `false` to cancel the transfer.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let client = PayloadClient::new("192.168.1.100", 1739).await?;
    /// client.with_progress(Box::new(|transferred, total| {
    ///     println!("Progress: {}/{} bytes", transferred, total);
    ///     true // continue transfer
    /// }));
    /// ```
    pub fn with_progress(mut self, callback: ProgressCallback) -> Self {
        self.progress_callback = Some(callback);
        self
    }

    /// Receive a file from the connected server
    ///
    /// Downloads the specified number of bytes and saves to a file.
    ///
    /// # Parameters
    ///
    /// - `save_path`: Path where the file should be saved
    /// - `expected_size`: Expected file size in bytes
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - File cannot be created
    /// - Transfer fails or times out
    /// - Size mismatch (received != expected)
    /// - Transfer is cancelled via progress callback
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let client = PayloadClient::new("192.168.1.100", 1739).await?;
    /// client.receive_file("/tmp/received_file.pdf", 1048576).await?;
    /// ```
    pub async fn receive_file(
        mut self,
        save_path: impl AsRef<Path>,
        expected_size: u64,
    ) -> Result<()> {
        let save_path = save_path.as_ref();
        info!(
            "Receiving file to {:?} ({} bytes expected)",
            save_path, expected_size
        );

        // Create file with safe error handling
        let mut file = match create_file_safe(save_path).await {
            Ok(f) => f,
            Err(e) => {
                warn!("Failed to create file {:?}: {}", save_path, e);
                return Err(e);
            }
        };

        // Read and write data
        let mut buffer = vec![0u8; BUFFER_SIZE];
        let mut total_bytes = 0u64;

        let result = async {
            while total_bytes < expected_size {
                let remaining = expected_size - total_bytes;
                let to_read = std::cmp::min(remaining, BUFFER_SIZE as u64) as usize;

                // Read from stream
                let bytes_read = timeout(TRANSFER_TIMEOUT, self.stream.read(&mut buffer[..to_read]))
                    .await
                    .map_err(|_| {
                        ProtocolError::Timeout("Stream read timeout during file transfer".to_string())
                    })?
                    .map_err(|e| ProtocolError::Io(e))?;

                if bytes_read == 0 {
                    return Err(ProtocolError::Io(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        format!(
                            "Connection closed prematurely: received {} bytes, expected {}",
                            total_bytes, expected_size
                        ),
                    )));
                }

                // Write to file with safe error handling
                write_file_safe(&mut file, &buffer[..bytes_read]).await?;

                total_bytes += bytes_read as u64;

                debug!(
                    "Received {} bytes ({}/{} total)",
                    bytes_read, total_bytes, expected_size
                );

                // Call progress callback if set
                if let Some(ref callback) = self.progress_callback {
                    if !callback(total_bytes, expected_size) {
                        info!("Transfer cancelled by progress callback");
                        return Err(ProtocolError::Io(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "Transfer cancelled",
                        )));
                    }
                }
            }

            // Flush file
            file.flush().await.map_err(|e| ProtocolError::Io(e))?;

            info!(
                "File transfer complete: {} bytes received to {:?}",
                total_bytes, save_path
            );

            Ok(())
        }
        .await;

        // Clean up partial file on error
        if result.is_err() {
            warn!(
                "Transfer failed, cleaning up partial file: {:?}",
                save_path
            );
            cleanup_partial_file(save_path).await;
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_file_transfer_info_from_path() {
        // Create temporary file
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"test content").unwrap();
        temp_file.flush().unwrap();

        let info = FileTransferInfo::from_path(temp_file.path())
            .await
            .unwrap();

        assert_eq!(info.size, 12);
        assert!(info.filename.len() > 0);
        assert!(info.last_modified.is_some());
    }

    #[tokio::test]
    async fn test_payload_server_creation() {
        let server = PayloadServer::new().await.unwrap();
        let port = server.port();

        assert!(port >= PORT_RANGE_START);
        assert!(port <= PORT_RANGE_END);
    }

    #[tokio::test]
    async fn test_file_transfer_round_trip() {
        // Create temporary source file with test data
        let mut source_file = NamedTempFile::new().unwrap();
        let test_data = b"Hello, this is a test file for payload transfer!";
        source_file.write_all(test_data).unwrap();
        source_file.flush().unwrap();
        let source_path = source_file.path().to_owned();

        // Create temporary destination file
        let dest_file = NamedTempFile::new().unwrap();
        let dest_path = dest_file.path().to_owned();

        // Start server
        let server = PayloadServer::new().await.unwrap();
        let port = server.port();

        // Spawn server task
        let source_path_clone = source_path.clone();
        let server_task = tokio::spawn(async move {
            server.send_file(source_path_clone).await
        });

        // Give server time to start listening
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Connect client and receive file
        let client = PayloadClient::new("127.0.0.1", port).await.unwrap();
        client
            .receive_file(&dest_path, test_data.len() as u64)
            .await
            .unwrap();

        // Wait for server to complete
        server_task.await.unwrap().unwrap();

        // Verify file contents
        let received_data = tokio::fs::read(&dest_path).await.unwrap();
        assert_eq!(&received_data[..], test_data);
    }

    #[tokio::test]
    async fn test_file_transfer_info_conversion() {
        let transfer_info = FileTransferInfo {
            filename: "test.txt".to_string(),
            size: 1024,
            path: "/tmp/test.txt".to_string(),
            creation_time: Some(1640000000000),
            last_modified: Some(1640000000000),
        };

        let share_info: crate::plugins::share::FileShareInfo = transfer_info.into();

        assert_eq!(share_info.filename, "test.txt");
        assert_eq!(share_info.size, 1024);
        assert_eq!(share_info.creation_time, Some(1640000000000));
        assert_eq!(share_info.last_modified, Some(1640000000000));
        assert_eq!(share_info.open, false);
    }

    #[tokio::test]
    async fn test_connection_timeout() {
        let server = PayloadServer::new().await.unwrap();

        // Don't connect - should timeout
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            server.send_file("/dev/null")
        ).await;

        assert!(result.is_err() || result.unwrap().is_err());
    }

    #[tokio::test]
    async fn test_invalid_file_path() {
        let server = PayloadServer::new().await.unwrap();
        let port = server.port();

        // Start server with invalid file path
        let server_task = tokio::spawn(async move {
            server.send_file("/nonexistent/file.txt").await
        });

        // Connect client
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = TcpStream::connect(format!("127.0.0.1:{}", port)).await;

        // Server should fail
        let result = server_task.await.unwrap();
        assert!(result.is_err());
    }
}
