# Waydroid Testing Guide

This guide explains how to test `cosmic-connect-android` on your local machine using Waydroid.

## Prerequisites

- Linux machine with Wayland support.
- Waydroid installed and initialized (`sudo waydroid init`).

## Network Configuration

Waydroid needs to be on the same "logical" network as the host to support UDP broadcasts for device discovery.

### 1. Enable UDP/TCP Forwarding

By default, Waydroid uses NAT. You may need to open the following ports on your host firewall:

- **Discovery**: 1814-1864 (UDP/TCP)
- **Transfer**: 1739-1764 (TCP)

### 2. Install the APK

Build the Android app and install the APK into Waydroid:

```bash
waydroid app install app-debug.apk
```

## Running the Tests

1. Start the COSMIC Connect Desktop Daemon on your host:
   ```bash
   just run-daemon
   ```

2. Open the COSMIC Connect app in Waydroid.
3. Your host machine should appear in the device list.
4. Attempt to Pair.

## Troubleshooting

### Discovery Fails
If the host doesn't see Waydroid, try pinging the Waydroid IP from the host:
```bash
waydroid status # Get IP
ping <waydroid_ip>
```

If ping works but discovery doesn't, it's likely a broadcast/multicast filtering issue in the `waydroid-net` bridge. You may need to manually add the device by IP in the Android app.
