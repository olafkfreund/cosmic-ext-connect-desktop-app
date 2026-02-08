//! Network streaming module for WebRTC-based video transmission
//!
//! This module provides real-time video streaming functionality using WebRTC,
//! enabling low-latency transmission of encoded video frames to Android tablets.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
//! │  EncodedFrame   │────▶│  StreamingServer │────▶│  Android Client │
//! │  (H.264 NALs)   │     │  (WebRTC)        │     │  (WebRTC)       │
//! └─────────────────┘     └──────────────────┘     └─────────────────┘
//!                                │
//!                                │ Signaling
//!                                ▼
//!                         ┌──────────────────┐
//!                         │  SignalingServer │
//!                         │  (WebSocket)     │
//!                         └──────────────────┘
//! ```
//!
//! ## Features
//!
//! - WebRTC peer connections with ICE for NAT traversal
//! - WebSocket-based signaling server
//! - Support for `WiFi` and USB (ADB port forwarding) connections
//! - Connection statistics and monitoring
//! - Adaptive bitrate hints
//!
//! ## Example
//!
//! ```no_run
//! use cosmic_display_stream::streaming::{StreamingServer, StreamConfig, TransportMode};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create streaming server
//! let config = StreamConfig::default()
//!     .with_signaling_port(8080)
//!     .with_transport(TransportMode::WiFi);
//!
//! let mut server = StreamingServer::new(config)?;
//!
//! // Start the server
//! server.start().await?;
//!
//! // Send encoded frames
//! // server.send_frame(encoded_frame).await?;
//!
//! // Get connection stats
//! if let Some(stats) = server.get_stats().await {
//!     println!("RTT: {}ms, Bitrate: {} bps", stats.rtt_ms, stats.bitrate_bps);
//! }
//! # Ok(())
//! # }
//! ```

use crate::encoder::EncodedFrame;
use crate::error::{DisplayStreamError, Result};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{debug, error, info, warn};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::{TrackLocal, TrackLocalWriter};

/// Transport mode for streaming
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportMode {
    /// Stream over `WiFi` network
    #[default]
    WiFi,
    /// Stream over USB via ADB port forwarding
    Usb,
}

impl TransportMode {
    /// Get a human-readable name for this transport mode
    #[must_use] 
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::WiFi => "WiFi",
            Self::Usb => "USB (ADB)",
        }
    }
}

/// Streaming server configuration
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Port for WebSocket signaling server
    pub signaling_port: u16,
    /// Bind address for the server
    pub bind_address: String,
    /// Transport mode (`WiFi` or USB)
    pub transport: TransportMode,
    /// STUN servers for ICE
    pub stun_servers: Vec<String>,
    /// Maximum number of concurrent clients
    pub max_clients: usize,
    /// Enable connection encryption (DTLS)
    pub enable_encryption: bool,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            signaling_port: 8080,
            bind_address: "0.0.0.0".to_string(),
            transport: TransportMode::WiFi,
            stun_servers: vec![
                "stun:stun.l.google.com:19302".to_string(),
                "stun:stun1.l.google.com:19302".to_string(),
            ],
            max_clients: 1, // Single tablet for now
            enable_encryption: true,
        }
    }
}

impl StreamConfig {
    /// Create a new streaming configuration with default values
    #[must_use] 
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the signaling server port
    #[must_use] 
    pub fn with_signaling_port(mut self, port: u16) -> Self {
        self.signaling_port = port;
        self
    }

    /// Set the bind address
    #[must_use]
    pub fn with_bind_address(mut self, address: impl Into<String>) -> Self {
        self.bind_address = address.into();
        self
    }

    /// Set the transport mode
    #[must_use] 
    pub fn with_transport(mut self, transport: TransportMode) -> Self {
        self.transport = transport;
        self
    }

    /// Add a STUN server
    #[must_use]
    pub fn with_stun_server(mut self, server: impl Into<String>) -> Self {
        self.stun_servers.push(server.into());
        self
    }

    /// Set maximum number of clients
    #[must_use] 
    pub fn with_max_clients(mut self, max: usize) -> Self {
        self.max_clients = max;
        self
    }

    /// Get the full signaling server address
    #[must_use] 
    pub fn signaling_address(&self) -> String {
        format!("{}:{}", self.bind_address, self.signaling_port)
    }
}

/// Connection statistics for monitoring
#[derive(Debug, Clone, Default)]
pub struct ConnectionStats {
    /// Round-trip time in milliseconds
    pub rtt_ms: u32,
    /// Current bitrate in bits per second
    pub bitrate_bps: u64,
    /// Packets sent
    pub packets_sent: u64,
    /// Packets lost
    pub packets_lost: u64,
    /// Frames sent
    pub frames_sent: u64,
    /// Connection duration in seconds
    pub duration_secs: u64,
    /// ICE connection state
    pub ice_state: String,
    /// Peer connection state
    pub connection_state: String,
}

/// Client connection information
#[derive(Debug)]
struct ClientConnection {
    /// Client ID
    id: String,
    /// WebRTC peer connection
    peer_connection: Arc<RTCPeerConnection>,
    /// Video track for sending frames
    video_track: Arc<TrackLocalStaticRTP>,
    /// Connection timestamp
    connected_at: std::time::Instant,
}

/// Signaling message types
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SignalingMessage {
    /// SDP offer from client
    Offer(String),
    /// SDP answer from server
    Answer(String),
    /// ICE candidate
    IceCandidate(IceCandidateData),
    /// Client connected
    Connected {
        /// The connected client's ID
        client_id: String,
    },
    /// Client disconnected
    Disconnected {
        /// The disconnected client's ID
        client_id: String,
    },
    /// Error message
    Error {
        /// The error message
        message: String,
    },
    /// Server ready
    Ready {
        /// The server's unique ID
        server_id: String,
    },
}

/// ICE candidate data for signaling
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IceCandidateData {
    /// Candidate string
    pub candidate: String,
    /// SDP mid
    pub sdp_mid: Option<String>,
    /// SDP mline index
    pub sdp_mline_index: Option<u16>,
}

/// WebRTC streaming server
pub struct StreamingServer {
    /// Server configuration
    config: StreamConfig,
    /// WebRTC API (wrapped in Arc for sharing)
    api: Arc<webrtc::api::API>,
    /// Active client connections
    clients: Arc<RwLock<HashMap<String, ClientConnection>>>,
    /// Signaling server handle
    signaling_handle: Option<tokio::task::JoinHandle<()>>,
    /// Channel for sending frames to clients
    frame_tx: mpsc::Sender<EncodedFrame>,
    /// Frame receiver for the broadcast task
    frame_rx: Arc<Mutex<mpsc::Receiver<EncodedFrame>>>,
    /// Server running state
    running: Arc<RwLock<bool>>,
    /// Server ID
    server_id: String,
}

impl StreamingServer {
    /// Create a new streaming server
    ///
    /// # Arguments
    ///
    /// * `config` - Server configuration
    ///
    /// # Returns
    ///
    /// A new `StreamingServer` instance
    pub fn new(config: StreamConfig) -> Result<Self> {
        info!(
            "Creating streaming server on {} ({})",
            config.signaling_address(),
            config.transport.display_name()
        );

        // Create WebRTC API
        let api = Arc::new(Self::create_webrtc_api()?);

        // Create frame channel
        let (frame_tx, frame_rx) = mpsc::channel(32);

        let server_id = uuid::Uuid::new_v4().to_string();

        Ok(Self {
            config,
            api,
            clients: Arc::new(RwLock::new(HashMap::new())),
            signaling_handle: None,
            frame_tx,
            frame_rx: Arc::new(Mutex::new(frame_rx)),
            running: Arc::new(RwLock::new(false)),
            server_id,
        })
    }

    /// Create the WebRTC API with H.264 codec support
    fn create_webrtc_api() -> Result<webrtc::api::API> {
        let mut media_engine = MediaEngine::default();

        // Register H.264 codec
        media_engine.register_default_codecs().map_err(|e| {
            DisplayStreamError::Streaming(format!("Failed to register codecs: {e}"))
        })?;

        // Create interceptor registry for RTCP feedback
        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine).map_err(|e| {
            DisplayStreamError::Streaming(format!("Failed to register interceptors: {e}"))
        })?;

        // Build API
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        Ok(api)
    }

    /// Start the streaming server
    pub async fn start(&mut self) -> Result<()> {
        let mut running = self.running.write().await;
        if *running {
            return Ok(());
        }

        info!("Starting streaming server");

        // Start signaling server
        let signaling_handle = self.start_signaling_server().await?;
        self.signaling_handle = Some(signaling_handle);

        // Start frame broadcast task
        self.start_frame_broadcaster();

        *running = true;
        info!(
            "Streaming server started on {}",
            self.config.signaling_address()
        );

        Ok(())
    }

    /// Stop the streaming server
    pub async fn stop(&mut self) -> Result<()> {
        let mut running = self.running.write().await;
        if !*running {
            return Ok(());
        }

        info!("Stopping streaming server");

        // Close all client connections
        let mut clients = self.clients.write().await;
        for (id, client) in clients.drain() {
            debug!("Closing connection for client {}", id);
            let _ = client.peer_connection.close().await;
        }

        // Stop signaling server
        if let Some(handle) = self.signaling_handle.take() {
            handle.abort();
        }

        *running = false;
        info!("Streaming server stopped");

        Ok(())
    }

    /// Start the WebSocket signaling server
    async fn start_signaling_server(&self) -> Result<tokio::task::JoinHandle<()>> {
        let addr = self.config.signaling_address();
        let listener = TcpListener::bind(&addr).await.map_err(|e| {
            DisplayStreamError::Streaming(format!("Failed to bind signaling server: {e}"))
        })?;

        let clients = self.clients.clone();
        let api = self.api.clone();
        let config = self.config.clone();
        let running = self.running.clone();
        let server_id = self.server_id.clone();

        let handle = tokio::spawn(async move {
            info!("Signaling server listening on {}", addr);

            while *running.read().await {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        info!("New signaling connection from {}", peer_addr);
                        let clients = clients.clone();
                        let api = api.clone();
                        let config = config.clone();
                        let server_id = server_id.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_signaling_connection(
                                stream, peer_addr, clients, api, config, server_id,
                            )
                            .await
                            {
                                error!("Signaling connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept signaling connection: {}", e);
                    }
                }
            }
        });

        Ok(handle)
    }

    /// Handle a signaling connection
    #[allow(clippy::too_many_lines)]
    async fn handle_signaling_connection(
        stream: TcpStream,
        peer_addr: SocketAddr,
        clients: Arc<RwLock<HashMap<String, ClientConnection>>>,
        api: Arc<webrtc::api::API>,
        config: StreamConfig,
        server_id: String,
    ) -> Result<()> {
        let ws_stream = accept_async(stream).await.map_err(|e| {
            DisplayStreamError::Streaming(format!("WebSocket handshake failed: {e}"))
        })?;

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();
        let client_id = uuid::Uuid::new_v4().to_string();

        info!("Client {} connected from {}", client_id, peer_addr);

        // Send ready message
        let ready_msg = SignalingMessage::Ready {
            server_id: server_id.clone(),
        };
        Self::send_signaling_message(&mut ws_sender, &ready_msg).await?;

        // Create peer connection for this client
        let rtc_config = RTCConfiguration {
            ice_servers: config
                .stun_servers
                .iter()
                .map(|s| RTCIceServer {
                    urls: vec![s.clone()],
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };

        let peer_connection = Arc::new(api.new_peer_connection(rtc_config).await.map_err(|e| {
            DisplayStreamError::Streaming(format!("Failed to create peer connection: {e}"))
        })?);

        // Create video track
        let video_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: "video/H264".to_string(),
                clock_rate: 90000,
                ..Default::default()
            },
            "video".to_string(),
            "cosmic-display-stream".to_string(),
        ));

        // Add video track to peer connection
        let rtp_sender = peer_connection
            .add_track(Arc::clone(&video_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .map_err(|e| DisplayStreamError::Streaming(format!("Failed to add track: {e}")))?;

        // Handle RTCP packets (for stats)
        tokio::spawn(async move {
            let mut rtcp_buf = vec![0u8; 1500];
            while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {
                // Process RTCP feedback if needed
            }
        });

        // Set up ICE candidate handler
        let ws_sender_ice = Arc::new(Mutex::new(ws_sender));
        let ws_sender_clone = ws_sender_ice.clone();

        peer_connection.on_ice_candidate(Box::new(move |candidate| {
            let ws_sender = ws_sender_clone.clone();
            Box::pin(async move {
                if let Some(c) = candidate {
                    let candidate_data = IceCandidateData {
                        candidate: c.to_json().map(|j| j.candidate).unwrap_or_default(),
                        sdp_mid: c.to_json().ok().and_then(|j| j.sdp_mid),
                        sdp_mline_index: c.to_json().ok().and_then(|j| j.sdp_mline_index),
                    };
                    let msg = SignalingMessage::IceCandidate(candidate_data);
                    let mut sender = ws_sender.lock().await;
                    let _ = Self::send_signaling_message(&mut *sender, &msg).await;
                }
            })
        }));

        // Set up connection state handler
        let client_id_state = client_id.clone();
        peer_connection.on_peer_connection_state_change(Box::new(move |state| {
            info!("Client {} connection state: {:?}", client_id_state, state);
            Box::pin(async {})
        }));

        // Store client connection
        {
            let mut clients_guard = clients.write().await;
            clients_guard.insert(
                client_id.clone(),
                ClientConnection {
                    id: client_id.clone(),
                    peer_connection: peer_connection.clone(),
                    video_track,
                    connected_at: std::time::Instant::now(),
                },
            );
        }

        // Process signaling messages
        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    match serde_json::from_str::<SignalingMessage>(&text) {
                        Ok(SignalingMessage::Offer(sdp)) => {
                            debug!("Received offer from client {}", client_id);

                            // Set remote description
                            let offer = RTCSessionDescription::offer(sdp).map_err(|e| {
                                DisplayStreamError::Streaming(format!("Invalid offer: {e}"))
                            })?;

                            peer_connection
                                .set_remote_description(offer)
                                .await
                                .map_err(|e| {
                                    DisplayStreamError::Streaming(format!(
                                        "Failed to set remote description: {e}"
                                    ))
                                })?;

                            // Create answer
                            let answer =
                                peer_connection.create_answer(None).await.map_err(|e| {
                                    DisplayStreamError::Streaming(format!(
                                        "Failed to create answer: {e}"
                                    ))
                                })?;

                            // Set local description
                            peer_connection
                                .set_local_description(answer.clone())
                                .await
                                .map_err(|e| {
                                    DisplayStreamError::Streaming(format!(
                                        "Failed to set local description: {e}"
                                    ))
                                })?;

                            // Send answer
                            let answer_msg = SignalingMessage::Answer(answer.sdp);
                            let mut sender = ws_sender_ice.lock().await;
                            Self::send_signaling_message(&mut *sender, &answer_msg).await?;
                        }
                        Ok(SignalingMessage::IceCandidate(candidate_data)) => {
                            debug!("Received ICE candidate from client {}", client_id);

                            let candidate = RTCIceCandidateInit {
                                candidate: candidate_data.candidate,
                                sdp_mid: candidate_data.sdp_mid,
                                sdp_mline_index: candidate_data.sdp_mline_index,
                                ..Default::default()
                            };

                            peer_connection
                                .add_ice_candidate(candidate)
                                .await
                                .map_err(|e| {
                                    DisplayStreamError::Streaming(format!(
                                        "Failed to add ICE candidate: {e}"
                                    ))
                                })?;
                        }
                        Ok(other) => {
                            debug!("Received other message: {:?}", other);
                        }
                        Err(e) => {
                            warn!("Failed to parse signaling message: {}", e);
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    info!("Client {} disconnected", client_id);
                    break;
                }
                Err(e) => {
                    error!("WebSocket error for client {}: {}", client_id, e);
                    break;
                }
                _ => {}
            }
        }

        // Clean up client
        {
            let mut clients_guard = clients.write().await;
            if let Some(client) = clients_guard.remove(&client_id) {
                let _ = client.peer_connection.close().await;
            }
        }

        Ok(())
    }

    /// Send a signaling message over WebSocket
    async fn send_signaling_message<S>(sender: &mut S, msg: &SignalingMessage) -> Result<()>
    where
        S: futures_util::SinkExt<Message> + Unpin,
        S::Error: std::fmt::Display,
    {
        let json = serde_json::to_string(msg).map_err(|e| {
            DisplayStreamError::Streaming(format!("Failed to serialize message: {e}"))
        })?;

        sender
            .send(Message::Text(json.into()))
            .await
            .map_err(|e| DisplayStreamError::Streaming(format!("Failed to send message: {e}")))?;

        Ok(())
    }

    /// Start the frame broadcaster task
    fn start_frame_broadcaster(&self) {
        let clients = self.clients.clone();
        let frame_rx = self.frame_rx.clone();
        let running = self.running.clone();

        tokio::spawn(async move {
            let mut seq_num: u16 = 0;
            let mut timestamp: u32 = 0;

            while *running.read().await {
                let mut rx = frame_rx.lock().await;
                if let Some(frame) = rx.recv().await {
                    let clients_guard = clients.read().await;

                    for client in clients_guard.values() {
                        // Create RTP packet from encoded frame
                        if let Err(e) = Self::send_rtp_frame(
                            &client.video_track,
                            &frame,
                            &mut seq_num,
                            &mut timestamp,
                        )
                        .await
                        {
                            warn!("Failed to send frame to client {}: {}", client.id, e);
                        }
                    }
                }
            }
        });
    }

    /// Send an encoded frame as RTP packets
    async fn send_rtp_frame(
        track: &TrackLocalStaticRTP,
        frame: &EncodedFrame,
        seq_num: &mut u16,
        timestamp: &mut u32,
    ) -> Result<()> {
        // H.264 NAL unit packetization
        // For simplicity, we send the entire frame as a single packet
        // In production, this should be fragmented into FU-A packets for large NALs

        let rtp_packet = webrtc::rtp::packet::Packet {
            header: webrtc::rtp::header::Header {
                version: 2,
                padding: false,
                extension: false,
                marker: true,     // End of frame
                payload_type: 96, // H.264
                sequence_number: *seq_num,
                timestamp: *timestamp,
                ssrc: 0x1234_5678,
                ..Default::default()
            },
            payload: frame.data.clone().into(),
        };

        // Write RTP packet
        track.write_rtp(&rtp_packet).await.map_err(|e| {
            DisplayStreamError::Streaming(format!("Failed to write RTP packet: {e}"))
        })?;

        // Increment sequence number and timestamp
        *seq_num = seq_num.wrapping_add(1);
        // Assuming 90kHz clock rate and 60fps
        *timestamp = timestamp.wrapping_add(1500);

        Ok(())
    }

    /// Send an encoded frame to all connected clients
    pub async fn send_frame(&self, frame: EncodedFrame) -> Result<()> {
        self.frame_tx
            .send(frame)
            .await
            .map_err(|e| DisplayStreamError::Streaming(format!("Failed to queue frame: {e}")))?;
        Ok(())
    }

    /// Get connection statistics for all clients
    pub async fn get_stats(&self) -> Option<ConnectionStats> {
        let clients = self.clients.read().await;

        // Return stats for the first connected client
        if let Some(client) = clients.values().next() {
            let duration = client.connected_at.elapsed();

            // Get peer connection stats
            let pc_state = client.peer_connection.connection_state();
            let ice_state = client.peer_connection.ice_connection_state();

            Some(ConnectionStats {
                rtt_ms: 0, // Would need to parse RTCP for actual RTT
                bitrate_bps: 0,
                packets_sent: 0,
                packets_lost: 0,
                frames_sent: 0,
                duration_secs: duration.as_secs(),
                ice_state: format!("{ice_state:?}"),
                connection_state: format!("{pc_state:?}"),
            })
        } else {
            None
        }
    }

    /// Get the number of connected clients
    pub async fn client_count(&self) -> usize {
        self.clients.read().await.len()
    }

    /// Check if the server is running
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// Get the server configuration
    #[must_use] 
    pub fn config(&self) -> &StreamConfig {
        &self.config
    }

    /// Get the server ID
    #[must_use] 
    pub fn server_id(&self) -> &str {
        &self.server_id
    }
}

impl Drop for StreamingServer {
    fn drop(&mut self) {
        // Signal shutdown
        if let Ok(mut running) = self.running.try_write() {
            *running = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_config_default() {
        let config = StreamConfig::default();
        assert_eq!(config.signaling_port, 8080);
        assert_eq!(config.bind_address, "0.0.0.0");
        assert_eq!(config.transport, TransportMode::WiFi);
        assert_eq!(config.max_clients, 1);
        assert!(config.enable_encryption);
    }

    #[test]
    fn test_stream_config_builder() {
        let config = StreamConfig::new()
            .with_signaling_port(9000)
            .with_bind_address("127.0.0.1")
            .with_transport(TransportMode::Usb)
            .with_max_clients(2);

        assert_eq!(config.signaling_port, 9000);
        assert_eq!(config.bind_address, "127.0.0.1");
        assert_eq!(config.transport, TransportMode::Usb);
        assert_eq!(config.max_clients, 2);
    }

    #[test]
    fn test_signaling_address() {
        let config = StreamConfig::new()
            .with_bind_address("192.168.1.100")
            .with_signaling_port(8554);

        assert_eq!(config.signaling_address(), "192.168.1.100:8554");
    }

    #[test]
    fn test_transport_mode_display() {
        assert_eq!(TransportMode::WiFi.display_name(), "WiFi");
        assert_eq!(TransportMode::Usb.display_name(), "USB (ADB)");
    }

    #[test]
    fn test_connection_stats_default() {
        let stats = ConnectionStats::default();
        assert_eq!(stats.rtt_ms, 0);
        assert_eq!(stats.bitrate_bps, 0);
        assert_eq!(stats.packets_sent, 0);
    }

    #[test]
    fn test_signaling_message_serialization() {
        let msg = SignalingMessage::Ready {
            server_id: "test-123".to_string(),
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("Ready"));
        assert!(json.contains("test-123"));

        let parsed: SignalingMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            SignalingMessage::Ready { server_id } => {
                assert_eq!(server_id, "test-123");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_ice_candidate_data() {
        let data = IceCandidateData {
            candidate: "candidate:1 1 UDP 2122262783 192.168.1.100 12345 typ host".to_string(),
            sdp_mid: Some("0".to_string()),
            sdp_mline_index: Some(0),
        };

        let json = serde_json::to_string(&data).unwrap();
        let parsed: IceCandidateData = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.candidate, data.candidate);
        assert_eq!(parsed.sdp_mid, data.sdp_mid);
        assert_eq!(parsed.sdp_mline_index, data.sdp_mline_index);
    }
}
