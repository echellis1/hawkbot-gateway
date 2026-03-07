use crate::config::AppConfig;
use anyhow::{Context, Result};
use rumqttc::{AsyncClient, Event, EventLoop, LastWill, MqttOptions, Packet, QoS, Transport};
use serde::Serialize;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{error, info}; // Removed unused 'warn' to clear the warning
use std::fs;

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
            let ca_path = config.mqtt_ca_file.as_ref().context("Missing CA file path")?;
            let cert_path = config.mqtt_cert_file.as_ref().context("Missing Cert file path")?;
            let key_path = config.mqtt_key_file.as_ref().context("Missing Key file path")?;

            let ca_bytes = fs::read(ca_path).context("Failed to read CA")?;
            let cert_bytes = fs::read(cert_path).context("Failed to read Cert")?;
            let key_bytes = fs::read(key_path).context("Failed to read Key")?;

            // 1. Create Root Store
            let mut root_store = rumqttc::tokio_rustls::rustls::RootCertStore::empty();
            root_store.add(rumqttc::tokio_rustls::rustls::pki_types::CertificateDer::from(ca_bytes))
                .map_err(|e| anyhow::anyhow!("CA Error: {}", e))?;

            // 2. Prepare Client Cert Chain
            let cert_chain = vec![rumqttc::tokio_rustls::rustls::pki_types::CertificateDer::from(cert_bytes)];
            
            // 3. Prepare Private Key (Using the most compatible manual loading method)
            let key_string = std::string::String::from_utf8(key_bytes).context("Key file is not valid UTF-8")?;
            let private_key = rumqttc::tokio_rustls::rustls::pki_types::PrivateKeyDer::from_pem(&key_string)
                .map_err(|e| anyhow::anyhow!("Key Parsing Error (Check if key is PKCS#8): {}", e))?;

            // 4. Build TLS Config
            let client_config = rumqttc::tokio_rustls::rustls::ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_client_auth_cert(cert_chain, private_key)
                .map_err(|e| anyhow::anyhow!("TLS Builder Error: {}", e))?;

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

    pub async fn publish_config(&self, config: &AppConfig) {
        let topic = format!("devices/{}/config", config.mqtt_client_id);
        if let Err(err) = self.publish_json(&topic, config, true).await {
            error!(error = ?err, "failed to publish config");
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
                    info!("✅ MQTT Connected");
                }
                Ok(_) => {}
                Err(err) => {
                    let _ = mqtt_connected_tx.send(false);
                    error!(error = ?err, "MQTT Error, retrying...");
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
        let _ = self.publish_json(HEALTH_TOPIC, &payload, true).await;
    }
}
