//! Bluetooth Discovery Module
//!
//! This module provides Bluetooth Low Energy (BLE) device discovery for CConnect.
//! It scans for devices advertising the CConnect service UUID and emits discovery events.

use super::events::DiscoveryEvent;
use crate::transport::CCONNECT_SERVICE_UUID;
use crate::{DeviceInfo, ProtocolError, Result};
use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, RwLock};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

/// Default scan interval (10 seconds)
pub const DEFAULT_BT_SCAN_INTERVAL: Duration = Duration::from_secs(10);

/// Default device timeout (60 seconds - longer than TCP since BLE is less frequent)
pub const DEFAULT_BT_DEVICE_TIMEOUT: Duration = Duration::from_secs(60);

/// Configuration for Bluetooth discovery
#[derive(Debug, Clone)]
pub struct BluetoothDiscoveryConfig {
    /// How often to scan for devices
    pub scan_interval: Duration,

    /// How long before a device is considered timed out
    pub device_timeout: Duration,

    /// Whether to enable device timeout checking
    pub enable_timeout_check: bool,

    /// Optional filter for device addresses (empty = all devices)
    pub device_filter: Vec<String>,
}

impl Default for BluetoothDiscoveryConfig {
    fn default() -> Self {
        Self {
            scan_interval: DEFAULT_BT_SCAN_INTERVAL,
            device_timeout: DEFAULT_BT_DEVICE_TIMEOUT,
            enable_timeout_check: true,
            device_filter: Vec::new(),
        }
    }
}

/// Bluetooth discovery service
///
/// Scans for BLE devices advertising the CConnect service UUID
pub struct BluetoothDiscoveryService {
    /// Bluetooth adapter
    adapter: Option<Adapter>,

    /// Event channel sender
    event_tx: mpsc::UnboundedSender<DiscoveryEvent>,

    /// Event channel receiver
    event_rx: Arc<RwLock<mpsc::UnboundedReceiver<DiscoveryEvent>>>,

    /// Service configuration
    config: BluetoothDiscoveryConfig,

    /// Shutdown signal sender
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,

    /// Last seen timestamps for devices (device_id -> timestamp)
    last_seen: Arc<RwLock<HashMap<String, u64>>>,

    /// Device info cache (bt_address -> DeviceInfo)
    device_cache: Arc<RwLock<HashMap<String, DeviceInfo>>>,
}

impl BluetoothDiscoveryService {
    /// Create a new Bluetooth discovery service
    ///
    /// # Arguments
    ///
    /// * `config` - Service configuration
    pub async fn new(config: BluetoothDiscoveryConfig) -> Result<Self> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        // Try to get Bluetooth adapter
        let adapter = match Self::get_adapter().await {
            Ok(adapter) => Some(adapter),
            Err(e) => {
                warn!("Failed to get Bluetooth adapter: {}", e);
                None
            }
        };

        Ok(Self {
            adapter,
            event_tx,
            event_rx: Arc::new(RwLock::new(event_rx)),
            config,
            shutdown_tx: None,
            last_seen: Arc::new(RwLock::new(HashMap::new())),
            device_cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Create a Bluetooth discovery service with default configuration
    pub async fn with_defaults() -> Result<Self> {
        Self::new(BluetoothDiscoveryConfig::default()).await
    }

    /// Get Bluetooth adapter
    async fn get_adapter() -> Result<Adapter> {
        let manager = Manager::new()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        let adapters = manager
            .adapters()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        adapters
            .into_iter()
            .next()
            .ok_or_else(|| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "No Bluetooth adapter found",
                ))
            })
    }

    /// Get a receiver for discovery events
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

    /// Start the Bluetooth discovery service
    ///
    /// Spawns background tasks for scanning and monitoring devices.
    pub async fn start(&mut self) -> Result<()> {
        if self.adapter.is_none() {
            return Err(ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No Bluetooth adapter available",
            )));
        }

        info!("Starting Bluetooth discovery service");

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        // Spawn scanner task
        self.spawn_scanner(shutdown_rx);

        // Spawn timeout checker if enabled
        if self.config.enable_timeout_check {
            self.spawn_timeout_checker();
        }

        Ok(())
    }

    /// Spawn scanner task
    fn spawn_scanner(&self, mut shutdown_rx: tokio::sync::oneshot::Receiver<()>) {
        let adapter = self.adapter.clone().unwrap();
        let event_tx = self.event_tx.clone();
        let scan_interval = self.config.scan_interval;
        let device_filter = self.config.device_filter.clone();
        let last_seen = self.last_seen.clone();
        let device_cache = self.device_cache.clone();

        tokio::spawn(async move {
            let mut interval = interval(scan_interval);

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = Self::scan_devices(
                            &adapter,
                            &event_tx,
                            &device_filter,
                            &last_seen,
                            &device_cache,
                        ).await {
                            error!("Failed to scan for Bluetooth devices: {}", e);
                        }
                    }
                    _ = &mut shutdown_rx => {
                        info!("Bluetooth scanner shutting down");
                        break;
                    }
                }
            }
        });
    }

    /// Scan for Bluetooth devices
    async fn scan_devices(
        adapter: &Adapter,
        event_tx: &mpsc::UnboundedSender<DiscoveryEvent>,
        device_filter: &[String],
        last_seen: &Arc<RwLock<HashMap<String, u64>>>,
        device_cache: &Arc<RwLock<HashMap<String, DeviceInfo>>>,
    ) -> Result<()> {
        debug!("Starting Bluetooth scan");

        // Start scanning for CConnect service
        adapter
            .start_scan(ScanFilter {
                services: vec![CCONNECT_SERVICE_UUID],
            })
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        // Wait for scan to complete (scan for 5 seconds)
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Stop scanning
        adapter
            .stop_scan()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        // Get discovered peripherals
        let peripherals = adapter
            .peripherals()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        debug!("Found {} potential peripherals", peripherals.len());

        for peripheral in peripherals {
            if let Err(e) = Self::process_peripheral(
                &peripheral,
                event_tx,
                device_filter,
                last_seen,
                device_cache,
            )
            .await
            {
                debug!("Error processing peripheral: {}", e);
            }
        }

        Ok(())
    }

    /// Process a discovered peripheral
    async fn process_peripheral(
        peripheral: &Peripheral,
        event_tx: &mpsc::UnboundedSender<DiscoveryEvent>,
        device_filter: &[String],
        last_seen: &Arc<RwLock<HashMap<String, u64>>>,
        device_cache: &Arc<RwLock<HashMap<String, DeviceInfo>>>,
    ) -> Result<()> {
        // Get peripheral properties
        let properties = peripheral
            .properties()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?
            .ok_or_else(|| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "No properties available",
                ))
            })?;

        // Get Bluetooth address
        let bt_address = peripheral.address().to_string();

        // Check if device matches filter
        if !device_filter.is_empty() && !device_filter.contains(&bt_address) {
            debug!("Skipping filtered device: {}", bt_address);
            return Ok(());
        }

        // Check if peripheral advertises CConnect service
        if !properties.service_data.contains_key(&CCONNECT_SERVICE_UUID) {
            debug!("Device {} doesn't advertise CConnect service", bt_address);
            return Ok(());
        }

        // Try to get device name from properties
        let device_name = properties
            .local_name
            .unwrap_or_else(|| format!("BT Device {}", &bt_address[..8]));

        // Try to extract device info from advertising data
        // For now, create a basic DeviceInfo - full identity exchange would happen on connection
        let device_info = DeviceInfo::new(&device_name, crate::DeviceType::Phone, 1816);

        let current_time = current_timestamp();
        let mut last_seen_map = last_seen.write().await;
        let mut device_cache_map = device_cache.write().await;

        // Check if this is a new device or update
        let is_new = !last_seen_map.contains_key(&device_info.device_id);
        last_seen_map.insert(device_info.device_id.clone(), current_time);
        device_cache_map.insert(bt_address.clone(), device_info.clone());
        drop(last_seen_map);
        drop(device_cache_map);

        // Emit appropriate event
        if is_new {
            info!(
                "Discovered new Bluetooth device: {} ({}) at {}",
                device_info.device_name,
                device_info.device_type.as_str(),
                bt_address
            );
            let _ = event_tx.send(DiscoveryEvent::bluetooth_discovered(
                device_info,
                bt_address,
            ));
        } else {
            debug!("Updated Bluetooth device: {} at {}", device_info.device_name, bt_address);
            let _ = event_tx.send(DiscoveryEvent::bluetooth_updated(device_info, bt_address));
        }

        Ok(())
    }

    /// Spawn timeout checker task
    fn spawn_timeout_checker(&self) {
        let last_seen = self.last_seen.clone();
        let event_tx = self.event_tx.clone();
        let timeout_duration = self.config.device_timeout;

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(10));

            loop {
                interval.tick().await;

                let current_time = current_timestamp();
                let mut last_seen_map = last_seen.write().await;
                let mut timed_out = Vec::new();

                for (device_id, &last_seen_time) in last_seen_map.iter() {
                    if current_time - last_seen_time > timeout_duration.as_secs() {
                        timed_out.push(device_id.clone());
                    }
                }

                for device_id in timed_out {
                    info!("Bluetooth device timed out: {}", device_id);
                    last_seen_map.remove(&device_id);
                    let _ = event_tx.send(DiscoveryEvent::DeviceTimeout { device_id });
                }
            }
        });
    }

    /// Stop the Bluetooth discovery service
    pub async fn stop(&mut self) {
        info!("Stopping Bluetooth discovery service");

        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
    }

    /// Check if Bluetooth is available
    pub fn is_available(&self) -> bool {
        self.adapter.is_some()
    }
}

/// Get current UNIX timestamp in seconds
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bluetooth_discovery_config_defaults() {
        let config = BluetoothDiscoveryConfig::default();
        assert_eq!(config.scan_interval, DEFAULT_BT_SCAN_INTERVAL);
        assert_eq!(config.device_timeout, DEFAULT_BT_DEVICE_TIMEOUT);
        assert!(config.enable_timeout_check);
        assert!(config.device_filter.is_empty());
    }

    #[tokio::test]
    async fn test_bluetooth_discovery_creation() {
        let service = BluetoothDiscoveryService::with_defaults().await;
        assert!(service.is_ok());
    }
}
