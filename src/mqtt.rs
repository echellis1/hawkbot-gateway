use crate::config::AppConfig;
use anyhow::{Context, Result};
use rumqttc::{AsyncClient, Event, EventLoop, LastWill, MqttOptions, Packet, QoS, Transport};
use serde::Serialize;
use std::fs;
use std::io::BufReader;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{error, info};

#[derive(Clone)]
pub struct MqttPublisher {
    client: AsyncClient,
    base_topic: String,
    health_topic: String,
    mqtt_connected_tx: watch::Sender<bool>,
}

impl MqttPublisher {
    pub fn new(config: &AppConfig) -> Result<(Self, EventLoop, watch::Receiver<bool>)> {
        let base_topic = format!("scoreboard/{}", config.mqtt_client_id);
        let health_topic = format!("{}/health", base_topic);

        let mut options =
            MqttOptions::new(&config.mqtt_client_id, &config.mqtt_host, config.mqtt_port);
        options.set_keep_alive(Duration::from_secs(10));

        if let (Some(u), Some(p)) = (&config.mqtt_username, &config.mqtt_password) {
            options.set_credentials(u, p);
        }

        options.set_last_will(LastWill::new(
            &health_topic,
            r#"{"ok":false,"message":"disconnected"}"#,
            QoS::AtLeastOnce,
            true,
        ));

        if config.mqtt_use_tls {
            let ca_path = config.mqtt_ca_file.as_ref().context("Missing CA file path")?;
            let cert_path = config.mqtt_cert_file.as_ref().context("Missing Cert file path")?;
            let key_path = config.mqtt_key_file.as_ref().context("Missing Key file path")?;

            let ca_bytes = fs::read(ca_path).context("Failed to read CA")?;
            let cert_bytes = fs::read(cert_path).context("Failed to read Cert")?;
            let key_bytes = fs::read(key_path).context("Failed to read Key")?;

            let mut root_store = rumqttc::tokio_rustls::rustls::RootCertStore::empty();
            let mut ca_reader = BufReader::new(&ca_bytes[..]);

            let ca_certs: Vec<_> = rustls_pemfile::certs(&mut ca_reader)
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("Failed to parse CA certificates")?;

            if ca_certs.is_empty() {
                anyhow::bail!("No certificates found in CA file at {}", ca_path);
            }

            for cert in ca_certs {
                root_store
                    .add(cert)
                    .map_err(|e| anyhow::anyhow!("CA Store Error: {}", e))?;
            }

            let mut cert_reader = BufReader::new(&cert_bytes[..]);
            let cert_chain: Vec<_> = rustls_pemfile::certs(&mut cert_reader)
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("Failed to parse Client certificate")?;

            let mut key_reader = BufReader::new(&key_bytes[..]);
            let key_der = rustls_pemfile::private_key(&mut key_reader)
                .map_err(|e| anyhow::anyhow!("Key Parse Error: {}", e))?
                .context("No private key found in key file.")?;

            let private_key =
                rumqttc::tokio_rustls::rustls::pki_types::PrivateKeyDer::from(key_der);

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
                base_topic,
                health_topic,
                mqtt_connected_tx: tx,
            },
            event_loop,
            rx,
        ))
    }

    pub fn status_topic(&self) -> String {
        format!("{}/status", self.base_topic)
    }

    pub fn json_topic(&self) -> String {
        format!("{}/json", self.base_topic)
    }

    pub fn xml_topic(&self) -> String {
        format!("{}/xml", self.base_topic)
    }

    pub fn config_topic(&self) -> String {
        format!("{}/config", self.base_topic)
    }

    pub fn health_topic(&self) -> &str {
        &self.health_topic
    }

    pub fn command_topic(client_id: &str) -> String {
        format!("devices/{}/commands/#", client_id)
    }

    pub async fn publish_config(&self, config: &AppConfig) {
        let _ = self.publish_json(&self.config_topic(), config, true).await;
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

    pub async fn run_event_loop(
        mut event_loop: EventLoop,
        mqtt_connected_tx: watch::Sender<bool>,
    ) {
        loop {
            match event_loop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    let _ = mqtt_connected_tx.send(true);
                    info!("✅ MQTT Connected");
                }
                Ok(_) => {}
                Err(err) => {
                    let _ = mqtt_connected_tx.send(false);
                    error!(error = ?err, "MQTT Connection Lost");
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
        let _ = self.publish_json(self.health_topic(), &payload, true).await;
    }
}
