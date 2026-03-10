use crate::config::{save_config, AppConfig, SharedConfig, CONFIG_PATH};
use crate::decoder::{rtd_profile_for_sport_name, SharedSerialDebugBuffer};
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
use tower_http::services::ServeDir;

const SUPPORTED_SPORTS: [&str; 5] = ["basketball", "volleyball", "football", "soccer", "lacrosse"];

#[derive(Clone)]
pub struct WebState {
    pub config: SharedConfig,
    pub config_tx: watch::Sender<AppConfig>,
    pub status_tx: watch::Sender<NormalizedScoreboardStatus>,
    pub status_rx: watch::Receiver<NormalizedScoreboardStatus>,
    pub mqtt: MqttPublisher,
    pub serial_debug_samples: SharedSerialDebugBuffer,
}

pub fn router(state: WebState) -> Router {
    Router::new()
        .route("/", get(get_index))
        .route("/status.json", get(get_status_json))
        .route("/%22/status.json/%22", get(get_status_json))
        .route("/\"/status.json/\"", get(get_status_json))
        .route("/admin", get(get_admin).post(post_admin))
        .route("/debug/serial.json", get(get_serial_debug_json))
        .route("/admin/simulate", axum::routing::post(post_admin_simulate))
        .route("/%22/admin/%22", get(get_admin).post(post_admin))
        .route("/\"/admin/\"", get(get_admin).post(post_admin))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
}

async fn get_index() -> Html<&'static str> {
    Html(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1"/>
    <title>Daktronics Gateway</title>
    <link rel="stylesheet" href="/static/styles.css"/>
  </head>
  <body>
    <div class="app-shell">
      <header class="app-header">
        <div>
          <h1 class="brand-title">Daktronics Gateway Dashboard</h1>
          <p class="brand-subtitle">Live scoreboard relay and control surface</p>
        </div>
        <nav class="action-links" aria-label="Primary actions">
          <a class="action-link" href="/status.json">View Status JSON</a>
          <a class="action-link" href="/admin">Open Admin Panel</a>
        </nav>
      </header>

      <main>
        <section class="scoreboard-card" aria-label="Primary scoreboard">
          <div class="teams-grid">
            <article class="team-panel">
              <span class="team-label">Home</span>
              <span class="team-name" id="home-team-name">Home Team</span>
              <span class="score-value" id="home-score">--</span>
            </article>

            <aside class="game-meta" aria-label="Game clock and period">
              <span class="meta-label">Period</span>
              <span class="meta-value" id="segment-display">--</span>
              <span class="meta-label">Clock</span>
              <span class="meta-value" id="clock-main">--:--</span>
            </aside>

            <article class="team-panel">
              <span class="team-label">Away</span>
              <span class="team-name" id="away-team-name">Away Team</span>
              <span class="score-value" id="away-score">--</span>
            </article>
          </div>

          <div class="info-grid" aria-label="Secondary game metadata">
            <div class="info-chip"><span class="label" id="info-label-1">Home Timeouts</span><span class="value" id="info-value-1">--</span></div>
            <div class="info-chip"><span class="label" id="info-label-2">Away Timeouts</span><span class="value" id="info-value-2">--</span></div>
            <div class="info-chip"><span class="label" id="info-label-3">Detail 1</span><span class="value" id="info-value-3">--</span></div>
            <div class="info-chip"><span class="label" id="info-label-4">Detail 2</span><span class="value" id="info-value-4">--</span></div>
            <div class="info-chip"><span class="label" id="info-label-5">Detail 3</span><span class="value" id="info-value-5">--</span></div>
            <div class="info-chip"><span class="label" id="info-label-6">Detail 6</span><span class="value" id="info-value-6">--</span></div>
          </div>
        </section>
      </main>

      <footer class="app-footer">
        <p class="footer-note">Gateway is running and ready for sport-specific overlays. <span id="status-indicator" data-field="status-indicator">Connecting…</span></p>
        <div class="action-links" aria-label="Footer actions">
          <a class="action-link" href="/status.json">Status Feed</a>
          <a class="action-link" href="/admin">Settings</a>
        </div>
      </footer>
    </div>
    <script>
      (() => {
        const DASHBOARD_STATUS_URL = '/status.json';
        const POLL_MS = 500;

        const byId = (id) => document.getElementById(id);
        const textOrFallback = (value, fallback = '--') => {
          if (value === null || value === undefined || value === '') {
            return fallback;
          }
          return String(value);
        };

        const toTitleCase = (value) =>
          String(value)
            .replace(/_/g, ' ')
            .replace(/\b\w/g, (match) => match.toUpperCase());

        const segmentLabel = (kind, number) => {
          if (!kind && (number === null || number === undefined)) {
            return '--';
          }

          if (kind && number !== null && number !== undefined) {
            if (String(kind).toLowerCase() === 'quarter') {
              return `Q${number}`;
            }
            return `${toTitleCase(kind)} ${number}`;
          }

          return toTitleCase(kind || number);
        };

        const setText = (id, value) => {
          const node = byId(id);
          if (node) {
            node.textContent = value;
          }
        };

        const hasOwn = (obj, key) =>
          Object.prototype.hasOwnProperty.call(obj, key);

        const sportSpecificEntries = (status, sportSpecific) => {
          const sport = textOrFallback(status?.sport_type, '').toLowerCase();

          const preferredKeys = {
            football: ['down', 'to_go', 'ball_on'],
            basketball: ['fouls_home', 'fouls_away', 'bonus_home', 'bonus_away'],
            volleyball: ['sets_home', 'sets_away', 'serving'],
            soccer: ['shots_home', 'shots_away', 'fouls_home', 'fouls_away'],
            lacrosse: ['penalties_home', 'penalties_away'],
          };

          const selectedKeys = preferredKeys[sport] ?? [];
          const selectedEntries = selectedKeys
            .filter((key) => hasOwn(sportSpecific, key))
            .map((key) => [key, sportSpecific[key]]);

          if (selectedEntries.length > 0) {
            return selectedEntries;
          }

          return Object.entries(sportSpecific);
        };

        const buildInfoChips = (status) => {
          const extras = status?.extras ?? {};
          const sportSpecific = extras?.sport_specific ?? {};
          const details = sportSpecificEntries(status, sportSpecific);

          const chips = [
            ['home_timeouts', status?.home_timeouts],
            ['away_timeouts', status?.away_timeouts],
            ...details,
          ];

          return chips.slice(0, 6);
        };

        const updateFromStatus = (status) => {
          const infoChips = buildInfoChips(status);

          setText('home-team-name', textOrFallback(status?.extras?.sport_specific?.home_team_name, 'Home Team'));
          setText('away-team-name', textOrFallback(status?.extras?.sport_specific?.away_team_name, 'Away Team'));
          setText('home-score', textOrFallback(status?.home_score));
          setText('away-score', textOrFallback(status?.away_score));
          setText('clock-main', textOrFallback(status?.clock_main, '--:--'));
          setText('segment-display', segmentLabel(status?.segment_kind, status?.segment_number));

          for (let i = 0; i < 6; i += 1) {
            const chip = infoChips[i];
            const index = i + 1;
            if (chip) {
              setText(`info-label-${index}`, toTitleCase(chip[0]));
              setText(`info-value-${index}`, textOrFallback(chip[1]));
            } else {
              setText(`info-label-${index}`, `Detail ${index}`);
              setText(`info-value-${index}`, '--');
            }
          }
        };

        const pollStatus = async () => {
          try {
            const response = await fetch(DASHBOARD_STATUS_URL, { cache: 'no-store' });
            if (!response.ok) {
              return;
            }
            const data = await response.json();
            updateFromStatus(data);
          } catch {
            // Keep last visible values on intermittent failures.
          }
        };

        pollStatus();
        setInterval(pollStatus, POLL_MS);
      })();
    </script>
  </body>
</html>"#,
    )
}

async fn get_status_json(State(state): State<WebState>) -> Json<NormalizedScoreboardStatus> {
    Json(state.status_rx.borrow().clone())
}

async fn get_serial_debug_json(State(state): State<WebState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers).await {
        return unauthorized();
    }

    let samples = {
        let guard = state.serial_debug_samples.lock().await;
        guard.iter().cloned().collect::<Vec<_>>()
    };

    Json(samples).into_response()
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
    #[serde(default)]
    serial_debug_raw: Option<String>,
    #[serde(default)]
    serial_debug_publish: Option<String>,
    #[serde(default)]
    serial_debug_topic: String,
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
    cfg.serial_debug_raw = form.serial_debug_raw.is_some();
    cfg.serial_debug_publish = form.serial_debug_publish.is_some();
    cfg.serial_debug_topic = if form.serial_debug_topic.trim().is_empty() {
        None
    } else {
        Some(form.serial_debug_topic)
    };

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
            format!(r#"<button class="btn btn-secondary" type="submit" name="sport_type" value="{sport}">{sport}</button>"#)
        })
        .collect::<Vec<_>>()
        .join(" ");

    let debug_output_script = r#"<script>
      (() => {
        const output = document.getElementById('debug-output');
        const POLL_MS = 1000;

        const renderSample = (sample) => {
          const ts = sample.timestamp_rfc3339 || '--';
          const sport = sample.sport || '--';
          const frame = sample.frame_index === undefined || sample.frame_index === null
            ? '-'
            : sample.frame_index;
          const len = sample.byte_len || 0;
          const hex = sample.hex_preview || '';
          return `[${ts}] sport=${sport} frame=${frame} bytes=${len}\nhex: ${hex}`;
        };

        const pollDebug = async () => {
          try {
            const response = await fetch('/debug/serial.json', { cache: 'no-store' });
            if (!response.ok) {
              output.textContent = `Unable to load debug output (${response.status}).`;
              return;
            }

            const samples = await response.json();
            if (!Array.isArray(samples) || samples.length === 0) {
              output.textContent = 'No debug samples yet. Enable "Publish debug samples" and wait for serial traffic.';
              return;
            }

            const recent = samples.slice(-20).reverse().map(renderSample).join('\n\n');
            output.textContent = recent;
          } catch {
            output.textContent = 'Unable to load debug output.';
          }
        };

        pollDebug();
        setInterval(pollDebug, POLL_MS);
      })();
    </script>"#;

    format!(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1"/>
    <title>Scoreboard Admin</title>
    <link rel="stylesheet" href="/static/styles.css"/>
  </head>
  <body>
    <div class="app-shell">
      <header class="app-header">
        <div>
          <h1 class="brand-title">Daktronics Gateway Admin</h1>
          <p class="brand-subtitle">Configuration and simulation controls</p>
        </div>
        <nav class="action-links" aria-label="Primary actions">
          <a class="action-link" href="/">Open Dashboard</a>
          <a class="action-link" href="/status.json">View Status JSON</a>
        </nav>
      </header>

      <main class="admin-main">
        <form method="post" action="/admin" class="settings-grid">
          <section class="section-card" aria-label="Connection settings">
            <h2 class="section-title">Connection settings</h2>
            <p class="section-description">Serial feed and MQTT broker details.</p>
            <div class="form-grid">
              <div class="form-group">
                <label for="serial_device">Serial Device</label>
                <input id="serial_device" name="serial_device" value="{}"/>
              </div>
              <div class="form-group">
                <label for="mqtt_host">MQTT Host</label>
                <input id="mqtt_host" name="mqtt_host" value="{}"/>
              </div>
              <div class="form-group">
                <label for="mqtt_port">MQTT Port</label>
                <input id="mqtt_port" name="mqtt_port" type="number" value="{}"/>
              </div>
            </div>
          </section>

          <section class="section-card" aria-label="Sport and controller settings">
            <h2 class="section-title">Sport/controller settings</h2>
            <p class="section-description">Select decoder profile and controller family.</p>
            <div class="form-grid">
              <div class="form-group">
                <label for="controller_type">Controller Type</label>
                {}
              </div>
              <div class="form-group">
                <label for="sport_type">Sport Type</label>
                {}
              </div>
            </div>
          </section>

          <section class="section-card" aria-label="Publish settings">
            <h2 class="section-title">Publish settings</h2>
            <p class="section-description">Topic and cadence used for outbound updates.</p>
            <div class="form-grid">
              <div class="form-group">
                <label for="mqtt_topic">MQTT Topic</label>
                <input id="mqtt_topic" name="mqtt_topic" value="{}"/>
              </div>
              <div class="form-group">
                <label for="publish_interval_ms">Publish Interval (ms)</label>
                <input id="publish_interval_ms" name="publish_interval_ms" type="number" value="{}"/>
              </div>
            </div>
          </section>

          <section class="section-card" aria-label="Serial debug settings">
            <h2 class="section-title">Serial debug settings</h2>
            <p class="section-description">Enable short-term serial frame diagnostics and publish a live sample feed.</p>
            <div class="form-grid">
              <label class="checkbox-row" for="serial_debug_raw">
                <input id="serial_debug_raw" name="serial_debug_raw" type="checkbox" {} />
                Emit raw frame previews to gateway logs
              </label>
              <label class="checkbox-row" for="serial_debug_publish">
                <input id="serial_debug_publish" name="serial_debug_publish" type="checkbox" {} />
                Publish debug samples and expose them in the admin output panel
              </label>
              <div class="form-group">
                <label for="serial_debug_topic">Serial Debug MQTT Topic (optional)</label>
                <input id="serial_debug_topic" name="serial_debug_topic" value="{}" placeholder="Derived from MQTT topic when empty"/>
              </div>
            </div>
          </section>

          <section class="section-card" aria-label="Save settings">
            <h2 class="section-title">Save configuration</h2>
            <p class="section-description">Apply changes to the active runtime config.</p>
            <div class="button-row">
              <button class="btn btn-primary" type="submit">Save</button>
            </div>
          </section>
        </form>

        <section class="section-card" aria-label="Simulation controls">
          <h2 class="section-title">Simulation controls</h2>
          <p class="section-description">Publish sample sport payloads for testing without serial input.</p>
          <form method="post" action="/admin/simulate">
            <div class="button-row">
              {}
            </div>
          </form>
        </section>

        <section class="section-card" aria-label="Serial debug output">
          <h2 class="section-title">Serial debug output</h2>
          <p class="section-description">Most recent debug samples from <code>/debug/serial.json</code> (requires debug publishing to be enabled).</p>
          <pre class="debug-output" id="debug-output">Waiting for debug samples…</pre>
        </section>
      </main>

      <footer class="app-footer">
        <p class="footer-note">Admin actions preserve existing backend routes and field names.</p>
      </footer>
    </div>
    {}
  </body>
</html>"#,
        cfg.serial_device,
        cfg.mqtt_host,
        cfg.mqtt_port,
        controller_type_select,
        sport_type_select,
        cfg.mqtt_topic,
        cfg.publish_interval_ms,
        if cfg.serial_debug_raw { "checked" } else { "" },
        if cfg.serial_debug_publish {
            "checked"
        } else {
            ""
        },
        cfg.serial_debug_topic.clone().unwrap_or_default(),
        simulation_buttons,
        debug_output_script
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

        assert!(html.contains("<form method=\"post\" action=\"/admin\" class=\"settings-grid\">"));
        assert!(!html.contains("\\\""));
        assert!(html.contains("action=\"/admin/simulate\""));
    }

    #[tokio::test]
    async fn dashboard_contains_live_status_bindings() {
        let page = super::get_index().await.0;

        assert!(page.contains("id=\"home-score\""));
        assert!(page.contains("id=\"away-score\""));
        assert!(page.contains("id=\"clock-main\""));
        assert!(page.contains("setInterval(pollStatus, POLL_MS)"));
        assert!(page.contains("fetch(DASHBOARD_STATUS_URL"));
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
    let mut html = format!("<select id=\"{name}\" name=\"{name}\">");

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
