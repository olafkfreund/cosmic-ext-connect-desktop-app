# Open Interface Integration Tests

This document describes integration tests for the "Open on Phone" DBus interface.

## Overview

The OpenInterface provides DBus methods for sending URLs and files to connected Android devices.

**DBus Details:**
- Service: `com.system76.CosmicConnect`
- Object Path: `/com/system76/CosmicConnect/Open`
- Interface: `com.system76.CosmicConnect.Open`

## Test Requirements

### Prerequisites
1. cosmic-connect-daemon running
2. At least one paired Android device
3. Android device reachable and connected
4. DBus tools installed (`dbus-send`, `gdbus`, or `busctl`)

## Manual Test Cases

### Test 1: Open HTTPS URL on Phone

**Purpose:** Verify URL with allowed scheme is sent successfully

**Steps:**
```bash
# Using busctl
busctl --user call \
  com.system76.CosmicConnect \
  /com/system76/CosmicConnect/Open \
  com.system76.CosmicConnect.Open \
  OpenOnPhone s "https://example.com"

# Using gdbus
gdbus call --session \
  --dest com.system76.CosmicConnect \
  --object-path /com/system76/CosmicConnect/Open \
  --method com.system76.CosmicConnect.Open.OpenOnPhone \
  "https://example.com"
```

**Expected Result:**
- Returns a request ID (packet ID as string)
- Android device opens URL in default browser
- Daemon logs show "Open on phone request for URL: https://example.com"

### Test 2: Open HTTP URL on Phone

**Purpose:** Verify HTTP URLs are accepted

**Steps:**
```bash
busctl --user call \
  com.system76.CosmicConnect \
  /com/system76/CosmicConnect/Open \
  com.system76.CosmicConnect.Open \
  OpenOnPhone s "http://example.com"
```

**Expected Result:**
- Returns request ID
- Android device opens URL
- No security errors

### Test 3: Open Mailto Link

**Purpose:** Verify mailto scheme works

**Steps:**
```bash
busctl --user call \
  com.system76.CosmicConnect \
  /com/system76/CosmicConnect/Open \
  com.system76.CosmicConnect.Open \
  OpenOnPhone s "mailto:test@example.com"
```

**Expected Result:**
- Returns request ID
- Android device opens email client with recipient filled

### Test 4: Open Tel Link

**Purpose:** Verify telephone links work

**Steps:**
```bash
busctl --user call \
  com.system76.CosmicConnect \
  /com/system76/CosmicConnect/Open \
  com.system76.CosmicConnect.Open \
  OpenOnPhone s "tel:+1234567890"
```

**Expected Result:**
- Returns request ID
- Android device opens phone dialer with number filled

### Test 5: Reject JavaScript URL (Security Test)

**Purpose:** Verify malicious URL schemes are rejected

**Steps:**
```bash
busctl --user call \
  com.system76.CosmicConnect \
  /com/system76/CosmicConnect/Open \
  com.system76.CosmicConnect.Open \
  OpenOnPhone s "javascript:alert(1)"
```

**Expected Result:**
- Returns error: "URL scheme not allowed"
- No packet sent to device
- Daemon logs show "URL scheme not allowed: javascript:alert(1)"

### Test 6: Reject Data URL (Security Test)

**Purpose:** Verify data URLs are rejected

**Steps:**
```bash
busctl --user call \
  com.system76.CosmicConnect \
  /com/system76/CosmicConnect/Open \
  com.system76.CosmicConnect.Open \
  OpenOnPhone s "data:text/html,<script>alert(1)</script>"
```

**Expected Result:**
- Returns error: "URL scheme not allowed"
- No packet sent to device

### Test 7: List Open Capable Devices

**Purpose:** Verify device capability detection

**Steps:**
```bash
busctl --user call \
  com.system76.CosmicConnect \
  /com/system76/CosmicConnect/Open \
  com.system76.CosmicConnect.Open \
  ListOpenCapableDevices
```

**Expected Result:**
- Returns array of device IDs
- Only includes paired and reachable devices
- Only includes devices with "cconnect.share.request" capability

### Test 8: Error When No Device Available

**Purpose:** Verify proper error handling when no device is connected

**Steps:**
1. Ensure no devices are connected
2. Run:
```bash
busctl --user call \
  com.system76.CosmicConnect \
  /com/system76/CosmicConnect/Open \
  com.system76.CosmicConnect.Open \
  OpenOnPhone s "https://example.com"
```

**Expected Result:**
- Returns error: "No suitable device found"
- No crash or hang

### Test 9: Open File on Phone (Not Yet Implemented)

**Purpose:** Verify file transfer method returns appropriate error

**Steps:**
```bash
busctl --user call \
  com.system76.CosmicConnect \
  /com/system76/CosmicConnect/Open \
  com.system76.CosmicConnect.Open \
  OpenFileOnPhone ss "/tmp/test.txt" ""
```

**Expected Result:**
- Returns error: "File transfer + open not yet implemented"
- Suggests using share plugin directly

## Automated Test Implementation

When build environment is fixed, implement these tests in `cosmic-connect-daemon/tests/open_interface_integration_tests.rs`:

```rust
#[tokio::test]
async fn test_open_https_url() {
    // Setup: Start daemon, connect mock device
    // Action: Call OpenOnPhone with https URL
    // Assert: Packet sent with correct format
}

#[tokio::test]
async fn test_reject_javascript_url() {
    // Setup: Start daemon
    // Action: Call OpenOnPhone with javascript: URL
    // Assert: Returns error, no packet sent
}

#[tokio::test]
async fn test_list_capable_devices() {
    // Setup: Connect mock devices with different capabilities
    // Action: Call ListOpenCapableDevices
    // Assert: Returns only devices with share capability
}
```

## Verification Checklist

- [ ] HTTPS URLs open in Android browser
- [ ] HTTP URLs open in Android browser
- [ ] Mailto links open email client
- [ ] Tel links open phone dialer
- [ ] SMS links open messaging app
- [ ] FTP links handled appropriately
- [ ] JavaScript URLs rejected
- [ ] Data URLs rejected
- [ ] VBScript URLs rejected
- [ ] About URLs rejected
- [ ] ListOpenCapableDevices returns correct devices
- [ ] Error handling works when no devices available
- [ ] Error handling works when device disconnects mid-request

## Security Considerations

### Allowed URL Schemes
- `http://` - Web content
- `https://` - Secure web content
- `ftp://` - FTP resources
- `ftps://` - Secure FTP resources
- `mailto:` - Email addresses
- `tel:` - Telephone numbers
- `sms:` - SMS messages
- `geo:` - Geographic coordinates
- `file://` - Local files (Android handles permissions)

### Rejected URL Schemes
- `javascript:` - Code execution risk
- `data:` - XSS risk
- `vbscript:` - Code execution risk
- `about:` - Internal browser pages
- Any non-whitelisted scheme

## Performance Tests

### Latency Test
Measure time from DBus call to packet sent:
```bash
time busctl --user call \
  com.system76.CosmicConnect \
  /com/system76/CosmicConnect/Open \
  com.system76.CosmicConnect.Open \
  OpenOnPhone s "https://example.com"
```

**Expected:** < 50ms for local DBus call + packet creation

### Concurrent Requests
Send multiple URLs simultaneously:
```bash
for i in {1..10}; do
  busctl --user call \
    com.system76.CosmicConnect \
    /com/system76/CosmicConnect/Open \
    com.system76.CosmicConnect.Open \
    OpenOnPhone s "https://example.com/$i" &
done
wait
```

**Expected:** All requests handled without errors

## Debugging

### Enable Trace Logging
```bash
RUST_LOG=cosmic_connect_daemon=trace cosmic-connect-daemon
```

### Monitor DBus Traffic
```bash
dbus-monitor --session "interface='com.system76.CosmicConnect.Open'"
```

### Check Daemon Logs
```bash
journalctl --user -u cosmic-connect-daemon -f
```

## Known Limitations

1. **File transfer not implemented:** `OpenFileOnPhone` returns error
2. **No device selection:** Currently uses first available device
3. **No request tracking:** Request ID returned but not tracked for completion
4. **No response handling:** Android doesn't send success/failure notification

## Future Enhancements

1. Implement file transfer with automatic opening
2. Add device selection parameter to `OpenOnPhone`
3. Add request tracking and completion signals
4. Add response handling when Android implements it
5. Add support for opening specific app types (maps, etc.)
