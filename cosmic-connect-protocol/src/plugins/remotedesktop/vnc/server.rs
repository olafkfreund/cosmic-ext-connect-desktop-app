//! VNC Server Implementation
//!
//! Implements RFB Protocol 3.8 server for remote desktop access.
//!
//! ## Architecture
//!
//! ```text
//! VncServer
//! ├── TCP Listener (port 5900)
//! ├── Client Connection Handler
//! │   ├── RFB Handshake
//! │   ├── Authentication
//! │   └── Protocol Loop
//! └── Streaming Session
//!     └── Framebuffer Updates
//! ```
//!
//! ## Usage
//!
//! ```rust,no_run
//! # use cosmic_connect_protocol::plugins::remotedesktop::vnc::VncServer;
//! # use cosmic_connect_protocol::plugins::remotedesktop::capture::WaylandCapture;
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let capture = WaylandCapture::new().await?;
//! let server = VncServer::new(5900, "password123".to_string());
//! server.start(capture).await?;
//! # Ok(())
//! # }
//! ```

use super::{
    auth::{generate_password, VncAuth},
    protocol::*,
    StreamConfig, StreamingSession,
};
use crate::{
    plugins::remotedesktop::{
        capture::{EncodedFrame, EncodingType, QualityPreset, WaylandCapture},
        input::InputHandler,
    },
    Result,
};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// VNC server state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerState {
    /// Not started
    Idle,
    /// Listening for connections
    Listening,
    /// Client connected
    Connected,
    /// Stopped
    Stopped,
}

/// VNC server for RFB protocol
#[cfg(feature = "remotedesktop")]
pub struct VncServer {
    /// TCP port to listen on
    port: u16,

    /// VNC password
    password: String,

    /// Server state
    state: Arc<RwLock<ServerState>>,

    /// Framebuffer dimensions
    width: u16,
    height: u16,
}

#[cfg(feature = "remotedesktop")]
impl VncServer {
    /// Create new VNC server
    ///
    /// # Arguments
    ///
    /// * `port` - TCP port to listen on (typically 5900)
    /// * `password` - VNC authentication password (empty for no auth)
    pub fn new(port: u16, password: String) -> Self {
        info!("Creating VNC server on port {}", port);

        Self {
            port,
            password,
            state: Arc::new(RwLock::new(ServerState::Idle)),
            width: 1920,
            height: 1080,
        }
    }

    /// Create VNC server with auto-generated password
    pub fn with_generated_password(port: u16) -> (Self, String) {
        let password = generate_password();
        let server = Self::new(port, password.clone());
        (server, password)
    }

    /// Get current server state
    pub async fn state(&self) -> ServerState {
        *self.state.read().await
    }

    /// Start VNC server and begin accepting connections
    pub async fn start(&mut self, capture: WaylandCapture) -> Result<()> {
        info!("Starting VNC server on port {}", self.port);

        // Update state
        *self.state.write().await = ServerState::Listening;

        // Create streaming session
        let config = StreamConfig {
            target_fps: 30,
            quality: QualityPreset::Medium,
            buffer_size: 3,
            allow_frame_skip: true,
        };

        let mut session = StreamingSession::new(config);

        // Get framebuffer dimensions from capture
        // For now, use defaults; in production, get from capture monitors
        self.width = 1920;
        self.height = 1080;

        // Start streaming session
        session.start(capture).await?;

        // Bind TCP listener
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = TcpListener::bind(&addr).map_err(|e| {
            crate::ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!("Failed to bind to {}: {}", addr, e),
            ))
        })?;

        info!("VNC server listening on {}", addr);

        // Accept single connection (first client wins)
        match listener.accept() {
            Ok((stream, addr)) => {
                info!("Client connected from {}", addr);
                *self.state.write().await = ServerState::Connected;

                // Handle client connection
                if let Err(e) = self.handle_client(stream, session).await {
                    error!("Client connection error: {}", e);
                }
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
            }
        }

        *self.state.write().await = ServerState::Stopped;
        info!("VNC server stopped");

        Ok(())
    }

    /// Handle client connection
    async fn handle_client(
        &self,
        mut stream: TcpStream,
        mut session: StreamingSession,
    ) -> Result<()> {
        info!("Handling client connection");

        // RFB handshake
        self.perform_handshake(&mut stream).await?;

        // Client initialization
        let _shared_flag = self.handle_client_init(&mut stream)?;

        // Send server initialization
        self.send_server_init(&mut stream)?;

        // Create input handler for forwarding keyboard/mouse events
        let mut input_handler = InputHandler::new()?;
        info!("Input handler created for VNC input forwarding");

        // Protocol message loop
        self.protocol_loop(&mut stream, &mut session, &mut input_handler)
            .await?;

        info!("Client disconnected");
        Ok(())
    }

    /// Perform RFB protocol handshake
    async fn perform_handshake(&self, stream: &mut TcpStream) -> Result<()> {
        info!("Starting RFB handshake");

        // 1. Send protocol version
        debug!("Sending protocol version: RFB 003.008");
        stream.write_all(RFB_VERSION_3_8)?;

        // 2. Read client protocol version
        let mut client_version = [0u8; 12];
        stream.read_exact(&mut client_version)?;
        debug!(
            "Client version: {:?}",
            String::from_utf8_lossy(&client_version)
        );

        // Verify client version
        if client_version != *RFB_VERSION_3_8 && client_version != *RFB_VERSION_3_3 {
            warn!("Unsupported client version: {:?}", client_version);
            return Err(crate::ProtocolError::Plugin(
                "Unsupported RFB version".to_string(),
            ));
        }

        // 3. Send security types
        if self.password.is_empty() {
            debug!("Sending security type: None");
            stream.write_all(&[1, SECURITY_NONE])?;
        } else {
            debug!("Sending security type: VNC Auth");
            stream.write_all(&[1, SECURITY_VNC_AUTH])?;
        }

        // 4. Read client's chosen security type
        let mut security_type = [0u8; 1];
        stream.read_exact(&mut security_type)?;
        debug!("Client chose security type: {}", security_type[0]);

        // 5. Perform authentication if needed
        if security_type[0] == SECURITY_VNC_AUTH {
            info!("Performing VNC authentication");
            let auth = VncAuth::new(self.password.clone());

            if !auth.authenticate(stream).await? {
                // Send failure
                stream.write_u32(SECURITY_RESULT_FAILED)?;
                return Err(crate::ProtocolError::Plugin(
                    "VNC authentication failed".to_string(),
                ));
            }
        }

        // 6. Send security result: OK
        debug!("Sending security result: OK");
        stream.write_u32(SECURITY_RESULT_OK)?;

        info!("RFB handshake completed successfully");
        Ok(())
    }

    /// Handle client initialization
    fn handle_client_init(&self, stream: &mut TcpStream) -> Result<bool> {
        debug!("Waiting for ClientInit");

        let mut shared_flag = [0u8; 1];
        stream.read_exact(&mut shared_flag)?;

        debug!("Client shared flag: {}", shared_flag[0] != 0);
        Ok(shared_flag[0] != 0)
    }

    /// Send server initialization
    fn send_server_init(&self, stream: &mut TcpStream) -> Result<()> {
        debug!("Sending ServerInit");

        let init = ServerInit::new(self.width, self.height, "COSMIC Desktop".to_string());
        let bytes = init.to_bytes();

        stream.write_all(&bytes)?;

        info!("Server initialization sent: {}x{}", self.width, self.height);
        Ok(())
    }

    /// Protocol message loop
    async fn protocol_loop(
        &self,
        stream: &mut TcpStream,
        session: &mut StreamingSession,
        input_handler: &mut InputHandler,
    ) -> Result<()> {
        info!("Entering protocol loop");

        // Client encodings (supported by client)
        let mut client_encodings: Vec<RfbEncoding> = Vec::new();

        // Set stream to non-blocking for frame updates
        stream.set_nonblocking(true).ok();

        loop {
            // Try to read client message
            let mut msg_type = [0u8; 1];
            match stream.read_exact(&mut msg_type) {
                Ok(_) => {
                    if let Some(msg) = ClientMessage::from_u8(msg_type[0]) {
                        debug!("Received client message: {:?}", msg);

                        match msg {
                            ClientMessage::SetPixelFormat => {
                                self.handle_set_pixel_format(stream)?;
                            }
                            ClientMessage::SetEncodings => {
                                client_encodings = self.handle_set_encodings(stream)?;
                            }
                            ClientMessage::FramebufferUpdateRequest => {
                                let req = FramebufferUpdateRequest::from_reader(stream)?;
                                self.handle_framebuffer_update_request(stream, session, req)
                                    .await?;
                            }
                            ClientMessage::KeyEvent => {
                                let event = KeyEvent::from_reader(stream)?;
                                self.handle_key_event(event, input_handler).await?;
                            }
                            ClientMessage::PointerEvent => {
                                let event = PointerEvent::from_reader(stream)?;
                                self.handle_pointer_event(event, input_handler).await?;
                            }
                            ClientMessage::ClientCutText => {
                                self.handle_client_cut_text(stream)?;
                            }
                        }
                    } else {
                        warn!("Unknown client message type: {}", msg_type[0]);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No message available, continue
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
                Err(e) => {
                    error!("Error reading client message: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle SetPixelFormat message
    fn handle_set_pixel_format(&self, stream: &mut TcpStream) -> Result<()> {
        debug!("Handling SetPixelFormat");

        // Read pixel format (16 bytes) + padding (3 bytes)
        let mut buf = [0u8; 19];
        stream.read_exact(&mut buf)?;

        // Extract pixel format
        let mut pf_bytes = [0u8; 16];
        pf_bytes.copy_from_slice(&buf[3..19]);
        let pixel_format = PixelFormat::from_bytes(&pf_bytes);

        debug!("Client pixel format: {:?}", pixel_format);
        Ok(())
    }

    /// Handle SetEncodings message
    fn handle_set_encodings(&self, stream: &mut TcpStream) -> Result<Vec<RfbEncoding>> {
        debug!("Handling SetEncodings");

        // Read padding (1 byte) + number of encodings (2 bytes)
        let mut header = [0u8; 3];
        stream.read_exact(&mut header)?;

        let num_encodings = u16::from_be_bytes([header[1], header[2]]);
        debug!("Client supports {} encodings", num_encodings);

        // Read encoding types
        let mut encodings = Vec::new();
        for _ in 0..num_encodings {
            let mut encoding_bytes = [0u8; 4];
            stream.read_exact(&mut encoding_bytes)?;
            let encoding_value = i32::from_be_bytes(encoding_bytes);

            if let Some(encoding) = RfbEncoding::from_i32(encoding_value) {
                debug!("  - {:?}", encoding);
                encodings.push(encoding);
            } else {
                debug!("  - Unknown encoding: {}", encoding_value);
            }
        }

        Ok(encodings)
    }

    /// Handle FramebufferUpdateRequest message
    async fn handle_framebuffer_update_request(
        &self,
        stream: &mut TcpStream,
        session: &mut StreamingSession,
        req: FramebufferUpdateRequest,
    ) -> Result<()> {
        debug!("Handling FramebufferUpdateRequest: {:?}", req);

        // Get next frame from streaming session
        if let Some(encoded_frame) = session.next_frame().await {
            self.send_framebuffer_update(stream, &encoded_frame)?;
        }

        Ok(())
    }

    /// Send framebuffer update to client
    fn send_framebuffer_update(&self, stream: &mut TcpStream, frame: &EncodedFrame) -> Result<()> {
        // Map our encoding type to RFB encoding
        let rfb_encoding = match frame.encoding {
            EncodingType::Raw => RfbEncoding::Raw as i32,
            EncodingType::LZ4 => RfbEncoding::Raw as i32, // Send LZ4 as raw for now
            EncodingType::H264 => RfbEncoding::Raw as i32,
            EncodingType::Hextile => RfbEncoding::Hextile as i32,
        };

        // Create rectangle with frame data
        let rect = Rectangle::new(
            0,
            0,
            frame.width as u16,
            frame.height as u16,
            rfb_encoding,
            frame.data.clone(),
        );

        // Create framebuffer update
        let update = FramebufferUpdate::new(vec![rect]);

        // Send update
        let bytes = update.to_bytes();
        stream.write_all(&bytes)?;

        debug!(
            "Sent framebuffer update: {}x{} ({} bytes)",
            frame.width,
            frame.height,
            bytes.len()
        );

        Ok(())
    }

    /// Handle KeyEvent message
    async fn handle_key_event(
        &self,
        event: KeyEvent,
        input_handler: &mut InputHandler,
    ) -> Result<()> {
        debug!("Key event: down={}, key=0x{:08x}", event.down, event.key);

        // Forward to input handler
        input_handler
            .handle_key_event(event.key, event.down)
            .await?;

        Ok(())
    }

    /// Handle PointerEvent message
    async fn handle_pointer_event(
        &self,
        event: PointerEvent,
        input_handler: &mut InputHandler,
    ) -> Result<()> {
        debug!(
            "Pointer event: buttons=0x{:02x}, pos=({}, {})",
            event.button_mask, event.x, event.y
        );

        // Forward to input handler
        input_handler
            .handle_pointer_event(event.x, event.y, event.button_mask)
            .await?;

        Ok(())
    }

    /// Handle ClientCutText message
    fn handle_client_cut_text(&self, stream: &mut TcpStream) -> Result<()> {
        debug!("Handling ClientCutText");

        // Read padding (3 bytes) + length (4 bytes)
        let mut header = [0u8; 7];
        stream.read_exact(&mut header)?;

        let text_len = u32::from_be_bytes([header[3], header[4], header[5], header[6]]);

        // Read text
        let mut text_buf = vec![0u8; text_len as usize];
        stream.read_exact(&mut text_buf)?;

        debug!("Client cut text: {:?}", String::from_utf8_lossy(&text_buf));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vnc_server_creation() {
        let server = VncServer::new(5900, "test123".to_string());
        assert_eq!(server.port, 5900);
        assert_eq!(server.password, "test123");
    }

    #[test]
    fn test_vnc_server_with_generated_password() {
        let (server, password) = VncServer::with_generated_password(5900);
        assert_eq!(server.port, 5900);
        assert_eq!(password.len(), 8);
        assert_eq!(server.password, password);
    }

    #[tokio::test]
    async fn test_server_state() {
        let server = VncServer::new(5900, String::new());
        assert_eq!(server.state().await, ServerState::Idle);
    }
}
