use crate::config::AppConfig;
use crate::schema::NormalizedScoreboardStatus;
use chrono::Utc;
use serde_json::json;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::sync::watch;
use tokio_serial::SerialPortBuilderExt;
use tracing::{info, warn};

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

pub async fn run_decoder(
    config_rx: watch::Receiver<AppConfig>,
    status_tx: watch::Sender<NormalizedScoreboardStatus>,
    serial_connected_tx: watch::Sender<bool>,
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

                let mut buffer = [0u8; 512];
                loop {
                    tokio::select! {
                        read_result = serial.read(&mut buffer) => {
                            match read_result {
                                Ok(n) if n > 0 => {
                                    let payload = synthesize_payload(&cfg, &buffer[..n]);
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
