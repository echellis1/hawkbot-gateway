use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{env, path::Path, sync::Arc};
use tokio::{fs, sync::RwLock};

pub const CONFIG_PATH: &str = "config.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub controller_type: String,
    pub sport_type: String,
    pub serial_device: String,
    pub baud: u32,

    pub mqtt_host: String,
    pub mqtt_port: u16,
    pub mqtt_topic: String,
    pub mqtt_retain: bool,
    pub publish_interval_ms: u64,

    pub admin_user: String,
    pub admin_pass: String,

    pub mqtt_client_id: String,
    pub mqtt_username: Option<String>,
    pub mqtt_password: Option<String>,
    pub mqtt_use_tls: bool,
    pub mqtt_ca_file: Option<String>,
    pub mqtt_cert_file: Option<String>,
    pub mqtt_key_file: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            controller_type: "all_sport_5000".to_string(),
            sport_type: "basketball".to_string(),
            serial_device: "/dev/ttyUSB0".to_string(),
            baud: 19200,

            mqtt_host: "127.0.0.1".to_string(),
            mqtt_port: 1883,
            mqtt_topic: "scoreboard/status".to_string(),
            mqtt_retain: true,
            publish_interval_ms: 100,

            admin_user: "admin".to_string(),
            admin_pass: "admin".to_string(),

            mqtt_client_id: "hawkbot-gateway".to_string(),
            mqtt_username: None,
            mqtt_password: None,
            mqtt_use_tls: false,
            mqtt_ca_file: None,
            mqtt_cert_file: None,
            mqtt_key_file: None,
        }
    }
}

pub type SharedConfig = Arc<RwLock<AppConfig>>;

impl AppConfig {
    pub fn apply_env_overrides(mut self) -> Self {
        if let Ok(v) = env::var("CONTROLLER_TYPE") {
            self.controller_type = v;
        }
        if let Ok(v) = env::var("SPORT_TYPE") {
            self.sport_type = v;
        }
        if let Ok(v) = env::var("SERIAL_DEVICE") {
            self.serial_device = v;
        }
        if let Ok(v) = env::var("BAUD") {
            if let Ok(parsed) = v.parse::<u32>() {
                self.baud = parsed;
            }
        }

        if let Ok(v) = env::var("MQTT_HOST") {
            self.mqtt_host = v;
        }
        if let Ok(v) = env::var("MQTT_PORT") {
            if let Ok(parsed) = v.parse::<u16>() {
                self.mqtt_port = parsed;
            }
        }
        if let Ok(v) = env::var("MQTT_TOPIC") {
            self.mqtt_topic = v;
        }
        if let Ok(v) = env::var("MQTT_RETAIN") {
            self.mqtt_retain = parse_bool(&v).unwrap_or(self.mqtt_retain);
        }
        if let Ok(v) = env::var("PUBLISH_INTERVAL_MS") {
            if let Ok(parsed) = v.parse::<u64>() {
                self.publish_interval_ms = parsed;
            }
        }

        if let Ok(v) = env::var("ADMIN_USER") {
            self.admin_user = v;
        }
        if let Ok(v) = env::var("ADMIN_PASS") {
            self.admin_pass = v;
        }

        if let Ok(v) = env::var("MQTT_CLIENT_ID") {
            self.mqtt_client_id = v;
        }
        if let Ok(v) = env::var("MQTT_USERNAME") {
            self.mqtt_username = if v.trim().is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = env::var("MQTT_PASSWORD") {
            self.mqtt_password = if v.trim().is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = env::var("MQTT_USE_TLS") {
            self.mqtt_use_tls = parse_bool(&v).unwrap_or(self.mqtt_use_tls);
        }
        if let Ok(v) = env::var("MQTT_CA_FILE") {
            self.mqtt_ca_file = if v.trim().is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = env::var("MQTT_CERT_FILE") {
            self.mqtt_cert_file = if v.trim().is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = env::var("MQTT_KEY_FILE") {
            self.mqtt_key_file = if v.trim().is_empty() { None } else { Some(v) };
        }

        self
    }

    pub fn validate(&self) -> Result<()> {
        if self.serial_device.trim().is_empty() {
            anyhow::bail!("serial_device cannot be empty");
        }

        if self.mqtt_host.trim().is_empty() {
            anyhow::bail!("mqtt_host cannot be empty");
        }

        if self.mqtt_port == 0 {
            anyhow::bail!("mqtt_port must be greater than 0");
        }

        if self.mqtt_topic.trim().is_empty() {
            anyhow::bail!("mqtt_topic cannot be empty");
        }

        if self.mqtt_client_id.trim().is_empty() {
            anyhow::bail!("mqtt_client_id cannot be empty");
        }

        if self.mqtt_use_tls && self.mqtt_ca_file.is_none() {
            anyhow::bail!("mqtt_use_tls is true but mqtt_ca_file is not set");
        }

        Ok(())
    }
}

pub async fn load_or_create_config(path: &str) -> Result<AppConfig> {
    let cfg = if Path::new(path).exists() {
        let text = fs::read_to_string(path)
            .await
            .with_context(|| format!("reading config from {path}"))?;

        serde_json::from_str::<AppConfig>(&text).with_context(|| format!("parsing config JSON from {path}"))?
    } else {
        let cfg = AppConfig::default();
        save_config(path, &cfg).await?;
        cfg
    };

    let cfg = cfg.apply_env_overrides();
    cfg.validate()?;
    Ok(cfg)
}

pub async fn save_config(path: &str, cfg: &AppConfig) -> Result<()> {
    let text = serde_json::to_string_pretty(cfg).context("serializing config to JSON")?;

    fs::write(path, text)
        .await
        .with_context(|| format!("writing config to {path}"))?;

    Ok(())
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}
