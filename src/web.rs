use crate::config::{save_config, AppConfig, SharedConfig, CONFIG_PATH};
use crate::decoder::rtd_profile_for_sport_name;
use crate::mqtt::MqttPublisher;
use crate::schema::NormalizedScoreboardStatus;
use axum::extract::{Form, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;
use tokio::sync::watch;

const SUPPORTED_SPORTS: [&str; 5] = ["basketball", "volleyball", "football", "soccer", "lacrosse"];

#[derive(Clone)]
pub struct WebState {
    pub config: SharedConfig,
    pub config_tx: watch::Sender<AppConfig>,
    pub status_tx: watch::Sender<NormalizedScoreboardStatus>,
    pub status_rx: watch::Receiver<NormalizedScoreboardStatus>,
    pub mqtt: MqttPublisher,
}

pub fn router(state: WebState) -> Router {
    Router::new()
        .route("/", get(get_index))
        .route("/status.json", get(get_status_json))
        .route("/%22/status.json/%22", get(get_status_json))
        .route("/\"/status.json/\"", get(get_status_json))
        .route("/admin", get(get_admin).post(post_admin))
        .route("/admin/simulate", axum::routing::post(post_admin_simulate))
        .route("/%22/admin/%22", get(get_admin).post(post_admin))
        .route("/\"/admin/\"", get(get_admin).post(post_admin))
        .with_state(state)
}

async fn get_index() -> Html<&'static str> {
    Html(
        r#"<!doctype html>
<html>
  <head><meta charset="utf-8"/><title>Daktronics Gateway</title></head>
  <body>
    <h1>Daktronics Gateway</h1>
    <p>Gateway is running.</p>
    <ul>
      <li><a href="/status.json">Live status JSON</a></li>
      <li><a href="/admin">Admin settings</a></li>
    </ul>
  </body>
</html>"#,
    )
}

async fn get_status_json(State(state): State<WebState>) -> Json<NormalizedScoreboardStatus> {
    Json(state.status_rx.borrow().clone())
}

async fn get_admin(State(state): State<WebState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers).await {
        return unauthorized();
    }
    let cfg = state.config.read().await.clone();
    Html(render_admin_page(&cfg)).into_response()
}

#[derive(Debug, Deserialize)]
pub struct AdminForm {
    controller_type: String,
    sport_type: String,
    serial_device: String,
    mqtt_host: String,
    mqtt_port: u16,
    mqtt_topic: String,
    publish_interval_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct SimulateForm {
    sport_type: String,
}

async fn post_admin(
    State(state): State<WebState>,
    headers: HeaderMap,
    Form(form): Form<AdminForm>,
) -> Response {
    if !authorized(&state, &headers).await {
        return unauthorized();
    }

    let mut cfg = state.config.read().await.clone();
    cfg.controller_type = form.controller_type;
    cfg.sport_type = form.sport_type;
    cfg.serial_device = form.serial_device;
    cfg.mqtt_host = form.mqtt_host;
    cfg.mqtt_port = form.mqtt_port;
    cfg.mqtt_topic = form.mqtt_topic;
    cfg.publish_interval_ms = form.publish_interval_ms;

    if let Err(err) = save_config(CONFIG_PATH, &cfg).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to save config: {err}"),
        )
            .into_response();
    }

    {
        let mut guard = state.config.write().await;
        *guard = cfg.clone();
    }

    let _ = state.config_tx.send(cfg.clone());

    let mut current_status = state.status_rx.borrow().clone();
    current_status.controller_type = cfg.controller_type.clone();
    current_status.sport_type = cfg.sport_type.clone();
    current_status.timestamp_rfc3339 = chrono::Utc::now().to_rfc3339();
    current_status.extras = serde_json::json!({
        "rtd_profile": rtd_profile_for_sport_name(&cfg.sport_type),
    });
    let _ = state.status_tx.send(current_status);

    let mqtt = state.mqtt.clone();
    let cfg_for_publish = cfg.clone();
    tokio::spawn(async move {
        mqtt.publish_config(&cfg_for_publish).await;
    });

    Html(render_admin_page(&cfg)).into_response()
}

async fn post_admin_simulate(
    State(state): State<WebState>,
    headers: HeaderMap,
    Form(form): Form<SimulateForm>,
) -> Response {
    if !authorized(&state, &headers).await {
        return unauthorized();
    }

    let cfg = state.config.read().await.clone();
    let sport = if SUPPORTED_SPORTS.contains(&form.sport_type.as_str()) {
        form.sport_type.as_str()
    } else {
        cfg.sport_type.as_str()
    };

    let payload = simulated_status_for_sport(&cfg.controller_type, sport);
    let _ = state.status_tx.send(payload.clone());

    let mqtt = state.mqtt.clone();
    let topic = cfg.mqtt_topic.clone();
    let retain = cfg.mqtt_retain;
    tokio::spawn(async move {
        let _ = mqtt.publish_json(&topic, &payload, retain).await;
    });

    Html(render_admin_page(&cfg)).into_response()
}

async fn authorized(state: &WebState, headers: &HeaderMap) -> bool {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return false;
    };
    let Ok(value_str) = value.to_str() else {
        return false;
    };
    let Some(encoded) = value_str.strip_prefix("Basic ") else {
        return false;
    };
    let Ok(decoded) = STANDARD.decode(encoded) else {
        return false;
    };
    let Ok(decoded_str) = String::from_utf8(decoded) else {
        return false;
    };

    let cfg = state.config.read().await;
    decoded_str == format!("{}:{}", cfg.admin_user, cfg.admin_pass)
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=admin")],
        "Unauthorized",
    )
        .into_response()
}

fn render_admin_page(cfg: &AppConfig) -> String {
    let controller_type_select =
        render_select("controller_type", &cfg.controller_type, &["all_sport_5000"]);
    let sport_type_select = render_select("sport_type", &cfg.sport_type, &SUPPORTED_SPORTS);

    let simulation_buttons = SUPPORTED_SPORTS
        .iter()
        .map(|sport| {
            format!(r#"<button type="submit" name="sport_type" value="{sport}">{sport}</button>"#)
        })
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        r#"<!doctype html>
<html>
  <head><meta charset="utf-8"/><title>Scoreboard Admin</title></head>
  <body>
    <h1>Daktronics Gateway Admin</h1>
    <form method="post" action="/admin">
      <label>Controller Type: {}</label><br/>
      <label>Sport Type: {}</label><br/>
      <label>Serial Device: <input name="serial_device" value="{}"/></label><br/>
      <label>MQTT Host: <input name="mqtt_host" value="{}"/></label><br/>
      <label>MQTT Port: <input name="mqtt_port" type="number" value="{}"/></label><br/>
      <label>MQTT Topic: <input name="mqtt_topic" value="{}"/></label><br/>
      <label>Publish Interval (ms): <input name="publish_interval_ms" type="number" value="{}"/></label><br/>
      <button type="submit">Save</button>
    </form>
    <h2>Simulation</h2>
    <p>Publish sample data for testing without a serial feed.</p>
    <form method="post" action="/admin/simulate">
      {}
    </form>
  </body>
</html>"#,
        controller_type_select,
        sport_type_select,
        cfg.serial_device,
        cfg.mqtt_host,
        cfg.mqtt_port,
        cfg.mqtt_topic,
        cfg.publish_interval_ms,
        simulation_buttons
    )
}

fn simulated_status_for_sport(
    controller_type: &str,
    sport_type: &str,
) -> NormalizedScoreboardStatus {
    let (segment_kind, segment_number, clock_main, home_score, away_score, extras) =
        match sport_type {
            "volleyball" => (
                "set",
                3,
                "00:00",
                21,
                18,
                serde_json::json!({"sets_home": 2, "sets_away": 1, "serving": "home"}),
            ),
            "football" => (
                "quarter",
                4,
                "02:14",
                28,
                24,
                serde_json::json!({"down": 3, "to_go": 7, "ball_on": 42}),
            ),
            "soccer" => (
                "half",
                2,
                "67:33",
                2,
                1,
                serde_json::json!({"shots_home": 11, "shots_away": 8, "fouls_home": 5, "fouls_away": 7}),
            ),
            "lacrosse" => (
                "quarter",
                3,
                "03:51",
                10,
                9,
                serde_json::json!({"penalties_home": 1, "penalties_away": 2}),
            ),
            _ => (
                "period",
                2,
                "08:45",
                56,
                49,
                serde_json::json!({"fouls_home": 3, "fouls_away": 4, "bonus_home": true, "bonus_away": false}),
            ),
        };

    NormalizedScoreboardStatus {
        schema_version: 1,
        timestamp_rfc3339: chrono::Utc::now().to_rfc3339(),
        controller_type: controller_type.to_string(),
        sport_type: sport_type.to_string(),
        clock_main: Some(clock_main.to_string()),
        clock_secondary: Some("24".to_string()),
        segment_kind: Some(segment_kind.to_string()),
        segment_number: Some(segment_number),
        segment_text: None,
        home_score: Some(home_score),
        away_score: Some(away_score),
        home_timeouts: Some(2),
        away_timeouts: Some(1),
        possession: Some("home".to_string()),
        extras: serde_json::json!({
            "mode": "simulated",
            "rtd_profile": rtd_profile_for_sport_name(sport_type),
            "sport_specific": extras,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{render_admin_page, simulated_status_for_sport};
    use crate::config::AppConfig;

    #[test]
    fn admin_form_uses_valid_html_attributes() {
        let cfg = AppConfig::default();
        let html = render_admin_page(&cfg);

        assert!(html.contains("<form method=\"post\" action=\"/admin\">"));
        assert!(!html.contains("\\\""));
        assert!(html.contains("action=\"/admin/simulate\""));
    }

    #[test]
    fn simulation_payload_includes_selected_sport_profile() {
        let payload = simulated_status_for_sport("all_sport_5000", "football");

        assert_eq!(payload.sport_type, "football");
        assert_eq!(payload.segment_kind.as_deref(), Some("quarter"));
        assert_eq!(payload.extras["mode"], "simulated");
        assert_eq!(payload.extras["rtd_profile"], "rtd_football");
    }
}

fn render_select(name: &str, selected: &str, options: &[&str]) -> String {
    let mut html = format!("<select name=\"{name}\">");

    for option in options {
        if *option == selected {
            html.push_str(&format!(
                "<option value=\"{option}\" selected>{option}</option>"
            ));
        } else {
            html.push_str(&format!("<option value=\"{option}\">{option}</option>"));
        }
    }

    if !options.contains(&selected) {
        html.push_str(&format!(
            "<option value=\"{selected}\" selected>{selected}</option>"
        ));
    }

    html.push_str("</select>");
    html
}
