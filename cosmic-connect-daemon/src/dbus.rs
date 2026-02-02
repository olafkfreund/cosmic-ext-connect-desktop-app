//! DBus Interface for CConnect Daemon
//!
//! Provides IPC between the background daemon and COSMIC panel applet.
//! Exposes device management, pairing, and plugin actions via DBus.

use anyhow::{Context, Result};
use cosmic_connect_protocol::plugins::filesync::{
    ConflictStrategy as FilesyncConflictStrategy, FileSyncPlugin, SyncFolder as FilesyncFolder,
};
use cosmic_connect_protocol::{ConnectionManager, Device, DeviceManager, PluginManager};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use zbus::object_server::{InterfaceRef, SignalEmitter};
use zbus::{connection, interface, Connection};

/// Tracks active file transfers with cancellation support
pub struct TransferManager {
    /// Map of transfer_id -> cancellation flag
    active_transfers: Arc<RwLock<HashMap<String, Arc<AtomicBool>>>>,
}

impl TransferManager {
    /// Create a new transfer manager
    pub fn new() -> Self {
        Self {
            active_transfers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new transfer and get its cancellation flag
    pub async fn register_transfer(&self, transfer_id: String) -> Arc<AtomicBool> {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.active_transfers
            .write()
            .await
            .insert(transfer_id, cancel_flag.clone());
        cancel_flag
    }

    /// Cancel a transfer by ID
    pub async fn cancel_transfer(&self, transfer_id: &str) -> bool {
        if let Some(cancel_flag) = self.active_transfers.read().await.get(transfer_id) {
            cancel_flag.store(true, Ordering::SeqCst);
            info!("Transfer {} marked for cancellation", transfer_id);
            true
        } else {
            warn!("Transfer {} not found", transfer_id);
            false
        }
    }

    /// Remove a completed or cancelled transfer
    pub async fn remove_transfer(&self, transfer_id: &str) {
        self.active_transfers.write().await.remove(transfer_id);
        debug!("Transfer {} removed from tracking", transfer_id);
    }
}

impl Default for TransferManager {
    fn default() -> Self {
        Self::new()
    }
}

/// DBus service name
pub const SERVICE_NAME: &str = "com.system76.CosmicConnect";

/// DBus object path
pub const OBJECT_PATH: &str = "/com/system76/CosmicConnect";

/// DBus interface name
#[allow(dead_code)]
pub const INTERFACE_NAME: &str = "com.system76.CosmicConnect";

/// Device state for DBus serialization
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, zbus::zvariant::Type)]
pub struct DeviceInfo {
    /// Device ID
    pub id: String,
    /// Device name
    pub name: String,
    /// Device type
    pub device_type: String,
    /// Is device paired
    pub is_paired: bool,
    /// Is device reachable
    pub is_reachable: bool,
    /// Is device connected (TLS)
    pub is_connected: bool,
    /// Has pending pairing request
    pub has_pairing_request: bool,
    /// Last seen timestamp (UNIX timestamp)
    pub last_seen: i64,
    /// Supported incoming plugin capabilities
    pub incoming_capabilities: Vec<String>,
    /// Supported outgoing plugin capabilities
    pub outgoing_capabilities: Vec<String>,
}

impl From<&Device> for DeviceInfo {
    fn from(device: &Device) -> Self {
        Self {
            id: device.id().to_string(),
            name: device.name().to_string(),
            device_type: device.info.device_type.as_str().to_string(),
            is_paired: device.is_paired(),
            is_reachable: device.is_reachable(),
            is_connected: device.is_connected(),
            has_pairing_request: false, // Will be updated by caller if needed
            last_seen: device.last_seen as i64,
            incoming_capabilities: device.info.incoming_capabilities.clone(),
            outgoing_capabilities: device.info.outgoing_capabilities.clone(),
        }
    }
}

/// Battery status for DBus serialization
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, zbus::zvariant::Type)]
pub struct BatteryStatus {
    /// Battery level percentage (0-100)
    pub level: i32,
    /// Is device charging
    pub is_charging: bool,
}

/// Screen share statistics for DBus serialization
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, zbus::zvariant::Type)]
pub struct ScreenShareStats {
    /// Current number of viewers
    pub viewer_count: u32,
    /// Session duration in seconds
    pub duration_secs: u64,
    /// Total frames sent
    pub frames_sent: u64,
    /// Average FPS
    pub avg_fps: u64,
}

/// Contact information for DBus serialization
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, zbus::zvariant::Type)]
pub struct ContactInfo {
    /// Contact unique identifier
    pub uid: String,
    /// Contact name
    pub name: String,
    /// Phone numbers
    pub phone_numbers: Vec<String>,
    /// Email addresses
    pub emails: Vec<String>,
    /// vCard data
    pub vcard: String,
}

/// Daemon performance metrics for DBus serialization
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, zbus::zvariant::Type)]
pub struct DaemonMetrics {
    /// Uptime in seconds
    pub uptime_seconds: u64,
    /// Total packets sent
    pub packets_sent: u64,
    /// Total packets received
    pub packets_received: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Number of active connections
    pub active_connections: u32,
    /// Number of paired devices
    pub paired_devices: u32,
    /// Total plugin invocations
    pub plugin_invocations: u64,
    /// Total plugin errors
    pub plugin_errors: u64,
    /// Packets per second (averaged)
    pub packets_per_second: f64,
    /// Bandwidth in bytes per second (averaged)
    pub bandwidth_bps: f64,
}

/// Sync Folder configuration for DBus serialization
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, zbus::zvariant::Type)]
pub struct SyncFolderInfo {
    pub folder_id: String,
    pub path: String,
    pub strategy: String,
}

impl From<&FilesyncFolder> for SyncFolderInfo {
    fn from(folder: &FilesyncFolder) -> Self {
        Self {
            folder_id: folder.folder_id.clone(),
            path: folder.local_path.to_string_lossy().to_string(),
            strategy: format!("{:?}", folder.conflict_strategy),
        }
    }
}

/// DBus interface for CConnect daemon
pub struct CConnectInterface {
    /// Device manager
    device_manager: Arc<RwLock<DeviceManager>>,
    /// Plugin manager
    plugin_manager: Arc<RwLock<PluginManager>>,
    /// Connection manager
    connection_manager: Arc<RwLock<ConnectionManager>>,
    /// Device configuration registry
    device_config_registry: Arc<RwLock<crate::device_config::DeviceConfigRegistry>>,
    /// Pairing service (optional - may not be started yet)
    pairing_service: Option<Arc<RwLock<cosmic_connect_protocol::pairing::PairingService>>>,
    /// MPRIS manager for local media player control (optional)
    mpris_manager: Option<Arc<crate::mpris_manager::MprisManager>>,
    /// Pending pairing requests (device_id -> has_pending_request)
    pending_pairing_requests: Arc<RwLock<HashMap<String, bool>>>,
    /// DBus connection for emitting signals
    dbus_connection: Connection,
    /// Performance metrics (if enabled)
    metrics: Option<Arc<RwLock<crate::diagnostics::Metrics>>>,
    /// Daemon configuration (for settings management)
    config: Arc<RwLock<crate::config::Config>>,
    /// Transfer manager for tracking and cancelling file transfers
    transfer_manager: Arc<TransferManager>,
    /// Tokio runtime handle for spawning async tasks from zbus executor
    tokio_handle: Handle,
}

impl CConnectInterface {
    /// Create a new DBus interface
    pub fn new(
        device_manager: Arc<RwLock<DeviceManager>>,
        plugin_manager: Arc<RwLock<PluginManager>>,
        connection_manager: Arc<RwLock<ConnectionManager>>,
        device_config_registry: Arc<RwLock<crate::device_config::DeviceConfigRegistry>>,
        pairing_service: Option<Arc<RwLock<cosmic_connect_protocol::pairing::PairingService>>>,
        mpris_manager: Option<Arc<crate::mpris_manager::MprisManager>>,
        pending_pairing_requests: Arc<RwLock<HashMap<String, bool>>>,
        dbus_connection: Connection,
        metrics: Option<Arc<RwLock<crate::diagnostics::Metrics>>>,
        config: Arc<RwLock<crate::config::Config>>,
        tokio_handle: Handle,
    ) -> Self {
        Self {
            device_manager,
            plugin_manager,
            connection_manager,
            device_config_registry,
            pairing_service,
            mpris_manager,
            pending_pairing_requests,
            dbus_connection,
            metrics,
            config,
            transfer_manager: Arc::new(TransferManager::new()),
            tokio_handle,
        }
    }

    /// Emit a device plugin state changed signal
    async fn emit_plugin_state_changed(&self, device_id: &str, plugin_name: &str, enabled: bool) {
        let object_server = self.dbus_connection.object_server();
        let iface_ref = match object_server
            .interface::<_, CConnectInterface>(OBJECT_PATH)
            .await
        {
            Ok(iface) => iface,
            Err(e) => {
                warn!("Failed to get interface for signal emission: {}", e);
                return;
            }
        };

        if let Err(e) = Self::device_plugin_state_changed(
            iface_ref.signal_emitter(),
            device_id,
            plugin_name,
            enabled,
        )
        .await
        {
            warn!("Failed to emit device_plugin_state_changed signal: {}", e);
        } else {
            debug!(
                "Emitted device_plugin_state_changed signal: {} on {} = {}",
                plugin_name, device_id, enabled
            );
        }
    }
}

/// Attempt to manually connect to a device at the specified address
async fn attempt_manual_connection(
    connection_manager: &Arc<RwLock<ConnectionManager>>,
    device_manager: &Arc<RwLock<cosmic_connect_protocol::DeviceManager>>,
    addr: std::net::SocketAddr,
    temp_id: &str,
) -> Result<String> {
    // Try to connect - this will trigger identity exchange
    let conn_mgr = connection_manager.read().await;
    conn_mgr.connect(temp_id, addr).await?;
    drop(conn_mgr);

    // Wait a bit for identity exchange to complete
    // In a real implementation, we'd listen for ConnectionEvent::Connected
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Check if we got a real device ID from the identity exchange
    let dev_mgr = device_manager.read().await;

    // Look for a device with this transport address
    for device in dev_mgr.devices() {
        if let (Some(host), Some(port)) = (&device.host, device.port) {
            // Parse the host to an IP address and compare with the target socket address
            if let Ok(ip) = host.parse::<std::net::IpAddr>() {
                let device_addr = std::net::SocketAddr::new(ip, port);
                if device_addr == addr {
                    return Ok(device.id().to_string());
                }
            }
        }
    }

    Err(anyhow::anyhow!("Device did not respond with identity"))
}

/// Parse vCard data to extract contact information
fn parse_vcard(vcard_data: &str) -> (String, Vec<String>, Vec<String>) {
    let mut name = String::new();
    let mut phone_numbers = Vec::new();
    let mut emails = Vec::new();

    for line in vcard_data.lines() {
        let line = line.trim();
        if line.starts_with("FN:") {
            name = line[3..].to_string();
        } else if line.starts_with("TEL") {
            if let Some(number) = line.split(':').nth(1) {
                phone_numbers.push(number.to_string());
            }
        } else if line.starts_with("EMAIL") {
            if let Some(email) = line.split(':').nth(1) {
                emails.push(email.to_string());
            }
        }
    }

    // Use UID as fallback name if FN not found
    if name.is_empty() {
        name = "Unknown".to_string();
    }

    (name, phone_numbers, emails)
}

#[interface(name = "com.system76.CosmicConnect")]
impl CConnectInterface {
    /// List all known devices
    ///
    /// Returns a map of device ID to device information for all devices
    /// (paired and unpaired, reachable and unreachable).
    async fn list_devices(&self) -> zbus::fdo::Result<HashMap<String, DeviceInfo>> {
        debug!("DBus: ListDevices called");

        let device_manager = self.device_manager.read().await;
        let devices = device_manager.devices();

        let pending_requests = self.pending_pairing_requests.read().await;

        let mut result = HashMap::new();
        for device in devices {
            let device_id = device.id().to_string();
            let mut info = DeviceInfo::from(device);

            info.has_pairing_request = pending_requests.contains_key(&device_id);
            result.insert(device_id, info);
        }

        info!("DBus: Returning {} devices", result.len());
        Ok(result)
    }

    /// Get information about a specific device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to query
    ///
    /// # Returns
    /// Device information, or error if device not found
    async fn get_device(&self, device_id: String) -> Result<DeviceInfo, zbus::fdo::Error> {
        debug!("DBus: GetDevice called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        let mut info = DeviceInfo::from(device);
        info.has_pairing_request = self
            .pending_pairing_requests
            .read()
            .await
            .contains_key(&device_id);

        Ok(info)
    }

    /// Request pairing with a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to pair with
    ///
    /// # Returns
    /// Success or error message
    async fn pair_device(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: PairDevice called for {}", device_id);

        // Check if pairing service is available
        let pairing_service = self
            .pairing_service
            .as_ref()
            .ok_or_else(|| zbus::fdo::Error::Failed("Pairing service not initialized".to_string()))?
            .clone();

        // Get device info from device manager
        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        // Check if already paired
        if device.is_paired() {
            return Err(zbus::fdo::Error::Failed(format!(
                "Device {} is already paired",
                device_id
            )));
        }

        let device_info = device.info.clone();
        let remote_addr: std::net::SocketAddr = format!(
            "{}:{}",
            device.host.as_deref().unwrap_or("0.0.0.0"),
            device.port.unwrap_or(1816)
        )
        .parse()
        .map_err(|e| zbus::fdo::Error::Failed(format!("Invalid remote address: {}", e)))?;

        drop(device_manager);

        // Spawn the pairing request on the Tokio runtime
        // This is needed because zbus uses its own executor that isn't Tokio
        let _device_id_clone = device_id.clone();
        let _result = self
            .tokio_handle
            .spawn(async move {
                let pairing_service = pairing_service.read().await;
                pairing_service
                    .request_pairing(device_info, remote_addr)
                    .await
            })
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Tokio task failed: {}", e)))?
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to request pairing: {}", e)))?;

        info!("Pairing request sent to device {}", device_id);
        Ok(())
    }

    /// Unpair a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to unpair
    ///
    /// # Returns
    /// Success or error message
    async fn unpair_device(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: UnpairDevice called for {}", device_id);

        // Check if pairing service is available
        let pairing_service = self.pairing_service.as_ref().ok_or_else(|| {
            zbus::fdo::Error::Failed("Pairing service not initialized".to_string())
        })?;

        // Check if device exists
        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        // Check if device is paired
        if !device.is_paired() {
            return Err(zbus::fdo::Error::Failed(format!(
                "Device {} is not paired",
                device_id
            )));
        }

        drop(device_manager);

        // Unpair the device
        let pairing_service = pairing_service.read().await;
        pairing_service
            .unpair(&device_id)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to unpair device: {}", e)))?;

        info!("Device {} unpaired successfully", device_id);
        Ok(())
    }

    /// Accept a pairing request from a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to accept pairing from
    ///
    /// # Returns
    /// Success or error message
    async fn accept_pairing(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: AcceptPairing called for {}", device_id);

        // Check if pairing service is available
        let pairing_service = self.pairing_service.as_ref().ok_or_else(|| {
            zbus::fdo::Error::Failed("Pairing service not initialized".to_string())
        })?;

        // Accept pairing (certificate is retrieved from stored request)
        let pairing_service = pairing_service.read().await;
        pairing_service
            .accept_pairing(&device_id)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to accept pairing: {}", e)))?;

        info!("Pairing accepted for device {}", device_id);
        Ok(())
    }

    /// Reject a pairing request from a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to reject pairing from
    ///
    /// # Returns
    /// Success or error message
    async fn reject_pairing(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: RejectPairing called for {}", device_id);

        // Check if pairing service is available
        let pairing_service = self.pairing_service.as_ref().ok_or_else(|| {
            zbus::fdo::Error::Failed("Pairing service not initialized".to_string())
        })?;

        // Reject pairing
        let pairing_service = pairing_service.read().await;
        pairing_service
            .reject_pairing(&device_id)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to reject pairing: {}", e)))?;

        info!("Pairing rejected for device {}", device_id);
        Ok(())
    }

    /// Trigger device discovery
    ///
    /// Broadcasts UDP discovery packet to find new devices on the network.
    async fn refresh_discovery(&self) -> Result<(), zbus::fdo::Error> {
        info!("DBus: RefreshDiscovery called");

        // Discovery is continuous in the daemon, so this is a no-op
        // In a real implementation, you might trigger a broadcast here
        Ok(())
    }

    /// Connect to a device at a specific address
    ///
    /// # Arguments
    /// * `address` - The IP:port or hostname:port to connect to (defaults to port 1716 if not specified)
    ///
    /// # Returns
    /// * Success if connection attempt initiated
    async fn connect_to_address(&self, address: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: ConnectToAddress called with address: {}", address);

        // Parse the address, default to port 1716 if not specified
        use std::net::{SocketAddr, ToSocketAddrs};

        let socket_addr: SocketAddr = if address.contains(':') {
            // Address includes port
            address
                .to_socket_addrs()
                .map_err(|e| zbus::fdo::Error::Failed(format!("Invalid address format: {}", e)))?
                .next()
                .ok_or_else(|| zbus::fdo::Error::Failed("Could not resolve address".to_string()))?
        } else {
            // Address is just hostname/IP, add default port 1716
            format!("{}:1716", address)
                .to_socket_addrs()
                .map_err(|e| zbus::fdo::Error::Failed(format!("Invalid address format: {}", e)))?
                .next()
                .ok_or_else(|| zbus::fdo::Error::Failed("Could not resolve address".to_string()))?
        };

        info!("Attempting manual connection to {}", socket_addr);

        // Spawn a task to attempt connection
        // We'll connect and wait for the remote device to send its identity
        let connection_manager = self.connection_manager.clone();
        let device_manager = self.device_manager.clone();

        tokio::spawn(async move {
            // Create a temporary device ID for the connection attempt
            // The real device ID will be obtained from the identity packet
            let temp_id = format!("manual_{}", socket_addr);

            match attempt_manual_connection(
                &connection_manager,
                &device_manager,
                socket_addr,
                &temp_id,
            )
            .await
            {
                Ok(device_id) => {
                    info!("Successfully connected to device: {}", device_id);
                }
                Err(e) => {
                    warn!("Failed to connect to {}: {}", socket_addr, e);
                }
            }
        });

        Ok(())
    }

    /// Get device connection state
    ///
    /// # Arguments
    /// * `device_id` - The device ID to query
    ///
    /// # Returns
    /// Connection state: "connected", "paired", "reachable", or "unknown"
    async fn get_device_state(&self, device_id: String) -> Result<String, zbus::fdo::Error> {
        debug!("DBus: GetDeviceState called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        let state = if device.is_connected() {
            "connected"
        } else if device.is_paired() {
            "paired"
        } else if device.is_reachable() {
            "reachable"
        } else {
            "unknown"
        };

        Ok(state.to_string())
    }

    /// Send a ping to a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to ping
    /// * `message` - Optional message to include in the ping
    async fn send_ping(&self, device_id: String, message: String) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: SendPing called for {} with message '{}'",
            device_id, message
        );

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create ping packet
        use cosmic_connect_protocol::Packet;
        use serde_json::json;

        let body = if !message.is_empty() {
            json!({ "message": message })
        } else {
            json!({})
        };

        let packet = Packet::new("cconnect.ping", body);

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send ping: {}", e)))?;

        info!("DBus: Ping sent successfully to {}", device_id);
        Ok(())
    }

    /// Trigger find phone on a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to trigger find phone on
    async fn find_phone(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: FindPhone called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create findmyphone packet
        use cosmic_connect_protocol::Packet;
        use serde_json::json;

        let packet = Packet::new("cconnect.findmyphone.request", json!({}));

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Failed to send find phone request: {}", e))
            })?;

        info!(
            "DBus: Find phone request sent successfully to {}",
            device_id
        );
        Ok(())
    }

    /// Mute incoming call ringer on a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to mute call on
    async fn mute_call(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: MuteCall called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create telephony mute packet
        use cosmic_connect_protocol::Packet;
        use serde_json::json;

        let packet = Packet::new("cconnect.telephony.mute", json!({}));

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Failed to send mute call request: {}", e))
            })?;

        info!("DBus: Mute call request sent successfully to {}", device_id);
        Ok(())
    }

    /// Send SMS message from a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to send SMS from
    /// * `phone_number` - Recipient phone number
    /// * `message` - Message body
    async fn send_sms(
        &self,
        device_id: String,
        phone_number: String,
        message: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: SendSMS called for {} to {} with message length: {}",
            device_id,
            phone_number,
            message.len()
        );

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create SMS request packet
        use cosmic_connect_protocol::Packet;
        use serde_json::json;

        let packet = Packet::new(
            "cconnect.sms.request",
            json!({
                "phoneNumber": phone_number,
                "messageBody": message
            }),
        );

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send SMS request: {}", e)))?;

        info!("DBus: SMS request sent successfully to {}", device_id);
        Ok(())
    }

    /// Lock a device remotely
    ///
    /// # Arguments
    /// * `device_id` - The device ID to lock
    async fn lock_device(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: LockDevice called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create lock packet
        use cosmic_connect_protocol::Packet;
        use serde_json::json;

        let packet = Packet::new("cconnect.lock.request", json!({}));

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send lock request: {}", e)))?;

        info!("DBus: Lock request sent successfully to {}", device_id);
        Ok(())
    }

    /// Send a power control action to a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to control
    /// * `action` - Power action: "shutdown", "hibernate", "suspend"
    async fn power_action(
        &self,
        device_id: String,
        action: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: PowerAction called for {} with action: {}",
            device_id, action
        );

        // Validate action
        match action.as_str() {
            "shutdown" | "hibernate" | "suspend" => {}
            _ => {
                return Err(zbus::fdo::Error::Failed(format!(
                    "Invalid power action: {}. Must be 'shutdown', 'hibernate', or 'suspend'",
                    action
                )));
            }
        }

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create power packet
        use cosmic_connect_protocol::Packet;
        use serde_json::json;

        let packet = Packet::new("cconnect.power.request", json!({ "action": action }));

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send power action: {}", e)))?;

        info!(
            "DBus: Power action '{}' sent successfully to {}",
            action, device_id
        );
        Ok(())
    }

    /// Wake a device using Wake-on-LAN
    ///
    /// # Arguments
    /// * `device_id` - The device ID to wake
    async fn wake_device(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: WakeDevice called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let _device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        drop(device_manager);

        // Create WOL packet
        use cosmic_connect_protocol::Packet;
        use serde_json::json;

        let packet = Packet::new("cconnect.wol.request", json!({}));

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send wake request: {}", e)))?;

        info!("DBus: Wake request sent successfully to {}", device_id);
        Ok(())
    }

    /// Set volume on remote device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to control
    /// * `volume` - Volume level (0.0 to 1.0)
    async fn set_device_volume(
        &self,
        device_id: String,
        volume: f64,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: SetDeviceVolume called for {} with volume: {}",
            device_id, volume
        );

        // Validate volume range
        if !(0.0..=1.0).contains(&volume) {
            return Err(zbus::fdo::Error::Failed(format!(
                "Invalid volume: {}. Must be between 0.0 and 1.0",
                volume
            )));
        }

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create systemvolume packet
        use cosmic_connect_protocol::Packet;
        use serde_json::json;

        let packet = Packet::new(
            "cconnect.systemvolume.request",
            json!({
                "volume": volume
            }),
        );

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Failed to send volume request: {}", e))
            })?;

        info!("DBus: Volume request sent successfully to {}", device_id);
        Ok(())
    }

    /// Request system info from device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to query
    async fn request_system_info(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: RequestSystemInfo called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create systemmonitor packet
        use cosmic_connect_protocol::Packet;
        use serde_json::json;

        let packet = Packet::new("cconnect.systemmonitor.request", json!({}));

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Failed to send system info request: {}", e))
            })?;

        info!(
            "DBus: System info request sent successfully to {}",
            device_id
        );
        Ok(())
    }

    /// Request screenshot from device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to capture from
    async fn take_screenshot(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: TakeScreenshot called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create screenshot packet
        use cosmic_connect_protocol::Packet;
        use serde_json::json;

        let packet = Packet::new("cconnect.screenshot.request", json!({}));

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Failed to send screenshot request: {}", e))
            })?;

        info!(
            "DBus: Screenshot request sent successfully to {}",
            device_id
        );
        Ok(())
    }

    /// Share a file with a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to share with
    /// * `path` - Absolute path to the file to share
    async fn share_file(&self, device_id: String, path: String) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: ShareFile called for {} with path '{}'",
            device_id, path
        );

        // Validate device exists and is connected
        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Validate file exists (using std::fs which doesn't require tokio runtime)
        if !std::path::Path::new(&path).exists() {
            return Err(zbus::fdo::Error::Failed(format!(
                "File not found: {}",
                path
            )));
        }

        // Generate unique transfer ID
        let timestamp_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_else(|_| std::time::Duration::from_secs(0))
            .as_millis();
        let transfer_id = format!("{}_{}", device_id, timestamp_millis);

        // Register transfer and get cancellation flag
        let cancel_flag = self
            .transfer_manager
            .register_transfer(transfer_id.clone())
            .await;

        // Clone all needed values for the spawned task
        let file_path = path.clone();
        let device_id_clone = device_id.clone();
        let transfer_id_clone = transfer_id.clone();
        let dbus_conn = self.dbus_connection.clone();
        let transfer_manager = self.transfer_manager.clone();
        let conn_manager = self.connection_manager.clone();
        let tokio_handle = self.tokio_handle.clone();

        // Spawn the entire file transfer operation on tokio runtime
        // This ensures all tokio operations have access to the runtime
        // We use self.tokio_handle.spawn() because the zbus executor doesn't have a tokio runtime context
        self.tokio_handle.spawn(async move {
            use cosmic_connect_protocol::plugins::share::{FileShareInfo, SharePlugin};
            use cosmic_connect_protocol::{FileTransferInfo, TlsPayloadServer};

            // Extract file metadata (inside tokio runtime)
            let file_info = match FileTransferInfo::from_path(&file_path).await {
                Ok(info) => info,
                Err(e) => {
                    warn!("Failed to read file metadata: {}", e);
                    return;
                }
            };

            info!(
                "DBus: Sharing file '{}' ({} bytes) to {}",
                file_info.filename, file_info.size, device_id_clone
            );

            // Get TLS config from connection manager
            let tls_config = {
                let conn_mgr = conn_manager.read().await;
                conn_mgr.tls_config()
            };

            // Create TLS payload server on available port (inside tokio runtime)
            let server = match TlsPayloadServer::new(tls_config).await {
                Ok(s) => s,
                Err(e) => {
                    warn!("Failed to create TLS payload server: {}", e);
                    return;
                }
            };
            let port = server.port();

            info!("DBus: TLS Payload server listening on port {}", port);

            // Create share packet with file info and payload transfer port
            let share_info: FileShareInfo = file_info.clone().into();
            let plugin = SharePlugin::new();
            let packet = plugin.create_file_packet(share_info, port);

            // Send packet via ConnectionManager
            let conn_mgr = conn_manager.read().await;
            if let Err(e) = conn_mgr.send_packet(&device_id_clone, &packet).await {
                warn!("Failed to send share packet: {}", e);
                return;
            }
            drop(conn_mgr);

            info!(
                "DBus: Share packet sent to {}, waiting for connection",
                device_id_clone
            );

            let filename = file_info.filename.clone();

            // Create progress callback that emits DBus signals
            let conn = dbus_conn.clone();
            let tid = transfer_id_clone.clone();
            let did = device_id_clone.clone();
            let fname = filename.clone();
            let cancel_flag_inner = cancel_flag.clone();
            let handle_inner = tokio_handle.clone();

            let progress_callback =
                Box::new(move |bytes_transferred: u64, total_bytes: u64| -> bool {
                    // Check if transfer is cancelled
                    if cancel_flag_inner.load(Ordering::SeqCst) {
                        info!("Transfer {} cancelled by user", tid);
                        return false; // Stop transfer
                    }

                    let conn_clone = conn.clone();
                    let tid_clone = tid.clone();
                    let did_clone = did.clone();
                    let fname_clone = fname.clone();

                    // Emit progress signal (non-blocking)
                    // Use the handle to spawn since we may be called from a non-tokio context
                    handle_inner.spawn(async move {
                        if let Ok(object_server) = conn_clone
                            .object_server()
                            .interface::<_, CConnectInterface>(OBJECT_PATH)
                            .await
                        {
                            let _ = CConnectInterface::transfer_progress(
                                object_server.signal_emitter(),
                                &tid_clone,
                                &did_clone,
                                &fname_clone,
                                bytes_transferred,
                                total_bytes,
                                "sending",
                            )
                            .await;
                        }
                    });

                    true // Continue transfer
                });

            // Attach progress callback and start transfer
            let server_with_progress = server.with_progress(progress_callback);
            let result = server_with_progress.send_file(&file_path).await;

            // Determine completion status
            let (success, error_msg) = if cancel_flag.load(Ordering::SeqCst) {
                (false, "Transfer cancelled by user".to_string())
            } else {
                (
                    result.is_ok(),
                    result
                        .as_ref()
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_default(),
                )
            };

            // Emit completion signal
            if let Ok(object_server) = dbus_conn
                .object_server()
                .interface::<_, CConnectInterface>(OBJECT_PATH)
                .await
            {
                let _ = CConnectInterface::transfer_complete(
                    object_server.signal_emitter(),
                    &transfer_id_clone,
                    &device_id_clone,
                    &filename,
                    success,
                    &error_msg,
                )
                .await;
            }

            // Remove transfer from manager
            transfer_manager.remove_transfer(&transfer_id_clone).await;

            if success {
                info!(
                    "File transfer completed successfully for device {}",
                    device_id_clone
                );
            } else {
                warn!(
                    "File transfer failed for device {}: {}",
                    device_id_clone, error_msg
                );
            }
        });

        info!(
            "DBus: File sharing initiated for {} (transfer_id: {})",
            device_id, transfer_id
        );
        Ok(())
    }

    /// Share text or URL with a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to share with
    /// * `text` - Text or URL to share
    async fn share_text(&self, device_id: String, text: String) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: ShareText called for {} with text '{}'",
            device_id, text
        );

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create share text packet
        use cosmic_connect_protocol::Packet;
        use serde_json::json;

        let packet = Packet::new("cconnect.share.request", json!({ "text": text }));

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to share text: {}", e)))?;

        info!("DBus: Text shared successfully to {}", device_id);
        Ok(())
    }

    /// Share a URL with a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to share with
    /// * `url` - URL to share (will be opened in default browser on receiving device)
    async fn share_url(&self, device_id: String, url: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: ShareUrl called for {} with URL '{}'", device_id, url);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create share URL packet
        use cosmic_connect_protocol::Packet;
        use serde_json::json;

        let packet = Packet::new("cconnect.share.request", json!({ "url": url }));

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to share URL: {}", e)))?;

        info!("DBus: URL shared successfully to {}", device_id);
        Ok(())
    }

    /// Cancel an active file transfer
    ///
    /// # Arguments
    /// * `transfer_id` - The transfer ID to cancel
    ///
    /// # Returns
    /// * `Ok(())` if transfer was cancelled or not found
    /// * `Err` if cancellation failed
    async fn cancel_transfer(&self, transfer_id: String) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: CancelTransfer called for transfer_id: {}",
            transfer_id
        );

        let cancelled = self.transfer_manager.cancel_transfer(&transfer_id).await;

        if cancelled {
            info!("Transfer {} marked for cancellation", transfer_id);
            Ok(())
        } else {
            // Transfer not found - this is not an error, it may have already completed
            debug!(
                "Transfer {} not found (may have already completed)",
                transfer_id
            );
            Ok(())
        }
    }

    /// Send a notification to a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to send to
    /// * `title` - Notification title
    /// * `body` - Notification body text
    async fn send_notification(
        &self,
        device_id: String,
        title: String,
        body: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: SendNotification called for {} with title '{}'",
            device_id, title
        );

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create notification using NotificationPlugin's helper
        use cosmic_connect_protocol::plugins::notification::Notification;
        use cosmic_connect_protocol::Packet;
        use std::time::{SystemTime, UNIX_EPOCH};

        // Generate a unique notification ID based on timestamp
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| std::time::Duration::from_secs(0))
            .as_millis()
            .to_string();

        // Create notification with COSMIC Desktop as the app name
        let notification = Notification::new(
            timestamp.clone(),
            "COSMIC Desktop",
            title,
            body,
            true, // is_clearable
        );

        // Create notification packet
        let packet_body = serde_json::to_value(&notification).map_err(|e| {
            zbus::fdo::Error::Failed(format!("Failed to serialize notification: {}", e))
        })?;
        let packet = Packet::new("cconnect.notification", packet_body);

        // Send packet via ConnectionManager
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send notification: {}", e)))?;

        info!(
            "DBus: Notification sent successfully to {} (id: {})",
            device_id, timestamp
        );
        Ok(())
    }

    /// Add a folder to sync with a device
    async fn add_sync_folder(
        &self,
        device_id: String,
        folder_id: String,
        path: String,
        strategy: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: AddSyncFolder called for {} (path: {})",
            device_id, path
        );

        let mut plugin_manager = self.plugin_manager.write().await;

        // Get the filesync plugin for this device
        if let Some(plugin) = plugin_manager.get_device_plugin_mut(&device_id, "filesync") {
            // Downcast to concrete FileSyncPlugin
            if let Some(filesync) = plugin.as_any_mut().downcast_mut::<FileSyncPlugin>() {
                // Parse strategy
                let conflict_strategy = match strategy.to_lowercase().as_str() {
                    "localwins" | "local_wins" => FilesyncConflictStrategy::LastModifiedWins, // Defaulting for simple UI
                    "remotewins" | "remote_wins" => FilesyncConflictStrategy::LastModifiedWins, // Need proper mapping
                    "manual" => FilesyncConflictStrategy::Manual,
                    "keepboth" | "keep_both" => FilesyncConflictStrategy::KeepBoth,
                    "size" | "sizebased" => FilesyncConflictStrategy::SizeBased,
                    _ => FilesyncConflictStrategy::LastModifiedWins, // Default
                };

                let path_buf = PathBuf::from(&path);

                filesync
                    .configure_folder(folder_id, path_buf, conflict_strategy)
                    .await
                    .map_err(|e| {
                        zbus::fdo::Error::Failed(format!("Failed to configure folder: {}", e))
                    })?;

                info!("Added sync folder successfully");
                Ok(())
            } else {
                Err(zbus::fdo::Error::Failed(
                    "Plugin is not FileSyncPlugin".to_string(),
                ))
            }
        } else {
            Err(zbus::fdo::Error::Failed(
                "FileSync plugin not found for device".to_string(),
            ))
        }
    }

    /// Remove a sync folder from a device
    async fn remove_sync_folder(
        &self,
        device_id: String,
        folder_id: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: RemoveSyncFolder called for {} (folder: {})",
            device_id, folder_id
        );

        let mut plugin_manager = self.plugin_manager.write().await;

        if let Some(plugin) = plugin_manager.get_device_plugin_mut(&device_id, "filesync") {
            if let Some(filesync) = plugin.as_any_mut().downcast_mut::<FileSyncPlugin>() {
                filesync.remove_folder(&folder_id).await.map_err(|e| {
                    zbus::fdo::Error::Failed(format!("Failed to remove folder: {}", e))
                })?;

                info!("Removed sync folder successfully");
                Ok(())
            } else {
                Err(zbus::fdo::Error::Failed(
                    "Plugin is not FileSyncPlugin".to_string(),
                ))
            }
        } else {
            Err(zbus::fdo::Error::Failed(
                "FileSync plugin not found for device".to_string(),
            ))
        }
    }

    /// Get list of synced folders for a device
    async fn get_sync_folders(
        &self,
        device_id: String,
    ) -> Result<Vec<SyncFolderInfo>, zbus::fdo::Error> {
        info!("DBus: GetSyncFolders called for {}", device_id);

        // We only need read access here, but get_device_plugin requires iterating
        // However, PluginManager only has get_device_plugin (which returns &dyn Plugin)
        // and get_device_plugin_mut. We need to downcast.
        // Downcasting for &dyn Plugin to &Concrete requires the trait to implement as_any() which returns &dyn Any.
        // Let's assume Plugin trait has as_any(). checking... Yes it does. (lines 196 in mod.rs)

        let plugin_manager = self.plugin_manager.read().await;

        if let Some(plugin) = plugin_manager.get_device_plugin(&device_id, "filesync") {
            if let Some(filesync) = plugin.as_any().downcast_ref::<FileSyncPlugin>() {
                let folders: Vec<FilesyncFolder> = filesync.get_folders().await;
                let result: Vec<SyncFolderInfo> =
                    folders.iter().map(SyncFolderInfo::from).collect();
                Ok(result)
            } else {
                Err(zbus::fdo::Error::Failed(
                    "Plugin is not FileSyncPlugin".to_string(),
                ))
            }
        } else {
            // If plugin not found (e.g. device not connected or plugin not initialized), return empty list
            Ok(Vec::new())
        }
    }

    /// Get battery status from a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to query
    ///
    /// # Returns
    /// Battery status with level and charging state
    async fn get_battery_status(
        &self,
        device_id: String,
    ) -> Result<BatteryStatus, zbus::fdo::Error> {
        debug!("DBus: GetBatteryStatus called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Query battery status from plugin manager
        let plugin_manager = self.plugin_manager.read().await;
        let status = plugin_manager
            .get_device_battery_status(&device_id)
            .ok_or_else(|| {
                zbus::fdo::Error::Failed("No battery status available for device".to_string())
            })?;

        info!(
            "DBus: Battery status for {}: {}%, charging: {}",
            device_id, status.current_charge, status.is_charging
        );

        Ok(BatteryStatus {
            level: status.current_charge,
            is_charging: status.is_charging,
        })
    }

    /// Get screen share statistics from a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to query
    ///
    /// # Returns
    /// Screen share statistics including viewer count and session metrics
    async fn get_screen_share_stats(
        &self,
        device_id: String,
    ) -> Result<ScreenShareStats, zbus::fdo::Error> {
        debug!("DBus: GetScreenShareStats called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Query screen share stats from plugin manager
        let plugin_manager = self.plugin_manager.read().await;
        let stats = plugin_manager
            .get_device_screen_share_stats(&device_id)
            .ok_or_else(|| {
                zbus::fdo::Error::Failed("No screen share session active for device".to_string())
            })?;

        debug!(
            "DBus: Screen share stats for {}: {} viewers, {} seconds",
            device_id, stats.viewer_count, stats.duration_secs
        );

        Ok(ScreenShareStats {
            viewer_count: stats.viewer_count as u32,
            duration_secs: stats.duration_secs,
            frames_sent: stats.frames_sent,
            avg_fps: stats.avg_fps,
        })
    }

    /// Request battery update from device
    ///
    /// Sends a battery request packet to the device to get fresh battery status.
    ///
    /// # Arguments
    /// * `device_id` - The device ID to request from
    async fn request_battery_update(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: RequestBatteryUpdate called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create battery request packet
        use cosmic_connect_protocol::plugins::battery::BatteryPlugin;
        let plugin = BatteryPlugin::new();
        let packet = plugin.create_battery_request();

        // Send packet
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Failed to send battery request: {}", e))
            })?;

        info!("DBus: Battery update request sent to {}", device_id);
        Ok(())
    }

    /// Request contacts sync from device
    ///
    /// Sends a request to the device to sync all contacts.
    /// This requests UIDs and timestamps first, then can request vCards.
    ///
    /// # Arguments
    /// * `device_id` - The device ID to request from
    async fn request_contacts_sync(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: RequestContactsSync called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Create contacts request packet
        use cosmic_connect_protocol::plugins::contacts::ContactsPlugin;
        let plugin = ContactsPlugin::new();
        let packet = plugin.create_request_all_uids_timestamps();

        // Send packet
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("Failed to send contacts request: {}", e))
            })?;

        info!("DBus: Contacts sync request sent to {}", device_id);
        Ok(())
    }

    /// Get contacts from device
    ///
    /// Retrieves the list of cached contacts for a device.
    /// Call request_contacts_sync first to populate the cache.
    ///
    /// # Arguments
    /// * `device_id` - The device ID to query
    ///
    /// # Returns
    /// List of contacts with name, phone numbers, and emails
    async fn get_contacts(&self, device_id: String) -> Result<Vec<ContactInfo>, zbus::fdo::Error> {
        debug!("DBus: GetContacts called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Get contacts from plugin
        let plugin_manager = self.plugin_manager.read().await;
        let plugin = plugin_manager
            .get_device_plugin(&device_id, "contacts")
            .ok_or_else(|| {
                zbus::fdo::Error::Failed("Contacts plugin not available for device".to_string())
            })?;

        // Downcast to ContactsPlugin
        use cosmic_connect_protocol::plugins::contacts::ContactsPlugin;
        let contacts_plugin = plugin
            .as_any()
            .downcast_ref::<ContactsPlugin>()
            .ok_or_else(|| {
                zbus::fdo::Error::Failed("Failed to access contacts plugin".to_string())
            })?;

        // Get all contact UIDs
        let uids = contacts_plugin.get_all_contact_uids();

        // Build contact info list
        let mut contacts = Vec::new();
        for uid in uids {
            if let Some(vcard) = contacts_plugin.get_vcard(&uid) {
                // Parse vCard to extract name, phone numbers, emails
                let (name, phone_numbers, emails) = parse_vcard(vcard);
                contacts.push(ContactInfo {
                    uid: uid.clone(),
                    name,
                    phone_numbers,
                    emails,
                    vcard: vcard.clone(),
                });
            }
        }

        info!(
            "DBus: Retrieved {} contacts for {}",
            contacts.len(),
            device_id
        );
        Ok(contacts)
    }

    /// Send command list to device
    ///
    /// Sends the list of available commands to the remote device.
    /// Commands are executed on THIS desktop when requested by the remote device.
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    async fn send_command_list(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: SendCommandList called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        let device = device_manager
            .get_device(&device_id)
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", device_id)))?;

        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }

        drop(device_manager);

        // Get the runcommand plugin for this device
        let plugin_manager = self.plugin_manager.read().await;
        let plugin = plugin_manager
            .get_device_plugin(&device_id, "runcommand")
            .ok_or_else(|| {
                zbus::fdo::Error::Failed("RunCommand plugin not found for device".to_string())
            })?;

        // Downcast to RunCommandPlugin to access create_command_list_packet
        use cosmic_connect_protocol::plugins::runcommand::RunCommandPlugin;
        let runcommand_plugin = plugin
            .as_any()
            .downcast_ref::<RunCommandPlugin>()
            .ok_or_else(|| {
                zbus::fdo::Error::Failed("Failed to downcast to RunCommandPlugin".to_string())
            })?;

        // Create command list packet
        let packet = runcommand_plugin.create_command_list_packet().await;

        drop(plugin_manager);

        // Send packet
        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send command list: {}", e)))?;

        info!("DBus: Command list sent to {}", device_id);
        Ok(())
    }

    /// Set device nickname
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `nickname` - The new nickname (empty string to clear)
    async fn set_device_nickname(
        &self,
        device_id: String,
        nickname: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: SetDeviceNickname called for {}: {}",
            device_id, nickname
        );

        let mut registry = self.device_config_registry.write().await;
        let config = registry.get_or_create(&device_id);

        if nickname.is_empty() {
            config.nickname = None;
        } else {
            config.nickname = Some(nickname);
        }

        registry.save().map_err(|e| {
            zbus::fdo::Error::Failed(format!("Failed to save device config: {}", e))
        })?;

        Ok(())
    }

    /// Set notification preference for a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `preference` - The notification preference ("all", "important", or "none")
    async fn set_device_notification_preference(
        &self,
        device_id: String,
        preference: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: SetDeviceNotificationPreference called for {}: {}",
            device_id, preference
        );

        // Parse the preference string
        let pref = match preference.to_lowercase().as_str() {
            "all" => crate::device_config::NotificationPreference::All,
            "important" => crate::device_config::NotificationPreference::Important,
            "none" => crate::device_config::NotificationPreference::None,
            _ => {
                return Err(zbus::fdo::Error::Failed(format!(
                    "Invalid notification preference: {}. Must be 'all', 'important', or 'none'",
                    preference
                )));
            }
        };

        let mut registry = self.device_config_registry.write().await;
        let config = registry.get_or_create(&device_id);

        config.set_notification_preference(pref);

        registry.save().map_err(|e| {
            zbus::fdo::Error::Failed(format!("Failed to save device config: {}", e))
        })?;

        info!(
            "DBus: Notification preference for device {} set to {:?}",
            device_id, pref
        );

        Ok(())
    }

    /// Set plugin enabled state for a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `plugin_name` - The plugin name (ping, battery, notification, share, clipboard, mpris)
    /// * `enabled` - Whether the plugin should be enabled
    async fn set_device_plugin_enabled(
        &self,
        device_id: String,
        plugin_name: String,
        enabled: bool,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: SetDevicePluginEnabled called for {}, plugin {}: {}",
            device_id, plugin_name, enabled
        );

        let mut registry = self.device_config_registry.write().await;
        let config = registry.get_or_create(&device_id);

        config.set_plugin_enabled(&plugin_name, enabled);

        registry.save().map_err(|e| {
            zbus::fdo::Error::Failed(format!("Failed to save device config: {}", e))
        })?;

        info!(
            "DBus: Plugin {} {} for device {}",
            plugin_name,
            if enabled { "enabled" } else { "disabled" },
            device_id
        );

        self.emit_plugin_state_changed(&device_id, &plugin_name, enabled)
            .await;

        Ok(())
    }

    /// Clear device-specific plugin override (use global config)
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `plugin_name` - The plugin name
    async fn clear_device_plugin_override(
        &self,
        device_id: String,
        plugin_name: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: ClearDevicePluginOverride called for {}, plugin {}",
            device_id, plugin_name
        );

        // Get the resulting enabled state after clearing override (will use global config)
        let enabled = {
            let mut registry = self.device_config_registry.write().await;
            {
                let config = registry.get_or_create(&device_id);
                config.clear_plugin_override(&plugin_name);
            } // config borrow ends here

            registry.save().map_err(|e| {
                zbus::fdo::Error::Failed(format!("Failed to save device config: {}", e))
            })?;

            // Get global config to determine final state
            let global_config = self.config.read().await;
            let config = registry.get(&device_id).unwrap(); // Safe: we just created it
            config.is_plugin_enabled(&plugin_name, &global_config.plugins)
        };

        info!(
            "DBus: Plugin override cleared for {} on device {}, now using global config ({})",
            plugin_name,
            device_id,
            if enabled { "enabled" } else { "disabled" }
        );

        self.emit_plugin_state_changed(&device_id, &plugin_name, enabled)
            .await;

        Ok(())
    }

    /// Reset all plugin overrides for a device (revert to global config)
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    async fn reset_all_plugin_overrides(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: ResetAllPluginOverrides called for {}", device_id);

        // Get list of all plugins that have overrides
        let plugin_names: Vec<String> = {
            let registry = self.device_config_registry.read().await;
            if let Some(config) = registry.get(&device_id) {
                // Collect all plugin names that have overrides
                let mut names = Vec::new();
                if config.plugins.enable_ping.is_some() {
                    names.push("ping".to_string());
                }
                if config.plugins.enable_battery.is_some() {
                    names.push("battery".to_string());
                }
                if config.plugins.enable_notification.is_some() {
                    names.push("notification".to_string());
                }
                if config.plugins.enable_share.is_some() {
                    names.push("share".to_string());
                }
                if config.plugins.enable_clipboard.is_some() {
                    names.push("clipboard".to_string());
                }
                if config.plugins.enable_mpris.is_some() {
                    names.push("mpris".to_string());
                }
                if config.plugins.enable_remotedesktop.is_some() {
                    names.push("remotedesktop".to_string());
                }
                if config.plugins.enable_findmyphone.is_some() {
                    names.push("findmyphone".to_string());
                }
                names
            } else {
                Vec::new()
            }
        };

        // Clear all overrides
        {
            let mut registry = self.device_config_registry.write().await;
            let config = registry.get_or_create(&device_id);

            // Reset all plugin overrides to None
            config.plugins.enable_ping = None;
            config.plugins.enable_battery = None;
            config.plugins.enable_notification = None;
            config.plugins.enable_share = None;
            config.plugins.enable_clipboard = None;
            config.plugins.enable_mpris = None;
            config.plugins.enable_remotedesktop = None;
            config.plugins.enable_findmyphone = None;

            registry.save().map_err(|e| {
                zbus::fdo::Error::Failed(format!("Failed to save device config: {}", e))
            })?;
        }

        // Emit signals for all plugins that had overrides
        let num_affected = plugin_names.len();
        for plugin_name in plugin_names {
            // Get the global config to know what the default state is
            let enabled = {
                let config = self.config.read().await;
                match plugin_name.as_str() {
                    "ping" => config.plugins.enable_ping,
                    "battery" => config.plugins.enable_battery,
                    "notification" => config.plugins.enable_notification,
                    "share" => config.plugins.enable_share,
                    "clipboard" => config.plugins.enable_clipboard,
                    "mpris" => config.plugins.enable_mpris,
                    "remotedesktop" => config.plugins.enable_remotedesktop,
                    "findmyphone" => config.plugins.enable_findmyphone,
                    _ => false,
                }
            };

            self.emit_plugin_state_changed(&device_id, &plugin_name, enabled)
                .await;
        }

        info!(
            "All plugin overrides reset for device {} ({} plugins affected)",
            device_id, num_affected
        );

        Ok(())
    }

    /// Get device configuration as JSON string
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    ///
    /// # Returns
    /// JSON string with device configuration
    async fn get_device_config(&self, device_id: String) -> Result<String, zbus::fdo::Error> {
        debug!("DBus: GetDeviceConfig called for {}", device_id);

        // Use write lock and get_or_create to ensure a default config exists
        let mut registry = self.device_config_registry.write().await;
        let config = registry.get_or_create(&device_id);

        let json = serde_json::to_string_pretty(&config)
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to serialize config: {}", e)))?;

        Ok(json)
    }

    /// Get RemoteDesktop settings for a device as JSON
    ///
    /// Returns the RemoteDesktop-specific settings (quality, fps, resolution)
    /// for the specified device, or defaults if not configured.
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    ///
    /// # Returns
    /// JSON string with RemoteDesktop settings
    async fn get_remotedesktop_settings(
        &self,
        device_id: String,
    ) -> Result<String, zbus::fdo::Error> {
        debug!("DBus: GetRemoteDesktopSettings called for {}", device_id);

        let registry = self.device_config_registry.read().await;
        let config = registry.get(&device_id).ok_or_else(|| {
            zbus::fdo::Error::Failed(format!("No config for device: {}", device_id))
        })?;

        let settings = config.get_remotedesktop_settings();
        let json = serde_json::to_string_pretty(&settings)
            .map_err(|e| zbus::fdo::Error::Failed(format!("Serialization failed: {}", e)))?;

        Ok(json)
    }

    /// Set RemoteDesktop settings for a device
    ///
    /// Updates the RemoteDesktop-specific settings for the specified device.
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `settings_json` - JSON string with RemoteDesktop settings
    async fn set_remotedesktop_settings(
        &self,
        device_id: String,
        settings_json: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!("DBus: SetRemoteDesktopSettings called for {}", device_id);

        let settings: crate::device_config::RemoteDesktopSettings =
            serde_json::from_str(&settings_json)
                .map_err(|e| zbus::fdo::Error::Failed(format!("Invalid settings: {}", e)))?;

        let mut registry = self.device_config_registry.write().await;
        let config = registry.get_or_create(&device_id);
        config.set_remotedesktop_settings(settings);

        registry
            .save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Save failed: {}", e)))?;

        info!("DBus: RemoteDesktop settings updated for {}", device_id);
        Ok(())
    }

    /// Add a run command for a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `command_id` - Unique identifier for the command
    /// * `name` - User-friendly command name
    /// * `command` - Shell command to execute
    async fn add_run_command(
        &self,
        device_id: String,
        command_id: String,
        name: String,
        command: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: AddRunCommand called for {} - ID: {}, Name: {}",
            device_id, command_id, name
        );

        let plugin_manager = self.plugin_manager.read().await;
        let plugin = plugin_manager
            .get_device_plugin(&device_id, "runcommand")
            .ok_or_else(|| {
                zbus::fdo::Error::Failed(format!(
                    "RunCommand plugin not found for device: {}",
                    device_id
                ))
            })?;

        // Downcast to RunCommandPlugin
        use cosmic_connect_protocol::plugins::runcommand::RunCommandPlugin;
        let runcommand_plugin = plugin
            .as_any()
            .downcast_ref::<RunCommandPlugin>()
            .ok_or_else(|| {
                zbus::fdo::Error::Failed("Failed to downcast to RunCommandPlugin".to_string())
            })?;

        // Add the command
        runcommand_plugin
            .add_command(&command_id, &name, &command)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to add command: {}", e)))?;

        info!(
            "DBus: Command '{}' added successfully for device {}",
            command_id, device_id
        );
        Ok(())
    }

    /// Remove a run command from a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `command_id` - Command identifier to remove
    async fn remove_run_command(
        &self,
        device_id: String,
        command_id: String,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: RemoveRunCommand called for {} - ID: {}",
            device_id, command_id
        );

        let plugin_manager = self.plugin_manager.read().await;
        let plugin = plugin_manager
            .get_device_plugin(&device_id, "runcommand")
            .ok_or_else(|| {
                zbus::fdo::Error::Failed(format!(
                    "RunCommand plugin not found for device: {}",
                    device_id
                ))
            })?;

        // Downcast to RunCommandPlugin
        use cosmic_connect_protocol::plugins::runcommand::RunCommandPlugin;
        let runcommand_plugin = plugin
            .as_any()
            .downcast_ref::<RunCommandPlugin>()
            .ok_or_else(|| {
                zbus::fdo::Error::Failed("Failed to downcast to RunCommandPlugin".to_string())
            })?;

        // Remove the command
        runcommand_plugin
            .remove_command(&command_id)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to remove command: {}", e)))?;

        info!(
            "DBus: Command '{}' removed successfully from device {}",
            command_id, device_id
        );
        Ok(())
    }

    /// Get all run commands for a device as JSON
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    ///
    /// # Returns
    /// JSON string with command map {id: {name: string, command: string}}
    async fn get_run_commands(&self, device_id: String) -> Result<String, zbus::fdo::Error> {
        debug!("DBus: GetRunCommands called for {}", device_id);

        let plugin_manager = self.plugin_manager.read().await;
        let plugin = plugin_manager
            .get_device_plugin(&device_id, "runcommand")
            .ok_or_else(|| {
                zbus::fdo::Error::Failed(format!(
                    "RunCommand plugin not found for device: {}",
                    device_id
                ))
            })?;

        // Downcast to RunCommandPlugin
        use cosmic_connect_protocol::plugins::runcommand::RunCommandPlugin;
        let runcommand_plugin = plugin
            .as_any()
            .downcast_ref::<RunCommandPlugin>()
            .ok_or_else(|| {
                zbus::fdo::Error::Failed("Failed to downcast to RunCommandPlugin".to_string())
            })?;

        // Get all commands
        let commands = runcommand_plugin.get_commands().await;

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&commands).map_err(|e| {
            zbus::fdo::Error::Failed(format!("Failed to serialize commands: {}", e))
        })?;

        debug!(
            "DBus: Retrieved {} commands for device {}",
            commands.len(),
            device_id
        );
        Ok(json)
    }

    /// Clear all run commands for a device
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    async fn clear_run_commands(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: ClearRunCommands called for {}", device_id);

        let plugin_manager = self.plugin_manager.read().await;
        let plugin = plugin_manager
            .get_device_plugin(&device_id, "runcommand")
            .ok_or_else(|| {
                zbus::fdo::Error::Failed(format!(
                    "RunCommand plugin not found for device: {}",
                    device_id
                ))
            })?;

        // Downcast to RunCommandPlugin
        use cosmic_connect_protocol::plugins::runcommand::RunCommandPlugin;
        let runcommand_plugin = plugin
            .as_any()
            .downcast_ref::<RunCommandPlugin>()
            .ok_or_else(|| {
                zbus::fdo::Error::Failed("Failed to downcast to RunCommandPlugin".to_string())
            })?;

        // Clear all commands
        runcommand_plugin
            .clear_commands()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to clear commands: {}", e)))?;

        info!("DBus: All commands cleared for device {}", device_id);
        Ok(())
    }

    /// Start screen share session
    ///
    /// Configures the ScreenShare plugin with the local port to receive the stream.
    /// This triggers sending the ready packet to the remote device.
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `port` - The local TCP port where the stream receiver is listening
    async fn start_screen_share(
        &self,
        device_id: String,
        port: u16,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: StartScreenShare called for {} on port {}",
            device_id, port
        );

        let mut plugin_manager = self.plugin_manager.write().await;

        if let Some(plugin) = plugin_manager.get_device_plugin_mut(&device_id, "screenshare") {
            use cosmic_connect_protocol::plugins::screenshare::ScreenSharePlugin;

            if let Some(screenshare) = plugin.as_any_mut().downcast_mut::<ScreenSharePlugin>() {
                screenshare.set_local_port(port).await.map_err(|e| {
                    zbus::fdo::Error::Failed(format!("Failed to set local port: {}", e))
                })?;

                info!("Screen share configured for device {}", device_id);
                Ok(())
            } else {
                Err(zbus::fdo::Error::Failed(
                    "Plugin is not ScreenSharePlugin".to_string(),
                ))
            }
        } else {
            Err(zbus::fdo::Error::Failed(
                "ScreenShare plugin not found".to_string(),
            ))
        }
    }

    /// Share screen to a remote device
    ///
    /// Initiates screen sharing from this device to the specified remote device.
    /// This starts the local capture and sends a start packet to the remote.
    ///
    /// # Arguments
    /// * `device_id` - The device ID to share screen to
    async fn share_screen_to(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: ShareScreenTo called for {}", device_id);

        // Check device is connected
        let device_manager = self.device_manager.read().await;
        if !device_manager
            .get_device(&device_id)
            .map(|d| d.is_connected())
            .unwrap_or(false)
        {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }
        drop(device_manager);

        let mut plugin_manager = self.plugin_manager.write().await;

        if let Some(plugin) = plugin_manager.get_device_plugin_mut(&device_id, "screenshare") {
            use cosmic_connect_protocol::plugins::screenshare::{ScreenSharePlugin, ShareConfig};

            if let Some(screenshare) = plugin.as_any_mut().downcast_mut::<ScreenSharePlugin>() {
                // Use default config for now (30fps, 2Mbps, H264)
                let config = ShareConfig::default();

                screenshare.share_to_device(config).await.map_err(|e| {
                    zbus::fdo::Error::Failed(format!("Failed to start screen share: {}", e))
                })?;

                info!("Screen share initiated to device {}", device_id);
                Ok(())
            } else {
                Err(zbus::fdo::Error::Failed(
                    "Plugin is not ScreenSharePlugin".to_string(),
                ))
            }
        } else {
            Err(zbus::fdo::Error::Failed(
                "ScreenShare plugin not found".to_string(),
            ))
        }
    }

    /// Stop screen share session
    ///
    /// Stops an active screen share session for the specified device.
    ///
    /// # Arguments
    /// * `device_id` - The device ID to stop sharing with
    async fn stop_screen_share(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: StopScreenShare called for {}", device_id);

        let mut plugin_manager = self.plugin_manager.write().await;

        if let Some(plugin) = plugin_manager.get_device_plugin_mut(&device_id, "screenshare") {
            use cosmic_connect_protocol::plugins::screenshare::ScreenSharePlugin;

            if let Some(screenshare) = plugin.as_any_mut().downcast_mut::<ScreenSharePlugin>() {
                screenshare.stop_sharing().await.map_err(|e| {
                    zbus::fdo::Error::Failed(format!("Failed to stop screen share: {}", e))
                })?;

                info!("Screen share stopped for device {}", device_id);
                Ok(())
            } else {
                Err(zbus::fdo::Error::Failed(
                    "Plugin is not ScreenSharePlugin".to_string(),
                ))
            }
        } else {
            Err(zbus::fdo::Error::Failed(
                "ScreenShare plugin not found".to_string(),
            ))
        }
    }

    /// Pause screen share session
    ///
    /// Pauses an active screen share session. The session remains active but
    /// no new frames are captured. Viewers will see a frozen frame.
    ///
    /// # Arguments
    /// * `device_id` - The device ID to pause sharing with
    async fn pause_screen_share(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: PauseScreenShare called for {}", device_id);

        let mut plugin_manager = self.plugin_manager.write().await;

        if let Some(plugin) = plugin_manager.get_device_plugin_mut(&device_id, "screenshare") {
            use cosmic_connect_protocol::plugins::screenshare::ScreenSharePlugin;

            if let Some(screenshare) = plugin.as_any_mut().downcast_mut::<ScreenSharePlugin>() {
                screenshare.pause_sharing().await.map_err(|e| {
                    zbus::fdo::Error::Failed(format!("Failed to pause screen share: {}", e))
                })?;

                info!("Screen share paused for device {}", device_id);
                Ok(())
            } else {
                Err(zbus::fdo::Error::Failed(
                    "Plugin is not ScreenSharePlugin".to_string(),
                ))
            }
        } else {
            Err(zbus::fdo::Error::Failed(
                "ScreenShare plugin not found".to_string(),
            ))
        }
    }

    /// Resume screen share session
    ///
    /// Resumes a paused screen share session.
    ///
    /// # Arguments
    /// * `device_id` - The device ID to resume sharing with
    async fn resume_screen_share(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: ResumeScreenShare called for {}", device_id);

        let mut plugin_manager = self.plugin_manager.write().await;

        if let Some(plugin) = plugin_manager.get_device_plugin_mut(&device_id, "screenshare") {
            use cosmic_connect_protocol::plugins::screenshare::ScreenSharePlugin;

            if let Some(screenshare) = plugin.as_any_mut().downcast_mut::<ScreenSharePlugin>() {
                screenshare.resume_sharing().await.map_err(|e| {
                    zbus::fdo::Error::Failed(format!("Failed to resume screen share: {}", e))
                })?;

                info!("Screen share resumed for device {}", device_id);
                Ok(())
            } else {
                Err(zbus::fdo::Error::Failed(
                    "Plugin is not ScreenSharePlugin".to_string(),
                ))
            }
        } else {
            Err(zbus::fdo::Error::Failed(
                "ScreenShare plugin not found".to_string(),
            ))
        }
    }

    /// Send input event for screen mirroring
    async fn send_mirror_input(
        &self,
        device_id: String,
        x: f32,
        y: f32,
        action: String,
    ) -> Result<(), zbus::fdo::Error> {
        debug!(
            "DBus: SendMirrorInput called for {} (x: {}, y: {}, action: {})",
            device_id, x, y, action
        );

        let device_manager = self.device_manager.read().await;
        if !device_manager
            .get_device(&device_id)
            .map(|d| d.is_connected())
            .unwrap_or(false)
        {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }
        drop(device_manager);

        use cosmic_connect_protocol::Packet;
        let body = serde_json::json!({
            "x": x,
            "y": y,
            "action": action
        });
        let packet = Packet::new("cconnect.screenshare.input", body);

        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send input packet: {}", e)))?;

        Ok(())
    }

    /// Get daemon performance metrics
    ///
    /// Returns performance metrics if metrics collection is enabled.
    /// Returns an error if metrics are disabled (use --metrics flag to enable).
    async fn get_metrics(&self) -> Result<DaemonMetrics, zbus::fdo::Error> {
        let metrics = self.metrics.as_ref().ok_or_else(|| {
            zbus::fdo::Error::Failed(
                "Metrics not enabled. Start daemon with --metrics flag to enable".to_string(),
            )
        })?;

        let m = metrics.read().await;
        Ok(DaemonMetrics {
            uptime_seconds: m.uptime_seconds(),
            packets_sent: m.packets_sent(),
            packets_received: m.packets_received(),
            bytes_sent: m.bytes_sent(),
            bytes_received: m.bytes_received(),
            active_connections: m.active_connections() as u32,
            paired_devices: m.paired_devices() as u32,
            plugin_invocations: m.plugin_invocations(),
            plugin_errors: m.plugin_errors(),
            packets_per_second: m.packets_per_second(),
            bandwidth_bps: m.bandwidth_bps(),
        })
    }

    /// Get list of available MPRIS media players
    ///
    /// Returns list of player names that can be controlled.
    async fn get_mpris_players(&self) -> Result<Vec<String>, zbus::fdo::Error> {
        debug!("DBus: GetMprisPlayers called");

        let Some(mpris_manager) = &self.mpris_manager else {
            return Ok(Vec::new());
        };

        let players = mpris_manager.get_player_list().await;
        info!("DBus: Found {} MPRIS players", players.len());
        Ok(players)
    }

    /// Get detailed state for a specific MPRIS player
    ///
    /// Returns the full state including metadata (title, artist, album art)
    /// and playback status.
    ///
    /// # Arguments
    /// * `player` - Player name (as returned by GetMprisPlayers)
    ///
    /// # Returns
    /// JSON string of PlayerState
    async fn get_player_state(&self, player: String) -> Result<String, zbus::fdo::Error> {
        debug!("DBus: GetPlayerState called for {}", player);

        let Some(mpris_manager) = &self.mpris_manager else {
            return Err(zbus::fdo::Error::Failed(
                "MPRIS manager not available".to_string(),
            ));
        };

        let state = mpris_manager
            .get_player_state(&player)
            .await
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("Player not found: {}", player)))?;

        let json = serde_json::to_string_pretty(&state).map_err(|e| {
            zbus::fdo::Error::Failed(format!("Failed to serialize player state: {}", e))
        })?;

        Ok(json)
    }

    /// Control MPRIS player playback
    ///
    /// # Arguments
    /// * `player` - Player name (e.g., "spotify", "vlc")
    /// * `action` - Action: "Play", "Pause", "PlayPause", "Stop", "Next", "Previous"
    async fn mpris_control(&self, player: String, action: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: MprisControl called: {} - {}", player, action);

        let Some(mpris_manager) = &self.mpris_manager else {
            return Err(zbus::fdo::Error::Failed(
                "MPRIS manager not available".to_string(),
            ));
        };

        mpris_manager
            .call_player_method(&player, &action)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("MPRIS control failed: {}", e)))?;

        info!("DBus: MPRIS control {} executed for {}", action, player);
        Ok(())
    }

    /// Set MPRIS player volume
    ///
    /// # Arguments
    /// * `player` - Player name
    /// * `volume` - Volume level (0.0 to 1.0)
    async fn mpris_set_volume(&self, player: String, volume: f64) -> Result<(), zbus::fdo::Error> {
        info!("DBus: MprisSetVolume called: {} - {}", player, volume);

        let Some(mpris_manager) = &self.mpris_manager else {
            return Err(zbus::fdo::Error::Failed(
                "MPRIS manager not available".to_string(),
            ));
        };

        let clamped_volume = volume.clamp(0.0, 1.0);
        mpris_manager
            .set_volume(&player, clamped_volume)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to set volume: {}", e)))?;

        info!("DBus: Volume set to {} for {}", clamped_volume, player);
        Ok(())
    }

    /// Seek MPRIS player position
    ///
    /// # Arguments
    /// * `player` - Player name
    /// * `offset_microseconds` - Seek offset in microseconds (can be negative)
    async fn mpris_seek(
        &self,
        player: String,
        offset_microseconds: i64,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: MprisSeek called: {} - {}μs",
            player, offset_microseconds
        );

        let Some(mpris_manager) = &self.mpris_manager else {
            return Err(zbus::fdo::Error::Failed(
                "MPRIS manager not available".to_string(),
            ));
        };

        mpris_manager
            .seek(&player, offset_microseconds)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Seek failed: {}", e)))?;

        info!("DBus: Seek executed for {}", player);
        Ok(())
    }

    /// Raise MPRIS player window (bring to front)
    ///
    /// # Arguments
    /// * `player` - Player name
    async fn mpris_raise(&self, player: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: MprisRaise called: {}", player);

        let Some(mpris_manager) = &self.mpris_manager else {
            return Err(zbus::fdo::Error::Failed(
                "MPRIS manager not available".to_string(),
            ));
        };

        mpris_manager
            .raise(&player)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Raise failed: {}", e)))?;

        info!("DBus: Raise executed for {}", player);
        Ok(())
    }

    // ===== Settings Management Methods =====

    /// Get daemon configuration as JSON
    ///
    /// Returns the complete daemon configuration including device, network,
    /// transport, and plugin settings.
    ///
    /// # Returns
    /// JSON-serialized configuration
    async fn get_daemon_config(&self) -> Result<String, zbus::fdo::Error> {
        debug!("DBus: GetDaemonConfig called");

        let config = self.config.read().await;
        let json = serde_json::to_string_pretty(&*config)
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to serialize config: {}", e)))?;

        Ok(json)
    }

    /// Set device name
    ///
    /// # Arguments
    /// * `name` - New device name
    async fn set_device_name(&self, name: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: SetDeviceName called: {}", name);

        let mut config = self.config.write().await;
        config.device.name = name;

        config
            .save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to save config: {}", e)))?;

        info!("DBus: Device name updated successfully");
        Ok(())
    }

    /// Set device type
    ///
    /// # Arguments
    /// * `device_type` - Device type ("desktop", "laptop", "phone", "tablet", "tv")
    async fn set_device_type(&self, device_type: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: SetDeviceType called: {}", device_type);

        // Validate device type
        if !["desktop", "laptop", "phone", "tablet", "tv"].contains(&device_type.as_str()) {
            return Err(zbus::fdo::Error::Failed(format!(
                "Invalid device type: {}. Must be desktop, laptop, phone, tablet, or tv",
                device_type
            )));
        }

        let mut config = self.config.write().await;
        config.device.device_type = device_type;

        config
            .save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to save config: {}", e)))?;

        info!("DBus: Device type updated successfully");
        Ok(())
    }

    /// Get global plugin status
    ///
    /// Returns a map of plugin names to their enabled status.
    ///
    /// # Returns
    /// HashMap<plugin_name, enabled>
    async fn get_global_plugin_status(&self) -> HashMap<String, bool> {
        debug!("DBus: GetGlobalPluginStatus called");

        let config = self.config.read().await;
        let mut status = HashMap::new();

        status.insert("ping".to_string(), config.plugins.enable_ping);
        status.insert("battery".to_string(), config.plugins.enable_battery);
        status.insert(
            "notification".to_string(),
            config.plugins.enable_notification,
        );
        status.insert("share".to_string(), config.plugins.enable_share);
        status.insert("clipboard".to_string(), config.plugins.enable_clipboard);
        status.insert("mpris".to_string(), config.plugins.enable_mpris);
        status.insert("runcommand".to_string(), config.plugins.enable_runcommand);
        status.insert("remoteinput".to_string(), config.plugins.enable_remoteinput);
        status.insert("findmyphone".to_string(), config.plugins.enable_findmyphone);
        status.insert("telephony".to_string(), config.plugins.enable_telephony);
        status.insert("presenter".to_string(), config.plugins.enable_presenter);
        status.insert("contacts".to_string(), config.plugins.enable_contacts);

        status
    }

    /// Set global plugin enabled state
    ///
    /// Enable or disable a plugin globally. This affects all devices unless
    /// overridden per-device.
    ///
    /// # Arguments
    /// * `plugin` - Plugin name
    /// * `enabled` - Whether to enable the plugin
    async fn set_global_plugin_enabled(
        &self,
        plugin: String,
        enabled: bool,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: SetGlobalPluginEnabled called: {} = {}",
            plugin, enabled
        );

        let mut config = self.config.write().await;

        // Update the specific plugin flag
        match plugin.as_str() {
            "ping" => config.plugins.enable_ping = enabled,
            "battery" => config.plugins.enable_battery = enabled,
            "notification" => config.plugins.enable_notification = enabled,
            "share" => config.plugins.enable_share = enabled,
            "clipboard" => config.plugins.enable_clipboard = enabled,
            "mpris" => config.plugins.enable_mpris = enabled,
            "runcommand" => config.plugins.enable_runcommand = enabled,
            "remoteinput" => config.plugins.enable_remoteinput = enabled,
            "findmyphone" => config.plugins.enable_findmyphone = enabled,
            "telephony" => config.plugins.enable_telephony = enabled,
            "presenter" => config.plugins.enable_presenter = enabled,
            "contacts" => config.plugins.enable_contacts = enabled,
            _ => {
                return Err(zbus::fdo::Error::Failed(format!(
                    "Unknown plugin: {}",
                    plugin
                )))
            }
        }

        config
            .save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to save config: {}", e)))?;

        info!(
            "DBus: Plugin {} {} globally",
            plugin,
            if enabled { "enabled" } else { "disabled" }
        );
        Ok(())
    }

    /// Set TCP transport enabled
    ///
    /// # Arguments
    /// * `enabled` - Whether TCP transport should be enabled
    async fn set_tcp_enabled(&self, enabled: bool) -> Result<(), zbus::fdo::Error> {
        info!("DBus: SetTcpEnabled called: {}", enabled);

        let mut config = self.config.write().await;
        config.transport.enable_tcp = enabled;

        config
            .save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to save config: {}", e)))?;

        info!(
            "DBus: TCP transport {} (restart required)",
            if enabled { "enabled" } else { "disabled" }
        );
        Ok(())
    }

    /// Set Bluetooth transport enabled
    ///
    /// # Arguments
    /// * `enabled` - Whether Bluetooth transport should be enabled
    async fn set_bluetooth_enabled(&self, enabled: bool) -> Result<(), zbus::fdo::Error> {
        info!("DBus: SetBluetoothEnabled called: {}", enabled);

        let mut config = self.config.write().await;
        config.transport.enable_bluetooth = enabled;

        config
            .save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to save config: {}", e)))?;

        info!(
            "DBus: Bluetooth transport {} (restart required)",
            if enabled { "enabled" } else { "disabled" }
        );
        Ok(())
    }

    /// Set transport preference
    ///
    /// # Arguments
    /// * `preference` - Transport preference: "prefer_tcp", "prefer_bluetooth",
    ///                  "tcp_first", "bluetooth_first", "only_tcp", "only_bluetooth"
    async fn set_transport_preference(&self, preference: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: SetTransportPreference called: {}", preference);

        // Validate and convert preference
        use crate::config::TransportPreferenceConfig;
        let pref_config = match preference.as_str() {
            "prefer_tcp" => TransportPreferenceConfig::PreferTcp,
            "prefer_bluetooth" => TransportPreferenceConfig::PreferBluetooth,
            "tcp_first" => TransportPreferenceConfig::TcpFirst,
            "bluetooth_first" => TransportPreferenceConfig::BluetoothFirst,
            "only_tcp" => TransportPreferenceConfig::OnlyTcp,
            "only_bluetooth" => TransportPreferenceConfig::OnlyBluetooth,
            _ => {
                return Err(zbus::fdo::Error::Failed(format!(
                    "Invalid transport preference: {}. Must be prefer_tcp, prefer_bluetooth, tcp_first, bluetooth_first, only_tcp, or only_bluetooth",
                    preference
                )))
            }
        };

        let mut config = self.config.write().await;
        config.transport.preference = pref_config;

        config
            .save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to save config: {}", e)))?;

        info!(
            "DBus: Transport preference set to {} (restart required)",
            preference
        );
        Ok(())
    }

    /// Set auto fallback enabled
    ///
    /// When enabled, automatically tries alternative transport if primary fails.
    ///
    /// # Arguments
    /// * `enabled` - Whether auto fallback should be enabled
    async fn set_auto_fallback(&self, enabled: bool) -> Result<(), zbus::fdo::Error> {
        info!("DBus: SetAutoFallback called: {}", enabled);

        let mut config = self.config.write().await;
        config.transport.auto_fallback = enabled;

        config
            .save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to save config: {}", e)))?;

        info!(
            "DBus: Auto fallback {} (restart required)",
            if enabled { "enabled" } else { "disabled" }
        );
        Ok(())
    }

    /// Get discovery configuration
    ///
    /// Returns discovery-related network configuration as JSON.
    ///
    /// # Returns
    /// JSON string containing discovery_interval and device_timeout
    async fn get_discovery_config(&self) -> Result<String, zbus::fdo::Error> {
        debug!("DBus: GetDiscoveryConfig called");

        let config = self.config.read().await;
        let discovery_config = serde_json::json!({
            "discovery_interval": config.network.discovery_interval,
            "device_timeout": config.network.device_timeout,
            "discovery_port": config.network.discovery_port,
        });

        let json = serde_json::to_string_pretty(&discovery_config).map_err(|e| {
            zbus::fdo::Error::Failed(format!("Failed to serialize discovery config: {}", e))
        })?;

        Ok(json)
    }

    /// Set discovery interval
    ///
    /// # Arguments
    /// * `interval_secs` - Discovery broadcast interval in seconds (recommended: 3-30)
    async fn set_discovery_interval(&self, interval_secs: u64) -> Result<(), zbus::fdo::Error> {
        info!("DBus: SetDiscoveryInterval called: {}", interval_secs);

        // Validate interval (between 1 and 60 seconds)
        if interval_secs == 0 || interval_secs > 60 {
            return Err(zbus::fdo::Error::Failed(format!(
                "Invalid discovery interval: {}. Must be between 1 and 60 seconds",
                interval_secs
            )));
        }

        let mut config = self.config.write().await;
        config.network.discovery_interval = interval_secs;

        config
            .save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to save config: {}", e)))?;

        info!(
            "DBus: Discovery interval set to {} seconds (restart required)",
            interval_secs
        );
        Ok(())
    }

    /// Set device timeout
    ///
    /// # Arguments
    /// * `timeout_secs` - Device timeout in seconds (recommended: 10-120)
    async fn set_device_timeout(&self, timeout_secs: u64) -> Result<(), zbus::fdo::Error> {
        info!("DBus: SetDeviceTimeout called: {}", timeout_secs);

        // Validate timeout (between 5 and 300 seconds)
        if !(5..=300).contains(&timeout_secs) {
            return Err(zbus::fdo::Error::Failed(format!(
                "Invalid device timeout: {}. Must be between 5 and 300 seconds",
                timeout_secs
            )));
        }

        let mut config = self.config.write().await;
        config.network.device_timeout = timeout_secs;

        config
            .save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to save config: {}", e)))?;

        info!(
            "DBus: Device timeout set to {} seconds (restart required)",
            timeout_secs
        );
        Ok(())
    }

    /// Reset configuration to defaults
    ///
    /// Resets all daemon configuration to default values and saves to disk.
    /// Preserves only the device ID to maintain identity across reset.
    async fn reset_config_to_defaults(&self) -> Result<(), zbus::fdo::Error> {
        warn!("DBus: ResetConfigToDefaults called - resetting all configuration");

        let mut config = self.config.write().await;

        // Preserve device ID (should not change on config reset)
        let device_id = config.device.device_id.clone();

        // Reset to defaults
        *config = crate::config::Config::default();

        // Restore device ID
        config.device.device_id = device_id;

        config
            .save()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to save config: {}", e)))?;

        warn!("DBus: Configuration reset to defaults (restart required for full effect)");
        Ok(())
    }

    /// Restart daemon
    ///
    /// Initiates a graceful restart of the daemon. The daemon will:
    /// 1. Close all active connections
    /// 2. Save current state
    /// 3. Exit with code 0 (systemd will auto-restart if configured)
    ///
    /// Note: This method returns immediately. The actual restart happens
    /// asynchronously after a brief delay.
    async fn restart_daemon(&self) -> Result<(), zbus::fdo::Error> {
        warn!("DBus: RestartDaemon called - initiating graceful restart");

        // Spawn a task to restart after a brief delay (to allow this method to return)
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            warn!("Restarting daemon...");
            std::process::exit(0);
        });

        Ok(())
    }

    /// Start audio streaming from device
    ///
    /// # Arguments
    /// * `device_id` - The device ID to stream audio from
    async fn start_audio_stream(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: StartAudioStream called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        if !device_manager
            .get_device(&device_id)
            .map(|d| d.is_connected())
            .unwrap_or(false)
        {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }
        drop(device_manager);

        use cosmic_connect_protocol::Packet;
        let body = serde_json::json!({});
        let packet = Packet::new("cconnect.audiostream.start", body);

        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send packet: {}", e)))?;

        info!("Audio stream start request sent to device {}", device_id);
        Ok(())
    }

    /// Stop audio streaming
    ///
    /// # Arguments
    /// * `device_id` - The device ID to stop streaming from
    async fn stop_audio_stream(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: StopAudioStream called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        if !device_manager
            .get_device(&device_id)
            .map(|d| d.is_connected())
            .unwrap_or(false)
        {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }
        drop(device_manager);

        use cosmic_connect_protocol::Packet;
        let body = serde_json::json!({});
        let packet = Packet::new("cconnect.audiostream.stop", body);

        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send packet: {}", e)))?;

        info!("Audio stream stop request sent to device {}", device_id);
        Ok(())
    }

    /// Start presenter mode (use phone as presentation remote)
    ///
    /// # Arguments
    /// * `device_id` - The device ID to use as presenter
    async fn start_presenter(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: StartPresenter called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        if !device_manager
            .get_device(&device_id)
            .map(|d| d.is_connected())
            .unwrap_or(false)
        {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }
        drop(device_manager);

        use cosmic_connect_protocol::Packet;
        let body = serde_json::json!({});
        let packet = Packet::new("cconnect.presenter.start", body);

        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send packet: {}", e)))?;

        info!("Presenter mode start request sent to device {}", device_id);
        Ok(())
    }

    /// Stop presenter mode
    ///
    /// # Arguments
    /// * `device_id` - The device ID to stop presenter on
    async fn stop_presenter(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: StopPresenter called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        if !device_manager
            .get_device(&device_id)
            .map(|d| d.is_connected())
            .unwrap_or(false)
        {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }
        drop(device_manager);

        use cosmic_connect_protocol::Packet;
        let body = serde_json::json!({});
        let packet = Packet::new("cconnect.presenter.stop", body);

        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send packet: {}", e)))?;

        info!("Presenter mode stop request sent to device {}", device_id);
        Ok(())
    }

    /// Start camera streaming from Android device to V4L2 loopback
    ///
    /// Requests the phone to start streaming camera video which will be routed
    /// to a V4L2 loopback device for use as a webcam in video conferencing apps.
    ///
    /// # Arguments
    /// * `device_id` - The device ID to stream camera from
    /// * `camera_id` - Camera ID (0 = back, 1 = front typically)
    /// * `width` - Video width in pixels (e.g., 1280)
    /// * `height` - Video height in pixels (e.g., 720)
    /// * `fps` - Frame rate (e.g., 30)
    /// * `bitrate` - Bitrate in kbps (e.g., 2000)
    async fn start_camera(
        &self,
        device_id: String,
        camera_id: u32,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    ) -> Result<(), zbus::fdo::Error> {
        info!(
            "DBus: StartCamera called for {} (camera_id={}, {}x{} @ {}fps, {}kbps)",
            device_id, camera_id, width, height, fps, bitrate
        );

        let device_manager = self.device_manager.read().await;
        if !device_manager
            .get_device(&device_id)
            .map(|d| d.is_connected())
            .unwrap_or(false)
        {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }
        drop(device_manager);

        use cosmic_connect_protocol::Packet;
        let body = serde_json::json!({
            "cameraId": camera_id,
            "resolution": {
                "width": width,
                "height": height,
            },
            "fps": fps,
            "bitrate": bitrate,
            "codec": "h264",
        });
        let packet = Packet::new("cconnect.camera.start", body);

        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send packet: {}", e)))?;

        info!("Camera stream start request sent to device {}", device_id);
        Ok(())
    }

    /// Stop camera streaming
    ///
    /// # Arguments
    /// * `device_id` - The device ID to stop camera streaming on
    async fn stop_camera(&self, device_id: String) -> Result<(), zbus::fdo::Error> {
        info!("DBus: StopCamera called for {}", device_id);

        let device_manager = self.device_manager.read().await;
        if !device_manager
            .get_device(&device_id)
            .map(|d| d.is_connected())
            .unwrap_or(false)
        {
            return Err(zbus::fdo::Error::Failed("Device not connected".to_string()));
        }
        drop(device_manager);

        use cosmic_connect_protocol::Packet;
        let body = serde_json::json!({});
        let packet = Packet::new("cconnect.camera.stop", body);

        let conn_manager = self.connection_manager.read().await;
        conn_manager
            .send_packet(&device_id, &packet)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to send packet: {}", e)))?;

        info!("Camera stream stop request sent to device {}", device_id);
        Ok(())
    }

    /// Get camera status
    ///
    /// Returns the current camera streaming status from the camera plugin.
    ///
    /// # Arguments
    /// * `device_id` - The device ID to query camera status for
    ///
    /// # Returns
    /// JSON string with camera status: {status, cameraId, resolution, fps, bitrate, error}
    async fn get_camera_status(&self, device_id: String) -> Result<String, zbus::fdo::Error> {
        debug!("DBus: GetCameraStatus called for {}", device_id);

        let plugin_manager = self.plugin_manager.read().await;
        let camera_plugin = plugin_manager
            .get_device_plugin(&device_id, "camera")
            .ok_or_else(|| {
                zbus::fdo::Error::Failed(format!(
                    "Camera plugin not found for device {}",
                    device_id
                ))
            })?;

        let camera = camera_plugin
            .as_any()
            .downcast_ref::<cosmic_connect_protocol::plugins::camera::CameraPlugin>()
            .ok_or_else(|| {
                zbus::fdo::Error::Failed("Failed to downcast to CameraPlugin".to_string())
            })?;

        let is_streaming = camera.is_streaming().await;
        if let Some(session) = camera.get_session().await {
            let status = serde_json::json!({
                "streaming": is_streaming,
                "cameraId": session.camera_id,
                "resolution": session.resolution,
                "fps": session.fps,
                "quality": session.quality.as_str(),
                "frameNumber": session.frame_number,
                "startedAt": session.started_at,
            });
            serde_json::to_string(&status).map_err(|e| {
                zbus::fdo::Error::Failed(format!("Failed to serialize camera status: {}", e))
            })
        } else {
            // Return stopped status if no active session
            Ok(r#"{"streaming":false}"#.to_string())
        }
    }

    /// Signal: Device was added (discovered)
    ///
    /// Emitted when a new device is discovered on the network.
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `device_info` - Device information
    #[zbus(signal)]
    async fn device_added(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
        device_info: DeviceInfo,
    ) -> zbus::Result<()>;

    /// Signal: Device was removed (disappeared)
    ///
    /// Emitted when a device is no longer reachable on the network.
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    #[zbus(signal)]
    async fn device_removed(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
    ) -> zbus::Result<()>;

    /// Signal: Device state changed
    ///
    /// Emitted when a device's connection state changes (connected, paired, etc).
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `state` - New state: "connected", "paired", "reachable", or "unknown"
    #[zbus(signal)]
    async fn device_state_changed(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
        state: &str,
    ) -> zbus::Result<()>;

    /// Signal: Pairing request received
    ///
    /// Emitted when a device requests to pair with us.
    ///
    /// # Arguments
    /// * `device_id` - The device ID requesting pairing
    #[zbus(signal)]
    async fn pairing_request(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
    ) -> zbus::Result<()>;

    /// Signal: Pairing status changed
    ///
    /// Emitted when pairing completes or fails.
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `status` - Status: "paired", "rejected", or "failed"
    /// Signal: Messaging notification received
    ///
    /// Emitted when a messaging app notification arrives.
    #[zbus(signal)]
    async fn messaging_notification(
        signal_emitter: &SignalEmitter<'_>,
        messenger: &str,
        sender: &str,
        message: &str,
        conversation_id: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn pairing_status_changed(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
        status: &str,
    ) -> zbus::Result<()>;

    /// Signal: Plugin event
    ///
    /// Emitted when a plugin receives data or has something to notify about.
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `plugin` - Plugin name (e.g., "battery", "ping", "share")
    /// * `data` - Plugin-specific JSON data
    #[zbus(signal)]
    async fn plugin_event(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
        plugin: &str,
        data: &str,
    ) -> zbus::Result<()>;

    /// Signal: Device plugin state changed
    ///
    /// Emitted when a plugin is enabled or disabled for a specific device.
    /// Allows UI clients to update in real-time without polling or daemon restart.
    ///
    /// # Arguments
    /// * `device_id` - The device ID
    /// * `plugin_name` - Plugin name (e.g., "battery", "remotedesktop")
    /// * `enabled` - Whether the plugin is now enabled
    #[zbus(signal)]
    async fn device_plugin_state_changed(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
        plugin_name: &str,
        enabled: bool,
    ) -> zbus::Result<()>;

    /// Signal: File transfer progress
    ///
    /// Emitted during file transfers to report progress.
    ///
    /// # Arguments
    /// * `transfer_id` - Unique transfer ID
    /// * `device_id` - The device ID
    /// * `filename` - Name of the file being transferred
    /// * `bytes_transferred` - Bytes transferred so far
    /// * `total_bytes` - Total file size in bytes
    /// * `direction` - "sending" or "receiving"
    #[zbus(signal)]
    async fn transfer_progress(
        signal_emitter: &SignalEmitter<'_>,
        transfer_id: &str,
        device_id: &str,
        filename: &str,
        bytes_transferred: u64,
        total_bytes: u64,
        direction: &str,
    ) -> zbus::Result<()>;

    /// Signal: Transfer complete or cancelled
    ///
    /// Emitted when a file transfer finishes (successfully or not).
    ///
    /// # Arguments
    /// * `transfer_id` - Unique transfer ID
    /// * `device_id` - The device ID
    /// * `filename` - Name of the file
    /// * `success` - Whether transfer completed successfully
    /// * `error_message` - Error message if failed (empty if successful)
    #[zbus(signal)]
    async fn transfer_complete(
        signal_emitter: &SignalEmitter<'_>,
        transfer_id: &str,
        device_id: &str,
        filename: &str,
        success: bool,
        error_message: &str,
    ) -> zbus::Result<()>;

    /// Signal: Screen share requested
    ///
    /// Emitted when a remote device requests to share its screen.
    #[zbus(signal)]
    async fn screen_share_requested(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
    ) -> zbus::Result<()>;

    /// Signal: Screen share started
    ///
    /// Emitted when screen share streaming begins (either direction).
    #[zbus(signal)]
    async fn screen_share_started(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
        is_sender: bool,
    ) -> zbus::Result<()>;

    /// Signal: Screen share stopped
    ///
    /// Emitted when screen share streaming ends.
    #[zbus(signal)]
    async fn screen_share_stopped(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
    ) -> zbus::Result<()>;

    /// Signal: Screen share cursor update
    ///
    /// Emitted when presenter's cursor position changes.
    /// UI can use this to show a highlight effect around the cursor.
    #[zbus(signal)]
    async fn screen_share_cursor_update(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
        x: i32,
        y: i32,
        visible: bool,
    ) -> zbus::Result<()>;

    /// Signal: Screen share annotation
    ///
    /// Emitted when presenter draws an annotation on the shared screen.
    /// UI can use this to render drawing overlays.
    #[zbus(signal)]
    async fn screen_share_annotation(
        signal_emitter: &SignalEmitter<'_>,
        device_id: &str,
        annotation_type: &str,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        color: &str,
        width: u8,
    ) -> zbus::Result<()>;
}

/// DBus server for the daemon
pub struct DbusServer {
    /// DBus connection
    connection: Connection,
}

impl DbusServer {
    /// Create and start a new DBus server
    ///
    /// # Arguments
    /// * `device_manager` - Device manager reference
    /// * `plugin_manager` - Plugin manager reference
    /// * `connection_manager` - Connection manager reference
    /// * `device_config_registry` - Device configuration registry
    /// * `pairing_service` - Optional pairing service reference
    /// * `mpris_manager` - Optional MPRIS manager for local media player control
    /// * `config` - Daemon configuration (for settings management)
    ///
    /// # Returns
    /// DBus server instance with active connection
    pub async fn start(
        device_manager: Arc<RwLock<DeviceManager>>,
        plugin_manager: Arc<RwLock<PluginManager>>,
        connection_manager: Arc<RwLock<ConnectionManager>>,
        device_config_registry: Arc<RwLock<crate::device_config::DeviceConfigRegistry>>,
        pairing_service: Option<Arc<RwLock<cosmic_connect_protocol::pairing::PairingService>>>,
        mpris_manager: Option<Arc<crate::mpris_manager::MprisManager>>,
        pending_pairing_requests: Arc<RwLock<std::collections::HashMap<String, bool>>>,
        metrics: Option<Arc<RwLock<crate::diagnostics::Metrics>>>,
        config: Arc<RwLock<crate::config::Config>>,
    ) -> Result<Self> {
        info!("Starting DBus server on {}", SERVICE_NAME);

        // Create connection WITHOUT requesting name first
        let connection = connection::Builder::session()?
            .build()
            .await
            .context("Failed to build DBus connection")?;

        // Clone device_manager and connection_manager for the Open interface before moving to CConnectInterface
        let device_manager_for_open = device_manager.clone();
        let connection_manager_for_open = connection_manager.clone();

        // Create interface with connection reference
        // We pass the current Tokio handle so that zbus handlers can spawn tasks on the tokio runtime
        let interface = CConnectInterface::new(
            device_manager,
            plugin_manager,
            connection_manager,
            device_config_registry,
            pairing_service,
            mpris_manager,
            pending_pairing_requests,
            connection.clone(),
            metrics,
            config,
            Handle::current(),
        );

        // Serve the main interface BEFORE requesting the name
        connection
            .object_server()
            .at(OBJECT_PATH, interface)
            .await
            .context("Failed to serve interface")?;

        // Register the Open interface on a separate path
        let open_interface =
            OpenInterface::new(device_manager_for_open, connection_manager_for_open);
        connection
            .object_server()
            .at("/com/system76/CosmicConnect/Open", open_interface)
            .await
            .context("Failed to serve Open interface")?;

        info!("Registered Open interface at /com/system76/CosmicConnect/Open");

        // Now request the DBus name after all interfaces are registered
        connection
            .request_name(SERVICE_NAME)
            .await
            .context("Failed to request DBus name")?;

        info!("DBus server started successfully");

        Ok(Self { connection })
    }

    /// Get the DBus connection
    #[allow(dead_code)]
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Get the interface reference for signal emission
    async fn interface_ref(&self) -> Result<InterfaceRef<CConnectInterface>> {
        self.connection
            .object_server()
            .interface::<_, CConnectInterface>(OBJECT_PATH)
            .await
            .context("Failed to get interface reference")
    }

    /// Emit a device_added signal
    pub async fn emit_device_added(&self, device: &Device) -> Result<()> {
        let device_info = DeviceInfo::from(device);
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, CConnectInterface>(OBJECT_PATH)
            .await?;

        CConnectInterface::device_added(iface_ref.signal_emitter(), device.id(), device_info)
            .await?;

        debug!("Emitted DeviceAdded signal for {}", device.id());
        Ok(())
    }

    /// Emit a device_removed signal
    #[allow(dead_code)]
    pub async fn emit_device_removed(&self, device_id: &str) -> Result<()> {
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, CConnectInterface>(OBJECT_PATH)
            .await?;

        CConnectInterface::device_removed(iface_ref.signal_emitter(), device_id).await?;

        debug!("Emitted DeviceRemoved signal for {}", device_id);
        Ok(())
    }

    /// Emit a device_state_changed signal
    pub async fn emit_device_state_changed(&self, device_id: &str, state: &str) -> Result<()> {
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, CConnectInterface>(OBJECT_PATH)
            .await?;

        CConnectInterface::device_state_changed(iface_ref.signal_emitter(), device_id, state)
            .await?;

        debug!(
            "Emitted DeviceStateChanged signal for {} ({})",
            device_id, state
        );
        Ok(())
    }

    /// Emit a pairing_request signal
    pub async fn emit_pairing_request(&self, device_id: &str) -> Result<()> {
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, CConnectInterface>(OBJECT_PATH)
            .await?;

        CConnectInterface::pairing_request(iface_ref.signal_emitter(), device_id).await?;

        debug!("Emitted PairingRequest signal for {}", device_id);
        Ok(())
    }

    /// Emit a messaging_notification signal
    pub async fn emit_messaging_notification(
        &self,
        messenger: &str,
        sender: &str,
        message: &str,
        conversation_id: &str,
    ) -> Result<()> {
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, CConnectInterface>(OBJECT_PATH)
            .await?;

        CConnectInterface::messaging_notification(
            iface_ref.signal_emitter(),
            messenger,
            sender,
            message,
            conversation_id,
        )
        .await?;

        debug!("Emitted MessagingNotification signal for {}", messenger);
        Ok(())
    }

    /// Emit a pairing_status_changed signal
    pub async fn emit_pairing_status_changed(&self, device_id: &str, status: &str) -> Result<()> {
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, CConnectInterface>(OBJECT_PATH)
            .await?;

        CConnectInterface::pairing_status_changed(iface_ref.signal_emitter(), device_id, status)
            .await?;

        debug!(
            "Emitted PairingStatusChanged signal for {} ({})",
            device_id, status
        );
        Ok(())
    }

    /// Emit a plugin_event signal
    #[allow(dead_code)]
    pub async fn emit_plugin_event(&self, device_id: &str, plugin: &str, data: &str) -> Result<()> {
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, CConnectInterface>(OBJECT_PATH)
            .await?;

        CConnectInterface::plugin_event(iface_ref.signal_emitter(), device_id, plugin, data)
            .await?;

        debug!("Emitted PluginEvent signal for {} ({})", device_id, plugin);
        Ok(())
    }

    /// Emit a transfer_progress signal
    #[allow(dead_code)]
    pub async fn emit_transfer_progress(
        &self,
        transfer_id: &str,
        device_id: &str,
        filename: &str,
        bytes_transferred: u64,
        total_bytes: u64,
        direction: &str,
    ) -> Result<()> {
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, CConnectInterface>(OBJECT_PATH)
            .await?;

        CConnectInterface::transfer_progress(
            iface_ref.signal_emitter(),
            transfer_id,
            device_id,
            filename,
            bytes_transferred,
            total_bytes,
            direction,
        )
        .await?;

        debug!(
            "Emitted TransferProgress signal: {} - {}/{} bytes",
            transfer_id, bytes_transferred, total_bytes
        );
        Ok(())
    }

    /// Emit a transfer_complete signal
    #[allow(dead_code)]
    pub async fn emit_transfer_complete(
        &self,
        transfer_id: &str,
        device_id: &str,
        filename: &str,
        success: bool,
        error_message: &str,
    ) -> Result<()> {
        let object_server = self.connection.object_server();
        let iface_ref = object_server
            .interface::<_, CConnectInterface>(OBJECT_PATH)
            .await?;

        CConnectInterface::transfer_complete(
            iface_ref.signal_emitter(),
            transfer_id,
            device_id,
            filename,
            success,
            error_message,
        )
        .await?;

        Ok(())
    }

    /// Emit a screen_share_requested signal
    pub async fn emit_screen_share_requested(&self, device_id: &str) -> Result<()> {
        let iface_ref = self.interface_ref().await?;
        CConnectInterface::screen_share_requested(iface_ref.signal_emitter(), device_id).await?;
        debug!("Emitted ScreenShareRequested signal for {}", device_id);
        Ok(())
    }

    /// Emit a screen_share_started signal
    pub async fn emit_screen_share_started(&self, device_id: &str, is_sender: bool) -> Result<()> {
        let iface_ref = self.interface_ref().await?;
        CConnectInterface::screen_share_started(iface_ref.signal_emitter(), device_id, is_sender)
            .await?;
        debug!(
            "Emitted ScreenShareStarted signal for {} (sender={})",
            device_id, is_sender
        );
        Ok(())
    }

    /// Emit a screen_share_stopped signal
    pub async fn emit_screen_share_stopped(&self, device_id: &str) -> Result<()> {
        let iface_ref = self.interface_ref().await?;
        CConnectInterface::screen_share_stopped(iface_ref.signal_emitter(), device_id).await?;
        debug!("Emitted ScreenShareStopped signal for {}", device_id);
        Ok(())
    }

    /// Emit a screen_share_cursor_update signal
    pub async fn emit_screen_share_cursor_update(
        &self,
        device_id: &str,
        x: i32,
        y: i32,
        visible: bool,
    ) -> Result<()> {
        let iface_ref = self.interface_ref().await?;
        CConnectInterface::screen_share_cursor_update(
            iface_ref.signal_emitter(),
            device_id,
            x,
            y,
            visible,
        )
        .await?;
        tracing::trace!(
            "Emitted ScreenShareCursorUpdate signal for {} at ({}, {})",
            device_id,
            x,
            y
        );
        Ok(())
    }

    /// Emit a screen_share_annotation signal
    pub async fn emit_screen_share_annotation(
        &self,
        device_id: &str,
        annotation_type: &str,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        color: &str,
        width: u8,
    ) -> Result<()> {
        let iface_ref = self.interface_ref().await?;
        CConnectInterface::screen_share_annotation(
            iface_ref.signal_emitter(),
            device_id,
            annotation_type,
            x1,
            y1,
            x2,
            y2,
            color,
            width,
        )
        .await?;
        debug!(
            "Emitted ScreenShareAnnotation signal for {} ({})",
            device_id, annotation_type
        );
        Ok(())
    }
}

//
// ============================================================================
// Open on Phone Interface
// ============================================================================
//

/// Allowed URL schemes for security
const ALLOWED_URL_SCHEMES: &[&str] = &[
    "http", "https", "ftp", "ftps", "mailto", "tel", "sms", "geo", "file",
];

/// DBus interface for opening content on Android devices
///
/// Provides methods for sending URLs and files to connected devices
/// to be opened with appropriate handlers.
///
/// ## DBus Interface
///
/// **Service**: `com.system76.CosmicConnect`
/// **Object Path**: `/com/system76/CosmicConnect/Open`
/// **Interface**: `com.system76.CosmicConnect.Open`
pub struct OpenInterface {
    /// Device manager for accessing connected devices
    device_manager: Arc<RwLock<DeviceManager>>,
    /// Connection manager for sending packets
    connection_manager: Arc<RwLock<ConnectionManager>>,
}

impl OpenInterface {
    /// Create a new OpenInterface
    pub fn new(
        device_manager: Arc<RwLock<DeviceManager>>,
        connection_manager: Arc<RwLock<ConnectionManager>>,
    ) -> Self {
        Self {
            device_manager,
            connection_manager,
        }
    }

    /// Validate URL scheme against allowed list
    fn is_scheme_allowed(url: &str) -> bool {
        // Extract scheme by finding the first colon (handles both "http://" and "mailto:")
        let scheme = url.split(':').next().unwrap_or("").to_lowercase();
        ALLOWED_URL_SCHEMES.contains(&scheme.as_str())
    }

    /// Get a device by ID or return the default device
    async fn get_target_device(&self, device_id: Option<&str>) -> Result<Device, zbus::fdo::Error> {
        let manager = self.device_manager.read().await;

        let device = match device_id {
            Some(id) if !id.is_empty() => manager
                .get_device(id)
                .ok_or_else(|| zbus::fdo::Error::Failed(format!("Device not found: {}", id)))?
                .clone(),
            _ => {
                // Get first paired and reachable device with share capability
                manager
                    .devices()
                    .find(|d| {
                        d.is_paired()
                            && d.is_reachable()
                            && d.info
                                .outgoing_capabilities
                                .contains(&"cconnect.share.request".to_string())
                    })
                    .ok_or_else(|| {
                        zbus::fdo::Error::Failed("No suitable device found".to_string())
                    })?
                    .clone()
            }
        };

        // Verify device is connected
        if !device.is_connected() {
            return Err(zbus::fdo::Error::Failed(format!(
                "Device {} is not connected",
                device.name()
            )));
        }

        Ok(device)
    }

    /// Create an "open URL" packet using the share plugin format
    fn create_open_url_packet(url: String) -> cosmic_connect_protocol::Packet {
        cosmic_connect_protocol::Packet::new(
            "cconnect.share.request",
            serde_json::json!({
                "url": url
            }),
        )
    }
}

#[interface(name = "com.system76.CosmicConnect.Open")]
impl OpenInterface {
    /// Open a URL on a connected Android device
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to open (must have an allowed scheme)
    ///
    /// # Returns
    ///
    /// Returns the packet ID as a request identifier for tracking
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - URL scheme is not allowed
    /// - No suitable device is available
    /// - Device is not connected
    /// - Packet sending fails
    async fn open_on_phone(&self, url: String) -> zbus::fdo::Result<String> {
        info!("Open on phone request for URL: {}", url);

        // Validate URL scheme
        if !Self::is_scheme_allowed(&url) {
            error!("URL scheme not allowed: {}", url);
            return Err(zbus::fdo::Error::Failed(
                "URL scheme not allowed. Allowed schemes: http, https, ftp, ftps, mailto, tel, sms, geo, file".to_string()
            ));
        }

        // Get target device (default)
        let device = self.get_target_device(None).await?;
        debug!("Opening URL on device: {}", device.name());

        // Create and send packet
        let packet = Self::create_open_url_packet(url.clone());
        let request_id = packet.id.to_string();

        self.connection_manager
            .read()
            .await
            .send_packet(&device.id(), &packet)
            .await
            .map_err(|e| {
                error!("Failed to send open URL packet: {}", e);
                zbus::fdo::Error::Failed(format!("Failed to send packet: {}", e))
            })?;

        info!("Sent open URL request {} to {}", request_id, device.name());
        Ok(request_id)
    }

    /// Open a file on a connected Android device (transfer + open)
    ///
    /// # Arguments
    ///
    /// * `path` - Absolute path to the file to transfer and open
    /// * `device_id` - Target device ID (empty string for default device)
    ///
    /// # Returns
    ///
    /// Returns the transfer ID as a request identifier for tracking
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - File does not exist or cannot be read
    /// - Device is not found or not connected
    /// - File transfer fails to initiate
    #[allow(unused_variables)]
    async fn open_file_on_phone(
        &self,
        path: String,
        device_id: String,
    ) -> zbus::fdo::Result<String> {
        info!("Open file on phone request: {} -> {}", path, device_id);

        // Validate file exists
        let file_path = std::path::Path::new(&path);
        if !file_path.exists() {
            error!("File not found: {}", path);
            return Err(zbus::fdo::Error::Failed(format!(
                "File not found: {}",
                path
            )));
        }

        // Get target device
        let device_id_opt = if device_id.is_empty() {
            None
        } else {
            Some(device_id.as_str())
        };
        let device = self.get_target_device(device_id_opt).await?;
        debug!("Opening file on device: {}", device.name());

        // TODO: Implement file transfer + open
        // This requires integration with the Share plugin and payload transfer
        // For now, return a placeholder error
        warn!("File transfer not yet implemented");
        Err(zbus::fdo::Error::Failed(
            "File transfer + open not yet implemented. Use share plugin directly for now."
                .to_string(),
        ))
    }

    /// List devices that support opening content
    ///
    /// # Returns
    ///
    /// Returns a list of device IDs that have the share capability
    /// and are currently paired and reachable
    async fn list_open_capable_devices(&self) -> zbus::fdo::Result<Vec<String>> {
        debug!("Listing open-capable devices");

        let manager = self.device_manager.read().await;
        let devices: Vec<String> = manager
            .devices()
            .filter(|d| {
                d.is_paired()
                    && d.is_reachable()
                    && d.info
                        .outgoing_capabilities
                        .contains(&"cconnect.share.request".to_string())
            })
            .map(|d| d.id().to_string())
            .collect();

        info!("Found {} open-capable devices", devices.len());
        Ok(devices)
    }
}

#[cfg(test)]
mod open_interface_tests {
    use super::*;

    #[test]
    fn test_allowed_schemes() {
        assert!(OpenInterface::is_scheme_allowed("https://example.com"));
        assert!(OpenInterface::is_scheme_allowed("http://example.com"));
        assert!(OpenInterface::is_scheme_allowed("mailto:test@example.com"));
        assert!(OpenInterface::is_scheme_allowed("tel:+1234567890"));
        assert!(OpenInterface::is_scheme_allowed("sms:+1234567890"));
        assert!(OpenInterface::is_scheme_allowed("ftp://files.example.com"));
        assert!(OpenInterface::is_scheme_allowed("file:///path/to/file"));
    }

    #[test]
    fn test_disallowed_schemes() {
        assert!(!OpenInterface::is_scheme_allowed("javascript:alert(1)"));
        assert!(!OpenInterface::is_scheme_allowed(
            "data:text/html,<script>alert(1)</script>"
        ));
        assert!(!OpenInterface::is_scheme_allowed("vbscript:msgbox"));
        assert!(!OpenInterface::is_scheme_allowed("about:blank"));
    }

    #[test]
    fn test_create_open_url_packet() {
        let packet = OpenInterface::create_open_url_packet("https://example.com".to_string());
        assert_eq!(packet.packet_type, "cconnect.share.request");
        assert_eq!(packet.body["url"], "https://example.com");
    }
}
