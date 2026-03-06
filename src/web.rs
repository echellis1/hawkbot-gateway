use crate::config::{save_config, AppConfig, SharedConfig, CONFIG_PATH};
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

#[derive(Clone)]
pub struct WebState {
    pub config: SharedConfig,
    pub config_tx: watch::Sender<AppConfig>,
    pub status_rx: watch::Receiver<NormalizedScoreboardStatus>,
    pub mqtt: MqttPublisher,
}

pub fn router(state: WebState) -> Router {
    Router::new()
        .route("/", get(get_index))
        .route("/status.json", get(get_status_json))
        .route("/admin", get(get_admin).post(post_admin))
        .with_state(state)
}

async fn get_index() -> Html<&'static str> {
    Html(
        r#"<!doctype html>
<html>
  <head><meta charset=\"utf-8\"/><title>Daktronics Gateway</title></head>
  <body>
    <h1>Daktronics Gateway</h1>
    <p>Gateway is running.</p>
    <ul>
      <li><a href=\"/status.json\">Live status JSON</a></li>
      <li><a href=\"/admin\">Admin settings</a></li>
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
    state.mqtt.publish_config(&cfg).await;

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
    format!(
        r#"<!doctype html>
<html>
  <head><meta charset=\"utf-8\"/><title>Scoreboard Admin</title></head>
  <body>
    <h1>Daktronics Gateway Admin</h1>
    <form method=\"post\" action=\"/admin\">
      <label>Controller Type: <input name=\"controller_type\" value=\"{}\"/></label><br/>
      <label>Sport Type: <input name=\"sport_type\" value=\"{}\"/></label><br/>
      <label>Serial Device: <input name=\"serial_device\" value=\"{}\"/></label><br/>
      <label>MQTT Host: <input name=\"mqtt_host\" value=\"{}\"/></label><br/>
      <label>MQTT Port: <input name=\"mqtt_port\" type=\"number\" value=\"{}\"/></label><br/>
      <label>MQTT Topic: <input name=\"mqtt_topic\" value=\"{}\"/></label><br/>
      <label>Publish Interval (ms): <input name=\"publish_interval_ms\" type=\"number\" value=\"{}\"/></label><br/>
      <button type=\"submit\">Save</button>
    </form>
  </body>
</html>"#,
        cfg.controller_type,
        cfg.sport_type,
        cfg.serial_device,
        cfg.mqtt_host,
        cfg.mqtt_port,
        cfg.mqtt_topic,
        cfg.publish_interval_ms
    )
}
