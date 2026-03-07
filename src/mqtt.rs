use crate::config::AppConfig;
use anyhow::{Context, Result};
use rumqttc::{AsyncClient, Event, EventLoop, LastWill, MqttOptions, Packet, QoS, Transport};
use serde::Serialize;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{error, info};
use std::fs;
use std::io::BufReader;

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

            // 1. Create Root Store and load CA(s) correctly
            let mut root_store = rumqttc::tokio_rustls::rustls::RootCertStore::empty();
            let mut ca_reader = BufReader::new(&ca_bytes[..]);
            
            // Collect all certs found in the CA file
            let ca_certs: Vec<_> = rustls_pemfile::certs(&mut ca_reader)
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("Failed to parse CA certificates")?;

            if ca_certs.is_empty() {
                anyhow::bail!("No certificates found in CA file at {}", ca_path);
            }

            for cert in ca_certs {
                root_store.add(cert)
                    .map_err(|e| anyhow::anyhow!("CA Store Error: {}", e))?;
            }

            // 2. Prepare Client Cert Chain
            let mut cert_reader = BufReader::new(&cert_bytes[..]);
            let cert_chain: Vec<_> = rustls_pemfile::certs(&mut cert_reader)
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("Failed to parse Client certificate")?;

            // 3. Prepare Private Key
            let mut key_reader = BufReader::new(&key_bytes[..]);
            let key_der = rustls_pemfile::private_key(&mut key_reader)
                .map_err(|e| anyhow::anyhow!("Key Parse Error: {}", e))?
                .context("No private key found in key file. Ensure it is PEM formatted.")?;

            let private_key = rumqttc::tokio_rustls::rustls::pki_types::PrivateKeyDer::from(key_der);

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
        let _ = self.publish_json(&topic, config, true).await;
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
                    info!("✅ MQTT Connected successfully via mTLS");
                }
                Ok(_) => {}
                Err(err) => {
                    let _ = mqtt_connected_tx.send(false);
                    error!(error = ?err, "MQTT Error, retrying connection...");
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
