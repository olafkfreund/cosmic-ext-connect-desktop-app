#!/usr/bin/env bash
# Local testing script for COSMIC Connect
# Starts daemon and applet for local inspection and testing

set -e

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJECT_ROOT"

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${BLUE}════════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  COSMIC Connect Local Test Environment${NC}"
echo -e "${BLUE}════════════════════════════════════════════════════════════════${NC}"
echo ""

# Check if binaries are built
if [ ! -f "target/debug/cosmic-connect-daemon" ]; then
    echo -e "${RED}Error: Daemon not built. Run 'just build' or 'nix develop --command just build' first.${NC}"
    exit 1
fi

if [ ! -f "target/debug/cosmic-applet-connect" ]; then
    echo -e "${RED}Error: Applet not built. Run 'just build' or 'nix develop --command just build' first.${NC}"
    exit 1
fi

# Copy test config to expected location
echo -e "${BLUE}Setting up configuration...${NC}"
mkdir -p ~/.config/cosmic/cosmic-connect
cp -f test-config.toml ~/.config/cosmic/cosmic-connect/daemon.toml

# Check if config exists
if [ ! -f "test-config.toml" ]; then
    echo -e "${RED}Error: test-config.toml not found${NC}"
    exit 1
fi

# Show configuration
echo -e "${GREEN}Configuration:${NC}"
echo -e "  Config file: ${YELLOW}~/.config/cosmic/cosmic-connect/daemon.toml${NC}"
echo -e "  Data dir:    ${YELLOW}/tmp/cosmic-connect-test${NC}"
echo -e "  Daemon:      ${YELLOW}target/debug/cosmic-connect-daemon${NC}"
echo -e "  Applet:      ${YELLOW}target/debug/cosmic-applet-connect${NC}"
echo ""

# Show enabled plugins
echo -e "${GREEN}Enabled Plugins:${NC}"
grep "^enable_.*= true" test-config.toml | sed 's/enable_/  - /g' | sed 's/ = true//g'
echo ""

# Check if daemon is already running
if pgrep -f "cosmic-connect-daemon" > /dev/null; then
    echo -e "${YELLOW}Warning: Daemon already running. Stopping...${NC}"
    pkill -f "cosmic-connect-daemon"
    sleep 1
fi

# Start daemon in background
echo -e "${BLUE}Starting daemon...${NC}"
export RUST_LOG=info,cosmic_connect_daemon=debug,cosmic_connect_protocol=debug
export RUST_BACKTRACE=1

./target/debug/cosmic-connect-daemon > /tmp/cosmic-connect-daemon.log 2>&1 &
DAEMON_PID=$!

# Wait for daemon to start
echo -e "${BLUE}Waiting for daemon to initialize...${NC}"
sleep 2

# Check if daemon is running
if ! ps -p $DAEMON_PID > /dev/null; then
    echo -e "${RED}Error: Daemon failed to start. Check logs:${NC}"
    tail -20 /tmp/cosmic-connect-daemon.log
    exit 1
fi

echo -e "${GREEN}✓ Daemon started (PID: $DAEMON_PID)${NC}"
echo -e "  Log file: ${YELLOW}/tmp/cosmic-connect-daemon.log${NC}"
echo ""

# Start applet
echo -e "${BLUE}Starting applet UI...${NC}"
echo -e "${YELLOW}Note: Applet will run in foreground. Press Ctrl+C to stop.${NC}"
echo ""

# Give user a moment to read
sleep 1

# Start applet in foreground
export RUST_LOG=info,cosmic_applet_connect=debug
./target/debug/cosmic-applet-connect

# Cleanup on exit
echo ""
echo -e "${BLUE}Stopping daemon...${NC}"
kill $DAEMON_PID 2>/dev/null || true
echo -e "${GREEN}✓ Daemon stopped${NC}"
echo ""
echo -e "${BLUE}Test session ended${NC}"
