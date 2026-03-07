use crate::config::AppConfig;
use anyhow::{Context, Result};
use rumqttc::{AsyncClient, Event, EventLoop, LastWill, MqttOptions, Packet, QoS, Transport};
use serde::Serialize;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{error, info, warn};
use std::fs;

// Compatible types for rumqttc 0.24.0 (rustls 0.21/0.22)
use rumqttc::tokio_rustls::rustls::{ClientConfig, RootCertStore, Certificate, PrivateKey};

pub const HEALTH_TOPIC: &str = "scoreboard/health";

#[derive(Clone)]
pub struct MqttPublisher {
    client: AsyncClient,
    status_topic: String,
    mqtt_connected_tx: watch::Sender<bool>,
}

impl MqttPublisher {
    pub fn new(config: &AppConfig) -> Result<(Self, EventLoop, watch::Receiver<bool>)> {
        let mut options = MqttOptions::new(&config.mqtt_client_id, &config.mqtt_host, config.mqtt_port);
        options.set_keep_alive(Duration::from_secs(10));
        
        // Credentials check
        if let (Some(u), Some(p)) = (&config.mqtt_username, &config.mqtt_password) {
            options.set_credentials(u, p);
        }

        options.set_last_will(LastWill::new(
            HEALTH_TOPIC,
            r#"{"ok":false,"message":"disconnected"}"#,
            QoS::AtLeastOnce,
            true,
        ));

        // --- mTLS Configuration ---
        if config.mqtt_use_tls {
            let ca_path = config.mqtt_ca_file.as_ref().context("Missing CA file path in config")?;
            let cert_path = config.mqtt_cert_file.as_ref().context("Missing Cert file path in config")?;
            let key_path = config.mqtt_key_file.as_ref().context("Missing Key file path in config")?;

            let ca_bytes = fs::read(ca_path).context("Failed to read CA file")?;
            let cert_bytes = fs::read(cert_path).context("Failed to read Client Cert file")?;
            let key_bytes = fs::read(key_path).context("Failed to read Client Key file")?;

            let mut root_store = RootCertStore::empty();
            root_store.add(&Certificate(ca_bytes))
                .map_err(|e| anyhow::anyhow!("Failed to add CA to root store: {}", e))?;

            let cert_chain = vec![Certificate(cert_bytes)];
            let private_key = PrivateKey(key_bytes);

            // Using with_safe_defaults() required for this rustls version
            let client_config = ClientConfig::builder()
                .with_safe_defaults()
                .with_root_certificates(root_store)
                .with_client_auth_cert(cert_chain, private_key)
                .map_err(|e| anyhow::anyhow!("TLS Config Error: {}", e))?;

            options.set_transport(Transport::tls_with_config(client_config.into()));
        }

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

    /// Publishes the current application configuration to a dedicated topic
    pub async fn publish_config(&self, config: &AppConfig) {
        let topic = format!("devices/{}/config", config.mqtt_client_id);
        if let Err(err) = self.publish_json(&topic, config, true).await {
            error!(error = ?err, "Failed to publish config to {}", topic);
        } else {
            info!("Successfully published config to {}", topic);
        }
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
                    info!("✅ MQTT Connection Established");
                }
                Ok(Event::Incoming(Packet::Disconnect)) | Ok(Event::Outgoing(rumqttc::Outgoing::Disconnect)) => {
                    let _ = mqtt_connected_tx.send(false);
                    warn!("MQTT Connection Lost");
                }
                Ok(_) => {} // Handle other packets (SubAck, PubAck, etc.) if needed
                Err(err) => {
                    let _ = mqtt_connected_tx.send(false);
                    error!(error = ?err, "MQTT poll error, attempting reconnect in 5s...");
                    tokio::time::sleep(Duration::from_secs(5)).await;
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
            error!(error = ?err, "Failed to publish online health status");
        }
    }
}
