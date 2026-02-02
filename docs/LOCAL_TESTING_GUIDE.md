# COSMIC Connect Local Testing Guide

This guide helps you test COSMIC Connect locally on your COSMIC Desktop before deploying to multiple hosts.

## Prerequisites

- **NixOS** with COSMIC Desktop installed and running
- **Wayland** session active
- **PipeWire** running (for RemoteDesktop plugin)
- **Desktop Portal** (xdg-desktop-portal) running
- **VNC client** installed (TigerVNC recommended for RemoteDesktop testing)

## Quick Start

### 1. Build the Project

```bash
# Enter Nix development shell
nix develop

# Build all components
just build

# Or manually
cargo build
```

This builds:
- `cosmic-connect-daemon` - Background service
- `cosmic-applet-connect` - UI applet for COSMIC panel
- `cosmic-connect` - CLI tool

### 2. Start Local Test Environment

The easiest way to test locally:

```bash
# Make script executable (first time only)
chmod +x run-local-test.sh

# Start daemon and applet
./run-local-test.sh
```

This script will:
1. Create test directories in `/tmp/cosmic-connect-test/`
2. Start daemon with `test-config.toml`
3. Start applet UI in foreground
4. Show logs in `/tmp/cosmic-connect-daemon.log`

### 3. Stop Test Environment

Press `Ctrl+C` in the terminal running the applet. The script will automatically stop the daemon.

## Manual Testing

If you prefer manual control:

### Start Daemon

```bash
# In one terminal
export RUST_LOG=info,cosmic_connect_daemon=debug,cosmic_connect_protocol=debug
export RUST_BACKTRACE=1

./target/debug/cosmic-connect-daemon --config test-config.toml
```

### Start Applet

```bash
# In another terminal
export RUST_LOG=info,cosmic_applet_connect=debug

./target/debug/cosmic-applet-connect
```

### View Logs

```bash
# Follow daemon logs
tail -f /tmp/cosmic-connect-daemon.log

# With debug output
export RUST_LOG=debug
```

## Testing the UI

### Applet Interface

Once the applet is running, you should see:

1. **System Tray Icon** - COSMIC Connect icon in panel
2. **Device List** - Click icon to view paired devices
3. **Plugin Controls** - Per-device plugin toggles
4. **Settings Panel** - Configuration options

### What to Test

#### Basic Functionality

- [ ] Applet appears in COSMIC panel
- [ ] Click applet icon to open menu
- [ ] Device discovery works (if second device available)
- [ ] Device pairing workflow
- [ ] Plugin enable/disable toggles

#### Device Pairing

If you have a second device:

1. Start COSMIC Connect on second device
2. Devices should appear in discovery
3. Click "Pair" button
4. Accept pairing on both devices
5. Verify paired device shows as "Connected"

#### Plugin Testing

Each enabled plugin should appear with controls:

- **Ping** - Test connectivity button
- **Battery** - Shows remote battery status
- **Clipboard** - Sync clipboard between devices
- **MPRIS** - Control media playback
- **Screenshot** - Request screenshot from remote
- **RemoteDesktop** - Start VNC session (see below)

## Testing RemoteDesktop Plugin

The RemoteDesktop plugin enables VNC-based screen sharing between COSMIC Desktop machines.

### Configuration

The `test-config.toml` has RemoteDesktop enabled:

```toml
[plugins]
enable_remotedesktop = true
```

### Testing Procedure

#### 1. Prerequisites

```bash
# Install VNC client
sudo pacman -S tigervnc  # Arch/NixOS
# or
sudo apt install tigervnc-viewer  # Ubuntu/Debian
```

#### 2. Start VNC Session

**Option A: Via UI (when implemented)**
1. Click device in applet
2. Find "RemoteDesktop" plugin
3. Click "Start Session"
4. Note the password shown

**Option B: Via Packet (current)**

Since the UI for RemoteDesktop may not be fully wired yet, you can test via CLI:

```bash
# Send request packet to paired device
./target/debug/cosmic-connect send-packet <device-id> \
  --type "cconnect.remotedesktop.request" \
  --body '{"mode":"control","quality":"medium","fps":30}'

# Check logs for response with password
tail -f /tmp/cosmic-connect-daemon.log | grep -i "remotedesktop\|vnc"
```

#### 3. Connect VNC Client

When you see the response packet in logs:

```json
{
  "type": "cconnect.remotedesktop.response",
  "body": {
    "status": "ready",
    "port": 5900,
    "password": "abc12345",
    "resolution": {"width": 1920, "height": 1080}
  }
}
```

Connect with VNC client:

```bash
# TigerVNC
vncviewer localhost:5900

# Enter password when prompted (from response packet)
```

#### 4. Test Display and Input

In VNC client window:

- [ ] Screen content visible and updating
- [ ] Keyboard input works (type in remote apps)
- [ ] Mouse movement works
- [ ] Mouse clicks work (left, right, middle)
- [ ] Scroll wheel works
- [ ] Display updates at ~30 FPS
- [ ] Low latency (<100ms for input)

#### 5. Test Session Control

```bash
# Stop session
./target/debug/cosmic-connect send-packet <device-id> \
  --type "cconnect.remotedesktop.control" \
  --body '{"action":"stop"}'

# VNC client should disconnect
# Check logs for session cleanup
```

### Expected Behavior

**Success**:
- Desktop Portal permission dialog appears (approve it)
- VNC server starts on port 5900
- Password generated and returned in response
- VNC client connects successfully
- Screen visible in VNC client
- Keyboard/mouse control remote desktop

**Troubleshooting**:

```bash
# Check if VNC server is running
netstat -tuln | grep 5900

# Check daemon logs
tail -100 /tmp/cosmic-connect-daemon.log | grep -i remotedesktop

# Enable verbose VNC logging
export RUST_LOG=cosmic_connect_protocol::plugins::remotedesktop=trace
```

Common issues:

1. **Port already in use**:
   ```
   Error: Failed to bind to 0.0.0.0:5900
   ```
   Solution: Stop other VNC servers or change port in config

2. **No screen capture permission**:
   ```
   Error: No monitors available
   ```
   Solution: Approve Desktop Portal permission dialog

3. **Input not working**:
   - Check VirtualDevice permissions
   - Verify keysym mappings in logs

4. **Poor performance**:
   - Lower quality preset in request
   - Lower FPS (try 15 instead of 30)
   - Check CPU usage

## Inspecting the Daemon

### Check Status

```bash
# Is daemon running?
pgrep -a cosmic-connect-daemon

# What ports is it using?
netstat -tuln | grep -E '1716|1739|5900'

# Check logs
tail -50 /tmp/cosmic-connect-daemon.log
```

### View Device Registry

```bash
# Show paired devices
cat /tmp/cosmic-connect-test/devices.json | jq .

# Or use CLI
./target/debug/cosmic-connect list-devices
```

### Monitor Packets

```bash
# Watch packet traffic (if implemented)
./target/debug/cosmic-connect monitor --device <device-id>
```

## Testing Multiple Devices

To test device-to-device functionality:

### Setup

1. **Host A**: Your development machine (this one)
2. **Host B**: Another COSMIC Desktop machine (VM or physical)

### On Both Hosts

```bash
# Build and install
nix develop
just build

# Start with same configuration
./target/debug/cosmic-connect-daemon --config test-config.toml
```

### Pair Devices

1. Devices should discover each other automatically
2. Use applet UI to pair devices
3. Or use CLI:
   ```bash
   ./target/debug/cosmic-connect pair <device-id>
   ```

### Test RemoteDesktop Between Hosts

**On Host A** (viewing Host B's screen):
```bash
# Request session from Host B
cosmic-connect send-packet <host-b-device-id> \
  --type "cconnect.remotedesktop.request" \
  --body '{"mode":"control","quality":"medium","fps":30}'

# Get password from response
# VNC to Host B
vncviewer <host-b-ip>:5900
```

**On Host B** (viewing Host A's screen):
```bash
# Request session from Host A
cosmic-connect send-packet <host-a-device-id> \
  --type "cconnect.remotedesktop.request" \
  --body '{"mode":"control","quality":"medium","fps":30}'

# Get password from response
# VNC to Host A
vncviewer <host-a-ip>:5900
```

## Performance Testing

### Measure Frame Rate

```bash
# Run streaming example
cargo run --example test_streaming --features remotedesktop

# Expected: 30 FPS target, ~14-20 FPS actual with frame skipping
```

### Measure Latency

Manual test:
1. Start VNC session
2. Type rapidly in VNC client
3. Observe delay until characters appear

Target: <100ms end-to-end

### Monitor Resources

```bash
# CPU and memory usage
watch -n 1 'ps aux | grep cosmic-connect'

# Expected during RemoteDesktop session:
# - CPU: <40% on modern CPU
# - Memory: <200MB for daemon
```

## Configuration Reference

### test-config.toml

Key settings for testing:

```toml
[device]
name = "Test COSMIC Desktop"      # Customize device name
device_type = "desktop"            # desktop, laptop, phone, tablet

[network]
discovery_port = 1716              # UDP discovery
transfer_port_start = 1739         # TCP range start
transfer_port_end = 1764           # TCP range end
discovery_interval = 5             # Broadcast every 5 seconds
device_timeout = 30                # Mark offline after 30 seconds

[transport]
enable_tcp = true                  # TCP/IP over WiFi/Ethernet
enable_bluetooth = false           # Bluetooth (disable for testing)
preference = "TcpPreferred"        # TCP first, then Bluetooth

[plugins]
enable_remotedesktop = true        # Enable RemoteDesktop plugin
# ... other plugins

[paths]
data_dir = "/tmp/cosmic-connect-test/data"
# ... other paths in /tmp for testing
```

### Environment Variables

```bash
# Logging
export RUST_LOG=debug                    # All debug output
export RUST_LOG=cosmic_connect_protocol::plugins::remotedesktop=trace  # Verbose RemoteDesktop

# Debugging
export RUST_BACKTRACE=1                  # Show backtraces on panic
export RUST_BACKTRACE=full               # Full backtraces
```

## Cleaning Up

### Stop Everything

```bash
# Kill daemon
pkill cosmic-connect-daemon

# Kill applet
pkill cosmic-applet-connect

# Kill VNC sessions
pkill -f vncviewer
```

### Remove Test Data

```bash
# Remove test directories
rm -rf /tmp/cosmic-connect-test/

# Remove logs
rm -f /tmp/cosmic-connect-daemon.log
```

## Next Steps

After local testing is successful:

1. **NixOS Module**: Install via NixOS configuration
2. **System Service**: Enable as systemd service
3. **Multi-Host Testing**: Deploy to multiple machines
4. **Network Testing**: Test across WiFi/Ethernet
5. **Security Testing**: Test TLS certificate validation
6. **Performance Testing**: Profile under load

## Troubleshooting

### Applet Not Appearing

```bash
# Check if COSMIC panel is running
pgrep cosmic-panel

# Check applet logs
journalctl --user -f -u cosmic-applet-connect

# Restart COSMIC panel
systemctl --user restart cosmic-panel
```

### Daemon Not Starting

```bash
# Check logs
tail -100 /tmp/cosmic-connect-daemon.log

# Check config syntax
./target/debug/cosmic-connect-daemon --config test-config.toml --check

# Check port conflicts
netstat -tuln | grep 1716
```

### Device Discovery Not Working

```bash
# Check firewall
sudo firewall-cmd --list-all

# Allow UDP 1716
sudo firewall-cmd --add-port=1716/udp

# Check network interface
ip addr show

# Test UDP broadcast manually
# (network debugging tools)
```

### RemoteDesktop Issues

See `cosmic-connect-protocol/src/plugins/remotedesktop/TESTING.md` for comprehensive RemoteDesktop testing guide.

## Getting Help

1. **Check logs** - `/tmp/cosmic-connect-daemon.log`
2. **Enable debug logging** - `RUST_LOG=debug`
3. **Review documentation** - `cosmic-connect-protocol/src/plugins/remotedesktop/README.md`
4. **Report issues** - Include logs, environment, and steps to reproduce

## Reference

- **Main README**: `README.md`
- **RemoteDesktop README**: `cosmic-connect-protocol/src/plugins/remotedesktop/README.md`
- **RemoteDesktop Testing**: `cosmic-connect-protocol/src/plugins/remotedesktop/TESTING.md`
- **Implementation Status**: `cosmic-connect-protocol/src/plugins/remotedesktop/IMPLEMENTATION_STATUS.md`
