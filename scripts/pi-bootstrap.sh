#!/usr/bin/env bash
set -euo pipefail

# Bootstrap dependencies for HawkBot gateway on Raspberry Pi OS (Debian-based).
# This script can be run multiple times safely.

PROJECT_DIR="${PROJECT_DIR:-/opt/hawkbot-gateway}"
SERVICE_NAME="scoreboard-gateway.service"

echo "[1/8] Installing OS packages..."
sudo apt-get update
sudo apt-get install -y \
  build-essential \
  pkg-config \
  libssl-dev \
  ca-certificates \
  curl \
  git \
  mosquitto

if ! command -v rustup >/dev/null 2>&1; then
  echo "[2/8] Installing Rust toolchain via rustup..."
  curl https://sh.rustup.rs -sSf | sh -s -- -y
fi

# shellcheck source=/dev/null
source "$HOME/.cargo/env"

echo "[3/8] Ensuring stable toolchain is installed..."
rustup toolchain install stable
rustup default stable

if [[ ! -d "$PROJECT_DIR/.git" ]]; then
  echo "ERROR: PROJECT_DIR '$PROJECT_DIR' does not look like a git checkout."
  echo "Clone this repository there first, or set PROJECT_DIR to your checkout path."
  exit 1
fi

echo "[4/8] Building release binary..."
cd "$PROJECT_DIR"
cargo build --release

echo "[5/8] Creating runtime directories..."
sudo mkdir -p /opt/hawkbot-gateway
sudo cp "$PROJECT_DIR/target/release/hawkbot-gateway" /opt/hawkbot-gateway/hawkbot-gateway
sudo cp "$PROJECT_DIR/config.json" /opt/hawkbot-gateway/config.json
sudo chown -R pi:pi /opt/hawkbot-gateway

echo "[6/8] Installing systemd service file..."
sudo cp "$PROJECT_DIR/deploy/$SERVICE_NAME" /etc/systemd/system/$SERVICE_NAME

# Keep service file in sync with installed binary path.
sudo sed -i 's|ExecStart=.*|ExecStart=/opt/hawkbot-gateway/hawkbot-gateway|' /etc/systemd/system/$SERVICE_NAME

# Ensure working directory points to runtime config location.
sudo sed -i 's|WorkingDirectory=.*|WorkingDirectory=/opt/hawkbot-gateway|' /etc/systemd/system/$SERVICE_NAME

echo "[7/8] Enabling Mosquitto and gateway service..."
sudo systemctl enable --now mosquitto
sudo systemctl daemon-reload
sudo systemctl enable --now "$SERVICE_NAME"

echo "[8/8] Done. Useful checks:"
echo "  systemctl status $SERVICE_NAME --no-pager"
echo "  journalctl -u $SERVICE_NAME -f"
echo "  mosquitto_sub -t 'scoreboard/#' -v"
