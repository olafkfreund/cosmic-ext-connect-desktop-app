# cosmic-ext-applet-connect - Build Commands
# https://github.com/casey/just

# Default recipe (runs when you just type 'just')
default:
    @just --list

# Build all components
build:
    cargo build

# Build with optimizations
build-release:
    cargo build --release

# Build only the protocol library
build-protocol:
    cargo build -p cosmic-ext-connect-protocol

# Build only the applet
build-applet:
    cargo build -p cosmic-ext-applet-connect

# Build only the full application
build-app:
    cargo build -p cosmic-ext-connect-daemon

# Build only the daemon
build-daemon:
    cargo build -p cosmic-ext-connect-daemon

# Run the applet in development mode
run-applet:
    RUST_LOG=debug cargo run -p cosmic-ext-applet-connect

# Run the full application
run-app:
    RUST_LOG=debug cargo run -p cosmic-ext-connect-daemon

# Run the daemon
run-daemon:
    RUST_LOG=debug cargo run -p cosmic-ext-connect-daemon

# Run all tests
test:
    cargo test --all

# Run tests with output
test-verbose:
    cargo test --all -- --nocapture

# Run only protocol tests
test-protocol:
    cargo test -p cosmic-ext-connect-protocol

# Run integration tests
test-integration:
    cargo test --test '*' --all

# Test device discovery (requires network)
test-discovery:
    cargo test -p cosmic-ext-connect-protocol discovery -- --nocapture --ignored

# Run tests with coverage (requires cargo-tarpaulin)
test-coverage:
    cargo tarpaulin --all --out Html --output-dir coverage

# Format code
fmt:
    cargo fmt --all

# Check formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Run clippy linter
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Run clippy with fixes
lint-fix:
    cargo clippy --fix --all-targets --all-features

# Check code (format + lint + test)
check: fmt lint test
    @echo "✅ All checks passed!"

# Clean build artifacts
clean:
    cargo clean
    rm -rf target/
    rm -rf coverage/

# Security audit of dependencies
audit:
    cargo audit

# Update dependencies
update:
    cargo update

# Generate documentation
doc:
    cargo doc --no-deps --all --open

# Generate protocol documentation
doc-protocol:
    cargo doc --no-deps -p cosmic-ext-connect-protocol --open

# Install all components (requires sudo)
install: build-release
    sudo install -Dm755 target/release/cosmic-ext-applet-connect \
        /usr/bin/cosmic-ext-applet-connect
    sudo install -Dm755 target/release/cosmic-ext-connect-daemon \
        /usr/bin/cosmic-ext-connect-daemon
    sudo install -Dm755 target/release/cosmic-ext-connect-manager \
        /usr/bin/cosmic-ext-connect-manager
    sudo install -Dm644 cosmic-ext-applet-connect/data/cosmic-ext-applet-connect.desktop \
        /usr/share/applications/cosmic-ext-applet-connect.desktop
    sudo install -Dm644 data/icons/hicolor/scalable/apps/cosmic-ext-connect.svg \
        /usr/share/icons/hicolor/scalable/apps/cosmic-ext-connect.svg
    sudo install -Dm644 data/icons/hicolor/symbolic/apps/cosmic-ext-connect-symbolic.svg \
        /usr/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-symbolic.svg
    sudo install -Dm644 data/icons/hicolor/symbolic/apps/cosmic-ext-connect-phone-symbolic.svg \
        /usr/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-phone-symbolic.svg
    sudo install -Dm644 data/icons/hicolor/symbolic/apps/cosmic-ext-connect-tablet-symbolic.svg \
        /usr/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-tablet-symbolic.svg
    sudo install -Dm644 data/icons/hicolor/symbolic/apps/cosmic-ext-connect-laptop-symbolic.svg \
        /usr/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-laptop-symbolic.svg
    sudo install -Dm644 data/icons/hicolor/symbolic/apps/cosmic-ext-connect-desktop-symbolic.svg \
        /usr/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-desktop-symbolic.svg
    sudo install -Dm644 data/icons/hicolor/symbolic/apps/cosmic-ext-connect-tv-symbolic.svg \
        /usr/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-tv-symbolic.svg
    @echo "Installed successfully!"

# Install only the applet
install-applet: build-release
    sudo install -Dm755 target/release/cosmic-ext-applet-connect \
        /usr/bin/cosmic-ext-applet-connect
    sudo install -Dm644 cosmic-ext-applet-connect/data/cosmic-ext-applet-connect.desktop \
        /usr/share/applications/cosmic-ext-applet-connect.desktop

# Install to local directory (no sudo required)
install-local PREFIX="$HOME/.local": build-release
    install -Dm755 target/release/cosmic-ext-applet-connect \
        {{PREFIX}}/bin/cosmic-ext-applet-connect
    install -Dm755 target/release/cosmic-ext-connect-daemon \
        {{PREFIX}}/bin/cosmic-ext-connect-daemon
    install -Dm755 target/release/cosmic-ext-connect-manager \
        {{PREFIX}}/bin/cosmic-ext-connect-manager
    install -Dm644 cosmic-ext-applet-connect/data/cosmic-ext-applet-connect.desktop \
        {{PREFIX}}/share/applications/cosmic-ext-applet-connect.desktop
    install -Dm644 data/icons/hicolor/scalable/apps/cosmic-ext-connect.svg \
        {{PREFIX}}/share/icons/hicolor/scalable/apps/cosmic-ext-connect.svg
    install -Dm644 data/icons/hicolor/symbolic/apps/cosmic-ext-connect-symbolic.svg \
        {{PREFIX}}/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-symbolic.svg
    install -Dm644 data/icons/hicolor/symbolic/apps/cosmic-ext-connect-phone-symbolic.svg \
        {{PREFIX}}/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-phone-symbolic.svg
    install -Dm644 data/icons/hicolor/symbolic/apps/cosmic-ext-connect-tablet-symbolic.svg \
        {{PREFIX}}/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-tablet-symbolic.svg
    install -Dm644 data/icons/hicolor/symbolic/apps/cosmic-ext-connect-laptop-symbolic.svg \
        {{PREFIX}}/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-laptop-symbolic.svg
    install -Dm644 data/icons/hicolor/symbolic/apps/cosmic-ext-connect-desktop-symbolic.svg \
        {{PREFIX}}/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-desktop-symbolic.svg
    install -Dm644 data/icons/hicolor/symbolic/apps/cosmic-ext-connect-tv-symbolic.svg \
        {{PREFIX}}/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-tv-symbolic.svg

# Uninstall all components
uninstall:
    sudo rm -f /usr/bin/cosmic-ext-applet-connect
    sudo rm -f /usr/bin/cosmic-ext-connect-daemon
    sudo rm -f /usr/bin/cosmic-ext-connect-manager
    sudo rm -f /usr/share/applications/cosmic-ext-applet-connect.desktop
    sudo rm -f /usr/share/icons/hicolor/scalable/apps/cosmic-ext-connect.svg
    sudo rm -f /usr/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-symbolic.svg
    sudo rm -f /usr/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-phone-symbolic.svg
    sudo rm -f /usr/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-tablet-symbolic.svg
    sudo rm -f /usr/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-laptop-symbolic.svg
    sudo rm -f /usr/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-desktop-symbolic.svg
    sudo rm -f /usr/share/icons/hicolor/symbolic/apps/cosmic-ext-connect-tv-symbolic.svg

# Setup development environment
setup:
    @echo "🔧 Setting up development environment..."
    rustup component add rustfmt clippy rust-src rust-analyzer
    just install-hooks
    @echo "✅ Development environment ready!"

# Install git hooks
install-hooks:
    @echo "🪝 Installing git hooks..."
    @mkdir -p .git/hooks
    @cp hooks/pre-commit .git/hooks/pre-commit
    @cp hooks/commit-msg .git/hooks/commit-msg
    @chmod +x .git/hooks/pre-commit
    @chmod +x .git/hooks/commit-msg
    @echo "✅ Git hooks installed!"
    @echo "   - pre-commit: Formats code and runs checks"
    @echo "   - commit-msg: Enforces conventional commits"
    @echo ""
    @echo "To bypass hooks: git commit --no-verify (not recommended)"

# Uninstall git hooks
uninstall-hooks:
    @echo "Removing git hooks..."
    @rm -f .git/hooks/pre-commit
    @rm -f .git/hooks/commit-msg
    @echo "✅ Git hooks uninstalled!"

# Test git hooks (without committing)
test-hooks:
    @echo "Testing pre-commit hook..."
    @bash hooks/pre-commit || true
    @echo ""
    @echo "Testing commit-msg hook..."
    @echo "test(example): test commit message" > /tmp/test-commit-msg
    @bash hooks/commit-msg /tmp/test-commit-msg || true
    @rm -f /tmp/test-commit-msg

# Watch for changes and rebuild
watch:
    cargo watch -x build

# Watch and run tests
watch-test:
    cargo watch -x test

# Watch and run applet
watch-applet:
    cargo watch -x 'run -p cosmic-ext-applet-connect'

# Benchmark performance
bench:
    cargo bench

# Profile build time
profile-build:
    cargo build --timings

# Check for outdated dependencies
outdated:
    cargo outdated

# Create a new release
release VERSION:
    @echo "Creating release {{VERSION}}..."
    git tag -a v{{VERSION}} -m "Release {{VERSION}}"
    git push origin v{{VERSION}}
    cargo build --release
    @echo "✅ Release {{VERSION}} created!"

# Package for distribution
package: build-release
    @echo "📦 Creating distribution package..."
    mkdir -p dist
    tar czf dist/cosmic-ext-applet-connect-$(cargo metadata --format-version 1 | jq -r '.packages[] | select(.name == "cosmic-ext-applet-connect") | .version').tar.gz \
        -C target/release \
        cosmic-ext-applet-connect \
        cosmic-ext-connect-daemon \
        cosmic-ext-connect-daemon
    @echo "✅ Package created in dist/"

# Deploy to a remote NixOS host (e.g., just deploy-remote user@host)
deploy-remote TARGET:
    @echo "🚀 Deploying to {{TARGET}}..."
    nix copy --to ssh://{{TARGET}} .#default
    ssh {{TARGET}} "sudo systemctl restart cosmic-ext-connect-daemon || systemctl --user restart cosmic-ext-connect-daemon"
    @echo "✅ Deployment complete!"

# Print project statistics
stats:
    @echo "📊 Project Statistics"
    @echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    @echo "Lines of code:"
    @tokei
    @echo ""
    @echo "Dependencies:"
    @cargo tree | head -20
    @echo ""
    @echo "Binary sizes:"
    @ls -lh target/release/cosmic-ext-applet-connect target/release/cosmic-ext-connect-daemon target/release/cosmic-ext-connect-daemon 2>/dev/null || echo "  (not built yet)"

# Development helpers
dev-server:
    @echo "Starting development tools..."
    @echo "Press Ctrl+C to stop"
    just watch &
    just run-daemon

# Validate desktop entries
validate-desktop:
    desktop-file-validate cosmic-ext-applet-connect/data/cosmic-ext-applet-connect.desktop

# Generate a new plugin template
new-plugin NAME:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Generating new plugin: {{NAME}}"
    mkdir -p cosmic-ext-connect-protocol/src/plugins
    cat > cosmic-ext-connect-protocol/src/plugins/{{NAME}}.rs << 'TEMPLATE_EOF'
    use crate::{Packet, Plugin, ProtocolError};
    use async_trait::async_trait;

    pub struct {{NAME}}Plugin {
        // Add fields here
    }

    #[async_trait]
    impl Plugin for {{NAME}}Plugin {
        fn id(&self) -> &str {
            "{{NAME}}"
        }

        fn incoming_capabilities(&self) -> Vec<String> {
            vec!["kdeconnect.{{NAME}}".to_string()]
        }

        fn outgoing_capabilities(&self) -> Vec<String> {
            vec!["kdeconnect.{{NAME}}".to_string()]
        }

        async fn handle_packet(&mut self, packet: Packet) -> Result<(), ProtocolError> {
            // Handle packet
            Ok(())
        }
    }
    TEMPLATE_EOF
    echo "✅ Plugin template created at cosmic-ext-connect-protocol/src/plugins/{{NAME}}.rs"

# List all plugins
list-plugins:
    @echo "Available plugins:"
    @find cosmic-ext-connect-protocol/src/plugins -name "*.rs" -not -name "mod.rs" | xargs -I {} basename {} .rs

# Run with specific log level
run-debug LEVEL="debug":
    RUST_LOG={{LEVEL}} cargo run -p cosmic-ext-applet-connect

# Memory profiling with valgrind
profile-memory:
    cargo build --release
    valgrind --leak-check=full --show-leak-kinds=all \
        target/release/cosmic-ext-applet-connect

# CPU profiling with perf
profile-cpu:
    cargo build --release
    perf record -g target/release/cosmic-ext-applet-connect
    perf report

# Display help for firewall configuration
firewall-help:
    @echo "🔥 Firewall Configuration for KDE Connect"
    @echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    @echo ""
    @echo "KDE Connect requires ports 1714-1764 (TCP and UDP) to be open."
    @echo ""
    @echo "For NixOS, add to configuration.nix:"
    @echo "  networking.firewall = {"
    @echo "    allowedTCPPortRanges = [{ from = 1714; to = 1764; }];"
    @echo "    allowedUDPPortRanges = [{ from = 1714; to = 1764; }];"
    @echo "  };"
    @echo ""
    @echo "For firewalld:"
    @echo "  sudo firewall-cmd --zone=public --permanent --add-port=1714-1764/tcp"
    @echo "  sudo firewall-cmd --zone=public --permanent --add-port=1714-1764/udp"
    @echo "  sudo firewall-cmd --reload"
    @echo ""
    @echo "For ufw:"
    @echo "  sudo ufw allow 1714:1764/tcp"
    @echo "  sudo ufw allow 1714:1764/udp"
