# COSMIC Connect Documentation

Welcome to the COSMIC Connect documentation! This page serves as the central hub for all project documentation.

##  Quick Start

- **[Setup Guide](development/Setup.md)** - Get started with development
- **[Contributing](development/Contributing.md)** - How to contribute to the project
- **[README](../README.md)** - Project overview and quick start

##  Architecture

Understand the system design and technical architecture:

- **[Architecture Overview](architecture/Architecture.md)** - Multi-platform architecture, shared core library, and integration points
- **[Protocol Specification](architecture/Protocol.md)** - KDE Connect protocol implementation details

### Key Architectural Concepts

- **70%+ Code Sharing** - Unified Rust core shared between desktop and Android
- **cosmic-connect-core** - Shared library providing TLS, protocol, and plugins
- **cosmic-connect-protocol** - Desktop-specific protocol extensions
- **FFI Bindings** - Kotlin/Swift support via uniffi-rs

##  Development

Everything you need to develop COSMIC Connect:

### Getting Started

- **[Development Guide](development/Development-Guide.md)** - Comprehensive development documentation
- **[Setup Guide](development/Setup.md)** - Environment setup for NixOS and other distros
- **[Build Fixes](development/Build-Fixes.md)** - Common build issues and solutions

### Component Development

- **[Applet Development](development/Applet-Development.md)** - COSMIC panel applet development guide
- **[Contributing Guidelines](development/Contributing.md)** - Code style, workflow, and standards

### Development Workflow

1. Clone [cosmic-connect-core](https://github.com/olafkfreund/cosmic-connect-core) as sibling directory
2. Clone this repository
3. Enter Nix development shell: `nix develop`
4. Build: `cargo build`
5. Run tests: `cargo test`

##  Project Management

Track project progress and planning:

- **[Project Status](project/Status.md)** - Current implementation status and roadmap
- **[Acceptance Criteria](project/Acceptance-Criteria.md)** - Quality standards and definition of done
- **[GitHub Issues](project/Issues.md)** - Issue management and tracking
- **[Issue Breakdown](project/Issue-37-Breakdown.md)** - Detailed breakdown of major issues

### Implementation Status

 **98% Complete** - Production ready for COSMIC Desktop

**Core Features:**
- Device discovery (mDNS + UDP)
- Secure TLS pairing
- 12 plugins implemented
- Background daemon with DBus
- COSMIC panel applet

**In Progress:**
- Transfer progress tracking
- iOS support

## 🔌 Plugin Development

COSMIC Connect uses an extensible plugin system:

### Implemented Plugins (12)

- **Ping** - Connection testing
- **Battery** - Battery status sync
- **Clipboard** - Bidirectional sync
- **Share** - File/text/URL sharing
- **Notification** - Notification forwarding
- **Find My Phone** - Ring device remotely
- **MPRIS** - Media player control
- **Run Command** - Remote command execution
- **Presenter** - Presentation control
- **Remote Input** - Mouse/keyboard control
- **Telephony** - Call/SMS notifications
- **Contacts** - Contact sync

### Adding New Plugins

Plugins are defined in `cosmic-connect-protocol/src/plugins/`. See [Development Guide](development/Development-Guide.md) for details.

##  Multi-Platform Ecosystem

COSMIC Connect is part of a larger ecosystem:

### Related Repositories

- **[cosmic-connect-core](https://github.com/olafkfreund/cosmic-connect-core)** - Shared Rust library (TLS, protocol, plugins)
- **[cosmic-connect-android](https://github.com/olafkfreund/cosmic-connect-android)** - Android app with Kotlin FFI
- **[cosmic-connect-desktop-app](https://github.com/olafkfreund/cosmic-connect-desktop-app)** - This repository

### Integration Architecture

```
cosmic-connect-core (Shared Library)
├── Protocol v7 implementation
├── TLS/crypto layer (rustls)
├── Network & discovery
├── Plugin system
└── FFI bindings (uniffi-rs) ──┐
                                │
                                ├──→ Desktop (This Repo)
                                │    ├── cosmic-connect-protocol
                                │    ├── cosmic-connect-daemon
                                │    └── cosmic-applet-connect
                                │
                                └──→ Android App
                                     └── Kotlin via FFI
```

##  Testing

- **114 Unit Tests** - Comprehensive test coverage
- **12 Integration Tests** - End-to-end testing
- **CI/CD** - GitHub Actions automation

Run tests:
```bash
cargo test                           # All tests
cargo test -p cosmic-connect-protocol # Protocol tests only
cargo test -- --nocapture            # Verbose output
```

## 📚 Additional Resources

### External Documentation

- [COSMIC Desktop](https://system76.com/cosmic) - Modern desktop environment
- [libcosmic](https://pop-os.github.io/libcosmic-book/) - COSMIC widget toolkit
- [KDE Connect Protocol](https://invent.kde.org/network/kdeconnect-kde) - Original protocol
- [Valent Protocol Reference](https://valent.andyholmes.ca/documentation/protocol.html) - Protocol docs
- [uniffi-rs](https://github.com/mozilla/uniffi-rs) - FFI binding generator
- [rustls](https://github.com/rustls/rustls) - TLS implementation

### Technology Stack

- **Language**: Rust 🦀 (100%)
- **GUI**: libcosmic (COSMIC native)
- **Async**: tokio
- **TLS**: rustls (no OpenSSL)
- **DBus**: zbus
- **FFI**: uniffi-rs

##  Documentation Index

### By Category

#### Architecture & Design
- [Architecture Overview](architecture/Architecture.md)
- [Protocol Specification](architecture/Protocol.md)

#### Development Guides
- [Contributing Guidelines](development/Contributing.md)
- [Development Guide](development/Development-Guide.md)
- [Setup Guide](development/Setup.md)
- [Build Fixes](development/Build-Fixes.md)
- [Applet Development](development/Applet-Development.md)

#### Project Management
- [Project Status](project/Status.md)
- [Acceptance Criteria](project/Acceptance-Criteria.md)
- [GitHub Issues](project/Issues.md)
- [Issue #37 Breakdown](project/Issue-37-Breakdown.md)
- [Issues To Close](project/Issues-To-Close.md)

#### Archive
- [Old Project Context](archive/old-context.md) - Historical reference

##  Contributing

We welcome contributions! Please see:

1. **[Contributing Guidelines](development/Contributing.md)** - Code style and workflow
2. **[Development Guide](development/Development-Guide.md)** - Development setup
3. **[Project Status](project/Status.md)** - Current priorities

### Commit Convention

```
type(scope): description

feat: New feature
fix: Bug fix
docs: Documentation
refactor: Code refactoring
test: Tests
chore: Build/tooling
```

##  Support

- **Issues**: [GitHub Issues](https://github.com/olafkfreund/cosmic-connect-desktop-app/issues)
- **Discussions**: [GitHub Discussions](https://github.com/olafkfreund/cosmic-connect-desktop-app/discussions)
- **COSMIC Community**: [Pop!_OS Mattermost](https://chat.pop-os.org/)

##  License

This project is licensed under the **GNU General Public License v3.0 or later**.

---

**Navigation**: [↑ Back to Top](#cosmic-connect-documentation) | [← Main README](../README.md)

*Last Updated: 2026-01-15*
