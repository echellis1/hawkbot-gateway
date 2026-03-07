mod config;
mod decoder;
mod mqtt;
mod schema;
mod web;

use crate::config::{load_or_create_config, AppConfig, SharedConfig, CONFIG_PATH};
use crate::mqtt::MqttPublisher;
use crate::schema::NormalizedScoreboardStatus;
use anyhow::Result;
use axum::Router;
use std::sync::Arc;
use tokio::sync::{watch, RwLock};
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Initialize Logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    info!("🚀 Starting Hawkbot Gateway (Status Only Mode)...");

    // 2. Load Configuration
    let config = load_or_create_config(CONFIG_PATH).await?;
    let shared_config: SharedConfig = Arc::new(RwLock::new(config.clone()));
    info!("📂 Configuration loaded from {}", CONFIG_PATH);

    let initial = NormalizedScoreboardStatus::blank(&config.controller_type, &config.sport_type);
    let (status_tx, status_rx) = watch::channel(initial);
    let (config_tx, config_rx) = watch::channel(config.clone());
    let (serial_connected_tx, _serial_connected_rx) = watch::channel(false);

    // 3. Initialize MQTT
    let (mqtt, event_loop, _mqtt_connected_rx) = MqttPublisher::new(&config)?;
    info!("📡 MQTT Publisher initialized for {}:{}", config.mqtt_host, config.mqtt_port);

    // 4. Start the MQTT Event Loop
    tokio::spawn(MqttPublisher::run_event_loop(
        event_loop,
        mqtt.mqtt_connected_sender(),
    ));

    // 5. Start Background Tasks (Decoder and Status Publisher only)
    let decoder_handle = tokio::spawn(decoder::run_decoder(
        config_rx.clone(),
        status_tx.clone(),
        serial_connected_tx,
    ));

    tokio::spawn(publish_status_loop(
        config_rx.clone(),
        status_rx.clone(),
        mqtt.clone(),
    ));

    // 6. Start the Web Server
    let app = build_router(shared_config, config_tx, status_tx, status_rx, mqtt);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    info!("✅ HTTP Server listening on 0.0.0.0:8080");

    tokio::select! {
        result = axum::serve(listener, app) => {
            if let Err(err) = result {
                error!(error=?err, "http server exited with error");
            }
        }
        _ = decoder_handle => {
            error!("decoder task terminated unexpectedly");
        }
    }

    Ok(())
}

fn build_router(
    shared_config: SharedConfig,
    config_tx: watch::Sender<AppConfig>,
    status_tx: watch::Sender<NormalizedScoreboardStatus>,
    status_rx: watch::Receiver<NormalizedScoreboardStatus>,
    mqtt: MqttPublisher,
) -> Router {
    web::router(web::WebState {
        config: shared_config,
        config_tx,
        status_tx,
        status_rx,
        mqtt,
    })
}

async fn publish_status_loop(
    mut config_rx: watch::Receiver<AppConfig>,
    status_rx: watch::Receiver<NormalizedScoreboardStatus>,
    mqtt: MqttPublisher,
) {
    loop {
        let cfg = config_rx.borrow().clone();
        let interval_ms = cfg.publish_interval_ms.max(100);
        let payload = status_rx.borrow().clone();
        
        // This is the ONLY data actively published by the gateway
        if let Err(err) = mqtt
            .publish_json(&cfg.mqtt_topic, &payload, cfg.mqtt_retain)
            .await
        {
            error!(error = ?err, "failed to publish status");
        }

        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(interval_ms)) => {}
            _ = config_rx.changed() => {}
        }
    }
}
