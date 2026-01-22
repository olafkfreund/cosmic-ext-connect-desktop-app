//! RunCommand Plugin
//!
//! Enables defining and executing shell commands remotely from connected devices.
//! Commands are pre-configured on the desktop and can be triggered from mobile devices.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `cconnect.runcommand` - Command list response (outgoing)
//! - `cconnect.runcommand.request` - Command execution request (incoming)
//!
//! **Capabilities**:
//! - Incoming: `cconnect.runcommand.request` - Receives command execution requests
//! - Outgoing: `cconnect.runcommand` - Sends command list to devices
//!
//! ## Packet Formats
//!
//! ### Command List Response (`cconnect.runcommand`)
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.runcommand",
//!     "body": {
//!         "commandList": "{\"cmd1\":{\"name\":\"List Files\",\"command\":\"ls -la\"},\"cmd2\":{...}}",
//!         "canAddCommand": true
//!     }
//! }
//! ```
//!
//! ### Command Execution Request (`cconnect.runcommand.request`)
//!
//! Execute a specific command:
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.runcommand.request",
//!     "body": {
//!         "key": "cmd1"
//!     }
//! }
//! ```
//!
//! Request command list:
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.runcommand.request",
//!     "body": {
//!         "requestCommandList": true
//!     }
//! }
//! ```
//!
//! ## Configuration
//!
//! Commands are stored in a JSON configuration file per device:
//! `~/.config/kdeconnect/<device_id>/kdeconnect_runcommand/commands.json`
//!
//! ```json
//! {
//!     "commands": {
//!         "cmd1": {
//!             "name": "List Files",
//!             "command": "ls -la"
//!         },
//!         "cmd2": {
//!             "name": "Check Disk Space",
//!             "command": "df -h"
//!         }
//!     }
//! }
//! ```
//!
//! ## Security
//!
//! - Commands are pre-configured by the user on the desktop
//! - Only paired devices can trigger commands
//! - Commands execute with the user's permissions
//! - No arbitrary command execution from mobile devices
//!
//! ## Example
//!
//! ```rust,ignore
//! use cosmic_connect_core::plugins::runcommand::RunCommandPlugin;
//! use cosmic_connect_core::Plugin;
//!
//! // Create plugin
//! let mut plugin = RunCommandPlugin::new();
//!
//! // Add a command
//! plugin.add_command("backup", "Backup Home", "tar -czf ~/backup.tar.gz ~").await?;
//!
//! // List commands
//! let commands = plugin.get_commands().await;
//!
//! // Send command list to device
//! let packet = plugin.create_command_list_packet().await;
//! ```
//!
//! ## References
//!
//! - [CConnect RunCommand Plugin](https://github.com/KDE/cconnect-kde/tree/master/plugins/runcommand)
//! - [Valent Protocol - RunCommand](https://valent.andyholmes.ca/documentation/protocol.html)

use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use super::{Plugin, PluginFactory};

/// A runnable command definition
///
/// Represents a pre-configured shell command that can be executed
/// remotely from a connected device.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Command {
    /// User-friendly name displayed on the mobile device
    pub name: String,

    /// Shell command to execute
    pub command: String,
}

impl Command {
    /// Create a new command
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
        }
    }
}

/// Configuration storage for runcommand plugin
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RunCommandConfig {
    /// Map of command ID to command definition
    #[serde(default)]
    commands: HashMap<String, Command>,
}

/// RunCommand plugin for remote command execution
///
/// Handles `cconnect.runcommand.request` packets and executes
/// pre-configured shell commands.
///
/// ## Features
///
/// - Store and manage shell commands
/// - Execute commands remotely from mobile devices
/// - Send command list to devices
/// - Per-device configuration
/// - Secure: Only pre-configured commands can be executed
///
/// ## Example
///
/// ```rust
/// use cosmic_connect_core::plugins::runcommand::RunCommandPlugin;
///
/// let plugin = RunCommandPlugin::new();
/// assert_eq!(plugin.name(), "runcommand");
/// ```
#[derive(Debug)]
pub struct RunCommandPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Configuration storage
    config: Arc<RwLock<RunCommandConfig>>,

    /// Path to configuration file
    config_path: Option<PathBuf>,

    /// Count of commands executed
    commands_executed: Arc<RwLock<u64>>,

    /// Channel to send packets
    packet_sender: Option<tokio::sync::mpsc::Sender<(String, Packet)>>,
}

impl RunCommandPlugin {
    /// Create a new runcommand plugin
    ///
    /// # Example
    ///
    /// ```rust
    /// use cosmic_connect_core::plugins::runcommand::RunCommandPlugin;
    ///
    /// let plugin = RunCommandPlugin::new();
    /// assert_eq!(plugin.name(), "runcommand");
    /// ```
    pub fn new() -> Self {
        Self {
            device_id: None,
            config: Arc::new(RwLock::new(RunCommandConfig::default())),
            config_path: None,
            commands_executed: Arc::new(RwLock::new(0)),
            packet_sender: None,
        }
    }

    /// Get the configuration file path for a device
    fn get_config_path(device_id: &str) -> Result<PathBuf> {
        // Use $HOME/.config/kdeconnect/<device_id>/kdeconnect_runcommand/commands.json
        let home_dir = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| {
                ProtocolError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not determine home directory",
                ))
            })?;

        let plugin_dir = PathBuf::from(home_dir)
            .join(".config")
            .join("cconnect")
            .join(device_id)
            .join("kdeconnect_runcommand");

        Ok(plugin_dir.join("commands.json"))
    }

    /// Load configuration from disk
    async fn load_config(&self) -> Result<()> {
        if let Some(config_path) = &self.config_path {
            if config_path.exists() {
                let contents = fs::read_to_string(config_path).await.map_err(|e| {
                    ProtocolError::Plugin(format!("Failed to read config file: {}", e))
                })?;

                let loaded_config: RunCommandConfig =
                    serde_json::from_str(&contents).map_err(|e| {
                        ProtocolError::Plugin(format!("Failed to parse config file: {}", e))
                    })?;

                let mut config = self.config.write().await;
                *config = loaded_config;

                info!("Loaded {} commands from config", config.commands.len());
            } else {
                debug!("Config file does not exist yet: {:?}", config_path);
            }
        }

        Ok(())
    }

    /// Save configuration to disk
    async fn save_config(&self) -> Result<()> {
        if let Some(config_path) = &self.config_path {
            // Create parent directory if it doesn't exist
            if let Some(parent) = config_path.parent() {
                fs::create_dir_all(parent).await.map_err(|e| {
                    ProtocolError::Plugin(format!("Failed to create config directory: {}", e))
                })?;
            }

            let config = self.config.read().await;
            let contents = serde_json::to_string_pretty(&*config)
                .map_err(|e| ProtocolError::Plugin(format!("Failed to serialize config: {}", e)))?;

            fs::write(config_path, contents).await.map_err(|e| {
                ProtocolError::Plugin(format!("Failed to write config file: {}", e))
            })?;

            debug!("Saved configuration to {:?}", config_path);
        }

        Ok(())
    }

    /// Add a new command
    ///
    /// # Parameters
    ///
    /// - `id`: Unique identifier for the command
    /// - `name`: User-friendly name
    /// - `command`: Shell command to execute
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// plugin.add_command("backup", "Backup Home", "tar -czf ~/backup.tar.gz ~").await?;
    /// ```
    pub async fn add_command(
        &self,
        id: impl Into<String>,
        name: impl Into<String>,
        command: impl Into<String>,
    ) -> Result<()> {
        let id = id.into();
        let cmd = Command::new(name, command);

        let mut config = self.config.write().await;
        config.commands.insert(id.clone(), cmd.clone());
        drop(config);

        self.save_config().await?;

        info!("Added command '{}': {}", id, cmd.name);
        Ok(())
    }

    /// Remove a command
    ///
    /// # Parameters
    ///
    /// - `id`: Command identifier to remove
    pub async fn remove_command(&self, id: &str) -> Result<()> {
        let mut config = self.config.write().await;
        if config.commands.remove(id).is_some() {
            drop(config);
            self.save_config().await?;
            info!("Removed command '{}'", id);
            Ok(())
        } else {
            Err(ProtocolError::Plugin(format!("Command '{}' not found", id)))
        }
    }

    /// Get all commands
    ///
    /// Returns a clone of the current command map
    pub async fn get_commands(&self) -> HashMap<String, Command> {
        let config = self.config.read().await;
        config.commands.clone()
    }

    /// Get a specific command
    pub async fn get_command(&self, id: &str) -> Option<Command> {
        let config = self.config.read().await;
        config.commands.get(id).cloned()
    }

    /// Clear all commands
    pub async fn clear_commands(&self) -> Result<()> {
        let mut config = self.config.write().await;
        config.commands.clear();
        drop(config);

        self.save_config().await?;
        info!("Cleared all commands");
        Ok(())
    }

    /// Create a command list packet
    ///
    /// Creates a `cconnect.runcommand` packet containing all configured commands.
    ///
    /// # Returns
    ///
    /// A `Packet` ready to be sent to the device
    pub async fn create_command_list_packet(&self) -> Packet {
        let config = self.config.read().await;

        // Serialize command list as JSON string (as per protocol spec)
        let command_list_json =
            serde_json::to_string(&config.commands).unwrap_or_else(|_| "{}".to_string());

        Packet::new(
            "cconnect.runcommand",
            json!({
                "commandList": command_list_json,
                "canAddCommand": true
            }),
        )
    }

    /// Execute a command by ID
    ///
    /// Looks up the command and executes it using the system shell.
    ///
    /// # Parameters
    ///
    /// - `id`: Command identifier to execute
    ///
    /// # Returns
    ///
    /// `Ok(())` if command executed successfully, `Err` otherwise
    async fn execute_command(&self, id: &str) -> Result<()> {
        let command = self
            .get_command(id)
            .await
            .ok_or_else(|| ProtocolError::Plugin(format!("Command '{}' not found", id)))?;

        info!("Executing command '{}': {}", id, command.name);
        debug!("Command: {}", command.command);

        // Execute command using sh -c (Linux/Unix) or cmd /C (Windows)
        #[cfg(target_os = "windows")]
        let (shell, flag) = ("cmd", "/C");

        #[cfg(not(target_os = "windows"))]
        let (shell, flag) = ("/bin/sh", "-c");

        // Spawn command detached (non-blocking)
        match tokio::process::Command::new(shell)
            .arg(flag)
            .arg(&command.command)
            .spawn()
        {
            Ok(mut child) => {
                // Increment execution counter
                let mut count = self.commands_executed.write().await;
                *count += 1;
                drop(count);

                info!(
                    "Command '{}' started successfully (PID: {:?})",
                    id,
                    child.id()
                );

                // Wait for completion in background
                let id_clone = id.to_string();
                tokio::spawn(async move {
                    match child.wait().await {
                        Ok(status) => {
                            if status.success() {
                                debug!("Command '{}' completed successfully", id_clone);
                            } else {
                                warn!("Command '{}' exited with status: {}", id_clone, status);
                            }
                        }
                        Err(e) => {
                            error!("Failed to wait for command '{}': {}", id_clone, e);
                        }
                    }
                });

                Ok(())
            }
            Err(e) => {
                error!("Failed to execute command '{}': {}", id, e);
                Err(ProtocolError::Plugin(format!(
                    "Failed to execute command: {}",
                    e
                )))
            }
        }
    }

    /// Handle a command request packet
    async fn handle_request(&mut self, packet: &Packet) -> Result<Option<Packet>> {
        // Check if it's a command list request
        if let Some(request_list) = packet.body.get("requestCommandList") {
            if request_list.as_bool().unwrap_or(false) {
                info!("Received command list request");
                let response = self.create_command_list_packet().await;
                return Ok(Some(response));
            }
        }

        // Check if it's a command execution request
        if let Some(key) = packet.body.get("key").and_then(|v| v.as_str()) {
            info!("Received command execution request for '{}'", key);

            if let Err(e) = self.execute_command(key).await {
                warn!("Failed to execute command '{}': {}", key, e);
                // Don't return error - just log it
            }

            // No response packet needed for execution
            return Ok(None);
        }

        warn!("Received runcommand request with no valid action");
        Ok(None)
    }

    /// Get number of commands executed
    pub async fn commands_executed(&self) -> u64 {
        *self.commands_executed.read().await
    }
}

impl Default for RunCommandPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for RunCommandPlugin {
    fn name(&self) -> &str {
        "runcommand"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec!["cconnect.runcommand.request".to_string()]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec!["cconnect.runcommand".to_string()]
    }

    async fn init(&mut self, device: &Device, packet_sender: tokio::sync::mpsc::Sender<(String, Packet)>) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        self.packet_sender = Some(packet_sender);

        // Set up config path
        self.config_path = Some(Self::get_config_path(device.id())?);

        // Load existing configuration
        if let Err(e) = self.load_config().await {
            warn!("Failed to load config: {}", e);
            // Continue with empty config
        }

        info!("RunCommand plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        let command_count = self.get_commands().await.len();
        info!("RunCommand plugin started with {} commands", command_count);
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        let executed = self.commands_executed().await;
        info!("RunCommand plugin stopped - {} commands executed", executed);
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, _device: &mut Device) -> Result<()> {
        if packet.packet_type == "cconnect.runcommand.request" {
            if let Some(response) = self.handle_request(packet).await? {
                if let Some(sender) = &self.packet_sender {
                    if let Some(device_id) = &self.device_id {
                        if let Err(e) = sender.send((device_id.clone(), response)).await {
                            error!("Failed to send runcommand response: {}", e);
                        } else {
                            debug!("Sent runcommand response");
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Factory for creating RunCommandPlugin instances
///
/// # Example
///
/// ```rust
/// use cosmic_connect_core::plugins::runcommand::RunCommandPluginFactory;
/// use cosmic_connect_core::plugins::PluginFactory;
/// use std::sync::Arc;
///
/// let factory: Arc<dyn PluginFactory> = Arc::new(RunCommandPluginFactory);
/// let plugin = factory.create();
/// assert_eq!(plugin.name(), "runcommand");
/// ```
#[derive(Debug, Clone, Copy)]
pub struct RunCommandPluginFactory;

impl PluginFactory for RunCommandPluginFactory {
    fn name(&self) -> &str {
        "runcommand"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec!["cconnect.runcommand.request".to_string()]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec!["cconnect.runcommand".to_string()]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(RunCommandPlugin::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceInfo, DeviceType};

    fn create_test_device() -> Device {
        let info = DeviceInfo::new("Test Device", DeviceType::Desktop, 1716);
        Device::from_discovery(info)
    }

    #[test]
    fn test_plugin_creation() {
        let plugin = RunCommandPlugin::new();
        assert_eq!(plugin.name(), "runcommand");
    }

    #[test]
    fn test_capabilities() {
        let plugin = RunCommandPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0], "cconnect.runcommand.request");

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0], "cconnect.runcommand");
    }

    #[test]
    fn test_command_creation() {
        let cmd = Command::new("List Files", "ls -la");
        assert_eq!(cmd.name, "List Files");
        assert_eq!(cmd.command, "ls -la");
    }

    #[tokio::test]
    async fn test_add_and_get_command() {
        let plugin = RunCommandPlugin::new();

        // Add command
        plugin
            .add_command("test1", "Test Command", "echo hello")
            .await
            .unwrap();

        // Get command
        let cmd = plugin.get_command("test1").await;
        assert!(cmd.is_some());

        let cmd = cmd.unwrap();
        assert_eq!(cmd.name, "Test Command");
        assert_eq!(cmd.command, "echo hello");
    }

    #[tokio::test]
    async fn test_get_all_commands() {
        let plugin = RunCommandPlugin::new();

        // Add multiple commands
        plugin
            .add_command("cmd1", "Command 1", "echo 1")
            .await
            .unwrap();
        plugin
            .add_command("cmd2", "Command 2", "echo 2")
            .await
            .unwrap();

        let commands = plugin.get_commands().await;
        assert_eq!(commands.len(), 2);
        assert!(commands.contains_key("cmd1"));
        assert!(commands.contains_key("cmd2"));
    }

    #[tokio::test]
    async fn test_remove_command() {
        let plugin = RunCommandPlugin::new();

        // Add and remove command
        plugin
            .add_command("test1", "Test", "echo test")
            .await
            .unwrap();

        assert!(plugin.get_command("test1").await.is_some());

        plugin.remove_command("test1").await.unwrap();
        assert!(plugin.get_command("test1").await.is_none());
    }

    #[tokio::test]
    async fn test_clear_commands() {
        let plugin = RunCommandPlugin::new();

        // Add commands
        plugin.add_command("cmd1", "C1", "echo 1").await.unwrap();
        plugin.add_command("cmd2", "C2", "echo 2").await.unwrap();

        assert_eq!(plugin.get_commands().await.len(), 2);

        // Clear all
        plugin.clear_commands().await.unwrap();
        assert_eq!(plugin.get_commands().await.len(), 0);
    }

    #[tokio::test]
    async fn test_create_command_list_packet() {
        let plugin = RunCommandPlugin::new();

        // Add commands
        plugin
            .add_command("cmd1", "List Files", "ls -la")
            .await
            .unwrap();
        plugin
            .add_command("cmd2", "Check Disk", "df -h")
            .await
            .unwrap();

        let packet = plugin.create_command_list_packet().await;

        assert_eq!(packet.packet_type, "cconnect.runcommand");
        assert!(packet.body.get("commandList").is_some());
        assert_eq!(
            packet.body.get("canAddCommand").and_then(|v| v.as_bool()),
            Some(true)
        );

        // Verify commandList is a JSON string
        let command_list_str = packet
            .body
            .get("commandList")
            .and_then(|v| v.as_str())
            .unwrap();
        let parsed: HashMap<String, Command> = serde_json::from_str(command_list_str).unwrap();
        assert_eq!(parsed.len(), 2);
    }

    #[tokio::test]
    async fn test_handle_command_list_request() {
        let mut plugin = RunCommandPlugin::new();
        let device = create_test_device();
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();

        // Add a command
        plugin
            .add_command("test", "Test", "echo test")
            .await
            .unwrap();

        // Create request packet
        let packet = Packet::new(
            "cconnect.runcommand.request",
            json!({ "requestCommandList": true }),
        );

        let response = plugin.handle_request(&packet).await.unwrap();

        assert!(response.is_some());
        let response = response.unwrap();
        assert_eq!(response.packet_type, "cconnect.runcommand");
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = RunCommandPlugin::new();
        let device = create_test_device();

        // Initialize
        plugin.init(&device, tokio::sync::mpsc::channel(100).0).await.unwrap();
        assert!(plugin.device_id.is_some());
        assert!(plugin.config_path.is_some());

        // Start
        plugin.start().await.unwrap();

        // Stop
        plugin.stop().await.unwrap();
    }

    #[test]
    fn test_factory() {
        let factory = RunCommandPluginFactory;
        assert_eq!(factory.name(), "runcommand");

        let plugin = factory.create();
        assert_eq!(plugin.name(), "runcommand");
    }
}
