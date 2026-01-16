# Debugging and Diagnostic Tools

This guide covers the debugging and diagnostic capabilities available in COSMIC Connect for troubleshooting issues and monitoring performance.

## Table of Contents

- [Logging](#logging)
- [Diagnostic Commands](#diagnostic-commands)
- [Performance Metrics](#performance-metrics)
- [Debug Mode](#debug-mode)
- [Common Issues](#common-issues)

---

## Logging

### Log Levels

COSMIC Connect uses structured logging with the following levels:

- **ERROR**: Critical failures that prevent operation
- **WARN**: Issues that don't prevent operation but should be addressed
- **INFO**: General informational messages (default)
- **DEBUG**: Detailed debugging information
- **TRACE**: Very detailed trace-level information

### Daemon Logging

Configure logging via command-line options:

```bash
# Set log level
cosmic-connect-daemon --log-level debug

# Enable JSON structured logging
cosmic-connect-daemon --json-logs

# Disable timestamps
cosmic-connect-daemon --timestamps false

# Combine options
cosmic-connect-daemon --log-level trace --json-logs
```

Environment variable control:

```bash
# Set log level via environment
RUST_LOG=debug cosmic-connect-daemon

# Module-specific logging
RUST_LOG=cosmic_connect_protocol=trace,cosmic_connect_daemon=debug cosmic-connect-daemon
```

### Applet Logging

The applet supports environment variable-based log control:

```bash
# Debug level logging
RUST_LOG=debug cosmic-applet-connect

# Trace level for detailed output
RUST_LOG=trace cosmic-applet-connect
```

### Log Output

Logs include:
- **Timestamp**: When the event occurred
- **Level**: ERROR, WARN, INFO, DEBUG, or TRACE
- **Target**: Module that generated the log
- **File and Line**: Source location
- **Message**: The log content

Example log output:
```
2026-01-15T21:10:30.123Z INFO cosmic_connect_daemon::main@1810: Starting KDE Connect daemon...
2026-01-15T21:10:30.456Z DEBUG cosmic_connect_protocol::discovery@142: Broadcasting discovery packet
```

---

## Diagnostic Commands

COSMIC Connect daemon includes several diagnostic commands for troubleshooting.

### Version Information

Show version and build information:

```bash
# Basic version
cosmic-connect-daemon version

# Detailed build information
cosmic-connect-daemon version --verbose
```

Output includes:
- Version number
- Git commit hash
- Build timestamp
- Rust compiler version
- Protocol version
- Platform and architecture

### List Devices

Show all known devices:

```bash
# Basic list
cosmic-connect-daemon list-devices

# Detailed device information
cosmic-connect-daemon list-devices --verbose
```

Shows:
- Device name and ID
- Connection status (CONNECTED, PAIRED, AVAILABLE)
- Device type
- Last seen timestamp
- Host and port (if available)

### Device Information

Get detailed information about a specific device:

```bash
cosmic-connect-daemon device-info <device-id>
```

Shows:
- Device details (name, ID, type)
- Connection and pairing status
- Trust status
- Network information
- Certificate fingerprint
- Capabilities (incoming and outgoing)

### Test Connectivity

Test connection to a specific device:

```bash
# Test with default 10 second timeout
cosmic-connect-daemon test-connectivity <device-id>

# Custom timeout
cosmic-connect-daemon test-connectivity <device-id> --timeout 30
```

### Dump Configuration

Show current daemon configuration:

```bash
# Basic configuration
cosmic-connect-daemon dump-config

# Include sensitive information (paths, etc.)
cosmic-connect-daemon dump-config --show-sensitive
```

Shows:
- Device configuration (name, type, ID)
- Network settings (ports, intervals)
- Plugin enable/disable status
- File paths (with --show-sensitive)

### Export Logs

Export logs for bug reporting:

```bash
# Export last 1000 lines (default)
cosmic-connect-daemon export-logs

# Custom output file and line count
cosmic-connect-daemon export-logs --output my-logs.txt --lines 5000
```

Note: Currently provides instructions for manual journal extraction:
```bash
journalctl -u cosmic-connect-daemon -n 1000 > cosmic-connect-logs.txt
```

---

## Performance Metrics

### Enable Metrics

Run daemon with metrics collection:

```bash
cosmic-connect-daemon --metrics
```

### View Metrics

Display performance metrics:

```bash
# Show 10 updates with 1 second intervals (default)
cosmic-connect-daemon metrics

# Custom interval and count
cosmic-connect-daemon metrics --interval 5 --count 20

# Continuous monitoring
cosmic-connect-daemon metrics --count 0
```

### Metrics Collected

The following metrics are tracked:

**Uptime**:
- Total daemon uptime in hours/minutes/seconds

**Network**:
- Packets sent and received
- Bytes sent and received
- Throughput (packets/second, bytes/second)

**Devices**:
- Active connections count
- Paired devices count

**Plugins**:
- Plugin invocations count
- Plugin errors count
- Error rate percentage

Note: Metrics integration with runtime is pending (Issue #36).

---

## Debug Mode

### Enable Debug Features

```bash
# Enable packet dumping
cosmic-connect-daemon --dump-packets

# Combined with debug logging
cosmic-connect-daemon --log-level debug --dump-packets --metrics
```

### Packet Dumping

When `--dump-packets` is enabled, all sent and received packets are logged in detail, showing:
- Packet type
- Full packet body (JSON)
- Device ID
- Timestamp

**Warning**: This generates a large amount of log output. Only use for debugging specific issues.

Note: Packet dumping implementation is pending (Issue #36).

---

## Common Issues

### Device Not Found

```bash
# List all known devices
cosmic-connect-daemon list-devices

# Check device registry
cat ~/.local/share/cosmic/cosmic-connect/devices.json
```

### Connection Problems

```bash
# Test connectivity
cosmic-connect-daemon test-connectivity <device-id>

# Check daemon logs
journalctl -u cosmic-connect-daemon --since "5 minutes ago"

# Verify network settings
cosmic-connect-daemon dump-config
```

### Pairing Issues

```bash
# Check device certificate
cosmic-connect-daemon device-info <device-id>

# View pairing-related logs
journalctl -u cosmic-connect-daemon | grep -i pair
```

### Plugin Not Working

```bash
# Verify plugin is enabled
cosmic-connect-daemon dump-config

# Check device capabilities
cosmic-connect-daemon device-info <device-id>

# Monitor plugin activity
RUST_LOG=cosmic_connect_protocol::plugins=debug cosmic-connect-daemon
```

---

## SystemD Integration

### View Daemon Status

```bash
systemctl status cosmic-connect-daemon
```

### View Logs

```bash
# Recent logs
journalctl -u cosmic-connect-daemon

# Follow logs in real-time
journalctl -u cosmic-connect-daemon -f

# Logs since boot
journalctl -u cosmic-connect-daemon -b

# Last 100 lines
journalctl -u cosmic-connect-daemon -n 100
```

### Restart Daemon

```bash
systemctl restart cosmic-connect-daemon
```

---

## Filing Bug Reports

When filing a bug report, include:

1. **Version Information**:
   ```bash
   cosmic-connect-daemon version --verbose
   ```

2. **Configuration**:
   ```bash
   cosmic-connect-daemon dump-config
   ```

3. **Device List**:
   ```bash
   cosmic-connect-daemon list-devices --verbose
   ```

4. **Logs**:
   ```bash
   journalctl -u cosmic-connect-daemon --since "1 hour ago" > logs.txt
   ```

5. **Steps to Reproduce**: Clear description of the issue

6. **Expected vs Actual Behavior**: What should happen vs what actually happens

---

## Future Enhancements

The following debug features are planned (Issue #36):

- [ ] Full metrics integration with runtime
- [ ] Packet capture mode implementation
- [ ] DBus method to expose metrics
- [ ] Log rotation and management
- [ ] Syslog integration
- [ ] Automated bug report generation

---

## See Also

- [User Guide](USER_GUIDE.md) - General usage information
- [Build Fixes](development/Build-Fixes.md) - Build troubleshooting
- [Plugin Testing](PLUGIN_TESTING_GUIDE.md) - Plugin-specific debugging
