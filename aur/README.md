# COSMIC Connect - AUR Package

This directory contains the AUR (Arch User Repository) package files for COSMIC Connect.

## Package Information

- **Package Name**: `cosmic-connect`
- **Description**: Device connectivity for COSMIC Desktop
- **License**: GPL-3.0-or-later
- **URL**: https://github.com/olafkfreund/cosmic-connect-desktop-app
- **AUR URL**: https://aur.archlinux.org/packages/cosmic-connect (pending)

## Files

- `PKGBUILD` - Main package build script
- `.SRCINFO` - AUR metadata (generated from PKGBUILD)
- `cosmic-connect.install` - Post-install hooks
- `cosmic-connect-daemon.service` - Systemd user service
- `README.md` - This file

## Building the Package

### Prerequisites

Install the base-devel group and rust toolchain:

```bash
sudo pacman -S base-devel rust
```

### Build Steps

1. Clone the AUR repository (once published):
   ```bash
   git clone https://aur.archlinux.org/cosmic-connect.git
   cd cosmic-connect
   ```

2. Build and install:
   ```bash
   makepkg -si
   ```

### Local Testing

To test the package locally before publishing:

```bash
cd /path/to/cosmic-connect-desktop-app/aur
makepkg -si
```

## Updating the Package

### For New Releases

1. Update `pkgver` in PKGBUILD
2. Reset `pkgrel` to 1
3. Update checksums:
   ```bash
   updpkgsums
   ```
4. Regenerate .SRCINFO:
   ```bash
   makepkg --printsrcinfo > .SRCINFO
   ```
5. Test build:
   ```bash
   makepkg -si
   ```
6. Commit and push to AUR:
   ```bash
   git add PKGBUILD .SRCINFO
   git commit -m "Update to version X.Y.Z"
   git push
   ```

### For Package Fixes

1. Increment `pkgrel` in PKGBUILD
2. Make necessary changes
3. Update .SRCINFO
4. Test and commit

## Dependencies

### Build Dependencies (makedepends)

- `cargo` - Rust package manager
- `git` - Version control (for fetching git dependencies)
- `pkg-config` - Helper tool for compiling
- `rust` - Rust compiler

### Runtime Dependencies (depends)

- `dbus` - Message bus system
- `gcc-libs` - Runtime libraries from GCC
- `glibc` - GNU C Library
- `gstreamer` - Multimedia framework
- `gst-plugins-base` - GStreamer base plugins
- `gst-plugins-good` - GStreamer good plugins
- `gst-plugins-bad` - GStreamer bad plugins (for advanced codecs)
- `gtk3` - GTK3 widget toolkit
- `libpulse` - PulseAudio client library
- `openssl` - Cryptography library
- `pipewire` - Multimedia processing server
- `wayland` - Wayland compositor support
- `webkit2gtk-4.1` - Web content engine for GTK

### Optional Dependencies (optdepends)

- `wireplumber` - PipeWire session manager (recommended for RemoteDesktop)
- `gst-plugins-ugly` - Additional codec support
- `gst-libav` - FFmpeg-based codec support

## Package Components

The package installs the following binaries:

- `cosmic-applet-connect` - COSMIC panel applet for device status
- `cosmic-connect-daemon` - Background service with D-Bus activation
- `cosmic-connect-manager` - Standalone device manager window
- `cosmic-messages` - TUI messaging interface
- `cosmic-messages-popup` - Web-based messaging popup
- `cosmic-display-stream` - Display streaming utility

## Features

Built with the following features enabled:

- RemoteDesktop plugin (requires PipeWire and Wayland)
- Full workspace build (all components)

## Troubleshooting

### Build Failures

If the build fails due to missing dependencies:

```bash
# Check for missing system libraries
namcap PKGBUILD
```

### Service Not Starting

Check the service status:

```bash
systemctl --user status cosmic-connect-daemon.service
journalctl --user -u cosmic-connect-daemon.service
```

### Dependency Issues

Some dependencies (like `libcosmic`) may be in the AUR. Install them first:

```bash
yay -S libcosmic-git  # or your preferred AUR helper
```

## Contributing

To contribute to the AUR package:

1. Test your changes thoroughly
2. Follow [Arch package guidelines](https://wiki.archlinux.org/title/Arch_package_guidelines)
3. Follow [Rust package guidelines](https://wiki.archlinux.org/title/Rust_package_guidelines)
4. Update this README if needed
5. Submit changes via AUR git repository

## Resources

- [Arch Package Guidelines](https://wiki.archlinux.org/title/Arch_package_guidelines)
- [Rust Package Guidelines](https://wiki.archlinux.org/title/Rust_package_guidelines)
- [AUR Submission Guidelines](https://wiki.archlinux.org/title/AUR_submission_guidelines)
- [COSMIC Connect Documentation](https://github.com/olafkfreund/cosmic-connect-desktop-app)

## Maintainer Notes

### Pre-Release Checklist

- [ ] Update `pkgver` in PKGBUILD
- [ ] Run `updpkgsums` to update checksums
- [ ] Regenerate `.SRCINFO`
- [ ] Test build with `makepkg -si`
- [ ] Check binary functionality
- [ ] Verify systemd service works
- [ ] Run `namcap PKGBUILD` for validation
- [ ] Run `namcap cosmic-connect-*.pkg.tar.zst` on built package

### Release Process

1. Wait for upstream GitHub release
2. Update PKGBUILD version and checksums
3. Test build locally
4. Update .SRCINFO
5. Commit with message: "Update to vX.Y.Z"
6. Push to AUR

## License

This PKGBUILD is released under the GPL-3.0-or-later license, matching the upstream project.
