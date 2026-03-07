use crate::config::AppConfig;
use anyhow::Result;
use rumqttc::{AsyncClient, Event, EventLoop, LastWill, MqttOptions, Packet, QoS};
use serde::Serialize;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{error, info, warn};

pub const HEALTH_TOPIC: &str = "scoreboard/health";

#[derive(Clone)]
pub struct MqttPublisher {
    client: AsyncClient,
    status_topic: String,
    mqtt_connected_tx: watch::Sender<bool>,
}

impl MqttPublisher {
    pub fn new(config: &AppConfig) -> Result<(Self, EventLoop, watch::Receiver<bool>)> {
        let mut options = MqttOptions::new("hawkbot-gateway", &config.mqtt_host, config.mqtt_port);
        options.set_keep_alive(Duration::from_secs(10));
        options.set_last_will(LastWill::new(
            HEALTH_TOPIC,
            r#"{"ok":false,"message":"disconnected"}"#,
            QoS::AtLeastOnce,
            true,
        ));
        let (client, event_loop) = AsyncClient::new(options, 100);
        let (tx, rx) = watch::channel(false);
        Ok((
            Self {
                client,
                status_topic: config.mqtt_topic.clone(),
                mqtt_connected_tx: tx,
            },
            event_loop,
            rx,
        ))
    }

    pub async fn publish_json<T: Serialize>(
        &self,
        topic: &str,
        payload: &T,
        retain: bool,
    ) -> Result<()> {
        let bytes = serde_json::to_vec(payload)?;
        self.client
            .publish(topic, QoS::AtLeastOnce, retain, bytes)
            .await?;
        Ok(())
    }

    pub async fn run_event_loop(mut event_loop: EventLoop, mqtt_connected_tx: watch::Sender<bool>) {
        loop {
            match event_loop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    let _ = mqtt_connected_tx.send(true);
                    info!("MQTT connected");
                }
                Ok(_) => {}
                Err(err) => {
                    let _ = mqtt_connected_tx.send(false);
                    warn!(error = ?err, "MQTT event loop error, reconnecting");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    pub fn mqtt_connected_sender(&self) -> watch::Sender<bool> {
        self.mqtt_connected_tx.clone()
    }

    pub fn status_topic(&self) -> &str {
        &self.status_topic
    }

    pub async fn publish_online(&self) {
        let payload = serde_json::json!({
            "ok": true,
            "message": "online"
        });

        if let Err(err) = self.publish_json(HEALTH_TOPIC, &payload, true).await {
            error!(error = ?err, "failed to publish online status");
        }
    }

}
