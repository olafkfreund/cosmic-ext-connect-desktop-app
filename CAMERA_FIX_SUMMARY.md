# Camera Sharing Fix Summary

Fixes for Issue #139 - Camera sharing breaks when Android reconnects

## Problem 1: Plugin Reinitialization on Reconnect

### Root Cause
When Android reconnects with a new socket, the connection manager was treating this as a full disconnect/reconnect cycle:
1. Old connection received `ConnectionCommand::Close`
2. Connection handler exited and cleaned up
3. Device was marked as disconnected
4. Daemon received `Disconnected` event and called `cleanup_device_plugins()`
5. All plugins (including camera) were destroyed, killing active streams

### Solution
Added a `reconnect` flag to distinguish socket replacement from genuine disconnection:

**Changes Made:**

1. **`cosmic-connect-protocol/src/connection/events.rs`**
   - Added `reconnect: bool` field to `ConnectionEvent::Disconnected`
   - When `true`, indicates socket replacement (plugins should be preserved)

2. **`cosmic-connect-protocol/src/connection/manager.rs`**
   - Added `ConnectionCommand::CloseForReconnect` variant
   - Socket replacement now sends `CloseForReconnect` instead of `Close`
   - Connection handler tracks `is_reconnect` flag
   - Emits `Disconnected` event with `reconnect: true` on socket replacement
   - Emits `Disconnected` event with `reconnect: false` on genuine disconnection

3. **`cosmic-connect-daemon/src/main.rs`**
   - Check `reconnect` field in `Disconnected` event handler
   - Skip `cleanup_device_plugins()` when `reconnect == true`
   - Log "Socket replacement - preserving plugin state" message

4. **Other files updated for compilation:**
   - `cosmic-connect-protocol/src/recovery_coordinator.rs`
   - `cosmic-connect-protocol/src/transport_manager.rs`

### Expected Behavior After Fix
```
Device e7286da6b81348fe94d5325c165ade69 reconnecting from ... - replacing socket (preserving plugins)
Closing connection to e7286da6b81348fe94d5325c165ade69 for socket replacement (preserving plugins)
Socket replacement for e7286da6b81348fe94d5325c165ade69 - preserving plugin state
```

Camera streams continue uninterrupted when Android reconnects.

## Problem 2: Camera Frame Payload Not Received

### Root Cause
The daemon had code to receive camera frame payloads, but with two issues:
1. Used `read_buf()` which doesn't guarantee reading all bytes
2. Comments suggested TLS was needed but not implemented

### Solution
Fixed payload reception to use `read_exact()` for reliable byte reading:

**Changes Made:**

1. **`cosmic-connect-daemon/src/main.rs` (lines 1958-2002)**
   - Changed from `Vec::with_capacity()` + `read_buf()` to `vec![0u8; size]` + `read_exact()`
   - `read_exact()` guarantees all `payload_size` bytes are received
   - Improved error messages and logging
   - Plain TCP is sufficient (matches Android implementation)

### Code Flow
1. Daemon receives `cconnect.camera.frame` packet with `payloadTransferInfo` and `payloadSize`
2. Extracts port from `payloadTransferInfo.port`
3. Spawns async task to connect to Android device's IP on that port
4. Uses `read_exact()` to receive exactly `payloadSize` bytes
5. Calls `camera_plugin.process_camera_frame_payload(packet, payload)`
6. Camera plugin decodes H.264 frame and outputs to V4L2 device

## Files Modified

### Protocol Library
- `cosmic-connect-protocol/src/connection/events.rs` - Added `reconnect` field
- `cosmic-connect-protocol/src/connection/manager.rs` - Socket replacement logic
- `cosmic-connect-protocol/src/recovery_coordinator.rs` - Handle new field
- `cosmic-connect-protocol/src/transport_manager.rs` - Handle new field

### Daemon
- `cosmic-connect-daemon/src/main.rs` - Skip plugin cleanup on reconnect, fix payload reception

## Testing

### Compilation
```bash
nix develop --command cargo check --package cosmic-connect-protocol
nix develop --command cargo check --package cosmic-connect-daemon
```
Both compile successfully.

### Runtime Testing Required
1. **Test reconnection:**
   - Start camera sharing from Android
   - Wait for Android to reconnect (or force reconnect by toggling network)
   - Verify camera stream continues without interruption
   - Check logs for "Socket replacement - preserving plugin state"

2. **Test payload reception:**
   - Start camera sharing from Android
   - Monitor logs for:
     - "Receiving camera frame payload: N bytes from IP:PORT"
     - "Connected to payload port IP:PORT for camera frame"
     - "Received complete camera frame payload: N bytes"
     - "Camera frame payload processed successfully"
   - Verify V4L2 virtual camera receives frames

## Logging

New log messages added:

### Info Level
- "Device {id} reconnecting from {addr} - replacing socket (preserving plugins)"
- "Closing connection to {id} for socket replacement (preserving plugins)"
- "Socket replacement for {id} - plugins preserved"
- "Socket replacement for {id} - preserving plugin state"
- "Connected to payload port {addr} for camera frame"
- "Received complete camera frame payload: {size} bytes"

### Debug Level
- "Camera frame payload processed successfully"

### Error Level
- "Camera plugin not initialized for device {id}"
- "Failed to get camera plugin for device {id}"
- "Failed to receive camera frame payload ({size} bytes): {error}"

## Related Issues
- Issue #139 - Camera sharing broken
- Issue #52 - Socket replacement implementation

## Future Improvements
- Consider TLS for payload transfers if Android implementation requires it
- Add metrics/monitoring for frame drop rate during reconnection
- Implement automatic recovery if camera stream does fail
