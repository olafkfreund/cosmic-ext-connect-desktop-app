# Plugin Testing Guide

**Purpose:** Systematically test all 12 plugins to verify end-to-end functionality after pairing fix.

**Date:** 2026-01-15
**Test Device:** Pixel 9 Pro XL (Android)
**Desktop:** CD-p620 (COSMIC Desktop, NixOS)

---

## Prerequisites

### 1. Verify Pairing
```bash
# Check daemon is running
ps aux | grep kdeconnect-daemon

# Check app is running
ps aux | grep cosmic-kdeconnect

# Monitor logs in real-time
tail -f /tmp/daemon-debug-verbose.log
```

### 2. Verify Connection
Look for recent packet activity in logs:
```bash
tail -50 /tmp/daemon-debug-verbose.log | grep "Received packet"
```

You should see packets like:
- `kdeconnect.battery` - Battery status updates
- `kdeconnect.identity` - Connection keepalive
- `kdeconnect.ping` - Connectivity tests

---

## Testing Methodology

### For Each Plugin:

1. **Open the app:** `cosmic-kdeconnect` UI
2. **Execute test action** (see plugin-specific instructions below)
3. **Verify result** on target device
4. **Check logs** for errors:
   ```bash
   tail -100 /tmp/daemon-debug-verbose.log | grep -i error
   ```
5. **Document result** ( Pass /  Fail /  Partial)

---

## Plugin Test Instructions

### 1. 🏓 Ping Plugin

**Purpose:** Test basic connectivity and packet delivery

**Desktop → Phone:**
```bash
# Send ping via DBus
busctl --user call com.system76.CosmicKdeConnect \
  /com/system76/CosmicKdeConnect \
  com.system76.CosmicKdeConnect \
  SendPing s "1b7bbb613c0c42bb9a0b80b24d28631d"
```

**Expected:**
- Notification appears on phone: "Ping received from CD-p620"
- Log shows: `Sending ping to device`

**Phone → Desktop:**
- Open KDE Connect app on phone
- Select your desktop device
- Tap "Send Ping"
- Check for notification on desktop

**Pass Criteria:** Bidirectional ping with < 1s latency

---

### 2. 🔋 Battery Plugin

**Purpose:** Sync phone battery status to desktop

**Test:**
1. Open cosmic-kdeconnect app
2. View device details
3. Check battery percentage and charging status

**Expected:**
- Battery % matches phone
- Charging indicator accurate
- Updates in real-time (within 30s)

**Verify in logs:**
```bash
tail -100 /tmp/daemon-debug-verbose.log | grep "kdeconnect.battery"
```

**Pass Criteria:** Accurate battery status within 5% margin

---

### 3.  Notification Plugin

**Purpose:** Sync notifications between devices

**Phone → Desktop:**
1. Send yourself a text message on phone
2. Check if notification appears on desktop
3. Try dismissing it on desktop
4. Verify it syncs to phone

**Desktop → Phone:**
```bash
# Send test notification to phone
busctl --user call com.system76.CosmicKdeConnect \
  /com/system76/CosmicKdeConnect \
  com.system76.CosmicKdeConnect \
  SendNotification ssss \
  "1b7bbb613c0c42bb9a0b80b24d28631d" \
  "Test Notification" \
  "This is a test from desktop" \
  ""
```

**Expected:**
- Notifications appear on both devices
- Dismissal syncs bidirectionally
- Icons/images display correctly

**Pass Criteria:** Bidirectional sync with < 2s latency

---

### 4.  Share/File Transfer Plugin

**Purpose:** Send files between devices

**Phone → Desktop:**
1. Open any file on phone (photo, PDF, etc.)
2. Tap "Share" → Select "KDE Connect"
3. Choose your desktop device
4. File should appear in `~/Downloads/` on desktop

**Desktop → Phone:**
```bash
# Send a test file
busctl --user call com.system76.CosmicKdeConnect \
  /com/system76/CosmicKdeConnect \
  com.system76.CosmicKdeConnect \
  ShareFile ss \
  "1b7bbb613c0c42bb9a0b80b24d28631d" \
  "/path/to/test/file.txt"
```

**Test Cases:**
- Small text file (< 1KB)
- Medium image (1-5MB)
- Large video (> 10MB)
- PDF document

**Expected:**
- Files transfer successfully
- Progress indicators work
- File integrity preserved (check MD5 hashes)

**Pass Criteria:** All file types transfer without corruption

---

### 5.  Clipboard Plugin

**Purpose:** Sync clipboard content

**Phone → Desktop:**
1. Copy text on phone (long-press text → Copy)
2. Paste on desktop (Ctrl+V)
3. Verify content matches

**Desktop → Phone:**
1. Copy text on desktop (Ctrl+C)
2. Paste on phone (long-press → Paste)
3. Verify content matches

**Test Cases:**
- Plain text
- Text with emojis 
- URLs
- Multi-line text

**Expected:**
- Automatic sync (< 1s delay)
- All character types preserved
- No truncation

**Pass Criteria:** Bidirectional sync with special characters

---

### 6.  MPRIS Media Control Plugin

**Purpose:** Control phone media playback from desktop

**Prerequisites:**
- Play music on phone (Spotify, YouTube Music, etc.)

**Test Actions:**
1. Open cosmic-kdeconnect app
2. Find media controls for your device
3. Try: Play/Pause, Next Track, Previous Track
4. Check metadata display (song name, artist, album)

**Via CLI:**
```bash
# Get current media info
busctl --user call com.system76.CosmicKdeConnect \
  /com/system76/CosmicKdeConnect \
  com.system76.CosmicKdeConnect \
  GetMediaInfo s "1b7bbb613c0c42bb9a0b80b24d28631d"
```

**Expected:**
- Commands execute on phone within 1s
- Metadata accurate
- Album art displays (if supported)
- Volume control works

**Pass Criteria:** Full playback control with accurate metadata

---

### 7.  Remote Input Plugin

**Purpose:** Control phone with desktop mouse/keyboard

**Enable Remote Input on Phone:**
1. Open KDE Connect on phone
2. Select your desktop device
3. Enable "Remote input" plugin
4. Grant accessibility permissions if prompted

**Test Mouse:**
```bash
# The daemon should have remote input active when plugin is enabled
# Try moving mouse in the cosmic-kdeconnect app's remote input mode
```

**Test Keyboard:**
```bash
# Type in a text field on phone using desktop keyboard
```

**Test Cases:**
- Mouse cursor movement
- Tap/click actions
- Keyboard typing
- Special keys (Enter, Backspace, Arrow keys)
- Mouse scroll

**Expected:**
- Cursor tracks desktop mouse movements
- Clicks register accurately
- Keyboard input appears in phone apps
- < 100ms input latency

**Pass Criteria:** Usable mouse/keyboard control with low latency

---

### 8.  Find My Phone Plugin

**Purpose:** Make phone ring to locate it

**Test:**
```bash
# Trigger find my phone
busctl --user call com.system76.CosmicKdeConnect \
  /com/system76/CosmicKdeConnect \
  com.system76.CosmicKdeConnect \
  FindMyPhone s "1b7bbb613c0c42bb9a0b80b24d28631d"
```

**Or via app:**
1. Open cosmic-kdeconnect
2. Click "Find My Phone" button for device

**Expected:**
- Phone rings at maximum volume immediately
- Ring bypasses silent/vibrate mode
- Ring stops when acknowledged on phone
- Can trigger multiple times

**Pass Criteria:** Phone rings loud enough to locate

---

### 9. 📞 Telephony/SMS Plugin

**Purpose:** Receive calls and send/receive SMS from desktop

**Test Call Notification:**
1. Call your phone from another device
2. Check if call notification appears on desktop
3. Verify caller ID/contact name

**Test SMS Send:**
```bash
# Send SMS via desktop (if implemented)
busctl --user call com.system76.CosmicKdeConnect \
  /com/system76/CosmicKdeConnect \
  com.system76.CosmicKdeConnect \
  SendSMS sss \
  "1b7bbb613c0c42bb9a0b80b24d28631d" \
  "+1234567890" \
  "Test message from desktop"
```

**Test SMS Receive:**
1. Send SMS to phone from another device
2. Check if it appears on desktop

**Expected:**
- Call notifications with caller info
- SMS send functionality
- SMS receive notifications
- Contact name resolution

**Pass Criteria:** Call notifications and SMS messaging work

---

### 10.  Presenter Plugin

**Purpose:** Use phone as presentation remote

**Prerequisites:**
- Open presentation software (LibreOffice Impress, etc.)
- Start slideshow mode

**Test via Phone:**
1. Open KDE Connect on phone
2. Select Presenter plugin
3. Try next/previous slide buttons

**Expected:**
- Slide navigation works
- Pointer/laser mode available
- Low latency (< 500ms)
- Works with multiple presentation apps

**Pass Criteria:** Reliable slide navigation

---

### 11.  Run Command Plugin

**Purpose:** Execute commands on remote device

**List Available Commands:**
```bash
busctl --user call com.system76.CosmicKdeConnect \
  /com/system76/CosmicKdeConnect \
  com.system76.CosmicKdeConnect \
  ListCommands s "1b7bbb613c0c42bb9a0b80b24d28631d"
```

**Execute Command:**
```bash
# Run a configured command
busctl --user call com.system76.CosmicKdeConnect \
  /com/system76/CosmicKdeConnect \
  com.system76.CosmicKdeConnect \
  RunCommand ss \
  "1b7bbb613c0c42bb9a0b80b24d28631d" \
  "command_key"
```

**Expected:**
- Commands listed correctly
- Execution succeeds
- Output/result visible
- Error handling for failed commands

**Pass Criteria:** Commands execute successfully

---

### 12. 👥 Contacts Plugin (If Implemented)

**Purpose:** Sync contact list

**Test:**
```bash
# Request contact sync
busctl --user call com.system76.CosmicKdeConnect \
  /com/system76/CosmicKdeConnect \
  com.system76.CosmicKdeConnect \
  SyncContacts s "1b7bbb613c0c42bb9a0b80b24d28631d"
```

**Expected:**
- Contacts download from phone
- Searchable on desktop
- Updates sync bidirectionally

**Note:** This plugin may not be fully implemented yet.

**Pass Criteria:** Basic contact sync works

---

## Results Recording

### Test Results Template

Copy this template for each plugin test:

```markdown
## Plugin: [Name]

**Date:** 2026-01-15
**Tester:** [Your Name]
**Result:**  Pass /  Fail /  Partial

### Test Results
- Test Case 1: [Result]
- Test Case 2: [Result]
- Test Case 3: [Result]

### Issues Found
1. [Issue description]
2. [Issue description]

### Log Excerpts
```
[Relevant logs]
```

### Notes
[Additional observations]
```

---

## Common Issues & Troubleshooting

### Device Not Connected
```bash
# Check connection status
tail -50 /tmp/daemon-debug-verbose.log | grep "Connected\|Disconnected"

# Reconnect by triggering discovery
# On phone: KDE Connect → Refresh
```

### Plugin Not Working
```bash
# Check if plugin is enabled in daemon config
cat ~/.config/cosmic/kdeconnect/config.json | grep -A 5 "plugins"

# Check plugin initialization logs
grep "Initialized plugins" /tmp/daemon-debug-verbose.log
```

### Permissions Issues (Remote Input, etc.)
- Grant all requested permissions on phone
- Check Android accessibility settings
- Verify uinput permissions on desktop

### File Transfer Fails
```bash
# Check downloads directory exists
ls -la ~/Downloads/

# Check file permissions
```

---

## Reporting Issues

When you find bugs, create GitHub issues with:

1. **Title:** `[Plugin Name] - Brief Description`
2. **Labels:** `bug`, `plugin`, plugin-specific label
3. **Content:**
   - Steps to reproduce
   - Expected vs actual behavior
   - Log excerpts
   - Screenshots (if applicable)
   - Environment details

**Template:**
```markdown
### Plugin
[Plugin Name]

### Description
[What went wrong]

### Steps to Reproduce
1. Step 1
2. Step 2
3. Step 3

### Expected Behavior
[What should happen]

### Actual Behavior
[What actually happens]

### Logs
```
[Log excerpts]
```

### Environment
- Desktop: CD-p620 / COSMIC / NixOS
- Phone: Pixel 9 Pro XL / Android [version]
- Commit: [git hash]
```

---

## Quick Test Script

For rapid testing, use this script:

```bash
#!/bin/bash
# quick-plugin-test.sh

DEVICE_ID="1b7bbb613c0c42bb9a0b80b24d28631d"

echo "🏓 Testing Ping..."
busctl --user call com.system76.CosmicKdeConnect \
  /com/system76/CosmicKdeConnect \
  com.system76.CosmicKdeConnect \
  SendPing s "$DEVICE_ID"

sleep 2

echo " Testing Find My Phone..."
busctl --user call com.system76.CosmicKdeConnect \
  /com/system76/CosmicKdeConnect \
  com.system76.CosmicKdeConnect \
  FindMyPhone s "$DEVICE_ID"

echo " Quick tests complete. Check phone for results."
```

---

## Success Criteria Summary

**Project is ready for v1.0 when:**
-  All 12 plugins pass tests
-  No critical bugs found
-  Performance acceptable (< 2s latency for most operations)
-  Stable over multiple connection cycles
-  Works with at least 3 different Android devices

**Current Status:** Testing in progress

---

**Next Steps After Testing:**
1. Document all test results
2. Create issues for any bugs found
3. Fix critical bugs
4. Re-test failed plugins
5. Move to infrastructure phase (CI/CD, packaging)
