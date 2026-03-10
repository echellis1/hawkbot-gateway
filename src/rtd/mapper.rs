use chrono::Utc;
use serde_json::json;

use crate::{
    config::AppConfig,
    decoder::{rtd_profile_for_sport_name, SportKind},
    schema::NormalizedScoreboardStatus,
};

use super::state::{RtdFieldJustification, RtdState};

#[derive(Clone, Copy)]
struct FieldMap {
    main_clock: (usize, usize),
    home_score: (usize, usize),
    away_score: (usize, usize),
    period: (usize, usize),
}

fn map_for_sport(sport: SportKind) -> FieldMap {
    match sport {
        SportKind::Basketball => FieldMap {
            main_clock: (1, 7),
            home_score: (8, 3),
            away_score: (11, 3),
            period: (14, 1),
        },
        SportKind::Volleyball => FieldMap {
            main_clock: (1, 7),
            home_score: (8, 3),
            away_score: (11, 3),
            period: (14, 1),
        },
        SportKind::Football => FieldMap {
            main_clock: (1, 7),
            home_score: (8, 3),
            away_score: (11, 3),
            period: (14, 1),
        },
        SportKind::Soccer => FieldMap {
            main_clock: (1, 7),
            home_score: (8, 3),
            away_score: (11, 3),
            period: (14, 1),
        },
        SportKind::Lacrosse => FieldMap {
            main_clock: (1, 7),
            home_score: (8, 3),
            away_score: (11, 3),
            period: (14, 1),
        },
    }
}

pub fn to_normalized(
    cfg: &AppConfig,
    state: &RtdState,
    packet_count: u64,
) -> NormalizedScoreboardStatus {
    let sport = SportKind::from_sport_name(&cfg.sport_type);
    let map = map_for_sport(sport);

    let clock_main = state
        .field_str(
            map.main_clock.0,
            map.main_clock.1,
            RtdFieldJustification::Left,
        )
        .ok()
        .map(ToOwned::to_owned);
    let home_score = state
        .field_i32(
            map.home_score.0,
            map.home_score.1,
            RtdFieldJustification::Right,
        )
        .ok()
        .and_then(|n| u16::try_from(n).ok());
    let away_score = state
        .field_i32(
            map.away_score.0,
            map.away_score.1,
            RtdFieldJustification::Right,
        )
        .ok()
        .and_then(|n| u16::try_from(n).ok());
    let period = state
        .field_i32(map.period.0, map.period.1, RtdFieldJustification::Right)
        .ok()
        .and_then(|n| u16::try_from(n).ok());

    NormalizedScoreboardStatus {
        schema_version: 1,
        timestamp_rfc3339: Utc::now().to_rfc3339(),
        controller_type: cfg.controller_type.clone(),
        sport_type: cfg.sport_type.clone(),
        clock_main,
        clock_secondary: None,
        segment_kind: Some("period".to_string()),
        segment_number: period,
        segment_text: None,
        home_score,
        away_score,
        home_timeouts: None,
        away_timeouts: None,
        possession: state
            .field_bool(15)
            .ok()
            .map(|is_home| if is_home { "home" } else { "away" }.to_string()),
        extras: json!({
            "decoder_note": "rtd framed parser with stateful packet application",
            "rtd_profile": rtd_profile_for_sport_name(&cfg.sport_type),
            "packet_count": packet_count,
            "state_size": state.snapshot().len(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_scores_from_state() {
        let cfg = AppConfig::default();
        let mut state = RtdState::with_capacity(64);
        state
            .apply_packet(&crate::rtd::packet::Packet {
                start_index: 0,
                data: b"12:34  0450991H".to_vec(),
            })
            .expect("packet apply");

        let payload = to_normalized(&cfg, &state, 4);

        assert_eq!(payload.clock_main.as_deref(), Some("12:34"));
        assert_eq!(payload.home_score, Some(45));
        assert_eq!(payload.away_score, Some(99));
        assert_eq!(payload.segment_number, Some(1));
        assert_eq!(payload.extras["packet_count"], 4);
    }
}
