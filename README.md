# Daktronics RTD → MQTT + JSON Gateway

Rust service that reads Daktronics All Sport 5000 RTD bytes from serial, normalizes them, publishes to MQTT, and exposes HTTP endpoints.

## Features

- Reads serial RTD stream (`/dev/ttyUSB0`, 19200 8N1 default)
- Supports sport selection: basketball, volleyball, football, soccer, lacrosse
- Normalized scoreboard schema on:
  - MQTT status topic (`scoreboard/status` default)
  - HTTP `GET /status.json`
- Health topic (`scoreboard/health`)
- Config topic (`scoreboard/config`) when config changes
- Basic Auth protected admin form at `GET/POST /admin`
- Live config updates restart the decoder loop
- Serial reconnect every 2 seconds on disconnect

## Raspberry Pi dependencies

### Hardware

- Raspberry Pi 4 or 5
- Daktronics controller serial feed into a USB-RS232 adapter

### OS packages

Install these packages on Raspberry Pi OS (Debian-based):

```bash
sudo apt-get update
sudo apt-get install -y \
  build-essential pkg-config libssl-dev ca-certificates curl git mosquitto
```

### Rust

Install Rust with rustup:

```bash
curl https://sh.rustup.rs -sSf | sh -s -- -y
source "$HOME/.cargo/env"
rustup toolchain install stable
rustup default stable
```

### One-shot bootstrap script

A helper script is included:

```bash
PROJECT_DIR=/opt/hawkbot-gateway ./scripts/pi-bootstrap.sh
```

The script installs OS dependencies, installs Rust if missing, builds release, installs files into `/opt/hawkbot-gateway`, and enables both `mosquitto` and the gateway service.

## Run (manual)

```bash
cargo run
```

If `cargo run` returns `-bash: cargo: command not found`, your Rust bin path is not in the current shell yet. Run:

```bash
source "$HOME/.cargo/env"
cargo --version
```

If that still fails, install Rust first:

```bash
curl https://sh.rustup.rs -sSf | sh -s -- -y
source "$HOME/.cargo/env"
rustup toolchain install stable
rustup default stable
```

Server listens on `0.0.0.0:8080`.

## Configuration

`config.json` is auto-created if missing.

Important fields:

- `controller_type`
- `sport_type`
- `serial_device`
- `baud`
- `serial_debug_raw` (default `false`; emits truncated raw serial byte previews at debug level)
- `mqtt_host`
- `mqtt_port`
- `mqtt_topic`
- `mqtt_retain`
- `publish_interval_ms`
- `admin_user`
- `admin_pass`


### Raw serial debug logging

Enable structured raw frame logging (hex + ASCII preview) by setting `serial_debug_raw` in `config.json` or with an environment variable override:

```bash
SERIAL_DEBUG_RAW=true cargo run
```

Set to `false` to disable this mode (default).

## MQTT payloads

- `scoreboard/status`: normalized scoreboard JSON
- `scoreboard/health`: runtime health (`ok`, serial, decoder, mqtt)
- `scoreboard/config`: current runtime config

## Systemd deployment

Copy service file:

```bash
sudo cp deploy/scoreboard-gateway.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now scoreboard-gateway.service
```

## Notes

- The decoder module wires in `daktronics-allsport-5000` sport and RTD types and provides a placeholder byte-to-schema mapping. Replace `synthesize_payload` with direct crate field mapping for production.
