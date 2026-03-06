use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NormalizedScoreboardStatus {
    pub schema_version: u8,
    pub timestamp_rfc3339: String,
    pub controller_type: String,
    pub sport_type: String,
    pub clock_main: Option<String>,
    pub clock_secondary: Option<String>,
    pub segment_kind: Option<String>,
    pub segment_number: Option<u16>,
    pub segment_text: Option<String>,
    pub home_score: Option<u16>,
    pub away_score: Option<u16>,
    pub home_timeouts: Option<u8>,
    pub away_timeouts: Option<u8>,
    pub possession: Option<String>,
    pub extras: Value,
}

impl NormalizedScoreboardStatus {
    pub fn blank(controller_type: &str, sport_type: &str) -> Self {
        Self {
            schema_version: 1,
            timestamp_rfc3339: Utc::now().to_rfc3339(),
            controller_type: controller_type.to_string(),
            sport_type: sport_type.to_string(),
            clock_main: None,
            clock_secondary: None,
            segment_kind: None,
            segment_number: None,
            segment_text: None,
            home_score: None,
            away_score: None,
            home_timeouts: None,
            away_timeouts: None,
            possession: None,
            extras: json!({}),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthStatus {
    pub ok: bool,
    pub serial_connected: bool,
    pub decoder_running: bool,
    pub mqtt_connected: bool,
    pub timestamp_rfc3339: String,
    pub message: String,
}

impl HealthStatus {
    pub fn running(serial_connected: bool, mqtt_connected: bool) -> Self {
        Self {
            ok: serial_connected && mqtt_connected,
            serial_connected,
            decoder_running: true,
            mqtt_connected,
            timestamp_rfc3339: Utc::now().to_rfc3339(),
            message: if serial_connected {
                "running".to_string()
            } else {
                "waiting for serial".to_string()
            },
        }
    }
}
