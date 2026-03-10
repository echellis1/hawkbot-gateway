use crate::config::AppConfig;
use crate::rtd::{codec::SerialRtdFramer, mapper::to_normalized, packet::Packet, state::RtdState};
use crate::schema::NormalizedScoreboardStatus;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::sync::watch;
use tokio_serial::SerialPortBuilderExt;
use tracing::{info, warn};

#[derive(Debug, Clone, Copy)]
pub(crate) enum SportKind {
    Basketball,
    Volleyball,
    Football,
    Soccer,
    Lacrosse,
}

impl SportKind {
    pub(crate) fn from_sport_name(name: &str) -> Self {
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
                let mut rx_buffer = Vec::new();
                let mut framer = SerialRtdFramer::default();
                let mut rtd_state = RtdState::with_capacity(1024);
                let mut packet_count = 0_u64;
                loop {
                    tokio::select! {
                        read_result = serial.read(&mut buffer) => {
                            match read_result {
                                Ok(n) if n > 0 => {
                                    framer.push_bytes(&mut rx_buffer, &buffer[..n]);
                                    while let Some(frame) = framer.pop_frame(&mut rx_buffer) {
                                        match Packet::parse(&frame) {
                                            Ok(packet) => {
                                                if let Err(err) = rtd_state.apply_packet(&packet) {
                                                    warn!(error = ?err, start_index = packet.start_index, packet_len = packet.data.len(), "failed to apply packet to RTD state");
                                                    continue;
                                                }
                                                packet_count += 1;
                                                let payload = to_normalized(&cfg, &rtd_state, packet_count);
                                                let _ = status_tx.send(payload);
                                            }
                                            Err(err) => {
                                                warn!(error = ?err, frame_len = frame.len(), "failed to parse RTD frame");
                                            }
                                        }
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

#[cfg(test)]
mod tests {
    use crate::config::AppConfig;
    use crate::rtd::mapper::to_normalized;
    use crate::rtd::packet::Packet;
    use crate::rtd::state::RtdState;

    #[test]
    fn payload_uses_sport_specific_rtd_profile() {
        let mut cfg = AppConfig::default();
        cfg.sport_type = "soccer".to_string();
        let mut state = RtdState::with_capacity(64);
        state
            .apply_packet(&Packet {
                start_index: 0,
                data: b"12:34  0010021 ".to_vec(),
            })
            .expect("packet should apply");

        let payload = to_normalized(&cfg, &state, 1);

        assert_eq!(payload.sport_type, "soccer");
        assert_eq!(payload.extras["rtd_profile"], "rtd_soccer");
    }
}
