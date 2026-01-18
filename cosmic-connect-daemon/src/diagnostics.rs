//! Diagnostics and Debug Logging
//!
//! Provides enhanced logging, diagnostic commands, and performance metrics
//! for troubleshooting and debugging the CConnect daemon.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::time::Instant;
use tracing::{info, Level};
use tracing_subscriber::{fmt, EnvFilter};

/// CConnect daemon command-line interface
#[derive(Parser, Debug)]
#[command(name = "cconnect-daemon")]
#[command(about = "CConnect daemon for COSMIC Desktop", long_about = None)]
#[command(version)]
pub struct Cli {
    /// Set log level (error, warn, info, debug, trace)
    #[arg(short, long, value_name = "LEVEL", default_value = "info")]
    pub log_level: String,

    /// Enable JSON structured logging
    #[arg(long)]
    pub json_logs: bool,

    /// Show timestamps in logs
    #[arg(long, default_value = "true")]
    pub timestamps: bool,

    /// Enable packet dumping (debug mode)
    #[arg(long)]
    pub dump_packets: bool,

    /// Enable performance metrics
    #[arg(long)]
    pub metrics: bool,

    /// Diagnostic subcommand
    #[command(subcommand)]
    pub command: Option<DiagnosticCommand>,
}

/// Diagnostic commands for troubleshooting
#[derive(Subcommand, Debug)]
pub enum DiagnosticCommand {
    /// Show daemon version and build information
    Version {
        /// Show detailed build information
        #[arg(long)]
        verbose: bool,
    },

    /// List all known devices
    ListDevices {
        /// Show detailed device information
        #[arg(long)]
        verbose: bool,
    },

    /// Show detailed information about a specific device
    DeviceInfo {
        /// Device ID
        device_id: String,
    },

    /// Test connectivity to a device
    TestConnectivity {
        /// Device ID
        device_id: String,

        /// Timeout in seconds
        #[arg(short, long, default_value = "10")]
        timeout: u64,
    },

    /// Show current configuration
    DumpConfig {
        /// Show sensitive information (certificate paths, etc.)
        #[arg(long)]
        show_sensitive: bool,
    },

    /// Export logs for bug reporting
    ExportLogs {
        /// Output file path
        #[arg(short, long, default_value = "cconnect-logs.txt")]
        output: String,

        /// Include last N lines
        #[arg(short, long, default_value = "1000")]
        lines: usize,
    },

    /// Show performance metrics
    Metrics {
        /// Update interval in seconds
        #[arg(short, long, default_value = "1")]
        interval: u64,

        /// Number of updates (0 = infinite)
        #[arg(short, long, default_value = "10")]
        count: usize,
    },
}

/// Initialize logging based on CLI configuration
pub fn init_logging(cli: &Cli) -> Result<()> {
    let log_level = cli.log_level.parse::<Level>().with_context(|| {
        format!(
            "Invalid log level '{}'. Valid levels: error, warn, info, debug, trace",
            cli.log_level
        )
    })?;

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(log_level.as_str()))
        .context("Failed to create log filter")?;

    // Build base formatter configuration
    let subscriber = fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(true)
        .with_line_number(true);

    // Apply format and timestamp options
    match (cli.json_logs, cli.timestamps) {
        (true, true) => subscriber.json().init(),
        (true, false) => subscriber.without_time().json().init(),
        (false, true) => subscriber.init(),
        (false, false) => subscriber.without_time().init(),
    }

    info!(
        "Logging initialized: level={}, json={}, timestamps={}",
        log_level, cli.json_logs, cli.timestamps
    );

    if cli.dump_packets {
        info!("Packet dumping enabled (debug mode)");
    }

    if cli.metrics {
        info!("Performance metrics enabled");
    }

    Ok(())
}

/// Performance metrics for daemon operations
#[derive(Debug, Default)]
pub struct Metrics {
    /// Daemon start time
    start_time: Option<Instant>,

    /// Total packets sent
    packets_sent: u64,

    /// Total packets received
    packets_received: u64,

    /// Total bytes sent
    bytes_sent: u64,

    /// Total bytes received
    bytes_received: u64,

    /// Number of active connections
    active_connections: usize,

    /// Number of paired devices
    paired_devices: usize,

    /// Total plugin invocations
    plugin_invocations: u64,

    /// Total plugin errors
    plugin_errors: u64,
}

impl Metrics {
    /// Create new metrics instance
    pub fn new() -> Self {
        Self {
            start_time: Some(Instant::now()),
            ..Default::default()
        }
    }

    /// Record a sent packet
    pub fn record_packet_sent(&mut self, size: usize) {
        self.packets_sent += 1;
        self.bytes_sent += size as u64;
    }

    /// Record a received packet
    pub fn record_packet_received(&mut self, size: usize) {
        self.packets_received += 1;
        self.bytes_received += size as u64;
    }

    /// Update connection count
    pub fn update_connections(&mut self, count: usize) {
        self.active_connections = count;
    }

    /// Update paired device count
    pub fn update_paired_devices(&mut self, count: usize) {
        self.paired_devices = count;
    }

    /// Record plugin invocation
    pub fn record_plugin_invocation(&mut self) {
        self.plugin_invocations += 1;
    }

    /// Record plugin error
    pub fn record_plugin_error(&mut self) {
        self.plugin_errors += 1;
    }

    /// Get uptime in seconds
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time
            .map(|start| start.elapsed().as_secs())
            .unwrap_or(0)
    }

    /// Get packets per second (averaged over uptime)
    pub fn packets_per_second(&self) -> f64 {
        let uptime = self.uptime_seconds();
        if uptime > 0 {
            (self.packets_sent + self.packets_received) as f64 / uptime as f64
        } else {
            0.0
        }
    }

    /// Get bandwidth in bytes per second (averaged over uptime)
    pub fn bandwidth_bps(&self) -> f64 {
        let uptime = self.uptime_seconds();
        if uptime > 0 {
            (self.bytes_sent + self.bytes_received) as f64 / uptime as f64
        } else {
            0.0
        }
    }

    /// Display metrics summary
    pub fn display(&self) {
        let uptime = self.uptime_seconds();
        let hours = uptime / 3600;
        let minutes = (uptime % 3600) / 60;
        let seconds = uptime % 60;

        println!("\n=== CConnect Daemon Metrics ===");
        println!("Uptime: {}h {}m {}s", hours, minutes, seconds);
        println!("\nNetwork:");
        println!(
            "  Packets: {} sent, {} received",
            self.packets_sent, self.packets_received
        );
        println!(
            "  Bytes: {} sent, {} received",
            format_bytes(self.bytes_sent),
            format_bytes(self.bytes_received)
        );
        println!(
            "  Throughput: {:.2} packets/s, {}/s",
            self.packets_per_second(),
            format_bytes(self.bandwidth_bps() as u64)
        );
        println!("\nDevices:");
        println!("  Active connections: {}", self.active_connections);
        println!("  Paired devices: {}", self.paired_devices);
        println!("\nPlugins:");
        println!("  Invocations: {}", self.plugin_invocations);
        println!("  Errors: {}", self.plugin_errors);
        if self.plugin_invocations > 0 {
            println!(
                "  Error rate: {:.2}%",
                (self.plugin_errors as f64 / self.plugin_invocations as f64) * 100.0
            );
        }
        println!();
    }

    // Getter methods for encapsulated fields

    /// Get total packets sent
    pub fn packets_sent(&self) -> u64 {
        self.packets_sent
    }

    /// Get total packets received
    pub fn packets_received(&self) -> u64 {
        self.packets_received
    }

    /// Get total bytes sent
    pub fn bytes_sent(&self) -> u64 {
        self.bytes_sent
    }

    /// Get total bytes received
    pub fn bytes_received(&self) -> u64 {
        self.bytes_received
    }

    /// Get number of active connections
    pub fn active_connections(&self) -> usize {
        self.active_connections
    }

    /// Get number of paired devices
    pub fn paired_devices(&self) -> usize {
        self.paired_devices
    }

    /// Get total plugin invocations
    pub fn plugin_invocations(&self) -> u64 {
        self.plugin_invocations
    }

    /// Get total plugin errors
    pub fn plugin_errors(&self) -> u64 {
        self.plugin_errors
    }
}

/// Format bytes in human-readable form
fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_index = 0;

    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.2} {}", value, UNITS[unit_index])
    }
}

/// Build information for diagnostics
pub struct BuildInfo {
    pub version: &'static str,
    pub git_hash: Option<&'static str>,
    pub build_timestamp: &'static str,
    pub rustc_version: &'static str,
}

impl BuildInfo {
    /// Get build information
    pub fn get() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            git_hash: option_env!("GIT_HASH"),
            build_timestamp: env!("BUILD_TIMESTAMP"),
            rustc_version: env!("RUSTC_VERSION"),
        }
    }

    /// Display build information
    pub fn display(&self, verbose: bool) {
        println!("CConnect Daemon v{}", self.version);

        if verbose {
            if let Some(hash) = self.git_hash {
                println!("Git commit: {}", hash);
            }
            println!("Build time: {}", self.build_timestamp);
            println!("Rust compiler: {}", self.rustc_version);
            println!("Protocol version: 7");
            println!("Platform: {}", std::env::consts::OS);
            println!("Architecture: {}", std::env::consts::ARCH);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1048576), "1.00 MB");
        assert_eq!(format_bytes(1073741824), "1.00 GB");
    }

    #[test]
    fn test_metrics_new() {
        let metrics = Metrics::new();
        assert_eq!(metrics.packets_sent(), 0);
        assert_eq!(metrics.packets_received(), 0);
        assert_eq!(metrics.bytes_sent(), 0);
        assert_eq!(metrics.bytes_received(), 0);
        assert_eq!(metrics.active_connections(), 0);
        assert_eq!(metrics.paired_devices(), 0);
        assert_eq!(metrics.plugin_invocations(), 0);
        assert_eq!(metrics.plugin_errors(), 0);
        assert!(metrics.uptime_seconds() >= 0);
    }

    #[test]
    fn test_metrics_record_packets() {
        let mut metrics = Metrics::new();

        metrics.record_packet_sent(100);
        assert_eq!(metrics.packets_sent(), 1);
        assert_eq!(metrics.bytes_sent(), 100);

        metrics.record_packet_received(200);
        assert_eq!(metrics.packets_received(), 1);
        assert_eq!(metrics.bytes_received(), 200);
    }

    #[test]
    fn test_metrics_plugin_tracking() {
        let mut metrics = Metrics::new();

        metrics.record_plugin_invocation();
        metrics.record_plugin_invocation();
        metrics.record_plugin_error();

        assert_eq!(metrics.plugin_invocations(), 2);
        assert_eq!(metrics.plugin_errors(), 1);
    }

    #[test]
    fn test_metrics_connections_and_devices() {
        let mut metrics = Metrics::new();

        metrics.update_connections(3);
        assert_eq!(metrics.active_connections(), 3);

        metrics.update_paired_devices(5);
        assert_eq!(metrics.paired_devices(), 5);
    }

    #[test]
    fn test_metrics_calculations() {
        let mut metrics = Metrics::new();

        // Record some data
        metrics.record_packet_sent(100);
        metrics.record_packet_sent(200);
        metrics.record_packet_received(150);

        // Verify totals
        assert_eq!(metrics.packets_sent(), 2);
        assert_eq!(metrics.packets_received(), 1);
        assert_eq!(metrics.bytes_sent(), 300);
        assert_eq!(metrics.bytes_received(), 150);

        // Verify calculations (should be >= 0)
        assert!(metrics.packets_per_second() >= 0.0);
        assert!(metrics.bandwidth_bps() >= 0.0);
    }

    #[test]
    fn test_build_info() {
        let build_info = BuildInfo::get();

        // Verify version is not empty
        assert!(!build_info.version.is_empty());

        // Verify build timestamp is not empty
        assert!(!build_info.build_timestamp.is_empty());

        // Verify rustc version is not empty
        assert!(!build_info.rustc_version.is_empty());
    }
}
