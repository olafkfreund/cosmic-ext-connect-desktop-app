# Contributing to COSMIC Connect

Thank you for your interest in contributing to COSMIC Connect! This guide will help you get started.

##  MANDATORY: Pre-Commit Checks

### Before Every Commit - Run BOTH Checks

**Step 1: COSMIC Code Review** (REQUIRED)
```bash
@cosmic-code-reviewer /pre-commit-check
```

**Step 2: Code Simplification** (REQUIRED)
```bash
@code-simplifier review the changes we made
```

These checks are **mandatory** before any commit. They catch:
- Hard-coded values (colors, dimensions, radii)
- Unsafe error handling (`.unwrap()`, `.expect()`)
- COSMIC Desktop pattern violations
- Code quality issues
- Redundant patterns

**Exception:** Skip only for trivial changes (typo fixes, comments only).

See [CLAUDE.md](CLAUDE.md) for detailed pre-commit workflow.

---

## Table of Contents

- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Claude Code Skill](#claude-code-skill)
- [Code Style](#code-style)
- [Commit Guidelines](#commit-guidelines)
- [Pull Request Process](#pull-request-process)
- [Testing](#testing)
- [Documentation](#documentation)

## Getting Started

COSMIC Connect is a device connectivity solution for COSMIC Desktop, implementing the KDE Connect protocol. Before contributing, familiarize yourself with:

- [KDE Connect Protocol](https://community.kde.org/KDEConnect)
- [COSMIC Desktop](https://system76.com/cosmic)
- [libcosmic Book](https://pop-os.github.io/libcosmic-book/)

## Development Setup

### Prerequisites

#### NixOS (Recommended)
```bash
# The flake.nix includes all dependencies
nix develop
```

#### Ubuntu/Debian
```bash
sudo apt install cargo cmake just libexpat1-dev libfontconfig-dev \
    libfreetype-dev libxkbcommon-dev pkgconf libssl-dev
```

### Clone and Build

```bash
git clone https://github.com/olafkfreund/cosmic-connect-desktop-app
cd cosmic-connect-desktop-app
nix develop  # Or ensure dependencies are installed
just build
```

### Running Tests

```bash
just test           # Run all tests
just lint           # Run clippy linter
just fmt            # Format code
```

## Claude Code Skill

This project includes a custom Claude Code skill to assist with COSMIC Desktop development best practices.

### Installation

Install the skill for AI-assisted development:

```bash
./.claude/skills/install.sh
```

After installation, **restart Claude Code** to activate the skill.

### Using the Skill

The skill provides 7 specialized agents:

#### Quick Pre-Commit Check
```bash
@cosmic-code-reviewer /pre-commit-check
```

#### Architecture Review
```bash
@cosmic-architect review this application structure
@cosmic-architect /suggest-refactoring
```

#### Theming Audit
```bash
@cosmic-theme-expert /audit-theming
@cosmic-theme-expert check for hard-coded values
```

#### Applet Development
```bash
@cosmic-applet-specialist review this applet
@cosmic-applet-specialist /fix-popup
```

#### Error Handling
```bash
@cosmic-error-handler /remove-unwraps
@cosmic-error-handler audit error handling
```

#### Performance
```bash
@cosmic-performance-optimizer /find-bottlenecks
@cosmic-performance-optimizer check for blocking operations
```

#### Comprehensive Review
```bash
@cosmic-code-reviewer /full-review
```

See `.claude/skills/cosmic-ui-design-skill/README.md` for complete documentation.

## Code Style

### Rust Code Style

Follow the project's Rust style guidelines:

- Run `just fmt` before committing
- Use `just lint` to check for issues
- Follow patterns in existing code
- Use `tracing` for logging (not `println!`)
- Avoid `.unwrap()` and `.expect()` - use proper error handling

### COSMIC-Specific Guidelines

1. **No Hard-Coded Values**
   - Use `theme::spacing()` for layout spacing
   - Use theme colors via `theme.cosmic()`
   - No hard-coded dimensions or corner radii

2. **Widget Composition**
   - Use libcosmic widgets from `cosmic::widget`
   - Follow existing UI patterns
   - Use symbolic icons (`name-symbolic`)

3. **Error Handling**
   - Use `Result` types and `?` operator
   - Add proper `tracing` logs for errors
   - Provide fallback values where appropriate

4. **Async Operations**
   - Return `Task` for long-running operations
   - Don't block in `update()` method
   - Use `tokio::spawn` for CPU-intensive work

See `CLAUDE.md` for detailed development standards.

## Commit Guidelines

### Pre-Commit Checklist

Before creating a commit, **ALWAYS** run the code-simplifier agent:

```bash
@code-simplifier review the changes we made
```

This ensures:
- Code clarity and consistency
- Removal of redundant patterns
- Better Rust idioms
- Alignment with codebase conventions

**Exception:** Skip only for trivial changes (typo fixes, comments only).

### Commit Message Format

Use conventional commit format:

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

**Types:**
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting)
- `refactor`: Code refactoring
- `test`: Adding or updating tests
- `chore`: Maintenance tasks

**Examples:**
```
feat(telephony): add SMS message handling
fix(discovery): resolve UDP broadcast issue
docs(diagnostics): update debugging guide
```

### Co-Authoring

If using AI assistance, include:
```
Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>
```

## Pull Request Process

### Before Submitting

1. **Run Pre-Commit Checks**
   ```bash
   just check
   just test
   @cosmic-code-reviewer /pre-commit-check
   ```

2. **Update Documentation**
   - Update README if adding features
   - Add/update doc comments
   - Update CHANGELOG if applicable

3. **Test Thoroughly**
   - Test with real devices if possible
   - Verify UI in both light and dark themes
   - Check for memory leaks in long-running tests

### Pull Request Description

Include in your PR description:

```markdown
## Changes
- Brief description of changes

## Testing
- How you tested the changes
- Test devices/configurations used

## Screenshots/Videos
- UI changes should include screenshots
- Complex interactions should include videos

## Checklist
- [ ] Code follows style guidelines
- [ ] Tests pass (`just test`)
- [ ] Lint passes (`just lint`)
- [ ] Documentation updated
- [ ] AI code review completed
```

### Review Process

1. Automated checks must pass (CI/CD when available)
2. Code review by maintainers
3. Testing on real hardware if applicable
4. Approval from at least one maintainer

## Testing

### Unit Tests

Write unit tests for plugins and core functionality:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_functionality() {
        // Test implementation
    }
}
```

### Integration Tests

For daemon and protocol testing, use integration tests in `tests/` directory.

### Manual Testing

For UI and hardware integration:
1. Test with Android/iOS KDE Connect apps
2. Test all supported plugins
3. Test pairing and connectivity
4. Test in both light and dark themes

## Documentation

### Code Documentation

- Add doc comments to public APIs
- Include usage examples in doc comments
- Document error conditions
- Update module-level documentation

### User Documentation

- Update README for user-facing features
- Update docs/ directory for detailed guides
- Include screenshots for UI features
- Document configuration options

### Debug Documentation

See `docs/DEBUGGING.md` for:
- Diagnostic commands
- Log analysis
- Troubleshooting procedures
- Performance metrics

## Plugin Development

When adding new plugins:

1. Create plugin in `cosmic-connect-protocol/src/plugins/`
2. Implement `Plugin` and `PluginFactory` traits
3. Add config flag in `cosmic-connect-daemon/src/config.rs`
4. Register factory in `cosmic-connect-daemon/src/main.rs`
5. Follow existing plugin patterns (ping, battery, etc.)
6. Add comprehensive tests
7. Document plugin capabilities

See existing plugins for reference implementation.

## Submitting to nixpkgs

### Overview

To make COSMIC Connect available in the official NixOS package repository, we need to submit it to [nixpkgs](https://github.com/NixOS/nixpkgs).

### Prerequisites

Before submission, ensure:

- [ ] Package builds successfully on latest nixpkgs-unstable
- [ ] All tests pass
- [ ] Package follows nixpkgs conventions
- [ ] You have a GitHub account
- [ ] You're willing to maintain the package

### Step-by-Step Submission Process

#### 1. Prepare Package Hashes

The nixpkgs version requires proper source and dependency hashes. Update `nix/pkgs/cosmic-connect.nix`:

**Get the source hash:**
```bash
# After tagging a release (e.g., v0.1.0)
nix-prefetch-url --unpack https://github.com/olafkfreund/cosmic-connect-desktop-app/archive/refs/tags/v0.1.0.tar.gz
```

**Get the cosmic-connect-core git dependency hash:**
```bash
# Find the commit hash from Cargo.lock
grep -A 2 "cosmic-connect-core" Cargo.lock

# Get the hash
nix-prefetch-git https://github.com/olafkfreund/cosmic-connect-core.git --rev <COMMIT_HASH>
```

Update both hashes in `nix/pkgs/cosmic-connect.nix`.

#### 2. Copy Cargo.lock

The package references `./Cargo.lock` which needs to be in the same directory for nixpkgs:

```bash
# In your nixpkgs fork
cp /path/to/cosmic-connect/Cargo.lock pkgs/by-name/co/cosmic-connect/Cargo.lock
```

#### 3. Fork and Clone nixpkgs

```bash
# Fork https://github.com/NixOS/nixpkgs on GitHub
git clone https://github.com/YOUR_USERNAME/nixpkgs
cd nixpkgs
git remote add upstream https://github.com/NixOS/nixpkgs
git fetch upstream
```

#### 4. Create Feature Branch

```bash
git checkout -b cosmic-connect upstream/master
```

#### 5. Add Package to nixpkgs

Using the new `pkgs/by-name` structure:

```bash
mkdir -p pkgs/by-name/co/cosmic-connect
cp /path/to/cosmic-connect/nix/pkgs/cosmic-connect.nix pkgs/by-name/co/cosmic-connect/package.nix
cp /path/to/cosmic-connect/Cargo.lock pkgs/by-name/co/cosmic-connect/Cargo.lock
```

**Important:** The file must be named `package.nix` in the by-name structure, not `cosmic-connect.nix`.

#### 6. Add Maintainer Information

Edit your package to include your maintainer info:

```nix
maintainers = with lib.maintainers; [ your-github-username ];
```

If you're not yet in the maintainers list, add yourself to `maintainers/maintainer-list.nix`:

```nix
your-github-username = {
  email = "your-email@example.com";
  github = "your-github-username";
  githubId = 12345678; # Your GitHub user ID
  name = "Your Name";
};
```

Find your GitHub ID at: `https://api.github.com/users/YOUR_USERNAME`

#### 7. Test the Package

Build the package in your nixpkgs fork:

```bash
nix-build -A cosmic-connect
```

Test installation:

```bash
nix-shell -p cosmic-connect --command "cosmic-applet-connect --version"
```

#### 8. Add NixOS Module (Optional)

If including the NixOS module:

```bash
mkdir -p nixos/modules/services/desktops/cosmic
cp /path/to/cosmic-connect/nix/module.nix \
   nixos/modules/services/desktops/cosmic/cosmic-connect.nix
```

Add to `nixos/modules/module-list.nix`:

```nix
./services/desktops/cosmic/cosmic-connect.nix
```

Test the module:

```bash
nixos-rebuild build-vm -I nixos-config=./test-config.nix
```

Where `test-config.nix` contains:

```nix
{ config, pkgs, ... }:
{
  imports = [ <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix> ];
  services.cosmic-connect.enable = true;
}
```

#### 9. Run nixpkgs Checks

```bash
# Check package evaluation
nix-instantiate --eval -E 'with import ./. {}; cosmic-connect.meta'

# Run package review script (if available)
nix-shell -p nix-review --run "nix-review wip"
```

#### 10. Commit Changes

Follow nixpkgs commit conventions:

```bash
git add pkgs/by-name/co/cosmic-connect/
git add maintainers/maintainer-list.nix  # If you added yourself

git commit -m "cosmic-connect: init at 0.1.0"
```

Commit message format:
- First line: `package-name: init at VERSION` for new packages
- Body: Brief description of what the package does
- Reference: Include homepage URL

Example:
```
cosmic-connect: init at 0.1.0

Device connectivity for COSMIC Desktop. Provides seamless integration
between Android devices and COSMIC Desktop with features like file
sharing, clipboard sync, notification mirroring, and remote desktop.

https://github.com/olafkfreund/cosmic-connect-desktop-app
```

#### 11. Push and Create Pull Request

```bash
git push origin cosmic-connect
```

Create PR on GitHub with this template:

```markdown
##### Motivation

Add cosmic-connect package for COSMIC Desktop device connectivity.

##### Description of changes

- Add cosmic-connect package to pkgs/by-name/co/cosmic-connect/
- Add maintainer entry for [your-username]
- [Optional] Add NixOS module for cosmic-connect service

##### Things done

- [ ] Built on platform(s)
   - [ ] x86_64-linux
   - [ ] aarch64-linux
- [ ] Tested using sandboxing ([nix.useSandbox](https://nixos.org/nixos/manual/options.html#opt-nix.useSandbox) on NixOS, or option `sandbox` in [`nix.conf`](https://nixos.org/nix/manual/#sec-conf-file) on non-NixOS linux)
- [ ] Tested execution of all binary files (usually in `./result/bin/`)
- [ ] Fits [CONTRIBUTING.md](https://github.com/NixOS/nixpkgs/blob/master/CONTRIBUTING.md)
```

#### 12. Respond to Review

Maintainers will review your PR. Common feedback:

- **License verification**: Ensure GPL-3.0-or-later matches source
- **Dependency versions**: Verify all dependencies are in nixpkgs
- **Build on multiple platforms**: May need aarch64-linux testing
- **Meta information**: Ensure description is accurate
- **Security**: Review systemd hardening settings

Address feedback by:

```bash
# Make changes
git add -u
git commit --amend
git push --force origin cosmic-connect
```

#### 13. After Merge

Once merged to nixpkgs:

1. **Update README**: Add nixpkgs installation instructions
2. **Close Issue #43**
3. **Announce**: Share on COSMIC community channels
4. **Monitor**: Watch for user issues in nixpkgs

### Ongoing Maintenance

As a package maintainer, you'll need to:

- Update the package for new releases
- Respond to build failures
- Address security issues promptly
- Update dependencies when needed

**Updating the package:**

```bash
# In nixpkgs fork
git checkout master
git pull upstream master
git checkout -b cosmic-connect-0.2.0

# Update version and hashes in package.nix
# Test build
nix-build -A cosmic-connect

git commit -am "cosmic-connect: 0.1.0 -> 0.2.0"
git push origin cosmic-connect-0.2.0
# Create PR
```

### Resources

- [nixpkgs Contributing Guide](https://github.com/NixOS/nixpkgs/blob/master/CONTRIBUTING.md)
- [Nixpkgs Manual - Rust](https://nixos.org/manual/nixpkgs/stable/#rust)
- [NixOS Module Writing](https://nixos.org/manual/nixos/stable/#sec-writing-modules)
- [pkgs/by-name Documentation](https://github.com/NixOS/nixpkgs/tree/master/pkgs/by-name)

### Getting Help with Submission

- **nixpkgs Issues**: Ask in your PR comments
- **NixOS Discourse**: [discourse.nixos.org](https://discourse.nixos.org)
- **Matrix Chat**: #nixos:nixos.org
- **Rust Package Help**: Search for similar Rust packages in nixpkgs

## Getting Help

- **Questions**: Open a GitHub Discussion
- **Bugs**: Open a GitHub Issue
- **Security**: See SECURITY.md (if exists)
- **Chat**: Join COSMIC community channels
- **nixpkgs Help**: See "Submitting to nixpkgs" section above

## Code of Conduct

Be respectful, constructive, and professional. We want to build a welcoming community for all contributors.

## License

By contributing, you agree that your contributions will be licensed under the GPL-3.0 License.

---

**Thank you for contributing to COSMIC Connect!** 

Your contributions help make device connectivity on COSMIC Desktop better for everyone.
