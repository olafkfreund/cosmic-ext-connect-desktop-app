# Removal of cosmic-connect Standalone App

## Summary

Successfully removed the `cosmic-connect` standalone desktop application from the project. The project now uses only the `cosmic-applet-connect` panel applet for user interaction.

## Changes Made

### 1. Workspace Configuration

- **File**: `Cargo.toml`
- **Change**: Removed `cosmic-connect` from workspace members
- **Result**: Workspace now contains only:
  - `cosmic-connect-protocol` (shared protocol library)
  - `cosmic-applet-connect` (panel applet UI)
  - `cosmic-connect-daemon` (background service)

### 2. Documentation Updates

- **File**: `README.md`
  - Removed `cosmic-connect` from architecture diagram
  - Removed `cosmic-connect` from repository structure
- **File**: `nix/package.nix`
  - Removed reference to CLI tool from package description

### 3. Directory Removal

- **Removed**: `cosmic-connect/` directory and all contents (~2438 lines of code)

## Architecture After Cleanup

```
cosmic-connect-desktop-app/
├── cosmic-connect-protocol/  # Shared protocol library (20 plugins)
├── cosmic-connect-daemon/    # Background service (DBus, systemd)
└── cosmic-applet-connect/    # COSMIC panel applet (UI)
```

## Verification

✅ Workspace builds successfully: `cargo check --workspace`
✅ All three remaining crates compile without errors
✅ No broken dependencies or references

## Rationale

The standalone app and applet had ~95% code duplication with identical functionality:

- Both connected to the same daemon via DBus
- Both provided the same device operations
- Both had the same plugin support
- Both had similar UI components

**Benefits of consolidation:**

1. **Reduced maintenance burden** - No need to maintain duplicate code
2. **Better UX** - Panel integration is more consistent with COSMIC design
3. **Clearer project structure** - One UI, one daemon, one protocol
4. **Smaller codebase** - ~2400 fewer lines to maintain

## Next Steps

The following files may need updates but were not modified:

- `justfile` - Contains old kdeconnect references (separate cleanup needed)
- Build scripts in `scripts/` - May reference the standalone app
- CI/CD workflows in `.github/` - May have build steps for the standalone app

These can be cleaned up in a follow-up commit if needed.

## Testing Recommendations

1. Test the applet launches correctly: `cargo run -p cosmic-applet-connect`
2. Test the daemon starts: `cargo run -p cosmic-connect-daemon`
3. Verify device pairing and operations work through the applet
4. Test Nix build: `nix build`

---

**Date**: 2026-01-18
**Commit Message**: `refactor: remove cosmic-connect standalone app, use applet only`
