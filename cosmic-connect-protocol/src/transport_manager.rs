//! Transport Manager
//!
//! Provides a unified facade for managing connections across multiple transport types
//! (TCP/TLS and Bluetooth). This allows the daemon to support both transport methods
//! while maintaining a consistent interface.
//!
//! ## Architecture
//!
//! ```text
//! TransportManager (facade)
//!   ├── ConnectionManager (TLS/TCP)
//!   │     └── TlsConnection
//!   └── BluetoothConnectionManager (Bluetooth)
//!         └── BluetoothConnection
//! ```
//!
//! ## Transport Selection
//!
//! The TransportManager automatically selects the appropriate transport based on:
//! - User configuration (preference)
//! - Transport availability
//! - Connection address type
//! - Auto-fallback settings

use crate::{
    bluetooth_connection_manager::BluetoothConnectionManager,
    connection::{ConnectionEvent, ConnectionManager},
    transport::{TransportAddress, TransportPreference, TransportType},
    Packet, Result,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};

/// Transport manager configuration
///
/// This configuration controls which transports are enabled and how they
/// should be used for device connections.
#[derive(Debug, Clone)]
pub struct TransportManagerConfig {
    /// Enable TCP/IP transport
    pub enable_tcp: bool,

    /// Enable Bluetooth transport
    pub enable_bluetooth: bool,

    /// Transport preference for new connections
    pub preference: TransportPreference,

    /// TCP operation timeout
    pub tcp_timeout: Duration,

    /// Bluetooth operation timeout
    pub bluetooth_timeout: Duration,

    /// Automatically fallback to alternative transport if primary fails
    pub auto_fallback: bool,

    /// Bluetooth device filtering (empty = no filter, accepts all)
    pub bluetooth_device_filter: Vec<String>,
}

impl Default for TransportManagerConfig {
    fn default() -> Self {
        Self {
            enable_tcp: true,
            enable_bluetooth: false,
            preference: TransportPreference::PreferTcp,
            tcp_timeout: Duration::from_secs(10),
            bluetooth_timeout: Duration::from_secs(15),
            auto_fallback: true,
            bluetooth_device_filter: Vec::new(),
        }
    }
}

/// Transport manager events
///
/// These events are emitted by the TransportManager to notify about
/// connection state changes across all transport types.
#[derive(Debug, Clone)]
pub enum TransportManagerEvent {
    /// A transport manager started successfully
    Started { transport_type: TransportType },

    /// A device connected via a transport
    Connected {
        device_id: String,
        transport_type: TransportType,
    },

    /// A device disconnected from a transport
    Disconnected {
        device_id: String,
        transport_type: TransportType,
        reason: Option<String>,
    },

    /// A packet was received from a device
    PacketReceived {
        device_id: String,
        packet: Packet,
        transport_type: TransportType,
    },

    /// An error occurred
    Error {
        transport_type: TransportType,
        message: String,
    },
}

/// Transport manager for coordinating multiple transport types
///
/// The TransportManager provides a unified interface for managing connections
/// across TCP/TLS and Bluetooth transports. It handles:
/// - Transport selection based on configuration
/// - Connection management for each transport
/// - Automatic fallback between transports
/// - Event routing from all transports
pub struct TransportManager {
    /// TCP/TLS connection manager (always available)
    tcp_manager: Arc<RwLock<ConnectionManager>>,

    /// Bluetooth connection manager (optional, based on config)
    bluetooth_manager: Option<Arc<RwLock<BluetoothConnectionManager>>>,

    /// Transport configuration
    config: TransportManagerConfig,

    /// Event channel sender
    event_tx: mpsc::UnboundedSender<TransportManagerEvent>,

    /// Event channel receiver
    event_rx: Arc<RwLock<mpsc::UnboundedReceiver<TransportManagerEvent>>>,
}

impl TransportManager {
    /// Create a new transport manager
    ///
    /// # Arguments
    ///
    /// * `tcp_manager` - Existing TCP/TLS connection manager
    /// * `config` - Transport configuration
    pub fn new(
        tcp_manager: Arc<RwLock<ConnectionManager>>,
        config: TransportManagerConfig,
    ) -> Result<Self> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        // Initialize Bluetooth manager if enabled
        let bluetooth_manager = if config.enable_bluetooth {
            info!("Bluetooth transport enabled in configuration");
            match BluetoothConnectionManager::new(config.bluetooth_timeout) {
                Ok(bt_mgr) => {
                    info!("Bluetooth connection manager created");
                    Some(Arc::new(RwLock::new(bt_mgr)))
                }
                Err(e) => {
                    warn!("Failed to create Bluetooth connection manager: {}", e);
                    warn!("Bluetooth transport will be unavailable");
                    None
                }
            }
        } else {
            debug!("Bluetooth transport disabled in configuration");
            None
        };

        Ok(Self {
            tcp_manager,
            bluetooth_manager,
            config,
            event_tx,
            event_rx: Arc::new(RwLock::new(event_rx)),
        })
    }

    /// Start the transport manager
    ///
    /// This starts all enabled transport managers and begins listening for connections.
    pub async fn start(&self) -> Result<()> {
        info!("Starting transport manager...");

        // Start TCP manager (always enabled)
        if self.config.enable_tcp {
            let tcp_mgr = self.tcp_manager.read().await;
            let port = tcp_mgr.start().await?;
            drop(tcp_mgr);

            info!("TCP transport started on port {}", port);
            let _ = self.event_tx.send(TransportManagerEvent::Started {
                transport_type: TransportType::Tcp,
            });

            // Forward TCP events
            self.forward_tcp_events().await;
        }

        // Start Bluetooth manager (if enabled and available)
        if self.config.enable_bluetooth {
            if let Some(bt_mgr) = &self.bluetooth_manager {
                let bt = bt_mgr.read().await;
                bt.start().await?;
                drop(bt);

                info!("Bluetooth transport started");
                let _ = self.event_tx.send(TransportManagerEvent::Started {
                    transport_type: TransportType::Bluetooth,
                });

                // Forward Bluetooth events
                self.forward_bluetooth_events().await;
            }
        }

        info!("Transport manager started successfully");
        Ok(())
    }

    /// Forward TCP connection events to transport manager events
    async fn forward_tcp_events(&self) {
        let tcp_mgr = self.tcp_manager.clone();
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let mgr = tcp_mgr.read().await;
            let mut conn_events = mgr.subscribe().await;
            drop(mgr);

            while let Some(event) = conn_events.recv().await {
                let transport_event = match event {
                    ConnectionEvent::Connected {
                        device_id,
                        remote_addr: _,
                    } => TransportManagerEvent::Connected {
                        device_id,
                        transport_type: TransportType::Tcp,
                    },
                    ConnectionEvent::Disconnected { device_id, reason, reconnect: _ } => {
                        // Note: reconnect field is handled at the daemon level for plugin cleanup
                        // Transport manager just forwards the disconnection event
                        TransportManagerEvent::Disconnected {
                            device_id,
                            transport_type: TransportType::Tcp,
                            reason,
                        }
                    }
                    ConnectionEvent::PacketReceived {
                        device_id,
                        packet,
                        remote_addr: _,
                    } => TransportManagerEvent::PacketReceived {
                        device_id,
                        packet,
                        transport_type: TransportType::Tcp,
                    },
                    ConnectionEvent::ConnectionError { device_id, message } => {
                        TransportManagerEvent::Error {
                            transport_type: TransportType::Tcp,
                            message: format!("Device {:?}: {}", device_id, message),
                        }
                    }
                    ConnectionEvent::ManagerStarted { .. } => continue,
                    ConnectionEvent::ManagerStopped => continue,
                };

                if event_tx.send(transport_event).is_err() {
                    break;
                }
            }
        });
    }

    /// Forward Bluetooth connection events to transport manager events
    async fn forward_bluetooth_events(&self) {
        let bt_mgr = self.bluetooth_manager.as_ref().unwrap().clone();
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let mgr = bt_mgr.read().await;
            let mut bt_events = mgr.subscribe().await;
            drop(mgr);

            while let Some(event) = bt_events.recv().await {
                if event_tx.send(event).is_err() {
                    break;
                }
            }
        });
    }

    /// Connect to a device using the configured transport preference
    ///
    /// This method selects the appropriate transport based on:
    /// - The address type (TCP vs Bluetooth)
    /// - The configured preference
    /// - Transport availability
    /// - Auto-fallback settings
    pub async fn connect(&self, device_id: &str, address: TransportAddress) -> Result<()> {
        debug!(
            "Connecting to device {} using preference {:?}",
            device_id, self.config.preference
        );

        // Determine which transport to try first based on address and preference
        let primary_transport = self.select_primary_transport(&address);
        let secondary_transport = self.select_secondary_transport(&address);

        // Try primary transport
        match self
            .connect_with_transport(device_id, &address, primary_transport)
            .await
        {
            Ok(()) => {
                info!("Connected to {} via {:?}", device_id, primary_transport);
                Ok(())
            }
            Err(e) => {
                warn!(
                    "Failed to connect to {} via {:?}: {}",
                    device_id, primary_transport, e
                );

                // Try fallback if enabled and available
                if self.config.auto_fallback {
                    if let Some(fallback) = secondary_transport {
                        info!(
                            "Attempting fallback to {:?} for device {}",
                            fallback, device_id
                        );

                        return self
                            .connect_with_transport(device_id, &address, fallback)
                            .await;
                    }
                }

                // No fallback available or disabled
                Err(e)
            }
        }
    }

    /// Select the primary transport to try based on preference and address
    fn select_primary_transport(&self, address: &TransportAddress) -> TransportType {
        match (&self.config.preference, address) {
            // Explicit "Only" preferences
            (TransportPreference::Only(t), _) => *t,

            // For TCP addresses, prefer TCP unless explicitly configured otherwise
            (_, TransportAddress::Tcp(_)) => match self.config.preference {
                TransportPreference::PreferBluetooth => TransportType::Bluetooth,
                TransportPreference::BluetoothFirst => TransportType::Bluetooth,
                _ => TransportType::Tcp,
            },

            // For Bluetooth addresses, prefer Bluetooth unless explicitly configured otherwise
            (_, TransportAddress::Bluetooth { .. }) => match self.config.preference {
                TransportPreference::PreferTcp => TransportType::Tcp,
                TransportPreference::TcpFirst => TransportType::Tcp,
                _ => TransportType::Bluetooth,
            },
        }
    }

    /// Select the secondary (fallback) transport if auto-fallback is enabled
    fn select_secondary_transport(&self, address: &TransportAddress) -> Option<TransportType> {
        if !self.config.auto_fallback {
            return None;
        }

        match (&self.config.preference, address) {
            // "Only" preferences have no fallback
            (TransportPreference::Only(_), _) => None,

            // TcpFirst/BluetoothFirst always have a fallback
            (TransportPreference::TcpFirst, _) => Some(TransportType::Bluetooth),
            (TransportPreference::BluetoothFirst, _) => Some(TransportType::Tcp),

            // Prefer* might have fallback based on address
            (TransportPreference::PreferTcp, TransportAddress::Tcp(_)) => {
                Some(TransportType::Bluetooth)
            }
            (TransportPreference::PreferBluetooth, TransportAddress::Bluetooth { .. }) => {
                Some(TransportType::Tcp)
            }

            _ => None,
        }
    }

    /// Connect using a specific transport type
    async fn connect_with_transport(
        &self,
        device_id: &str,
        address: &TransportAddress,
        transport_type: TransportType,
    ) -> Result<()> {
        match transport_type {
            TransportType::Tcp => {
                if !self.config.enable_tcp {
                    return Err(crate::ProtocolError::Transport(
                        "TCP transport is disabled".to_string(),
                    ));
                }

                let addr = match address {
                    TransportAddress::Tcp(addr) => *addr,
                    _ => {
                        return Err(crate::ProtocolError::Transport(
                            "Invalid address type for TCP transport".to_string(),
                        ))
                    }
                };

                let tcp_mgr = self.tcp_manager.read().await;
                tcp_mgr.connect(device_id, addr).await
            }

            TransportType::Bluetooth => {
                if !self.config.enable_bluetooth {
                    return Err(crate::ProtocolError::Transport(
                        "Bluetooth transport is disabled".to_string(),
                    ));
                }

                let bt_mgr = self.bluetooth_manager.as_ref().ok_or_else(|| {
                    crate::ProtocolError::Transport("Bluetooth manager not available".to_string())
                })?;

                let bt_address = match address {
                    TransportAddress::Bluetooth {
                        address,
                        service_uuid: _,
                    } => address.clone(),
                    _ => {
                        return Err(crate::ProtocolError::Transport(
                            "Invalid address type for Bluetooth transport".to_string(),
                        ))
                    }
                };

                // Use default RFCOMM channel (None = use default)
                let bt = bt_mgr.read().await;
                bt.connect(device_id, &bt_address, None).await
            }
        }
    }

    /// Send a packet to a device
    ///
    /// This automatically routes the packet to the appropriate transport
    /// based on the active connection for the device.
    pub async fn send_packet(&self, device_id: &str, packet: &Packet) -> Result<()> {
        // Try TCP first (most devices use TCP)
        if self.config.enable_tcp {
            let tcp_mgr = self.tcp_manager.read().await;
            if tcp_mgr.has_connection(device_id).await {
                return tcp_mgr.send_packet(device_id, packet).await;
            }
        }

        // Try Bluetooth if available
        if self.config.enable_bluetooth {
            if let Some(bt_mgr) = &self.bluetooth_manager {
                let bt = bt_mgr.read().await;
                if bt.has_connection(device_id).await {
                    return bt.send_packet(device_id, packet).await;
                }
            }
        }

        Err(crate::ProtocolError::DeviceNotFound(format!(
            "No active connection to device {}",
            device_id
        )))
    }

    /// Disconnect from a device
    ///
    /// This disconnects from the device on all active transports.
    pub async fn disconnect(&self, device_id: &str) -> Result<()> {
        let mut had_connection = false;

        // Disconnect TCP
        if self.config.enable_tcp {
            let tcp_mgr = self.tcp_manager.read().await;
            if tcp_mgr.has_connection(device_id).await {
                tcp_mgr.disconnect(device_id).await?;
                had_connection = true;
            }
        }

        // Disconnect Bluetooth
        if self.config.enable_bluetooth {
            if let Some(bt_mgr) = &self.bluetooth_manager {
                let bt = bt_mgr.read().await;
                if bt.has_connection(device_id).await {
                    bt.disconnect(device_id).await?;
                    had_connection = true;
                }
            }
        }

        if !had_connection {
            return Err(crate::ProtocolError::DeviceNotFound(format!(
                "No active connection to device {}",
                device_id
            )));
        }

        Ok(())
    }

    /// Check if there's an active connection to a device on any transport
    pub async fn has_connection(&self, device_id: &str) -> bool {
        // Check TCP
        if self.config.enable_tcp {
            let tcp_mgr = self.tcp_manager.read().await;
            if tcp_mgr.has_connection(device_id).await {
                return true;
            }
        }

        // Check Bluetooth
        if self.config.enable_bluetooth {
            if let Some(bt_mgr) = &self.bluetooth_manager {
                let bt = bt_mgr.read().await;
                if bt.has_connection(device_id).await {
                    return true;
                }
            }
        }

        false
    }

    /// Subscribe to transport manager events
    pub async fn subscribe(&self) -> mpsc::UnboundedReceiver<TransportManagerEvent> {
        let (tx, rx) = mpsc::unbounded_channel();

        // Forward events
        let event_rx = self.event_rx.clone();
        tokio::spawn(async move {
            let mut rx_lock = event_rx.write().await;
            while let Some(event) = rx_lock.recv().await {
                if tx.send(event).is_err() {
                    break;
                }
            }
        });

        rx
    }

    /// Stop the transport manager and all transports
    pub async fn stop(&self) {
        info!("Stopping transport manager...");

        // Stop TCP manager
        if self.config.enable_tcp {
            let tcp_mgr = self.tcp_manager.read().await;
            tcp_mgr.stop().await;
        }

        // Stop Bluetooth manager
        if self.config.enable_bluetooth {
            if let Some(bt_mgr) = &self.bluetooth_manager {
                let bt = bt_mgr.read().await;
                bt.stop().await;
            }
        }

        info!("Transport manager stopped");
    }
}
