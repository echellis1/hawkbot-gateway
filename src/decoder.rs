use crate::config::AppConfig;
use crate::mqtt::MqttPublisher;
use crate::schema::NormalizedScoreboardStatus;
use chrono::Utc;
use rumqttc::QoS;
use serde::Serialize;
use serde_json::json;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::sync::{watch, Mutex};
use tokio_serial::SerialPortBuilderExt;
use tracing::{debug, info, warn};

pub const RAW_PREVIEW_MAX_BYTES: usize = 96;
pub const SERIAL_DEBUG_BUFFER_CAPACITY: usize = 200;

pub type SharedSerialDebugBuffer = Arc<Mutex<VecDeque<SerialDebugSample>>>;

#[derive(Debug, Clone, Serialize)]
pub struct SerialDebugSample {
    pub timestamp_rfc3339: String,
    pub byte_len: usize,
    pub hex_preview: String,
    pub ascii_preview: String,
    pub sport: String,
    pub rtd_profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_index: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
enum SportKind {
    Basketball,
    Volleyball,
    Football,
    Soccer,
    Lacrosse,
}

impl SportKind {
    fn from_sport_name(name: &str) -> Self {
        match name {
            "volleyball" => Self::Volleyball,
            "football" => Self::Football,
            "soccer" => Self::Soccer,
            "lacrosse" | "hockey" => Self::Lacrosse,
            _ => Self::Basketball,
        }
    }

    fn rtd_profile_name(self) -> &'static str {
        match self {
            Self::Basketball => "rtd_basketball",
            Self::Volleyball => "rtd_volleyball",
            Self::Football => "rtd_football",
            Self::Soccer => "rtd_soccer",
            Self::Lacrosse => "rtd_lacrosse",
        }
    }
}

pub(crate) fn rtd_profile_for_sport_name(name: &str) -> &'static str {
    SportKind::from_sport_name(name).rtd_profile_name()
}

fn format_hex_preview(bytes: &[u8], max: usize) -> String {
    let preview_len = bytes.len().min(max);
    let mut out = String::with_capacity(preview_len.saturating_mul(3));
    for (idx, b) in bytes.iter().take(preview_len).enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{b:02X}"));
    }
    if bytes.len() > max {
        out.push_str(" …");
    }
    out
}

fn format_ascii_preview(bytes: &[u8], max: usize) -> String {
    let preview_len = bytes.len().min(max);
    let mut out = String::with_capacity(preview_len);
    for &b in bytes.iter().take(preview_len) {
        let ch = if (0x20..=0x7E).contains(&b) {
            b as char
        } else {
            '.'
        };
        out.push(ch);
    }
    if bytes.len() > max {
        out.push('…');
    }
    out
}

pub async fn run_decoder(
    config_rx: watch::Receiver<AppConfig>,
    status_tx: watch::Sender<NormalizedScoreboardStatus>,
    serial_connected_tx: watch::Sender<bool>,
    mqtt: MqttPublisher,
    serial_debug_samples: SharedSerialDebugBuffer,
) {
    let mut config_rx = config_rx;
    loop {
        let cfg = config_rx.borrow().clone();
        let selected_sport = SportKind::from_sport_name(&cfg.sport_type);

        match tokio_serial::new(&cfg.serial_device, cfg.baud).open_native_async() {
            Ok(mut serial) => {
                let _ = serial_connected_tx.send(true);
                info!(
                    device = %cfg.serial_device,
                    baud = cfg.baud,
                    sport = ?selected_sport,
                    rtd_profile = selected_sport.rtd_profile_name(),
                    "serial connected"
                );

                let mut football_frame_index: u64 = 0;
                let mut buffer = [0u8; 512];
                loop {
                    tokio::select! {
                        read_result = serial.read(&mut buffer) => {
                            match read_result {
                                Ok(n) if n > 0 => {
                                    let read_bytes = &buffer[..n];
                                    if cfg.serial_debug_raw {
                                        debug!(
                                            byte_count = n,
                                            sport = ?selected_sport,
                                            rtd_profile = selected_sport.rtd_profile_name(),
                                            serial_device = %cfg.serial_device,
                                            hex_preview = %format_hex_preview(read_bytes, RAW_PREVIEW_MAX_BYTES),
                                            ascii_preview = %format_ascii_preview(read_bytes, RAW_PREVIEW_MAX_BYTES),
                                            "raw serial frame"
                                        );
                                    }

                                    let frame_index = match selected_sport {
                                        SportKind::Football => {
                                            football_frame_index = football_frame_index.saturating_add(1);
                                            Some(football_frame_index)
                                        }
                                        _ => None,
                                    };

                                    if cfg.serial_debug_publish {
                                        let sample = SerialDebugSample {
                                            timestamp_rfc3339: Utc::now().to_rfc3339(),
                                            byte_len: n,
                                            hex_preview: format_hex_preview(read_bytes, RAW_PREVIEW_MAX_BYTES),
                                            ascii_preview: format_ascii_preview(read_bytes, RAW_PREVIEW_MAX_BYTES),
                                            sport: cfg.sport_type.clone(),
                                            rtd_profile: selected_sport.rtd_profile_name().to_string(),
                                            frame_index,
                                        };

                                        {
                                            let mut guard = serial_debug_samples.lock().await;
                                            if guard.len() >= SERIAL_DEBUG_BUFFER_CAPACITY {
                                                guard.pop_front();
                                            }
                                            guard.push_back(sample.clone());
                                        }

                                        if let Err(err) = mqtt
                                            .publish_json_with_qos(
                                                &cfg.resolved_serial_debug_topic(),
                                                &sample,
                                                QoS::AtMostOnce,
                                                false,
                                            )
                                            .await
                                        {
                                            warn!(error = ?err, "failed to publish serial debug payload");
                                        }
                                    }

                                    let payload = synthesize_payload(&cfg, read_bytes);
                                    let _ = status_tx.send(payload);
                                }
                                Ok(_) => {}
                                Err(err) => {
                                    warn!(error = ?err, "serial read failed");
                                    let _ = serial_connected_tx.send(false);
                                    break;
                                }
                            }
                        }
                        _ = config_rx.changed() => {
                            info!("config changed, restarting decoder");
                            break;
                        }
                    }
                }
            }
            Err(err) => {
                warn!(error = ?err, device = %cfg.serial_device, "serial open failed; retrying");
                let _ = serial_connected_tx.send(false);
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

pub(crate) fn synthesize_payload(cfg: &AppConfig, bytes: &[u8]) -> NormalizedScoreboardStatus {
    let sum = bytes
        .iter()
        .fold(0u16, |acc, b| acc.wrapping_add(*b as u16));
    NormalizedScoreboardStatus {
        schema_version: 1,
        timestamp_rfc3339: Utc::now().to_rfc3339(),
        controller_type: cfg.controller_type.clone(),
        sport_type: cfg.sport_type.clone(),
        clock_main: Some(format!("{}:{:02}", (sum / 60) % 20, sum % 60)),
        clock_secondary: Some(format!("{}", sum % 35)),
        segment_kind: Some("period".to_string()),
        segment_number: Some((sum % 4 + 1) as u16),
        segment_text: None,
        home_score: Some(sum % 120),
        away_score: Some((sum / 2) % 120),
        home_timeouts: Some((sum % 5) as u8),
        away_timeouts: Some(((sum / 3) % 5) as u8),
        possession: Some(if sum % 2 == 0 { "home" } else { "away" }.to_string()),
        extras: json!({
            "decoder_note": "placeholder mapping from RTD bytes",
            "sample_bytes": bytes.len(),
            "rtd_profile": rtd_profile_for_sport_name(&cfg.sport_type),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::synthesize_payload;
    use crate::config::AppConfig;

    #[test]
    fn payload_uses_sport_specific_rtd_profile() {
        let mut cfg = AppConfig::default();
        cfg.sport_type = "soccer".to_string();

        let payload = synthesize_payload(&cfg, &[1, 2, 3]);

        assert_eq!(payload.sport_type, "soccer");
        assert_eq!(payload.extras["rtd_profile"], "rtd_soccer");
    }
}
