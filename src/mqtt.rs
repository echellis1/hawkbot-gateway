use crate::config::AppConfig;
use anyhow::Result;
use rumqttc::{AsyncClient, Event, EventLoop, LastWill, MqttOptions, Packet, QoS, Transport};
use serde::Serialize;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{error, info, warn};
use std::fs;

// Import TLS types re-exported by rumqttc to avoid version mismatch
use rumqttc::tokio_rustls::rustls::{ClientConfig, RootCertStore, CertificateDer, PrivateKeyDer};

pub const HEALTH_TOPIC: &str = "scoreboard/health";

#[derive(Clone)]
pub struct MqttPublisher {
    client: AsyncClient,
    status_topic: String,
    mqtt_connected_tx: watch::Sender<bool>,
}

impl MqttPublisher {
    pub fn new(config: &AppConfig) -> Result<(Self, EventLoop, watch::Receiver<bool>)> {
        // Use the Client ID from config
        let mut options = MqttOptions::new(&config.mqtt_client_id, &config.mqtt_host, config.mqtt_port);
        options.set_keep_alive(Duration::from_secs(10));
        
        // Handle Authentication if provided
        if let (Some(u), Some(p)) = (&config.mqtt_username, &config.mqtt_password) {
            options.set_credentials(u, p);
        }

        options.set_last_will(LastWill::new(
            HEALTH_TOPIC,
            r#"{"ok":false,"message":"disconnected"}"#,
            QoS::AtLeastOnce,
            true,
        ));

        // --- TLS CONFIGURATION ---
        if config.mqtt_use_tls {
            let ca = fs::read(config.mqtt_ca_file.as_ref().context("Missing CA file path")?)?;
            let cert = fs::read(config.mqtt_cert_file.as_ref().context("Missing Cert file path")?)?;
            let key = fs::read(config.mqtt_key_file.as_ref().context("Missing Key file path")?)?;

            let mut root_store = RootCertStore::empty();
            root_store.add(CertificateDer::from(ca))?;

            let cert_chain = vec![CertificateDer::from(cert)];
            let private_key = PrivateKeyDer::from_pem_slice(&key)?;

            let client_config = ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_client_auth_cert(cert_chain, private_key)?;

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

    // This is the method main.rs was looking for!
    pub async fn publish_config(&self, config: &AppConfig) {
        let topic = format!("devices/{}/config", config.mqtt_client_id);
        if let Err(err) = self.publish_json(&topic, config, true).await {
            error!(error = ?err, "failed to publish config");
        } else {
            info!("Published config to {}", topic);
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
                    info!("✅ MQTT Connected to broker");
                }
                Ok(Event::Incoming(Packet::Disconnect)) | Ok(Event::Outgoing(rumqttc::Outgoing::Disconnect)) => {
                    let _ = mqtt_connected_tx.send(false);
                    warn!("MQTT Disconnected");
                }
                Ok(_) => {}
                Err(err) => {
                    let _ = mqtt_connected_tx.send(false);
                    error!(error = ?err, "MQTT connection error, retrying...");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    pub fn mqtt_connected_sender(&self) -> watch::Sender<bool> {
        self.mqtt_connected_tx.clone()
    }

    pub async fn publish_online(&self) {
        let payload = serde_json::json!({ "ok": true, "message": "online" });
        if let Err(err) = self.publish_json(HEALTH_TOPIC, &payload, true).await {
            error!(error = ?err, "failed to publish online status");
        }
    }
}
