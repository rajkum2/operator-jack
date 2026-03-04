#!/bin/bash
# Operator Jack installer — for users who don't use Homebrew
# Usage: curl -fsSL https://raw.githubusercontent.com/rajkum2/operator-jack/main/install.sh | bash

set -euo pipefail

REPO="rajkum2/operator-jack"
INSTALL_DIR="/usr/local/bin"

echo "Installing Operator Jack..."

# Detect architecture
ARCH=$(uname -m)
case "$ARCH" in
  arm64|aarch64) ;;
  x86_64) ;;
  *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

# Get latest release tag
LATEST=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed 's/.*"tag_name": "\(.*\)".*/\1/')
if [ -z "$LATEST" ]; then
  echo "Error: could not determine latest release"
  exit 1
fi
echo "Latest version: $LATEST"

# Download tarball
TARBALL="operator-jack-${LATEST}-macos-universal.tar.gz"
URL="https://github.com/$REPO/releases/download/$LATEST/$TARBALL"
CHECKSUM_URL="${URL}.sha256"

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading $URL..."
curl -fsSL -o "$TMPDIR/$TARBALL" "$URL"
curl -fsSL -o "$TMPDIR/$TARBALL.sha256" "$CHECKSUM_URL"

# Verify checksum
echo "Verifying checksum..."
cd "$TMPDIR"
if command -v shasum &>/dev/null; then
  shasum -a 256 -c "$TARBALL.sha256"
else
  echo "Warning: shasum not found, skipping verification"
fi

# Extract and install
echo "Extracting..."
tar xzf "$TARBALL"

echo "Installing to $INSTALL_DIR (may require sudo)..."
if [ -w "$INSTALL_DIR" ]; then
  cp operator-jack "$INSTALL_DIR/"
  cp operator-macos-helper "$INSTALL_DIR/"
  chmod +x "$INSTALL_DIR/operator-jack" "$INSTALL_DIR/operator-macos-helper"
else
  sudo cp operator-jack "$INSTALL_DIR/"
  sudo cp operator-macos-helper "$INSTALL_DIR/"
  sudo chmod +x "$INSTALL_DIR/operator-jack" "$INSTALL_DIR/operator-macos-helper"
fi

echo ""
echo "Operator Jack $LATEST installed successfully!"
echo ""
echo "Next steps:"
echo "  1. Run: operator-jack doctor"
echo "  2. Grant Accessibility permission to your terminal app"
echo "     (System Settings > Privacy & Security > Accessibility)"
echo "  3. Try: operator-jack run --plan-file example.json --yes"
