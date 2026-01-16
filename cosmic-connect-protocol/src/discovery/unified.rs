//! Unified Discovery Service
//!
//! This module provides a unified discovery service that coordinates both
//! UDP (TCP/IP) and Bluetooth discovery, emitting unified DiscoveryEvents.

use super::bluetooth::{BluetoothDiscoveryConfig, BluetoothDiscoveryService};
use super::events::DiscoveryEvent;
use super::service::{DiscoveryConfig, DiscoveryService};
use crate::{DeviceInfo, Result};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};

/// Configuration for unified discovery
#[derive(Debug, Clone)]
pub struct UnifiedDiscoveryConfig {
    /// Enable TCP/IP (UDP) discovery
    pub enable_tcp: bool,

    /// Enable Bluetooth discovery
    pub enable_bluetooth: bool,

    /// TCP discovery configuration
    pub tcp_config: DiscoveryConfig,

    /// Bluetooth discovery configuration
    pub bluetooth_config: BluetoothDiscoveryConfig,
}

impl Default for UnifiedDiscoveryConfig {
    fn default() -> Self {
        Self {
            enable_tcp: true,
            enable_bluetooth: false, // Opt-in
            tcp_config: DiscoveryConfig::default(),
            bluetooth_config: BluetoothDiscoveryConfig::default(),
        }
    }
}

/// Unified discovery service coordinating multiple discovery methods
///
/// This service acts as a facade coordinating:
/// - UDP broadcast discovery (TCP/IP)
/// - Bluetooth Low Energy (BLE) discovery
///
/// It emits unified DiscoveryEvents regardless of the discovery method used.
pub struct UnifiedDiscoveryService {
    /// TCP discovery service (always present)
    tcp_service: Arc<RwLock<DiscoveryService>>,

    /// Bluetooth discovery service (optional)
    bluetooth_service: Option<Arc<RwLock<BluetoothDiscoveryService>>>,

    /// Unified event channel sender
    event_tx: mpsc::UnboundedSender<DiscoveryEvent>,

    /// Unified event channel receiver
    event_rx: Arc<RwLock<mpsc::UnboundedReceiver<DiscoveryEvent>>>,

    /// Configuration
    config: UnifiedDiscoveryConfig,
}

impl UnifiedDiscoveryService {
    /// Create a new unified discovery service
    ///
    /// # Arguments
    ///
    /// * `device_info` - Information about this device
    /// * `config` - Unified discovery configuration
    pub async fn new(device_info: DeviceInfo, config: UnifiedDiscoveryConfig) -> Result<Self> {
        info!(
            "Creating unified discovery service (TCP: {}, Bluetooth: {})",
            config.enable_tcp, config.enable_bluetooth
        );

        // Create TCP discovery service (always enabled)
        let tcp_service = DiscoveryService::new(device_info.clone(), config.tcp_config.clone())?;
        let tcp_service = Arc::new(RwLock::new(tcp_service));

        // Create Bluetooth discovery service if enabled
        let bluetooth_service = if config.enable_bluetooth {
            match BluetoothDiscoveryService::new(config.bluetooth_config.clone()).await {
                Ok(bt_service) => {
                    if bt_service.is_available() {
                        info!("Bluetooth discovery service created successfully");
                        Some(Arc::new(RwLock::new(bt_service)))
                    } else {
                        warn!("Bluetooth adapter not available, Bluetooth discovery disabled");
                        None
                    }
                }
                Err(e) => {
                    warn!("Failed to create Bluetooth discovery service: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Create unified event channel
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        Ok(Self {
            tcp_service,
            bluetooth_service,
            event_tx,
            event_rx: Arc::new(RwLock::new(event_rx)),
            config,
        })
    }

    /// Create a unified discovery service with default configuration
    pub async fn with_defaults(device_info: DeviceInfo) -> Result<Self> {
        Self::new(device_info, UnifiedDiscoveryConfig::default()).await
    }

    /// Get a receiver for unified discovery events
    pub async fn subscribe(&self) -> mpsc::UnboundedReceiver<DiscoveryEvent> {
        let (tx, rx) = mpsc::unbounded_channel();

        // Create a task to forward events
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

    /// Start the unified discovery service
    ///
    /// Starts all enabled discovery methods and begins forwarding events.
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting unified discovery service");

        // Start TCP discovery
        if self.config.enable_tcp {
            let mut tcp_service = self.tcp_service.write().await;
            tcp_service.start().await?;
            drop(tcp_service);

            // Forward TCP events
            self.spawn_tcp_event_forwarder();
        }

        // Start Bluetooth discovery if available
        if let Some(bluetooth_service) = &self.bluetooth_service {
            let mut bt_service = bluetooth_service.write().await;
            if let Err(e) = bt_service.start().await {
                warn!("Failed to start Bluetooth discovery: {}", e);
            } else {
                drop(bt_service);

                // Forward Bluetooth events
                self.spawn_bluetooth_event_forwarder();
            }
        }

        info!("Unified discovery service started successfully");
        Ok(())
    }

    /// Spawn task to forward TCP discovery events
    fn spawn_tcp_event_forwarder(&self) {
        let tcp_service = self.tcp_service.clone();
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let tcp_srv = tcp_service.read().await;
            let mut tcp_events = tcp_srv.subscribe().await;
            drop(tcp_srv);

            while let Some(event) = tcp_events.recv().await {
                let _ = event_tx.send(event);
            }
        });
    }

    /// Spawn task to forward Bluetooth discovery events
    fn spawn_bluetooth_event_forwarder(&self) {
        if let Some(bluetooth_service) = &self.bluetooth_service {
            let bt_service = bluetooth_service.clone();
            let event_tx = self.event_tx.clone();

            tokio::spawn(async move {
                let bt_srv = bt_service.read().await;
                let mut bt_events = bt_srv.subscribe().await;
                drop(bt_srv);

                while let Some(event) = bt_events.recv().await {
                    let _ = event_tx.send(event);
                }
            });
        }
    }

    /// Stop the unified discovery service
    pub async fn stop(&mut self) {
        info!("Stopping unified discovery service");

        // Stop TCP service
        if self.config.enable_tcp {
            let mut tcp_service = self.tcp_service.write().await;
            tcp_service.stop().await;
        }

        // Stop Bluetooth service if available
        if let Some(bluetooth_service) = &self.bluetooth_service {
            let mut bt_service = bluetooth_service.write().await;
            bt_service.stop().await;
        }

        info!("Unified discovery service stopped");
    }

    /// Get the local TCP port this service is bound to
    pub async fn tcp_port(&self) -> Result<u16> {
        let tcp_service = self.tcp_service.read().await;
        tcp_service.local_port()
    }

    /// Check if Bluetooth discovery is active
    pub fn has_bluetooth(&self) -> bool {
        self.bluetooth_service.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceType;

    #[tokio::test]
    async fn test_unified_discovery_creation() {
        let device_info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1816);
        let config = UnifiedDiscoveryConfig {
            enable_tcp: true,
            enable_bluetooth: false,
            ..Default::default()
        };

        let service = UnifiedDiscoveryService::new(device_info, config).await;
        assert!(service.is_ok());
    }

    #[tokio::test]
    async fn test_unified_discovery_with_defaults() {
        let device_info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1816);
        let service = UnifiedDiscoveryService::with_defaults(device_info).await;
        assert!(service.is_ok());
    }

    #[tokio::test]
    async fn test_unified_discovery_tcp_only() {
        let device_info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1816);
        let config = UnifiedDiscoveryConfig {
            enable_tcp: true,
            enable_bluetooth: false,
            ..Default::default()
        };

        let service = UnifiedDiscoveryService::new(device_info, config).await.unwrap();
        assert!(!service.has_bluetooth());
    }
}
