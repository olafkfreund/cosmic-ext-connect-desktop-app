# Submitting to AUR

This guide covers the initial submission of the `cosmic-connect` package to the AUR.

## Prerequisites

1. Create an AUR account at https://aur.archlinux.org/register
2. Add your SSH public key to your AUR account
3. Install required tools:
   ```bash
   sudo pacman -S git base-devel
   ```

## Initial Submission Steps

### 1. Verify Package Locally

Test the package builds correctly:

```bash
cd aur/
makepkg -si
```

Verify the installed package works:

```bash
systemctl --user start cosmic-connect-daemon.service
cosmic-connect-manager
```

### 2. Update Checksums

Generate real checksums (replace SKIP placeholders):

```bash
updpkgsums
```

This will download the source and calculate SHA256 checksums.

### 3. Regenerate .SRCINFO

After any PKGBUILD changes:

```bash
makepkg --printsrcinfo > .SRCINFO
```

### 4. Validate Package

Run validation checks:

```bash
# Check PKGBUILD syntax and conventions
namcap PKGBUILD

# Build the package
makepkg -f

# Check the built package
namcap cosmic-connect-*.pkg.tar.zst
```

Fix any warnings or errors before proceeding.

### 5. Clone AUR Repository

Clone the empty AUR repository for your package:

```bash
git clone ssh://aur@aur.archlinux.org/cosmic-connect.git aur-repo
cd aur-repo
```

### 6. Add Package Files

Copy your package files:

```bash
cp ../PKGBUILD .
cp ../.SRCINFO .
cp ../cosmic-connect.install .
cp ../cosmic-connect-daemon.service .
```

### 7. Initial Commit

Commit and push to AUR:

```bash
git add PKGBUILD .SRCINFO cosmic-connect.install cosmic-connect-daemon.service
git commit -m "Initial import of cosmic-connect version 0.1.0"
git push origin master
```

### 8. Verify Submission

Check your package is visible:
- Visit https://aur.archlinux.org/packages/cosmic-connect
- Verify metadata is correct
- Check package can be installed via AUR helpers

## Post-Submission

### Enable Notifications

1. Go to https://aur.archlinux.org/packages/cosmic-connect
2. Click "Notify" to receive update notifications
3. Enable comment notifications

### Respond to Comments

Monitor and respond to:
- Build issues
- Dependency problems
- Feature requests
- Bug reports

## Updating the Package

When a new version is released:

```bash
cd aur-repo/

# Update version in PKGBUILD
vim PKGBUILD  # Change pkgver, reset pkgrel to 1

# Update checksums
updpkgsums

# Regenerate metadata
makepkg --printsrcinfo > .SRCINFO

# Test build
makepkg -si

# Commit and push
git add PKGBUILD .SRCINFO
git commit -m "Update to version X.Y.Z"
git push
```

## Troubleshooting

### SSH Connection Issues

If you get "Permission denied" errors:

```bash
# Test SSH connection
ssh -T aur@aur.archlinux.org

# Should output: "Hi username! You've successfully authenticated..."
```

Verify your SSH key is added to your AUR account.

### Package Already Exists

If someone else already submitted a package with this name:
- Contact the current maintainer
- Request co-maintainership or orphan adoption
- Or choose a different package name (e.g., cosmic-connect-git)

### Build Failures on AUR

Common issues:
- Missing build dependencies
- Incorrect source URLs
- Network issues during build
- Missing Cargo.lock file

Check build logs and update PKGBUILD accordingly.

## Maintenance Responsibilities

As an AUR maintainer, you should:

1. Keep the package updated with upstream releases
2. Respond to user comments within a reasonable timeframe
3. Fix build issues when dependencies change
4. Mark package as orphaned if you can no longer maintain it

## Resources

- [AUR Submission Guidelines](https://wiki.archlinux.org/title/AUR_submission_guidelines)
- [AUR Package Maintenance](https://wiki.archlinux.org/title/AUR_package_maintenance)
- [Arch Package Guidelines](https://wiki.archlinux.org/title/Arch_package_guidelines)
- [Rust Package Guidelines](https://wiki.archlinux.org/title/Rust_package_guidelines)

## Getting Help

- AUR mailing list: aur-general@lists.archlinux.org
- IRC: #archlinux-aur on Libera.Chat
- Forums: https://bbs.archlinux.org/viewforum.php?id=4
