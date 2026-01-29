//! Bluetooth Discovery Module
//!
//! This module provides Bluetooth device discovery for CConnect using RFCOMM.
//! It scans for paired devices and checks if they have the CConnect service
//! registered via SDP (Service Discovery Protocol).
//!
//! ## Discovery Approach
//!
//! Unlike BLE which uses advertising, RFCOMM discovery works by:
//! 1. Enumerating paired Bluetooth devices from BlueZ
//! 2. Optionally checking SDP for CConnect service UUID
//! 3. Emitting discovery events for compatible devices
//!
//! Note: Android uses `fetchUuidsWithSdp()` for service discovery.

use super::events::DiscoveryEvent;
use crate::transport::CCONNECT_SERVICE_UUID;
use crate::{DeviceInfo, DeviceType, ProtocolError, Result};
use bluer::{Address, Adapter, Session};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, RwLock};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

/// Default scan interval (10 seconds)
pub const DEFAULT_BT_SCAN_INTERVAL: Duration = Duration::from_secs(10);

/// Default device timeout (60 seconds - longer than TCP since Bluetooth is less frequent)
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

    /// Whether to only include paired devices
    pub paired_only: bool,
}

impl Default for BluetoothDiscoveryConfig {
    fn default() -> Self {
        Self {
            scan_interval: DEFAULT_BT_SCAN_INTERVAL,
            device_timeout: DEFAULT_BT_DEVICE_TIMEOUT,
            enable_timeout_check: true,
            device_filter: Vec::new(),
            paired_only: true, // Default to paired devices only for RFCOMM
        }
    }
}

/// Bluetooth discovery service
///
/// Scans for paired Bluetooth devices that may run CConnect
pub struct BluetoothDiscoveryService {
    /// BlueZ session
    session: Option<Session>,

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

        // Try to get BlueZ session and adapter
        let (session, adapter) = match Self::get_session_and_adapter().await {
            Ok((session, adapter)) => (Some(session), Some(adapter)),
            Err(e) => {
                warn!("Failed to get Bluetooth session/adapter: {}", e);
                (None, None)
            }
        };

        Ok(Self {
            session,
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

    /// Get BlueZ session and adapter
    async fn get_session_and_adapter() -> Result<(Session, Adapter)> {
        let session = Session::new()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        let adapter_names = session
            .adapter_names()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        let adapter_name = adapter_names.into_iter().next().ok_or_else(|| {
            ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No Bluetooth adapter found",
            ))
        })?;

        let adapter = session
            .adapter(&adapter_name)
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        Ok((session, adapter))
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

        info!("Starting Bluetooth discovery service (RFCOMM mode)");

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
        let paired_only = self.config.paired_only;
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
                            paired_only,
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
        paired_only: bool,
        last_seen: &Arc<RwLock<HashMap<String, u64>>>,
        device_cache: &Arc<RwLock<HashMap<String, DeviceInfo>>>,
    ) -> Result<()> {
        debug!("Scanning for Bluetooth devices (paired_only={})", paired_only);

        // Get all device addresses known to the adapter
        let device_addresses = adapter
            .device_addresses()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        debug!("Found {} known devices", device_addresses.len());

        for addr in device_addresses {
            if let Err(e) = Self::process_device(
                adapter,
                addr,
                event_tx,
                device_filter,
                paired_only,
                last_seen,
                device_cache,
            )
            .await
            {
                debug!("Error processing device {}: {}", addr, e);
            }
        }

        Ok(())
    }

    /// Process a discovered device
    async fn process_device(
        adapter: &Adapter,
        addr: Address,
        event_tx: &mpsc::UnboundedSender<DiscoveryEvent>,
        device_filter: &[String],
        paired_only: bool,
        last_seen: &Arc<RwLock<HashMap<String, u64>>>,
        device_cache: &Arc<RwLock<HashMap<String, DeviceInfo>>>,
    ) -> Result<()> {
        let bt_address = addr.to_string();

        // Check if device matches filter
        if !device_filter.is_empty() && !device_filter.contains(&bt_address) {
            debug!("Skipping filtered device: {}", bt_address);
            return Ok(());
        }

        // Get device from adapter
        let device = adapter
            .device(addr)
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        // Check if paired (if required)
        if paired_only {
            let is_paired = device.is_paired().await.unwrap_or(false);
            if !is_paired {
                debug!("Skipping unpaired device: {}", bt_address);
                return Ok(());
            }
        }

        // Get device name
        let device_name = device
            .name()
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| format!("BT Device {}", &bt_address[..8]));

        // Get device class to determine type
        let device_type = match device.class().await.ok().flatten() {
            Some(class) => device_type_from_class(class),
            None => DeviceType::Phone, // Default assumption
        };

        // Check if device has CConnect service UUID
        // This requires SDP lookup which may not always be available
        let has_cconnect_service = match device.uuids().await {
            Ok(Some(uuids)) => {
                let service_uuid_str = CCONNECT_SERVICE_UUID.to_string();
                uuids.iter().any(|u| u.to_string() == service_uuid_str)
            }
            _ => {
                // UUIDs not available - include device anyway for user to try
                debug!("Could not get UUIDs for device {}", bt_address);
                true
            }
        };

        if !has_cconnect_service {
            debug!(
                "Device {} doesn't have CConnect service UUID",
                bt_address
            );
            // Still emit the device - user might want to pair/connect manually
        }

        // Create device info
        // Use Bluetooth address as device_id since we don't have identity yet
        let device_info = DeviceInfo::new(&device_name, device_type, 1816);

        let current_time = current_timestamp();
        let mut last_seen_map = last_seen.write().await;
        let mut device_cache_map = device_cache.write().await;

        // Check if this is a new device or update
        let is_new = !device_cache_map.contains_key(&bt_address);
        last_seen_map.insert(bt_address.clone(), current_time);
        device_cache_map.insert(bt_address.clone(), device_info.clone());
        drop(last_seen_map);
        drop(device_cache_map);

        // Emit appropriate event
        if is_new {
            info!(
                "Discovered Bluetooth device: {} ({}) at {}",
                device_info.device_name,
                device_info.device_type.as_str(),
                bt_address
            );
            let _ = event_tx.send(DiscoveryEvent::bluetooth_discovered(
                device_info,
                bt_address,
            ));
        } else {
            debug!(
                "Updated Bluetooth device: {} at {}",
                device_info.device_name, bt_address
            );
            let _ = event_tx.send(DiscoveryEvent::bluetooth_updated(device_info, bt_address));
        }

        Ok(())
    }

    /// Spawn timeout checker task
    fn spawn_timeout_checker(&self) {
        let last_seen = self.last_seen.clone();
        let device_cache = self.device_cache.clone();
        let event_tx = self.event_tx.clone();
        let timeout_duration = self.config.device_timeout;

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(10));

            loop {
                interval.tick().await;

                let current_time = current_timestamp();
                let mut last_seen_map = last_seen.write().await;
                let mut device_cache_map = device_cache.write().await;
                let mut timed_out = Vec::new();

                for (bt_address, &last_seen_time) in last_seen_map.iter() {
                    if current_time - last_seen_time > timeout_duration.as_secs() {
                        timed_out.push(bt_address.clone());
                    }
                }

                for bt_address in timed_out {
                    if let Some(device_info) = device_cache_map.remove(&bt_address) {
                        info!("Bluetooth device timed out: {} ({})", device_info.device_name, bt_address);
                        last_seen_map.remove(&bt_address);
                        let _ = event_tx.send(DiscoveryEvent::DeviceTimeout {
                            device_id: device_info.device_id,
                        });
                    }
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

    /// Get list of paired devices
    pub async fn get_paired_devices(&self) -> Result<Vec<(Address, String)>> {
        let adapter = self.adapter.as_ref().ok_or_else(|| {
            ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No Bluetooth adapter available",
            ))
        })?;

        let devices = adapter
            .device_addresses()
            .await
            .map_err(|e| ProtocolError::Io(std::io::Error::other(e)))?;

        let mut paired = Vec::new();
        for addr in devices {
            if let Ok(device) = adapter.device(addr) {
                if device.is_paired().await.unwrap_or(false) {
                    let name = device
                        .name()
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| addr.to_string());
                    paired.push((addr, name));
                }
            }
        }

        Ok(paired)
    }
}

/// Determine device type from Bluetooth class
fn device_type_from_class(class: u32) -> DeviceType {
    // Bluetooth device class major codes
    // See https://www.bluetooth.com/specifications/assigned-numbers/baseband/
    let major_class = (class >> 8) & 0x1F;

    match major_class {
        0x01 => DeviceType::Desktop,   // Computer
        0x02 => DeviceType::Phone,     // Phone
        0x03 => DeviceType::Desktop,   // LAN/Network Access Point
        0x04 => DeviceType::Desktop,   // Audio/Video
        0x05 => DeviceType::Desktop,   // Peripheral
        0x06 => DeviceType::Desktop,   // Imaging
        0x07 => DeviceType::Desktop,   // Wearable
        0x08 => DeviceType::Desktop,   // Toy
        0x09 => DeviceType::Desktop,   // Health
        _ => DeviceType::Phone,        // Unknown - assume phone
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
        assert!(config.paired_only);
    }

    #[tokio::test]
    async fn test_bluetooth_discovery_creation() {
        let service = BluetoothDiscoveryService::with_defaults().await;
        assert!(service.is_ok());
    }

    #[test]
    fn test_device_type_from_class() {
        // Computer
        assert_eq!(device_type_from_class(0x100), DeviceType::Desktop);
        // Phone
        assert_eq!(device_type_from_class(0x200), DeviceType::Phone);
        // Unknown
        assert_eq!(device_type_from_class(0x000), DeviceType::Phone);
    }
}
