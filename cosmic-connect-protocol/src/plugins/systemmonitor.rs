//! System Monitor Plugin
//!
//! Provides real-time system monitoring capabilities for remote desktop machines.
//! Allows viewing CPU, memory, disk, network statistics, and process lists.
//!
//! ## Protocol
//!
//! **Packet Types**:
//! - `cconnect.systemmonitor.request` - Request system statistics
//! - `cconnect.systemmonitor.stats` - System statistics response
//! - `cconnect.systemmonitor.processes` - Process list response
//!
//! **Capabilities**:
//! - Incoming: `cconnect.systemmonitor.request`
//! - Outgoing: `cconnect.systemmonitor.stats`, `cconnect.systemmonitor.processes`
//!
//! ## Packet Formats
//!
//! ### Request System Statistics
//!
//! ```json
//! {
//!     "id": 1234567890,
//!     "type": "cconnect.systemmonitor.request",
//!     "body": {
//!         "requestType": "stats"
//!     }
//! }
//! ```
//!
//! ### Statistics Response
//!
//! ```json
//! {
//!     "id": 1234567891,
//!     "type": "cconnect.systemmonitor.stats",
//!     "body": {
//!         "cpu": {
//!             "usage": 45.2,
//!             "cores": [12.3, 45.6, 78.9, 34.5]
//!         },
//!         "memory": {
//!             "total": 16777216000,
//!             "used": 8388608000,
//!             "available": 8388608000,
//!             "usagePercent": 50.0
//!         },
//!         "disk": [
//!             {
//!                 "mountPoint": "/",
//!                 "total": 500000000000,
//!                 "used": 250000000000,
//!                 "available": 250000000000,
//!                 "usagePercent": 50.0
//!             }
//!         ],
//!         "network": {
//!             "bytesReceived": 1234567890,
//!             "bytesSent": 987654321
//!         },
//!         "uptime": 86400
//!     }
//! }
//! ```
//!
//! ### Request Process List
//!
//! ```json
//! {
//!     "id": 1234567892,
//!     "type": "cconnect.systemmonitor.request",
//!     "body": {
//!         "requestType": "processes",
//!         "limit": 10
//!     }
//! }
//! ```
//!
//! ### Process List Response
//!
//! ```json
//! {
//!     "id": 1234567893,
//!     "type": "cconnect.systemmonitor.processes",
//!     "body": {
//!         "processes": [
//!             {
//!                 "pid": 1234,
//!                 "name": "firefox",
//!                 "cpu": 12.5,
//!                 "memory": 1073741824
//!             }
//!         ]
//!     }
//! }
//! ```
//!
//! ## Use Cases
//!
//! - Monitor remote desktop system resources
//! - View CPU and memory usage
//! - Check disk space availability
//! - Monitor network traffic
//! - Identify resource-intensive processes
//!
//! ## Platform Support
//!
//! - **Linux**: Full support via /proc filesystem
//! - **macOS**: Limited support (minimal stats)
//! - **Windows**: Limited support (minimal stats)

use crate::{Device, Packet, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use super::{Plugin, PluginFactory};

/// System Monitor plugin for viewing remote system resources
///
/// Handles `cconnect.systemmonitor.*` packets for system monitoring.
#[derive(Debug)]
pub struct SystemMonitorPlugin {
    /// Device ID this plugin is attached to
    device_id: Option<String>,

    /// Whether the plugin is enabled
    enabled: bool,
}

impl SystemMonitorPlugin {
    /// Create a new SystemMonitor plugin
    pub fn new() -> Self {
        Self {
            device_id: None,
            enabled: true,
        }
    }

    /// Collect current system statistics
    ///
    /// Gathers CPU, memory, disk, network, and uptime information.
    fn collect_system_stats(&self) -> Value {
        #[cfg(target_os = "linux")]
        {
            json!({
                "cpu": self.get_cpu_usage(),
                "memory": self.get_memory_info(),
                "disk": self.get_disk_info(),
                "network": self.get_network_info(),
                "uptime": self.get_uptime(),
            })
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Minimal stats for non-Linux platforms
            json!({
                "cpu": { "usage": 0.0, "cores": [] },
                "memory": { "total": 0, "used": 0, "available": 0, "usagePercent": 0.0 },
                "disk": [],
                "network": { "bytesReceived": 0, "bytesSent": 0 },
                "uptime": 0,
            })
        }
    }

    #[cfg(target_os = "linux")]
    fn get_cpu_usage(&self) -> Value {
        use std::fs;

        if let Ok(stat_content) = fs::read_to_string("/proc/stat") {
            let lines: Vec<&str> = stat_content.lines().collect();

            let mut cores = Vec::new();
            let mut total_usage = 0.0;
            let mut core_count = 0;

            for line in lines.iter() {
                if line.starts_with("cpu") && !line.starts_with("cpu ") {
                    if let Some(usage) = self.parse_cpu_line(line) {
                        cores.push(usage);
                        total_usage += usage;
                        core_count += 1;
                    }
                }
            }

            if core_count > 0 {
                total_usage /= core_count as f64;
            }

            return json!({
                "usage": (total_usage * 100.0).round() / 100.0,
                "cores": cores.iter().map(|u| (u * 100.0).round() / 100.0).collect::<Vec<f64>>(),
            });
        }

        json!({ "usage": 0.0, "cores": [] })
    }

    #[cfg(target_os = "linux")]
    fn parse_cpu_line(&self, line: &str) -> Option<f64> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            return None;
        }

        let user: u64 = parts.get(1)?.parse().ok()?;
        let nice: u64 = parts.get(2)?.parse().ok()?;
        let system: u64 = parts.get(3)?.parse().ok()?;
        let idle: u64 = parts.get(4)?.parse().ok()?;

        let total = user + nice + system + idle;
        let active = user + nice + system;

        if total == 0 {
            return Some(0.0);
        }

        Some(active as f64 / total as f64)
    }

    #[cfg(target_os = "linux")]
    fn get_memory_info(&self) -> Value {
        use std::fs;

        if let Ok(meminfo_content) = fs::read_to_string("/proc/meminfo") {
            let mut mem_total = 0u64;
            let mut mem_available = 0u64;

            for line in meminfo_content.lines() {
                if line.starts_with("MemTotal:") {
                    if let Some(value) = line.split_whitespace().nth(1) {
                        mem_total = value.parse().unwrap_or(0) * 1024;
                    }
                } else if line.starts_with("MemAvailable:") {
                    if let Some(value) = line.split_whitespace().nth(1) {
                        mem_available = value.parse().unwrap_or(0) * 1024;
                    }
                }
            }

            let mem_used = mem_total.saturating_sub(mem_available);
            let usage_percent = if mem_total > 0 {
                (mem_used as f64 / mem_total as f64) * 100.0
            } else {
                0.0
            };

            return json!({
                "total": mem_total,
                "used": mem_used,
                "available": mem_available,
                "usagePercent": (usage_percent * 100.0).round() / 100.0,
            });
        }

        json!({ "total": 0, "used": 0, "available": 0, "usagePercent": 0.0 })
    }

    #[cfg(target_os = "linux")]
    fn get_disk_info(&self) -> Value {
        use std::collections::HashSet;
        use std::fs;

        let mut disks = Vec::new();
        let mut seen_devices = HashSet::new();

        if let Ok(mounts_content) = fs::read_to_string("/proc/mounts") {
            for line in mounts_content.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 3 {
                    continue;
                }

                let device = parts[0];
                let mount_point = parts[1];
                let fs_type = parts[2];

                // Skip non-physical filesystems
                if !device.starts_with("/dev/") || fs_type == "squashfs" || fs_type == "tmpfs" {
                    continue;
                }

                if seen_devices.contains(device) {
                    continue;
                }
                seen_devices.insert(device.to_string());

                // Get disk usage using statvfs
                if let Ok(stat) = nix::sys::statvfs::statvfs(mount_point) {
                    let block_size = stat.block_size();
                    let total = stat.blocks() * block_size;
                    let available = stat.blocks_available() * block_size;
                    let used = total - available;

                    let usage_percent = if total > 0 {
                        (used as f64 / total as f64) * 100.0
                    } else {
                        0.0
                    };

                    disks.push(json!({
                        "mountPoint": mount_point,
                        "total": total,
                        "used": used,
                        "available": available,
                        "usagePercent": (usage_percent * 100.0).round() / 100.0,
                    }));
                }
            }
        }

        json!(disks)
    }

    #[cfg(target_os = "linux")]
    fn get_network_info(&self) -> Value {
        use std::fs;

        if let Ok(netdev_content) = fs::read_to_string("/proc/net/dev") {
            let mut total_received = 0u64;
            let mut total_sent = 0u64;

            for line in netdev_content.lines().skip(2) {
                if let Some((iface, stats)) = line.split_once(':') {
                    let iface = iface.trim();

                    if iface == "lo" {
                        continue;
                    }

                    let parts: Vec<&str> = stats.split_whitespace().collect();
                    if parts.len() >= 9 {
                        if let Ok(rx) = parts[0].parse::<u64>() {
                            total_received += rx;
                        }
                        if let Ok(tx) = parts[8].parse::<u64>() {
                            total_sent += tx;
                        }
                    }
                }
            }

            return json!({
                "bytesReceived": total_received,
                "bytesSent": total_sent,
            });
        }

        json!({ "bytesReceived": 0, "bytesSent": 0 })
    }

    #[cfg(target_os = "linux")]
    fn get_uptime(&self) -> u64 {
        use std::fs;

        if let Ok(uptime_content) = fs::read_to_string("/proc/uptime") {
            if let Some(uptime_str) = uptime_content.split_whitespace().next() {
                if let Ok(uptime_float) = uptime_str.parse::<f64>() {
                    return uptime_float as u64;
                }
            }
        }
        0
    }

    /// Collect top processes by resource usage
    fn collect_process_list(&self, limit: usize) -> Value {
        #[cfg(target_os = "linux")]
        {
            use std::fs;

            let mut processes = Vec::new();

            if let Ok(entries) = fs::read_dir("/proc") {
                for entry in entries.flatten() {
                    if let Ok(file_name) = entry.file_name().into_string() {
                        if let Ok(pid) = file_name.parse::<u32>() {
                            if let Some(process_info) = self.get_process_info(pid) {
                                processes.push(process_info);
                            }
                        }
                    }
                }
            }

            // Sort by CPU usage (descending)
            processes.sort_by(|a, b| {
                let cpu_a = a["cpu"].as_f64().unwrap_or(0.0);
                let cpu_b = b["cpu"].as_f64().unwrap_or(0.0);
                cpu_b
                    .partial_cmp(&cpu_a)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            processes.truncate(limit);

            json!({ "processes": processes })
        }

        #[cfg(not(target_os = "linux"))]
        {
            json!({ "processes": [] })
        }
    }

    #[cfg(target_os = "linux")]
    fn get_process_info(&self, pid: u32) -> Option<Value> {
        use std::fs;

        let stat_path = format!("/proc/{}/stat", pid);
        let stat_content = fs::read_to_string(&stat_path).ok()?;

        let start = stat_content.find('(')?;
        let end = stat_content.rfind(')')?;
        let name = &stat_content[start + 1..end];

        let stats_part = &stat_content[end + 2..];
        let parts: Vec<&str> = stats_part.split_whitespace().collect();

        let statm_path = format!("/proc/{}/statm", pid);
        let memory = if let Ok(statm_content) = fs::read_to_string(&statm_path) {
            if let Some(rss) = statm_content.split_whitespace().nth(1) {
                rss.parse::<u64>().unwrap_or(0) * 4096
            } else {
                0
            }
        } else {
            0
        };

        let utime: u64 = parts.get(11)?.parse().ok()?;
        let stime: u64 = parts.get(12)?.parse().ok()?;
        let total_time = utime + stime;

        let cpu_percent = (total_time as f64 / 1000.0).min(100.0);

        Some(json!({
            "pid": pid,
            "name": name,
            "cpu": (cpu_percent * 100.0).round() / 100.0,
            "memory": memory,
        }))
    }

    /// Handle system monitor request
    async fn handle_request(&mut self, packet: &Packet, device: &Device) -> Result<()> {
        debug!("Handling system monitor request from {}", device.name());

        let body = &packet.body;
        let request_type = body
            .get("requestType")
            .and_then(|v| v.as_str())
            .unwrap_or("stats");

        match request_type {
            "stats" => {
                info!("Collecting system statistics for {}", device.name());
                let stats = self.collect_system_stats();

                // Create response packet
                let response = Packet::new("cconnect.systemmonitor.stats", stats);
                debug!(
                    "System stats collected for {}: {:?}",
                    device.name(),
                    response.body
                );

                // In a real implementation, send the response packet through the device connection
                // device.send_packet(response).await?;
            }
            "processes" => {
                let limit = body.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

                info!("Collecting top {} processes for {}", limit, device.name());
                let process_list = self.collect_process_list(limit);

                let response = Packet::new("cconnect.systemmonitor.processes", process_list);
                debug!(
                    "Process list collected for {}: {:?}",
                    device.name(),
                    response.body
                );

                // device.send_packet(response).await?;
            }
            _ => {
                warn!("Unknown request type: {}", request_type);
            }
        }

        Ok(())
    }
}

impl Default for SystemMonitorPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for SystemMonitorPlugin {
    fn name(&self) -> &str {
        "systemmonitor"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec!["cconnect.systemmonitor.request".to_string()]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.systemmonitor.stats".to_string(),
            "cconnect.systemmonitor.processes".to_string(),
        ]
    }

    async fn init(&mut self, device: &Device) -> Result<()> {
        self.device_id = Some(device.id().to_string());
        info!(
            "SystemMonitor plugin initialized for device {}",
            device.name()
        );
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        info!("SystemMonitor plugin started");
        self.enabled = true;
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        info!("SystemMonitor plugin stopped");
        self.enabled = false;
        Ok(())
    }

    async fn handle_packet(&mut self, packet: &Packet, device: &mut Device) -> Result<()> {
        if !self.enabled {
            debug!("SystemMonitor plugin is disabled, ignoring packet");
            return Ok(());
        }

        match packet.packet_type.as_str() {
            "cconnect.systemmonitor.request" => self.handle_request(packet, device).await,
            _ => {
                warn!("Unknown packet type: {}", packet.packet_type);
                Ok(())
            }
        }
    }
}

/// Factory for creating SystemMonitorPlugin instances
#[derive(Debug, Clone, Copy)]
pub struct SystemMonitorPluginFactory;

impl PluginFactory for SystemMonitorPluginFactory {
    fn name(&self) -> &str {
        "systemmonitor"
    }

    fn incoming_capabilities(&self) -> Vec<String> {
        vec!["cconnect.systemmonitor.request".to_string()]
    }

    fn outgoing_capabilities(&self) -> Vec<String> {
        vec![
            "cconnect.systemmonitor.stats".to_string(),
            "cconnect.systemmonitor.processes".to_string(),
        ]
    }

    fn create(&self) -> Box<dyn Plugin> {
        Box::new(SystemMonitorPlugin::new())
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
        let plugin = SystemMonitorPlugin::new();
        assert_eq!(plugin.name(), "systemmonitor");
        assert!(plugin.enabled);
    }

    #[test]
    fn test_capabilities() {
        let plugin = SystemMonitorPlugin::new();

        let incoming = plugin.incoming_capabilities();
        assert_eq!(incoming.len(), 1);
        assert!(incoming.contains(&"cconnect.systemmonitor.request".to_string()));

        let outgoing = plugin.outgoing_capabilities();
        assert_eq!(outgoing.len(), 2);
        assert!(outgoing.contains(&"cconnect.systemmonitor.stats".to_string()));
        assert!(outgoing.contains(&"cconnect.systemmonitor.processes".to_string()));
    }

    #[tokio::test]
    async fn test_plugin_lifecycle() {
        let mut plugin = SystemMonitorPlugin::new();
        let device = create_test_device();

        plugin.init(&device).await.unwrap();
        assert!(plugin.device_id.is_some());

        plugin.start().await.unwrap();
        assert!(plugin.enabled);

        plugin.stop().await.unwrap();
        assert!(!plugin.enabled);
    }

    #[tokio::test]
    async fn test_handle_stats_request() {
        let mut plugin = SystemMonitorPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();
        plugin.start().await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.systemmonitor.request",
            json!({
                "requestType": "stats"
            }),
        );

        let result = plugin.handle_packet(&packet, &mut device).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_processes_request() {
        let mut plugin = SystemMonitorPlugin::new();
        let device = create_test_device();
        plugin.init(&device).await.unwrap();
        plugin.start().await.unwrap();

        let mut device = create_test_device();
        let packet = Packet::new(
            "cconnect.systemmonitor.request",
            json!({
                "requestType": "processes",
                "limit": 5
            }),
        );

        let result = plugin.handle_packet(&packet, &mut device).await;
        assert!(result.is_ok());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_collect_system_stats() {
        let plugin = SystemMonitorPlugin::new();
        let stats = plugin.collect_system_stats();

        assert!(stats.get("cpu").is_some());
        assert!(stats.get("memory").is_some());
        assert!(stats.get("disk").is_some());
        assert!(stats.get("network").is_some());
        assert!(stats.get("uptime").is_some());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_collect_process_list() {
        let plugin = SystemMonitorPlugin::new();
        let processes = plugin.collect_process_list(5);

        assert!(processes.get("processes").is_some());
        assert!(processes["processes"].is_array());
    }

    #[test]
    fn test_factory() {
        let factory = SystemMonitorPluginFactory;
        assert_eq!(factory.name(), "systemmonitor");

        let plugin = factory.create();
        assert_eq!(plugin.name(), "systemmonitor");
    }
}
