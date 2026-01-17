#!/usr/bin/env bash

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  COSMIC Connect Development Environment Setup${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

# Check if Nix is installed
echo -e "${YELLOW}Checking prerequisites...${NC}"
if ! command -v nix &> /dev/null; then
    echo -e "${RED}✗ Nix is not installed${NC}"
    echo ""
    echo "Please install Nix first:"
    echo "  curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install"
    echo ""
    echo "Or on NixOS, it's already available!"
    exit 1
fi
echo -e "${GREEN}✓ Nix is installed${NC}"

# Check if flakes are enabled
if ! nix flake metadata --help &> /dev/null 2>&1; then
    echo -e "${RED}✗ Nix flakes are not enabled${NC}"
    echo ""
    echo "Please enable flakes in your nix configuration:"
    echo "  ~/.config/nix/nix.conf:"
    echo "    experimental-features = nix-command flakes"
    echo ""
    echo "On NixOS, add to /etc/nixos/configuration.nix:"
    echo "  nix.settings.experimental-features = [ \"nix-command\" \"flakes\" ];"
    exit 1
fi
echo -e "${GREEN}✓ Nix flakes are enabled${NC}"

# Check if direnv is installed
if ! command -v direnv &> /dev/null; then
    echo -e "${YELLOW}⚠ direnv is not installed (recommended but optional)${NC}"
    echo ""
    echo "For automatic environment loading, install direnv:"
    echo ""
    echo "On NixOS, add to configuration.nix:"
    echo "  programs.direnv.enable = true;"
    echo ""
    echo "Or with Home Manager:"
    echo "  programs.direnv = {"
    echo "    enable = true;"
    echo "    nix-direnv.enable = true;"
    echo "  };"
    echo ""
    read -p "Continue without direnv? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
    USE_DIRENV=false
else
    echo -e "${GREEN}✓ direnv is installed${NC}"
    USE_DIRENV=true
fi

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Setting up development environment...${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

if [ "$USE_DIRENV" = true ]; then
    # Setup with direnv
    echo -e "${YELLOW}Enabling direnv for this repository...${NC}"

    if [ -f .envrc ]; then
        echo -e "${GREEN}✓ .envrc file already exists${NC}"
    else
        echo -e "${RED}✗ .envrc file not found${NC}"
        echo "This is unexpected - the file should exist in the repository."
        exit 1
    fi

    echo ""
    echo -e "${YELLOW}Running 'direnv allow'...${NC}"
    direnv allow

    echo ""
    echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${GREEN}  ✓ Setup complete!${NC}"
    echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
    echo "Next steps:"
    echo "  1. Exit and re-enter this directory to load the environment"
    echo "  2. Or source direnv manually:"
    echo "       eval \"\$(direnv export bash)\""
    echo "  3. Then run: cargo check"
    echo ""
    echo "The environment will automatically load whenever you enter this directory!"

else
    # Setup without direnv - manual mode
    echo -e "${YELLOW}Starting Nix development shell...${NC}"
    echo ""
    echo "Since direnv is not installed, you'll need to manually enter the development shell."
    echo ""
    echo "Run this command to enter the development environment:"
    echo -e "${BLUE}  nix develop${NC}"
    echo ""
    read -p "Press Enter to start the development shell now, or Ctrl+C to exit..."

    exec nix develop
fi
