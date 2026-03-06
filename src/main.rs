mod config;
mod mqtt;

use crate::config::{load_or_create_config, SharedConfig, CONFIG_PATH};
use crate::mqtt::MqttPublisher;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("starting hawkbot gateway");

    // Load config
    let config = load_or_create_config(CONFIG_PATH).await?;
    let shared_config: SharedConfig = Arc::new(RwLock::new(config.clone()));

    // Initialize MQTT
    let (mqtt, event_loop, mqtt_connected_rx) = MqttPublisher::new(&config)?;

    // Start MQTT event loop
    let mqtt_tx = mqtt.mqtt_connected_sender();
    tokio::spawn(async move {
        MqttPublisher::run_event_loop(event_loop, mqtt_tx).await;
    });

    // Wait for MQTT connection
    let mut rx = mqtt_connected_rx.clone();
    tokio::spawn({
        let mqtt = mqtt.clone();
        async move {
            loop {
                if *rx.borrow() {
                    info!("MQTT connection established");

                    mqtt.publish_online().await;

                    break;
                }

                rx.changed().await.unwrap();
            }
        }
    });

    // Main publish loop
    loop {
        let cfg = shared_config.read().await;

        let topic = mqtt.status_topic();

        let payload = serde_json::json!({
            "bot": cfg.mqtt_client_id,
            "sport": cfg.sport_type,
            "controller": cfg.controller_type,
            "status": "running"
        });

        if let Err(err) = mqtt.publish_json(&topic, &payload, cfg.mqtt_retain).await {
            error!(error = ?err, "failed to publish scoreboard status");
        }

        sleep(Duration::from_millis(cfg.publish_interval_ms)).await;
    }
}
