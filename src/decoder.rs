use crate::config::AppConfig;
use crate::schema::NormalizedScoreboardStatus;
use chrono::Utc;
use serde_json::json;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::sync::watch;
use tokio_serial::SerialPortBuilderExt;
use tracing::{info, warn};

const FOOTBALL_RTD_FRAME_LEN: usize = 240;

#[derive(Clone, Copy)]
enum FieldJustification {
    Left,
    Right,
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
        match name.trim().to_ascii_lowercase().as_str() {
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
                let mut frame_buffer = Vec::<u8>::new();
                loop {
                    tokio::select! {
                        read_result = serial.read(&mut buffer) => {
                            match read_result {
                                Ok(n) if n > 0 => {
                                    let read_bytes = &buffer[..n];
                                    if matches!(selected_sport, SportKind::Football) {
                                        frame_buffer.extend_from_slice(read_bytes);
                                        let frames = drain_fixed_frames(&mut frame_buffer, FOOTBALL_RTD_FRAME_LEN);
                                        for frame in frames {
                                            let payload = synthesize_payload(&cfg, &frame);
                                            let _ = status_tx.send(payload);
                                        }
                                    } else {
                                        let payload = synthesize_payload(&cfg, read_bytes);
                                        let _ = status_tx.send(payload);
                                    }
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
    let sport = SportKind::from_sport_name(&cfg.sport_type);
    let mut payload = NormalizedScoreboardStatus {
        schema_version: 1,
        timestamp_rfc3339: Utc::now().to_rfc3339(),
        controller_type: cfg.controller_type.clone(),
        sport_type: cfg.sport_type.clone(),
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
        extras: json!({
            "rtd_profile": sport.rtd_profile_name(),
            "sample_bytes": bytes.len(),
        }),
    };

    if matches!(sport, SportKind::Football) {
        payload.clock_main =
            parse_clock_pair(bytes, 1, 6).or_else(|| parse_clock_pair(bytes, 14, 19));
        payload.home_score = parse_u16_field(bytes, 108, 4, FieldJustification::Right);
        payload.away_score = parse_u16_field(bytes, 112, 4, FieldJustification::Right);
        payload.home_timeouts = parse_u8_field(bytes, 122, 2, FieldJustification::Right);
        payload.away_timeouts = parse_u8_field(bytes, 130, 2, FieldJustification::Right);
        payload.segment_kind = Some("quarter".to_string());
        payload.segment_number = parse_u16_field(bytes, 142, 2, FieldJustification::Right);
        payload.possession = parse_possession(bytes);

        payload.extras = json!({
            "rtd_profile": sport.rtd_profile_name(),
            "sample_bytes": bytes.len(),
            "football_fields": {
                "clock_item_1": parse_text_field(bytes, 1, 2, FieldJustification::Right),
                "clock_item_2": parse_text_field(bytes, 6, 2, FieldJustification::Right),
                "clock_item_3": parse_text_field(bytes, 14, 2, FieldJustification::Right),
                "clock_item_4": parse_text_field(bytes, 19, 2, FieldJustification::Right),
                "possession_home_indicator": parse_text_field(bytes, 210, 1, FieldJustification::Left),
                "possession_home_text": parse_text_field(bytes, 211, 4, FieldJustification::Left),
                "possession_guest_indicator": parse_text_field(bytes, 215, 1, FieldJustification::Left),
                "possession_guest_text": parse_text_field(bytes, 216, 4, FieldJustification::Left),
                "down": parse_u8_field(bytes, 222, 2, FieldJustification::Right),
                "to_go": parse_u8_field(bytes, 225, 2, FieldJustification::Right),
                "ball_on": parse_u8_field(bytes, 220, 2, FieldJustification::Right),
            }
        });
    }

    payload
}

fn rtd_to_slice_range(
    position_1_based: usize,
    len: usize,
    total_len: usize,
) -> Option<(usize, usize)> {
    if position_1_based == 0 || len == 0 {
        return None;
    }
    let start = position_1_based.checked_sub(1)?;
    let end = start.checked_add(len)?;
    (end <= total_len).then_some((start, end))
}

fn parse_text_field(
    bytes: &[u8],
    position_1_based: usize,
    len: usize,
    justification: FieldJustification,
) -> Option<String> {
    let (start, end) = rtd_to_slice_range(position_1_based, len, bytes.len())?;
    let raw = String::from_utf8_lossy(&bytes[start..end]);
    let normalized = match justification {
        FieldJustification::Left => raw.trim_end(),
        FieldJustification::Right => raw.trim_start(),
    }
    .trim_matches(char::from(0))
    .trim();

    (!normalized.is_empty()).then(|| normalized.to_string())
}

fn parse_u16_field(
    bytes: &[u8],
    position_1_based: usize,
    len: usize,
    justification: FieldJustification,
) -> Option<u16> {
    parse_text_field(bytes, position_1_based, len, justification)?
        .parse()
        .ok()
}

fn parse_u8_field(
    bytes: &[u8],
    position_1_based: usize,
    len: usize,
    justification: FieldJustification,
) -> Option<u8> {
    parse_text_field(bytes, position_1_based, len, justification)?
        .parse()
        .ok()
}

fn parse_clock_pair(bytes: &[u8], minutes_pos: usize, seconds_pos: usize) -> Option<String> {
    let minutes = parse_u16_field(bytes, minutes_pos, 2, FieldJustification::Right)?;
    let seconds = parse_u8_field(bytes, seconds_pos, 2, FieldJustification::Right)?;
    (seconds < 60).then(|| format!("{}:{seconds:02}", minutes))
}

fn parse_possession(bytes: &[u8]) -> Option<String> {
    let home_indicator = parse_text_field(bytes, 210, 1, FieldJustification::Left);
    let guest_indicator = parse_text_field(bytes, 215, 1, FieldJustification::Left);
    let home_text = parse_text_field(bytes, 211, 4, FieldJustification::Left);
    let guest_text = parse_text_field(bytes, 216, 4, FieldJustification::Left);

    if home_indicator.is_some() || home_text.is_some() {
        return Some("home".to_string());
    }
    if guest_indicator.is_some() || guest_text.is_some() {
        return Some("away".to_string());
    }
    None
}

fn drain_fixed_frames(buffer: &mut Vec<u8>, frame_len: usize) -> Vec<Vec<u8>> {
    if frame_len == 0 {
        return Vec::new();
    }

    let frame_count = buffer.len() / frame_len;
    let mut out = Vec::with_capacity(frame_count);
    for _ in 0..frame_count {
        let frame: Vec<u8> = buffer.drain(..frame_len).collect();
        out.push(frame);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{drain_fixed_frames, synthesize_payload};
    use crate::config::AppConfig;

    fn set_field(frame: &mut [u8], position_1_based: usize, value: &str) {
        let start = position_1_based - 1;
        frame[start..start + value.len()].copy_from_slice(value.as_bytes());
    }

    #[test]
    fn payload_uses_sport_specific_rtd_profile() {
        let mut cfg = AppConfig::default();
        cfg.sport_type = "soccer".to_string();

        let payload = synthesize_payload(&cfg, &[1, 2, 3]);

        assert_eq!(payload.sport_type, "soccer");
        assert_eq!(payload.extras["rtd_profile"], "rtd_soccer");
    }

    #[test]
    fn football_payload_uses_rtd_offsets() {
        let mut cfg = AppConfig::default();
        cfg.sport_type = "football".to_string();

        let mut frame = vec![b' '; 240];
        set_field(&mut frame, 1, "12");
        set_field(&mut frame, 6, "34");
        set_field(&mut frame, 108, "  7");
        set_field(&mut frame, 112, " 14");
        set_field(&mut frame, 122, " 3");
        set_field(&mut frame, 130, " 1");
        set_field(&mut frame, 142, " 2");
        set_field(&mut frame, 210, "<");
        set_field(&mut frame, 220, "42");
        set_field(&mut frame, 222, " 3");
        set_field(&mut frame, 225, "10");

        let payload = synthesize_payload(&cfg, &frame);

        assert_eq!(payload.clock_main.as_deref(), Some("12:34"));
        assert_eq!(payload.home_score, Some(7));
        assert_eq!(payload.away_score, Some(14));
        assert_eq!(payload.home_timeouts, Some(3));
        assert_eq!(payload.away_timeouts, Some(1));
        assert_eq!(payload.segment_number, Some(2));
        assert_eq!(payload.possession.as_deref(), Some("home"));
        assert_eq!(payload.extras["football_fields"]["down"], 3);
        assert_eq!(payload.extras["football_fields"]["to_go"], 10);
        assert_eq!(payload.extras["football_fields"]["ball_on"], 42);
    }

    #[test]
    fn drain_fixed_frames_reassembles_partial_reads() {
        let mut buffer = vec![1, 2, 3, 4, 5];
        let frames = drain_fixed_frames(&mut buffer, 2);

        assert_eq!(frames, vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(buffer, vec![5]);
    }

    #[test]
    fn sport_kind_match_is_case_insensitive() {
        let mut cfg = AppConfig::default();
        cfg.sport_type = "Football".to_string();

        let mut frame = vec![b' '; 240];
        set_field(&mut frame, 1, "01");
        set_field(&mut frame, 6, "05");

        let payload = synthesize_payload(&cfg, &frame);
        assert_eq!(payload.clock_main.as_deref(), Some("1:05"));
    }
}
