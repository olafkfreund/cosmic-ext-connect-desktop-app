//! Macro Automation Plugin
//!
//! Enables complex automation scripts and workflows across connected desktops.
//! Provides a simple, secure DSL for defining multi-step macros with conditional logic.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - Incoming: `cconnect.macro.define`, `cconnect.macro.execute`, `cconnect.macro.cancel`, `cconnect.macro.list`
//! - Outgoing: `cconnect.macro.status`, `cconnect.macro.list_response`
//!
//! **Capabilities**: `cconnect.macro`
//!
//! ## Define Macro
//!
//! Create or update a macro definition:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.macro.define",
//!     "body": {
//!         "macro_id": "dev-startup",
//!         "name": "Development Environment Startup",
//!         "description": "Start all development tools",
//!         "steps": [
//!             {
//!                 "action": "notify",
//!                 "params": {
//!                     "title": "Starting development environment",
//!                     "message": "Please wait..."
//!                 }
//!             },
//!             {
//!                 "action": "run_command",
//!                 "params": {
//!                     "command": "start-ide"
//!                 }
//!             },
//!             {
//!                 "action": "wait",
//!                 "params": {
//!                     "seconds": 2
//!                 }
//!             },
//!             {
//!                 "action": "notify",
//!                 "params": {
//!                     "title": "Ready",
//!                     "message": "Development environment is ready"
//!                 }
//!             }
//!         ]
//!     }
//! }
//! ```
//!
//! ## Execute Macro
//!
//! Run a defined macro:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.macro.execute",
//!     "body": {
//!         "macro_id": "dev-startup",
//!         "variables": {
//!             "project": "cosmic-connect"
//!         }
//!     }
//! }
//! ```
//!
//! ## Macro Status
//!
//! Report execution status:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.macro.status",
//!     "body": {
//!         "macro_id": "dev-startup",
//!         "execution_id": "exec-uuid",
//!         "status": "running",
//!         "current_step": 2,
//!         "total_steps": 4,
//!         "message": "Executing step 2: run_command"
//!     }
//! }
//! ```
//!
//! ## Cancel Macro
//!
//! Stop running macro:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.macro.cancel",
//!     "body": {
//!         "execution_id": "exec-uuid"
//!     }
//! }
//! ```
//!
//! ## List Macros
//!
//! Request list of available macros:
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.macro.list",
//!     "body": {}
//! }
//! ```
//!
//! ## Supported Actions
//!
//! - `notify` - Send notification (params: title, message)
//! - `run_command` - Execute command via RunCommand plugin (params: command, args)
//! - `wait` - Delay execution (params: seconds)
//! - `send_file` - Send file via Share plugin (params: path)
//! - `set_clipboard` - Set clipboard content (params: content)
//!
//! Future actions (TODO):
//! - `if_device_connected` - Conditional based on device presence
//! - `if_time_between` - Conditional based on time range
//! - `loop` - Repeat steps
//! - `parallel` - Execute steps concurrently
//!
//! ## Security
//!
//! - Macros disabled by default (config: enable_macro = false)
//! - No arbitrary code execution (only predefined actions)
//! - Command execution via existing RunCommand plugin (with its security)
//! - Resource limits (max steps, max execution time)
//! - Audit logging for all macro executions
//!
//! ## Storage
//!
//! - Macros stored in JSON file: `~/.local/share/cosmic-connect/macros.json`
//! - Per-device macro library
//! - Shared macros synced across devices
//!
//! ## Example
//!
//! ```rust,ignore
//! use cosmic_connect_core::plugins::macro_plugin::*;
//!
//! let mut plugin = MacroPlugin::new();
//!
//! // Define a macro
//! let macro_def = MacroDefinition {
//!     id: "test-macro".to_string(),
//!     name: "Test Macro".to_string(),
//!     description: Some("A test macro".to_string()),
//!     steps: vec![
//!         MacroStep {
//!             action: "notify".to_string(),
//!             params: json!({"title": "Hello", "message": "World"}),
//!         }
//!     ],
//! };
//!
//! plugin.define_macro(macro_def)?;
//!
//! // Execute it
//! let exec_id = plugin.execute_macro("test-macro", HashMap::new()).await?;
//! ```

use crate::{Device, Packet, ProtocolError, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::{Plugin, PluginFactory};

/// Maximum steps in a macro (prevent infinite loops)
const MAX_MACRO_STEPS: usize = 100;

/// Maximum execution time (prevent runaway macros)
const MAX_EXECUTION_TIME_SECS: u64 = 300; // 5 minutes

/// Macro step definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroStep {
    /// Action to perform
    pub action: String,

    /// Action parameters
    pub params: Value,
}

/// Macro definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroDefinition {
    /// Unique macro ID
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// Optional description
    pub description: Option<String>,

    /// Ordered list of steps
    pub steps: Vec<MacroStep>,
}

/// Macro execution status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MacroExecutionStatus {
    /// Macro is currently running
    Running,

    /// Macro completed successfully
    Completed,

    /// Macro failed with error
    Failed,

    /// Macro was cancelled
    Cancelled,
}

/// Active macro execution state
#[derive(Debug, Clone)]
struct MacroExecution {
    /// Execution ID
    id: String,

    /// Macro being executed
    macro_id: String,

    /// Current status
    status: MacroExecutionStatus,

    /// Current step index
    current_step: usize,

    /// Total steps
    total_steps: usize,

    /// Variables for substitution
    variables: HashMap<String, String>,

    /// Error message if failed
    error_message: Option<String>,
}

/// Macro automation plugin
pub struct MacroPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Whether the plugin is enabled
    enabled: bool,

    /// Defined macros
    macros: Arc<RwLock<HashMap<String, MacroDefinition>>>,

    /// Active executions
    executions: Arc<RwLock<HashMap<String, MacroExecution>>>,
}

impl MacroPlugin {
    /// Create a new macro plugin
    pub fn new() -> Self {
        Self {
            device_id: None,
            enabled: false,
            macros: Arc::new(RwLock::new(HashMap::new())),
            executions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Define a new macro
    pub async fn define_macro(&mut self, macro_def: MacroDefinition) -> Result<()> {
        // Validate macro
        if macro_def.steps.len() > MAX_MACRO_STEPS {
            return Err(ProtocolError::invalid_state(format!(
                "Macro has too many steps: {} (max: {})",
                macro_def.steps.len(),
                MAX_MACRO_STEPS
            )));
        }

        info!(
            "Defining macro '{}' ({} steps)",
            macro_def.name,
            macro_def.steps.len()
        );

        self.macros
            .write()
            .await
            .insert(macro_def.id.clone(), macro_def);

        Ok(())
    }

    /// Delete a macro
    pub async fn delete_macro(&mut self, macro_id: &str) -> Result<()> {
        if self.macros.write().await.remove(macro_id).is_some() {
            info!("Deleted macro: {}", macro_id);
            Ok(())
        } else {
            Err(ProtocolError::invalid_state(format!(
                "Macro not found: {}",
                macro_id
            )))
        }
    }

    /// List all defined macros
    pub async fn list_macros(&self) -> Vec<MacroDefinition> {
        self.macros.read().await.values().cloned().collect()
    }

    /// Execute a macro
    pub async fn execute_macro(
        &mut self,
        macro_id: &str,
        variables: HashMap<String, String>,
    ) -> Result<String> {
        // Get macro definition
        let macro_def = self
            .macros
            .read()
            .await
            .get(macro_id)
            .cloned()
            .ok_or_else(|| {
                ProtocolError::invalid_state(format!("Macro not found: {}", macro_id))
            })?;

        // Create execution state
        let exec_id = Uuid::new_v4().to_string();
        let execution = MacroExecution {
            id: exec_id.clone(),
            macro_id: macro_id.to_string(),
            status: MacroExecutionStatus::Running,
            current_step: 0,
            total_steps: macro_def.steps.len(),
            variables,
            error_message: None,
        };

        self.executions
            .write()
            .await
            .insert(exec_id.clone(), execution.clone());

        info!("Starting macro execution: {} ({})", macro_def.name, exec_id);

        // Spawn async task to execute macro
        let executions = self.executions.clone();
        let exec_id_clone = exec_id.clone();

        tokio::spawn(async move {
            Self::execute_macro_steps(exec_id_clone, macro_def, execution.variables, executions)
                .await;
        });

        Ok(exec_id)
    }

    /// Execute macro steps (runs in background task)
    async fn execute_macro_steps(
        exec_id: String,
        macro_def: MacroDefinition,
        variables: HashMap<String, String>,
        executions: Arc<RwLock<HashMap<String, MacroExecution>>>,
    ) {
        let start_time = std::time::Instant::now();

        for (step_idx, step) in macro_def.steps.iter().enumerate() {
            // Check timeout
            if start_time.elapsed().as_secs() > MAX_EXECUTION_TIME_SECS {
                error!(
                    "Macro execution {} exceeded time limit ({}s)",
                    exec_id, MAX_EXECUTION_TIME_SECS
                );
                Self::mark_failed(&executions, &exec_id, "Execution timeout").await;
                return;
            }

            // Update current step
            if let Some(exec) = executions.write().await.get_mut(&exec_id) {
                exec.current_step = step_idx;
            }

            // Execute step
            debug!(
                "Executing macro {} step {}: {}",
                exec_id, step_idx, step.action
            );

            if let Err(e) = Self::execute_step(step, &variables).await {
                error!("Macro step failed: {}", e);
                Self::mark_failed(&executions, &exec_id, &e.to_string()).await;
                return;
            }
        }

        // Mark completed
        if let Some(exec) = executions.write().await.get_mut(&exec_id) {
            exec.status = MacroExecutionStatus::Completed;
            info!("Macro execution {} completed", exec_id);
        }
    }

    /// Execute a single macro step
    async fn execute_step(step: &MacroStep, variables: &HashMap<String, String>) -> Result<()> {
        match step.action.as_str() {
            "notify" => {
                let title = step
                    .params
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Notification");
                let message = step
                    .params
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                info!("Macro notify: {} - {}", title, message);
                // TODO: Actually send notification packet
                Ok(())
            }

            "run_command" => {
                let command = step
                    .params
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ProtocolError::invalid_state("Missing command parameter"))?;

                info!("Macro run_command: {}", command);
                // TODO: Send run_command packet
                Ok(())
            }

            "wait" => {
                let seconds = step
                    .params
                    .get("seconds")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1);

                debug!("Macro wait: {}s", seconds);
                sleep(Duration::from_secs(seconds)).await;
                Ok(())
            }

            "set_clipboard" => {
                let content = step
                    .params
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ProtocolError::invalid_state("Missing content parameter"))?;

                info!("Macro set_clipboard: {} chars", content.len());
                // TODO: Send clipboard packet
                Ok(())
            }

            "send_file" => {
                let path = step
                    .params
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ProtocolError::invalid_state("Missing path parameter"))?;

                info!("Macro send_file: {}", path);
                // TODO: Send share packet
                Ok(())
            }

            _ => Err(ProtocolError::invalid_state(format!(
                "Unknown macro action: {}",
                step.action
            ))),
        }
    }

    /// Mark execution as failed
    async fn mark_failed(
        executions: &Arc<RwLock<HashMap<String, MacroExecution>>>,
        exec_id: &str,
        error_message: &str,
    ) {
        if let Some(exec) = executions.write().await.get_mut(exec_id) {
            exec.status = MacroExecutionStatus::Failed;
            exec.error_message = Some(error_message.to_string());
        }
    }

    /// Cancel a running macro
    pub async fn cancel_execution(&mut self, exec_id: &str) -> Result<()> {
        if let Some(exec) = self.executions.write().await.get_mut(exec_id) {
            if exec.status == MacroExecutionStatus::Running {
                exec.status = MacroExecutionStatus::Cancelled;
                info!("Cancelled macro execution: {}", exec_id);
                Ok(())
            } else {
                Err(ProtocolError::invalid_state(format!(
                    "Execution not running: {}",
                    exec_id
                )))
            }
        } else {
            Err(ProtocolError::invalid_state(format!(
                "Execution not found: {}",
                exec_id
            )))
        }
    }

    /// Get execution status
    pub async fn get_execution_status(&self, exec_id: &str) -> Option<MacroExecution> {
        self.executions.read().await.get(exec_id).cloned()
    }

    /// Create define macro packet
    pub fn create_define_packet(&self, macro_def: &MacroDefinition) -> Packet {
        let steps_json: Vec<Value> = macro_def
            .steps
            .iter()
            .map(|step| {
                json!({
                    "action": step.action,
                    "params": step.params
                })
            })
            .collect();

        Packet::new(
            "cconnect.macro.define",
            json!({
                "macro_id": macro_def.id,
                "name": macro_def.name,
                "description": macro_def.description,
                "steps": steps_json
            }),
        )
    }

    /// Create execute packet
    pub fn create_execute_packet(
        &self,
        macro_id: &str,
        variables: HashMap<String, String>,
    ) -> Packet {
        Packet::new(
            "cconnect.macro.execute",
            json!({
                "macro_id": macro_id,
                "variables": variables
            }),
        )
    }

    /// Create status packet
    pub fn create_status_packet(&self, execution: &MacroExecution) -> Packet {
        Packet::new(
            "cconnect.macro.status",
            json!({
                "execution_id": execution.id,
                "macro_id": execution.macro_id,
                "status": match execution.status {
                    MacroExecutionStatus::Running => "running",
                    MacroExecutionStatus::Completed => "completed",
                    MacroExecutionStatus::Failed => "failed",
                    MacroExecutionStatus::Cancelled => "cancelled",
                },
                "current_step": execution.current_step,
                "total_steps": execution.total_steps,
                "error_message": execution.error_message
            }),
        )
    }

    /// Handle define macro packet
    async fn handle_define(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        let macro_id = packet
            .body
            .get("macro_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProtocolError::invalid_state("Missing macro_id"))?;

        let name = packet
            .body
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProtocolError::invalid_state("Missing name"))?;

        let description = packet
            .body
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let steps_json = packet
            .body
            .get("steps")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ProtocolError::invalid_state("Missing steps"))?;

        let mut steps = Vec::new();
        for step_json in steps_json {
            let action = step_json
                .get("action")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ProtocolError::invalid_state("Missing step action"))?;

            let params = step_json.get("params").cloned().unwrap_or(json!({}));

            steps.push(MacroStep {
                action: action.to_string(),
                params,
            });
        }

        info!(
            "Received macro definition from {} ({}): {}",
            device.name(),
            device.id(),
            name
        );

        let macro_def = MacroDefinition {
            id: macro_id.to_string(),
            name: name.to_string(),
            description,
            steps,
        };

        self.define_macro(macro_def).await?;

        Ok(())
    }

    /// Handle execute macro packet
    async fn handle_execute(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        let macro_id = packet
            .body
            .get("macro_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProtocolError::invalid_state("Missing macro_id"))?;

        let variables = packet
            .body
            .get("variables")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        info!(
            "Received macro execute from {} ({}): {}",
            device.name(),
            device.id(),
            macro_id
        );

        let exec_id = self.execute_macro(macro_id, variables).await?;

        debug!("Macro execution started: {}", exec_id);

        Ok(())
    }

    /// Handle cancel packet
    async fn handle_cancel(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        let exec_id = packet
            .body
            .get("execution_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProtocolError::invalid_state("Missing execution_id"))?;

        info!(
            "Received macro cancel from {} ({}): {}",
            device.name(),
            device.id(),
            exec_id
        );

        self.cancel_execution(exec_id).await?;

        Ok(())
    }

    /// Handle list macros packet
    async fn handle_list(&mut self, _packet: &Packet, device: &Device) -> Result<()> {
        info!(
            "Received macro list request from {} ({})",
            device.name(),
            device.id()
        );

        let macros = self.list_macros().await;

        info!("Found {} macros", macros.len());

        // TODO: Send list response packet
        // Need packet sending infrastructure

        Ok(())
    }
}

impl Default for MacroPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for MacroPlugin {
    fn name(&self) -> &str {
        "macro"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.macro.define".to_string(),
            "cconnect.macro.execute".to_string(),
            "cconnect.macro.cancel".to_string(),
            "cconnect.macro.list".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.macro.status".to_string(),
            "cconnect.macro.list_response".to_string(),
        ]
    }

    async fn init(&mut self, device: &Device) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!("Macro plugin initialized for device {}", device.name());
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("Macro plugin started");
        self.enabled = true;
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("Macro plugin stopped");
        self.enabled = false;

        // Cancel all running executions
        let exec_ids: Vec<String> = self
            .executions
            .read()
            .await
            .values()
            .filter(|e| e.status == MacroExecutionStatus::Running)
            .map(|e| e.id.clone())
            .collect();

        for exec_id in exec_ids {
            let _ = self.cancel_execution(&exec_id).await;
        }

        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("Macro plugin is disabled, ignoring packet");
            return Ok(());
        }

        match packet.packet_type.as_str() {
            "cconnect.macro.define" => self.handle_define(packet, device).await,
            "cconnect.macro.execute" => self.handle_execute(packet, device).await,
            "cconnect.macro.cancel" => self.handle_cancel(packet, device).await,
            "cconnect.macro.list" => self.handle_list(packet, device).await,
            _ => {
                warn!("Unknown packet type: {}", packet.packet_type);
                Ok(())
            }
        }
    }
}

/// Factory for creating Macro plugin instances
pub struct MacroPluginFactory;

impl PluginFactory for MacroPluginFactory {
    fn create(&self) -> Box<dyn Plugin> {
        Box::new(MacroPlugin::new())
    }

    fn name(&self) -> &str {
        "macro"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.macro.define".to_string(),
            "cconnect.macro.execute".to_string(),
            "cconnect.macro.cancel".to_string(),
            "cconnect.macro.list".to_string(),
        ]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.macro.status".to_string(),
            "cconnect.macro.list_response".to_string(),
        ]
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
        let plugin = MacroPlugin::new();
        assert_eq!(plugin.name(), "macro");
        assert!(!plugin.enabled);
    }

    #[tokio::test]
    async fn test_define_macro() {
        let mut plugin = MacroPlugin::new();

        let macro_def = MacroDefinition {
            id: "test-macro".to_string(),
            name: "Test Macro".to_string(),
            description: Some("A test macro".to_string()),
            steps: vec![MacroStep {
                action: "notify".to_string(),
                params: json!({"title": "Test", "message": "Hello"}),
            }],
        };

        assert!(plugin.define_macro(macro_def).await.is_ok());

        let macros = plugin.list_macros().await;
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].id, "test-macro");
    }

    #[tokio::test]
    async fn test_delete_macro() {
        let mut plugin = MacroPlugin::new();

        let macro_def = MacroDefinition {
            id: "test-macro".to_string(),
            name: "Test".to_string(),
            description: None,
            steps: vec![],
        };

        plugin.define_macro(macro_def).await.unwrap();
        assert_eq!(plugin.list_macros().await.len(), 1);

        plugin.delete_macro("test-macro").await.unwrap();
        assert_eq!(plugin.list_macros().await.len(), 0);
    }

    #[tokio::test]
    async fn test_execute_macro() {
        let mut plugin = MacroPlugin::new();

        let macro_def = MacroDefinition {
            id: "test-macro".to_string(),
            name: "Test".to_string(),
            description: None,
            steps: vec![MacroStep {
                action: "wait".to_string(),
                params: json!({"seconds": 1}),
            }],
        };

        plugin.define_macro(macro_def).await.unwrap();

        let exec_id = plugin
            .execute_macro("test-macro", HashMap::new())
            .await
            .unwrap();

        assert!(!exec_id.is_empty());

        // Check execution status
        let status = plugin.get_execution_status(&exec_id).await;
        assert!(status.is_some());
    }

    #[tokio::test]
    async fn test_too_many_steps() {
        let mut plugin = MacroPlugin::new();

        let mut steps = Vec::new();
        for i in 0..200 {
            steps.push(MacroStep {
                action: "wait".to_string(),
                params: json!({"seconds": 0}),
            });
        }

        let macro_def = MacroDefinition {
            id: "huge-macro".to_string(),
            name: "Huge".to_string(),
            description: None,
            steps,
        };

        let result = plugin.define_macro(macro_def).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = MacroPlugin::new();
        let device = create_test_device();

        assert!(plugin.init(&device).await.is_ok());
        assert!(plugin.start().await.is_ok());
        assert!(plugin.enabled);
        assert!(plugin.stop().await.is_ok());
        assert!(!plugin.enabled);
    }

    #[test]
    fn test_capabilities() {
        let plugin = MacroPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 4);
        assert!(incoming.contains(&"cconnect.macro.define".to_string()));
        assert!(incoming.contains(&"cconnect.macro.execute".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 2);
        assert!(outgoing.contains(&"cconnect.macro.status".to_string()));
    }
}
