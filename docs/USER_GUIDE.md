# User Guide

Complete guide to using COSMIC Connect for device synchronization and file sharing.

## Table of Contents

- [Getting Started](#getting-started)
- [First Time Setup](#first-time-setup)
- [Pairing Devices](#pairing-devices)
- [Using Features](#using-features)
- [Managing Devices](#managing-devices)
- [Plugin Guide](#plugin-guide)
- [Troubleshooting](#troubleshooting)

## Getting Started

### Prerequisites

Before using COSMIC Connect, ensure:

1. **Desktop Requirements**
   - COSMIC Desktop Environment installed
   - COSMIC Connect applet installed
   - Daemon running (starts automatically on login)

2. **Mobile Requirements**
   - KDE Connect or COSMIC Connect (Android) app installed on your phone/tablet
   - Both devices connected to the same WiFi network
   - Firewall ports 1714-1764 open (TCP and UDP)

3. **Network Requirements**
   - Both devices on the same local network
   - No VPN blocking local traffic
   - Router allows device-to-device communication

## First Time Setup

### Step 1: Add Applet to Panel

1. Right-click on your COSMIC panel (top bar)
2. Select **"Panel Settings"** or **"Configure Panel"**
3. Click **"Add Applet"** or **"Add Widget"**
4. Scroll to find **"COSMIC Connect"** in the list
5. Click to add it to your panel
6. Close the panel settings

You should now see the COSMIC Connect icon (phone symbol) in your panel.

### Step 2: Start the Daemon

The daemon should start automatically on login. If not:

```bash
# Enable auto-start
systemctl --user enable cosmic-connect-daemon

# Start now
systemctl --user start cosmic-connect-daemon

# Verify it's running
systemctl --user status cosmic-connect-daemon
```

### Step 3: Install Mobile App

#### Android

**Option 1: Google Play Store**
1. Open Play Store
2. Search "KDE Connect" (or "COSMIC Connect" when released)
3. Install the app

**Option 2: F-Droid** (Open Source)
1. Open F-Droid app
2. Search "KDE Connect"
3. Install

#### iOS

1. Open App Store
2. Search "KDE Connect"
3. Install "KDE Connect" by KDE e.V.

## Pairing Devices

### Automatic Discovery

When both devices are on the same network, they should discover each other automatically.

### Pairing Process

#### From Desktop (COSMIC)

1. **Open the Applet**
   - Click the COSMIC Connect icon in your panel
   - A popup will show available devices

2. **Find Your Device**
   - Look for your phone/tablet name in the list under "Available"
   - You should see: `[Device Name] - Not paired`

3. **Initiate Pairing**
   - Click the **"Pair"** button next to your device

4. **Accept on Mobile**
   - A notification will appear on your phone/tablet
   - Tap the notification
   - Tap **"Accept"** in the mobile app

5. **Confirmation**
   - Desktop will show the device under "Connected"
   - Green status indicator confirms successful pairing

#### From Mobile Device

1. **Open Mobile App**
2. **Find Your Desktop**
   - You should see your computer in the "Available devices" list
   - Example: "MyDesktop - COSMIC"

3. **Tap to Pair**
   - Tap on your computer name
   - Tap **"Request pairing"**

4. **Accept on Desktop**
   - A notification will appear on your desktop
   - Click **"Accept"** on the notification or in the applet

### Troubleshooting Discovery

If devices don't appear:

1. **Check Network**: Ensure both are on the same WiFi (no Guest networks).
2. **Check Firewall**: Verify ports 1714-1764 (TCP/UDP) are open.
3. **Restart**: Restart the daemon (`systemctl --user restart cosmic-connect-daemon`) and the mobile app.
4. **Manual Connection**: In the mobile app settings, add device by IP address.

## Using Features

### File Sharing

#### Send File from Desktop to Mobile

1. **Via Applet**:
   - Click COSMIC Connect icon
   - Click **"Send File"** button (document arrow icon) next to your device
   - Browse and select file
   - Click **"Open"**

2. **Via Drag & Drop** (Supported Apps):
   - Drag files onto the device name in the applet (Coming Soon)

#### Send File from Mobile to Desktop

1. Open any app with share functionality (Photos, Files, etc.)
2. Tap the **Share** button
3. Select **"KDE Connect"** / **"COSMIC Connect"**
4. Choose your desktop
5. File appears in `~/Downloads/`

#### Monitor Transfers

- Click the **"Transfer Queue"** button in the applet (arrow icon in Active Transfers section)
- View progress of all active sending/receiving files

### Clipboard Synchronization

Automatically sync clipboard content between devices.

1. **Enable Plugin**: Ensure "Clipboard" plugin is enabled in Device Settings.
2. **Copy on Phone**: Copy text on your phone → immediately paste on desktop.
3. **Copy on Desktop**: Copy text on desktop → long-press paste on phone.

**Note**: For Android 10+, you may need to grant "Draw over other apps" or "Background clipboard access" permission in Android settings.

### Battery Monitoring

View your phone's battery level from your desktop.

- Battery level and charging status appear directly on the device card in the applet.
- Low battery notifications (below 15%) appear on your desktop.

### Notification Mirroring

Receive your phone's notifications on your desktop.

1. **Enable Plugin**: Ensure "Notification" plugin is enabled on both devices.
2. **Grant Permissions**: On Android, grant "Notification Access" permission when prompted.
3. **Use**: Phone notifications appear as native COSMIC notifications. You can reply to messages directly from the notification if supported.

### Media Control (MPRIS)

Control desktop media players from your phone.

1. **Desktop**: Play music/video (Spotify, VLC, Browser).
2. **Mobile**: Open "Multimedia control" in the app.
3. **Control**: Play, pause, skip tracks, and adjust volume remotely.

The desktop applet also shows currently playing media from the phone if the phone supports broadcasting status.

### Remote Input

Use your phone as a touchpad and keyboard for your computer.

1. **Enable Plugin**: Ensure "Remote Input" plugin is enabled.
2. **Mobile**: Tap "Remote Input".
3. **Use**:
   - Swipe screen to move mouse cursor.
   - Tap to click.
   - Two-finger tap to right-click.
   - Two-finger swipe to scroll.
   - Tap keyboard icon to type on desktop.

### Run Commands

Execute predefined desktop commands from your phone.

1. **Setup**:
   - Open applet → Device Details → **Run Commands** settings.
   - Click **"Add Command"**.
   - Enter Name (e.g., "Lock Screen") and Command (e.g., `loginctl lock-session`).
2. **Execute**:
   - Open mobile app → **"Run Command"**.
   - Tap the command to execute it on your desktop.

### Find My Phone

Ring your phone remotely to locate it.

1. Open applet.
2. Click the **"Find Phone"** button (location icon) next to your device.
3. Your phone will ring even if in silent mode.

### Network Share (SFTP)

Mount your phone's filesystem wirelessly.

1. **Enable Plugin**: Ensure "Network Share" or "SFTP" is enabled on mobile.
2. **Mount**: (Integration in progress) Currently automatic upon request from mobile app or via file manager integration.

### Contacts

Synchronize contacts from your phone.

1. **Enable Plugin**: Ensure "Contacts" plugin is enabled.
2. **Sync**: Contacts are synced to the desktop database automatically when connected.

## Managing Devices

### Device Details

Click the **"Details"** button (properties icon) on a device card to view:
- Full device name and ID
- Connection status and IP address
- Protocol version
- Active plugins list

### Settings & Plugins

In **Device Details**, click **"Plugin Settings"** to:
- Enable/Disable specific plugins for that device.
- Configure plugin-specific settings (e.g., Run Commands).
- Rename device (nickname).

### Unpair / Forget

1. Open Applet.
2. Click **"Unpair"** on the device card (or in Device Details).
3. The device will move to "Available" or "Offline" list.

## Plugin Guide

| Plugin | Direction | Description |
|--------|-----------|-------------|
| **Ping** | ↔ | Test connectivity |
| **Battery** | → | Monitor phone battery |
| **Notification** | → | Mirror phone notifications |
| **Share** | ↔ | Share files and URLs |
| **Clipboard** | ↔ | Sync clipboard text |
| **MPRIS** | → | Control desktop media players |
| **Remote Input** | → | Use phone as mouse/keyboard |
| **Run Command** | → | Execute desktop commands |
| **Find Phone** | → | Ring device |
| **Telephony** | → | Call/SMS notifications |
| **Contacts** | → | Sync contacts database |
| **Network Share** | → | Mount phone filesystem |

## Troubleshooting

### Devices Not Finding Each Other
- Ensure both are on **same WiFi**.
- Check if **Firewall** ports 1714-1764 are open.
- Try **Refresh** button in applet.
- Restart daemon: `systemctl --user restart cosmic-connect-daemon`.

### File Transfer Fails
- Check write permissions for `~/Downloads`.
- Ensure mobile screen is on (some phones throttle background apps).

### Clipboard Not Syncing
- Android 10+ restricts background clipboard access. You may need to open the app or enable a specific setting/permission (ADB hack may be required on some devices).

---

**Need more help?** Check [TROUBLESHOOTING.md](TROUBLESHOOTING.md) or open an issue on GitHub.