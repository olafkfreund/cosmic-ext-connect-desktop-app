#!/usr/bin/env bash
# test-plugins.sh - Quick automated plugin testing script
#
# Usage: ./scripts/test-plugins.sh [device_id]
# If device_id not provided, will detect automatically

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Get device ID
DEVICE_ID="${1:-}"

if [ -z "$DEVICE_ID" ]; then
    echo -e "${BLUE}🔍 Auto-detecting paired device...${NC}"
    DEVICE_ID=$(busctl --user call com.system76.CosmicKdeConnect \
        /com/system76/CosmicKdeConnect \
        com.system76.CosmicKdeConnect \
        ListDevices | grep -oP '1b7bbb613c0c42bb9a0b80b24d28631d' | head -1 || echo "")

    if [ -z "$DEVICE_ID" ]; then
        echo -e "${RED}❌ No paired device found. Please pair a device first.${NC}"
        exit 1
    fi
    echo -e "${GREEN}✓ Found device: $DEVICE_ID${NC}"
fi

echo ""
echo "╔════════════════════════════════════════════╗"
echo "║   KDE Connect Plugin Testing Suite        ║"
echo "║   Device: ${DEVICE_ID:0:16}...  ║"
echo "╚════════════════════════════════════════════╝"
echo ""

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_TOTAL=0

# Function to test a plugin
test_plugin() {
    local plugin_name="$1"
    local test_command="$2"
    local expected_result="$3"

    TESTS_TOTAL=$((TESTS_TOTAL + 1))
    echo -n "Testing ${plugin_name}... "

    if eval "$test_command" >/dev/null 2>&1; then
        echo -e "${GREEN}✓ PASS${NC}"
        TESTS_PASSED=$((TESTS_PASSED + 1))
        return 0
    else
        echo -e "${RED}✗ FAIL${NC}"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        return 1
    fi
}

# Test 1: Ping Plugin
echo -e "${BLUE}1. 🏓 Ping Plugin${NC}"
test_plugin "Ping" \
    "busctl --user call com.system76.CosmicKdeConnect \
        /com/system76/CosmicKdeConnect \
        com.system76.CosmicKdeConnect \
        SendPing s '$DEVICE_ID'" \
    "success"
sleep 1

# Test 2: Battery Plugin
echo -e "${BLUE}2. 🔋 Battery Plugin${NC}"
test_plugin "Battery Status" \
    "busctl --user get-property com.system76.CosmicKdeConnect \
        /com/system76/CosmicKdeConnect \
        com.system76.CosmicKdeConnect \
        BatteryLevel" \
    "success" || echo -e "   ${YELLOW}ℹ Check if battery status is syncing${NC}"
sleep 1

# Test 3: Find My Phone Plugin
echo -e "${BLUE}3. 🔍 Find My Phone Plugin${NC}"
echo -e "   ${YELLOW}⚠ This will make your phone ring loudly!${NC}"
read -p "   Press Enter to test or Ctrl+C to skip..."
test_plugin "Find My Phone" \
    "busctl --user call com.system76.CosmicKdeConnect \
        /com/system76/CosmicKdeConnect \
        com.system76.CosmicKdeConnect \
        FindMyPhone s '$DEVICE_ID'" \
    "success"
echo -e "   ${YELLOW}ℹ Check if phone is ringing${NC}"
sleep 3

# Test 4: Check if device is paired
echo -e "${BLUE}4. 🔗 Pairing Status${NC}"
test_plugin "Device Paired" \
    "busctl --user call com.system76.CosmicKdeConnect \
        /com/system76/CosmicKdeConnect \
        com.system76.CosmicKdeConnect \
        IsPaired s '$DEVICE_ID' | grep -q 'true'" \
    "success"

# Test 5: Check if device is connected
echo -e "${BLUE}5. 🌐 Connection Status${NC}"
test_plugin "Device Connected" \
    "busctl --user call com.system76.CosmicKdeConnect \
        /com/system76/CosmicKdeConnect \
        com.system76.CosmicKdeConnect \
        IsReachable s '$DEVICE_ID' | grep -q 'true'" \
    "success"

# Summary
echo ""
echo "╔════════════════════════════════════════════╗"
echo "║            Test Results Summary            ║"
echo "╠════════════════════════════════════════════╣"
echo -e "║  Total Tests:  ${TESTS_TOTAL}                              ║"
echo -e "║  ${GREEN}Passed:       ${TESTS_PASSED}${NC}                              ║"
echo -e "║  ${RED}Failed:       ${TESTS_FAILED}${NC}                              ║"
echo "╚════════════════════════════════════════════╝"
echo ""

if [ $TESTS_FAILED -eq 0 ]; then
    echo -e "${GREEN}✓ All automated tests passed!${NC}"
    echo ""
    echo "Next steps:"
    echo "  1. Test remaining plugins manually (see docs/PLUGIN_TESTING_GUIDE.md)"
    echo "  2. Test file transfer: Share a file from phone → desktop"
    echo "  3. Test clipboard sync: Copy text on phone → paste on desktop"
    echo "  4. Test MPRIS: Control media playback from desktop"
    echo "  5. Test remote input: Control phone with desktop mouse/keyboard"
    exit 0
else
    echo -e "${RED}✗ Some tests failed. Check the output above.${NC}"
    echo ""
    echo "Troubleshooting:"
    echo "  1. Ensure device is paired and connected"
    echo "  2. Check daemon logs: tail -f /tmp/daemon-debug-verbose.log"
    echo "  3. Verify plugins are enabled in config"
    echo "  4. Try reconnecting the device"
    exit 1
fi
